//! The SSDP dialect Bambu printers speak.
//!
//! Printers announce themselves with HTTPU `NOTIFY` datagrams to the standard
//! SSDP multicast group `239.255.255.250` — but on port **2021**, not 1900.
//! They also answer `M-SEARCH` queries sent to that group, with an
//! `HTTP/1.1 200 OK` unicast back to the querying socket. The interesting
//! payload is carried in Bambu's custom headers:
//!
//! ```text
//! NOTIFY * HTTP/1.1
//! HOST: 239.255.255.250:2021
//! Server: UPnP/1.0
//! Location: 192.168.0.2
//! NT: urn:bambulab-com:device:3dprinter:1
//! USN: 00M09A000000000
//! Cache-Control: max-age=1800
//! DevModel.bambu.com: 3DPrinter-X1-Carbon
//! DevName.bambu.com: REDACTED-NAME
//! DevSignal.bambu.com: -44
//! DevConnect.bambu.com: lan
//! DevBind.bambu.com: free
//! ```
//!
//! `USN` is the device serial, `Location` its IP. Everything in this module
//! is pure — bytes in, structured announcement out — so the whole dialect is
//! testable from `fixtures/ssdp/` without a socket in sight.

use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use pandaspy_proto::{DeviceSerial, PrinterModel};
use serde::Serialize;

use crate::types::{DiscoveredPrinter, DiscoverySource};

/// The standard SSDP multicast group.
pub const SSDP_MULTICAST_V4: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);

/// Bambu's SSDP port. Deliberately not 1900 — the printers run their own
/// announcer beside any standard UPnP stack.
pub const SSDP_PORT: u16 = 2021;

/// The group:port pair as a socket address.
#[must_use]
pub fn multicast_target() -> SocketAddr {
    SocketAddr::from((SSDP_MULTICAST_V4, SSDP_PORT))
}

/// The search target printers answer to.
///
/// TODO(fixture): taken from community documentation; confirm against a real
/// M-SEARCH capture from Bambu Studio.
pub const SEARCH_TARGET: &str = "urn:bambulab-com:device:3dprinter:1";

/// The M-SEARCH datagram we transmit. Deterministic, CRLF line endings.
#[must_use]
pub fn m_search_payload() -> Vec<u8> {
    format!(
        "M-SEARCH * HTTP/1.1\r\n\
         HOST: {}:{}\r\n\
         MAN: \"ssdp:discover\"\r\n\
         MX: 3\r\n\
         ST: {}\r\n\
         \r\n",
        SSDP_MULTICAST_V4, SSDP_PORT, SEARCH_TARGET
    )
    .into_bytes()
}

/// What kind of SSDP message a datagram was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum SsdpMessageKind {
    /// Unsolicited announcement (`NOTIFY * HTTP/1.1`).
    Notify,
    /// Reply to an M-SEARCH (`HTTP/1.1 200 OK`).
    SearchResponse,
    /// Somebody's query (possibly our own, echoed back by multicast
    /// loopback). Not an announcement; counted but never yields a printer.
    MSearch,
    /// Valid HTTPU shape, unrecognised start line.
    Unknown,
}

/// A parsed SSDP datagram.
///
/// Headers are kept in full (keys lowercased, values trimmed) so diagnostics
/// can show exactly what a printer said, including headers this build does
/// not interpret yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SsdpAnnouncement {
    pub kind: SsdpMessageKind,
    pub headers: BTreeMap<String, String>,
}

