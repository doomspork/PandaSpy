use thiserror::Error;

/// Everything that can go wrong turning printer bytes into domain types.
///
/// Deliberately small. An unrecognised field, an unrecognised enum value or a
/// missing key is *not* an error — see the crate-level parsing discipline.
/// Only bytes that are not JSON at all are unusable; even an envelope this
/// build has never seen classifies as [`crate::ReportKind::Unknown`] rather
/// than failing.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProtoError {
    /// The payload was not valid JSON, or the accumulated document could not
    /// be viewed as typed state at a level we cannot recover from.
    #[error("malformed payload: {0}")]
    Malformed(#[from] serde_json::Error),
}
