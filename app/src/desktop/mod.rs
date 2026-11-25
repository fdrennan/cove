//! The desktop window's native pieces: the App panel's embedded browser, and
//! the hand-over to the build that carries it. Nothing here compiles for wasm32.

#[cfg(all(target_os = "linux", not(feature = "webview")))]
pub(crate) mod desk;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod webview;
