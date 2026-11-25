//! The public face: the front page's slides (showcase), Visualization mode
//! for anyone signed in or not (view, browser only), and where a build machine
//! is (stage), which the slides show.

pub(crate) mod showcase;
pub(crate) mod stage;
#[cfg(target_arch = "wasm32")]
pub(crate) mod view;
