//! Cove's windows: up to four columns, each up to four windows, each
//! window a strip of tabs (operator, 2026-11-18: "we want to be able to go
//! 4x4"; it was 3x3 from 2026-10-19: "instead of 2x2 tab
//! locations want to be able to go up to 3x3 where you can stretch windows to
//! those locations, only 3x3 if have two open and move on to middle for
//! example, should be able to get sucked into existing tab location").
//!
//! The four fixed corners it replaces could only open a window where a corner
//! already was. Here the layout is what is open: columns left to right, each
//! a stack of windows top to bottom, so a tab dropped on the middle of a
//! window joins it, and one dropped on a window's edge opens a column or a
//! window beside it - between two columns, or between two windows - up to
//! three of each. A window left without tabs closes, and its neighbours take
//! its room.
//!
//! Pure data here; app.rs draws it and turns a drag into a `Drop`.

use serde::{Deserialize, Serialize};

use crate::ui::ide::Tab;

/// At most this many columns, and windows in a column.
pub const MAX: usize = 4;

/// A window's place: (column, row).
pub type At = (usize, usize);

/// One window: its tabs, and the one in front.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Win {
    pub tabs: Vec<Tab>,
    pub front: Option<Tab>,
}

impl Win {
    fn of(t: Tab) -> Win {
        Win { tabs: vec![t], front: Some(t) }
    }

    /// The tab shown: the front one, else the first.
    pub fn shown(&self) -> Option<Tab> {
        self.front.filter(|t| self.tabs.contains(t)).or_else(|| self.tabs.first().copied())
    }
}

/// Where a dragged tab lands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drop {
    /// Into the window at (column, row), in front.
    Join(usize, usize),
    /// A new column at this index (0 = the far left).
    Col(usize),
    /// A new window in column `.0`, at row `.1`.
    Row(usize, usize),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grid {
    pub cols: Vec<Vec<Win>>,
    /// Each column's share of the width, and each window's of its column's
    /// height; each list sums to 1.
    pub widths: Vec<f32>,
    pub heights: Vec<Vec<f32>>,
}

impl Default for Grid {
    /// Agents first and in front, then Inputs, then Files (operator,
    /// 2026-10-18: "it should say agents, inputs, files, and it should always
    /// start with agents"); the domain's site, its environment, the stack,
    /// plots, the Viewer and the account on the right.
    fn default() -> Self {
        let left = Win { tabs: vec![Tab::Agents, Tab::Console, Tab::Files], front: Some(Tab::Agents) };
        let right = Win {
            tabs: vec![Tab::App, Tab::Environment, Tab::Install, Tab::Plots, Tab::Viewer, Tab::Account],
            front: Some(Tab::App),
        };
        Grid { cols: vec![vec![left], vec![right]], widths: vec![0.6, 0.4], heights: vec![vec![1.0], vec![1.0]] }
    }
}

impl Grid {
    /// The 2×2 corners a layout was saved in before 2026-10-19 (top-left,
    /// top-right, bottom-left, bottom-right) and the left column's share:
    /// read back as the same windows, so nobody's layout is lost.
    pub fn from_corners(cells: &[Vec<Tab>; 4], front: &[Option<Tab>; 4], split_x: Option<f32>) -> Grid {
        let mut cols = Vec::new();
        for col in [[0, 2], [1, 3]] {
            let wins: Vec<Win> = col.iter().filter(|&&c| !cells[c].is_empty()).map(|&c| Win { tabs: cells[c].clone(), front: front[c] }).collect();
            if !wins.is_empty() {
                cols.push(wins);
            }
        }
        let widths = match cols.len() {
            2 => {
                let x = split_x.unwrap_or(0.6).clamp(0.1, 0.9);
                vec![x, 1.0 - x]
            }
            _ => Vec::new(),
        };
        let mut g = Grid { cols, widths, heights: Vec::new() };
        g.mend();
        g
    }

    pub fn find(&self, t: Tab) -> Option<At> {
        self.cols.iter().enumerate().find_map(|(c, col)| col.iter().position(|w| w.tabs.contains(&t)).map(|r| (c, r)))
    }

    pub fn win(&self, (c, r): At) -> Option<&Win> {
        self.cols.get(c)?.get(r)
    }

    pub fn win_mut(&mut self, (c, r): At) -> Option<&mut Win> {
        self.cols.get_mut(c)?.get_mut(r)
    }

    /// Every window, left to right, top to bottom.
    pub fn windows(&self) -> Vec<At> {
        self.cols.iter().enumerate().flat_map(|(c, col)| (0..col.len()).map(move |r| (c, r))).collect()
    }

    /// `t` in front of its window; `soft` only where a console is in front
    /// (a run swaps Console and Terminal, never covers the agent or a file).
    pub fn bring(&mut self, t: Tab, soft: bool) {
        let Some(at) = self.find(t) else { return };
        let w = self.win_mut(at).expect("found");
        if !soft || matches!(w.shown(), Some(Tab::Console | Tab::Terminal)) {
            w.front = Some(t);
        }
    }

