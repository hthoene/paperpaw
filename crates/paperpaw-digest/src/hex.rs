//! Bytes as lower-case hexadecimal digits and back, as digests and kept
//! keys are written.

use alloc::collections::TryReserveError;
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

/// One digit of either case.
fn digit(c: u8) -> Option<u8> {
    char::from(c).to_digit(16).map(|d| d as u8)
}

/// Decodes `text` onto the front of `out` and returns those bytes: two
/// digits each, either case. Nothing is allocated. `None` if `text` has an
/// odd number of digits, anything but digits, or more bytes than `out`
/// holds; `out` is not read, and its contents are unspecified after a
/// refusal.
pub fn decode_into<'o>(text: &str, out: &'o mut [u8]) -> Option<&'o [u8]> {
    let text = text.as_bytes();
    if !text.len().is_multiple_of(2) || text.len() / 2 > out.len() {
        return None;
    }
    let (pairs, _) = text.as_chunks::<2>();
    for (pair, byte) in pairs.iter().zip(out.iter_mut()) {
        *byte = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Some(&out[..pairs.len()])
}

/// Exactly `N` bytes from `2 * N` digits of `text` (either case), on the
/// stack. `None` for any other length or anything but digits.
pub fn decode_array<const N: usize>(text: &str) -> Option<[u8; N]> {
    // A longer text would have `decode_into` stop early, so check the length.
    if text.len() != N.checked_mul(2)? {
        return None;
    }
    let mut out = [0; N];
    decode_into(text, &mut out)?;
    Some(out)
}

/// The bytes of `text`, two digits each (either case), in a vector reserved
/// without aborting: `Err` if the memory is not there, `Ok(None)` if `text`
/// has an odd number of digits or anything else.
///
/// # Errors
/// [`TryReserveError`] when the vector cannot be reserved.
pub fn try_decode(text: &str) -> Result<Option<Vec<u8>>, TryReserveError> {
    if !text.len().is_multiple_of(2) {
        return Ok(None);
    }
    let mut out = Vec::new();
    out.try_reserve_exact(text.len() / 2)?;
    out.resize(text.len() / 2, 0);
    let decoded = decode_into(text, &mut out).is_some();
    Ok(decoded.then_some(out))
}

/// The bytes of `text`, two digits each (either case); nothing if it has
/// an odd number of digits or anything else. The vector is allocated
/// infallibly: where memory is scarce use [`decode_into`],
/// [`decode_array`] or [`try_decode`].
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let text = text.as_bytes();
    if !text.len().is_multiple_of(2) {
        return None;
    }
    text.as_chunks::<2>()
        .0
        .iter()
        .map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?))
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

    #[test]
    fn decode_into_fills_the_front_of_a_buffer() {
        let mut out = [0xaa; 4];
        assert_eq!(decode_into("009FfF", &mut out), Some(&[0, 0x9f, 0xff][..]));
        assert_eq!(decode_into("", &mut out), Some(&[][..]));
        assert_eq!(
            decode_into("00112233", &mut out),
            Some(&[0, 0x11, 0x22, 0x33][..])
        );
        for bad in ["0", "zz", "0g", "00112233aa", " 0", "é"] {
            assert_eq!(decode_into(bad, &mut out), None, "{bad:?}");
        }
        assert_eq!(decode_into("00", &mut []), None);
    }

    #[test]
    fn decode_array_needs_exactly_its_digits() {
        assert_eq!(decode_array::<2>("00fF"), Some([0, 0xff]));
        assert_eq!(decode_array::<0>(""), Some([]));
        for bad in ["00", "00ff00", "0", "zzzz", "00f"] {
            assert_eq!(decode_array::<2>(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn try_decode_agrees_with_decode() {
        for text in ["", "00", "009FfF", "0", "zz", "0g", "é"] {
            assert_eq!(try_decode(text), Ok(decode(text)), "{text:?}");
        }
    }
}
