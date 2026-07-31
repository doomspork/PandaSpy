use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;

/// "Is something listening there?", abstracted so the subnet walk can be tested
/// against a map of addresses instead of a real /24.
pub trait PortProbe: Send + Sync + std::fmt::Debug {
    /// Resolves to `true` if a TCP connection was established within `timeout`.
    ///
    /// A probe never errors: from the walk's point of view, "refused",
    /// "unreachable" and "timed out" are all just "no".
    fn probe(&self, target: SocketAddr, timeout: Duration) -> impl Future<Output = bool> + Send;
}

// TODO(scaffold): the subnet walk goes here. Keep it generic over
// `P: PortProbe`, bounded in concurrency, and cancellable — it is the slow path
// and users will close the window mid-scan.
