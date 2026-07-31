use std::future::Future;
use std::io;
use std::net::SocketAddr;

/// A multicast socket, as much of one as discovery needs.
///
/// Returning `impl Future + Send` rather than writing `async fn` is deliberate:
/// `async fn` in a public trait leaves the future's auto-traits unspecified,
/// which means callers cannot spawn the result on a multi-threaded runtime.
/// Spelling out `+ Send` here keeps that option open for `src-tauri`.
///
/// The trade-off is that the trait is not `dyn`-safe. Discovery is generic over
/// the transport instead, which is fine — there is exactly one real
/// implementation and one test double.
pub trait SsdpSocket: Send + Sync + std::fmt::Debug {
    /// Send an M-SEARCH (or equivalent) to the multicast group.
    fn search(&self, request: &[u8]) -> impl Future<Output = io::Result<()>> + Send;

    /// Await the next datagram. Returns bytes written and the sender.
    fn recv(&self, buf: &mut [u8]) -> impl Future<Output = io::Result<(usize, SocketAddr)>> + Send;
}

// TODO(scaffold): the SSDP search loop goes here. It should be a plain function
// generic over `S: SsdpSocket` that returns `Vec<DiscoveredPrinter>`, so a test
// can drive it with a scripted socket and assert on the parsed results.

#[cfg(test)]
mod tests {
    use super::*;

    /// A socket that answers from a script. The point of the trait seam: this
    /// compiles and runs with no network and no async runtime in sight.
    #[derive(Debug, Default)]
    struct ScriptedSocket {
        _responses: Vec<(Vec<u8>, SocketAddr)>,
    }

    impl SsdpSocket for ScriptedSocket {
        async fn search(&self, _request: &[u8]) -> io::Result<()> {
            Ok(())
        }

        async fn recv(&self, _buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
            Err(io::Error::from(io::ErrorKind::WouldBlock))
        }
    }

    fn accepts_any_socket<S: SsdpSocket>(_socket: &S) {}

    #[test]
    fn a_test_double_satisfies_the_transport_seam() {
        accepts_any_socket(&ScriptedSocket::default());
    }
}
