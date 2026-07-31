//! Lenient deserializers for Bambu's inconsistently-typed wire values.
//!
//! The same field arrives as `28.5`, `"28.5"` or `""` depending on printer
//! model and firmware; AMS ids are strings on one firmware and integers on
//! another; bitmasks are hex *strings*. A parser that insists on one shape
//! breaks on the next firmware, so these helpers accept anything plausible and
//! answer `None` for anything else — per the crate's parsing discipline,
//! an unreadable value is missing data, not an error.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// A number, a numeric string, or nothing.
pub(crate) fn opt_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(
        Option::<Value>::deserialize(deserializer)?.and_then(|value| match value {
            Value::Number(number) => number.as_f64(),
            Value::String(text) => {
                let text = text.trim();
                if text.is_empty() {
                    None
                } else {
                    text.parse().ok()
                }
            }
            _ => None,
        }),
    )
}

/// An integer, a float that happens to be integral, an integer string, or
/// nothing.
pub(crate) fn opt_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<Value>::deserialize(deserializer)?.and_then(lenient_i64))
}

pub(crate) fn lenient_i64(value: Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|float| float as i64)),
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                None
            } else {
                text.parse::<i64>()
                    .ok()
                    .or_else(|| text.parse::<f64>().ok().map(|float| float as i64))
            }
        }
        _ => None,
    }
}

/// A bitmask that arrives as a hex string (`"b"` = 0b1011) or, on some
/// firmwares, as a plain number.
pub(crate) fn opt_bits<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(
        Option::<Value>::deserialize(deserializer)?.and_then(|value| match value {
            Value::Number(number) => number.as_u64(),
            Value::String(text) => {
                let text = text.trim();
                if text.is_empty() {
                    None
                } else {
                    u64::from_str_radix(text, 16).ok()
                }
            }
            _ => None,
        }),
    )
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Debug, Default, Deserialize, PartialEq)]
    #[serde(default)]
    struct Probe {
        #[serde(deserialize_with = "super::opt_f64")]
        f: Option<f64>,
        #[serde(deserialize_with = "super::opt_i64")]
        i: Option<i64>,
        #[serde(deserialize_with = "super::opt_bits")]
        b: Option<u64>,
    }

    fn probe(json: &str) -> Probe {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn numbers_and_numeric_strings_are_equivalent() {
        assert_eq!(probe(r#"{"f": 28.5}"#).f, Some(28.5));
        assert_eq!(probe(r#"{"f": "28.5"}"#).f, Some(28.5));
        assert_eq!(probe(r#"{"i": 42}"#).i, Some(42));
        assert_eq!(probe(r#"{"i": "42"}"#).i, Some(42));
        // X1 firmware has been seen sending integral floats.
        assert_eq!(probe(r#"{"i": 42.0}"#).i, Some(42));
    }

    #[test]
    fn bitmasks_are_hex_strings_or_numbers() {
        // "b" is 0b1011 — trays 0, 1 and 3 present.
        assert_eq!(probe(r#"{"b": "b"}"#).b, Some(0b1011));
        assert_eq!(probe(r#"{"b": "3"}"#).b, Some(3));
        assert_eq!(probe(r#"{"b": 3}"#).b, Some(3));
    }

    #[test]
    fn garbage_is_missing_data_not_an_error() {
        // The discipline: a value we cannot read is `None`, never a parse
        // failure that takes the whole report down with it.
        assert_eq!(probe(r#"{"f": ""}"#).f, None);
        assert_eq!(probe(r#"{"f": "n/a"}"#).f, None);
        assert_eq!(probe(r#"{"f": null}"#).f, None);
        assert_eq!(probe(r#"{"f": [1]}"#).f, None);
        assert_eq!(probe(r#"{}"#).f, None);
    }
}
