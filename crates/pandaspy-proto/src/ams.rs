//! The AMS (Automatic Material System) model.
//!
//! One printer can carry several units of different kinds — the standard
//! 4-slot AMS, the open-frame AMS Lite, the AMS 2 Pro with active drying, and
//! the single-slot AMS HT. The wire shape is one `ams` object holding a list
//! of units, each holding a list of trays, plus a handful of printer-global
//! bitmasks and indices.
//!
//! Two rules this module enforces:
//!
//! * **Empty slots are positions, not absences.** A unit with filament in
//!   slots 0, 1 and 3 reports four trays, the third one bare. The `Vec<Tray>`
//!   preserves that ordering; nothing compacts it.
//! * **Indices are preserved raw and interpreted lazily.** `tray_now` and the
//!   `*_bits` masks are kept as the printer sent them; decoding into
//!   unit/slot coordinates happens in accessors that can say "I don't know".

use serde::{Deserialize, Serialize};

use crate::de;

/// Global tray index meaning "the external spool holder" in `tray_now`.
pub const TRAY_EXTERNAL: i64 = 254;

/// Global tray index meaning "no tray selected" in `tray_now`.
pub const TRAY_NONE: i64 = 255;

/// The whole material system as one printer reports it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AmsSystem {
    /// The units, in wire order. May legitimately be empty (A1 with no AMS
    /// still reports the envelope).
    #[serde(rename = "ams")]
    pub units: Vec<AmsUnit>,

    /// Bitmask of connected unit positions, as a hex string on the wire.
    #[serde(deserialize_with = "de::opt_bits")]
    pub ams_exist_bits: Option<u64>,
    /// Bitmask of occupied trays, 4 bits per unit position.
    #[serde(deserialize_with = "de::opt_bits")]
    pub tray_exist_bits: Option<u64>,
    /// Bitmask of trays carrying Bambu RFID-tagged filament.
    #[serde(deserialize_with = "de::opt_bits")]
    pub tray_is_bbl_bits: Option<u64>,
    /// Bitmask of trays whose RFID read has completed.
    #[serde(deserialize_with = "de::opt_bits")]
    pub tray_read_done_bits: Option<u64>,

    /// Global index of the tray currently feeding the extruder.
    /// `254` = external spool, `255` = none. See [`AmsSystem::active_tray`].
    #[serde(deserialize_with = "de::opt_i64")]
    pub tray_now: Option<i64>,
    /// Target tray of an in-progress filament change.
    #[serde(deserialize_with = "de::opt_i64")]
    pub tray_tar: Option<i64>,
    /// Previously active tray.
    #[serde(deserialize_with = "de::opt_i64")]
    pub tray_pre: Option<i64>,

    #[serde(deserialize_with = "de::opt_i64")]
    pub version: Option<i64>,
    pub insert_flag: Option<bool>,
    pub power_on_flag: Option<bool>,
}

impl AmsSystem {
    /// Unit positions the printer says are connected, decoded from
    /// `ams_exist_bits`.
    #[must_use]
    pub fn connected_positions(&self) -> Vec<u8> {
        let Some(bits) = self.ams_exist_bits else {
            return Vec::new();
        };
        (0..64).filter(|bit| bits & (1 << bit) != 0).collect()
    }

    /// What is currently feeding the extruder, per `tray_now`.
    #[must_use]
    pub fn active_tray(&self) -> Option<ActiveTray> {
        self.tray_now.map(ActiveTray::from_global_index)
    }

    /// Resolve a global tray index to the actual tray, by matching unit and
    /// slot ids rather than assuming array positions — units arrive in wire
    /// order, which is not guaranteed to be id order.
    #[must_use]
    pub fn tray_at(&self, unit_id: i64, slot_id: i64) -> Option<&Tray> {
        self.units
            .iter()
            .find(|unit| unit.id == Some(unit_id))?
            .trays
            .iter()
            .find(|tray| tray.id == Some(slot_id))
    }
}

/// Where the filament feeding the extruder is coming from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ActiveTray {
    /// No tray engaged.
    None,
    /// The external spool holder (`vt_tray` in the report).
    ExternalSpool,
    /// A slot in an AMS unit.
    ///
    /// TODO(fixture): the `global / 4` decode is community-verified for
    /// standard units. How AMS HT units (which report ids ≥ 128) participate
    /// in the global index needs a real capture to confirm.
    Slot { unit: i64, slot: i64 },
    /// An index outside every documented range, preserved for display.
    Unknown(i64),
}

impl ActiveTray {
    /// Decode a `tray_now` / `tray_tar` / `tray_pre` global index.
    #[must_use]
    pub fn from_global_index(index: i64) -> Self {
        match index {
            TRAY_NONE => Self::None,
            TRAY_EXTERNAL => Self::ExternalSpool,
            0..=251 => Self::Slot {
                unit: index / 4,
                slot: index % 4,
            },
            other => Self::Unknown(other),
        }
    }
}

