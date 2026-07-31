use serde::{Deserialize, Serialize};

use crate::model::{DeviceSerial, PrinterModel};

/// What the printer is currently doing.
///
/// Like [`PrinterModel`], unknown values are preserved verbatim rather than
/// rejected. Firmware adds stages; a build from last month must still show
/// *something* sensible when it meets one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
#[non_exhaustive]
pub enum JobStage {
    Idle,
    Running,
    Paused,
    Finished,
    Failed,
    /// A stage string this build does not know about, kept as reported.
    Unknown(String),
}

impl JobStage {
    /// The exact string the printer used.
    #[must_use]
    pub fn as_wire_str(&self) -> &str {
        match self {
            Self::Idle => "IDLE",
            Self::Running => "RUNNING",
            Self::Paused => "PAUSE",
            Self::Finished => "FINISH",
            Self::Failed => "FAILED",
            Self::Unknown(raw) => raw,
        }
    }
}

impl From<String> for JobStage {
    fn from(raw: String) -> Self {
        match raw.as_str() {
            "IDLE" => Self::Idle,
            "RUNNING" => Self::Running,
            "PAUSE" => Self::Paused,
            "FINISH" => Self::Finished,
            "FAILED" => Self::Failed,
            _ => Self::Unknown(raw),
        }
    }
}

impl From<JobStage> for String {
    fn from(stage: JobStage) -> Self {
        match stage {
            JobStage::Unknown(raw) => raw,
            other => other.as_wire_str().to_owned(),
        }
    }
}

/// Everything PandaSpy knows about one printer at one instant.
///
/// # Why every field is `Option`
///
/// The printer sends one large report on connect and then a stream of partial
/// reports containing only what changed. `None` means "the printer did not
/// mention this", which is different from "the printer said zero". Collapsing
/// the two would make a paused print look like a finished one.
///
/// TODO(scaffold): this field set is a placeholder that exists to exercise the
/// merge and the fixture harness. Grow it from recorded fixtures — add a
/// fixture first, then the field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
// NB: no `deny_unknown_fields`. Ever. See the crate docs.
pub struct PrinterState {
    pub serial: Option<DeviceSerial>,
    pub model: Option<PrinterModel>,
    pub stage: Option<JobStage>,
    pub nozzle_temperature_c: Option<f64>,
    pub bed_temperature_c: Option<f64>,
    pub progress_percent: Option<u8>,
}

impl PrinterState {
    /// Fold a partial report into this state.
    ///
    /// Fields the delta does not mention are left alone; fields it does mention
    /// overwrite. This is the only place printer state is allowed to change, so
    /// that the "did the printer really say that?" question has one answer.
    pub fn merge_from(&mut self, delta: &Self) {
        // Destructured exhaustively on purpose: adding a field to
        // `PrinterState` will fail to compile until it is handled here, which
        // is exactly the reminder you want when the protocol grows.
        let Self {
            serial,
            model,
            stage,
            nozzle_temperature_c,
            bed_temperature_c,
            progress_percent,
        } = delta;

        if serial.is_some() {
            self.serial = serial.clone();
        }
        if model.is_some() {
            self.model = model.clone();
        }
        if stage.is_some() {
            self.stage = stage.clone();
        }
        if nozzle_temperature_c.is_some() {
            self.nozzle_temperature_c = *nozzle_temperature_c;
        }
        if bed_temperature_c.is_some() {
            self.bed_temperature_c = *bed_temperature_c;
        }
        if progress_percent.is_some() {
            self.progress_percent = *progress_percent;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_fields_in_a_delta_do_not_clobber_known_state() {
        let mut state = PrinterState {
            stage: Some(JobStage::Running),
            nozzle_temperature_c: Some(220.0),
            progress_percent: Some(42),
            ..PrinterState::default()
        };

        // A partial report mentioning only the nozzle.
        let delta = PrinterState {
            nozzle_temperature_c: Some(221.5),
            ..PrinterState::default()
        };
        state.merge_from(&delta);

        assert_eq!(state.nozzle_temperature_c, Some(221.5));
        assert_eq!(state.stage, Some(JobStage::Running), "stage was clobbered");
        assert_eq!(state.progress_percent, Some(42), "progress was clobbered");
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
            serde_json::from_str(r#"{"progress_percent":7,"chamber_fan_rpm":1200}"#).unwrap();
        assert_eq!(state.progress_percent, Some(7));
    }

    #[test]
    fn unknown_stages_survive_a_round_trip() {
        let state: PrinterState = serde_json::from_str(r#"{"stage":"PREPARE"}"#).unwrap();
        assert_eq!(state.stage, Some(JobStage::Unknown("PREPARE".to_owned())));
        assert!(serde_json::to_string(&state).unwrap().contains("PREPARE"));
    }
}
