//! Jellyfish Drift: the game itself, in Rust, compiled to WebAssembly for the
//! page (build-web.sh) and natively for the tests.
//!
//! No wasm-bindgen: the page talks to five `extern "C"` functions, and reads
//! the whole world each frame as one array of f64 in the module's memory
//! (`frame`). One data structure crosses the boundary, so adding a field is
//! one line here and one in web/main.js, never a new export.

pub const WIDTH: f64 = 960.0;
pub const HEIGHT: f64 = 560.0;
pub const PLAYER_X: f64 = 230.0;
pub const PLAYER_RADIUS: f64 = 19.0;
/// Floats ahead of the gates in a frame; web/main.js reads the same layout.
pub const HEAD: usize = 11;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Ready = 0,
    Playing = 1,
    Over = 2,
}

/// A coral gate: two columns with a gap to swim through.
struct Gate {
    x: f64,
    center: f64,
    gap: f64,
    passed: bool,
}

struct Pearl {
    x: f64,
    y: f64,
}

pub struct Game {
    pub y: f64,
    pub velocity: f64,
    pub time: f64,
    pub played: f64,
    pub score: u32,
    pub best: u32,
    pub lives: u32,
    pub pearls_taken: u32,
    pub state: State,
    gates: Vec<Gate>,
    pearls: Vec<Pearl>,
    spawn_timer: f64,
    speed: f64,
    seed: u32,
    frame: Vec<f64>,
}

impl Game {
    pub fn new(seed: u32) -> Game {
        Game {
            y: HEIGHT / 2.0,
            velocity: 0.0,
            time: 0.0,
            played: 0.0,
            score: 0,
            best: 0,
            lives: 3,
            pearls_taken: 0,
            state: State::Ready,
            gates: Vec::new(),
            pearls: Vec::new(),
            spawn_timer: 1.7,
            speed: 175.0,
            // xorshift never leaves zero, so zero is not a seed.
            seed: seed.max(1),
            frame: Vec::new(),
        }
    }

    /// A new run, keeping the best score and moving the seed on so the next
    /// reef is a different one.
    pub fn reset(&mut self) {
        let (best, seed, time) = (self.best, self.seed.wrapping_add(0x9e37_79b9), self.time);
        *self = Game::new(seed);
        self.best = best;
        self.time = time;
        self.state = State::Playing;
    }

    /// Space, tap or click: start, restart, or swim up.
    pub fn pulse(&mut self) {
        match self.state {
            State::Ready => self.state = State::Playing,
            State::Over => self.reset(),
            State::Playing => self.velocity = (self.velocity - 215.0).max(-325.0),
        }
    }

    pub fn update(&mut self, raw_dt: f64) {
        let dt = raw_dt.clamp(0.0, 0.034);
        self.time += dt;
        if self.state != State::Playing {
            return;
        }
        self.played += dt;

        // Sinking: gravity with a slow swell in it, and water drag.
        self.velocity += (475.0 + (self.time * 1.4).sin() * 28.0) * dt;
        self.velocity *= 1.0 - 0.22 * dt;
        self.y += self.velocity * dt;
        if self.y < PLAYER_RADIUS + 18.0 {
            self.y = PLAYER_RADIUS + 18.0;
            self.velocity = 70.0;
        }
        if self.y > HEIGHT - PLAYER_RADIUS - 18.0 {
            self.hit();
            self.y = HEIGHT - PLAYER_RADIUS - 20.0;
            self.velocity = -220.0;
        }

        // Faster and tighter as the score climbs, up to a limit.
        self.speed = 175.0 + (self.score as f64 * 1.5).min(135.0);
        self.spawn_timer -= dt;
        if self.spawn_timer <= 0.0 {
            let center = 160.0 + self.rand() * 245.0;
            let gap = (235.0 - self.score as f64 * 1.1).max(165.0);
            self.gates.push(Gate { x: WIDTH + 70.0, center, gap, passed: false });
            self.pearls.push(Pearl { x: WIDTH + 70.0, y: center });
            self.spawn_timer = 1.9;
        }

        let step = self.speed * dt;
        let mut collide = false;
        for gate in &mut self.gates {
            gate.x -= step;
            let near = (PLAYER_X - gate.x).abs() < 48.0;
            let outside = self.y - PLAYER_RADIUS < gate.center - gate.gap / 2.0 || self.y + PLAYER_RADIUS > gate.center + gate.gap / 2.0;
            // A gate scores once or hurts once, never both.
            if !gate.passed && near && outside {
                gate.passed = true;
                collide = true;
            } else if !gate.passed && gate.x + 33.0 < PLAYER_X {
                gate.passed = true;
                self.score += 5;
            }
        }
        if collide {
            self.hit();
            self.velocity = -140.0;
        }
        let y = self.y;
        let mut taken = 0;
        for p in &mut self.pearls {
            p.x -= step;
        }
        // A pearl leaves the list taken (scores) or swum past (does not).
        self.pearls.retain(|p| {
            let touched = (PLAYER_X - p.x).hypot(y - p.y) < 32.0;
            taken += touched as u32;
            !touched && p.x > -80.0
        });
        self.pearls_taken += taken;
        self.score += 20 * taken;
        self.gates.retain(|g| g.x > -80.0);
        self.best = self.best.max(self.score);
    }

