//! [`KeyChord`] and its [`Mods`] modifier set — a keybinding that (de)serializes as a string.
//!
//! Semantics are the *simplified* model from the README: a modifier set plus a lowercase-normalized
//! key token, compared exactly. Modifier order/case is irrelevant (handled at parse time); Shift is
//! an ordinary modifier (`"r"` != `"Shift+r"`); there is no symbol/Shift special-casing and no
//! `Cmd`/`Super` aliasing.
//!
//! Collaborators: used as a field type in consumer config structs. [`find_conflicts`] (below) uses
//! `KeyChord: Hash + Eq` as a canonical identity to detect duplicate bindings.

use std::collections::HashMap;
use std::fmt;
use std::ops::{BitOr, BitOrAssign};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A set of keyboard modifiers, stored as a bitmask. `Copy` and cheap to compare.
///
/// `Default` is [`Mods::NONE`]. `Ord` compares the raw bitmask: an arbitrary but stable total
/// order, provided so [`KeyChord`] can be sorted deterministically.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mods(u8);

impl Mods {
    /// No modifiers.
    pub const NONE: Mods = Mods(0);
    /// Control.
    pub const CTRL: Mods = Mods(1 << 0);
    /// Shift.
    pub const SHIFT: Mods = Mods(1 << 1);
    /// Alt / Option.
    pub const ALT: Mods = Mods(1 << 2);
    /// Meta / Super / Command (single canonical name — no aliasing in the simplified model).
    pub const META: Mods = Mods(1 << 3);

    /// True if `self` contains every bit in `other`.
    pub const fn contains(self, other: Mods) -> bool {
        self.0 & other.0 == other.0
    }

    /// True if no modifiers are set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Map a single textual token to a modifier, case-insensitively. `None` for a non-modifier
    /// token (which the chord parser then treats as the key, or an error if a modifier was
    /// expected).
    fn from_token(token: &str) -> Option<Mods> {
        // Case-insensitive; only the four canonical spellings (no Cmd/Super/Win aliasing).
        match token.to_ascii_lowercase().as_str() {
            "ctrl" => Some(Mods::CTRL),
            "shift" => Some(Mods::SHIFT),
            "alt" => Some(Mods::ALT),
            "meta" => Some(Mods::META),
            _ => None,
        }
    }
}

impl BitOr for Mods {
    type Output = Mods;
    fn bitor(self, rhs: Mods) -> Mods {
        Mods(self.0 | rhs.0)
    }
}

impl BitOrAssign for Mods {
    fn bitor_assign(&mut self, rhs: Mods) {
        self.0 |= rhs.0;
    }
}

/// A canonical keybinding: a [`Mods`] set plus a normalized key token.
///
/// Construct with [`KeyChord::key`] / [`KeyChord::new`], or parse a whole chord string via
/// [`FromStr`]. The key is stored lowercase so case is only expressible through [`Mods::SHIFT`],
/// which makes `Eq`/`Hash` a true canonical identity for conflict detection.
///
/// Every constructor validates, so a `KeyChord` that exists is always one [`FromStr`] accepts:
/// `chord.to_string().parse() == Ok(chord)` holds for all values. `Ord` sorts by modifier bitmask
/// then key — arbitrary but stable, for deterministic reports.
///
/// Serde uses the same string form. `settl` never writes a config back, but `Serialize` lets a
/// config struct holding a `KeyChord` still derive it, e.g. to print the effective config.
///
/// # Matching key events
///
/// `settl` never sees your toolkit's key events: build a `KeyChord` from each one and compare it
/// with `==`, or look it up in a map keyed by the configured chords.
///
/// ```
/// use settl::{KeyChord, Mods};
///
/// // What a toolkit might report: modifier flags plus a key name or character.
/// let (ctrl, shift, key) = (true, false, "Q");
///
/// let mut mods = Mods::NONE;
/// if ctrl {
///     mods |= Mods::CTRL;
/// }
/// if shift {
///     mods |= Mods::SHIFT;
/// }
/// let pressed = KeyChord::new(mods, key)?;
///
/// let quit: KeyChord = "Ctrl+q".parse()?;
/// assert_eq!(pressed, quit);
/// # Ok::<(), settl::ParseKeyChordError>(())
/// ```
///
/// Where the simplified model bites:
///
/// - **Key names are yours to map.** Any single token is a valid key, because toolkits disagree on
///   names (`ArrowUp` vs `Up`). Choose the names your users write, translate every event into them,
///   and check the configured chords' [`key_str`](KeyChord::key_str) against that set at load time:
///   a misspelled `"Ctrl+Entre"` otherwise loads fine and never fires.
/// - **Whitespace keys need a name.** A toolkit reporting Space as the character `' '` must map it
///   to e.g. `"space"`; [`KeyChord::new`] rejects whitespace.
/// - **Shift changes the character.** On a US layout, Shift+1 usually arrives as `!` with Shift
///   held, which becomes `Shift+!` and matches neither `"!"` nor `"Shift+1"`. Either drop Shift
///   when the reported character already reflects it, or tell users to write `"Shift+!"`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyChord {
    mods: Mods,
    key: Box<str>, // normalized (lowercase) key token; see `new`
}

