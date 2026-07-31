use thiserror::Error;

/// Why a discovery attempt could not be completed.
///
/// "Found nothing" is not an error — it is an empty result. Only a broken
/// transport belongs here.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DiscoveryError {
    /// The transport failed outright: no multicast route, permission denied,
    /// interface disappeared mid-scan.
    #[error("discovery transport failed: {0}")]
    Transport(String),

    /// A device answered, but with something we could not turn into an address.
    ///
    /// NB: the field is `origin`, not `source`. `thiserror` treats a field
    /// literally named `source` as the error-chain parent and requires it to
    /// implement `std::error::Error`.
    #[error("unusable announcement from {origin}: {reason}")]
    Unusable { origin: String, reason: String },
}
