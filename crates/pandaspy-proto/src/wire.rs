//! The MQTT-carried wire protocol: topics, connection constants, report
//! classification, and the two requests a read-only monitor is allowed to
//! send.
//!
//! MQTT framing itself (3.1.1, TLS on port [`MQTT_PORT`]) is the transport's
//! job — `pandaspy-client` owns the socket. This module owns everything about
//! the bytes that ride on it: which topics to use, what a request payload
//! looks like, and what kind of thing a report payload is.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::ProtoError;
use crate::de;

/// MQTT-over-TLS port every Bambu printer listens on in LAN mode.
pub const MQTT_PORT: u16 = 8883;

/// The only username LAN-mode MQTT accepts. The password is the printer's
/// access code, which lives in `pandaspy-store` — never here.
pub const MQTT_USERNAME: &str = "bblp";

/// Topic the printer publishes state on.
#[must_use]
pub fn report_topic(serial: &str) -> String {
    format!("device/{serial}/report")
}

/// Topic the printer accepts commands on.
#[must_use]
pub fn request_topic(serial: &str) -> String {
    format!("device/{serial}/request")
}

/// The requests PandaSpy sends. Two, and deliberately only two.
///
/// PandaSpy is read-only by design — no pause, no resume, no stop. That is a
/// product decision (see the non-goals in `CLAUDE.md`), and keeping this enum
/// minimal is how the protocol layer enforces it: a command that cannot be
/// expressed cannot be sent by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Request {
    /// Ask for a full state snapshot on the report topic. Sent after every
    /// (re)connect — deltas that arrived while disconnected are gone forever,
    /// so accumulated state is stale until this lands.
    Pushall,
    /// Ask for module firmware versions and serials.
    GetVersion,
}

impl Request {
    /// The JSON payload to publish on [`request_topic`].
    ///
    /// `sequence_id` is echoed back by the printer; callers use it to pair
    /// responses with requests. The wire carries it as a string.
    #[must_use]
    pub fn payload(&self, sequence_id: u64) -> String {
        let sequence_id = sequence_id.to_string();
        let value = match self {
            // `version` and `push_target` are what Bambu's own client sends;
            // some firmwares ignore a pushall without them.
            Self::Pushall => json!({
                "pushing": {
                    "sequence_id": sequence_id,
                    "command": "pushall",
                    "version": 1,
                    "push_target": 1,
                }
            }),
            Self::GetVersion => json!({
                "info": {
                    "sequence_id": sequence_id,
                    "command": "get_version",
                }
            }),
        };
        value.to_string()
    }
}

/// What kind of message arrived on the report topic.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReportKind {
    /// `print.command == "push_status"` — printer state, full or partial.
    /// The only kind [`crate::StateAccumulator`] merges.
    PrintPushStatus,
    /// Some other `print.*` message (`project_file`, acks, …), preserved with
    /// its command name.
    PrintOther(String),
    /// `info.*` — module versions; view it with [`Report::info`].
    Info,
    /// `system.*` — acks for system commands.
    System,
    /// `event.*` notifications.
    Event,
    /// `liveview.*` — camera negotiation. Out of scope for PandaSpy (camera is
    /// a non-goal) but classified so logs can name it.
    Liveview,
    /// `upgrade.*` — firmware upgrade progress.
    Upgrade,
    /// `mc_print.*` — low-level SD-card print chatter.
    McPrint,
    /// An envelope this build has never seen. Not an error — firmware grows
    /// new ones, and a monitor that crashes on novelty is a liability.
    Unknown,
}

/// One message from `device/{serial}/report`, classified but with the raw
/// document retained.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    raw: Value,
    kind: ReportKind,
}

impl Report {
    /// Parse raw payload bytes.
    ///
    /// # Errors
    ///
    /// Only if the bytes are not JSON. Unrecognised envelopes succeed as
    /// [`ReportKind::Unknown`].
    pub fn parse(payload: &[u8]) -> Result<Self, ProtoError> {
        let raw: Value = serde_json::from_slice(payload)?;
        Ok(Self::from_value(raw))
    }

    /// Classify an already-parsed document.
    #[must_use]
    pub fn from_value(raw: Value) -> Self {
        let kind = classify(&raw);
        Self { raw, kind }
    }

    #[must_use]
    pub fn kind(&self) -> &ReportKind {
        &self.kind
    }

    /// The whole document, untouched. This is what fixtures snapshot and what
    /// diagnostics show — never a lossy reconstruction.
    #[must_use]
    pub fn raw(&self) -> &Value {
        &self.raw
    }

    /// The `print` object, when there is one.
    #[must_use]
    pub fn print(&self) -> Option<&Map<String, Value>> {
        self.raw.get("print").and_then(Value::as_object)
    }

    /// The `info` object viewed as a typed version report, when there is one.
    #[must_use]
    pub fn info(&self) -> Option<InfoReport> {
        let info = self.raw.get("info")?;
        serde_json::from_value(info.clone()).ok()
    }
}

