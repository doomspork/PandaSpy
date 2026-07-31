//! HMS (Health Management System) error resolution.
//!
//! Printers report health events as `{attr, code}` integer pairs. The pair
//! encodes a 16-hex-digit code (`0300_0200_0001_0001`), a severity, and an
//! originating module; the human-readable description lives in a table Bambu
//! publishes.
//!
//! # The table is embedded, not fetched
//!
//! `assets/hms/<lang>.json` is a snapshot of that public table, baked into
//! the binary with `include_str!`. PandaSpy never fetches it at runtime — the
//! app is offline-first, and "your printer's error text arrives without any
//! network call leaving your LAN" is a privacy commitment, not an
//! optimisation. Refreshing the snapshot is a build-time act: re-run the
//! fetch, read the diff, commit it.
//!
//! Codes missing from the snapshot resolve to `None`; the UI shows the raw
//! code and the wiki URL rather than hiding the event.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::de;

/// One entry from a report's `hms` array, exactly as the printer sent it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HmsEntry {
    #[serde(deserialize_with = "de::opt_i64")]
    pub attr: Option<i64>,
    #[serde(deserialize_with = "de::opt_i64")]
    pub code: Option<i64>,
}

impl HmsEntry {
    fn attr_u32(&self) -> Option<u32> {
        self.attr.and_then(|value| u32::try_from(value).ok())
    }

    fn code_u32(&self) -> Option<u32> {
        self.code.and_then(|value| u32::try_from(value).ok())
    }

    /// The code as Bambu displays it: `0300_0200_0001_0001`.
    #[must_use]
    pub fn display_code(&self) -> Option<String> {
        let (attr, code) = (self.attr_u32()?, self.code_u32()?);
        Some(format!(
            "{:04X}_{:04X}_{:04X}_{:04X}",
            attr >> 16,
            attr & 0xffff,
            code >> 16,
            code & 0xffff
        ))
    }

    /// The code in the table's key format: 16 lowercase hex digits.
    #[must_use]
    pub fn ecode(&self) -> Option<String> {
        let (attr, code) = (self.attr_u32()?, self.code_u32()?);
        Some(format!("{attr:08x}{code:08x}"))
    }

    /// Severity, encoded in the upper half of `code`.
    ///
    /// The `1=fatal / 2=serious / 3=common / 4=info` mapping is the
    /// community-documented one, and the embedded table corroborates it: the
    /// severity field of every published code is 1, 2 or 3.
    #[must_use]
    pub fn severity(&self) -> Option<HmsSeverity> {
        Some(HmsSeverity::from_wire((self.code_u32()? >> 16) & 0xffff))
    }

    /// Originating module, encoded in the top byte of `attr`.
    #[must_use]
    pub fn module(&self) -> Option<HmsModule> {
        Some(HmsModule::from_wire((self.attr_u32()? >> 24) as u8))
    }

    /// Bambu's public wiki page for this code — the escape hatch for codes
    /// the embedded table cannot describe.
    #[must_use]
    pub fn wiki_url(&self) -> Option<String> {
        Some(format!(
            "https://wiki.bambulab.com/en/x1/troubleshooting/hmscode/{}",
            self.display_code()?
        ))
    }

    /// Human-readable description from the embedded table, in the requested
    /// language, falling back to English, then to `None`.
    #[must_use]
    pub fn describe(&self, lang: &str) -> Option<&'static str> {
        table().describe_hms(&self.ecode()?, lang)
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
    /// A severity level this build does not recognise, preserved raw.
    Unknown(u32),
}

impl HmsSeverity {
    #[must_use]
    pub fn from_wire(raw: u32) -> Self {
        match raw {
            1 => Self::Fatal,
            2 => Self::Serious,
            3 => Self::Common,
            4 => Self::Info,
            other => Self::Unknown(other),
        }
    }
}

/// Which subsystem raised the event.
///
/// TODO(fixture): the byte values are community-documented; confirm against
/// captures as they arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HmsModule {
    MotionController,
    Mainboard,
    Ams,
    Toolhead,
    Xcam,
    Unknown(u8),
}

impl HmsModule {
    #[must_use]
    pub fn from_wire(raw: u8) -> Self {
        match raw {
            0x03 => Self::MotionController,
            0x05 => Self::Mainboard,
            0x07 => Self::Ams,
            0x08 => Self::Toolhead,
            0x0c => Self::Xcam,
            other => Self::Unknown(other),
        }
    }
}

/// The embedded description tables, one per language.
///
/// Parsed lazily on first use — `LazyLock` is initialisation order, not I/O,
/// so the crate stays pure and wasm-buildable.
pub struct HmsTable {
    languages: BTreeMap<&'static str, LangTable>,
}

impl std::fmt::Debug for HmsTable {
    /// Thousands of entries are not a debug representation; the shape is.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut map = f.debug_map();
        for (lang, table) in &self.languages {
            map.entry(
                lang,
                &format_args!("{} hms, {} errors", table.hms.len(), table.errors.len()),
            );
        }
        map.finish()
    }
}

#[derive(Deserialize)]
struct LangTable {
    /// 16-hex-digit HMS ecode → description.
    hms: BTreeMap<String, String>,
    /// 8-hex-digit `print_error` ecode → description.
    errors: BTreeMap<String, String>,
}

/// Snapshot files. Adding a language is adding a line here plus the asset —
/// the table is *data shipped with the binary*, unlike `locales/`, which is
/// UI copy and stays code-free to extend.
static EMBEDDED: &[(&str, &str)] = &[
    ("en", include_str!("../assets/hms/en.json")),
    ("pl", include_str!("../assets/hms/pl.json")),
];