    fn hit(&mut self) {
        self.lives = self.lives.saturating_sub(1);
        if self.lives == 0 {
            self.state = State::Over;
        }
    }

    /// xorshift32: the same reef for the same seed, on every machine.
    fn rand(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f64 / u32::MAX as f64
    }

    /// The world as one array: HEAD floats, then x, center, gap for each
    /// gate, then x, y for each pearl.
    pub fn frame(&mut self) -> &[f64] {
        self.frame.clear();
        self.frame.extend_from_slice(&[
            self.y,
            self.velocity,
            self.time,
            self.score as f64,
            self.best as f64,
            self.lives as f64,
            self.state as u8 as f64,
            self.gates.len() as f64,
            self.pearls.len() as f64,
            self.pearls_taken as f64,
            self.played,
        ]);
        for g in &self.gates {
            self.frame.extend_from_slice(&[g.x, g.center, g.gap]);
        }
        for p in &self.pearls {
            self.frame.extend_from_slice(&[p.x, p.y]);
        }
        &self.frame
    }
}

// ---- the WebAssembly boundary ----------------------------------------------
// The page owns one Game through the pointer game_new returns. Single-threaded
// by construction (one page, one module instance).

#[no_mangle]
pub extern "C" fn game_new(seed: u32) -> *mut Game {
    Box::into_raw(Box::new(Game::new(seed)))
}

/// # Safety
/// `g` is a pointer game_new returned.
#[no_mangle]
pub unsafe extern "C" fn game_pulse(g: *mut Game) {
    (*g).pulse();
}

/// # Safety
/// `g` is a pointer game_new returned.
#[no_mangle]
pub unsafe extern "C" fn game_update(g: *mut Game, dt: f64) {
    (*g).update(dt);
}

/// The frame's address in the module's memory. Valid until the next call
/// into the module: read it, then call again.
///
/// # Safety
/// `g` is a pointer game_new returned.
#[no_mangle]
pub unsafe extern "C" fn game_frame(g: *mut Game) -> *const f64 {
    (*g).frame().as_ptr()
}

/// # Safety
/// `g` is a pointer game_new returned, and game_frame was called first.
#[no_mangle]
pub unsafe extern "C" fn game_frame_len(g: *const Game) -> usize {
    (*g).frame.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(seed: u32) -> Game {
        let mut g = Game::new(seed);
        g.pulse();
        g
    }

    #[test]
    fn the_first_pulse_starts_and_later_ones_swim_up() {
        let mut g = Game::new(7);
        assert_eq!(g.state, State::Ready);
        g.pulse();
        assert_eq!(g.state, State::Playing);
        g.pulse();
        assert!(g.velocity < 0.0);
    }

    #[test]
    fn sinking_to_the_floor_costs_all_three_hearts() {
        let mut g = playing(7);
        for _ in 0..60 * 60 {
            g.update(1.0 / 60.0);
            if g.state == State::Over {
                break;
            }
        }
        assert_eq!(g.state, State::Over);
        assert_eq!(g.lives, 0);
    }

    #[test]
    fn the_same_seed_builds_the_same_reef() {
        let (mut a, mut b) = (playing(42), playing(42));
        for _ in 0..600 {
            a.update(1.0 / 60.0);
            b.update(1.0 / 60.0);
        }
        assert_eq!(a.frame(), b.frame());
    }

    #[test]
    fn a_pearl_on_the_path_scores_twenty() {
        let mut g = playing(7);
        g.pearls.push(Pearl { x: PLAYER_X + 1.0, y: g.y });
        g.update(0.001);
        assert_eq!(g.pearls_taken, 1);
        assert_eq!(g.score, 20);
    }

    #[test]
    fn a_pearl_swum_past_scores_nothing() {
        let mut g = playing(7);
        g.pearls.push(Pearl { x: -79.9, y: 0.0 });
        g.update(0.01);
        assert_eq!(g.pearls_taken, 0);
        assert_eq!(g.score, 0);
    }

    #[test]
    fn the_frame_is_head_then_gates_then_pearls() {
        let mut g = playing(7);
        for _ in 0..200 {
            g.update(1.0 / 60.0);
        }
        let f = g.frame().to_vec();
        let (gates, pearls) = (f[7] as usize, f[8] as usize);
        assert!(gates > 0);
        assert_eq!(f.len(), HEAD + 3 * gates + 2 * pearls);
    }

    #[test]
    fn restart_keeps_the_best_and_changes_the_reef() {
        let mut g = playing(7);
        g.score = 50;
        g.best = 50;
        g.state = State::Over;
        let seed = g.seed;
        g.pulse();
        assert_eq!((g.state, g.score, g.best, g.lives), (State::Playing, 0, 50, 3));
        assert_ne!(g.seed, seed);
    }
}
