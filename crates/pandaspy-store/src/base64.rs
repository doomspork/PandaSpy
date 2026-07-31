//! A tiny, self-contained standard-alphabet base64 codec (RFC 4648, padded).
//!
//! It exists so the encrypted-secret file can store its salt, nonce and
//! ciphertext as compact, inspectable strings without pulling in a `base64`
//! dependency the crate is not permitted to add. The scope is exactly what that
//! file needs: encode bytes, decode what we encoded, and reject anything that
//! is not well-formed (a decode failure surfaces as a corrupt file, never a
//! silent wrong answer). It is covered by round-trip and known-vector tests.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes as padded standard base64.
pub(crate) fn encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b1 = chunk.get(1).copied();
        let b2 = chunk.get(2).copied();
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(b1.unwrap_or(0)) << 8)
            | u32::from(b2.unwrap_or(0));

        out.push(char::from(ALPHABET[((n >> 18) & 0x3f) as usize]));
        out.push(char::from(ALPHABET[((n >> 12) & 0x3f) as usize]));
        out.push(if b1.is_some() {
            char::from(ALPHABET[((n >> 6) & 0x3f) as usize])
        } else {
            '='
        });
        out.push(if b2.is_some() {
            char::from(ALPHABET[(n & 0x3f) as usize])
        } else {
            '='
        });
    }
    out
}

/// Decode padded standard base64. `None` for anything malformed — wrong length,
/// a stray character, or padding that is not at the very end.
pub(crate) fn decode(input: &str) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return Some(Vec::new());
    }
    if !bytes.len().is_multiple_of(4) {
        return None;
    }

    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let last_chunk = bytes.len() / 4 - 1;
    for (i, chunk) in bytes.chunks_exact(4).enumerate() {
        let mut vals = [0u8; 4];
        let mut pad = 0usize;
        for (j, &c) in chunk.iter().enumerate() {
            match c {
                b'A'..=b'Z' => vals[j] = c - b'A',
                b'a'..=b'z' => vals[j] = c - b'a' + 26,
                b'0'..=b'9' => vals[j] = c - b'0' + 52,
                b'+' => vals[j] = 62,
                b'/' => vals[j] = 63,
                // Padding is only ever the last one or two characters.
                b'=' if i == last_chunk && j >= 2 => pad += 1,
                _ => return None,
            }
        }
        // Once padding starts it must run to the end of the chunk.
        if pad == 1 && chunk[3] != b'=' {
            return None;
        }

        let n = (u32::from(vals[0]) << 18)
            | (u32::from(vals[1]) << 12)
            | (u32::from(vals[2]) << 6)
            | u32::from(vals[3]);
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors_match_rfc_4648() {
        for (raw, encoded) in [
            (&b""[..], ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(raw), encoded, "encode {raw:?}");
            assert_eq!(decode(encoded).as_deref(), Some(raw), "decode {encoded}");
        }
    }

    #[test]
    fn round_trips_the_sizes_the_secret_file_uses() {
        for len in [0usize, 1, 2, 3, 12, 16, 32, 48, 100] {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 7 + 1) as u8).collect();
            assert_eq!(decode(&encode(&bytes)).as_deref(), Some(bytes.as_slice()));
        }
    }

    #[test]
    fn malformed_input_is_rejected_not_guessed() {
        assert_eq!(decode("Zg="), None, "length not a multiple of four");
        assert_eq!(decode("Zg=A"), None, "data after padding");
        assert_eq!(decode("Z g="), None, "stray character");
        assert_eq!(decode("****"), None, "outside the alphabet");
    }
}
