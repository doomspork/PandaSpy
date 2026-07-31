//! A hand-rolled MQTT 3.1.1 codec — only the packets a read-only monitor needs.
//!
//! PandaSpy subscribes to one topic and publishes the odd `pushall`/`get_version`
//! request; it never does QoS 1/2, retained messages, wills, or topic
//! wildcards. So this is deliberately not a general MQTT library — it is the
//! smallest correct subset, kept pure (bytes in, packets out) so the whole
//! wire format is unit-testable without a socket. The async read/write helpers
//! at the bottom are the only I/O, and they are thin.
//!
//! Reference: OASIS MQTT 3.1.1, protocol level 4.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Largest control packet we will accept off the wire. A printer's full status
/// report is tens of kilobytes; 1 MiB is generous headroom and a hard cap so a
/// hostile or wedged peer cannot make us allocate without bound.
pub const MAX_PACKET_LEN: usize = 1024 * 1024;

/// MQTT control packet types (the high nibble of byte 1).
mod packet_type {
    pub const CONNECT: u8 = 1;
    pub const CONNACK: u8 = 2;
    pub const PUBLISH: u8 = 3;
    pub const SUBSCRIBE: u8 = 8;
    pub const SUBACK: u8 = 9;
    pub const PINGREQ: u8 = 12;
    pub const PINGRESP: u8 = 13;
    pub const DISCONNECT: u8 = 14;
}

/// What a CONNACK told us. The distinctions matter: they are what lets the UI
/// say "wrong access code" instead of a shrug.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectReturnCode {
    Accepted,
    UnacceptableProtocolVersion,
    IdentifierRejected,
    ServerUnavailable,
    BadCredentials,
    NotAuthorized,
    /// A code outside the 0–5 the spec defines, preserved for diagnostics.
    Other(u8),
}

impl ConnectReturnCode {
    #[must_use]
    fn from_wire(byte: u8) -> Self {
        match byte {
            0 => Self::Accepted,
            1 => Self::UnacceptableProtocolVersion,
            2 => Self::IdentifierRejected,
            3 => Self::ServerUnavailable,
            4 => Self::BadCredentials,
            5 => Self::NotAuthorized,
            other => Self::Other(other),
        }
    }

    #[must_use]
    fn to_wire(self) -> u8 {
        match self {
            Self::Accepted => 0,
            Self::UnacceptableProtocolVersion => 1,
            Self::IdentifierRejected => 2,
            Self::ServerUnavailable => 3,
            Self::BadCredentials => 4,
            Self::NotAuthorized => 5,
            Self::Other(other) => other,
        }
    }
}

/// A CONNECT, as PandaSpy sends it: clean session, username + password, no will.
///
/// `Debug` is hand-written so the `password` (the printer's access code) cannot
/// reach a log or a crash report through a stray `{:?}` — the same discipline
/// as [`crate::Credentials`].
#[derive(Clone, PartialEq, Eq)]
pub struct Connect {
    pub client_id: String,
    pub keep_alive_secs: u16,
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for Connect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connect")
            .field("client_id", &self.client_id)
            .field("keep_alive_secs", &self.keep_alive_secs)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// A CONNECT's flag byte for our fixed shape: clean session, username and
/// password present, no will, no retain.
const CONNECT_FLAGS: u8 = 0b1100_0010;

/// A CONNACK reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnAck {
    pub session_present: bool,
    pub code: ConnectReturnCode,
}

/// A QoS-0 SUBSCRIBE to a single topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscribe {
    pub packet_id: u16,
    pub topic: String,
}

/// A SUBACK reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubAck {
    pub packet_id: u16,
    /// One granted-QoS (0–2) or `0x80` failure byte per subscription.
    pub return_codes: Vec<u8>,
}

/// A QoS-0 PUBLISH. No packet id exists at QoS 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Publish {
    pub topic: String,
    pub payload: Vec<u8>,
}

