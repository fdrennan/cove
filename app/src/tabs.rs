//! Projects as a browser's tabs (operator, 2026-11-14: "cove.ndexr.io should
//! act like a browser with domains as tabs, and it should have a full tab
//! menu"). Each open project is a tab along the top; the one in front is the
//! project the IDE is in, and switching tabs is opening that project (ide.rs
//! `open_domain` parks the old one's files and brings the new one's back, so
//! a tab keeps its place). This file is only the list and what the tab menu
//! does to it; ide.rs `tab_strip` draws it and acts.

use serde::{Deserialize, Serialize};

/// How many closed tabs "Reopen closed tab" remembers.
pub const CLOSED_MAX: usize = 10;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tabs {
    /// The open projects' hosts, left to right; the pinned ones first.
    #[serde(default)]
    pub open: Vec<String>,
    /// Pinned: kept at the left, narrow, and not closed by "close others".
    #[serde(default)]
    pub pinned: Vec<String>,
    /// Closed, newest last, for "Reopen closed tab".
    #[serde(default)]
    pub closed: Vec<String>,
    /// Favorites (operator, 2026-11-14: "cove needs ability to add as
    /// favorite and search favorites"): starred projects, open or not, listed
    /// first in Projects and searchable on their own. Newest star last.
    #[serde(default)]
    pub favorites: Vec<String>,
}

impl Tabs {
    pub fn is_pinned(&self, host: &str) -> bool {
        self.pinned.iter().any(|h| h == host)
    }

    /// A project was opened: its tab, if it has none, goes right of the tab
    /// that was in front (`from`), as a link opened from a page does - at the
    /// end when there was none.
    pub fn opened(&mut self, host: &str, from: Option<&str>) {
        if host.is_empty() || self.open.iter().any(|h| h == host) {
            return;
        }
        let at = from.and_then(|f| self.open.iter().position(|h| h == f)).map_or(self.open.len(), |i| i + 1);
        // Never among the pinned unless it is pinned itself.
        let at = at.max(self.pinned_count());
        self.open.insert(at, host.to_owned());
        self.closed.retain(|h| h != host);
    }

    fn pinned_count(&self) -> usize {
        self.open.iter().filter(|h| self.is_pinned(h)).count()
    }

    /// Close `hosts`. Returns the tab to bring forward when the one in front
    /// (`active`) was among them: the next to its right, else to its left;
    /// None when it stays, or when no tab is left.
    pub fn close(&mut self, hosts: &[String], active: Option<&str>) -> Option<String> {
        let at = active.and_then(|a| self.open.iter().position(|h| h == a));
        let closing_active = active.is_some_and(|a| hosts.iter().any(|h| h == a));
        let before = self.open.clone();
        for h in hosts {
            if self.open.iter().any(|x| x == h) {
                self.closed.retain(|x| x != h);
                self.closed.push(h.clone());
            }
        }
        let over = self.closed.len().saturating_sub(CLOSED_MAX);
        self.closed.drain(..over);
        self.open.retain(|h| !hosts.contains(h));
        self.pinned.retain(|h| self.open.contains(h));
        if !closing_active {
            return None;
        }
        let at = at?;
        // The nearest survivor: right first, then left.
        before[at + 1..].iter().find(|h| self.open.contains(h)).or_else(|| before[..at].iter().rev().find(|h| self.open.contains(h))).cloned()
    }

    /// Every tab but `keep` and the pinned ones.
    pub fn others(&self, keep: &str) -> Vec<String> {
        self.open.iter().filter(|h| *h != keep && !self.is_pinned(h)).cloned().collect()
    }

    /// The unpinned tabs right of `of`.
    pub fn right_of(&self, of: &str) -> Vec<String> {
        let Some(i) = self.open.iter().position(|h| h == of) else { return Vec::new() };
        self.open[i + 1..].iter().filter(|h| !self.is_pinned(h)).cloned().collect()
    }

    /// The unpinned tabs left of `of`.
    pub fn left_of(&self, of: &str) -> Vec<String> {
        let Some(i) = self.open.iter().position(|h| h == of) else { return Vec::new() };
        self.open[..i].iter().filter(|h| !self.is_pinned(h)).cloned().collect()
    }

    /// The tab closed last, taken off the list.
    pub fn reopen(&mut self) -> Option<String> {
        self.closed.pop()
    }

