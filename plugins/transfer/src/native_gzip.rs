//! The transfer page as one gzip stream, put together as the browser is
//! sent it (#428). The page is 11 KB of HTML, with the device's texts in
//! the middle of it; the program carries what comes before and after them
//! deflated (`build.rs`) and writes the texts between as a stored block, so
//! the page is made the way it was, in memory, and only deflated.
//!
//! ```text
//! gzip header, 10 bytes
//! the text before the texts, deflated, ended on a byte boundary (not the last block)
//! the texts, in stored blocks of at most 65,535 bytes
//! the text after them, deflated, the last block
//! the CRC-32 of all of it, its length, 4 bytes each
//! ```
//!
//! The CRC-32 is that of the parts joined (`combine`).

use alloc::vec::Vec;
use paperpaw_sdk::native::Error;

include!(concat!(env!("OUT_DIR"), "/page_parts.rs"));

static HEAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/page_head.deflate"));
static TAIL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/page_tail.deflate"));

/// A gzip header: deflate, no flags, no time, no extra flags, any system.
const HEADER: [u8; 10] = [0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
/// The most a stored block holds.
const STORED: usize = 0xffff;

/// The page, in gzip, with `texts` where the page names the place for them.
///
/// # Errors
/// `NoMemory`.
pub fn page(texts: &str) -> Result<Vec<u8>, Error> {
    let blocks = texts.len().div_ceil(STORED);
    let mut out = Vec::new();
    out.try_reserve_exact(HEADER.len() + HEAD.len() + TAIL.len() + texts.len() + blocks * 5 + 8)
        .map_err(|_| Error::NoMemory)?;
    out.extend_from_slice(&HEADER);
    out.extend_from_slice(HEAD);
    for block in texts.as_bytes().chunks(STORED) {
        // Not the last block, stored; its length and the length's complement.
        let len = (block.len() as u16).to_le_bytes();
        out.push(0);
        out.extend_from_slice(&len);
        out.extend_from_slice(&[!len[0], !len[1]]);
        out.extend_from_slice(block);
    }
    out.extend_from_slice(TAIL);
    let crc = combine(
        combine(HEAD_CRC, crc32(texts.as_bytes()), texts.len() as u32),
        TAIL_CRC,
        TAIL_LEN,
    );
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(HEAD_LEN + texts.len() as u32 + TAIL_LEN).to_le_bytes());
    Ok(out)
}

/// The CRC-32 (IEEE) of `data`.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = crc >> 1 ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

/// `mat` times the bit vector `vec`, over GF(2).
fn times(mat: &[u32; 32], mut vec: u32) -> u32 {
    let mut sum = 0;
    for row in mat {
        if vec == 0 {
            break;
        }
        if vec & 1 != 0 {
            sum ^= row;
        }
        vec >>= 1;
    }
    sum
}

fn square(mat: &[u32; 32]) -> [u32; 32] {
    core::array::from_fn(|n| times(mat, mat[n]))
}

/// The CRC-32 of two pieces joined, from that of each and the second's
/// length (zlib's `crc32_combine`: the first's CRC moved over as many zero
/// bytes as the second is long, by powers of the matrix that moves it over
/// one bit).
fn combine(mut first: u32, second: u32, mut len: u32) -> u32 {
    if len == 0 {
        return first;
    }
    // The operator for one zero bit, then (by squaring) two, four and eight bits.
    let mut odd = [0u32; 32];
    odd[0] = 0xedb8_8320;
    for n in 1..32 {
        odd[n] = 1 << (n - 1);
    }
    let mut even = square(&odd);
    odd = square(&even);
    loop {
        even = square(&odd);
        if len & 1 != 0 {
            first = times(&even, first);
        }
        len >>= 1;
        if len == 0 {
            break;
        }
        odd = square(&even);
        if len & 1 != 0 {
            first = times(&odd, first);
        }
        len >>= 1;
        if len == 0 {
            break;
        }
    }
    first ^ second
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use std::io::Read;

    const PAGE: &str = include_str!("../data/page.html");
    const PLACEHOLDER: &str = "/*STRINGS*/{}";

    fn inflated(gzip: &[u8]) -> String {
        let mut text = String::new();
        flate2::read::GzDecoder::new(gzip)
            .read_to_string(&mut text)
            .expect("a gzip stream the browser reads: the checksum and the length hold");
        text
    }

    #[test]
    fn the_page_inflates_to_what_it_was_with_the_texts_in_place() {
        for texts in [
            "{}",
            "{a:1}",
            "{title:\"Sändé ü\",books:[\"epub\",\"pdf\"]}",
            // Longer than one stored block.
            &alloc::format!("{{x:\"{}\"}}", "y".repeat(150_000)),
            "",
        ] {
            let sent = page(texts).unwrap();
            assert_eq!(
                inflated(&sent),
                PAGE.replacen(PLACEHOLDER, texts, 1),
                "{}",
                texts.len()
            );
        }
    }

    #[test]
    fn the_checksum_of_parts_is_that_of_the_whole() {
        let (a, b) = ("The quick brown fox ", "jumps over the lazy dog");
        let whole = crc32(alloc::format!("{a}{b}").as_bytes());
        assert_eq!(whole, 0x414f_a339);
        assert_eq!(
            combine(crc32(a.as_bytes()), crc32(b.as_bytes()), b.len() as u32),
            whole
        );
        assert_eq!(combine(crc32(a.as_bytes()), 0, 0), crc32(a.as_bytes()));
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn the_page_is_a_third_of_its_size() {
        assert!(page("{}").unwrap().len() * 2 < PAGE.len());
    }
}
