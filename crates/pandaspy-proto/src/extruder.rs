//! The extruder block dual-nozzle printers (H2D, X2D) report under
//! `device.extruder`.
//!
//! On these machines `ams.tray_now` is **not** a global tray index: it carries
//! only the slot number within whichever unit is feeding, so the `global / 4`
//! decode lands on the wrong unit. A real X2D capture
//! (`fixtures/reports/x2d-dual-extruder-feeding-ams2.json`) printing from the
//! second AMS, slot 3, reports `tray_now: "3"` — which decodes as the first
//! AMS — while the active extruder's `snow` reads `259` (`0x0103`: unit 1,
//! slot 3). Each extruder reports its own `snow` ("slot now"), and
//! `extruder.state` says which extruder is printing.

use serde::{Deserialize, Serialize};

use crate::ams::{ActiveTray, TRAY_EXTERNAL, TRAY_NONE};
use crate::de;

/// `snow` value meaning "nothing loaded in this extruder".
pub const SLOT_NONE: i64 = 0xFFFF;

/// The `device` object. Only what is modelled is typed; the rest still
/// accumulates in the merged document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Device {
    pub extruder: Option<ExtruderSystem>,
}

/// `device.extruder`: per-extruder state plus a packed printer-wide word.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtruderSystem {
    pub info: Vec<Extruder>,
    /// Packed word. Low nibble: extruder count; next nibble: the active
    /// extruder's id (X2D capture: `33042` = `0x8112` → two extruders, id 1
    /// active, matching the extruder whose `hnow`/`snow` were populated).
    #[serde(deserialize_with = "de::opt_i64")]
    pub state: Option<i64>,
}

impl ExtruderSystem {
    /// Id of the extruder currently printing, from `state`.
    #[must_use]
    pub fn active_id(&self) -> Option<i64> {
        Some((self.state? >> 4) & 0xF)
    }

    /// What is feeding the active extruder, per its `snow`. `None` when the
    /// block is too incomplete to say.
    #[must_use]
    pub fn active_tray(&self) -> Option<ActiveTray> {
        let id = self.active_id()?;
        let extruder = self.info.iter().find(|e| e.id == Some(id))?;
        extruder.snow.map(ActiveTray::from_slot_now)
    }
}

/// One extruder (nozzle) on a multi-extruder printer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Extruder {
    #[serde(deserialize_with = "de::opt_i64")]
    pub id: Option<i64>,
    /// Tray feeding this extruder, as `(unit_id << 8) | slot`.
    /// `0xFFFF` = none. See [`ActiveTray::from_slot_now`].
    #[serde(deserialize_with = "de::opt_i64")]
    pub snow: Option<i64>,
    /// Previous tray, same encoding as `snow`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub spre: Option<i64>,
    /// Target tray of an in-progress change, same encoding as `snow`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub star: Option<i64>,
}

impl ActiveTray {
    /// Decode an extruder's `snow` / `spre` / `star`: unit id in the high
    /// byte, slot in the low byte. Unit ids are the AMS `id`s themselves, so
    /// AMS HT units (ids from 128) need no special case here.
    ///
    /// TODO(fixture): external spools are assumed to use the `vir_slot` ids
    /// (254/255) as the unit byte. The X2D capture only shows an AMS feed.
    #[must_use]
    pub fn from_slot_now(raw: i64) -> Self {
        if raw == SLOT_NONE {
            return Self::None;
        }
        match (raw >> 8, raw & 0xFF) {
            (TRAY_EXTERNAL | TRAY_NONE, _) => Self::ExternalSpool,
            (unit @ 0..=253, slot) => Self::Slot { unit, slot },
            _ => Self::Unknown(raw),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extruders(json: &str) -> ExtruderSystem {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn slot_now_decodes_unit_and_slot_bytes() {
        assert_eq!(
            ActiveTray::from_slot_now(259),
            ActiveTray::Slot { unit: 1, slot: 3 }
        );
        assert_eq!(
            ActiveTray::from_slot_now(0),
            ActiveTray::Slot { unit: 0, slot: 0 }
        );
        assert_eq!(
            ActiveTray::from_slot_now(128 << 8),
            ActiveTray::Slot { unit: 128, slot: 0 },
            "AMS HT ids pass straight through"
        );
        assert_eq!(ActiveTray::from_slot_now(0xFFFF), ActiveTray::None);
        assert_eq!(
            ActiveTray::from_slot_now(254 << 8),
            ActiveTray::ExternalSpool
        );
        assert_eq!(ActiveTray::from_slot_now(-1), ActiveTray::Unknown(-1));
    }

    #[test]
    fn active_extruder_comes_from_state() {
        // Shape of the real X2D capture: extruder 0 empty, extruder 1 active.
        let ex = extruders(
            r#"{"state": 33042, "info": [
                {"id": 0, "snow": 65535},
                {"id": 1, "snow": 259}
            ]}"#,
        );
        assert_eq!(ex.active_id(), Some(1));
        assert_eq!(
            ex.active_tray(),
            Some(ActiveTray::Slot { unit: 1, slot: 3 })
        );

        // Same trays, extruder 0 active: it has nothing loaded.
        let ex = extruders(
            r#"{"state": 2, "info": [
                {"id": 0, "snow": 65535},
                {"id": 1, "snow": 259}
            ]}"#,
        );
        assert_eq!(ex.active_tray(), Some(ActiveTray::None));
    }

    #[test]
    fn incomplete_block_says_nothing() {
        assert_eq!(
            extruders(r#"{"info": [{"id": 0}, {"id": 1}]}"#).active_tray(),
            None
        );
        assert_eq!(
            extruders(r#"{"state": 18, "info": [{"id": 1}]}"#).active_tray(),
            None
        );
    }
}
