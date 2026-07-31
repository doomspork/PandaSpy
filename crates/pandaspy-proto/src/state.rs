//! Accumulated printer state, and the accumulator that builds it.
//!
//! # Architecture
//!
//! Reports are merged as JSON documents ([`crate::merge`]) and *then* viewed
//! as typed state, rather than merged field-by-field on a typed struct. Two
//! reasons:
//!
//! * The merge semantics (absent = unchanged, null = cleared) are defined
//!   once, at the document layer, instead of being re-implemented — and
//!   eventually got wrong — for every one of dozens of fields.
//! * Fields this build does not model yet still accumulate faithfully in the
//!   document, visible via [`StateAccumulator::document`] for diagnostics and
//!   ready for the day they are modelled, instead of being dropped by a
//!   struct that never knew them.
//!
//! # Contract with the connection layer
//!
//! Deltas that arrive while disconnected are lost forever, so accumulated
//! state is stale after any gap. The client must [`StateAccumulator::reset`]
//! on reconnect and request a fresh `pushall` — merge semantics cannot repair
//! a hole in the stream.

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ams::{ActiveTray, AmsSystem, Tray};
use crate::hms::HmsEntry;
use crate::job::{GcodeState, PrintStage, PrinterStatus};
use crate::merge::deep_merge;
use crate::wire::{Report, ReportKind};
use crate::{ProtoError, de};

/// Folds `push_status` reports into one accumulated document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StateAccumulator {
    print: Value,
    applied: u64,
}

impl StateAccumulator {
    #[must_use]
    pub fn new() -> Self {
        Self {
            print: Value::Object(Map::new()),
            applied: 0,
        }
    }

    /// Merge a report. Only [`ReportKind::PrintPushStatus`] carries state;
    /// everything else is ignored and `false` is returned.
    pub fn apply(&mut self, report: &Report) -> bool {
        if *report.kind() != ReportKind::PrintPushStatus {
            return false;
        }
        let Some(print) = report.raw().get("print") else {
            return false;
        };
        if self.print.is_null() {
            self.print = Value::Object(Map::new());
        }
        deep_merge(&mut self.print, print);
        self.applied += 1;
        true
    }

    /// Parse raw payload bytes and merge in one step.
    ///
    /// # Errors
    ///
    /// Only if the bytes are not JSON.
    pub fn apply_payload(&mut self, payload: &[u8]) -> Result<bool, ProtoError> {
        Ok(self.apply(&Report::parse(payload)?))
    }

    /// The typed view of everything accumulated so far.
    ///
    /// # Errors
    ///
    /// Only on a structural surprise (e.g. firmware sending `ams` as a
    /// string) — scalar oddities are absorbed by the lenient deserializers.
    pub fn state(&self) -> Result<PrinterState, ProtoError> {
        Ok(serde_json::from_value(self.print.clone())?)
    }

    /// The raw accumulated document — includes fields the typed view does
    /// not model yet. This is what a diagnostics screen should show.
    #[must_use]
    pub fn document(&self) -> &Value {
        &self.print
    }

    /// How many state reports have been merged since the last reset.
    #[must_use]
    pub fn reports_applied(&self) -> u64 {
        self.applied
    }

    /// Forget everything. Call on reconnect, before requesting `pushall` —
    /// state assembled before a gap in the delta stream cannot be trusted.
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

/// Everything PandaSpy knows about one printer at one instant.
///
/// Field names deliberately match the wire (`mc_percent`, not `progress`):
/// this struct *is* the deserialized accumulated document, and an invented
/// nicer name is one more mapping to get wrong. Derived accessors provide the
/// clean vocabulary.
///
/// Every field is `Option` (or a container that can be empty). `None` means
/// "the printer has not said", which is different from any real value — see
/// the crate docs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrinterState {
    // ─── Temperatures (°C) ───────────────────────────────────────────────
    #[serde(deserialize_with = "de::opt_f64")]
    pub nozzle_temper: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub nozzle_target_temper: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub bed_temper: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub bed_target_temper: Option<f64>,
    /// Only enclosed models report this; `None` on an A1 is normal.
    #[serde(deserialize_with = "de::opt_f64")]
    pub chamber_temper: Option<f64>,

