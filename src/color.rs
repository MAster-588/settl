//! [`Color`] — an RGBA newtype that (de)serializes as a hex string.
//!
//! A `Color` is just a packed `0xrrggbbaa` value: every `u32` is a valid color, so the only
//! validation is parsing the hex *string* form. The serde impls route through [`Display`] and
//! [`FromStr`], so the hex grammar lives in exactly one place.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An RGBA color packed as `0xrrggbbaa`.
///
/// Any `u32` is valid, so construct it directly from a literal for defaults — `Color(0x1e1e2eff)` —
/// and read the raw value through the public field. Parsing only matters for the *string* form:
/// it accepts `#rrggbb` (alpha `0xFF`) and `#rrggbbaa`, and `Display` emits `#rrggbb` when opaque,
/// otherwise `#rrggbbaa`.
///
/// `Default` is `Color(0)` — transparent black — so consumer structs can use `#[serde(default)]`
/// without hand-writing a `Default` impl. Prefer an explicit literal for a real theme default.
///
/// Serde uses the same hex string form. `settl` never writes a config back, but `Serialize` lets a
/// config struct holding a `Color` still derive it, e.g. to print the effective config.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Color(pub u32);

impl FromStr for Color {
    type Err = ParseColorError;

    /// Parse `#rrggbb` / `#rrggbbaa` (case-insensitive) into a packed `0xrrggbbaa` color.
    ///
    /// Surrounding whitespace is trimmed, matching [`crate::KeyChord`]'s parser.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || ParseColorError {
            input: s.to_owned(),
        };
        let hex = s.trim().strip_prefix('#').ok_or_else(invalid)?;
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let rgba = match hex.len() {
            // RGB: parse the 3 bytes, then shift in an opaque alpha.
            6 => (u32::from_str_radix(hex, 16).map_err(|_| invalid())? << 8) | 0xFF,
            8 => u32::from_str_radix(hex, 16).map_err(|_| invalid())?,
            _ => return Err(invalid()),
        };
        Ok(Color(rgba))
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Emit `#rrggbb` when fully opaque, else `#rrggbbaa`, lowercase and zero-padded.
        let [r, g, b, a] = self.0.to_be_bytes();
        if a == 0xFF {
            write!(f, "#{r:02x}{g:02x}{b:02x}")
        } else {
            write!(f, "#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Serialize as the canonical hex string produced by Display.
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Route through FromStr so the grammar isn't duplicated.
        crate::de::parse_str(
            deserializer,
            "a hex color string like \"#rrggbb\" or \"#rrggbbaa\"",
        )
    }
}

/// Error returned when a string can't be parsed as a [`Color`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid color {input:?}: expected #rrggbb or #rrggbbaa")]
pub struct ParseColorError {
    /// The string that failed to parse.
    pub input: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn rrggbb_gets_opaque_alpha() {
        // 6-digit form fills alpha with 0xFF.
        assert_eq!("#1e1e2e".parse::<Color>().unwrap(), Color(0x1e1e2eff));
    }

    #[test]
    fn rrggbbaa_keeps_explicit_alpha() {
        assert_eq!("#1e1e2e80".parse::<Color>().unwrap(), Color(0x1e1e2e80));
    }

    #[test]
    fn parsing_is_case_insensitive() {
        assert_eq!(
            "#AABBCC".parse::<Color>().unwrap(),
            "#aabbcc".parse::<Color>().unwrap()
        );
    }

    #[test]
    fn display_omits_alpha_when_opaque() {
        assert_eq!(Color(0x1e1e2eff).to_string(), "#1e1e2e");
    }

    #[test]
    fn display_includes_alpha_when_translucent() {
        assert_eq!(Color(0x1e1e2e80).to_string(), "#1e1e2e80");
    }

    #[test]
    fn rejects_missing_hash() {
        let err = "1e1e2e".parse::<Color>().unwrap_err();
        assert_eq!(err.input, "1e1e2e");
    }

    #[test]
    fn rejects_wrong_length() {
        // 3 and 4 digit shorthand are not supported; only 6 or 8.
        assert!("#fff".parse::<Color>().is_err());
        assert!("#1e1e2".parse::<Color>().is_err());
        assert!("#1e1e2e8".parse::<Color>().is_err());
    }

    #[test]
    fn rejects_non_hex_digits() {
        assert!("#gggggg".parse::<Color>().is_err());
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        // Matches KeyChord::from_str, which trims too.
        assert_eq!("  #1e1e2e\t".parse::<Color>().unwrap(), Color(0x1e1e2eff));
    }

    #[test]
    fn default_is_transparent_black() {
        assert_eq!(Color::default(), Color(0));
    }

    #[test]
    fn deserializes_from_toml_string() {
        let v: Color = toml::from_str::<toml::Value>("c = \"#cba6f7\"")
            .and_then(|t| t["c"].clone().try_into())
            .unwrap();
        assert_eq!(v, Color(0xcba6f7ff));
    }

    #[test]
    fn serializes_to_canonical_hex_string() {
        // Round-trips through TOML as the Display string, dropping a fully-opaque alpha.
        let toml = toml::to_string(&Wrap {
            c: Color(0xcba6f7ff),
        })
        .unwrap();
        assert_eq!(toml.trim(), r##"c = "#cba6f7""##);
    }

    #[derive(Serialize)]
    struct Wrap {
        c: Color,
    }

    proptest! {
        /// Every u32 is a valid color, so Display -> FromStr must reproduce it exactly.
        #[test]
        fn display_then_parse_roundtrips(raw: u32) {
            let color = Color(raw);
            prop_assert_eq!(color.to_string().parse::<Color>().unwrap(), color);
        }
    }
}