impl KeyChord {
    /// Build a chord from a modifier set and a key, normalizing and validating the key.
    ///
    /// # Errors
    /// Returns [`ParseKeyChordError`] if `key` is empty, is a modifier name (`"ctrl"`, `"shift"`,
    /// …), contains whitespace, or contains a `'+'` while not being the literal `'+'` key. These
    /// are exactly the tokens [`FromStr`] rejects, so an accepted key always round-trips.
    pub fn new(mods: Mods, key: &str) -> Result<Self, ParseKeyChordError> {
        // Canonical key token: trimmed + lowercased, so "K" and "k" collapse (case is only
        // meaningful via SHIFT) and named keys (Space, Enter, Left, F1..) normalize the same way.
        // `to_lowercase` (not `to_ascii_lowercase`) so non-ASCII keys canonicalize too.
        let normalized = key.trim().to_lowercase();

        // Only the token's *shape* is checked, never its name against a known-key list: toolkits
        // disagree on names, so any list would reject keys some consumer needs, and tightening it
        // later would break configs that load today. Spelling is checked where events are mapped.
        let reason = if normalized.is_empty() {
            Some("missing key".to_owned())
        } else if Mods::from_token(&normalized).is_some() {
            Some(format!("{normalized:?} is a modifier, not a key"))
        } else if normalized.split_whitespace().count() != 1 {
            // Catches "Ctrl Q" (space instead of '+') and other multi-word tokens, which would
            // otherwise parse into a binding that no key event can ever match.
            Some(format!("{normalized:?} is not a single key token"))
        } else if normalized != "+" && normalized.contains('+') {
            Some(format!(
                "{normalized:?} contains '+'; use it only to separate modifiers"
            ))
        } else {
            None
        };
        match reason {
            // `input` is the key as given; `FromStr` replaces it with the whole chord string.
            Some(reason) => Err(ParseKeyChordError {
                input: key.to_owned(),
                reason,
            }),
            None => Ok(Self {
                mods,
                key: normalized.into_boxed_str(),
            }),
        }
    }

    /// Build a chord with no modifiers. Shorthand for `KeyChord::new(Mods::NONE, key)`.
    ///
    /// # Errors
    /// As [`KeyChord::new`]. In particular a full chord string (`"Ctrl+q"`) is rejected here —
    /// parse those with [`FromStr`] instead.
    pub fn key(key: &str) -> Result<Self, ParseKeyChordError> {
        Self::new(Mods::NONE, key)
    }

    /// The modifier set.
    pub fn mods(&self) -> Mods {
        self.mods
    }

    /// The normalized key token.
    pub fn key_str(&self) -> &str {
        &self.key
    }

    /// View this single chord as a one-element slice, for passing to [`find_conflicts`].
    pub fn as_slice(&self) -> &[KeyChord] {
        std::slice::from_ref(self)
    }
}