    // ─── Fans (raw gears, see [`fan_gear_to_percent`]) ───────────────────
    #[serde(deserialize_with = "de::opt_i64")]
    pub cooling_fan_speed: Option<i64>,
    #[serde(deserialize_with = "de::opt_i64")]
    pub big_fan1_speed: Option<i64>,
    #[serde(deserialize_with = "de::opt_i64")]
    pub big_fan2_speed: Option<i64>,
    #[serde(deserialize_with = "de::opt_i64")]
    pub heatbreak_fan_speed: Option<i64>,

    // ─── The job ─────────────────────────────────────────────────────────
    pub gcode_state: Option<GcodeState>,
    pub gcode_file: Option<String>,
    /// Name of the running task as the slicer titled it.
    pub subtask_name: Option<String>,
    /// Progress percent, `0..=100`.
    #[serde(deserialize_with = "de::opt_i64")]
    pub mc_percent: Option<i64>,
    /// Remaining time in minutes.
    #[serde(deserialize_with = "de::opt_i64")]
    pub mc_remaining_time: Option<i64>,
    #[serde(deserialize_with = "de::opt_i64")]
    pub layer_num: Option<i64>,
    #[serde(deserialize_with = "de::opt_i64")]
    pub total_layer_num: Option<i64>,
    /// Raw sub-stage id — see [`PrinterState::stage`].
    #[serde(deserialize_with = "de::opt_i64")]
    pub stg_cur: Option<i64>,
    /// `"local"` / `"cloud"` / `"idle"`.
    pub print_type: Option<String>,
    /// Device-error code; `0` is "no error". See
    /// [`PrinterState::print_error_description`].
    #[serde(deserialize_with = "de::opt_i64")]
    pub print_error: Option<i64>,

    // ─── Speed ───────────────────────────────────────────────────────────
    /// Speed level `1..=4` (silent / standard / sport / ludicrous).
    #[serde(deserialize_with = "de::opt_i64")]
    pub spd_lvl: Option<i64>,
    /// Speed magnitude percent.
    #[serde(deserialize_with = "de::opt_i64")]
    pub spd_mag: Option<i64>,

    // ─── Hardware & environment ──────────────────────────────────────────
    /// e.g. `"0.4"`. A string on the wire, and the exact marking on the
    /// physical nozzle, so it stays one.
    pub nozzle_diameter: Option<String>,
    pub nozzle_type: Option<NozzleType>,
    /// e.g. `"-46dBm"`, kept verbatim for display.
    pub wifi_signal: Option<String>,
    pub sdcard: Option<bool>,
    /// Bitfield of misc flags; undecoded until fixtures pin meanings.
    #[serde(deserialize_with = "de::opt_i64")]
    pub home_flag: Option<i64>,
    pub lights_report: Vec<LightReport>,

    // ─── Materials ───────────────────────────────────────────────────────
    pub ams: Option<AmsSystem>,
    /// The external spool holder ("virtual tray").
    pub vt_tray: Option<Tray>,

    // ─── Health ──────────────────────────────────────────────────────────
    pub hms: Vec<HmsEntry>,
}

impl PrinterState {
    /// The five-way status a card headline or tray glyph renders.
    #[must_use]
    pub fn status(&self) -> Option<PrinterStatus> {
        self.gcode_state.as_ref().map(GcodeState::status)
    }

    /// The fine-grained machine stage, decoded from `stg_cur`.
    #[must_use]
    pub fn stage(&self) -> Option<PrintStage> {
        self.stg_cur.map(PrintStage::from_stg_cur)
    }

    /// Progress clamped to `0..=100`.
    #[must_use]
    pub fn progress_percent(&self) -> Option<u8> {
        self.mc_percent.map(|value| value.clamp(0, 100) as u8)
    }

    /// Remaining print time.
    #[must_use]
    pub fn remaining(&self) -> Option<Duration> {
        self.mc_remaining_time
            .filter(|minutes| *minutes >= 0)
            .map(|minutes| Duration::from_secs(minutes as u64 * 60))
    }

