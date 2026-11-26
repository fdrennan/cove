//! Everything Cove draws, compiled for the desktop and for the browser
//! (wasm32) alike. `app` is the frame around it all; each other file is one
//! surface of it, and each directory a toolkit or a surface with parts.

pub(crate) mod kit;
pub(crate) mod app;
pub(crate) mod connect;
pub(crate) mod ide;
pub(crate) mod editor;
pub(crate) mod agent;
pub(crate) mod atlas;
pub(crate) mod software;
pub(crate) mod manage;
pub(crate) mod inbox;
pub(crate) mod recents;
pub(crate) mod setup;
pub(crate) mod tour;
pub(crate) mod release;
pub(crate) mod front;