impl FromStr for KeyChord {
    type Err = ParseKeyChordError;

    /// Parse a `"[Mod+]...Key"` string, e.g. `"Ctrl+Shift+k"`.
    ///
    /// Grammar: `(<modifier> '+')* <key>`. Each modifier owns its trailing `'+'` separator, so the
    /// literal `'+'` key needs a doubled tail: `"Ctrl++"` is Ctrl plus the `+` key.
    /// The key is whatever remains after peeling the modifier prefix, and must be a single token
    /// that is not itself a modifier name (so `"Ctrl+Shift"` and `"Ctrl+Shift+"` are both errors).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = |reason: String| ParseKeyChordError {
            input: s.to_owned(),
            reason,
        };
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(err("empty keybinding".to_owned()));
        }

        // Peel `<modifier>+` units from the left; the first non-modifier token ends the prefix and
        // begins the key (which keeps any remaining text for `new` to validate and reject).
        let mut mods = Mods::NONE;
        let mut rest = trimmed;
        while let Some((head, tail)) = rest.split_once('+') {
            match Mods::from_token(head.trim()) {
                Some(m) => {
                    mods |= m;
                    rest = tail;
                }
                None => break,
            }
        }

        // A leftover '+' (other than the literal '+' key) means the token before it was meant to be
        // a modifier but wasn't recognized — a better message than `new`'s generic one.
        let key = rest.trim();
        if key != "+"
            && let Some((unknown, _)) = key.split_once('+')
        {
            return Err(err(format!("unknown modifier {:?}", unknown.trim())));
        }
        // Everything else (empty, whitespace, bare modifier name) is `new`'s job, so the two
        // constructors can never disagree about what a valid key is.
        Self::new(mods, key).map_err(|e| err(e.reason))
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Fixed canonical order (Ctrl, Shift, Alt, Meta) then the key, so output round-trips
        // through FromStr. A '+' key renders as a trailing '+', e.g. "Ctrl++".
        if self.mods.contains(Mods::CTRL) {
            f.write_str("Ctrl+")?;
        }
        if self.mods.contains(Mods::SHIFT) {
            f.write_str("Shift+")?;
        }
        if self.mods.contains(Mods::ALT) {
            f.write_str("Alt+")?;
        }
        if self.mods.contains(Mods::META) {
            f.write_str("Meta+")?;
        }
        f.write_str(&self.key)
    }
}

impl Serialize for KeyChord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Serialize as the canonical string produced by Display.
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for KeyChord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Route through FromStr so the grammar lives only in one place.
        crate::de::parse_str(deserializer, "a keybinding string like \"Ctrl+q\"")
    }
}

/// Error returned when a string can't be parsed as a [`KeyChord`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid keybinding {input:?}: {reason}")]
pub struct ParseKeyChordError {
    /// The string that failed to parse.
    pub input: String,
    /// Human-readable reason (e.g. unknown modifier, empty input).
    pub reason: String,
}

/// One chord bound to more than one action within a single scope.
///
/// Borrows the action names from the caller's input (`'a`), so the names aren't re-allocated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict<'a> {
    /// The chord bound more than once.
    pub chord: KeyChord,
    /// The actions sharing it, in first-seen order. Always length >= 2.
    pub actions: Vec<&'a str>,
}