    /// Estimated completion, given the caller's clock.
    ///
    /// `now` is a parameter because this crate has no clock — determinism is
    /// part of its contract, and the wasm purity build enforces it.
    #[must_use]
    pub fn eta(&self, now: SystemTime) -> Option<SystemTime> {
        now.checked_add(self.remaining()?)
    }

    /// Current and total layer, when both are known.
    #[must_use]
    pub fn layer_progress(&self) -> Option<(i64, i64)> {
        Some((self.layer_num?, self.total_layer_num?))
    }

    /// What is feeding the extruder right now.
    #[must_use]
    pub fn active_tray(&self) -> Option<ActiveTray> {
        self.ams.as_ref()?.active_tray()
    }

    /// The tray behind [`Self::active_tray`], resolved to its filament data.
    #[must_use]
    pub fn active_filament(&self) -> Option<&Tray> {
        match self.active_tray()? {
            ActiveTray::Slot { unit, slot } => self.ams.as_ref()?.tray_at(unit, slot),
            ActiveTray::ExternalSpool => self.vt_tray.as_ref(),
            ActiveTray::None | ActiveTray::Unknown(_) => None,
        }
    }

    /// Human-readable text for `print_error`, from the embedded table.
    #[must_use]
    pub fn print_error_description(&self, lang: &str) -> Option<&'static str> {
        crate::hms::table().describe_print_error(self.print_error?, lang)
    }
}

/// Nozzle material, as reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
#[non_exhaustive]
pub enum NozzleType {
    HardenedSteel,
    StainlessSteel,
    Unknown(String),
}

impl From<String> for NozzleType {
    fn from(raw: String) -> Self {
        match raw.as_str() {
            "hardened_steel" => Self::HardenedSteel,
            "stainless_steel" => Self::StainlessSteel,
            _ => Self::Unknown(raw),
        }
    }
}

impl From<NozzleType> for String {
    fn from(value: NozzleType) -> Self {
        match value {
            NozzleType::HardenedSteel => "hardened_steel".to_owned(),
            NozzleType::StainlessSteel => "stainless_steel".to_owned(),
            NozzleType::Unknown(raw) => raw,
        }
    }
}

/// One entry of `lights_report`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightReport {
    /// e.g. `"chamber_light"`.
    pub node: Option<String>,
    /// e.g. `"on"`, `"off"`, `"flashing"`.
    pub mode: Option<String>,
}

