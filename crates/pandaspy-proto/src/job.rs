//! Job vocabulary: `gcode_state`, the `stg_cur` stage table, and the
//! simplified status the UI keys off.

use serde::{Deserialize, Serialize};

/// The printer's `gcode_state` — its own word for what the job is doing.
///
/// Unknown values are preserved verbatim: firmware adds states, and a build
/// from last month must still show *something* honest when it meets one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
#[non_exhaustive]
pub enum GcodeState {
    Idle,
    Prepare,
    Slicing,
    Running,
    Pause,
    Finish,
    Failed,
    /// A state string this build does not know about, kept as reported.
    Unknown(String),
}

impl GcodeState {
    /// The exact string the printer used.
    #[must_use]
    pub fn as_wire_str(&self) -> &str {
        match self {
            Self::Idle => "IDLE",
            Self::Prepare => "PREPARE",
            Self::Slicing => "SLICING",
            Self::Running => "RUNNING",
            Self::Pause => "PAUSE",
            Self::Finish => "FINISH",
            Self::Failed => "FAILED",
            Self::Unknown(raw) => raw,
        }
    }

    /// Collapse to the five states a monitoring UI actually renders.
    #[must_use]
    pub fn status(&self) -> PrinterStatus {
        match self {
            Self::Idle => PrinterStatus::Idle,
            Self::Prepare | Self::Slicing => PrinterStatus::Preparing,
            Self::Running => PrinterStatus::Printing,
            Self::Pause => PrinterStatus::Paused,
            Self::Finish => PrinterStatus::Finished,
            Self::Failed => PrinterStatus::Failed,
            Self::Unknown(_) => PrinterStatus::Unknown,
        }
    }
}

impl From<String> for GcodeState {
    fn from(raw: String) -> Self {
        match raw.as_str() {
            "IDLE" => Self::Idle,
            "PREPARE" => Self::Prepare,
            "SLICING" => Self::Slicing,
            "RUNNING" => Self::Running,
            "PAUSE" => Self::Pause,
            "FINISH" => Self::Finish,
            "FAILED" => Self::Failed,
            _ => Self::Unknown(raw),
        }
    }
}

impl From<GcodeState> for String {
    fn from(state: GcodeState) -> Self {
        state.as_wire_str().to_owned()
    }
}

/// The rendered summary of a printer: what a tray icon or card headline says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PrinterStatus {
    Idle,
    Preparing,
    Printing,
    Paused,
    Finished,
    Failed,
    Unknown,
}

/// The `stg_cur` sub-stage — what the machine is physically doing right now,
/// at a finer grain than [`GcodeState`] ("RUNNING" covers both extruding and
/// bed levelling; this is how you tell them apart).
///
/// The table is the community-documented mapping (OpenBambuAPI); values not
/// in it are preserved as [`PrintStage::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PrintStage {
    Idle,
    Printing,
    AutoBedLeveling,
    HeatbedPreheating,
    SweepingXyMechMode,
    ChangingFilament,
    M400Pause,
    PausedFilamentRunout,
    HeatingHotend,
    CalibratingExtrusion,
    ScanningBedSurface,
    InspectingFirstLayer,
    IdentifyingBuildPlate,
    CalibratingMicroLidar,
    HomingToolhead,
    CleaningNozzleTip,
    CheckingExtruderTemperature,
    PausedByUser,
    PausedFrontCoverFalling,
    RecalibratingMicroLidar,
    CalibratingExtrusionFlow,
    PausedNozzleTemperatureMalfunction,
    PausedHeatBedTemperatureMalfunction,
    FilamentUnloading,
    PausedSkippedStep,
    FilamentLoading,
    CalibratingMotorNoise,
    PausedAmsLost,
    PausedLowFanSpeedHeatBreak,
    PausedChamberTemperatureControlError,
    CoolingChamber,
    PausedUserGcode,
    MotorNoiseShowoff,
    PausedNozzleFilamentCoveredDetected,
    PausedCutterError,
    PausedFirstLayerError,
    PausedNozzleClog,
    /// A stage id this build does not know about, preserved as reported.
    Unknown(i64),
}

impl PrintStage {
    /// Decode a raw `stg_cur` value.
    #[must_use]
    pub fn from_stg_cur(raw: i64) -> Self {
        match raw {
            -1 => Self::Idle,
            0 => Self::Printing,
            1 => Self::AutoBedLeveling,
            2 => Self::HeatbedPreheating,
            3 => Self::SweepingXyMechMode,
            4 => Self::ChangingFilament,
            5 => Self::M400Pause,
            6 => Self::PausedFilamentRunout,
            7 => Self::HeatingHotend,
            8 => Self::CalibratingExtrusion,
            9 => Self::ScanningBedSurface,
            10 => Self::InspectingFirstLayer,
            11 => Self::IdentifyingBuildPlate,
            12 => Self::CalibratingMicroLidar,
            13 => Self::HomingToolhead,
            14 => Self::CleaningNozzleTip,
            15 => Self::CheckingExtruderTemperature,
            16 => Self::PausedByUser,
            17 => Self::PausedFrontCoverFalling,
            18 => Self::RecalibratingMicroLidar,
            19 => Self::CalibratingExtrusionFlow,
            20 => Self::PausedNozzleTemperatureMalfunction,
            21 => Self::PausedHeatBedTemperatureMalfunction,
            22 => Self::FilamentUnloading,
            23 => Self::PausedSkippedStep,
            24 => Self::FilamentLoading,
            25 => Self::CalibratingMotorNoise,
            26 => Self::PausedAmsLost,
            27 => Self::PausedLowFanSpeedHeatBreak,
            28 => Self::PausedChamberTemperatureControlError,
            29 => Self::CoolingChamber,
            30 => Self::PausedUserGcode,
            31 => Self::MotorNoiseShowoff,
            32 => Self::PausedNozzleFilamentCoveredDetected,
            33 => Self::PausedCutterError,
            34 => Self::PausedFirstLayerError,
            35 => Self::PausedNozzleClog,
            other => Self::Unknown(other),
        }
    }

