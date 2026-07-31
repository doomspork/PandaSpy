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

/// Printer model, covering the machines PandaSpy targets.
///
/// Recognition works from the identifiers printers actually put on the wire:
/// the product code carried in Bambu Studio configs and some announcements
/// (`C11`, `N2S`, …) and the SSDP `DevModel` strings
/// (`3DPrinter-X1-Carbon`, …).
///
/// # The `Unknown` variant is the point
///
/// Bambu ships new models faster than we ship releases. A model string we do
/// not recognise survives a decode/encode round-trip unchanged so that logs,
/// fixtures and the UI can all show the raw value rather than a lie.
///
/// TODO(fixture): the alias table below is assembled from community-documented
/// identifiers, not from captures. Confirm each against a real announcement or
/// Studio config as recordings arrive.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
#[non_exhaustive]
pub enum PrinterModel {
    X1,
    X1Carbon,
    X1E,
    P1P,
    P1S,
    A1,
    A1Mini,
    H2D,
    /// Value reported by the printer that this build does not know about.
    /// Preserved verbatim.
    Unknown(String),
}

impl PrinterModel {
    /// The canonical product code, or the raw string for [`Self::Unknown`].
    #[must_use]
    pub fn product_code(&self) -> &str {
        match self {
            Self::X1 => "BL-P002",
            Self::X1Carbon => "BL-P001",
            Self::X1E => "C13",
            Self::P1P => "C11",
            Self::P1S => "C12",
            Self::A1 => "N2S",
            Self::A1Mini => "N1",
            Self::H2D => "O1D",
            Self::Unknown(raw) => raw,
        }
    }

    /// Human-readable name, matching Bambu's marketing capitalisation.
    #[must_use]
    pub fn display_name(&self) -> &str {
        match self {
            Self::X1 => "X1",
            Self::X1Carbon => "X1 Carbon",
            Self::X1E => "X1E",
            Self::P1P => "P1P",
            Self::P1S => "P1S",
            Self::A1 => "A1",
            Self::A1Mini => "A1 mini",
            Self::H2D => "H2D",
            Self::Unknown(raw) => raw,
        }
    }

    /// Whether this build recognises the model.
    #[must_use]
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }

    /// Whether the model has an enclosed, temperature-reporting chamber.
    ///
    /// Drives whether the UI shows a chamber temperature at all — an open
    /// A1 reporting nothing is normal, not missing data.
    #[must_use]
    pub fn has_chamber(&self) -> Option<bool> {
        match self {
            Self::X1 | Self::X1Carbon | Self::X1E | Self::P1S | Self::H2D => Some(true),
            Self::P1P | Self::A1 | Self::A1Mini => Some(false),
            Self::Unknown(_) => None,
        }
    }
}

impl From<String> for PrinterModel {
    fn from(raw: String) -> Self {
        // Product codes first, then the SSDP `DevModel` strings. Matching is
        // exact on purpose: a fuzzy match that guesses wrong is worse than an
        // honest Unknown.
        match raw.as_str() {
            "BL-P002" | "3DPrinter-X1" => Self::X1,
            "BL-P001" | "3DPrinter-X1-Carbon" => Self::X1Carbon,
            "C13" => Self::X1E,
            "C11" => Self::P1P,
            "C12" => Self::P1S,
            "N2S" => Self::A1,
            "N1" => Self::A1Mini,
            "O1D" => Self::H2D,
            _ => Self::Unknown(raw),
        }
    }
}

impl From<PrinterModel> for String {
    fn from(model: PrinterModel) -> Self {
        model.product_code().to_owned()
    }
}

impl fmt::Display for PrinterModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_codes_and_ssdp_strings_resolve_to_the_same_model() {
        assert_eq!(
            PrinterModel::from("BL-P001".to_owned()),
            PrinterModel::X1Carbon
        );
        assert_eq!(
            PrinterModel::from("3DPrinter-X1-Carbon".to_owned()),
            PrinterModel::X1Carbon
        );
        assert_eq!(PrinterModel::from("C12".to_owned()), PrinterModel::P1S);
        assert_eq!(PrinterModel::from("N1".to_owned()), PrinterModel::A1Mini);
        assert_eq!(PrinterModel::from("O1D".to_owned()), PrinterModel::H2D);
    }

    #[test]
    fn unknown_models_round_trip_verbatim() {
        // A firmware that starts reporting "X2-Ultra" must not become
        // "unknown printer" in a lossy way, and far less a parse error.
        let json = "\"X2-Ultra\"";
        let model: PrinterModel = serde_json::from_str(json).unwrap();

        assert_eq!(model, PrinterModel::Unknown("X2-Ultra".to_owned()));
        assert!(!model.is_known());
        assert_eq!(
            model.has_chamber(),
            None,
            "no guessing about unknown hardware"
        );
        assert_eq!(serde_json::to_string(&model).unwrap(), json);
    }

    #[test]
    fn known_models_serialise_to_their_canonical_product_code() {
        assert_eq!(
            serde_json::to_string(&PrinterModel::P1S).unwrap(),
            "\"C12\""
        );
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
