//! Last Signal: the game itself, in Rust, compiled to WebAssembly for the page
//! (build-web.sh) and natively for the tests.
//!
//! The machines woke up. A handful of survivors hold a Haven on the west edge
//! of a ruined city; to the east, AI Cores grow the Grid - a glowing lattice
//! that makes every machine on it faster and self-repairing. By day you
//! salvage scrap and take in survivors; at nightfall the Cores send a wave.
//! Hold out, push the Grid back with Jammers, turn their machines with
//! Hackers, and burn the Cores out.
//!
//! No wasm-bindgen: the page calls a handful of `extern "C"` functions and
//! reads the whole world each frame as one array of f64 in the module's
//! memory (`frame`), plus the map as one byte per tile (`tiles`). Adding a
//! field is one line here and one in web/main.js, never a new export.

pub const MW: usize = 48;
pub const MH: usize = 30;
/// Seconds of daylight, then of night.
pub const DAY: f64 = 95.0;
pub const NIGHT: f64 = 50.0;
const TICK: f64 = 1.0 / 30.0;
/// Floats ahead of the lists in a frame; web/main.js reads the same layout.
pub const HEAD: usize = 26;
pub const CACHE_F: usize = 3;
pub const UNIT_F: usize = 11;
pub const BLD_F: usize = 10;
pub const WRECK_F: usize = 3;
pub const FX_F: usize = 7;

// ---- the map ---------------------------------------------------------------

/// What a tile is. The Grid is a layer over ground and road, not a tile kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tile {
    Ground = 0,
    Road = 1,
    Rubble = 2,
    Ruin = 3,
    Water = 4,
}

impl Tile {
    fn open(self) -> bool {
        matches!(self, Tile::Ground | Tile::Road)
    }
}

// ---- sides, units, buildings -------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Human = 0,
    Machine = 1,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UKind {
    Scavenger = 0,
    Rifleman = 1,
    Emp = 2,
    Hacker = 3,
    Technical = 4,
    Crawler = 5,
    Sentry = 6,
    Titan = 7,
    Reclaimer = 8,
    /// Pets (2026-11-01, from a young player: "dogs, cats, and all sorts of
    /// pets ... that do different things"). Not survivors: they cost scrap
    /// only, take no housing, and at most PETS keep you company.
    Dog = 9,
    Cat = 10,
}

/// How many pets at once, counting those on their way.
pub const PETS: usize = 6;

/// What a unit is: hit points, speed (tiles/s), and its one weapon - range
/// (tiles), damage, seconds between shots, splash radius, stun seconds.
pub struct USpec {
    pub hp: f64,
    pub speed: f64,
    pub range: f64,
    pub damage: f64,
    pub reload: f64,
    pub splash: f64,
    pub stun: f64,
    pub cost: u32,
    /// The building that trains it; None: the machines make it.
    pub from: Option<BKind>,
    /// Scrap its wreck holds when it falls.
    pub salvage: u32,
}

impl UKind {
    pub const HUMAN: [UKind; 7] = [UKind::Scavenger, UKind::Rifleman, UKind::Emp, UKind::Hacker, UKind::Technical, UKind::Dog, UKind::Cat];

    pub fn pet(self) -> bool {
        matches!(self, UKind::Dog | UKind::Cat)
    }

    pub fn spec(self) -> USpec {
        let u = |hp, speed, range, damage, reload, splash, stun, cost, from, salvage| USpec { hp, speed, range, damage, reload, splash, stun, cost, from, salvage };
        match self {
            UKind::Scavenger => u(45.0, 2.4, 0.0, 0.0, 1.0, 0.0, 0.0, 50, Some(BKind::Haven), 10),
            UKind::Rifleman => u(60.0, 2.0, 4.0, 9.0, 0.8, 0.0, 0.0, 60, Some(BKind::Armory), 10),
            UKind::Emp => u(55.0, 1.9, 3.5, 7.0, 3.0, 1.3, 2.5, 90, Some(BKind::Armory), 15),
            UKind::Hacker => u(45.0, 2.1, 2.6, 0.0, 1.0, 0.0, 0.0, 140, Some(BKind::Armory), 15),
            UKind::Technical => u(170.0, 3.6, 4.5, 5.0, 0.25, 0.0, 0.0, 170, Some(BKind::Garage), 40),
            UKind::Crawler => u(38.0, 2.7, 0.9, 5.0, 0.6, 0.0, 0.0, 0, None, 20),
            UKind::Sentry => u(85.0, 1.6, 5.0, 12.0, 1.2, 0.0, 0.0, 0, None, 40),
            UKind::Titan => u(650.0, 0.9, 4.0, 42.0, 2.2, 1.6, 0.0, 0, None, 140),
            UKind::Reclaimer => u(75.0, 2.1, 0.0, 0.0, 1.0, 0.0, 0.0, 0, None, 30),
            // The dog bites and fetches night caches; the cat sees in the dark
            // and purrs the hurt back to health.
            UKind::Dog => u(90.0, 3.4, 0.9, 8.0, 0.5, 0.0, 0.0, 70, Some(BKind::Haven), 0),
            UKind::Cat => u(60.0, 2.8, 0.0, 0.0, 1.0, 0.0, 0.0, 60, Some(BKind::Haven), 0),
        }
    }

