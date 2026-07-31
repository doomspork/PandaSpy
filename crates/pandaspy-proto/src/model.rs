use std::fmt;

use serde::{Deserialize, Serialize};

/// A printer's serial number, as reported by the device.
///
/// Serials identify a physical machine and appear in MQTT topics, so they are
/// personal-ish data: redact them before committing a fixture.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceSerial(pub String);

impl fmt::Display for DeviceSerial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for DeviceSerial {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// Printer model, as advertised by the device.
///
/// # The `Unknown` variant is the point
///
/// Bambu ships new models faster than we ship releases. A model string we do
/// not recognise must survive a decode/encode round-trip unchanged so that
/// logs, fixtures and the UI can all show the raw value rather than a lie.
///
/// The `from`/`into` attributes are what make that work: serde treats this enum
/// as a plain string on the wire, and the conversion functions below decide how
/// to interpret it. Adding `#[serde(other)]` instead would collapse every
/// unknown model into one indistinguishable variant.
///
/// TODO(scaffold): the known-model list is a placeholder. Populate it from
/// recorded fixtures, not from guesswork.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
#[non_exhaustive]
pub enum PrinterModel {
    /// Value reported by the printer that this build does not know about.
    /// Preserved verbatim.
    Unknown(String),
}

impl PrinterModel {
    /// The exact string the printer used. Never lossy, even for [`Self::Unknown`].
    #[must_use]
    pub fn as_wire_str(&self) -> &str {
        match self {
            Self::Unknown(raw) => raw,
        }
    }

    /// Whether this build recognises the model.
    ///
    /// Useful for telemetry-free diagnostics: "we saw a printer we do not know
    /// about" is worth surfacing in the UI, quietly.
    #[must_use]
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

impl From<String> for PrinterModel {
    fn from(raw: String) -> Self {
        // TODO(scaffold): match known model identifiers here first.
        Self::Unknown(raw)
    }
}

impl From<PrinterModel> for String {
    fn from(model: PrinterModel) -> Self {
        match model {
            PrinterModel::Unknown(raw) => raw,
        }
    }
}

impl fmt::Display for PrinterModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_models_round_trip_verbatim() {
        // The whole reason `Unknown` carries a String: a firmware that starts
        // reporting "H2D" must not become "unknown printer" in the UI or, far
        // worse, a hard parse error.
        let json = "\"H2D\"";
        let model: PrinterModel = serde_json::from_str(json).unwrap();

        assert_eq!(model, PrinterModel::Unknown("H2D".to_owned()));
        assert_eq!(model.as_wire_str(), "H2D");
        assert!(!model.is_known());
        assert_eq!(serde_json::to_string(&model).unwrap(), json);
    }

    #[test]
    fn device_serial_is_transparent_on_the_wire() {
        let serial: DeviceSerial = serde_json::from_str("\"00M09A000000000\"").unwrap();
        assert_eq!(serial.to_string(), "00M09A000000000");
        assert_eq!(
            serde_json::to_string(&serial).unwrap(),
            "\"00M09A000000000\""
        );
    }
}