    /// A stable machine-readable name, usable as a Fluent key suffix when the
    /// UI localises stages. `Unknown` has no name — render the raw number.
    #[must_use]
    pub fn key(&self) -> Option<&'static str> {
        Some(match self {
            Self::Idle => "idle",
            Self::Printing => "printing",
            Self::AutoBedLeveling => "auto-bed-leveling",
            Self::HeatbedPreheating => "heatbed-preheating",
            Self::SweepingXyMechMode => "sweeping-xy-mech-mode",
            Self::ChangingFilament => "changing-filament",
            Self::M400Pause => "m400-pause",
            Self::PausedFilamentRunout => "paused-filament-runout",
            Self::HeatingHotend => "heating-hotend",
            Self::CalibratingExtrusion => "calibrating-extrusion",
            Self::ScanningBedSurface => "scanning-bed-surface",
            Self::InspectingFirstLayer => "inspecting-first-layer",
            Self::IdentifyingBuildPlate => "identifying-build-plate",
            Self::CalibratingMicroLidar => "calibrating-micro-lidar",
            Self::HomingToolhead => "homing-toolhead",
            Self::CleaningNozzleTip => "cleaning-nozzle-tip",
            Self::CheckingExtruderTemperature => "checking-extruder-temperature",
            Self::PausedByUser => "paused-by-user",
            Self::PausedFrontCoverFalling => "paused-front-cover-falling",
            Self::RecalibratingMicroLidar => "recalibrating-micro-lidar",
            Self::CalibratingExtrusionFlow => "calibrating-extrusion-flow",
            Self::PausedNozzleTemperatureMalfunction => "paused-nozzle-temperature-malfunction",
            Self::PausedHeatBedTemperatureMalfunction => "paused-heat-bed-temperature-malfunction",
            Self::FilamentUnloading => "filament-unloading",
            Self::PausedSkippedStep => "paused-skipped-step",
            Self::FilamentLoading => "filament-loading",
            Self::CalibratingMotorNoise => "calibrating-motor-noise",
            Self::PausedAmsLost => "paused-ams-lost",
            Self::PausedLowFanSpeedHeatBreak => "paused-low-fan-speed-heat-break",
            Self::PausedChamberTemperatureControlError => {
                "paused-chamber-temperature-control-error"
            }
            Self::CoolingChamber => "cooling-chamber",
            Self::PausedUserGcode => "paused-user-gcode",
            Self::MotorNoiseShowoff => "motor-noise-showoff",
            Self::PausedNozzleFilamentCoveredDetected => "paused-nozzle-filament-covered-detected",
            Self::PausedCutterError => "paused-cutter-error",
            Self::PausedFirstLayerError => "paused-first-layer-error",
            Self::PausedNozzleClog => "paused-nozzle-clog",
            Self::Unknown(_) => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_gcode_states_survive_a_round_trip() {
        let state: GcodeState = serde_json::from_str("\"ANNEALING\"").unwrap();
        assert_eq!(state, GcodeState::Unknown("ANNEALING".to_owned()));
        assert_eq!(state.status(), PrinterStatus::Unknown);
        assert_eq!(serde_json::to_string(&state).unwrap(), "\"ANNEALING\"");
    }

    #[test]
    fn gcode_states_collapse_to_ui_statuses() {
        assert_eq!(GcodeState::Running.status(), PrinterStatus::Printing);
        assert_eq!(GcodeState::Prepare.status(), PrinterStatus::Preparing);
        assert_eq!(GcodeState::Pause.status(), PrinterStatus::Paused);
    }

    #[test]
    fn the_stage_table_covers_the_documented_range() {
        assert_eq!(PrintStage::from_stg_cur(-1), PrintStage::Idle);
        assert_eq!(PrintStage::from_stg_cur(0), PrintStage::Printing);
        assert_eq!(PrintStage::from_stg_cur(16), PrintStage::PausedByUser);
        assert_eq!(PrintStage::from_stg_cur(35), PrintStage::PausedNozzleClog);
    }

    #[test]
    fn a_novel_stage_id_is_preserved_not_flattened() {
        let stage = PrintStage::from_stg_cur(99);
        assert_eq!(stage, PrintStage::Unknown(99));
        assert_eq!(stage.key(), None, "no name means render the raw number");
    }

    #[test]
    fn every_known_stage_has_a_stable_key() {
        for id in -1..=35 {
            let stage = PrintStage::from_stg_cur(id);
            assert!(stage.key().is_some(), "stage {id} has no key");
        }
    }
}