    /// The layout after `t` is dropped at `d`. Its window loses it (and
    /// closes if that was its last tab); the target gains it, in front.
    pub fn dropped(&self, t: Tab, d: Drop) -> Grid {
        let mut g = self.clone();
        let Some(src) = g.find(t) else { return g };
        if d == Drop::Join(src.0, src.1) {
            g.cols[src.0][src.1].front = Some(t);
            return g;
        }
        let w = &mut g.cols[src.0][src.1];
        w.tabs.retain(|x| *x != t);
        if w.front == Some(t) {
            w.front = w.tabs.first().copied();
        }
        // New places get a zero share, sized below; indices are the layout's
        // before anything closes, so nothing has moved yet.
        match d {
            Drop::Join(c, r) => {
                if let Some(w) = g.win_mut((c, r)) {
                    w.tabs.push(t);
                    w.front = Some(t);
                }
            }
            Drop::Col(at) => {
                let at = at.min(g.cols.len());
                g.cols.insert(at, vec![Win::of(t)]);
                g.widths.insert(at.min(g.widths.len()), 0.0);
                g.heights.insert(at.min(g.heights.len()), vec![1.0]);
            }
            Drop::Row(c, at) => {
                if let Some(col) = g.cols.get_mut(c) {
                    let at = at.min(col.len());
                    col.insert(at, Win::of(t));
                    if let Some(h) = g.heights.get_mut(c) {
                        h.insert(at.min(h.len()), 0.0);
                    }
                }
            }
        }
        g.prune();
        g.fix_sizes();
        g
    }

    /// Whether `d` keeps the layout within MAX × MAX.
    pub fn fits(&self, t: Tab, d: Drop) -> bool {
        let g = self.dropped(t, d);
        g.cols.len() <= MAX && g.cols.iter().all(|c| c.len() <= MAX)
    }

    /// Windows without tabs close, and columns without windows; their shares
    /// go with them.
    fn prune(&mut self) {
        self.widths.resize(self.cols.len(), 0.0);
        self.heights.resize(self.cols.len(), Vec::new());
        for c in (0..self.cols.len()).rev() {
            self.heights[c].resize(self.cols[c].len(), 0.0);
            for r in (0..self.cols[c].len()).rev() {
                if self.cols[c][r].tabs.is_empty() {
                    self.cols[c].remove(r);
                    self.heights[c].remove(r);
                }
            }
            if self.cols[c].is_empty() {
                self.cols.remove(c);
                self.widths.remove(c);
                self.heights.remove(c);
            }
        }
    }

    /// Every list as long as what it sizes and summing to 1. A new place (a
    /// zero share) makes its list even: the new window gets an equal part.
    fn fix_sizes(&mut self) {
        fn fix(v: &mut Vec<f32>, n: usize) {
            v.resize(n, 0.0);
            if n == 0 {
                return;
            }
            if v.iter().any(|x| !x.is_finite() || *x <= 0.01) {
                *v = vec![1.0 / n as f32; n];
                return;
            }
            let sum: f32 = v.iter().sum();
            v.iter_mut().for_each(|x| *x /= sum);
        }
        fix(&mut self.widths, self.cols.len());
        self.heights.resize(self.cols.len(), Vec::new());
        for (c, col) in self.cols.iter().enumerate() {
            fix(&mut self.heights[c], col.len());
        }
    }

    /// Each tab once, and every window showing one of its own: a layout saved
    /// by an older build (or a tab added since) is mended, not trusted.
    pub fn mend(&mut self) {
        let mut seen = Vec::new();
        for col in &mut self.cols {
            for w in col.iter_mut() {
                w.tabs.retain(|t| {
                    // A tab that is no longer one (Exec, Data) leaves.
                    let new = !seen.contains(t) && Tab::ALL.contains(t);
                    seen.push(*t);
                    new
                });
            }
        }
        self.prune();
        if self.cols.is_empty() {
            *self = Grid::default();
            return;
        }
        // More than MAX × MAX (a hand-edited store): the extras join the last place.
        while self.cols.len() > MAX {
            let extra = self.cols.pop().unwrap();
            self.widths.pop();
            self.heights.pop();
            let last = self.cols.last_mut().unwrap().last_mut().unwrap();
            last.tabs.extend(extra.into_iter().flat_map(|w| w.tabs));
        }
        for col in &mut self.cols {
            while col.len() > MAX {
                let extra = col.pop().unwrap();
                col.last_mut().unwrap().tabs.extend(extra.tabs);
            }
        }
        // A tab new since the layout was saved goes beside the tab before it
        // in Tab::ALL, where the default has it.
        for (i, t) in Tab::ALL.into_iter().enumerate() {
            if self.find(t).is_some() {
                continue;
            }
            let near = Tab::ALL[..i].iter().rev().find_map(|x| self.find(*x).map(|at| (at, *x)));
            match near {
                Some((at, x)) => {
                    let w = self.win_mut(at).unwrap();
                    let p = w.tabs.iter().position(|y| *y == x).map_or(w.tabs.len(), |p| p + 1);
                    w.tabs.insert(p, t);
                }
                None => self.cols[0][0].tabs.insert(0, t),
            }
        }
        for col in &mut self.cols {
            for w in col.iter_mut() {
                w.front = w.shown();
            }
        }
        self.fix_sizes();
    }

