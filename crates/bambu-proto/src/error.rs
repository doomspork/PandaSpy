use thiserror::Error;

/// Everything that can go wrong turning printer bytes into domain types.
///
/// Deliberately small. An unrecognised field, an unrecognised enum value or a
/// missing key is *not* an error — see the crate-level parsing discipline.
/// Only genuinely unusable input belongs in here.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProtoError {
    /// The payload was not valid JSON, or did not match the expected shape at
    /// a level we cannot recover from.
    #[error("malformed payload: {0}")]
    Malformed(#[from] serde_json::Error),

    /// The payload was valid JSON but not a recognised message envelope.
    #[error("unrecognised message envelope")]
    UnknownEnvelope,
}
