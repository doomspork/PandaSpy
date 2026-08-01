//! The frontend contract: clean, UI-facing view types and the mapping from the
//! domain crates onto them.
//!
//! The frontend never sees `pandaspy-proto`'s wire-named fields (`nozzle_temper`,
//! `mc_percent`) or the client's internal enums directly. This module is the
//! one place that translates domain types into the shape the window renders, so
//! the boundary is explicit and the UI has nothing to know about the protocol.
//!
//! Everything here is `Serialize` in `camelCase`; `src/lib/ipc.ts` mirrors it.

use pandaspy_client::{ConnectionState, FailureReason};
use pandaspy_proto::{ActiveTray, PrinterState};
use serde::Serialize;

/// One printer as the UI lists it: identity, where it is, how the connection is
/// doing, and the latest parsed state (absent until the first report).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterView {
    pub serial: String,
    pub nickname: Option<String>,
    pub model: Option<String>,
    pub address: Option<String>,
    pub connection: ConnectionView,
    pub state: Option<PrinterSnapshot>,
}

/// The connection lifecycle, flattened for the UI: a stable `status` string and
/// an optional human `detail` (the failure reason).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    /// `disconnected` / `connecting` / `handshaking` / `connected` / `failed`.
    pub status: &'static str,
    /// For `failed`: a stable reason key the UI localises
    /// (`wrong-access-code`, `unreachable`, `certificate-changed`, …).
    pub reason: Option<&'static str>,
}

impl ConnectionView {
    pub const DISCONNECTED: Self = Self {
        status: "disconnected",
        reason: None,
    };

    #[must_use]
    pub fn from_state(state: &ConnectionState) -> Self {
        match state {
            ConnectionState::Disconnected => Self::DISCONNECTED,
            ConnectionState::Connecting => Self {
                status: "connecting",
                reason: None,
            },
            ConnectionState::Handshaking => Self {
                status: "handshaking",
                reason: None,
            },
            ConnectionState::Connected => Self {
                status: "connected",
                reason: None,
            },
            ConnectionState::Failed(reason) => Self {
                status: "failed",
                reason: Some(failure_key(reason)),
            },
            // `ConnectionState` is `#[non_exhaustive]`; a variant added later
            // shows as disconnected until this map learns it.
            _ => Self::DISCONNECTED,
        }
    }
}

/// Stable, localisable key per failure reason.
fn failure_key(reason: &FailureReason) -> &'static str {
    match reason {
        FailureReason::WrongAccessCode => "wrong-access-code",
        FailureReason::Unreachable { .. } => "unreachable",
        FailureReason::Tls { .. } => "tls",
        FailureReason::CertificateChanged => "certificate-changed",
        FailureReason::Protocol { .. } => "protocol",
        FailureReason::ConnectionClosed => "connection-closed",
        // `FailureReason` is `#[non_exhaustive]`; an unmapped future reason
        // still renders as a generic failure rather than failing to compile.
        _ => "unknown",
    }
}