static TABLE: LazyLock<HmsTable> = LazyLock::new(|| {
    let mut languages = BTreeMap::new();
    for (lang, raw) in EMBEDDED {
        match serde_json::from_str::<LangTable>(raw) {
            Ok(parsed) => {
                languages.insert(*lang, parsed);
            }
            Err(error) => {
                // Unreachable for a well-formed asset; the tests parse every
                // embedded file. Debug builds fail loudly, release builds
                // degrade to "code shown without description".
                debug_assert!(false, "embedded HMS table {lang} is malformed: {error}");
            }
        }
    }
    HmsTable { languages }
});

/// The process-wide table.
#[must_use]
pub fn table() -> &'static HmsTable {
    &TABLE
}

impl HmsTable {
    /// Languages with an embedded snapshot.
    #[must_use]
    pub fn languages(&self) -> Vec<&'static str> {
        self.languages.keys().copied().collect()
    }

    /// Describe an HMS ecode (16 hex digits; underscores and case are
    /// forgiven). Falls back to English before giving up.
    ///
    /// Borrows from the table — which, via [`table`], is `'static`.
    #[must_use]
    pub fn describe_hms(&self, ecode: &str, lang: &str) -> Option<&str> {
        self.lookup(ecode, lang, |table| &table.hms)
    }

    /// Describe a `print_error` value from the device-error table.
    #[must_use]
    pub fn describe_print_error(&self, print_error: i64, lang: &str) -> Option<&str> {
        let code = u32::try_from(print_error).ok()?;
        if code == 0 {
            return None;
        }
        self.lookup(&format!("{code:08x}"), lang, |table| &table.errors)
    }

    fn lookup<'table>(
        &'table self,
        ecode: &str,
        lang: &str,
        section: impl Fn(&'table LangTable) -> &'table BTreeMap<String, String>,
    ) -> Option<&'table str> {
        let key: String = ecode
            .chars()
            .filter(|c| *c != '_')
            .map(|c| c.to_ascii_lowercase())
            .collect();

        // "pl-PL" → "pl". BCP-47 region tags do not vary the table.
        let primary = lang
            .split(['-', '_'])
            .next()
            .unwrap_or(lang)
            .to_ascii_lowercase();

        for candidate in [primary.as_str(), "en"] {
            if let Some(text) = self
                .languages
                .get(candidate)
                .and_then(|table| section(table).get(&key))
            {
                return Some(text.as_str());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// attr/code for `0300_0200_0001_0001` — nozzle heater short circuit, a
    /// code present in both embedded snapshots.
    fn known_entry() -> HmsEntry {
        HmsEntry {
            attr: Some(0x0300_0200),
            code: Some(0x0001_0001),
        }
    }

    #[test]
    fn codes_format_the_way_bambu_displays_them() {
        let entry = known_entry();
        assert_eq!(entry.display_code().as_deref(), Some("0300_0200_0001_0001"));
        assert_eq!(entry.ecode().as_deref(), Some("0300020000010001"));
        assert_eq!(
            entry.wiki_url().as_deref(),
            Some("https://wiki.bambulab.com/en/x1/troubleshooting/hmscode/0300_0200_0001_0001")
        );
    }

    #[test]
    fn severity_and_module_decode_from_the_documented_bits() {
        let entry = known_entry();
        assert_eq!(entry.severity(), Some(HmsSeverity::Fatal));
        assert_eq!(entry.module(), Some(HmsModule::MotionController));

        let unknown = HmsEntry {
            attr: Some(0x7f00_0000),
            code: Some(0x0009_0000),
        };
        assert_eq!(unknown.severity(), Some(HmsSeverity::Unknown(9)));
        assert_eq!(unknown.module(), Some(HmsModule::Unknown(0x7f)));
    }

    #[test]
    fn the_embedded_tables_parse_and_are_populated() {
        let table = table();
        assert_eq!(table.languages(), vec!["en", "pl"]);
        // Sizes at snapshot time; shrinkage on refresh deserves a question.
        assert!(table.describe_hms("0300020000010001", "en").is_some());
    }

    #[test]
    fn descriptions_resolve_per_language_with_english_fallback() {
        let entry = known_entry();

        let english = entry.describe("en-US").unwrap();
        assert!(english.contains("nozzle temperature"), "{english}");

        let polish = entry.describe("pl-PL").unwrap();
        assert!(polish.contains("dyszy"), "{polish}");

        // A language with no snapshot falls back to English, not to nothing.
        assert_eq!(entry.describe("de-DE"), Some(english));
    }

    #[test]
    fn lookup_forgives_underscores_and_case() {
        let table = table();
        assert_eq!(
            table.describe_hms("0300_0200_0001_0001", "en"),
            table.describe_hms("0300020000010001", "EN"),
        );
    }

    #[test]
    fn print_errors_resolve_from_the_device_error_table() {
        // 0x03004000: "Z axis homing failed; the task has been stopped."
        let text = table().describe_print_error(0x0300_4000, "en").unwrap();
        assert!(text.contains("homing failed"), "{text}");

        // Zero is "no error", not a table miss worth describing.
        assert_eq!(table().describe_print_error(0, "en"), None);
    }

    #[test]
    fn an_unknown_code_resolves_to_none_so_the_ui_shows_the_raw_code() {
        let entry = HmsEntry {
            attr: Some(0x7fff_ff00),
            code: Some(0x0001_ffff),
        };
        assert_eq!(entry.describe("en"), None);
        // …but the display code and wiki URL still work.
        assert!(entry.display_code().is_some());
        assert!(entry.wiki_url().is_some());
    }
}
