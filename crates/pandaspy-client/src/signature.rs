//! Handshake-signature verification that tolerates X.509 v1 certificates.
//!
//! Both of PandaSpy's TLS verifiers — this crate's TOFU transport and
//! `pandaspy-discovery`'s certificate probe — accept any certificate *chain*
//! but still verify the handshake signature, which is what proves the peer
//! holds the key behind the certificate it showed. rustls's helpers for that
//! parse the leaf with webpki, and webpki accepts only X.509 v3. The A1 serves
//! a v1 leaf, so rustls rejects its perfectly valid signature with
//! `UnsupportedCertVersion` and the printer can neither be found nor reached.
//!
//! So for a pre-v3 leaf, the public key is read out of the certificate here
//! and the signature checked against it with the provider's own algorithms —
//! the same check rustls performs, minus the v3-only parse. A v3 leaf goes
//! straight to rustls, unchanged. Only the *parse* is relaxed: the signature
//! is verified exactly as strictly either way, and nothing about the chain is
//! newly trusted, because no chain was ever validated.
//!
//! One implementation, shared by both verifiers, so the two cannot drift.

use rustls::client::danger::HandshakeSignatureValid;
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, SubjectPublicKeyInfoDer};
use rustls::{CertificateError, DigitallySignedStruct, Error, PeerMisbehaved};
use x509_parser::prelude::{FromDer, X509Certificate, X509Version};

/// Verify a TLS 1.2 handshake signature against `cert`'s public key.
///
/// A drop-in for [`rustls::crypto::verify_tls12_signature`] that also accepts
/// X.509 v1 and v2 leaves.
///
/// # Errors
///
/// Whatever rustls would return for a v3 leaf; for an older one, an error if
/// the scheme was not offered, the key type cannot make that scheme, or the
/// signature does not verify.
pub fn verify_tls12_signature(
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
    algorithms: &WebPkiSupportedAlgorithms,
) -> Result<HandshakeSignatureValid, Error> {
    let Some(spki) = pre_v3_public_key(cert) else {
        return rustls::crypto::verify_tls12_signature(message, cert, dss, algorithms);
    };
    let (key_algorithm, key) =
        split_spki(spki).ok_or(Error::InvalidCertificate(CertificateError::BadEncoding))?;

    // As in rustls: a TLS 1.2 scheme can map to several verification
    // algorithms, and the one to use is the first whose key type is the
    // certificate's.
    let candidates = algorithms
        .mapping
        .iter()
        .find(|(scheme, _)| *scheme == dss.scheme)
        .map(|(_, candidates)| *candidates)
        .ok_or(PeerMisbehaved::SignedHandshakeWithUnadvertisedSigScheme)?;
    let algorithm = candidates
        .iter()
        .find(|algorithm| algorithm.public_key_alg_id().as_ref() == key_algorithm)
        .ok_or(Error::InvalidCertificate(CertificateError::BadSignature))?;

    algorithm
        .verify_signature(key, message, dss.signature())
        .map(|()| HandshakeSignatureValid::assertion())
        .map_err(|_| Error::InvalidCertificate(CertificateError::BadSignature))
}

/// Verify a TLS 1.3 handshake signature against `cert`'s public key.
///
/// A drop-in for [`rustls::crypto::verify_tls13_signature`] that also accepts
/// X.509 v1 and v2 leaves, via rustls's own raw-public-key path.
///
/// # Errors
///
/// As [`rustls::crypto::verify_tls13_signature`].
pub fn verify_tls13_signature(
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
    algorithms: &WebPkiSupportedAlgorithms,
) -> Result<HandshakeSignatureValid, Error> {
    match pre_v3_public_key(cert) {
        Some(spki) => rustls::crypto::verify_tls13_signature_with_raw_key(
            message,
            &SubjectPublicKeyInfoDer::from(spki),
            dss,
            algorithms,
        ),
        None => rustls::crypto::verify_tls13_signature(message, cert, dss, algorithms),
    }
}

/// The DER `SubjectPublicKeyInfo` of a pre-v3 certificate, or `None` for a v3
/// one (or anything unparseable) — those are rustls's to judge.
fn pre_v3_public_key<'a>(cert: &'a CertificateDer<'_>) -> Option<&'a [u8]> {
    let (_, parsed) = X509Certificate::from_der(cert.as_ref()).ok()?;
    let X509Certificate {
        tbs_certificate, ..
    } = parsed;
    (tbs_certificate.version != X509Version::V3).then_some(tbs_certificate.subject_pki.raw)
}

/// Split `SubjectPublicKeyInfo ::= SEQUENCE { AlgorithmIdentifier, BIT STRING }`
/// into the algorithm identifier's contents (the form
/// `public_key_alg_id` compares against) and the key bits.
fn split_spki(spki: &[u8]) -> Option<(&[u8], &[u8])> {
    let (body, trailing) = der_element(spki, 0x30)?;
    let (algorithm, body) = der_element(body, 0x30)?;
    let (bits, rest) = der_element(body, 0x03)?;
    if !trailing.is_empty() || !rest.is_empty() {
        return None;
    }
    // A key is a whole number of bytes: the leading unused-bits count is 0.
    match bits.split_first()? {
        (0, key) => Some((algorithm, key)),
        _ => None,
    }
}

/// Read one DER element with the expected `tag`, returning its contents and
/// whatever follows it.
fn der_element(input: &[u8], tag: u8) -> Option<(&[u8], &[u8])> {
    let (&found, input) = input.split_first()?;
    if found != tag {
        return None;
    }
    let (&first, input) = input.split_first()?;
    let (length, input) = if first < 0x80 {
        (usize::from(first), input)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || input.len() < count {
            return None;
        }
        let (length, input) = input.split_at(count);
        let length = length
            .iter()
            .fold(0_usize, |acc, &byte| (acc << 8) | usize::from(byte));
        (length, input)
    };
    (input.len() >= length).then(|| input.split_at(length))
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1_CERT: &[u8] = include_bytes!("../testdata/v1-leaf.cert.der");

    #[test]
    fn a_v1_certificate_yields_its_public_key() {
        let cert = CertificateDer::from(V1_CERT);
        let spki = pre_v3_public_key(&cert).expect("the test certificate is v1");
        let (algorithm, key) = split_spki(spki).unwrap();
        // rsaEncryption OID + NULL parameters: what ring's RSA verifiers expect.
        assert_eq!(
            algorithm,
            [
                0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00
            ]
        );
        assert!(key.len() > 256, "an RSA-2048 public key");
    }

    #[test]
    fn a_v3_certificate_is_left_to_rustls() {
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = rcgen::CertificateParams::default()
            .self_signed(&key)
            .unwrap();
        assert_eq!(pre_v3_public_key(cert.der()), None);
    }

    #[test]
    fn malformed_spki_is_rejected_not_misread() {
        assert_eq!(split_spki(&[]), None);
        // Truncated: claims 0x10 bytes of body, has 2.
        assert_eq!(split_spki(&[0x30, 0x10, 0x30, 0x00]), None);
        // Nonzero unused-bits count.
        assert_eq!(
            split_spki(&[0x30, 0x06, 0x30, 0x00, 0x03, 0x02, 0x01, 0xff]),
            None
        );
        // Trailing bytes after the SEQUENCE.
        assert_eq!(
            split_spki(&[0x30, 0x06, 0x30, 0x00, 0x03, 0x02, 0x00, 0xff, 0x00]),
            None
        );
        assert_eq!(
            split_spki(&[0x30, 0x06, 0x30, 0x00, 0x03, 0x02, 0x00, 0xff]),
            Some((&[][..], &[0xff][..]))
        );
    }
}
