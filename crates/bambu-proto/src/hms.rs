use std::fmt;

use serde::{Deserialize, Serialize};

/// A Health Management System code as reported by the printer.
///
/// Kept as the raw reported string. Parsing it into fields is a protocol
/// decision that should be driven by fixtures, not by guesswork.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HmsCode(pub String);

impl fmt::Display for HmsCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How loudly the user should be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HmsSeverity {
    Info,
    Common,
    Serious,
    Fatal,
    /// A severity level this build does not recognise, kept as reported.
    Unknown(u8),
}

/// Resolve an HMS code to the Fluent message id describing it.
///
/// # Why a message id and not a string
///
/// HMS descriptions are user-facing text, so they are translated. This table
/// therefore maps codes to *keys* in `locales/*/hms.ftl`; the presentation
/// layer resolves the key against the user's locale. An English string
/// returned from here would be untranslatable and would leak presentation
/// concerns into the pure protocol crate.
///
/// Returns `None` for codes this build does not know, which the UI should
/// render as the raw code rather than hiding.
///
/// TODO(scaffold): the table is empty. Populate it alongside fixtures that
/// actually exhibit each code.
#[must_use]
pub fn lookup_hms(code: &HmsCode) -> Option<&'static str> {
    let _ = code;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_codes_resolve_to_none_so_the_ui_can_show_the_raw_code() {
        assert_eq!(lookup_hms(&HmsCode("0300_0100_0002_0001".to_owned())), None);
    }
}
