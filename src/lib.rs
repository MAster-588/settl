#![forbid(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]
// The README doubles as the crate docs, so docs.rs and crates.io show one text and its Rust
// examples run as doc-tests.
#![doc = include_str!("../README.md")]

mod color;
mod de;
mod error;
mod keychord;
mod load;

pub use color::{Color, ParseColorError};
pub use error::{Error, Result};
pub use keychord::{Conflict, KeyChord, Mods, ParseKeyChordError, find_conflicts};
pub use load::{config_path, load, load_from, load_or_create, load_or_create_from_path};