    /// Pin or unpin: a pinned tab moves to the end of the pinned ones, an
    /// unpinned one to just after them.
    pub fn toggle_pin(&mut self, host: &str) {
        let Some(i) = self.open.iter().position(|h| h == host) else { return };
        let h = self.open.remove(i);
        if self.is_pinned(host) {
            self.pinned.retain(|p| p != host);
        } else {
            self.pinned.push(h.clone());
        }
        let n = self.pinned_count();
        let at = if self.is_pinned(host) { n } else { n.min(self.open.len()) };
        self.open.insert(at.min(self.open.len()), h);
    }

    /// Move `host` to the start or the end of its group (pinned or not).
    pub fn move_to(&mut self, host: &str, start: bool) {
        let Some(i) = self.open.iter().position(|h| h == host) else { return };
        let h = self.open.remove(i);
        let n = self.pinned_count();
        let at = match (self.is_pinned(host), start) {
            (true, true) => 0,
            (true, false) => n,
            (false, true) => n,
            (false, false) => self.open.len(),
        };
        self.open.insert(at, h);
    }

    /// Drop `host` before the tab now at `at` (`open.len()` = the end), as a
    /// drag lets go (operator, 2026-11-14: "we need to be able to move around
    /// and reorganize the tabs"). It stays in its group: a pinned tab among
    /// the pinned, the rest after them.
    pub fn place(&mut self, host: &str, at: usize) {
        let Some(i) = self.open.iter().position(|h| h == host) else { return };
        let h = self.open.remove(i);
        // Removing it shifted everything after it one to the left.
        let at = if at > i { at - 1 } else { at };
        let n = self.pinned_count();
        let at = if self.is_pinned(host) { at.min(n) } else { at.clamp(n, self.open.len()) };
        self.open.insert(at, h);
    }

    /// One step left (-1) or right (+1) within its group.
    pub fn nudge(&mut self, host: &str, by: isize) {
        let Some(i) = self.open.iter().position(|h| h == host) else { return };
        let j = i as isize + by;
        if j < 0 || j as usize >= self.open.len() || self.is_pinned(host) != self.is_pinned(&self.open[j as usize]) {
            return;
        }
        self.open.swap(i, j as usize);
    }

    /// Keep only the projects that still exist (one archived or no longer
    /// shared closes), leaving the list alone while it is unknown.
    pub fn keep_listed(&mut self, listed: &[String]) {
        if listed.is_empty() {
            return;
        }
        self.open.retain(|h| listed.contains(h));
        self.pinned.retain(|h| self.open.contains(h));
        self.closed.retain(|h| listed.contains(h));
        self.favorites.retain(|h| listed.contains(h));
    }

    pub fn is_favorite(&self, host: &str) -> bool {
        self.favorites.iter().any(|h| h == host)
    }

    /// Star or unstar a project.
    pub fn toggle_favorite(&mut self, host: &str) {
        if self.is_favorite(host) {
            self.favorites.retain(|h| h != host);
        } else if !host.is_empty() {
            self.favorites.push(host.to_owned());
        }
    }

    /// The name on a tab: the host less ".ndexr.io".
    pub fn short(host: &str) -> &str {
        host.strip_suffix(".ndexr.io").or_else(|| host.strip_suffix(LOCAL)).unwrap_or(host)
    }
}

/// The name of a project that lives on this computer only: `<name>.localhost`
/// (RFC 6761 keeps .localhost for the machine itself, so it is never anyone's
/// public domain, yet still a hostname to everything that wants one). A
/// computer with no ndexr behind it names its projects so; deploying one is
/// giving it a public name (operator, 2026-11-15: "if a user doesnt have a
/// domain and has never logged in, they shouldnt have an ndexr.io domain").
pub const LOCAL: &str = ".localhost";

pub fn is_local(host: &str) -> bool {
    host.ends_with(LOCAL)
}

#[cfg(test)]
mod tests {
    use super::Tabs;

    fn tabs(open: &[&str]) -> Tabs {
        Tabs { open: open.iter().map(|s| s.to_string()).collect(), ..Default::default() }
    }
    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_project_opens_right_of_the_tab_in_front() {
        let mut t = tabs(&["a", "b", "c"]);
        t.opened("x", Some("a"));
        assert_eq!(t.open, v(&["a", "x", "b", "c"]));
        t.opened("y", None);
        assert_eq!(t.open, v(&["a", "x", "b", "c", "y"]));
        t.opened("b", Some("y"));
        assert_eq!(t.open.len(), 5, "an open project keeps its one tab");
    }