/// A UI-ready snapshot derived from `pandaspy_proto::PrinterState`. Clean names,
/// clamped values, HMS resolved to text — everything the card needs and nothing
/// about the wire.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterSnapshot {
    /// `idle` / `preparing` / `printing` / `paused` / `finished` / `failed` /
    /// `unknown`, or `None` if the printer has not said.
    pub status: Option<&'static str>,
    /// The fine sub-stage key (`auto-bed-leveling`, …), when known.
    pub stage: Option<String>,
    pub progress_percent: Option<u8>,
    pub remaining_secs: Option<u64>,
    pub layer: Option<i64>,
    pub total_layers: Option<i64>,
    pub nozzle_temp: Option<f64>,
    pub nozzle_target: Option<f64>,
    pub bed_temp: Option<f64>,
    pub bed_target: Option<f64>,
    pub chamber_temp: Option<f64>,
    pub task_name: Option<String>,
    pub ams: Vec<AmsUnitView>,
    /// Which tray is feeding the extruder: `{unit, slot}`, `external`, or absent.
    pub active_tray: Option<ActiveTrayView>,
    pub hms: Vec<HmsView>,
    /// Resolved text for a non-zero `print_error`, when there is one.
    pub print_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmsUnitView {
    pub id: Option<i64>,
    pub kind: Option<&'static str>,
    pub humidity: Option<i64>,
    pub temperature: Option<f64>,
    pub trays: Vec<TrayView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayView {
    pub id: Option<i64>,
    pub occupied: bool,
    pub filament_type: Option<String>,
    /// RGBA hex, e.g. `"00AE42FF"`, when the tray reported a colour.
    pub color: Option<String>,
    pub remain_percent: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveTrayView {
    /// `slot` / `external` / `none` / `unknown`.
    pub kind: &'static str,
    pub unit: Option<i64>,
    pub slot: Option<i64>,
}

/// A health event, resolved to displayable text in the caller's locale.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HmsView {
    /// `0300_0200_0001_0001`.
    pub code: Option<String>,
    /// `fatal` / `serious` / `common` / `info`, when decodable.
    pub severity: Option<&'static str>,
    /// Localised description from the embedded table, or `None` for an unknown
    /// code (the UI shows the raw code and the wiki link then).
    pub text: Option<String>,
    pub wiki_url: Option<String>,
}

impl PrinterSnapshot {
    /// Derive the snapshot, resolving HMS/error text in `lang`.
    #[must_use]
    pub fn from_state(state: &PrinterState, lang: &str) -> Self {
        Self {
            status: state.status().map(status_key),
            stage: state
                .stage()
                .and_then(|s| s.key().map(str::to_owned))
                .or_else(|| state.stg_cur.map(|n| n.to_string())),
            progress_percent: state.progress_percent(),
            remaining_secs: state.remaining().map(|d| d.as_secs()),
            layer: state.layer_num,
            total_layers: state.total_layer_num,
            nozzle_temp: state.nozzle_temper,
            nozzle_target: state.nozzle_target_temper,
            bed_temp: state.bed_temper,
            bed_target: state.bed_target_temper,
            chamber_temp: state.chamber_temper,
            task_name: state.subtask_name.clone(),
            ams: state
                .ams
                .as_ref()
                .map(|ams| ams.units.iter().map(AmsUnitView::from_unit).collect())
                .unwrap_or_default(),
            active_tray: state.active_tray().map(ActiveTrayView::from_active),
            hms: state
                .hms
                .iter()
                .map(|e| HmsView::from_entry(e, lang))
                .collect(),
            print_error: state.print_error_description(lang).map(str::to_owned),
        }
    }
}

impl AmsUnitView {
    fn from_unit(unit: &pandaspy_proto::AmsUnit) -> Self {
        Self {
            id: unit.id,
            kind: unit.unit_type().map(ams_kind),
            humidity: unit.humidity,
            temperature: unit.temp,
            trays: unit.trays.iter().map(TrayView::from_tray).collect(),
        }
    }
}

impl TrayView {
    fn from_tray(tray: &pandaspy_proto::Tray) -> Self {
        Self {
            id: tray.id,
            occupied: tray.is_occupied(),
            filament_type: tray.tray_type.clone().filter(|s| !s.is_empty()),
            color: tray.tray_color.clone().filter(|s| !s.is_empty()),
            remain_percent: tray.remain_percent(),
        }
    }
}

impl ActiveTrayView {
    fn from_active(active: ActiveTray) -> Self {
        match active {
            ActiveTray::None => Self {
                kind: "none",
                unit: None,
                slot: None,
            },
            ActiveTray::ExternalSpool => Self {
                kind: "external",
                unit: None,
                slot: None,
            },
            ActiveTray::Slot { unit, slot } => Self {
                kind: "slot",
                unit: Some(unit),
                slot: Some(slot),
            },
            ActiveTray::Unknown(_) => Self {
                kind: "unknown",
                unit: None,
                slot: None,
            },
            // `ActiveTray` is `#[non_exhaustive]`.
            _ => Self {
                kind: "unknown",
                unit: None,
                slot: None,
            },
        }
    }
}

impl HmsView {
    fn from_entry(entry: &pandaspy_proto::HmsEntry, lang: &str) -> Self {
        Self {
            code: entry.display_code(),
            severity: entry.severity().map(severity_key),
            text: entry.describe(lang).map(str::to_owned),
            wiki_url: entry.wiki_url(),
        }
    }
}

fn status_key(status: pandaspy_proto::PrinterStatus) -> &'static str {
    use pandaspy_proto::PrinterStatus::*;
    match status {
        Idle => "idle",
        Preparing => "preparing",
        Printing => "printing",
        Paused => "paused",
        Finished => "finished",
        Failed => "failed",
        Unknown => "unknown",
        _ => "unknown",
    }
}