/// Convert a raw fan value to a percentage.
///
/// X1/P1-era firmware reports fans as a `0..=15` gear; some newer payloads
/// carry a plain percent. Values in the gear range are scaled, values that
/// already look like percentages pass through, everything else clamps.
#[must_use]
pub fn fan_gear_to_percent(raw: i64) -> u8 {
    match raw {
        i64::MIN..=0 => 0,
        1..=15 => ((raw * 100 + 7) / 15) as u8,
        16..=100 => raw as u8,
        _ => 100,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push(json: &str) -> Report {
        Report::parse(json.as_bytes()).unwrap()
    }

    #[test]
    fn missing_keys_deserialise_to_none_rather_than_failing() {
        let state: PrinterState = serde_json::from_str("{}").unwrap();
        assert_eq!(state, PrinterState::default());
    }

    #[test]
    fn unknown_keys_are_ignored_not_rejected() {
        // A future firmware adds a field. Old builds must keep working.
        let state: PrinterState =
            serde_json::from_str(r#"{"mc_percent": 7, "chamber_fan_rpm": 1200}"#).unwrap();
        assert_eq!(state.mc_percent, Some(7));
    }

    #[test]
    fn absent_fields_in_a_delta_do_not_clobber_known_state() {
        let mut acc = StateAccumulator::new();
        acc.apply(&push(
            r#"{"print": {"command": "push_status",
                "gcode_state": "RUNNING", "mc_percent": 9, "bed_temper": 60.0}}"#,
        ));
        acc.apply(&push(
            r#"{"print": {"command": "push_status", "mc_percent": 10}}"#,
        ));

        let state = acc.state().unwrap();
        assert_eq!(state.mc_percent, Some(10));
        assert_eq!(state.bed_temper, Some(60.0), "bed_temper was clobbered");
        assert_eq!(state.gcode_state, Some(GcodeState::Running));
    }

    #[test]
    fn a_present_null_clears_where_absence_preserves() {
        let mut acc = StateAccumulator::new();
        acc.apply(&push(
            r#"{"print": {"command": "push_status", "wifi_signal": "-52dBm", "mc_percent": 5}}"#,
        ));
        acc.apply(&push(
            r#"{"print": {"command": "push_status", "wifi_signal": null}}"#,
        ));

        let state = acc.state().unwrap();
        assert_eq!(state.wifi_signal, None, "explicit null must clear");
        assert_eq!(state.mc_percent, Some(5), "absent field must survive");
    }

    #[test]
    fn non_state_reports_are_not_merged() {
        let mut acc = StateAccumulator::new();
        let merged = acc.apply(&push(
            r#"{"print": {"command": "project_file", "gcode_state": "SHOULD_NOT_LAND"}}"#,
        ));

        assert!(!merged);
        assert_eq!(acc.reports_applied(), 0);
        assert_eq!(acc.state().unwrap().gcode_state, None);
    }

    #[test]
    fn reset_forgets_everything() {
        let mut acc = StateAccumulator::new();
        acc.apply(&push(
            r#"{"print": {"command": "push_status", "mc_percent": 50}}"#,
        ));
        acc.reset();

        assert_eq!(acc.reports_applied(), 0);
        assert_eq!(acc.state().unwrap(), PrinterState::default());
    }

    #[test]
    fn unmodelled_fields_survive_in_the_document() {
        // The typed view does not know `aux_part_fan`; the document keeps it
        // anyway, which is what makes diagnostics and future modelling work.
        let mut acc = StateAccumulator::new();
        acc.apply(&push(
            r#"{"print": {"command": "push_status", "aux_part_fan": true}}"#,
        ));
        assert_eq!(acc.document()["aux_part_fan"], serde_json::json!(true));
    }

    #[test]
    fn derived_values_compute_from_raw_fields() {
        let state: PrinterState = serde_json::from_str(
            r#"{
                "gcode_state": "RUNNING", "stg_cur": 1,
                "mc_percent": 42, "mc_remaining_time": 96,
                "layer_num": 57, "total_layer_num": 137
            }"#,
        )
        .unwrap();

        assert_eq!(state.status(), Some(PrinterStatus::Printing));
        assert_eq!(state.stage(), Some(PrintStage::AutoBedLeveling));
        assert_eq!(state.progress_percent(), Some(42));
        assert_eq!(state.remaining(), Some(Duration::from_secs(96 * 60)));
        assert_eq!(state.layer_progress(), Some((57, 137)));

        let now = SystemTime::UNIX_EPOCH;
        assert_eq!(
            state.eta(now),
            Some(now + Duration::from_secs(96 * 60)),
            "eta is caller-clocked; this crate owns no clock"
        );
    }

    #[test]
    fn active_filament_resolves_through_ams_and_external_spool() {
        let state: PrinterState = serde_json::from_str(
            r#"{"ams": {
                    "tray_now": "1",
                    "ams": [{"id": "0", "tray": [
                        {"id": "0", "tray_type": "PLA"},
                        {"id": "1", "tray_type": "PETG"}
                    ]}]
                },
                "vt_tray": {"id": "254", "tray_type": "ABS"}}"#,
        )
        .unwrap();
        assert_eq!(
            state.active_filament().and_then(|t| t.tray_type.as_deref()),
            Some("PETG")
        );

        let state: PrinterState = serde_json::from_str(
            r#"{"ams": {"tray_now": "254", "ams": []},
                "vt_tray": {"id": "254", "tray_type": "ABS"}}"#,
        )
        .unwrap();
        assert_eq!(
            state.active_filament().and_then(|t| t.tray_type.as_deref()),
            Some("ABS")
        );
    }

    #[test]
    fn fan_gears_and_percentages_both_normalise() {
        assert_eq!(fan_gear_to_percent(0), 0);
        assert_eq!(fan_gear_to_percent(15), 100);
        assert_eq!(fan_gear_to_percent(9), 60);
        assert_eq!(fan_gear_to_percent(70), 70, "already a percent");
        assert_eq!(fan_gear_to_percent(900), 100, "garbage clamps");
    }
}
