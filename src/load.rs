//! Loader entry points: path resolution, reading, first-run creation, and serde-driven parsing.
//!
//! This module is the orchestration layer. The actual (de)serialization is delegated to `serde` +
//! the `toml` crate; `settl`'s job here is purely file I/O + path resolution + error wrapping.
//!
//! Flow: [`load_or_create`] is the common entry point. It resolves the path via [`config_path`],
//! and either parses an existing file via [`load_from`] or writes the caller's template and parses
//! that.

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use directories::ProjectDirs;
use serde::de::DeserializeOwned;
use tempfile::NamedTempFile;

use crate::error::{Error, Result};

/// Resolve the default config file path: `<platform-config-dir>/<app>/config.toml`.
///
/// `app` must be a bare application name, not a path — it becomes a directory component. The
/// resolved location is platform-specific:
///
/// | Platform | Path                                                                 |
/// | -------- | -------------------------------------------------------------------- |
/// | Linux    | `$XDG_CONFIG_HOME/<app>/config.toml`, else `~/.config/<app>/config.toml` |
/// | macOS    | `~/Library/Application Support/<app>/config.toml`                    |
/// | Windows  | `%APPDATA%\<app>\config\config.toml`                                 |
///
/// `<app>` is adjusted per platform: Linux lowercases it and removes whitespace (`"My App"` →
/// `myapp`), macOS replaces spaces with `-` (`My-App`), Windows keeps it as given. A lowercase,
/// space-free name gives the same directory name everywhere.
///
/// # Errors
/// [`Error::InvalidAppName`] if `app` is not a single ordinary path component (empty, `.`, `..`,
/// or containing a separator) or has no letter or digit; [`Error::NoConfigDir`] if the platform
/// directory can't be determined (e.g. no `$HOME`).
pub fn config_path(app: &str) -> Result<PathBuf> {
    // Unchecked, `app` is interpolated straight into a filesystem path: `""` silently resolves to
    // a location shared with every other app that does the same, and `".."` escapes the config dir.
    // Require exactly one ordinary component, which also rules out `.`, separators, and roots.
    let mut components = Path::new(app).components();
    let is_bare_name =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    // The component check alone isn't enough, because platforms rewrite the name: Linux strips
    // whitespace, turning `"  "` into `""` and `". ."` into `".."`, and Windows can trim trailing
    // dots and spaces. A letter or digit survives every rewrite.
    let survives_rewriting = app.chars().any(char::is_alphanumeric);
    if !is_bare_name || !survives_rewriting {
        return Err(Error::InvalidAppName {
            app: app.to_owned(),
        });
    }
    let dirs = ProjectDirs::from("", "", app).ok_or_else(|| Error::NoConfigDir {
        app: app.to_owned(),
    })?;
    Ok(dirs.config_dir().join("config.toml"))
}

/// Load and parse config from an explicit path. Errors if the file is missing.
///
/// `T` is the caller's `#[derive(Deserialize)]` config struct.
///
/// # Errors
/// [`Error::Read`] if the file is missing or unreadable; [`Error::Parse`] if the TOML is invalid
/// or a field doesn't match `T`.
pub fn load_from<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<T> {
    let path = path.as_ref();
    // Read the whole file to a string, mapping io::Error -> Error::Read.
    let text = fs::read_to_string(path).map_err(|error| Error::Read {
        path: path.to_path_buf(),
        error,
    })?;

    // Parse with `toml`, mapping toml::de::Error -> Error::Parse.
    toml::from_str(&text).map_err(|error| Error::Parse {
        path: path.to_path_buf(),
        error,
    })
}

/// Load from the app's default path, erroring if the file does not exist.
///
/// # Errors
/// As [`config_path`] and [`load_from`].
pub fn load<T: DeserializeOwned>(app: &str) -> Result<T> {
    let path = config_path(app)?;
    load_from(path)
}

/// Load the app's config, creating it from `template` on first run.
///
/// If the file is missing, the `template` string (a hand-written, commented `config.toml`
/// the caller embeds via `include_str!`) is written verbatim, then parsed and returned. Otherwise
/// the existing file is loaded unchanged. On Unix the created file is mode `0600`.
///
/// # Errors
/// As [`config_path`] and [`load_or_create_from_path`].
pub fn load_or_create<T: DeserializeOwned>(app: &str, template: &str) -> Result<T> {
    let path = config_path(app)?;
    load_or_create_from_path(path, template)
}

/// Load config from an explicit path, creating it from `template` if the file is missing.
///
/// Explicit-path variant of [`load_or_create`]; use when you manage the path yourself (e.g. a
/// path from a CLI flag or environment variable) and don't want XDG resolution.
///
/// # Errors
/// [`Error::Write`] if the parent directories or the file can't be created, plus everything
/// [`load_from`] can return.
pub fn load_or_create_from_path<T: DeserializeOwned>(
    path: impl AsRef<Path>,
    template: &str,
) -> Result<T> {
    let path = path.as_ref();

    // `try_exists`, not `exists`: the latter reports `false` for *any* failed stat (permission
    // denied, symlink loop), which would send us down the create branch and report a misleading
    // write error for a file that is merely unreadable.
    let exists = path.try_exists().map_err(|error| Error::Read {
        path: path.to_path_buf(),
        error,
    })?;
    if !exists {
        write_new(path, template)?;
    }

    load_from(path)
}

/// Create parent directories and write `contents` to `path` for first-run config creation.
///
/// Fills a sibling temp file and links it into place with `persist_noclobber`, which gives both
/// properties first-run creation needs: the config never appears half-written, and an existing one
/// is never overwritten — so there is no check-then-write window in which a config another process
/// just wrote could be clobbered. Losing that race is not an error: the winner wrote the same
/// template, and the caller reads whatever is there next. `tempfile` picks an unpredictable name,
/// creates it with `O_EXCL` and (on Unix) mode `0600`, and deletes it on every error path.
fn write_new(path: &Path, contents: &str) -> Result<()> {
    // `parent()` is `Some("")` for a bare relative filename, which is not a directory `tempfile`
    // can use.
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let failed = |error| Error::Write {
        path: path.to_path_buf(),
        error,
    };

    fs::create_dir_all(parent).map_err(|error| Error::Write {
        path: parent.to_path_buf(),
        error,
    })?;

    let mut tmp = NamedTempFile::new_in(parent).map_err(failed)?;
    tmp.write_all(contents.as_bytes()).map_err(failed)?;
    tmp.as_file().sync_all().map_err(failed)?;

    match tmp.persist_noclobber(path) {
        Ok(_) => Ok(()),
        // Someone else created it first — the outcome we wanted anyway.
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(failed(e.error)),
    }
}