impl SsdpAnnouncement {
    /// Parse a datagram. `None` means "not SSDP at all" — binary junk or
    /// something with no start line. Unrecognised but well-formed messages
    /// parse as [`SsdpMessageKind::Unknown`] instead, per the crate-wide
    /// tolerance discipline.
    #[must_use]
    pub fn parse(datagram: &[u8]) -> Option<Self> {
        // SSDP is ASCII-ish; tolerate stray bytes rather than rejecting the
        // whole datagram over one bad character.
        let text = String::from_utf8_lossy(datagram);
        let mut lines = text.split('\n').map(|line| line.trim_end_matches('\r'));

        let start = lines.next()?.trim();
        if start.is_empty() {
            return None;
        }

        let upper = start.to_ascii_uppercase();
        let kind = if upper.starts_with("NOTIFY") {
            SsdpMessageKind::Notify
        } else if upper.starts_with("HTTP/") && upper.contains(" 200") {
            SsdpMessageKind::SearchResponse
        } else if upper.starts_with("M-SEARCH") {
            SsdpMessageKind::MSearch
        } else if upper.contains("HTTP/") {
            SsdpMessageKind::Unknown
        } else {
            // No HTTP-shaped start line: this is not SSDP.
            return None;
        };

        let mut headers = BTreeMap::new();
        for line in lines {
            if line.trim().is_empty() {
                break; // end of headers; SSDP has no body worth reading
            }
            let Some((key, value)) = line.split_once(':') else {
                continue; // tolerate a malformed line rather than bailing
            };
            headers.insert(key.trim().to_ascii_lowercase(), value.trim().to_owned());
        }

        Some(Self { kind, headers })
    }

    /// Whether this is a message that can describe a printer.
    #[must_use]
    pub fn is_announcement(&self) -> bool {
        matches!(
            self.kind,
            SsdpMessageKind::Notify | SsdpMessageKind::SearchResponse
        )
    }

    fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// The printer's IP from `Location`. Tolerates the documented bare-IP
    /// form plus `http[s]://ip[:port]` and bracketed-IPv6 variants seen from
    /// other UPnP stacks. Scheme matching is case-insensitive because URL
    /// schemes are (`HTTP://` is as valid as `http://`).
    #[must_use]
    pub fn location_ip(&self) -> Option<IpAddr> {
        let raw = self.header("location")?;
        let raw = strip_scheme(raw);
        let host = raw.split('/').next()?;

        // Bracketed IPv6: `[2001:db8::1]` or `[2001:db8::1]:8883`.
        if let Some(rest) = host.strip_prefix('[') {
            let inside = rest.split(']').next()?;
            return inside.parse().ok();
        }
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Some(ip);
        }
        // `ip:port` — only meaningful for IPv4 here (a bare, unbracketed IPv6
        // literal has no unambiguous port form, and rsplit on its colons
        // would mangle it). Accept the split only if the left side parses.
        let (candidate, _port) = host.rsplit_once(':')?;
        candidate.parse().ok()
    }

    /// The device serial from `USN`, shorn of `uuid:`-style prefixes.
    #[must_use]
    pub fn serial(&self) -> Option<DeviceSerial> {
        let raw = self.header("usn")?;
        let raw = raw.strip_prefix("uuid:").unwrap_or(raw);
        let serial = raw.split("::").next()?.trim();
        if serial.is_empty() {
            return None;
        }
        Some(DeviceSerial(serial.to_owned()))
    }

    /// `DevModel.bambu.com`, decoded through the shared model table.
    #[must_use]
    pub fn model(&self) -> Option<PrinterModel> {
        self.header("devmodel.bambu.com")
            .map(|raw| PrinterModel::from(raw.to_owned()))
    }

    /// `DevName.bambu.com` — the user's name for the printer.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.header("devname.bambu.com")
    }

    /// `DevSignal.bambu.com` — printer-side Wi-Fi RSSI in dBm.
    #[must_use]
    pub fn signal_dbm(&self) -> Option<i16> {
        self.header("devsignal.bambu.com")?.trim().parse().ok()
    }

    /// `DevConnect.bambu.com` — `"lan"` or `"cloud"`.
    #[must_use]
    pub fn connect_mode(&self) -> Option<&str> {
        self.header("devconnect.bambu.com")
    }

    /// `DevBind.bambu.com` — `"free"` or a binding state.
    #[must_use]
    pub fn bind_state(&self) -> Option<&str> {
        self.header("devbind.bambu.com")
    }

    /// Build a [`DiscoveredPrinter`], falling back to the datagram's sender
    /// address when `Location` is absent or unparsable.
    ///
    /// Returns `None` for non-announcements and for announcements that carry
    /// neither a serial nor anything else usable — a datagram that names
    /// nothing describes nothing.
    #[must_use]
    pub fn to_printer(&self, sender: SocketAddr) -> Option<DiscoveredPrinter> {
        if !self.is_announcement() {
            return None;
        }
        let serial = self.serial();
        if serial.is_none() && self.model().is_none() {
            return None;
        }
        Some(DiscoveredPrinter {
            address: self.location_ip().unwrap_or_else(|| sender.ip()),
            serial,
            model: self.model(),
            name: self.name().map(str::to_owned),
            signal_dbm: self.signal_dbm(),
            connect_mode: self.connect_mode().map(str::to_owned),
            bind_state: self.bind_state().map(str::to_owned),
            source: DiscoverySource::Ssdp,
        })
    }
}