/// The kind of AMS unit, decoded from the wire `type` field newer firmware
/// attaches to each unit.
///
/// TODO(fixture): the numeric mapping is a community-sourced guess and the
/// field is absent entirely on older firmware. Verify against real captures
/// from each variant; `None`/absent on old firmware means "standard AMS" in
/// practice for X1/P1 machines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AmsUnitType {
    /// The original enclosed 4-slot AMS.
    Standard,
    /// AMS Lite — open frame, A1 series.
    Lite,
    /// AMS 2 Pro — 4-slot with active drying.
    Pro2,
    /// AMS HT — single high-temperature slot.
    Ht,
    Unknown(i64),
}

impl AmsUnitType {
    #[must_use]
    pub fn from_wire(raw: i64) -> Self {
        match raw {
            0 => Self::Standard,
            1 => Self::Lite,
            2 => Self::Pro2,
            3 => Self::Ht,
            other => Self::Unknown(other),
        }
    }
}

/// One AMS unit.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AmsUnit {
    /// Unit id. `0..=3` for standard positions; HT units have been observed
    /// reporting ids from 128.
    #[serde(deserialize_with = "de::opt_i64")]
    pub id: Option<i64>,

    /// Humidity bucket `1..=5` as displayed by Bambu Studio.
    ///
    /// TODO(fixture): which end of the scale is "dry" is asserted confidently
    /// in opposite directions by different community sources. Stored raw;
    /// interpretation is deferred until a capture from a unit with a known
    /// ambient answers it.
    #[serde(deserialize_with = "de::opt_i64")]
    pub humidity: Option<i64>,

    /// Measured relative humidity percent, on units that have the sensor
    /// (AMS 2 Pro, AMS HT).
    #[serde(deserialize_with = "de::opt_i64")]
    pub humidity_raw: Option<i64>,

    /// Unit temperature in °C, where the hardware reports it.
    #[serde(deserialize_with = "de::opt_f64")]
    pub temp: Option<f64>,

    /// Raw unit kind — see [`AmsUnitType`].
    #[serde(rename = "type", deserialize_with = "de::opt_i64")]
    pub unit_type_raw: Option<i64>,

    /// Remaining drying time in minutes, on units that dry.
    #[serde(deserialize_with = "de::opt_i64")]
    pub dry_time: Option<i64>,

    /// Trays in slot order, empty slots included.
    #[serde(rename = "tray")]
    pub trays: Vec<Tray>,
}

impl AmsUnit {
    /// Decoded unit kind, when the firmware reported one.
    #[must_use]
    pub fn unit_type(&self) -> Option<AmsUnitType> {
        self.unit_type_raw.map(AmsUnitType::from_wire)
    }
}

/// One tray (slot). An empty slot is a `Tray` whose filament fields are
/// absent — it still occupies its position.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tray {
    #[serde(deserialize_with = "de::opt_i64")]
    pub id: Option<i64>,

    /// Filament kind as reported: "PLA", "PETG", "TPU", …
    pub tray_type: Option<String>,
    /// RGBA hex, e.g. `"00AE42FF"`. See [`Tray::color_rgba`].
    pub tray_color: Option<String>,
    /// Additional colours for gradient/dual-colour filaments.
    pub cols: Vec<String>,
    /// Bambu filament id, e.g. `"GFA00"`.
    pub tray_info_idx: Option<String>,
    /// Bambu spool variant, e.g. `"A00-G1"`.
    pub tray_id_name: Option<String>,
    /// Marketing name for tagged spools, e.g. `"PLA Basic"`.
    pub tray_sub_brands: Option<String>,

    /// Remaining filament percent. `-1` means the printer does not know —
    /// see [`Tray::remain_percent`] for the cleaned view.
    #[serde(deserialize_with = "de::opt_i64")]
    pub remain: Option<i64>,

    #[serde(deserialize_with = "de::opt_f64")]
    pub k: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub n: Option<f64>,

    #[serde(deserialize_with = "de::opt_f64")]
    pub nozzle_temp_min: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub nozzle_temp_max: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub bed_temp: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub tray_diameter: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub tray_weight: Option<f64>,

    pub tag_uid: Option<String>,
    pub tray_uuid: Option<String>,
}

impl Tray {
    /// Whether this slot has filament in it.
    #[must_use]
    pub fn is_occupied(&self) -> bool {
        self.tray_type
            .as_deref()
            .is_some_and(|kind| !kind.is_empty())
    }

    /// Remaining percent, with the wire's `-1` ("unknown") and other
    /// out-of-range values cleaned to `None`.
    #[must_use]
    pub fn remain_percent(&self) -> Option<u8> {
        match self.remain {
            Some(value @ 0..=100) => Some(value as u8),
            _ => None,
        }
    }

