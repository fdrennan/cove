//! What a project is, the same on both sides: its `.cove/` layout, what it
//! can start as, and finding in its files. No drawing and no machine access
//! here; the UI and the host both read it.

// Server-side but for TYPES, which the Settings tab's Type menu reads.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) mod dotcove;
pub(crate) mod find;
pub(crate) mod starters;