    /// Seconds a Hacker needs on it before it changes sides.
    pub fn hack_time(self) -> f64 {
        match self {
            UKind::Titan => 7.0,
            UKind::Sentry => 4.0,
            _ => 3.0,
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn from_i(i: u32) -> Option<UKind> {
        UKind::HUMAN.get(i as usize).copied()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BKind {
    Haven = 0,
    Shelter = 1,
    Armory = 2,
    Garage = 3,
    Barricade = 4,
    Turret = 5,
    Floodlight = 6,
    Jammer = 7,
    Core = 8,
}

pub struct BSpec {
    pub w: usize,
    pub h: usize,
    pub hp: f64,
    pub cost: u32,
    /// Seconds to raise.
    pub build: f64,
    pub housing: u32,
    pub range: f64,
    pub damage: f64,
    pub reload: f64,
}

impl BKind {
    /// What a person may place, in the build bar's order.
    pub const PLACEABLE: [BKind; 7] = [BKind::Shelter, BKind::Armory, BKind::Garage, BKind::Barricade, BKind::Turret, BKind::Floodlight, BKind::Jammer];

    pub fn spec(self) -> BSpec {
        let b = |w, h, hp, cost, build, housing, range, damage, reload| BSpec { w, h, hp, cost, build, housing, range, damage, reload };
        match self {
            BKind::Haven => b(3, 3, 1100.0, 0, 0.0, 6, 4.5, 9.0, 0.9),
            BKind::Shelter => b(2, 2, 320.0, 90, 8.0, 4, 0.0, 0.0, 1.0),
            BKind::Armory => b(2, 2, 420.0, 150, 12.0, 0, 0.0, 0.0, 1.0),
            BKind::Garage => b(3, 2, 480.0, 220, 15.0, 0, 0.0, 0.0, 1.0),
            BKind::Barricade => b(1, 1, 260.0, 15, 3.0, 0, 0.0, 0.0, 1.0),
            BKind::Turret => b(1, 1, 300.0, 120, 9.0, 0, 5.5, 13.0, 0.7),
            BKind::Floodlight => b(1, 1, 150.0, 60, 5.0, 0, 0.0, 0.0, 1.0),
            BKind::Jammer => b(1, 1, 240.0, 140, 10.0, 0, 0.0, 0.0, 1.0),
            BKind::Core => b(3, 3, 2200.0, 0, 0.0, 0, 0.0, 0.0, 1.0),
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn from_i(i: u32) -> Option<BKind> {
        BKind::PLACEABLE.get(i as usize).copied()
    }
}

/// A Floodlight's reach, in which machines are Exposed (take more damage),
/// and a Jammer's, in which the Grid cannot grow and is slowly erased.
pub const LIGHT_R: f64 = 5.0;
pub const JAM_R: f64 = 4.5;
pub const EXPOSED: f64 = 1.35;
/// How far from your buildings a new one may stand.
pub const BUILD_REACH: f64 = 6.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Order {
    Idle,
    Move(f64, f64),
    /// Move, fighting what it meets on the way.
    Assault(f64, f64),
    Attack(u32),
    /// A building of the other side.
    Raze(u32),
    Hack(u32),
    Gather(u32),
    Deliver,
}

pub struct Unit {
    pub id: u32,
    pub kind: UKind,
    pub side: Side,
    pub x: f64,
    pub y: f64,
    pub px: f64,
    pub py: f64,
    pub hp: f64,
    pub cooldown: f64,
    pub stunned: f64,
    pub hacking: f64,
    pub carry: u32,
    pub order: Order,
    pub path: Vec<(usize, usize)>,
    pub selected: bool,
    pub facing: f64,
    /// Turned by a Hacker: a machine fighting for the survivors.
    pub turned: bool,
    repath: f64,
    /// The wreck a Scavenger goes back to after unloading.
    pile: u32,
    /// The tile `path` was found for.
    path_goal: (usize, usize),
    /// A machine's strength: its night's multiplier on speed and damage (1 for
    /// everything else). Night 1's machines are slow and weak; each night more.
    pub power: f64,
}

pub struct Building {
    pub id: u32,
    pub kind: BKind,
    pub side: Side,
    pub tx: usize,
    pub ty: usize,
    pub hp: f64,
    pub progress: f64,
    pub cooldown: f64,
    pub selected: bool,
}

impl Building {
    pub fn done(&self) -> bool {
        self.progress >= 1.0
    }
    fn center(&self) -> (f64, f64) {
        let s = self.kind.spec();
        (self.tx as f64 + s.w as f64 / 2.0, self.ty as f64 + s.h as f64 / 2.0)
    }
    fn covers(&self, x: usize, y: usize) -> bool {
        let s = self.kind.spec();
        x >= self.tx && x < self.tx + s.w && y >= self.ty && y < self.ty + s.h
    }
    /// Distance from a point to the nearest edge of the footprint.
    fn reach(&self, x: f64, y: f64) -> f64 {
        let s = self.kind.spec();
        let dx = (self.tx as f64 - x).max(x - (self.tx + s.w) as f64).max(0.0);
        let dy = (self.ty as f64 - y).max(y - (self.ty + s.h) as f64).max(0.0);
        (dx * dx + dy * dy).sqrt()
    }
}

/// Something good lying in the dark: a cache of power cells that shows only at
/// night and is gone at dawn. Any unit of yours that walks over one takes it;
/// a dog goes looking for them.
pub struct Cache {
    pub x: f64,
    pub y: f64,
    pub scrap: u32,
    pub id: u32,
}

pub struct Wreck {
    pub x: f64,
    pub y: f64,
    pub scrap: u32,
    pub id: u32,
}

/// Something drawn for a moment: 0 shot, 1 EMP pulse, 2 hack beam, 3 blast, 4 grid zap, 5 cache taken.
pub struct Fx {
    pub kind: u8,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub age: f64,
    pub life: f64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Playing = 0,
    Won = 1,
    Lost = 2,
}

/// A short note for the page's message line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    None = 0,
    Nightfall = 1,
    Dawn = 2,
    NoScrap = 3,
    NoSurvivors = 4,
    NoHousing = 5,
    NeedBuilding = 6,
    BadSpot = 7,
    Turned = 8,
    CoreDown = 9,
    HavenHit = 10,
    Survivor = 11,
    Cache = 12,
    PetReady = 13,
    TooManyPets = 14,
}

pub struct Game {
    pub tiles: Vec<Tile>,
    /// The Grid, one byte per tile: 0 none, else how long it has grown (1-255).
    pub grid: Vec<u8>,
    pub units: Vec<Unit>,
    pub buildings: Vec<Building>,
    pub wrecks: Vec<Wreck>,
    pub caches: Vec<Cache>,
    pub found: u32,
    pub fx: Vec<Fx>,
    pub time: f64,
    pub scrap: u32,
    /// Survivors waiting at the Haven, ready to be trained.
    pub survivors: u32,
    pub nights: u32,
    pub kills: u32,
    pub turned: u32,
    pub cores_down: u32,
    pub state: State,
    pub note: Note,
    pub paused: bool,
    acc: f64,
    rng: u32,
    next_id: u32,
    night_was: bool,
    wave_left: u32,
    wave_clock: f64,
    patrol_clock: f64,
    grid_clock: f64,
    arrive_clock: f64,
    queue: Vec<(UKind, f64)>,
    frame: Vec<f64>,
    tile_bytes: Vec<u8>,
}

// ---- small helpers ------------------------------------------------------------

fn dist(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
}

fn idx(x: usize, y: usize) -> usize {
    y * MW + x
}

impl Game {
    fn rand(&mut self) -> u32 {
        // xorshift32: a seed is a city.
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    fn randf(&mut self) -> f64 {
        (self.rand() % 10_000) as f64 / 10_000.0
    }

    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    pub fn tile(&self, x: usize, y: usize) -> Tile {
        self.tiles[idx(x, y)]
    }

    pub fn is_night(&self) -> bool {
        self.time % (DAY + NIGHT) >= DAY
    }

    /// Seconds until the light changes.
    pub fn until_change(&self) -> f64 {
        let t = self.time % (DAY + NIGHT);
        if t < DAY { DAY - t } else { DAY + NIGHT - t }
    }

    pub fn housing(&self) -> u32 {
        self.buildings.iter().filter(|b| b.side == Side::Human && b.done()).map(|b| b.kind.spec().housing).sum()
    }

    /// Survivors in the field: every human unit not turned from the machines.
    pub fn people(&self) -> u32 {
        self.units.iter().filter(|u| u.side == Side::Human && !u.turned && !u.kind.pet()).count() as u32 + self.queue.iter().filter(|q| !q.0.pet()).count() as u32
    }

    pub fn score(&self) -> u32 {
        self.nights * 100 + self.kills * 5 + self.turned * 25 + self.found * 10 + self.cores_down * 500 + if self.state == State::Won { 1000 } else { 0 }
    }

    // ---- the city ----------------------------------------------------------------

    pub fn new(seed: u32) -> Game {
        let mut g = Game {
            tiles: vec![Tile::Ground; MW * MH],
            grid: vec![0; MW * MH],
            units: vec![],
            buildings: vec![],
            wrecks: vec![],
            caches: vec![],
            found: 0,
            fx: vec![],
            time: 0.0,
            scrap: 350,
            survivors: 1,
            nights: 0,
            kills: 0,
            turned: 0,
            cores_down: 0,
            state: State::Playing,
            note: Note::None,
            paused: false,
            acc: 0.0,
            rng: seed.max(1),
            next_id: 0,
            night_was: false,
            wave_left: 0,
            wave_clock: 0.0,
            patrol_clock: 70.0,
            grid_clock: 0.0,
            arrive_clock: 0.0,
            queue: vec![],
            frame: vec![],
            tile_bytes: vec![0; MW * MH],
        };
        for _ in 0..8 {
            g.rand();
        }
        g.lay_city();
        g
    }

    fn lay_city(&mut self) {
        // Ruined blocks in a loose street grid, rubble scattered, a canal.
        let canal = 14 + (self.rand() % 6) as usize;
        for y in 0..MH {
            for x in 0..MW {
                let street = x % 8 == 3 || y % 7 == 2;
                let r = self.rand() % 100;
                let t = if street {
                    Tile::Road
                } else if r < 9 {
                    Tile::Ruin
                } else if r < 17 {
                    Tile::Rubble
                } else {
                    Tile::Ground
                };
                self.tiles[idx(x, y)] = t;
            }
        }
        // The canal runs north-south with two bridges, on roads.
        for y in 0..MH {
            let x = canal + ((y as f64 * 0.35).sin() * 1.5) as usize;
            if y % 7 != 2 {
                self.tiles[idx(x, y)] = Tile::Water;
                self.tiles[idx(x + 1, y)] = Tile::Water;
            }
        }
        // Clear the ground the two sides start on.
        let clear = |g: &mut Game, x0: usize, y0: usize, x1: usize, y1: usize| {
            for y in y0..y1.min(MH) {
                for x in x0..x1.min(MW) {
                    g.tiles[idx(x, y)] = if g.tiles[idx(x, y)] == Tile::Road { Tile::Road } else { Tile::Ground };
                }
            }
        };
        clear(self, 1, 10, 12, 21);
        clear(self, 36, 2, 47, 11);
        clear(self, 36, 19, 47, 28);
        self.place(BKind::Haven, Side::Human, 3, 13, true);
        self.place(BKind::Core, Side::Machine, 42, 5, true);
        self.place(BKind::Core, Side::Machine, 42, 22, true);
        // The Grid starts around each Core.
        let cores: Vec<(f64, f64)> = self.buildings.iter().filter(|b| b.kind == BKind::Core).map(|b| b.center()).collect();
        for (cx, cy) in cores {
            for y in 0..MH {
                for x in 0..MW {
                    if dist(x as f64 + 0.5, y as f64 + 0.5, cx, cy) < 3.6 && self.tiles[idx(x, y)].open() {
                        self.grid[idx(x, y)] = 60;
                    }
                }
            }
        }
        // Salvage: wrecked cars and machines from before.
        let mut n = 0;
        while n < 14 {
            let (x, y) = (2 + self.rand() as usize % (MW - 4), 1 + self.rand() as usize % (MH - 2));
            if self.tiles[idx(x, y)].open() && self.building_at(x, y).is_none() && self.grid[idx(x, y)] == 0 {
                let scrap = 180 + self.rand() % 160;
                let id = self.id();
                self.wrecks.push(Wreck { x: x as f64 + 0.5, y: y as f64 + 0.5, scrap, id });
                n += 1;
            }
        }
        for (k, (dx, dy)) in [(UKind::Scavenger, (7.0, 13.5)), (UKind::Scavenger, (7.0, 15.5)), (UKind::Rifleman, (8.0, 12.5)), (UKind::Rifleman, (8.5, 14.5)), (UKind::Rifleman, (8.0, 16.5))] {
            self.spawn(k, Side::Human, dx, dy);
        }
    }

    fn place(&mut self, kind: BKind, side: Side, tx: usize, ty: usize, done: bool) -> u32 {
        let s = kind.spec();
        for y in ty..ty + s.h {
            for x in tx..tx + s.w {
                self.tiles[idx(x, y)] = if self.tiles[idx(x, y)] == Tile::Road { Tile::Road } else { Tile::Ground };
            }
        }
        let id = self.id();
        self.buildings.push(Building { id, kind, side, tx, ty, hp: s.hp, progress: if done { 1.0 } else { 0.0 }, cooldown: 0.0, selected: false });
        id
    }

    fn spawn(&mut self, kind: UKind, side: Side, x: f64, y: f64) -> u32 {
        let id = self.id();
        let power = if side == Side::Machine { self.night_power() } else { 1.0 };
        self.units.push(Unit {
            id, kind, side, x, y, px: x, py: y, hp: kind.spec().hp, cooldown: 0.0, stunned: 0.0, hacking: 0.0, carry: 0,
            order: Order::Idle, path: vec![], selected: false, facing: if side == Side::Human { 0.0 } else { std::f64::consts::PI }, turned: false, repath: 0.0, pile: 0, path_goal: (usize::MAX, 0), power,
        });
        id
    }

    fn building_at(&self, x: usize, y: usize) -> Option<usize> {
        self.buildings.iter().position(|b| b.covers(x, y))
    }

    pub fn walkable(&self, x: usize, y: usize) -> bool {
        x < MW && y < MH && self.tiles[idx(x, y)].open() && self.building_at(x, y).is_none()
    }

    // ---- paths ---------------------------------------------------------------------

    /// 8-way A* over open tiles, from a tile to the nearest reachable tile to
    /// the goal (a goal inside a building or a ruin still gets a path to its edge).
    pub fn path(&self, from: (usize, usize), goal: (usize, usize)) -> Vec<(usize, usize)> {
        use std::cmp::Ordering;
        use std::collections::BinaryHeap;
        #[derive(PartialEq)]
        struct N(f64, usize);
        impl Eq for N {}
        impl PartialOrd for N {
            fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
                Some(self.cmp(o))
            }
        }
        impl Ord for N {
            fn cmp(&self, o: &Self) -> Ordering {
                o.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal)
            }
        }
        let h = |i: usize| {
            let (x, y) = (i % MW, i / MW);
            let (dx, dy) = ((x as f64 - goal.0 as f64).abs(), (y as f64 - goal.1 as f64).abs());
            dx.max(dy) + 0.414 * dx.min(dy)
        };
        let start = idx(from.0.min(MW - 1), from.1.min(MH - 1));
        let target = idx(goal.0.min(MW - 1), goal.1.min(MH - 1));
        let mut cost = vec![f64::INFINITY; MW * MH];
        let mut prev = vec![usize::MAX; MW * MH];
        let mut open = BinaryHeap::new();
        cost[start] = 0.0;
        open.push(N(h(start), start));
        let (mut best, mut best_h) = (start, h(start));
        let mut expanded = 0;
        while let Some(N(_, i)) = open.pop() {
            if i == target {
                best = i;
                break;
            }
            expanded += 1;
            if expanded > 2400 {
                break;
            }
            let (x, y) = ((i % MW) as i64, (i / MW) as i64);
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= MW as i64 || ny >= MH as i64 {
                    continue;
                }
                let (ux, uy) = (nx as usize, ny as usize);
                let j = idx(ux, uy);
                if j != target && !self.walkable(ux, uy) {
                    continue;
                }
                // No cutting a corner between two blocked tiles.
                if dx != 0 && dy != 0 && (!self.walkable(x as usize, uy) || !self.walkable(ux, y as usize)) {
                    continue;
                }
                let step = if dx != 0 && dy != 0 { 1.414 } else { 1.0 };
                let road = if self.tiles[j] == Tile::Road { 0.85 } else { 1.0 };
                let c = cost[i] + step * road;
                if c < cost[j] {
                    cost[j] = c;
                    prev[j] = i;
                    open.push(N(c + h(j), j));
                    if h(j) < best_h && (j != target || self.walkable(ux, uy)) {
                        best_h = h(j);
                        best = j;
                    }
                }
            }
        }
        let mut out = vec![];
        let mut i = best;
        // The goal itself, when it is a building or a ruin, is not walked into.
        if i == target && !self.walkable(goal.0.min(MW - 1), goal.1.min(MH - 1)) && prev[i] != usize::MAX {
            i = prev[i];
        }
        while i != start && i != usize::MAX {
            out.push((i % MW, i / MW));
            i = prev[i];
        }
        out.reverse();
        out
    }

    // ---- what a person does ------------------------------------------------------

    pub fn select(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, add: bool) {
        let (ax, bx) = (x0.min(x1), x0.max(x1));
        let (ay, by) = (y0.min(y1), y0.max(y1));
        let click = bx - ax < 0.3 && by - ay < 0.3;
        if !add {
            for u in &mut self.units {
                u.selected = false;
            }
            for b in &mut self.buildings {
                b.selected = false;
            }
        }
        if click {
            // The nearest of yours under the pointer, a unit before a building.
            let near = self
                .units
                .iter_mut()
                .filter(|u| u.side == Side::Human && dist(u.x, u.y, ax, ay) < 0.75)
                .min_by(|a, b| dist(a.x, a.y, ax, ay).total_cmp(&dist(b.x, b.y, ax, ay)));
            if let Some(u) = near {
                u.selected = !(add && u.selected);
                return;
            }
            if let Some(b) = self.buildings.iter_mut().find(|b| b.side == Side::Human && b.covers(ax as usize, ay as usize)) {
                b.selected = true;
            }
            return;
        }
        for u in &mut self.units {
            if u.side == Side::Human && u.x >= ax && u.x <= bx && u.y >= ay && u.y <= by {
                u.selected = true;
            }
        }
    }

    /// The selected units' order for a click at (x, y): attack or hack a
    /// machine, raze a machine building, salvage a wreck, else move there
    /// (fighting on the way when `assault`).
    pub fn order(&mut self, x: f64, y: f64, assault: bool) {
        let enemy = self
            .units
            .iter()
            .filter(|u| u.side == Side::Machine && dist(u.x, u.y, x, y) < 0.8)
            .min_by(|a, b| dist(a.x, a.y, x, y).total_cmp(&dist(b.x, b.y, x, y)))
            .map(|u| u.id);
        let core = self.buildings.iter().find(|b| b.side == Side::Machine && b.covers(x as usize, y as usize)).map(|b| b.id);
        let wreck = self.wrecks.iter().find(|w| dist(w.x, w.y, x, y) < 0.8).map(|w| w.id);
        let mut k: f64 = 0.0;
        let chosen: Vec<usize> = (0..self.units.len()).filter(|&i| self.units[i].selected && self.units[i].side == Side::Human).collect();
        let n = chosen.len() as f64;
        for i in chosen {
            let u = &mut self.units[i];
            u.hacking = 0.0;
            u.order = match (u.kind, enemy, core, wreck) {
                (UKind::Hacker, Some(e), _, _) => Order::Hack(e),
                (UKind::Scavenger, _, _, Some(w)) => Order::Gather(w),
                (UKind::Scavenger, Some(_), _, _) | (UKind::Scavenger, _, Some(_), _) => Order::Move(x, y),
                (_, Some(e), _, _) if u.kind != UKind::Hacker => Order::Attack(e),
                (_, _, Some(c), _) if u.kind != UKind::Hacker => Order::Raze(c),
                _ => {
                    // A group spreads round the point instead of piling onto it.
                    let a = k * 2.399;
                    let r = if n > 1.0 { 0.55 * k.sqrt() } else { 0.0 };
                    k += 1.0;
                    let (tx, ty) = (x + a.cos() * r, y + a.sin() * r);
                    if assault && u.kind != UKind::Scavenger { Order::Assault(tx, ty) } else { Order::Move(tx, ty) }
                }
            };
            u.path.clear();
            u.repath = 0.0;
        }
    }

    pub fn stop(&mut self) {
        for u in &mut self.units {
            if u.selected && u.side == Side::Human {
                u.order = Order::Idle;
                u.path.clear();
                u.hacking = 0.0;
            }
        }
    }

    /// Can a building of `kind` stand with its corner at (tx, ty)?
    pub fn can_place(&self, kind: BKind, tx: usize, ty: usize) -> bool {
        let s = kind.spec();
        if tx + s.w > MW || ty + s.h > MH {
            return false;
        }
        for y in ty..ty + s.h {
            for x in tx..tx + s.w {
                if !self.walkable(x, y) || self.grid[idx(x, y)] > 0 {
                    return false;
                }
                if self.units.iter().any(|u| u.x as usize == x && u.y as usize == y) {
                    return false;
                }
            }
        }
        let (cx, cy) = (tx as f64 + s.w as f64 / 2.0, ty as f64 + s.h as f64 / 2.0);
        self.buildings.iter().any(|b| b.side == Side::Human && b.reach(cx, cy) <= BUILD_REACH)
    }

    pub fn build(&mut self, kind: BKind, tx: usize, ty: usize) -> bool {
        let cost = kind.spec().cost;
        if self.scrap < cost {
            self.note = Note::NoScrap;
            return false;
        }
        if !self.can_place(kind, tx, ty) {
            self.note = Note::BadSpot;
            return false;
        }
        self.scrap -= cost;
        self.place(kind, Side::Human, tx, ty, false);
        true
    }

    pub fn train(&mut self, kind: UKind) -> bool {
        let s = kind.spec();
        let Some(from) = s.from else { return false };
        if !self.buildings.iter().any(|b| b.side == Side::Human && b.kind == from && b.done()) {
            self.note = Note::NeedBuilding;
            return false;
        }
        if self.scrap < s.cost {
            self.note = Note::NoScrap;
            return false;
        }
        let pets = self.units.iter().filter(|u| u.side == Side::Human && u.kind.pet()).count() + self.queue.iter().filter(|q| q.0.pet()).count();
        if kind.pet() && pets >= PETS {
            self.note = Note::TooManyPets;
            return false;
        }
        if !kind.pet() && self.survivors == 0 {
            self.note = Note::NoSurvivors;
            return false;
        }
        self.scrap -= s.cost;
        if !kind.pet() {
            self.survivors -= 1;
        }
        let secs = 4.0 + s.cost as f64 / 25.0;
        self.queue.push((kind, secs));
        true
    }

    /// Take a selected building down: half its scrap back, survivors stay.
    pub fn salvage_selected(&mut self) {
        let mut back = 0;
        self.buildings.retain(|b| {
            let go = b.selected && b.side == Side::Human && b.kind != BKind::Haven;
            if go {
                back += b.kind.spec().cost / 2;
            }
            !go
        });
        self.scrap += back;
    }

    // ---- time --------------------------------------------------------------------------

    pub fn update(&mut self, dt: f64) {
        if self.paused || self.state != State::Playing {
            return;
        }
        self.acc = (self.acc + dt.clamp(0.0, 0.25)).min(0.25);
        while self.acc >= TICK {
            self.acc -= TICK;
            self.step(TICK);
            if self.state != State::Playing {
                break;
            }
        }
    }

    pub fn step(&mut self, dt: f64) {
        for u in &mut self.units {
            u.px = u.x;
            u.py = u.y;
        }
        self.time += dt;
        self.light(dt);
        self.machines(dt);
        self.grow_grid(dt);
        self.produce(dt);
        self.construct(dt);
        let n = self.units.len();
        for i in 0..n {
            self.act(i, dt);
        }
        self.towers(dt);
        self.pets(dt);
        self.separate();
        self.bury();
        for f in &mut self.fx {
            f.age += dt;
        }
        self.fx.retain(|f| f.age < f.life);
        self.judge();
    }

    fn light(&mut self, dt: f64) {
        let night = self.is_night();
        if night && !self.night_was {
            self.note = Note::Nightfall;
            let n = self.nights + 1;
            self.wave_left = 2 + 2 * n;
            self.wave_clock = 0.0;
            self.scatter_caches(3 + n.min(6));
        }
        if !night && self.night_was {
            self.nights += 1;
            self.note = Note::Dawn;
            self.caches.clear();
        }
        self.night_was = night;
        // By day, people find the Haven: one every 12 s while there is room.
        if !night && self.survivors + self.people() < self.housing() {
            self.arrive_clock += dt;
            if self.arrive_clock >= 12.0 {
                self.arrive_clock = 0.0;
                self.survivors += 1;
                self.note = Note::Survivor;
            }
        }
    }

    /// Power cells in the dark, in the open city between the Haven and the
    /// Grid: worth more the further out they lie.
    fn scatter_caches(&mut self, n: u32) {
        let mut placed = 0;
        let mut tries = 0;
        while placed < n && tries < 400 {
            tries += 1;
            let (x, y) = (6 + self.rand() as usize % (MW - 14), 1 + self.rand() as usize % (MH - 2));
            if !self.walkable(x, y) || self.grid[idx(x, y)] > 0 || self.caches.iter().any(|c| dist(c.x, c.y, x as f64, y as f64) < 4.0) {
                continue;
            }
            let scrap = 30 + (x as u32) * 2 + self.rand() % 25;
            let id = self.id();
            self.caches.push(Cache { x: x as f64 + 0.5, y: y as f64 + 0.5, scrap, id });
            placed += 1;
        }
    }

    /// The night's strength: night 1's machines move at 70% and hit at 75%;
    /// each night adds a tenth to both, up to 1.4.
    fn night_power(&self) -> f64 {
        (0.7 + 0.1 * self.nights as f64).min(1.4)
    }

    /// Where the Cores' units come out, round each Core in turn.
    fn core_gate(&mut self) -> Option<(f64, f64)> {
        let cores: Vec<(f64, f64)> = self.buildings.iter().filter(|b| b.kind == BKind::Core).map(|b| b.center()).collect();
        if cores.is_empty() {
            return None;
        }
        let (cx, cy) = cores[self.rand() as usize % cores.len()];
        let a = self.randf() * std::f64::consts::TAU;
        Some((cx + a.cos() * 2.3, cy + a.sin() * 2.3))
    }

    fn haven(&self) -> Option<(f64, f64)> {
        self.buildings.iter().find(|b| b.kind == BKind::Haven).map(|b| b.center())
    }

    fn machines(&mut self, dt: f64) {
        let Some((hx, hy)) = self.haven() else { return };
        // The night's wave, a unit every 0.9 s until it is out.
        if self.wave_left > 0 {
            self.wave_clock -= dt;
            if self.wave_clock <= 0.0 {
                self.wave_clock = 0.9;
                self.wave_left -= 1;
                let n = self.nights + 1;
                let r = self.rand() % 100;
                let kind = if n >= 3 && self.wave_left == 0 && n % 3 == 0 {
                    UKind::Titan
                } else if r < 20 + 5 * n.min(10) {
                    UKind::Sentry
                } else {
                    UKind::Crawler
                };
                if let Some((x, y)) = self.core_gate() {
                    let id = self.spawn(kind, Side::Machine, x, y);
                    let target = self.raid_target(x, y).unwrap_or((hx, hy));
                    if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
                        u.order = Order::Assault(target.0, target.1);
                    }
                }
            }
        }
        // By day a patrol now and then; a Reclaimer when wrecks lie about.
        self.patrol_clock -= dt;
        if self.patrol_clock <= 0.0 && !self.is_night() {
            self.patrol_clock = 35.0;
            let alive = self.units.iter().filter(|u| u.side == Side::Machine && !u.turned).count();
            if alive < 12 {
                for _ in 0..2 {
                    if let Some((x, y)) = self.core_gate() {
                        let id = self.spawn(UKind::Crawler, Side::Machine, x, y);
                        let (tx, ty) = (MW as f64 * (0.35 + self.randf() * 0.3), MH as f64 * self.randf());
                        if let Some(u) = self.units.iter_mut().find(|u| u.id == id) {
                            u.order = Order::Assault(tx, ty);
                        }
                    }
                }
            }
            let reclaimers = self.units.iter().filter(|u| u.kind == UKind::Reclaimer && u.side == Side::Machine).count();
            if reclaimers < 2 && !self.wrecks.is_empty() {
                if let Some((x, y)) = self.core_gate() {
                    self.spawn(UKind::Reclaimer, Side::Machine, x, y);
                }
            }
        }
    }

    /// What a wave goes for: the nearest building of the survivors.
    fn raid_target(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        self.buildings
            .iter()
            .filter(|b| b.side == Side::Human)
            .map(|b| b.center())
            .min_by(|a, b| dist(a.0, a.1, x, y).total_cmp(&dist(b.0, b.1, x, y)))
    }

    /// The Grid grows a tile at a time from its edge, and never inside a
    /// Jammer's reach; a Jammer erases it round itself.
    fn grow_grid(&mut self, dt: f64) {
        self.grid_clock += dt;
        let jammers: Vec<(f64, f64)> = self.buildings.iter().filter(|b| b.kind == BKind::Jammer && b.done()).map(|b| b.center()).collect();
        let jammed = |x: usize, y: usize| jammers.iter().any(|&(jx, jy)| dist(x as f64 + 0.5, y as f64 + 0.5, jx, jy) < JAM_R);
        for g in self.grid.iter_mut().filter(|g| **g > 0) {
            *g = g.saturating_add(1).max(1);
        }
        if self.grid_clock < 1.2 {
            return;
        }
        self.grid_clock = 0.0;
        // Erase: each Jammer takes its nearest Grid tile.
        for &(jx, jy) in &jammers {
            let near = (0..MW * MH)
                .filter(|&i| self.grid[i] > 0 && dist((i % MW) as f64 + 0.5, (i / MW) as f64 + 0.5, jx, jy) < JAM_R)
                .min_by(|&a, &b| dist((a % MW) as f64, (a / MW) as f64, jx, jy).total_cmp(&dist((b % MW) as f64, (b / MW) as f64, jx, jy)));
            if let Some(i) = near {
                self.grid[i] = 0;
                self.fx.push(Fx { kind: 4, x0: jx, y0: jy, x1: (i % MW) as f64 + 0.5, y1: (i / MW) as f64 + 0.5, age: 0.0, life: 0.4 });
            }
        }
        // Grow: a few tiles on the edge, faster at night.
        if self.buildings.iter().all(|b| b.kind != BKind::Core) {
            return;
        }
        let tries = if self.is_night() { 3 } else { 1 };
        let edge: Vec<usize> = (0..MW * MH)
            .filter(|&i| {
                let (x, y) = (i % MW, i / MW);
                self.grid[i] == 0
                    && self.tiles[i].open()
                    && self.building_at(x, y).is_none_or(|b| self.buildings[b].side == Side::Machine)
                    && !jammed(x, y)
                    && [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                        let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                        nx >= 0 && ny >= 0 && nx < MW as i64 && ny < MH as i64 && self.grid[idx(nx as usize, ny as usize)] > 0
                    })
            })
            .collect();
        for _ in 0..tries {
            if edge.is_empty() {
                break;
            }
            let i = edge[self.rand() as usize % edge.len()];
            self.grid[i] = 1;
        }
    }

    fn produce(&mut self, dt: f64) {
        let Some((hx, hy)) = self.haven() else { return };
        let mut ready = vec![];
        for q in &mut self.queue {
            q.1 -= dt;
            if q.1 <= 0.0 {
                ready.push(q.0);
            }
        }
        self.queue.retain(|q| q.1 > 0.0);
        for kind in ready {
            let from = kind.spec().from.unwrap_or(BKind::Haven);
            let (bx, by) = self.buildings.iter().find(|b| b.side == Side::Human && b.kind == from && b.done()).map(|b| b.center()).unwrap_or((hx, hy));
            let a = self.randf() * std::f64::consts::TAU;
            let (x, y) = (bx + 1.0 + a.cos() * 0.8 + 1.2, by + a.sin() * 1.4);
            self.spawn(kind, Side::Human, x.min(MW as f64 - 0.5), y.clamp(0.5, MH as f64 - 0.5));
            if kind.pet() {
                self.note = Note::PetReady;
            }
        }
    }

    fn construct(&mut self, dt: f64) {
        for b in &mut self.buildings {
            if !b.done() {
                b.progress = (b.progress + dt / b.kind.spec().build).min(1.0);
            }
        }
    }

    // ---- one unit, one tick -------------------------------------------------------

    fn find_unit(&self, id: u32) -> Option<usize> {
        self.units.iter().position(|u| u.id == id)
    }

    fn on_grid(&self, x: f64, y: f64) -> bool {
        let (tx, ty) = (x as usize, y as usize);
        tx < MW && ty < MH && self.grid[idx(tx, ty)] > 0
    }

    fn exposed(&self, x: f64, y: f64) -> bool {
        self.buildings.iter().any(|b| b.kind == BKind::Floodlight && b.done() && dist(b.center().0, b.center().1, x, y) < LIGHT_R)
    }

    /// The nearest enemy unit within `r`, or the nearest enemy building.
    fn nearest_foe(&self, i: usize, r: f64) -> Option<(bool, u32, f64)> {
        let u = &self.units[i];
        let unit = self
            .units
            .iter()
            .filter(|o| o.side != u.side && o.hp > 0.0)
            .map(|o| (o.id, dist(o.x, o.y, u.x, u.y)))
            .filter(|(_, d)| *d <= r)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, d)) = unit {
            return Some((true, id, d));
        }
        self.buildings
            .iter()
            .filter(|b| b.side != u.side)
            .map(|b| (b.id, b.reach(u.x, u.y)))
            .filter(|(_, d)| *d <= r)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, d)| (false, id, d))
    }

    fn act(&mut self, i: usize, dt: f64) {
        if i >= self.units.len() || self.units[i].hp <= 0.0 {
            return;
        }
        let machine = self.units[i].side == Side::Machine;
        if machine && self.on_grid(self.units[i].x, self.units[i].y) {
            let max = self.units[i].kind.spec().hp;
            self.units[i].hp = (self.units[i].hp + 2.5 * dt).min(max);
        }
        {
            let u = &mut self.units[i];
            u.cooldown = (u.cooldown - dt).max(0.0);
            if u.stunned > 0.0 {
                u.stunned -= dt;
                return;
            }
        }
        let kind = self.units[i].kind;
        let spec = kind.spec();
        match self.units[i].order {
            Order::Idle => {
                // A dog at night goes for the nearest cache within 14 tiles.
                if kind == UKind::Dog && !machine && self.is_night() {
                    let (ux, uy) = (self.units[i].x, self.units[i].y);
                    if let Some(c) = self.caches.iter().map(|c| (c.x, c.y, dist(c.x, c.y, ux, uy))).filter(|c| c.2 < 14.0).min_by(|a, b| a.2.total_cmp(&b.2)) {
                        self.units[i].order = Order::Move(c.0, c.1);
                        return;
                    }
                }
                // Guard: fire at anything in range; a machine hunts what it sees.
                if spec.damage > 0.0 {
                    let sight = if machine { 5.5 } else { spec.range + 0.5 };
                    if let Some((is_unit, id, d)) = self.nearest_foe(i, sight) {
                        if d <= spec.range {
                            self.shoot(i, is_unit, id);
                        } else if machine {
                            self.units[i].order = if is_unit { Order::Attack(id) } else { Order::Raze(id) };
                        }
                    }
                }
                if kind == UKind::Reclaimer && machine {
                    if let Some(w) = self.wrecks.iter().min_by(|a, b| {
                        let u = &self.units[i];
                        dist(a.x, a.y, u.x, u.y).total_cmp(&dist(b.x, b.y, u.x, u.y))
                    }) {
                        self.units[i].order = Order::Gather(w.id);
                    }
                }
                if kind == UKind::Scavenger && !machine {
                    // Idle hands salvage: the nearest wreck within 8 tiles.
                    let (ux, uy) = (self.units[i].x, self.units[i].y);
                    let near = self.wrecks.iter().map(|w| (w.id, dist(w.x, w.y, ux, uy))).filter(|(_, d)| *d < 8.0).min_by(|a, b| a.1.total_cmp(&b.1));
                    self.units[i].order = match (self.units[i].carry, near) {
                        (c, _) if c > 0 => Order::Deliver,
                        (_, Some((id, _))) => Order::Gather(id),
                        _ => Order::Idle,
                    };
                }
            }
            Order::Move(x, y) => {
                if self.walk(i, x, y, dt) {
                    self.units[i].order = Order::Idle;
                }
            }
            Order::Assault(x, y) => {
                let sight = if machine { 5.0 } else { spec.range + 1.0 };
                if spec.damage > 0.0 {
                    if let Some((is_unit, id, d)) = self.nearest_foe(i, sight) {
                        if d <= spec.range {
                            self.shoot(i, is_unit, id);
                        } else {
                            let (tx, ty) = self.pos_of(is_unit, id).unwrap_or((x, y));
                            self.walk(i, tx, ty, dt);
                        }
                        return;
                    }
                }
                if self.walk(i, x, y, dt) {
                    // Arrived: a machine looks for the next thing to break.
                    self.units[i].order = Order::Idle;
                    if machine {
                        let (ux, uy) = (self.units[i].x, self.units[i].y);
                        if let Some(t) = self.raid_target(ux, uy) {
                            self.units[i].order = Order::Assault(t.0, t.1);
                        }
                    }
                }
            }
            Order::Attack(id) | Order::Raze(id) => {
                let is_unit = matches!(self.units[i].order, Order::Attack(_));
                let Some((tx, ty)) = self.pos_of(is_unit, id) else {
                    self.units[i].order = Order::Idle;
                    return;
                };
                let d = if is_unit {
                    dist(tx, ty, self.units[i].x, self.units[i].y)
                } else {
                    self.buildings.iter().find(|b| b.id == id).map_or(f64::MAX, |b| b.reach(self.units[i].x, self.units[i].y))
                };
                if spec.damage <= 0.0 {
                    self.units[i].order = Order::Idle;
                } else if d <= spec.range {
                    self.shoot(i, is_unit, id);
                } else {
                    self.walk(i, tx, ty, dt);
                }
            }
            Order::Hack(id) => {
                let Some(t) = self.find_unit(id) else {
                    self.units[i].order = Order::Idle;
                    self.units[i].hacking = 0.0;
                    return;
                };
                if self.units[t].side == self.units[i].side {
                    self.units[i].order = Order::Idle;
                    return;
                }
                let (tx, ty) = (self.units[t].x, self.units[t].y);
                if dist(tx, ty, self.units[i].x, self.units[i].y) > spec.range {
                    self.units[i].hacking = 0.0;
                    self.walk(i, tx, ty, dt);
                    return;
                }
                self.units[i].hacking += dt;
                self.units[i].facing = (ty - self.units[i].y).atan2(tx - self.units[i].x);
                if self.fx.iter().all(|f| !(f.kind == 2 && f.age < 0.05 && (f.x0 - self.units[i].x).abs() < 0.01)) {
                    self.fx.push(Fx { kind: 2, x0: self.units[i].x, y0: self.units[i].y, x1: tx, y1: ty, age: 0.0, life: 0.12 });
                }
                if self.units[i].hacking >= self.units[t].kind.hack_time() {
                    let o = &mut self.units[t];
                    o.side = Side::Human;
                    o.turned = true;
                    o.order = Order::Idle;
                    o.path.clear();
                    o.stunned = 0.6;
                    self.units[i].hacking = 0.0;
                    self.units[i].order = Order::Idle;
                    self.turned += 1;
                    self.note = Note::Turned;
                }
            }
            Order::Gather(wid) => {
                let Some(w) = self.wrecks.iter().position(|w| w.id == wid) else {
                    self.units[i].order = if self.units[i].carry > 0 && !machine { Order::Deliver } else { Order::Idle };
                    return;
                };
                let (wx, wy) = (self.wrecks[w].x, self.wrecks[w].y);
                if dist(wx, wy, self.units[i].x, self.units[i].y) > 0.9 {
                    self.walk(i, wx, wy, dt);
                    return;
                }
                // Salvage 12 a second, 40 a load; a Reclaimer just eats it.
                let ticks = |t: f64| (t * 12.0) as u64;
                let take = (if ticks(self.time) != ticks(self.time - dt) { 1 } else { 0 }).min(self.wrecks[w].scrap);
                self.wrecks[w].scrap -= take;
                if machine {
                    if self.wrecks[w].scrap == 0 {
                        self.units[i].order = Order::Idle;
                    }
                } else {
                    self.units[i].carry += take;
                    if self.units[i].carry >= 40 || self.wrecks[w].scrap == 0 {
                        self.units[i].order = Order::Deliver;
                        // Remember the pile, to come back to.
                        self.units[i].pile = wid;
                    }
                }
                self.wrecks.retain(|w| w.scrap > 0);
            }
            Order::Deliver => {
                let (ux, uy) = (self.units[i].x, self.units[i].y);
                let home = self
                    .buildings
                    .iter()
                    .filter(|b| b.side == Side::Human && matches!(b.kind, BKind::Haven | BKind::Shelter) && b.done())
                    .min_by(|a, b| a.reach(ux, uy).total_cmp(&b.reach(ux, uy)))
                    .map(|b| (b.id, b.center(), b.reach(ux, uy)));
                let Some((_, (hx, hy), d)) = home else {
                    self.units[i].order = Order::Idle;
                    return;
                };
                if d > 0.8 {
                    self.walk(i, hx, hy, dt);
                    return;
                }
                self.scrap += self.units[i].carry;
                self.units[i].carry = 0;
                let back = self.units[i].pile;
                self.units[i].order = if self.wrecks.iter().any(|w| w.id == back) {
                    Order::Gather(back)
                } else {
                    // That pile is gone: the nearest other, if any.
                    self.wrecks
                        .iter()
                        .min_by(|a, b| dist(a.x, a.y, ux, uy).total_cmp(&dist(b.x, b.y, ux, uy)))
                        .map_or(Order::Idle, |w| Order::Gather(w.id))
                };
            }
        }
    }

    fn pos_of(&self, is_unit: bool, id: u32) -> Option<(f64, f64)> {
        if is_unit {
            self.units.iter().find(|u| u.id == id && u.hp > 0.0).map(|u| (u.x, u.y))
        } else {
            self.buildings.iter().find(|b| b.id == id).map(|b| b.center())
        }
    }

    /// One step toward (x, y) along a path; true when there.
    fn walk(&mut self, i: usize, x: f64, y: f64, dt: f64) -> bool {
        let (ux, uy) = (self.units[i].x, self.units[i].y);
        if dist(ux, uy, x, y) < 0.15 {
            return true;
        }
        let goal = (x.clamp(0.0, MW as f64 - 0.01) as usize, y.clamp(0.0, MH as f64 - 0.01) as usize);
        self.units[i].repath -= dt;
        // A path is found again for a new goal - at once when there is none
        // to walk, else at most twice a second (a chased unit moves its goal
        // every tile) - and every 2 s anyway, round whatever was built since.
        let moved = self.units[i].path_goal != goal;
        let lost = self.units[i].path.is_empty() && (ux as usize, uy as usize) != goal && moved;
        if lost || (moved && self.units[i].repath <= 0.0) || self.units[i].repath < -2.0 {
            let p = self.path((ux as usize, uy as usize), goal);
            let u = &mut self.units[i];
            u.path = p;
            u.path_goal = goal;
            u.repath = 0.5;
        }
        let mut speed = self.units[i].kind.spec().speed * self.units[i].power;
        if self.units[i].side == Side::Machine && self.on_grid(ux, uy) {
            speed *= 1.35;
        }
        if self.tiles[idx(ux as usize % MW, uy as usize % MH)] == Tile::Road {
            speed *= 1.15;
        }
        // The next waypoint, or the goal itself once on its tile.
        while self.units[i].path.len() > 1 && self.units[i].path[0] == (ux as usize, uy as usize) {
            self.units[i].path.remove(0);
        }
        let (wx, wy) = match self.units[i].path.first() {
            Some(&(tx, ty)) if (ux as usize, uy as usize) != (tx, ty) => (tx as f64 + 0.5, ty as f64 + 0.5),
            _ => (x, y),
        };
        let d = dist(ux, uy, wx, wy);
        let step = speed * dt;
        let u = &mut self.units[i];
        if d <= step {
            u.x = wx;
            u.y = wy;
            if !u.path.is_empty() {
                u.path.remove(0);
            }
        } else {
            u.x += (wx - ux) / d * step;
            u.y += (wy - uy) / d * step;
        }
        u.facing = (wy - uy).atan2(wx - ux);
        u.path.is_empty() && dist(u.x, u.y, x, y) < 0.2
    }

    fn shoot(&mut self, i: usize, is_unit: bool, id: u32) {
        let (ux, uy) = (self.units[i].x, self.units[i].y);
        let Some((tx, ty)) = self.pos_of(is_unit, id) else { return };
        self.units[i].facing = (ty - uy).atan2(tx - ux);
        if self.units[i].cooldown > 0.0 {
            return;
        }
        let s = self.units[i].kind.spec();
        self.units[i].cooldown = s.reload;
        let side = self.units[i].side;
        let kind = match self.units[i].kind {
            UKind::Emp => 1,
            UKind::Titan => 3,
            _ => 0,
        };
        self.fx.push(Fx { kind, x0: ux, y0: uy, x1: tx, y1: ty, age: 0.0, life: if kind == 0 { 0.08 } else { 0.35 } });
        let damage = s.damage * self.units[i].power;
        self.hit(side, tx, ty, is_unit, id, damage, s.splash, s.stun);
    }

    /// Damage at a point: the target, and anything of the other side in the
    /// splash. Machines in a Floodlight's reach take more.
    fn hit(&mut self, from: Side, x: f64, y: f64, is_unit: bool, id: u32, damage: f64, splash: f64, stun: f64) {
        let exposed: Vec<bool> = self.units.iter().map(|u| u.side == Side::Machine && self.exposed(u.x, u.y)).collect();
        for (k, u) in self.units.iter_mut().enumerate() {
            if u.side == from {
                continue;
            }
            let direct = is_unit && u.id == id;
            let near = splash > 0.0 && dist(u.x, u.y, x, y) <= splash;
            if direct || near {
                let mult = if exposed[k] { EXPOSED } else { 1.0 };
                u.hp -= damage * mult * if direct { 1.0 } else { 0.6 };
                if stun > 0.0 && u.side == Side::Machine {
                    u.stunned = u.stunned.max(stun);
                }
                // Hit while idle: answer it.
                if u.order == Order::Idle && u.kind.spec().damage > 0.0 && u.side == Side::Human {
                    u.order = Order::Assault(x, y);
                }
            }
        }
        for b in &mut self.buildings {
            if b.side == from {
                continue;
            }
            if (!is_unit && b.id == id) || (splash > 0.0 && b.reach(x, y) <= splash) {
                b.hp -= damage * if !is_unit && b.id == id { 1.0 } else { 0.5 };
                if b.kind == BKind::Haven {
                    self.note = Note::HavenHit;
                }
            }
        }
    }

    /// Caches go to whichever of yours reaches them; cats purr the wounded
    /// near them back to health (5 hp a second, within 2.5 tiles).
    fn pets(&mut self, dt: f64) {
        let mut got = 0;
        let mut taken = vec![];
        for c in &self.caches {
            if self.units.iter().any(|u| u.side == Side::Human && dist(u.x, u.y, c.x, c.y) < 0.7) {
                got += c.scrap;
                taken.push(c.id);
                self.fx.push(Fx { kind: 5, x0: c.x, y0: c.y, x1: c.x, y1: c.y, age: 0.0, life: 0.8 });
            }
        }
        if !taken.is_empty() {
            self.caches.retain(|c| !taken.contains(&c.id));
            self.scrap += got;
            self.found += taken.len() as u32;
            self.note = Note::Cache;
        }
        let cats: Vec<(f64, f64)> = self.units.iter().filter(|u| u.kind == UKind::Cat && u.side == Side::Human).map(|u| (u.x, u.y)).collect();
        for u in &mut self.units {
            if u.side == Side::Human && cats.iter().any(|&(x, y)| dist(x, y, u.x, u.y) < 2.5) {
                u.hp = (u.hp + 5.0 * dt).min(u.kind.spec().hp);
            }
        }
    }

    fn towers(&mut self, dt: f64) {
        let n = self.buildings.len();
        for k in 0..n {
            let b = &self.buildings[k];
            if !matches!(b.kind, BKind::Turret | BKind::Haven) || !b.done() {
                continue;
            }
            let s = b.kind.spec();
            let (bx, by) = b.center();
            let cd = (b.cooldown - dt).max(0.0);
            self.buildings[k].cooldown = cd;
            if cd > 0.0 {
                continue;
            }
            let target = self
                .units
                .iter()
                .filter(|u| u.side == Side::Machine && dist(u.x, u.y, bx, by) <= s.range)
                .min_by(|a, b| dist(a.x, a.y, bx, by).total_cmp(&dist(b.x, b.y, bx, by)))
                .map(|u| (u.id, u.x, u.y));
            if let Some((id, x, y)) = target {
                self.buildings[k].cooldown = s.reload;
                self.fx.push(Fx { kind: 0, x0: bx, y0: by, x1: x, y1: y, age: 0.0, life: 0.08 });
                self.hit(Side::Human, x, y, true, id, s.damage, 0.0, 0.0);
            }
        }
    }

    /// Units push softly apart, so a crowd spreads instead of stacking.
    fn separate(&mut self) {
        let n = self.units.len();
        for a in 0..n {
            for b in a + 1..n {
                let (dx, dy) = (self.units[b].x - self.units[a].x, self.units[b].y - self.units[a].y);
                let d2 = dx * dx + dy * dy;
                if d2 > 0.0 && d2 < 0.16 {
                    let d = d2.sqrt();
                    let push = (0.4 - d) * 0.25;
                    let (px, py) = (dx / d * push, dy / d * push);
                    for (k, s) in [(a, -1.0), (b, 1.0)] {
                        let (nx, ny) = (self.units[k].x + px * s, self.units[k].y + py * s);
                        if self.walkable(nx as usize, ny as usize) {
                            self.units[k].x = nx;
                            self.units[k].y = ny;
                        }
                    }
                }
            }
        }
    }

    /// The fallen leave wrecks; destroyed buildings leave rubble.
    fn bury(&mut self) {
        let mut dead = vec![];
        self.units.retain(|u| {
            if u.hp <= 0.0 {
                dead.push((u.x, u.y, u.kind, u.side));
                false
            } else {
                true
            }
        });
        for (x, y, kind, side) in dead {
            if side == Side::Machine {
                self.kills += 1;
            }
            let id = self.id();
            self.wrecks.push(Wreck { x, y, scrap: kind.spec().salvage, id });
            self.fx.push(Fx { kind: 3, x0: x, y0: y, x1: x, y1: y, age: 0.0, life: 0.5 });
        }
        let mut fallen = vec![];
        self.buildings.retain(|b| {
            if b.hp <= 0.0 {
                fallen.push((b.center(), b.kind));
                false
            } else {
                true
            }
        });
        for ((x, y), kind) in fallen {
            if kind == BKind::Core {
                self.cores_down += 1;
                self.note = Note::CoreDown;
                // A Core's Grid dies with it, near it.
                for i in 0..MW * MH {
                    if dist((i % MW) as f64 + 0.5, (i / MW) as f64 + 0.5, x, y) < 6.0 {
                        self.grid[i] = 0;
                    }
                }
            }
            let id = self.id();
            self.wrecks.push(Wreck { x, y, scrap: kind.spec().cost / 3 + if kind == BKind::Core { 400 } else { 0 }, id });
            self.fx.push(Fx { kind: 3, x0: x, y0: y, x1: x, y1: y, age: 0.0, life: 0.9 });
        }
    }

    fn judge(&mut self) {
        if !self.buildings.iter().any(|b| b.kind == BKind::Haven) {
            self.state = State::Lost;
        } else if !self.buildings.iter().any(|b| b.kind == BKind::Core) {
            self.state = State::Won;
        }
    }

    // ---- what the page reads ------------------------------------------------------

    /// The world as numbers. HEAD floats, then UNIT_F per unit, BLD_F per
    /// building, WRECK_F per wreck, FX_F per effect, then the training queue
    /// (kind, seconds left) - the layout web/main.js reads.
    pub fn frame(&mut self) -> &[f64] {
        let mut f = std::mem::take(&mut self.frame);
        f.clear();
        let cycle = self.time % (DAY + NIGHT);
        let day_t = if cycle < DAY { cycle / DAY } else { (cycle - DAY) / NIGHT };
        let housing = self.housing();
        let people = self.people();
        let night = self.is_night();
        let until = self.until_change();
        let score = self.score();
        let head = [
            self.state as u8 as f64,
            self.time,
            if night { 1.0 } else { 0.0 },
            day_t,
            until,
            self.scrap as f64,
            self.survivors as f64,
            people as f64,
            housing as f64,
            self.nights as f64,
            score as f64,
            self.kills as f64,
            self.turned as f64,
            self.cores_down as f64,
            self.buildings.iter().filter(|b| b.kind == BKind::Core).count() as f64,
            self.units.len() as f64,
            self.buildings.len() as f64,
            self.wrecks.len() as f64,
            self.fx.len() as f64,
            self.queue.len() as f64,
            self.note as u8 as f64,
            if self.paused { 1.0 } else { 0.0 },
            self.acc / TICK,
            self.wave_left as f64,
            self.caches.len() as f64,
            self.night_power(),
        ];
        f.extend_from_slice(&head);
        let a = self.acc / TICK;
        for u in &self.units {
            let max = u.kind.spec().hp;
            f.extend_from_slice(&[
                u.id as f64,
                u.kind as u8 as f64,
                u.side as u8 as f64,
                u.px + (u.x - u.px) * a,
                u.py + (u.y - u.py) * a,
                (u.hp / max).clamp(0.0, 1.0),
                if u.selected { 1.0 } else { 0.0 },
                u.facing,
                if u.stunned > 0.0 { 1.0 } else { 0.0 },
                if u.hacking > 0.0 { u.hacking / 7.0 } else { u.carry as f64 / 40.0 },
                if u.turned { 1.0 } else { 0.0 },
            ]);
        }
        for b in &self.buildings {
            let s = b.kind.spec();
            f.extend_from_slice(&[
                b.id as f64,
                b.kind as u8 as f64,
                b.side as u8 as f64,
                b.tx as f64,
                b.ty as f64,
                s.w as f64,
                s.h as f64,
                (b.hp / s.hp).clamp(0.0, 1.0),
                b.progress,
                if b.selected { 1.0 } else { 0.0 },
            ]);
        }
        for w in &self.wrecks {
            f.extend_from_slice(&[w.x, w.y, w.scrap as f64]);
        }
        for x in &self.fx {
            f.extend_from_slice(&[x.kind as f64, x.x0, x.y0, x.x1, x.y1, x.age / x.life, 0.0]);
        }
        for (k, t) in &self.queue {
            f.extend_from_slice(&[*k as u8 as f64, *t]);
        }
        for c in &self.caches {
            f.extend_from_slice(&[c.x, c.y, c.scrap as f64]);
        }
        self.note = Note::None;
        self.frame = f;
        &self.frame
    }

    /// The map, one byte a tile: the tile kind in the low 3 bits, 8 when the
    /// Grid is on it.
    pub fn tile_bytes(&mut self) -> &[u8] {
        for i in 0..MW * MH {
            self.tile_bytes[i] = self.tiles[i] as u8 | if self.grid[i] > 0 { 8 } else { 0 };
        }
        &self.tile_bytes
    }
}