    /// The tray colour as RGBA bytes. Accepts both `RRGGBBAA` and `RRGGBB`
    /// (alpha assumed opaque), which both occur in the wild.
    #[must_use]
    pub fn color_rgba(&self) -> Option<[u8; 4]> {
        let hex = self.tray_color.as_deref()?.trim();
        let parse = |slice: &str| u8::from_str_radix(slice, 16).ok();
        match hex.len() {
            8 => Some([
                parse(&hex[0..2])?,
                parse(&hex[2..4])?,
                parse(&hex[4..6])?,
                parse(&hex[6..8])?,
            ]),
            6 => Some([
                parse(&hex[0..2])?,
                parse(&hex[2..4])?,
                parse(&hex[4..6])?,
                0xff,
            ]),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(json: &str) -> AmsSystem {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn exist_bits_decode_unit_positions() {
        let ams = system(r#"{"ams_exist_bits": "3"}"#);
        assert_eq!(ams.connected_positions(), vec![0, 1]);
    }

    #[test]
    fn empty_slots_keep_their_position() {
        // Slots 0, 1 and 3 loaded; slot 2 bare. The bare slot must survive as
        // a positioned tray, not vanish.
        let ams = system(
            r#"{"ams": [{"id": "0", "tray": [
                {"id": "0", "tray_type": "PLA"},
                {"id": "1", "tray_type": "PETG"},
                {"id": "2"},
                {"id": "3", "tray_type": "TPU"}
            ]}]}"#,
        );

        let trays = &ams.units[0].trays;
        assert_eq!(trays.len(), 4);
        assert!(!trays[2].is_occupied());
        assert_eq!(trays[2].id, Some(2));
        assert!(trays[3].is_occupied());
    }

    #[test]
    fn tray_now_semantics() {
        assert_eq!(
            system(r#"{"tray_now": "255"}"#).active_tray(),
            Some(ActiveTray::None)
        );
        assert_eq!(
            system(r#"{"tray_now": "254"}"#).active_tray(),
            Some(ActiveTray::ExternalSpool)
        );
        assert_eq!(
            system(r#"{"tray_now": "6"}"#).active_tray(),
            Some(ActiveTray::Slot { unit: 1, slot: 2 })
        );
        // No tray_now at all: we genuinely don't know, which is not "none".
        assert_eq!(system("{}").active_tray(), None);
    }

    #[test]
    fn tray_resolution_matches_ids_not_array_positions() {
        // Units deliberately out of id order.
        let ams = system(
            r#"{"ams": [
                {"id": "1", "tray": [{"id": "0", "tray_type": "ABS"}]},
                {"id": "0", "tray": [{"id": "0", "tray_type": "PLA"}]}
            ]}"#,
        );
        assert_eq!(
            ams.tray_at(0, 0).and_then(|tray| tray.tray_type.as_deref()),
            Some("PLA")
        );
    }

    #[test]
    fn unknown_remain_is_none_not_a_number() {
        let tray: Tray = serde_json::from_str(r#"{"remain": -1}"#).unwrap();
        assert_eq!(tray.remain, Some(-1), "raw value preserved");
        assert_eq!(tray.remain_percent(), None, "cleaned view says unknown");
    }

    #[test]
    fn colors_parse_with_and_without_alpha() {
        let tray: Tray = serde_json::from_str(r#"{"tray_color": "00AE42FF"}"#).unwrap();
        assert_eq!(tray.color_rgba(), Some([0x00, 0xae, 0x42, 0xff]));

        let tray: Tray = serde_json::from_str(r#"{"tray_color": "00AE42"}"#).unwrap();
        assert_eq!(tray.color_rgba(), Some([0x00, 0xae, 0x42, 0xff]));

        let tray: Tray = serde_json::from_str(r#"{"tray_color": ""}"#).unwrap();
        assert_eq!(tray.color_rgba(), None);
    }

    #[test]
    fn unit_types_decode_with_unknown_preserved() {
        let unit: AmsUnit = serde_json::from_str(r#"{"id": "128", "type": 3}"#).unwrap();
        assert_eq!(unit.unit_type(), Some(AmsUnitType::Ht));
        assert_eq!(unit.id, Some(128));

        let unit: AmsUnit = serde_json::from_str(r#"{"type": 7}"#).unwrap();
        assert_eq!(unit.unit_type(), Some(AmsUnitType::Unknown(7)));

        // Older firmware sends no type at all: honestly unknown.
        let unit: AmsUnit = serde_json::from_str(r#"{"id": "0"}"#).unwrap();
        assert_eq!(unit.unit_type(), None);
    }

    #[test]
    fn string_and_numeric_ids_are_equivalent() {
        // A1-era firmware sends numeric ids where X1 sends strings.
        let a: AmsUnit = serde_json::from_str(r#"{"id": "2"}"#).unwrap();
        let b: AmsUnit = serde_json::from_str(r#"{"id": 2}"#).unwrap();
        assert_eq!(a.id, b.id);
    }
}