fn classify(raw: &Value) -> ReportKind {
    let Some(object) = raw.as_object() else {
        return ReportKind::Unknown;
    };

    if let Some(print) = object.get("print") {
        let command = print
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return if command == "push_status" {
            ReportKind::PrintPushStatus
        } else {
            ReportKind::PrintOther(command.to_owned())
        };
    }
    if object.contains_key("info") {
        return ReportKind::Info;
    }
    if object.contains_key("system") {
        return ReportKind::System;
    }
    if object.contains_key("event") {
        return ReportKind::Event;
    }
    if object.contains_key("liveview") {
        return ReportKind::Liveview;
    }
    if object.contains_key("upgrade") {
        return ReportKind::Upgrade;
    }
    if object.contains_key("mc_print") {
        return ReportKind::McPrint;
    }
    ReportKind::Unknown
}

/// A `get_version` response: one entry per firmware module.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InfoReport {
    pub command: Option<String>,
    pub sequence_id: Option<String>,
    #[serde(rename = "module")]
    pub modules: Vec<ModuleInfo>,
}

impl InfoReport {
    /// The printer's overall firmware version — the `ota` module's version,
    /// which is what Bambu Studio displays as "the" firmware version.
    #[must_use]
    pub fn firmware_version(&self) -> Option<&str> {
        self.module("ota").and_then(|m| m.sw_ver.as_deref())
    }

    /// The device serial, as reported by the `ota` module (falling back to
    /// any module that carries one).
    #[must_use]
    pub fn serial(&self) -> Option<&str> {
        self.module("ota")
            .and_then(|m| m.sn.as_deref())
            .or_else(|| self.modules.iter().find_map(|m| m.sn.as_deref()))
            .filter(|sn| !sn.is_empty())
    }

    fn module(&self, name: &str) -> Option<&ModuleInfo> {
        self.modules
            .iter()
            .find(|m| m.name.as_deref() == Some(name))
    }
}

/// One firmware module in a `get_version` response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModuleInfo {
    pub name: Option<String>,
    pub sw_ver: Option<String>,
    pub hw_ver: Option<String>,
    pub sn: Option<String>,
    pub loader_ver: Option<String>,
    #[serde(deserialize_with = "de::opt_i64")]
    pub flag: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_follow_the_documented_scheme() {
        assert_eq!(
            report_topic("00M09A000000000"),
            "device/00M09A000000000/report"
        );
        assert_eq!(
            request_topic("00M09A000000000"),
            "device/00M09A000000000/request"
        );
    }

    #[test]
    fn pushall_payload_matches_what_the_firmware_expects() {
        let payload: Value = serde_json::from_str(&Request::Pushall.payload(7)).unwrap();
        assert_eq!(
            payload,
            serde_json::json!({
                "pushing": {
                    "sequence_id": "7",
                    "command": "pushall",
                    "version": 1,
                    "push_target": 1,
                }
            })
        );
    }

    #[test]
    fn get_version_payload_is_the_info_envelope() {
        let payload: Value = serde_json::from_str(&Request::GetVersion.payload(0)).unwrap();
        assert_eq!(payload["info"]["command"], "get_version");
        assert_eq!(payload["info"]["sequence_id"], "0");
    }

    #[test]
    fn push_status_is_recognised() {
        let report = Report::parse(br#"{"print": {"command": "push_status", "msg": 1}}"#).unwrap();
        assert_eq!(*report.kind(), ReportKind::PrintPushStatus);
        assert!(report.print().is_some());
    }

    #[test]
    fn other_print_commands_keep_their_name_and_are_not_state() {
        let report = Report::parse(
            br#"{"print": {"command": "project_file", "param": "Metadata/plate_1.gcode"}}"#,
        )
        .unwrap();
        assert_eq!(
            *report.kind(),
            ReportKind::PrintOther("project_file".to_owned())
        );
    }

    #[test]
    fn a_novel_envelope_is_unknown_not_an_error() {
        let report = Report::parse(br#"{"holo_projector": {"engaged": true}}"#).unwrap();
        assert_eq!(*report.kind(), ReportKind::Unknown);
    }

    #[test]
    fn non_json_is_the_only_parse_failure() {
        assert!(Report::parse(b"\x00\x01not json").is_err());
    }

    #[test]
    fn info_reports_expose_firmware_and_serial() {
        let report = Report::parse(
            br#"{"info": {"command": "get_version", "module": [
                {"name": "ota", "sw_ver": "01.08.02.00", "sn": "00M09A000000000"},
                {"name": "esp32", "sw_ver": "01.13.12.66", "sn": ""}
            ]}}"#,
        )
        .unwrap();

        assert_eq!(*report.kind(), ReportKind::Info);
        let info = report.info().unwrap();
        assert_eq!(info.firmware_version(), Some("01.08.02.00"));
        assert_eq!(info.serial(), Some("00M09A000000000"));
    }
}