/// Every control packet this client speaks or hears.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Packet {
    Connect(Connect),
    ConnAck(ConnAck),
    Publish(Publish),
    Subscribe(Subscribe),
    SubAck(SubAck),
    PingReq,
    PingResp,
    Disconnect,
}

impl Packet {
    /// Serialise to the wire.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let (type_and_flags, body) = match self {
            Self::Connect(connect) => (packet_type::CONNECT << 4, encode_connect(connect)),
            Self::ConnAck(ack) => (
                packet_type::CONNACK << 4,
                vec![u8::from(ack.session_present), ack.code.to_wire()],
            ),
            Self::Publish(publish) => {
                // QoS 0 → all PUBLISH flags clear.
                let mut body = encode_string(&publish.topic);
                body.extend_from_slice(&publish.payload);
                (packet_type::PUBLISH << 4, body)
            }
            Self::Subscribe(subscribe) => {
                let mut body = Vec::new();
                body.extend_from_slice(&subscribe.packet_id.to_be_bytes());
                body.extend(encode_string(&subscribe.topic));
                body.push(0); // requested QoS 0
                // SUBSCRIBE mandates fixed-header flags 0b0010.
                ((packet_type::SUBSCRIBE << 4) | 0b0010, body)
            }
            Self::SubAck(ack) => {
                let mut body = ack.packet_id.to_be_bytes().to_vec();
                body.extend_from_slice(&ack.return_codes);
                (packet_type::SUBACK << 4, body)
            }
            Self::PingReq => (packet_type::PINGREQ << 4, Vec::new()),
            Self::PingResp => (packet_type::PINGRESP << 4, Vec::new()),
            Self::Disconnect => (packet_type::DISCONNECT << 4, Vec::new()),
        };

        let mut out = Vec::with_capacity(body.len() + 5);
        out.push(type_and_flags);
        encode_remaining_length(body.len(), &mut out);
        out.extend(body);
        out
    }

    /// Parse a packet from its fixed-header first byte and already-read body.
    ///
    /// # Errors
    ///
    /// If the body is malformed for the packet type, or the type is one this
    /// subset does not handle.
    pub fn decode(first_byte: u8, body: &[u8]) -> io::Result<Self> {
        let kind = first_byte >> 4;
        let flags = first_byte & 0x0f;
        match kind {
            packet_type::CONNACK => {
                let [flags_byte, code] = *body else {
                    return Err(malformed("CONNACK must be 2 bytes"));
                };
                Ok(Self::ConnAck(ConnAck {
                    session_present: flags_byte & 0x01 != 0,
                    code: ConnectReturnCode::from_wire(code),
                }))
            }
            packet_type::SUBACK => {
                if body.len() < 3 {
                    return Err(malformed("SUBACK too short"));
                }
                Ok(Self::SubAck(SubAck {
                    packet_id: u16::from_be_bytes([body[0], body[1]]),
                    return_codes: body[2..].to_vec(),
                }))
            }
            packet_type::PUBLISH => {
                // We only subscribe at QoS 0, so a well-behaved broker never
                // sends us a packet id. Reject QoS > 0 rather than mis-parse.
                let qos = (flags >> 1) & 0b11;
                if qos != 0 {
                    return Err(malformed("unexpected QoS > 0 PUBLISH"));
                }
                let (topic, rest) = decode_string(body)?;
                Ok(Self::Publish(Publish {
                    topic,
                    payload: rest.to_vec(),
                }))
            }
            packet_type::CONNECT => Ok(Self::Connect(decode_connect(body)?)),
            packet_type::SUBSCRIBE => {
                if body.len() < 2 {
                    return Err(malformed("SUBSCRIBE too short"));
                }
                let packet_id = u16::from_be_bytes([body[0], body[1]]);
                let (topic, _qos) = decode_string(&body[2..])?;
                Ok(Self::Subscribe(Subscribe { packet_id, topic }))
            }
            packet_type::PINGREQ => Ok(Self::PingReq),
            packet_type::PINGRESP => Ok(Self::PingResp),
            packet_type::DISCONNECT => Ok(Self::Disconnect),
            other => Err(malformed(&format!("unsupported packet type {other}"))),
        }
    }
}

