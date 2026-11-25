//! The shared toolkit every surface draws with: the look (theme, icons,
//! backdrop), the bars every control sits in (bar), rich text (md, ansi), the frameworks' marks, and what differs by
//! device (device, paste, links, pick, shots).

pub(crate) mod ansi;
pub(crate) mod bar;
pub(crate) mod backdrop;
pub(crate) mod device;
pub(crate) mod frameworks;
pub(crate) mod icons;
pub(crate) mod links;
pub(crate) mod md;
pub(crate) mod paste;
pub(crate) mod pick;
pub(crate) mod shots;
pub(crate) mod theme;