// ---- the WebAssembly boundary -------------------------------------------------------
//
// One game per page, behind a pointer the page holds. Every function takes it.

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;

    #[no_mangle]
    pub extern "C" fn game_new(seed: u32) -> *mut Game {
        Box::into_raw(Box::new(Game::new(seed)))
    }

    fn g<'a>(p: *mut Game) -> &'a mut Game {
        // SAFETY: the page only passes the pointer game_new gave it.
        unsafe { &mut *p }
    }

    #[no_mangle]
    pub extern "C" fn game_update(p: *mut Game, dt: f64) {
        g(p).update(dt)
    }
    #[no_mangle]
    pub extern "C" fn game_frame(p: *mut Game) -> *const f64 {
        g(p).frame().as_ptr()
    }
    /// Length of the frame game_frame last wrote.
    #[no_mangle]
    pub extern "C" fn game_frame_len(p: *mut Game) -> usize {
        g(p).frame.len()
    }
    #[no_mangle]
    pub extern "C" fn game_tiles(p: *mut Game) -> *const u8 {
        g(p).tile_bytes().as_ptr()
    }
    #[no_mangle]
    pub extern "C" fn game_select(p: *mut Game, x0: f64, y0: f64, x1: f64, y1: f64, add: u32) {
        g(p).select(x0, y0, x1, y1, add != 0)
    }
    #[no_mangle]
    pub extern "C" fn game_order(p: *mut Game, x: f64, y: f64, assault: u32) {
        g(p).order(x, y, assault != 0)
    }
    #[no_mangle]
    pub extern "C" fn game_stop(p: *mut Game) {
        g(p).stop()
    }
    /// kind: index in BKind::PLACEABLE. 1 placed, 0 not.
    #[no_mangle]
    pub extern "C" fn game_build(p: *mut Game, kind: u32, tx: u32, ty: u32) -> u32 {
        BKind::from_i(kind).is_some_and(|k| g(p).build(k, tx as usize, ty as usize)) as u32
    }
    #[no_mangle]
    pub extern "C" fn game_can_place(p: *mut Game, kind: u32, tx: u32, ty: u32) -> u32 {
        BKind::from_i(kind).is_some_and(|k| g(p).can_place(k, tx as usize, ty as usize)) as u32
    }
    /// kind: index in UKind::HUMAN.
    #[no_mangle]
    pub extern "C" fn game_train(p: *mut Game, kind: u32) -> u32 {
        UKind::from_i(kind).is_some_and(|k| g(p).train(k)) as u32
    }
    #[no_mangle]
    pub extern "C" fn game_salvage(p: *mut Game) {
        g(p).salvage_selected()
    }
    #[no_mangle]
    pub extern "C" fn game_pause(p: *mut Game, on: u32) {
        g(p).paused = on != 0
    }
    #[no_mangle]
    pub extern "C" fn game_free(p: *mut Game) {
        // SAFETY: as above; the page drops its pointer after this.
        drop(unsafe { Box::from_raw(p) })
    }
}