/// Find every chord bound to two or more actions in the given set.
///
/// Non-fatal: it *returns* duplicates rather than erroring, so the caller decides whether a shared
/// chord is a real problem (log a warning, prompt, ignore). It does **not** detect duplicate TOML
/// *keys* — TOML already rejects those at parse time as [`crate::Error::Parse`].
///
/// What counts as a duplicate: a chord listed twice under the *same* action (e.g. `["+", "+"]`), and
/// a chord shared between the global set and a context (an override), are both reported — it's the
/// caller's call whether either is a hard error or acceptable.
///
/// # Scoping
/// Conflicts are global across whatever you pass in. Keybindings are often context-dependent (the
/// same key means different things in different modes), so to avoid false positives call this once
/// per active context, each time with the global bindings plus that context's:
/// ```
/// # use settl::KeyChord;
/// # let (quit, find, open) = ("Ctrl+q".parse::<KeyChord>()?, "Ctrl+f".parse::<KeyChord>()?, "Ctrl+f".parse::<KeyChord>()?);
/// // A single chord becomes a one-element slice with `KeyChord::as_slice`.
/// let global = [("quit", quit.as_slice())];
/// let viewer = [("find", find.as_slice())];
/// let editor = [("open", open.as_slice())]; // same chord as viewer's `find`
///
/// let in_viewer = settl::find_conflicts(global.iter().chain(&viewer).copied());
/// let in_editor = settl::find_conflicts(global.iter().chain(&editor).copied());
/// // viewer vs editor are never compared, so their overlaps aren't flagged.
/// assert!(in_viewer.is_empty() && in_editor.is_empty());
/// # Ok::<(), settl::ParseKeyChordError>(())
/// ```
pub fn find_conflicts<'a>(
    bindings: impl IntoIterator<Item = (&'a str, &'a [KeyChord])>,
) -> Vec<Conflict<'a>> {
    // 1. Build chord -> [action names] by walking every (name, chords) pair and inserting each
    //    chord. `KeyChord: Hash + Eq` makes equal chords collide. Each action list is in
    //    first-seen order (the map itself is unordered; step 3 handles that).
    let mut by_chord: HashMap<KeyChord, Vec<&'a str>> = HashMap::new();
    for (name, chords) in bindings {
        for chord in chords {
            by_chord.entry(chord.clone()).or_default().push(name);
        }
    }

    // 2. Keep only chords bound >= 2 times and shape them into `Conflict`s. Same-action duplicates
    //    DO count (no dedup), so `["+", "+"]` is reported.
    let mut conflicts: Vec<Conflict<'a>> = by_chord
        .into_iter()
        .filter(|(_, actions)| actions.len() >= 2)
        .map(|(chord, actions)| Conflict { chord, actions })
        .collect();
    // 3. Deterministic output: HashMap iteration order is arbitrary, so sort by the chord's own
    //    `Ord` (modifier bitmask, then key) — a total order, so no ties are left to chance.
    conflicts.sort_by(|a, b| a.chord.cmp(&b.chord));
    conflicts
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn chord(s: &str) -> KeyChord {
        s.parse().unwrap()
    }

    #[test]
    fn modifier_order_and_case_are_irrelevant() {
        // All three spellings denote the same canonical chord.
        assert_eq!(chord("Ctrl+Shift+r"), chord("shift+CTRL+r"));
        assert_eq!(chord("Ctrl+Shift+r"), chord("ctrl+shift+r"));
    }

    #[test]
    fn key_is_normalized_to_lowercase() {
        let c = chord("F");
        assert_eq!(c.key_str(), "f");
        assert_eq!(chord("f"), chord("F"));
    }

    #[test]
    fn shift_is_an_ordinary_modifier() {
        // Per the simplified model, "r" and "Shift+r" are distinct (no symbol/Shift special-casing).
        assert_ne!(chord("r"), chord("Shift+r"));
        assert!(chord("Shift+r").mods().contains(Mods::SHIFT));
    }

    #[test]
    fn plus_can_be_the_key() {
        let c = chord("Ctrl++");
        assert_eq!(c.key_str(), "+");
        assert_eq!(c.mods(), Mods::CTRL);
    }

    #[test]
    fn plus_key_with_multiple_modifiers_needs_doubled_tail() {
        // Each modifier owns its '+', so the literal '+' key trails a second '+'.
        let c = chord("Ctrl+Shift++");
        assert_eq!(c.key_str(), "+");
        assert!(c.mods().contains(Mods::CTRL | Mods::SHIFT));
    }

    #[test]
    fn bare_key_has_no_modifiers() {
        assert!(chord("Tab").mods().is_empty());
    }

    #[test]
    fn rejects_empty() {
        assert!("".parse::<KeyChord>().is_err());
        assert!("   ".parse::<KeyChord>().is_err());
    }

    #[test]
    fn rejects_unknown_modifier() {
        let err = "Hyper+q".parse::<KeyChord>().unwrap_err();
        assert!(err.reason.contains("Hyper"), "got: {}", err.reason);
    }

    #[test]
    fn rejects_multiple_key_tokens() {
        // "f+g" is two key tokens: "f" is not a modifier, so the trailing "+g" is invalid.
        assert!("f+g".parse::<KeyChord>().is_err());
    }

    #[test]
    fn rejects_whitespace_separated_tokens() {
        // A space instead of '+' is a typo that must not become a binding no event can match.
        assert!("Ctrl q".parse::<KeyChord>().is_err());
        assert!("hello world".parse::<KeyChord>().is_err());
        assert!(KeyChord::new(Mods::CTRL, "a b").is_err());
    }

    #[test]
    fn constructors_reject_what_the_parser_rejects() {
        // `new`/`key` must not be able to build a chord that `FromStr` would refuse; otherwise
        // Display/Serialize output would not round-trip. See `roundtrip_or_rejected` below.
        assert!(KeyChord::key("").is_err());
        assert!(KeyChord::key("Ctrl").is_err()); // bare modifier name
        assert!(KeyChord::key("f+g").is_err());
        assert!(KeyChord::key("Ctrl+q").is_err()); // a whole chord: use `parse`, not `key`
        assert!(KeyChord::new(Mods::CTRL, "").is_err());
        assert!(KeyChord::new(Mods::CTRL, "shift").is_err());
    }

    #[test]
    fn key_normalization_is_unicode_aware() {
        // `to_ascii_lowercase` would leave these distinct, breaking canonical identity.
        assert_eq!(KeyChord::key("É").unwrap(), KeyChord::key("é").unwrap());
    }

    #[test]
    fn rejects_chord_ending_in_a_modifier() {
        // No real key once the modifiers are peeled off.
        assert!("Ctrl+Shift".parse::<KeyChord>().is_err());
        assert!("Ctrl+Shift+".parse::<KeyChord>().is_err());
        assert!("Ctrl+".parse::<KeyChord>().is_err());
    }

    #[test]
    fn display_uses_canonical_modifier_order() {
        // Input order is arbitrary; Display always emits Ctrl, Shift, Alt, Meta.
        assert_eq!(
            chord("meta+alt+shift+ctrl+r").to_string(),
            "Ctrl+Shift+Alt+Meta+r"
        );
    }

    #[test]
    fn equal_chords_hash_equally() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(chord("Ctrl+Shift+R"));
        // Different spelling, same canonical identity -> already present.
        assert!(!set.insert(chord("shift+ctrl+r")));
        assert_eq!(set.len(), 1);
    }

    proptest! {
        /// The core invariant, over *arbitrary* keys rather than a curated good-key list: for any
        /// input, `new` either rejects it or produces a chord whose Display re-parses identically.
        /// A narrower generator here would only re-test the keys already known to work.
        #[test]
        fn roundtrip_or_rejected(mod_bits in 0u8..16, key in ".*") {
            if let Ok(chord) = KeyChord::new(Mods(mod_bits), &key) {
                prop_assert_eq!(chord.to_string().parse::<KeyChord>().ok(), Some(chord));
            }
        }

        /// The same invariant from the other direction: anything `FromStr` accepts round-trips.
        #[test]
        fn parse_then_display_roundtrips(s in ".*") {
            if let Ok(chord) = s.parse::<KeyChord>() {
                prop_assert_eq!(chord.to_string().parse::<KeyChord>().ok(), Some(chord));
            }
        }

        /// Realistic chords are actually accepted — guards the generators above from vacuity.
        #[test]
        fn plausible_chords_are_accepted(
            mod_bits in 0u8..16,
            key in "[a-z0-9]|\\+|space|enter|tab|esc|up|down|left|right|f[0-9]",
        ) {
            prop_assert!(KeyChord::new(Mods(mod_bits), &key).is_ok());
        }
    }
}