    #[test]
    fn closing_the_front_tab_brings_its_neighbour() {
        let mut t = tabs(&["a", "b", "c"]);
        assert_eq!(t.close(&v(&["b"]), Some("b")), Some("c".into()), "the one to its right");
        assert_eq!(t.close(&v(&["c"]), Some("c")), Some("a".into()), "else the one to its left");
        assert_eq!(t.close(&v(&["a"]), Some("a")), None, "none left");
        assert_eq!(t.closed, v(&["b", "c", "a"]));
        assert_eq!(t.reopen(), Some("a".into()));
        let mut t = tabs(&["a", "b", "c"]);
        assert_eq!(t.close(&v(&["c"]), Some("a")), None, "another tab closing leaves the front one");
    }

    #[test]
    fn pinned_tabs_stay_left_and_survive_close_others() {
        let mut t = tabs(&["a", "b", "c", "d"]);
        t.toggle_pin("c");
        assert_eq!(t.open, v(&["c", "a", "b", "d"]));
        assert_eq!(t.others("b"), v(&["a", "d"]));
        assert_eq!(t.right_of("a"), v(&["b", "d"]));
        assert_eq!(t.left_of("b"), v(&["a"]));
        t.opened("x", Some("c"));
        assert_eq!(t.open, v(&["c", "x", "a", "b", "d"]), "a new tab never lands among the pinned");
        t.toggle_pin("c");
        assert_eq!(t.open, v(&["c", "x", "a", "b", "d"]));
        assert!(t.pinned.is_empty());
    }

    #[test]
    fn tabs_move_within_their_group() {
        let mut t = tabs(&["a", "b", "c"]);
        t.toggle_pin("a");
        t.move_to("c", true);
        assert_eq!(t.open, v(&["a", "c", "b"]), "to the start of the unpinned");
        t.move_to("c", false);
        assert_eq!(t.open, v(&["a", "b", "c"]));
        t.nudge("b", -1);
        assert_eq!(t.open, v(&["a", "b", "c"]), "not past a pinned tab");
        t.nudge("b", 1);
        assert_eq!(t.open, v(&["a", "c", "b"]));
    }

    #[test]
    fn a_dragged_tab_lands_where_it_is_dropped() {
        let mut t = tabs(&["a", "b", "c", "d"]);
        t.place("a", 3);
        assert_eq!(t.open, v(&["b", "c", "a", "d"]), "dropped before d");
        t.place("d", 0);
        assert_eq!(t.open, v(&["d", "b", "c", "a"]));
        t.place("b", 4);
        assert_eq!(t.open, v(&["d", "c", "a", "b"]), "dropped at the end");
        t.toggle_pin("a");
        assert_eq!(t.open, v(&["a", "d", "c", "b"]));
        t.place("b", 0);
        assert_eq!(t.open, v(&["a", "b", "d", "c"]), "not before a pinned tab");
        t.place("a", 4);
        assert_eq!(t.open, v(&["a", "b", "d", "c"]), "a pinned tab stays with the pinned");
    }

    #[test]
    fn favorites_star_unstar_and_follow_the_projects() {
        let mut t = tabs(&["a"]);
        t.toggle_favorite("b");
        t.toggle_favorite("a");
        assert_eq!(t.favorites, v(&["b", "a"]), "a favorite need not be open");
        assert!(t.is_favorite("b") && !t.is_favorite("c"));
        t.toggle_favorite("b");
        assert_eq!(t.favorites, v(&["a"]));
        t.keep_listed(&v(&["c"]));
        assert!(t.favorites.is_empty(), "a project gone is no longer a favorite");
    }

    #[test]
    fn gone_projects_close_and_closed_is_bounded() {
        let mut t = tabs(&["a", "b"]);
        t.keep_listed(&[]);
        assert_eq!(t.open.len(), 2, "an unknown list changes nothing");
        t.keep_listed(&v(&["b"]));
        assert_eq!(t.open, v(&["b"]));
        let mut t = Tabs::default();
        for i in 0..15 {
            let h = format!("p{i}");
            t.opened(&h, None);
            t.close(&[h], None);
        }
        assert_eq!(t.closed.len(), super::CLOSED_MAX);
        assert_eq!(t.closed.last().map(String::as_str), Some("p14"));
        assert_eq!(Tabs::short("rix.ndexr.io"), "rix");
        assert_eq!(Tabs::short("koodle.io"), "koodle.io");
    }
}
