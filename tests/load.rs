//! Integration tests for the loader entry points: path resolution, first-run creation, no-clobber,
//! concurrency, and the error variants. Uses real temp dirs (no mocked filesystem).

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use settl::{Error, config_path, load_from, load_or_create_from_path};
use tempfile::tempdir;

#[derive(Debug, Deserialize, PartialEq)]
struct Config {
    name: String,
    #[serde(default)]
    retries: u8,
}

const TEMPLATE: &str = "name = \"from-template\"\n";

#[test]
fn load_from_missing_file_is_a_read_error() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("config.toml");

    let err = load_from::<Config>(&missing).unwrap_err();
    assert!(
        matches!(&err, Error::Read { path, .. } if *path == missing),
        "expected Read error for the missing path, got: {err:?}"
    );
}

#[test]
fn load_or_create_writes_template_then_parses_it() {
    let dir = tempdir().unwrap();
    // Nested path: create_dir_all must materialize the parent on first run.
    let path = dir.path().join("nested").join("config.toml");

    let cfg: Config = load_or_create_from_path(&path, TEMPLATE).unwrap();

    assert_eq!(
        cfg,
        Config {
            name: "from-template".to_owned(),
            retries: 0
        }
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), TEMPLATE);
}

#[test]
fn load_or_create_does_not_clobber_an_existing_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "name = \"user-edited\"\nretries = 5\n").unwrap();

    let cfg: Config = load_or_create_from_path(&path, TEMPLATE).unwrap();

    // The existing file wins; the template is ignored and the file is left byte-for-byte intact.
    assert_eq!(
        cfg,
        Config {
            name: "user-edited".to_owned(),
            retries: 5
        }
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "name = \"user-edited\"\nretries = 5\n"
    );
}

#[test]
fn concurrent_first_run_yields_one_intact_config() {
    // Every thread sees a missing file and races to create it. `persist_noclobber` means exactly
    // one wins and the losers back off, so all of them observe one complete config — never a
    // half-written one, and never an error.
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let results: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..8)
            .map(|_| s.spawn(|| load_or_create_from_path::<Config>(&path, TEMPLATE)))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    for result in results {
        let cfg = result.expect("every racer gets a complete config");
        assert_eq!(cfg.name, "from-template");
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), TEMPLATE);
    // No temp files left behind.
    let strays: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n != "config.toml")
        .collect();
    assert!(strays.is_empty(), "leftover temp files: {strays:?}");
}

// The per-platform tests pin the documented locations exactly: if a `directories` upgrade moved
// them, apps would silently create a fresh default elsewhere and users would lose their config.
// Lowercase and space-free, because `directories` rewrites other names per platform.
const APP: &str = "settl-test-app";

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("HOME is set"))
}

#[cfg(target_os = "linux")]
#[test]
fn config_path_is_under_xdg_config_home_on_linux() {
    // `XDG_CONFIG_HOME` is honored only when absolute, per the XDG spec.
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .unwrap_or_else(|| home().join(".config"));
    assert_eq!(
        config_path(APP).unwrap(),
        config_home.join(APP).join("config.toml")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn config_path_is_under_application_support_on_macos() {
    assert_eq!(
        config_path(APP).unwrap(),
        home()
            .join("Library")
            .join("Application Support")
            .join(APP)
            .join("config.toml")
    );
}

#[cfg(windows)]
#[test]
fn config_path_is_under_roaming_appdata_on_windows() {
    let appdata = PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA is set"));
    assert_eq!(
        config_path(APP).unwrap(),
        appdata.join(APP).join("config").join("config.toml")
    );
}

#[test]
fn config_path_rejects_names_that_are_not_bare() {
    // An empty name silently resolves to a directory shared with every other app that does the
    // same; `..` escapes the config dir entirely. `"  "` and `". ."` get there via Linux's
    // whitespace stripping.
    // Note `\` is only a separator on Windows, so it isn't asserted here.
    for bad in ["", ".", "..", "../evil", "a/b", "  ", ". ."] {
        assert!(
            matches!(config_path(bad), Err(Error::InvalidAppName { .. })),
            "expected InvalidAppName for {bad:?}"
        );
    }
    assert!(config_path("myapp").is_ok());
}

#[test]
fn invalid_toml_is_a_parse_error_carrying_the_path() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "name = \"oops\"\nretries = \"not-a-number\"\n").unwrap();

    let err = load_from::<Config>(&path).unwrap_err();
    assert!(
        matches!(&err, Error::Parse { path: p, .. } if *p == path),
        "expected Parse error carrying the path, got: {err:?}"
    );
}