fn encode_connect(connect: &Connect) -> Vec<u8> {
    let mut body = encode_string("MQTT"); // protocol name
    body.push(4); // protocol level: 3.1.1
    body.push(CONNECT_FLAGS);
    body.extend_from_slice(&connect.keep_alive_secs.to_be_bytes());
    body.extend(encode_string(&connect.client_id));
    body.extend(encode_string(&connect.username));
    body.extend(encode_string(&connect.password));
    body
}

fn decode_connect(body: &[u8]) -> io::Result<Connect> {
    let (name, rest) = decode_string(body)?;
    if name != "MQTT" {
        return Err(malformed("unexpected protocol name"));
    }
    // protocol level (1) + flags (1) + keep-alive (2)
    if rest.len() < 4 {
        return Err(malformed("CONNECT header truncated"));
    }
    let keep_alive_secs = u16::from_be_bytes([rest[2], rest[3]]);
    let (client_id, rest) = decode_string(&rest[4..])?;
    let (username, rest) = decode_string(rest)?;
    let (password, _rest) = decode_string(rest)?;
    Ok(Connect {
        client_id,
        keep_alive_secs,
        username,
        password,
    })
}

/// Append an MQTT variable-length "Remaining Length" (1–4 bytes, base-128).
fn encode_remaining_length(mut len: usize, out: &mut Vec<u8>) {
    loop {
        let mut byte = (len % 128) as u8;
        len /= 128;
        if len > 0 {
            byte |= 0x80; // continuation bit
        }
        out.push(byte);
        if len == 0 {
            break;
        }
    }
}

/// A UTF-8 string with a 2-byte big-endian length prefix.
///
/// MQTT caps a string at 65535 bytes, and everything we encode (topics, the
/// client id, `bblp`, the access code) is far shorter. A value over the cap
/// would silently wrap the length prefix and corrupt the packet, so guard it:
/// debug builds assert, release builds clamp the prefix to the truncation that
/// at least keeps the framing self-consistent rather than emitting a length
/// that disagrees with the bytes.
fn encode_string(value: &str) -> Vec<u8> {
    debug_assert!(
        value.len() <= u16::MAX as usize,
        "MQTT string exceeds 65535 bytes"
    );
    let len = value.len().min(u16::MAX as usize);
    let mut out = Vec::with_capacity(len + 2);
    out.extend_from_slice(&(len as u16).to_be_bytes());
    out.extend_from_slice(&value.as_bytes()[..len]);
    out
}

/// Read a length-prefixed string, returning it and the remaining slice.
fn decode_string(bytes: &[u8]) -> io::Result<(String, &[u8])> {
    if bytes.len() < 2 {
        return Err(malformed("string length prefix truncated"));
    }
    let len = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
    let end = 2 + len;
    if bytes.len() < end {
        return Err(malformed("string body truncated"));
    }
    let text = std::str::from_utf8(&bytes[2..end])
        .map_err(|_| malformed("string is not UTF-8"))?
        .to_owned();
    Ok((text, &bytes[end..]))
}

fn malformed(reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("malformed MQTT: {reason}"),
    )
}

/// Write a packet to an async sink in one shot.
///
/// # Errors
///
/// Propagates the underlying write error.
pub async fn write_packet<W>(writer: &mut W, packet: &Packet) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&packet.encode()).await?;
    writer.flush().await
}

