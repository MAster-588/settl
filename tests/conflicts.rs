//! Integration tests for `find_conflicts`: detection, same-action duplicates, deterministic
//! ordering, and the per-context scoping pattern from the README.

use settl::{KeyChord, Mods, find_conflicts};

fn chord(s: &str) -> KeyChord {
    s.parse().unwrap()
}

#[test]
fn no_conflicts_when_every_chord_is_distinct() {
    let quit = [chord("Ctrl+q")];
    let save = [chord("Ctrl+s")];
    let conflicts = find_conflicts([("quit", &quit[..]), ("save", &save[..])]);
    assert!(conflicts.is_empty());
}

#[test]
fn reports_a_chord_shared_across_actions() {
    let quit = [chord("Ctrl+w")];
    let close = [chord("Ctrl+w")];
    let conflicts = find_conflicts([("quit", &quit[..]), ("close", &close[..])]);

    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].chord, chord("Ctrl+w"));
    // Actions are listed in first-seen order.
    assert_eq!(conflicts[0].actions, ["quit", "close"]);
}

#[test]
fn canonical_identity_collides_across_spellings() {
    // Different spellings of the same chord still count as one conflict.
    let a = [KeyChord::new(Mods::CTRL | Mods::SHIFT, "r").unwrap()];
    let b = [chord("shift+ctrl+R")];
    let conflicts = find_conflicts([("a", &a[..]), ("b", &b[..])]);
    assert_eq!(conflicts.len(), 1);
}

#[test]
fn single_chords_pass_through_as_slice() {
    // `KeyChord::as_slice` is the ergonomic path for the common one-chord-per-action case.
    let quit = chord("Ctrl+q");
    let close = chord("Ctrl+q");
    let conflicts = find_conflicts([("quit", quit.as_slice()), ("close", close.as_slice())]);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].actions, ["quit", "close"]);
}

#[test]
fn same_action_duplicate_is_reported() {
    // A chord listed twice under one action (e.g. ["+", "+"]) is a conflict with itself.
    let complete = [chord("+"), chord("+")];
    let conflicts = find_conflicts([("complete", &complete[..])]);

    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].actions, ["complete", "complete"]);
}

#[test]
fn output_is_sorted_by_canonical_chord() {
    // HashMap iteration order is arbitrary, so the result must be deterministically sorted.
    let z = [chord("Ctrl+z")];
    let z2 = [chord("Ctrl+z")];
    let a = [chord("Ctrl+a")];
    let a2 = [chord("Ctrl+a")];
    let conflicts = find_conflicts([
        ("z1", &z[..]),
        ("z2", &z2[..]),
        ("a1", &a[..]),
        ("a2", &a2[..]),
    ]);

    let chords: Vec<String> = conflicts.iter().map(|c| c.chord.to_string()).collect();
    assert_eq!(chords, ["Ctrl+a", "Ctrl+z"]);
}

#[test]
fn scoping_per_context_avoids_false_positives() {
    // The README pattern: a chord reused across unrelated contexts isn't a conflict because the
    // contexts are checked separately, each against the shared global bindings.
    let quit = [chord("Ctrl+q")];
    let editor_find = [chord("Ctrl+f")];
    let terminal_find = [chord("Ctrl+f")];

    let global = [("quit", &quit[..])];
    let in_editor = find_conflicts(global.iter().copied().chain([("find", &editor_find[..])]));
    let in_terminal = find_conflicts(global.iter().copied().chain([("find", &terminal_find[..])]));

    assert!(in_editor.is_empty(), "editor: {in_editor:?}");
    assert!(in_terminal.is_empty(), "terminal: {in_terminal:?}");
}