/// Strip a leading `http://` / `https://` regardless of case, leaving
/// everything else untouched.
fn strip_scheme(raw: &str) -> &str {
    let lower = raw.to_ascii_lowercase();
    for scheme in ["http://", "https://"] {
        if lower.starts_with(scheme) {
            return &raw[scheme.len()..];
        }
    }
    raw
}

/// A multicast socket, as much of one as discovery needs.
///
/// Returning `impl Future + Send` rather than writing `async fn` is
/// deliberate: `async fn` in a public trait leaves the future's auto-traits
/// unspecified, so callers could not spawn the result on a multi-threaded
/// runtime. The cost is that the trait is not `dyn`-safe, which is fine —
/// there is one real implementation ([`crate::net`]) and scripted fakes.
pub trait SsdpSocket: Send + Sync + std::fmt::Debug {
    /// Send one prebuilt M-SEARCH datagram to the multicast group.
    fn send_search(&self, payload: &[u8]) -> impl Future<Output = io::Result<()>> + Send;

    /// Await the next datagram. Returns bytes written and the sender.
    fn recv(&self, buf: &mut [u8]) -> impl Future<Output = io::Result<(usize, SocketAddr)>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTIFY: &str = "NOTIFY * HTTP/1.1\r\n\
        HOST: 239.255.255.250:2021\r\n\
        Server: UPnP/1.0\r\n\
        Location: 192.168.0.2\r\n\
        NT: urn:bambulab-com:device:3dprinter:1\r\n\
        USN: 00M09A000000000\r\n\
        DevModel.bambu.com: 3DPrinter-X1-Carbon\r\n\
        DevName.bambu.com: REDACTED-NAME\r\n\
        DevSignal.bambu.com: -44\r\n\
        DevConnect.bambu.com: lan\r\n\
        DevBind.bambu.com: free\r\n\
        \r\n";

    fn sender() -> SocketAddr {
        "192.168.0.2:2021".parse().unwrap()
    }

    #[test]
    fn a_notify_parses_into_a_printer() {
        let announcement = SsdpAnnouncement::parse(NOTIFY.as_bytes()).unwrap();
        assert_eq!(announcement.kind, SsdpMessageKind::Notify);

        let printer = announcement.to_printer(sender()).unwrap();
        assert_eq!(printer.address, "192.168.0.2".parse::<IpAddr>().unwrap());
        assert_eq!(printer.serial.as_ref().unwrap().0, "00M09A000000000");
        assert_eq!(printer.model, Some(PrinterModel::X1Carbon));
        assert_eq!(printer.name.as_deref(), Some("REDACTED-NAME"));
        assert_eq!(printer.signal_dbm, Some(-44));
        assert_eq!(printer.source, DiscoverySource::Ssdp);
    }

    #[test]
    fn header_lookup_is_case_insensitive_and_lf_tolerant() {
        // Same message, LF-only line endings and perverse header casing.
        let scrambled = NOTIFY.replace("\r\n", "\n").replace("USN:", "usn:");
        let announcement = SsdpAnnouncement::parse(scrambled.as_bytes()).unwrap();
        assert_eq!(
            announcement.serial().unwrap().0,
            "00M09A000000000",
            "line endings and casing are presentation, not meaning"
        );
    }

    #[test]
    fn a_search_response_is_an_announcement_too() {
        let response = NOTIFY.replace("NOTIFY * HTTP/1.1", "HTTP/1.1 200 OK");
        let announcement = SsdpAnnouncement::parse(response.as_bytes()).unwrap();
        assert_eq!(announcement.kind, SsdpMessageKind::SearchResponse);
        assert!(announcement.to_printer(sender()).is_some());
    }