/// Read one packet from an async source: the type byte, the base-128 remaining
/// length, then exactly that many body bytes.
///
/// # Errors
///
/// On EOF, a remaining length that overflows four bytes or exceeds
/// [`MAX_PACKET_LEN`], or a malformed body.
pub async fn read_packet<R>(reader: &mut R) -> io::Result<Packet>
where
    R: AsyncRead + Unpin,
{
    let first_byte = reader.read_u8().await?;

    let mut len = 0_usize;
    let mut multiplier = 1_usize;
    loop {
        let byte = reader.read_u8().await?;
        len += (byte & 0x7f) as usize * multiplier;
        if byte & 0x80 == 0 {
            break;
        }
        multiplier *= 128;
        if multiplier > 128 * 128 * 128 {
            return Err(malformed("remaining length exceeds four bytes"));
        }
    }
    if len > MAX_PACKET_LEN {
        return Err(malformed("packet larger than the accepted maximum"));
    }

    let mut body = vec![0_u8; len];
    reader.read_exact(&mut body).await?;
    Packet::decode(first_byte, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(packet: Packet) {
        let bytes = packet.encode();
        let decoded = Packet::decode(bytes[0], &bytes[header_len(&bytes)..]).unwrap();
        assert_eq!(decoded, packet, "round-trip changed the packet");
    }

    /// Bytes consumed by the fixed header (type byte + remaining-length varint).
    fn header_len(bytes: &[u8]) -> usize {
        let mut i = 1;
        while bytes[i] & 0x80 != 0 {
            i += 1;
        }
        i + 1
    }

    #[test]
    fn connect_round_trips_including_credentials() {
        roundtrip(Packet::Connect(Connect {
            client_id: "pandaspy-abc".to_owned(),
            keep_alive_secs: 30,
            username: "bblp".to_owned(),
            password: "12345678".to_owned(),
        }));
    }

    #[test]
    fn connect_encodes_the_documented_wire_shape() {
        let bytes = Packet::Connect(Connect {
            client_id: "x".to_owned(),
            keep_alive_secs: 60,
            username: "bblp".to_owned(),
            password: "pw".to_owned(),
        })
        .encode();
        // type 1, then remaining length, then "MQTT", level 4, flags 0xC2.
        assert_eq!(bytes[0], 0b0001_0000);
        let body = &bytes[header_len(&bytes)..];
        assert_eq!(&body[0..6], &[0x00, 0x04, b'M', b'Q', b'T', b'T']);
        assert_eq!(body[6], 4, "protocol level 3.1.1");
        assert_eq!(body[7], CONNECT_FLAGS);
        assert_eq!(&body[8..10], &60_u16.to_be_bytes());
    }

    #[test]
    fn connack_return_codes_map_both_ways() {
        for (byte, code) in [
            (0, ConnectReturnCode::Accepted),
            (4, ConnectReturnCode::BadCredentials),
            (5, ConnectReturnCode::NotAuthorized),
            (9, ConnectReturnCode::Other(9)),
        ] {
            let decoded = Packet::decode(packet_type::CONNACK << 4, &[0x01, byte]).unwrap();
            assert_eq!(
                decoded,
                Packet::ConnAck(ConnAck {
                    session_present: true,
                    code,
                })
            );
            assert_eq!(code.to_wire(), byte);
        }
    }

    #[test]
    fn subscribe_carries_the_mandatory_flag_bits() {
        let bytes = Packet::Subscribe(Subscribe {
            packet_id: 7,
            topic: "device/x/report".to_owned(),
        })
        .encode();
        assert_eq!(bytes[0] & 0x0f, 0b0010, "SUBSCRIBE fixed-header flags");
        roundtrip(Packet::Subscribe(Subscribe {
            packet_id: 7,
            topic: "device/x/report".to_owned(),
        }));
    }

    #[test]
    fn publish_at_qos0_carries_no_packet_id() {
        roundtrip(Packet::Publish(Publish {
            topic: "device/00M/report".to_owned(),
            payload: br#"{"print":{}}"#.to_vec(),
        }));
    }

    #[test]
    fn a_qos1_publish_is_rejected_rather_than_misparsed() {
        // type 3, flags 0b0010 (QoS 1). We never subscribe at QoS > 0, so this
        // is a broker doing something we did not ask for.
        let first = (packet_type::PUBLISH << 4) | 0b0010;
        assert!(Packet::decode(first, &encode_string("t")).is_err());
    }

    #[test]
    fn pingreq_pingresp_disconnect_have_empty_bodies() {
        for packet in [Packet::PingReq, Packet::PingResp, Packet::Disconnect] {
            let bytes = packet.encode();
            assert_eq!(bytes.len(), 2, "one type byte + a zero remaining length");
            assert_eq!(bytes[1], 0);
            roundtrip(packet);
        }
    }

    #[test]
    fn remaining_length_uses_the_base_128_boundaries() {
        // The classic MQTT boundary values: 1 vs 2 vs 3 encoding bytes.
        for (len, expected) in [
            (0_usize, vec![0x00]),
            (127, vec![0x7f]),
            (128, vec![0x80, 0x01]),
            (16_383, vec![0xff, 0x7f]),
            (16_384, vec![0x80, 0x80, 0x01]),
        ] {
            let mut out = Vec::new();
            encode_remaining_length(len, &mut out);
            assert_eq!(out, expected, "remaining length {len}");
        }
    }

    #[tokio::test]
    async fn read_packet_reassembles_a_multibyte_length_from_a_stream() {
        // A PUBLISH whose remaining length needs two varint bytes (>127).
        let publish = Packet::Publish(Publish {
            topic: "t".to_owned(),
            payload: vec![b'a'; 200],
        });
        let bytes = publish.encode();
        assert!(
            bytes[2] & 0x80 != 0 || bytes[1] & 0x80 != 0,
            "needs a 2-byte length"
        );

        let mut cursor = std::io::Cursor::new(bytes);
        let decoded = read_packet(&mut cursor).await.unwrap();
        assert_eq!(decoded, publish);
    }

    #[tokio::test]
    async fn write_then_read_round_trips_over_a_duplex() {
        let (mut a, mut b) = tokio::io::duplex(4096);
        let sent = Packet::Subscribe(Subscribe {
            packet_id: 42,
            topic: "device/00M09A000000000/report".to_owned(),
        });
        write_packet(&mut a, &sent).await.unwrap();
        let got = read_packet(&mut b).await.unwrap();
        assert_eq!(got, sent);
    }

    #[test]
    fn malformed_bodies_error_rather_than_panic() {
        // Each of these once risked a slice/index panic; they must be clean
        // errors instead. A printer's firmware is undocumented and changes, so
        // a surprise on the wire cannot be allowed to take the process down.
        assert!(
            Packet::decode(packet_type::CONNACK << 4, &[0x00]).is_err(),
            "short CONNACK"
        );
        assert!(
            Packet::decode(packet_type::SUBACK << 4, &[0x00]).is_err(),
            "short SUBACK"
        );
        // PUBLISH whose 2-byte topic-length prefix claims more than is present.
        assert!(
            Packet::decode(packet_type::PUBLISH << 4, &[0x00, 0x05, b'a']).is_err(),
            "truncated topic"
        );
        // A topic that is not valid UTF-8.
        assert!(
            Packet::decode(packet_type::PUBLISH << 4, &[0x00, 0x01, 0xff]).is_err(),
            "non-UTF-8 topic"
        );
        // An unsupported packet type (e.g. PUBREL = 6).
        assert!(Packet::decode(6 << 4, &[]).is_err(), "unsupported type");
    }

    #[test]
    fn connect_debug_never_reveals_the_access_code() {
        let rendered = format!(
            "{:?}",
            Packet::Connect(Connect {
                client_id: "x".to_owned(),
                keep_alive_secs: 30,
                username: "bblp".to_owned(),
                password: "12345678".to_owned(),
            })
        );
        assert!(!rendered.contains("12345678"), "leaked: {rendered}");
        assert!(rendered.contains("<redacted>"));
    }

    #[tokio::test]
    async fn an_oversize_remaining_length_is_refused() {
        // 0xFF 0xFF 0xFF 0xFF would decode to a huge length; we cap it.
        let mut cursor =
            std::io::Cursor::new(vec![packet_type::PUBLISH << 4, 0xff, 0xff, 0xff, 0x7f]);
        assert!(read_packet(&mut cursor).await.is_err());
    }
}
