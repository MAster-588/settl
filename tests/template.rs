//! Guards `examples/config.toml` against drifting from the `Config` struct in `examples/myapp.rs`:
//! a field renamed/added/removed on one side but not the other (or a `deny_unknown_fields`
//! violation) fails this deserialization.
//!
//! This lives in `tests/` rather than in a `#[cfg(test)]` block inside the example, because plain
//! `cargo test` does not run test modules inside examples — such a test would silently never run.

// Include the example's source so the check runs against the real struct, not a copy of it.
#[allow(dead_code)]
#[path = "../examples/myapp.rs"]
mod myapp;

#[test]
fn template_parses_into_config() {
    let _: myapp::Config = toml::from_str(myapp::TEMPLATE).expect("template is valid");
}
