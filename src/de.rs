//! Serde glue shared by the string-typed primitives ([`crate::Color`], [`crate::KeyChord`]).

use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use serde::Deserializer;
use serde::de::{Error, Visitor};

/// Deserialize a string through `T`'s [`FromStr`], naming `expected` when the value isn't a string.
///
/// A dedicated visitor rather than `String::deserialize`: that reports serde's generic "expected a
/// string" for a wrong-typed value such as `base = 0x1e1e2e` (a valid TOML integer), instead of the
/// format the user should have written.
pub(crate) fn parse_str<'de, D, T>(deserializer: D, expected: &'static str) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: fmt::Display,
{
    struct FromStrVisitor<T> {
        expected: &'static str,
        target: PhantomData<T>,
    }

    impl<T> Visitor<'_> for FromStrVisitor<T>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        type Value = T;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.expected)
        }

        fn visit_str<E: Error>(self, v: &str) -> Result<T, E> {
            v.parse().map_err(E::custom)
        }
    }

    deserializer.deserialize_str(FromStrVisitor {
        expected,
        target: PhantomData,
    })
}
