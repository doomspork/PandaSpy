//! The subnet probe seam: knock on 8883, read the certificate, learn the
//! serial.
//!
//! Bambu printers answer TLS on the MQTT port with a self-signed certificate
//! whose subject CN **is the device serial**. That turns a blind port scan
//! into positive identification: anything that completes a handshake and
//! presents a CN shaped like a serial is a printer, and we know which one —
//! no MQTT login, no access code involved.

use std::future::Future;
use std::net::SocketAddr;

use pandaspy_proto::DeviceSerial;
use serde::Serialize;

/// What knocking on one address produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum ProbeOutcome {
    /// TLS completed and the peer presented a leaf certificate.
    /// The DER is kept whole: the CN identifies the printer here, and the
    /// same bytes let `pandaspy-client` pin the certificate before the first
    /// MQTT connection ever happens.
    TlsPeer { cert_der: Vec<u8> },
    /// TCP connected but the TLS handshake failed — something lives there,
    /// but it does not speak printer.
    NotTls,
    /// Actively refused. A host exists; the port is closed.
    Refused,
    /// Nothing answered within the probe's own timeout.
    Timeout,
    /// Any other transport failure, kept as text for diagnostics.
    Failed(String),
}

/// "What is at this address?", abstracted so the walk can be tested against
/// a map instead of a real /24.
///
/// Same `impl Future + Send` construction as [`crate::SsdpSocket`], for the
/// same spawnability reason.
pub trait PortProbe: Send + Sync + std::fmt::Debug {
    /// Probe one address. Implementations bound their own connect/handshake
    /// time; the orchestrator adds an outer timeout as a backstop so a
    /// misbehaving implementation cannot stall the walk.
    fn probe(&self, target: SocketAddr) -> impl Future<Output = ProbeOutcome> + Send;
}

/// Pull the subject CN out of a DER certificate and vet that it is shaped
/// like a Bambu serial: 8–32 characters, ASCII alphanumeric, and containing
/// at least one digit.
///
/// The shape check matters — the probe knocks on every host in the subnet,
/// and a device with a self-signed cert must not be mistaken for a printer
/// just because it, too, has a CN. Requiring a digit is what rejects the
/// common impostor: a NAS or router whose CN is a bare hostname like
/// `synologynas` (alphanumeric, right length, but no digit). Real Bambu
/// serials are digit-led (`00M09A000000000`).
///
/// TODO(fixture): this is a heuristic, not the manufacturer's spec. Confirm
/// the exact serial grammar (length, prefix, character set) against real
/// certificates once captures exist, and tighten to it — a NAS whose
/// hostname happens to contain a digit still slips through today.
#[must_use]
pub fn serial_from_cert(cert_der: &[u8]) -> Option<DeviceSerial> {
    let (_, cert) = x509_parser::parse_x509_certificate(cert_der).ok()?;
    let cn = cert.subject().iter_common_name().next()?.as_str().ok()?;

    let plausible = (8..=32).contains(&cn.len())
        && cn.bytes().all(|byte| byte.is_ascii_alphanumeric())
        && cn.bytes().any(|byte| byte.is_ascii_digit());
    if !plausible {
        return None;
    }
    Some(DeviceSerial(cn.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cert_with_cn(cn: &str) -> Vec<u8> {
        // rcgen mints a throwaway self-signed cert — the same shape a
        // printer serves, which is exactly what the parser must read.
        let mut params = rcgen::CertificateParams::default();
        params.distinguished_name = rcgen::DistinguishedName::new();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, cn);
        let key = rcgen::KeyPair::generate().unwrap();
        params.self_signed(&key).unwrap().der().to_vec()
    }

    #[test]
    fn a_serial_shaped_cn_is_read_as_the_serial() {
        let der = cert_with_cn("00M09A000000000");
        assert_eq!(
            serial_from_cert(&der),
            Some(DeviceSerial("00M09A000000000".to_owned()))
        );
    }

    #[test]
    fn a_nas_with_a_self_signed_cert_is_not_a_printer() {
        for impostor in [
            "synology.local", // dot
            "my home nas",    // spaces
            "ab",             // too short
            "",               // empty
            "synologynas",    // right length, alphanumeric, but no digit
            "routerhostname", // ditto — the bare-hostname impostor
        ] {
            let der = cert_with_cn(impostor);
            assert_eq!(serial_from_cert(&der), None, "cn: {impostor:?}");
        }
    }

    #[test]
    fn serial_length_boundaries_are_inclusive() {
        // 8 and 32 are accepted; 7 and 33 are not. Each carries a digit so
        // only the length is under test.
        assert!(
            serial_from_cert(&cert_with_cn("A1234567")).is_some(),
            "8 chars"
        );
        assert!(
            serial_from_cert(&cert_with_cn(&format!("A{}", "0".repeat(31)))).is_some(),
            "32"
        );
        assert!(
            serial_from_cert(&cert_with_cn("A123456")).is_none(),
            "7 chars"
        );
        assert!(
            serial_from_cert(&cert_with_cn(&format!("A{}", "0".repeat(32)))).is_none(),
            "33"
        );
    }

    #[test]
    fn garbage_bytes_are_not_a_certificate() {
        assert_eq!(serial_from_cert(&[0x30, 0x82, 0xff, 0x01]), None);
        assert_eq!(serial_from_cert(b""), None);
    }
}