    /// Agents, Inputs, Files, in that order, in the window Inputs is in, with
    /// Agents in front - on every start, whatever a saved layout says: a
    /// layout saved earlier kept opening on Files and CONTEXT.md (operator,
    /// 2026-10-18).
    pub fn agents_first(&mut self) {
        const FIRST: [Tab; 3] = [Tab::Agents, Tab::Console, Tab::Files];
        let home = self.find(Tab::Console).unwrap_or((0, 0));
        for col in &mut self.cols {
            for w in col.iter_mut() {
                w.tabs.retain(|t| !FIRST.contains(t));
            }
        }
        if let Some(w) = self.win_mut(home) {
            w.tabs.splice(0..0, FIRST);
        }
        self.prune();
        self.fix_sizes();
        for col in &mut self.cols {
            for w in col.iter_mut() {
                w.front = w.shown();
            }
        }
        if let Some(at) = self.find(Tab::Agents) {
            self.win_mut(at).unwrap().front = Some(Tab::Agents);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(g: &Grid) -> Vec<Vec<Vec<Tab>>> {
        g.cols.iter().map(|c| c.iter().map(|w| w.tabs.clone()).collect()).collect()
    }

    #[test]
    fn a_tab_dropped_on_a_window_joins_it() {
        let g = Grid::default().dropped(Tab::Plots, Drop::Join(0, 0));
        assert_eq!(g.find(Tab::Plots), Some((0, 0)));
        assert_eq!(g.win((0, 0)).unwrap().front, Some(Tab::Plots));
        assert!(!g.win((1, 0)).unwrap().tabs.contains(&Tab::Plots));
    }

    #[test]
    fn a_middle_column_opens_between_two_and_no_fourth() {
        let g = Grid::default().dropped(Tab::Plots, Drop::Col(1));
        assert_eq!(g.cols.len(), 3);
        assert_eq!(g.find(Tab::Plots), Some((1, 0)));
        assert!(g.widths.iter().all(|w| (*w - 1.0 / 3.0).abs() < 1e-4), "{:?}", g.widths);
        // A fourth column fits now (4×4); a fifth does not.
        let four = g.dropped(Tab::Viewer, Drop::Col(3));
        assert_eq!(four.cols.len(), 4);
        assert!(!four.fits(Tab::Install, Drop::Col(4)), "4 columns at most");
        // The last tab out of a column closes it, and the others take its room.
        let back = g.dropped(Tab::Plots, Drop::Join(0, 0));
        assert_eq!(back.cols.len(), 2);
        assert!((back.widths.iter().sum::<f32>() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn windows_stack_three_high_in_a_column() {
        let g = Grid::default().dropped(Tab::Plots, Drop::Row(1, 1)).dropped(Tab::Viewer, Drop::Row(1, 0));
        assert_eq!(tabs(&g)[1].len(), 3);
        assert_eq!(g.find(Tab::Viewer), Some((1, 0)));
        assert_eq!(g.find(Tab::Plots), Some((1, 2)));
        let four = g.dropped(Tab::Account, Drop::Row(1, 0));
        assert_eq!(tabs(&four)[1].len(), 4, "a fourth window in a column fits");
        assert!(!four.fits(Tab::Install, Drop::Row(1, 0)), "4 windows a column at most");
        // Moving a window's only tab within its own column is not a new window.
        assert!(g.fits(Tab::Plots, Drop::Row(1, 0)));
    }

    #[test]
    fn a_2x2_layout_from_before_reads_back_as_the_same_windows() {
        let cells = [vec![Tab::Agents, Tab::Console, Tab::Files], vec![Tab::App], vec![Tab::Plots], vec![]];
        let front = [Some(Tab::Console), Some(Tab::App), Some(Tab::Plots), None];
        let g = Grid::from_corners(&cells, &front, Some(0.7));
        assert_eq!(g.cols.len(), 2);
        assert_eq!(g.find(Tab::Plots), Some((0, 1)));
        assert_eq!(g.win((0, 0)).unwrap().front, Some(Tab::Console));
        assert!((g.widths[0] - 0.7).abs() < 1e-4);
        // Every tab, once: the ones it lacked joined beside their neighbours.
        for t in Tab::ALL {
            assert!(g.find(t).is_some(), "{t:?}");
        }
    }

    #[test]
    fn agents_first_survives_any_layout() {
        let mut g = Grid::default().dropped(Tab::Agents, Drop::Col(2)).dropped(Tab::Files, Drop::Row(1, 1));
        g.agents_first();
        assert_eq!(g.win(g.find(Tab::Console).unwrap()).unwrap().tabs[..3], [Tab::Agents, Tab::Console, Tab::Files]);
        assert_eq!(g.win(g.find(Tab::Agents).unwrap()).unwrap().front, Some(Tab::Agents));
        assert!(g.cols.iter().all(|c| c.iter().all(|w| !w.tabs.is_empty())));
    }
}
