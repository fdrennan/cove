//! The editor's knowledge of what is being written: languages by file
//! (syntax), completion (edcomplete) and the keys that depend on the language
//! (editkeys).

pub(crate) mod edcomplete;
pub(crate) mod editkeys;
pub(crate) mod syntax;