fn severity_key(severity: pandaspy_proto::HmsSeverity) -> &'static str {
    use pandaspy_proto::HmsSeverity::*;
    match severity {
        Fatal => "fatal",
        Serious => "serious",
        Common => "common",
        Info => "info",
        Unknown(_) => "unknown",
        _ => "unknown",
    }
}

fn ams_kind(kind: pandaspy_proto::AmsUnitType) -> &'static str {
    use pandaspy_proto::AmsUnitType::*;
    match kind {
        Standard => "standard",
        Lite => "lite",
        Pro2 => "pro2",
        Ht => "ht",
        Unknown(_) => "unknown",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_printer_maps_to_a_clean_snapshot() {
        let state: PrinterState = serde_json::from_str(
            r#"{"gcode_state":"RUNNING","stg_cur":0,"mc_percent":42,
                "mc_remaining_time":96,"layer_num":57,"total_layer_num":137,
                "nozzle_temper":219.6,"bed_temper":60.0,"subtask_name":"benchy"}"#,
        )
        .unwrap();

        let snap = PrinterSnapshot::from_state(&state, "en-US");
        assert_eq!(snap.status, Some("printing"));
        assert_eq!(snap.stage.as_deref(), Some("printing"));
        assert_eq!(snap.progress_percent, Some(42));
        assert_eq!(snap.remaining_secs, Some(96 * 60));
        assert_eq!(snap.layer, Some(57));
        assert_eq!(snap.task_name.as_deref(), Some("benchy"));
        // A JSON round-trip proves the camelCase contract the frontend keys off.
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"progressPercent\":42"), "{json}");
        assert!(json.contains("\"totalLayers\":137"), "{json}");
    }

    #[test]
    fn ams_units_and_trays_map_with_empty_slots_preserved() {
        let state: PrinterState = serde_json::from_str(
            r#"{"ams":{"tray_now":"1","ams":[{"id":"0","humidity":"3","tray":[
                {"id":"0","tray_type":"PLA","tray_color":"00AE42FF","remain":92},
                {"id":"1"},
                {"id":"2","tray_type":"PETG","tray_color":"0A2989FF","remain":34}
            ]}]}}"#,
        )
        .unwrap();

        let snap = PrinterSnapshot::from_state(&state, "en-US");
        assert_eq!(snap.ams.len(), 1);
        let trays = &snap.ams[0].trays;
        assert_eq!(trays.len(), 3, "the empty slot keeps its position");
        assert!(trays[0].occupied);
        assert!(!trays[1].occupied);
        assert_eq!(trays[0].remain_percent, Some(92));
        assert_eq!(
            snap.active_tray.as_ref().map(|a| a.kind),
            Some("slot"),
            "tray_now 1 → a slot"
        );
    }

    #[test]
    fn hms_resolves_to_localised_text() {
        // 0300_0200_0001_0001 is in the embedded table (both locales).
        let state: PrinterState =
            serde_json::from_str(r#"{"hms":[{"attr":50332160,"code":65537}]}"#).unwrap();

        let english = PrinterSnapshot::from_state(&state, "en-US");
        assert_eq!(english.hms.len(), 1);
        assert_eq!(english.hms[0].severity, Some("fatal"));
        assert!(english.hms[0].text.as_ref().unwrap().contains("nozzle"));
        assert!(english.hms[0].code.as_deref() == Some("0300_0200_0001_0001"));

        let polish = PrinterSnapshot::from_state(&state, "pl-PL");
        assert!(polish.hms[0].text.as_ref().unwrap().contains("dyszy"));
    }

    #[test]
    fn connection_states_flatten_for_the_ui() {
        assert_eq!(
            ConnectionView::from_state(&ConnectionState::Connected).status,
            "connected"
        );
        let failed =
            ConnectionView::from_state(&ConnectionState::Failed(FailureReason::WrongAccessCode));
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.reason, Some("wrong-access-code"));
    }
}