// ---- tests ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn run(g: &mut Game, secs: f64) {
        for _ in 0..(secs / TICK) as usize {
            g.step(TICK);
            if g.state != State::Playing {
                break;
            }
        }
    }

    fn unit(g: &Game, kind: UKind, side: Side) -> usize {
        g.units.iter().position(|u| u.kind == kind && u.side == side).expect("a unit of that kind")
    }

    #[test]
    fn a_seed_is_a_city_and_replays_the_same() {
        let (mut a, mut b) = (Game::new(7), Game::new(7));
        run(&mut a, 200.0);
        run(&mut b, 200.0);
        assert_eq!(a.tiles, b.tiles);
        assert_eq!(a.grid, b.grid);
        assert_eq!(a.units.len(), b.units.len());
        assert_eq!(a.scrap, b.scrap);
        assert_ne!(Game::new(7).tiles, Game::new(8).tiles);
    }

    #[test]
    fn the_start_is_a_haven_two_cores_and_a_squad() {
        let g = Game::new(3);
        assert_eq!(g.buildings.iter().filter(|b| b.kind == BKind::Haven).count(), 1);
        assert_eq!(g.buildings.iter().filter(|b| b.kind == BKind::Core).count(), 2);
        assert_eq!(g.units.iter().filter(|u| u.side == Side::Human).count(), 5);
        assert!(g.grid.iter().filter(|&&c| c > 0).count() > 20);
        assert!(!g.is_night());
    }

    #[test]
    fn a_path_goes_round_what_blocks_it() {
        let mut g = Game::new(11);
        for y in 0..MH {
            for x in 0..MW {
                g.tiles[idx(x, y)] = Tile::Ground;
            }
        }
        g.buildings.clear();
        for y in 2..MH - 1 {
            g.tiles[idx(20, y)] = Tile::Ruin;
        }
        let p = g.path((10, 15), (30, 15));
        assert_eq!(*p.last().unwrap(), (30, 15));
        assert!(p.iter().all(|&(x, y)| g.walkable(x, y)));
        assert!(p.iter().any(|&(x, y)| x == 20 && y < 2), "it goes over the top of the wall");
    }

    #[test]
    fn scavengers_salvage_wrecks_into_scrap() {
        let mut g = Game::new(5);
        let s = unit(&g, UKind::Scavenger, Side::Human);
        let (sx, sy) = (g.units[s].x, g.units[s].y);
        let id = g.id();
        g.wrecks.push(Wreck { x: sx + 3.0, y: sy, scrap: 500, id });
        let before = g.scrap;
        g.units[s].order = Order::Gather(id);
        g.paused = false;
        // Keep the machines out of it.
        g.units.retain(|u| u.side == Side::Human);
        run(&mut g, 25.0);
        assert!(g.scrap >= before + 80, "scrap {} from {before}", g.scrap);
    }

    #[test]
    fn night_brings_a_wave_and_dawn_counts_it() {
        let mut g = Game::new(9);
        run(&mut g, DAY + 8.0);
        assert!(g.is_night());
        assert!(g.units.iter().filter(|u| u.side == Side::Machine && u.kind != UKind::Reclaimer).count() >= 3);
        run(&mut g, NIGHT);
        assert!(!g.is_night() || g.state != State::Playing);
        assert!(g.nights >= 1 || g.state == State::Lost);
    }

    #[test]
    fn a_hacker_turns_a_machine() {
        let mut g = Game::new(13);
        g.units.clear();
        let h = g.spawn(UKind::Hacker, Side::Human, 20.5, 15.5);
        let c = g.spawn(UKind::Crawler, Side::Machine, 22.0, 15.5);
        g.units.iter_mut().find(|u| u.id == c).unwrap().stunned = 10.0;
        g.units.iter_mut().find(|u| u.id == h).unwrap().order = Order::Hack(c);
        run(&mut g, 4.0);
        let t = g.units.iter().find(|u| u.id == c).unwrap();
        assert_eq!(t.side, Side::Human);
        assert!(t.turned);
        assert_eq!(g.turned, 1);
        // A turned machine is no survivor: it costs no housing.
        assert_eq!(g.people(), 1);
    }

    #[test]
    fn an_emp_stuns_and_a_floodlight_exposes() {
        let mut g = Game::new(17);
        g.units.clear();
        let c = g.spawn(UKind::Sentry, Side::Machine, 20.5, 15.5);
        g.hit(Side::Human, 20.5, 15.5, true, c, 7.0, 1.3, 2.5);
        let u = g.units.iter().find(|u| u.id == c).unwrap();
        assert!(u.stunned > 2.0);
        let plain = 85.0 - u.hp;
        g.place(BKind::Floodlight, Side::Human, 21, 16, true);
        g.hit(Side::Human, 20.5, 15.5, true, c, 7.0, 0.0, 0.0);
        let lit = 85.0 - plain - g.units.iter().find(|u| u.id == c).unwrap().hp;
        assert!((lit - plain * EXPOSED).abs() < 1e-6, "lit {lit} plain {plain}");
    }

    #[test]
    fn the_grid_grows_and_a_jammer_holds_it_back() {
        let mut g = Game::new(21);
        let start = g.grid.iter().filter(|&&c| c > 0).count();
        g.units.clear();
        g.wave_left = 0;
        run(&mut g, 60.0);
        let grown = g.grid.iter().filter(|&&c| c > 0).count();
        assert!(grown > start, "{grown} > {start}");
        // A Jammer by a Core: the Grid round it goes and stays gone.
        let core = g.buildings.iter().find(|b| b.kind == BKind::Core).unwrap();
        let (jx, jy) = (core.tx - 2, core.ty + 1);
        g.tiles[idx(jx, jy)] = Tile::Ground;
        g.grid[idx(jx, jy)] = 0;
        g.place(BKind::Jammer, Side::Human, jx, jy, true);
        // Nothing comes for the Jammer here: this measures the Grid alone.
        for _ in 0..40 {
            g.units.clear();
            g.wave_left = 0;
            run(&mut g, 1.0);
        }
        let near = (0..MW * MH).filter(|&i| g.grid[i] > 0 && dist((i % MW) as f64 + 0.5, (i / MW) as f64 + 0.5, jx as f64 + 0.5, jy as f64 + 0.5) < JAM_R - 0.6).count();
        assert_eq!(near, 0);
    }

    #[test]
    fn building_and_training_cost_scrap_and_survivors() {
        let mut g = Game::new(25);
        let (hx, hy) = (g.buildings[0].tx, g.buildings[0].ty);
        assert!(!g.train(UKind::Rifleman), "no Armory yet");
        let spot = (0..MH).flat_map(|y| (0..MW).map(move |x| (x, y))).find(|&(x, y)| {
            x + 1 >= hx + 4 && y >= hy && g.can_place(BKind::Armory, x, y)
        });
        let (ax, ay) = spot.expect("somewhere to build");
        let before = g.scrap;
        assert!(g.build(BKind::Armory, ax, ay));
        assert_eq!(g.scrap, before - 150);
        g.units.retain(|u| u.side == Side::Human);
        run(&mut g, 13.0);
        g.scrap = 500;
        g.survivors = 1;
        assert!(g.train(UKind::Rifleman));
        assert!(!g.train(UKind::Rifleman), "no survivor left");
        let n = g.units.len();
        run(&mut g, 8.0);
        assert_eq!(g.units.len(), n + 1);
        // Too far from the base: refused.
        assert!(!g.can_place(BKind::Shelter, 40, 15));
    }

    #[test]
    fn burning_out_the_cores_wins_and_losing_the_haven_loses() {
        let mut g = Game::new(29);
        for b in &mut g.buildings {
            if b.kind == BKind::Core {
                b.hp = 0.0;
            }
        }
        g.step(TICK);
        assert_eq!(g.state, State::Won);
        assert_eq!(g.cores_down, 2);
        assert!(g.score() >= 2000);
        let mut g = Game::new(29);
        g.buildings.iter_mut().find(|b| b.kind == BKind::Haven).unwrap().hp = 0.0;
        g.step(TICK);
        assert_eq!(g.state, State::Lost);
    }

    #[test]
    fn pets_cost_scrap_not_survivors_and_are_capped() {
        let mut g = Game::new(41);
        g.scrap = 2000;
        g.survivors = 0;
        assert!(g.train(UKind::Dog), "a pet needs no survivor");
        assert!(g.train(UKind::Cat));
        for _ in 0..4 {
            assert!(g.train(UKind::Dog));
        }
        assert!(!g.train(UKind::Cat), "six pets at most");
        let people = g.people();
        g.units.retain(|u| u.side == Side::Human);
        run(&mut g, 20.0);
        assert_eq!(g.units.iter().filter(|u| u.kind.pet()).count(), PETS);
        assert_eq!(g.people(), people, "pets are not people");
    }

    #[test]
    fn caches_show_at_night_go_at_dawn_and_a_dog_fetches_them() {
        let mut g = Game::new(43);
        assert!(g.caches.is_empty());
        g.units.retain(|u| u.side == Side::Human && u.kind == UKind::Scavenger);
        let d = g.spawn(UKind::Dog, Side::Human, 10.5, 15.5);
        for _ in 0..((DAY + 1.0) / TICK) as usize {
            g.step(TICK);
            g.units.retain(|u| u.side == Side::Human);
            g.wave_left = 0;
        }
        assert!(g.is_night());
        assert!(g.caches.len() >= 4, "{} caches", g.caches.len());
        let before = g.scrap;
        for _ in 0..(40.0 / TICK) as usize {
            g.step(TICK);
            g.units.retain(|u| u.side == Side::Human);
            g.wave_left = 0;
        }
        assert!(g.found >= 1, "the dog found nothing");
        assert!(g.scrap > before);
        assert!(g.units.iter().any(|u| u.id == d));
        run(&mut g, NIGHT);
        assert!(!g.is_night());
        assert!(g.caches.is_empty(), "gone at dawn");
    }

    #[test]
    fn a_cat_purrs_the_wounded_back() {
        let mut g = Game::new(47);
        g.units.clear();
        let r = g.spawn(UKind::Rifleman, Side::Human, 10.5, 15.5);
        g.spawn(UKind::Cat, Side::Human, 11.5, 15.5);
        g.units.iter_mut().find(|u| u.id == r).unwrap().hp = 20.0;
        run(&mut g, 4.0);
        assert!(g.units.iter().find(|u| u.id == r).unwrap().hp >= 39.0);
    }

    #[test]
    fn the_machines_grow_stronger_each_night() {
        let mut g = Game::new(53);
        assert!((g.night_power() - 0.7).abs() < 1e-9);
        let c = g.spawn(UKind::Crawler, Side::Machine, 30.5, 15.5);
        assert!((g.units.iter().find(|u| u.id == c).unwrap().power - 0.7).abs() < 1e-9);
        g.nights = 4;
        let t = g.spawn(UKind::Crawler, Side::Machine, 30.5, 16.5);
        assert!((g.units.iter().find(|u| u.id == t).unwrap().power - 1.1).abs() < 1e-9);
        g.nights = 40;
        assert_eq!(g.night_power(), 1.4);
    }

    #[test]
    fn the_frame_is_the_layout_the_page_reads() {
        let mut g = Game::new(31);
        let (nu, nb, nw) = (g.units.len(), g.buildings.len(), g.wrecks.len());
        let f = g.frame().to_vec();
        assert_eq!(f[15] as usize, nu);
        assert_eq!(f[16] as usize, nb);
        assert_eq!(f[17] as usize, nw);
        assert_eq!(f.len(), HEAD + nu * UNIT_F + nb * BLD_F + nw * WRECK_F + f[18] as usize * FX_F + f[19] as usize * 2 + f[24] as usize * CACHE_F);
        assert_eq!(g.tile_bytes().len(), MW * MH);
    }
}

#[cfg(test)]
mod balance {
    use super::*;
    #[test]
    #[ignore]
    fn hands_off() {
        for seed in [1u32, 2, 3] {
            let mut g = Game::new(seed);
            let mut t = 0.0;
            while g.state == State::Playing && t < 900.0 {
                for _ in 0..30 { g.step(TICK); }
                t += 1.0;
                if (t as u32) % 60 == 0 {
                    let m = g.units.iter().filter(|u| u.side == Side::Machine).count();
                    let h = g.units.iter().filter(|u| u.side == Side::Human).count();
                    let haven = g.buildings.iter().find(|b| b.kind == BKind::Haven).map_or(0.0, |b| b.hp);
                    let grid = g.grid.iter().filter(|&&c| c > 0).count();
                    eprintln!("seed {seed} t{t} night {} nights {} humans {h} machines {m} haven {haven:.0} grid {grid} scrap {} surv {}", g.is_night(), g.nights, g.scrap, g.survivors);
                }
            }
            eprintln!("seed {seed} END {:?} at {t}s nights {} score {}", g.state, g.nights, g.score());
        }
    }
}
