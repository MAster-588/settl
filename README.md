# settl

A small TOML config loader for Rust apps. You define config as a normal `serde` struct; `settl` adds the two things serde doesn't give you — typed `Color` and `KeyChord` primitives — plus thin helpers to create the file on first run, load it, and detect keybinding conflicts.

## Requirements

- Rust 1.88+ (edition 2024).

## How it works

### Step 1: Define your config

Define your config in rust as an ordinary `serde` struct:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
// Note that you can use normal serde rename features
// Also `deny_unknown_fields` is recommended to surface an outdated/misspelled field as a hard error instead of silently ignoring it.
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Config {
    // required field has no serde default value -> error if missing in the config file
    required_field: String,

    // optional field with serde custom default value -> `custom_default_value()` is used if missing in the config file
    #[serde(default = "custom_default_value")]
    optional_field1: u8,

    // optional field with serde type default value -> u8::Default = `0` is used if missing in the config file
    #[serde(default)]
    optional_field2: u8,
}

fn custom_default_value() -> u8 {
    42
}
```

### Step 2: Create the initial config file

Create a `config.toml` template file with comments explaining each field. This file will be used to generate the initial config file on first run. It should match your rust `Config` default values.

```toml
# <myapp> configuration.
# Written on first run to match rust default values; then left for the user to edit.
# Any invalid edit will result in an error while any missing non-required field will be filled in with the rust default value.

# REQUIRED — Must be explicitly set by the user. Loading fails with "missing field" if absent. Rare use case.
# required-field =

# Optional. If absent, the rust default value is used.
optional-field1 = 42

# Optional. If absent, the rust default value is used.
optional-field2 = 0
```

Note that you can also comment out the optional fields, the `Config` default value will still be used for them.
The difference of setting it directly to the `Config` default is a more intuitive experience for the user and less surprise if the default value changes in the future.

### Step 3: Load the config file

In your app, include the initial config file template and load the actual config file (see [Config location](#config-location)). If the config file does not exist, the template will be used first to create it.

<!-- `ignore`: needs Step 1's `Config` and a real `config.toml`; `examples/myapp.rs` compiles the same flow. -->

```rust,ignore
use std::process::ExitCode;

// Documented config file written verbatim on first run.
// Should match the rust default values in the Config struct.
const TEMPLATE: &str = include_str!("config.toml");

