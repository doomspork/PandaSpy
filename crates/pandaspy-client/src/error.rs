use thiserror::Error;

/// Why a session could not be established or could not continue.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ClientError {
    /// Network-level failure: refused, unreachable, reset.
    #[error("connection failed: {0}")]
    Connect(String),

    /// The printer's certificate did not match the pinned fingerprint. This is
    /// never retried automatically — a changed certificate is either a
    /// firmware reflash or someone in the middle, and only the user can say
    /// which.
    #[error("certificate for {serial} does not match the pinned fingerprint")]
    CertificateChanged { serial: String },

    /// The printer rejected our credentials — usually a stale access code
    /// after a factory reset.
    #[error("printer rejected the access code")]
    Unauthorised,

    /// A payload arrived that `pandaspy-proto` could not use.
    #[error(transparent)]
    Protocol(#[from] pandaspy_proto::ProtoError),
}
