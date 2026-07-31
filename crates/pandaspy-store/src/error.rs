use thiserror::Error;

/// Why a read or write against durable storage failed.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The config file exists but could not be read or written.
    #[error("config i/o failed: {0}")]
    Io(String),

    /// The config file is present but not something this build understands.
    /// Never silently discard it — a user's printer list is not disposable.
    #[error("config at {path} is not readable by this version: {reason}")]
    Corrupt { path: String, reason: String },

    /// The OS secret store refused, or is not available (a Linux session with
    /// no Secret Service, a locked keychain).
    #[error("secret backend unavailable: {0}")]
    SecretBackend(String),

    /// The requested secret is simply not there.
    #[error("no secret stored for {0}")]
    SecretMissing(String),
}