fn main() -> ExitCode {
    // Load the config file from the default path, creating it from TEMPLATE if missing.
    let cfg: Config = match settl::load_or_create("<myapp>", TEMPLATE) {
        Ok(cfg) => cfg,
        Err(e) => {
            // `{e}` prints the path plus toml's line/column snippet. Returning the error from
            // `main` instead would print its `Debug` form: a raw struct dump.
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("{cfg:#?}");
    ExitCode::SUCCESS
}
```

## Limitations

- **Limited forward compatibility** — when you later add a field to the config, the user does not see it (unless they regenerate the default config) and its Rust default will be used. Also with `#[serde(deny_unknown_fields)]` (recommended above) you are sacrificing cross-version compatibility in the case you remove/rename a config option. This is arguably better than silently ignoring a field, but it is a tradeoff to consider.

- **No write-back** — `settl` never serializes a config struct back over the user's file; that would erase comments. In-place edits would need `toml_edit`. Maybe a future feature, but for now the expectation is that users edit the file by hand.

## Usage

A complete end-to-end example — required vs. optional (Rust-defaulted) fields, enum field, nested tables, multi-value keybindings, and context-scoped conflict detection — lives in [`examples/myapp.rs`](https://github.com/MAster-588/settl/blob/main/examples/myapp.rs) and [`examples/config.toml`](https://github.com/MAster-588/settl/blob/main/examples/config.toml). Run it with:

```sh
cargo run --example myapp # (writes the config file on first run!)
```

## Config location

`config_path(app)` resolves the platform config directory (via `directories`), so the default is not
`~/.config` everywhere:

| Platform | Path                                                                     |
| -------- | ------------------------------------------------------------------------ |
| Linux    | `$XDG_CONFIG_HOME/<app>/config.toml`, else `~/.config/<app>/config.toml` |
| macOS    | `~/Library/Application Support/<app>/config.toml`                        |
| Windows  | `%APPDATA%\<app>\config\config.toml`                                     |

`<app>` is adjusted per platform: Linux lowercases it and removes whitespace (`"My App"` → `myapp`),
macOS replaces spaces with `-` (`My-App`), Windows keeps it as given. A lowercase, space-free name
gives the same directory name everywhere.

`app` must be a bare application name — a single path component, not a path — with at least one
letter or digit. Use `load_from` / `load_or_create_from_path` when you want to choose the location
yourself. On Unix the file is created with mode `0600`.

## API

| Item                                                              | Purpose                                                                    |
| ----------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `load<T>(app) -> Result<T, Error>`                                | Load from the default config path; error if missing.                       |
| `load_from<T>(path) -> Result<T, Error>`                          | Explicit-path variant of `load`.                                           |
| `load_or_create<T>(app, template) -> Result<T, Error>`            | Load from the default config path, creating it from `template` if missing. |
| `load_or_create_from_path<T>(path, template) -> Result<T, Error>` | Explicit-path variant of `load_or_create`.                                 |
| `config_path(app) -> Result<PathBuf, Error>`                      | Resolve the default config path.                                           |
| `find_conflicts(iter) -> Vec<Conflict>`                           | Duplicate-chord detection over `(name, &[KeyChord])` pairs.                |

`Error` is a `thiserror` enum: invalid app name, missing config dir, read/write I/O (with path), and
parse (with path and the `toml` line-numbered message). Its `Display` is the complete message, so
`eprintln!("{e}")` is all a user needs. It is `#[non_exhaustive]`, so match it with a trailing `_ =>`
arm.

### `Color`

A validated container for a color packed as `0xRRGGBBAA u32`. Every `u32` is a valid color, so validation only applies to the string form — the whole type is one `FromStr` + one `Display` + serde glue. `FromStr` accepts `#RRGGBB` (alpha `0xFF`) and `#RRGGBBAA`; `Display` emits `#rrggbb` when opaque, else `#rrggbbaa`.

Defaults are plain literals — `Color(0x1e1e2eff)` — needing no parse or `.unwrap()`, and the raw value is just `color.0`. `Default` is `Color(0)` (transparent black).

### `KeyChord`

A simple modifier set (bitset) plus a single key token:

- Modifiers `Ctrl`, `Shift`, `Alt`, `Meta` — case-insensitive, order-independent, separated by `+`.
- Key token `0`, `r`, `F`, `=`, `Space`, `Enter`, `Tab`, `Esc`, `Up`, `Down`, `Left`, `Right`, etc. — case-insensitive, lowercased when stored. Any single token that isn't a modifier name is accepted; it just has to contain no whitespace and no `+` (unless it _is_ `+`). `settl` can't know your toolkit's key names, so it doesn't police the spelling beyond that — see [matching key events](https://docs.rs/settl/latest/settl/struct.KeyChord.html#matching-key-events) for turning your toolkit's events into chords and catching misspelled keys.

Examples:

- `Ctrl+Shift+R`, `ctrl+shift+r`, `shift+CTRL+r` all parse to the same canonical `KeyChord`.
- `Ctrl+Shift+R` and `Ctrl+R` are distinct chords.
- `Ctrl++` is valid (modifier + `+` key), but `Ctrl+Shift+` or `Ctrl+Shift` are not (no key token).
- `f` and `F` are (the same) valid chord; `f+g` is not (multiple key tokens), and neither is `Ctrl q` (space instead of `+`).
- `KeyChord` implements `Eq`/`Hash` by canonical identity, so you can reliably detect duplicates in a `HashSet` or `HashMap`.

Construction is validated, so a `KeyChord` that exists always round-trips through its string form:

```rust
use settl::{KeyChord, Mods};

fn demo() -> Result<(), settl::ParseKeyChordError> {
    // Parse a whole chord string (config files, CLI flags):
    let quit: KeyChord = "Ctrl+q".parse()?;

    // Or build one directly, for hardcoded defaults. Both validate, so a typo is caught here
    // rather than becoming a binding that silently never fires:
    let save = KeyChord::new(Mods::CTRL, "s")?;
    assert!(KeyChord::key("Ctrl+q").is_err()); // a whole chord — use `parse` for that
    assert!(KeyChord::key("Ctrl").is_err()); // a modifier is not a key

    // `as_slice` adapts a single chord where a slice is wanted (e.g. `find_conflicts`):
    let bindings = [("quit", quit.as_slice()), ("save", save.as_slice())];
    assert!(settl::find_conflicts(bindings).is_empty());
    Ok(())
}
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/MAster-588/settl/blob/main/LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/MAster-588/settl/blob/main/LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
