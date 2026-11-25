//! Diffs as data, the same on both sides (operator, 2026-11-25: "keep Git
//! operations, patch parsing, diff computation, rendering, and interaction
//! logic independently partitioned"). No drawing and no git here:
//!
//! - `patch`: unified diff text into files, hunks and lines, and back into a
//!   patch git can apply - a whole file or chosen hunks.
//! - `words`: what changed inside a line, word by word.
//! - `model`: one file's diff laid out once for every view - its rows,
//!   numbered on both sides, its changes, the hunks they make, where
//!   unchanged code folds, and how rows pair side by side or inline.
//!
//! Git runs in host/git.rs; the window that draws this is ui/ide/gitview.

pub(crate) mod model;
pub(crate) mod patch;
pub(crate) mod words;
