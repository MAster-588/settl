//! Error type shared by the loader entry points.
//!
//! Parsing of the typed primitives ([`crate::Color`], [`crate::KeyChord`]) has its own small
//! `Parse*Error` types local to those modules; those surface here only indirectly, wrapped by
//! `toml` inside [`Error::Parse`] when they fail during deserialization.

use std::path::PathBuf;

/// Anything that can go wrong while resolving, reading, creating, or parsing a config file.
///
/// Each I/O variant carries the offending path so callers can produce an actionable message
/// without re-deriving it. [`Error::Parse`] wraps the `toml` error verbatim, preserving its
/// line/column information.
///
/// The underlying error is part of the `Display` message and deliberately *not* returned by
/// [`source`](std::error::Error::source): `eprintln!("{e}")` alone is then a complete,
/// user-facing message, and chain-walking reporters (anyhow's `{:#}`) don't print it twice.
/// Match the variant's `error` field to inspect it programmatically.
///
/// Both the enum and its variants are `#[non_exhaustive]`: match with a trailing `_ =>` arm and
/// `..` in struct patterns, and new failure modes can be added without a major version bump.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The application name was not a usable directory name; see [`crate::config_path`].
    #[error("invalid application name {app:?}: expected a bare name with a letter or digit")]
    #[non_exhaustive]
    InvalidAppName {
        /// Application name passed to the loader.
        app: String,
    },

    /// The platform config directory could not be determined (e.g. no `$HOME`).
    #[error("could not determine a config directory for {app:?}")]
    #[non_exhaustive]
    NoConfigDir {
        /// Application name passed to the loader.
        app: String,
    },

    /// Reading the config file failed.
    #[error("reading config at {path}: {error}")]
    #[non_exhaustive]
    Read {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O failure.
        error: std::io::Error,
    },

    /// Writing the file (first-run template, or creating parent dirs) failed.
    #[error("writing config at {path}: {error}")]
    #[non_exhaustive]
    Write {
        /// The file or directory that could not be written.
        path: PathBuf,
        /// The underlying I/O failure.
        error: std::io::Error,
    },

    /// The TOML was syntactically or structurally invalid, or a typed field failed to parse.
    #[error("parsing config at {path}: {error}")]
    #[non_exhaustive]
    Parse {
        /// The file that could not be parsed.
        path: PathBuf,
        /// The `toml` error, carrying line/column information.
        error: toml::de::Error,
    },
}

/// Convenience alias for results returned by this crate's loader functions.
pub type Result<T> = std::result::Result<T, Error>;
