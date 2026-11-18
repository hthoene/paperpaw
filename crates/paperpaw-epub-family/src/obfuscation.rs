//! Font obfuscation: many EPUBs scramble the start of their embedded fonts
//! so they cannot simply be copied out (not DRM: the key is the book's own
//! identifier). `META-INF/encryption.xml` names the fonts and the method:
//!
//! - IDPF: the first 1040 bytes XOR the SHA-1 of the unique identifier
//!   (whitespace removed).
//! - Adobe: the first 1024 bytes XOR the 16 bytes of the identifier's
//!   UUID (`urn:uuid:…`).

use alloc::string::String;
use alloc::vec::Vec;

/// How a font is obfuscated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Method {
    None,
    Idpf,
    Adobe,
}

impl Method {
    /// The method of an `EncryptionMethod` algorithm URI.
    pub fn from_algorithm(uri: &str) -> Method {
        match uri.trim() {
            "http://www.idpf.org/2008/embedding" => Method::Idpf,
            "http://ns.adobe.com/pdf/enc#RC" => Method::Adobe,
            _ => Method::None,
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Method::None => 0,
            Method::Idpf => 1,
            Method::Adobe => 2,
        }
    }

    pub fn from_code(code: u8) -> Method {
        match code {
            1 => Method::Idpf,
            2 => Method::Adobe,
            _ => Method::None,
        }
    }
}

/// Undoes `method` on font `data`, with the book's unique `identifier`.
pub(crate) fn reveal(data: &mut [u8], method: Method, identifier: &str) {
    reveal_at(data, 0, method, identifier);
}

/// As [`reveal`], for `data` that is the bytes of the font from `offset` on
/// (a font read in chunks).
pub(crate) fn reveal_at(data: &mut [u8], offset: usize, method: Method, identifier: &str) {
    if matches!(method, Method::None) || offset >= 1040 {
        return;
    }
    let (key, length): (Vec<u8>, usize) = match method {
        Method::None => return,
        Method::Idpf => {
            let id: String = identifier.chars().filter(|c| !c.is_whitespace()).collect();
            let mut sha = paperpaw_sdk::digest::sha1::Sha1::new();
            sha.update(id.as_bytes());
            (sha.finish().to_vec(), 1040)
        }
        Method::Adobe => match uuid_bytes(identifier) {
            Some(key) => (key.to_vec(), 1024),
            None => return,
        },
    };
    for (i, byte) in data.iter_mut().enumerate() {
        let at = offset + i;
        if at >= length {
            break;
        }
        *byte ^= key[at % key.len()];
    }
}

/// The 16 bytes of a UUID in `identifier` (e.g. `urn:uuid:…`).
fn uuid_bytes(identifier: &str) -> Option<[u8; 16]> {
    let hex: Vec<u8> = identifier
        .trim()
        .trim_start_matches("urn:")
        .trim_start_matches("uuid:")
        .bytes()
        .filter(|b| *b != b'-')
        .collect();
    if hex.len() != 32 {
        return None;
    }
    let digit = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = [0u8; 16];
    for (i, pair) in hex.chunks(2).enumerate() {
        out[i] = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_undoes_both_methods() {
        let original: Vec<u8> = (0..2000).map(|i| (i % 251) as u8).collect();
        for (method, id) in [
            (Method::Idpf, "urn:isbn: 978 0 00 000000 0"),
            (
                Method::Adobe,
                "urn:uuid:12345678-9abc-def0-1234-56789abcdef0",
            ),
        ] {
            let mut data = original.clone();
            reveal(&mut data, method, id);
            assert_ne!(data, original);
            // XOR twice is the original again.
            reveal(&mut data, method, id);
            assert_eq!(data, original);
        }
        let mut data = original.clone();
        reveal(&mut data, Method::Adobe, "not a uuid");
        assert_eq!(data, original);
    }

    #[test]
    fn a_font_revealed_in_pieces_is_the_font_revealed_whole() {
        // The key (20 bytes of SHA-1, 16 of a UUID) and the 1,040 / 1,024
        // bytes it covers span pieces of any size, whatever their edges.
        let original: Vec<u8> = (0..3000).map(|i| (i % 253) as u8).collect();
        for (method, id) in [
            (Method::Idpf, "urn:isbn:9780000000000"),
            (
                Method::Adobe,
                "urn:uuid:12345678-9abc-def0-1234-56789abcdef0",
            ),
        ] {
            let mut whole = original.clone();
            reveal(&mut whole, method, id);
            for piece in [1, 7, 16, 19, 20, 511, 512, 513, 1023, 1024, 1025, 1041] {
                let mut data = original.clone();
                let mut at = 0;
                for part in data.chunks_mut(piece) {
                    reveal_at(part, at, method, id);
                    at += part.len();
                }
                assert_eq!(data, whole, "pieces of {piece}");
            }
        }
    }
}