    #[test]
    fn our_own_msearch_echo_is_not_a_printer() {
        // Multicast loopback hands us our own query; it must be classified,
        // counted, and never mistaken for a device.
        let announcement = SsdpAnnouncement::parse(&m_search_payload()).unwrap();
        assert_eq!(announcement.kind, SsdpMessageKind::MSearch);
        assert_eq!(announcement.to_printer(sender()), None);
    }

    #[test]
    fn missing_location_falls_back_to_the_sender_address() {
        let no_location = NOTIFY.replace("Location: 192.168.0.2\r\n", "");
        let announcement = SsdpAnnouncement::parse(no_location.as_bytes()).unwrap();
        let printer = announcement.to_printer(sender()).unwrap();
        assert_eq!(printer.address, sender().ip());
    }

    #[test]
    fn url_shaped_locations_still_yield_the_ip() {
        for form in [
            "http://192.168.0.2",
            "http://192.168.0.2:8883/desc",
            "192.168.0.2:2021",
            "HTTP://192.168.0.2",       // scheme case is not meaning
            "HtTpS://192.168.0.2/desc", // …in either scheme
        ] {
            let msg = NOTIFY.replace("Location: 192.168.0.2", &format!("Location: {form}"));
            let announcement = SsdpAnnouncement::parse(msg.as_bytes()).unwrap();
            assert_eq!(
                announcement.location_ip(),
                Some("192.168.0.2".parse().unwrap()),
                "form: {form}"
            );
        }
    }

    #[test]
    fn bracketed_ipv6_locations_parse() {
        // Not a shape Bambu is known to send, but other UPnP stacks do, and a
        // colon-splitting parser that mangled it would be a lurking trap.
        for (form, expected) in [
            ("[2001:db8::1]", "2001:db8::1"),
            ("[2001:db8::1]:8883", "2001:db8::1"),
            ("http://[2001:db8::1]:8883/desc", "2001:db8::1"),
        ] {
            let msg = NOTIFY.replace("Location: 192.168.0.2", &format!("Location: {form}"));
            let announcement = SsdpAnnouncement::parse(msg.as_bytes()).unwrap();
            assert_eq!(
                announcement.location_ip(),
                Some(expected.parse().unwrap()),
                "form: {form}"
            );
        }
    }

    #[test]
    fn usn_prefixes_and_suffixes_are_stripped() {
        // Some UPnP stacks wrap the USN as `uuid:<id>::<service>`. The serial
        // is the id, without either affix.
        let wrapped = NOTIFY.replace(
            "USN: 00M09A000000000",
            "USN: uuid:00M09A000000000::urn:bambulab-com:device:3dprinter:1",
        );
        let announcement = SsdpAnnouncement::parse(wrapped.as_bytes()).unwrap();
        assert_eq!(announcement.serial().unwrap().0, "00M09A000000000");
    }

    #[test]
    fn garbage_is_none_but_odd_http_is_unknown() {
        assert_eq!(SsdpAnnouncement::parse(&[0x00, 0xff, 0x13]), None);
        assert_eq!(SsdpAnnouncement::parse(b""), None);

        let odd = SsdpAnnouncement::parse(b"SUBSCRIBE * HTTP/1.1\r\nHOST: x\r\n\r\n").unwrap();
        assert_eq!(odd.kind, SsdpMessageKind::Unknown);
        assert_eq!(odd.to_printer(sender()), None);
    }

    #[test]
    fn an_announcement_naming_nothing_is_dropped() {
        let empty = SsdpAnnouncement::parse(b"NOTIFY * HTTP/1.1\r\nHOST: x\r\n\r\n").unwrap();
        assert_eq!(empty.to_printer(sender()), None);
    }

    #[test]
    fn the_msearch_payload_is_wellformed_httpu() {
        let payload = m_search_payload();
        let text = std::str::from_utf8(&payload).unwrap();
        assert!(text.starts_with("M-SEARCH * HTTP/1.1\r\n"));
        assert!(text.ends_with("\r\n\r\n"));
        assert!(text.contains("HOST: 239.255.255.250:2021\r\n"));
        assert!(text.contains("MAN: \"ssdp:discover\"\r\n"));
        assert!(text.contains(SEARCH_TARGET));
    }
}
