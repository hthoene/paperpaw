//! Bytes as lower-case hexadecimal digits and back, as digests and kept
//! keys are written.

use alloc::string::String;
use alloc::vec::Vec;

pub(crate) const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Writes `byte` as two digits onto `out`.
pub fn push(out: &mut String, byte: u8) {
    out.push(char::from(DIGITS[usize::from(byte >> 4)]));
    out.push(char::from(DIGITS[usize::from(byte & 15)]));
}

/// `bytes` as two digits each.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        push(&mut out, *byte);
    }
    out
}

/// The bytes of `text`, two digits each (either case); nothing if it has
/// an odd number of digits or anything else.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let text = text.as_bytes();
    if !text.len().is_multiple_of(2) {
        return None;
    }
    text.as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |c: u8| char::from(c).to_digit(16);
            Some((digit(pair[0])? << 4 | digit(pair[1])?) as u8)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_ways() {
        assert_eq!(encode(&[0, 0x9f, 0xff]), "009fff");
        assert_eq!(decode("009FfF"), Some(alloc::vec![0, 0x9f, 0xff]));
        assert_eq!(decode("0"), None);
        assert_eq!(decode("zz"), None);
        assert_eq!(decode(""), Some(alloc::vec![]));
    }
}
