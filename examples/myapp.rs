//! End-to-end example for a fictional app, `myapp`.
//!
//! Run it: `cargo run --example myapp` (writes `~/.config/myapp/config.toml` on first run).

use std::process::ExitCode;

use serde::{Deserialize, Serialize};
use settl::{Color, KeyChord, Mods};

/// Top-level config.
///
/// Rule used throughout — *optionality is signaled by a serde default attribute*:
/// - **required**: no serde default attribute → a missing key is a hard error
/// - **optional**: a serde default attribute supplies a missing key — use `#[serde(default)]`
///   for the type's / struct's `Default` or `#[serde(default = "fn")]` for a custom value.
///
/// Notes:
/// - `Option<T>` defaults to `None` rather than erroring even without `#[serde(default)]`.
/// - `#[serde(rename)]` / `rename_all` map fields to other key spellings (e.g. kebab-case in the
///   file instead of Rust's snake_case), including TOML keys that aren't valid Rust identifiers.
/// - `deny_unknown_fields` is recommended to surface an old/misspelled key as a hard error instead
///   of silently ignoring it — it trades cross-version compatibility for catching mistakes.
// `pub` so `tests/template.rs` can include this file as a module and check the template against
// the real struct rather than a copy of it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    /// REQUIRED — the editor command to launch. No default attribute, so a missing key is an error.
    editor_cmd: String,

    /// OPTIONAL, default value is supplied through a custom defined function -> `4`; type's default
    /// would have been `0` which is not what we want.
    #[serde(default = "Config::default_tab_width")]
    tab_width: u8,

    /// OPTIONAL, default value is type's default -> `Auto`; serde parses the string for free.
    /// Note that for enums we can use a shortcut and mark the default value with `#[default]`.
    #[serde(default)]
    theme_mode: ThemeMode,

    /// NESTED `[theme]`.
    #[serde(default)]
    theme: Theme,

    /// NESTED context-scoped keybindings `[keys]` and its sub-tables.
    #[serde(default)]
    keys: Keys,
}

impl Config {
    fn default_tab_width() -> u8 {
        4
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")] // e.g. use 'dark' instead of 'Dark'
enum ThemeMode {
    Dark,
    Light,
    #[default]
    Auto,
}

/// Defaults are plain `0xRRGGBBAA` literals — every `u32` is a valid `Color`, no parsing needed.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)] // use per struct default instead of per field to avoid repetition.
struct Theme {
    base: Color,
    text: Color,
    accent: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            base: Color(0x1e1e2eff),
            text: Color(0xcdd6f4ff),
            accent: Color(0xcba6f7ff),
        }
    }
}

/// A global binding plus one sub-table per UI context. Reusing a chord across contexts is fine; the
/// contexts are checked separately in `main`, so it isn't a false positive.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
struct Keys {
    quit: KeyChord,
    editor: EditorKeys,
    terminal: TerminalKeys,
}

/// Hardcoded default chord. `KeyChord::new` validates, so a typo is a loud panic on the very first
/// run rather than a silent binding that no key event can ever match.
fn chord(mods: Mods, key: &str) -> KeyChord {
    KeyChord::new(mods, key).expect("hardcoded default chord is valid")
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            quit: chord(Mods::CTRL, "q"),
            editor: EditorKeys::default(),
            terminal: TerminalKeys::default(),
        }
    }
}

/// `complete` is multi-value (`Vec<KeyChord>`); the rest are single chords.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
struct EditorKeys {
    save: KeyChord,
    complete: Vec<KeyChord>,
    find: KeyChord,
}

impl Default for EditorKeys {
    fn default() -> Self {
        Self {
            save: chord(Mods::CTRL, "s"),
            complete: vec![chord(Mods::NONE, "Tab"), chord(Mods::CTRL, "Space")],
            find: chord(Mods::CTRL, "f"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
struct TerminalKeys {
    clear: KeyChord,
    /// Same chord as `editor.find`, but a different context — so not a conflict.
    find: KeyChord,
}

impl Default for TerminalKeys {
    fn default() -> Self {
        Self {
            clear: chord(Mods::CTRL, "l"),
            find: chord(Mods::CTRL, "f"),
        }
    }
}

/// Hand-written, commented template written to disk on first run. It documents the defaults for the
/// user; the defaults themselves live in the `Default` impls above (serde fills missing fields).
pub const TEMPLATE: &str = include_str!("config.toml");

fn main() -> ExitCode {
    // Print with `{e}` rather than returning the error from `main`, which would show its `Debug`
    // form: a raw struct dump instead of the path plus toml's line/column snippet.
    let cfg: Config = match settl::load_or_create("myapp", TEMPLATE) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Conflict detection is scoped: check the global bindings together with each context's, so a
    // chord reused across unrelated contexts (here `find`) isn't flagged.
    let global = [("quit", cfg.keys.quit.as_slice())];
    let editor = [
        ("save", cfg.keys.editor.save.as_slice()),
        ("complete", cfg.keys.editor.complete.as_slice()),
        ("find", cfg.keys.editor.find.as_slice()),
    ];
    let terminal = [
        ("clear", cfg.keys.terminal.clear.as_slice()),
        ("find", cfg.keys.terminal.find.as_slice()),
    ];
    for (context, keys) in [("editor", &editor[..]), ("terminal", &terminal[..])] {
        for c in settl::find_conflicts(global.iter().chain(keys).copied()) {
            eprintln!(
                "warning: [{context}] {} is bound to {}",
                c.chord,
                c.actions.join(", ")
            );
        }
    }

    // Print the fully-resolved config: user-supplied values plus the Rust defaults that filled in
    // every omitted key.
    println!("{cfg:#?}");
    ExitCode::SUCCESS
}

// The template-vs-`Config` drift guard lives in `tests/template.rs`, not here: plain `cargo test`
// does not run test modules inside examples, so a `#[cfg(test)]` block in this file would silently
// never execute.
