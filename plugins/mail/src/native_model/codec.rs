//! The encodings of mail (RFC 2045, 2047) for the native plugin: base64,
//! quoted-printable and the encoded words of headers, and the characters of the
//! common charsets as UTF-8. It is the interface-6 `codec` made fallible (a
//! refused allocation is `NoMemory`, never an abort) and free of indexing that
//! could panic; each part is tested to give the same answers as the old one.
//! Everything takes bytes of any shape and gives something back.

use alloc::string::String;
use alloc::vec::Vec;

use super::mem::{
    Error, SecretBytes, lossy, push_byte, push_bytes, push_char, push_str, string_with, vec_with,
};

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

/// Bytes of base64 text; anything that is no digit of it (line breaks,
/// spaces) is skipped, and the first `=` ends it.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn decode_base64(input: &[u8]) -> Result<Vec<u8>, Error> {
    let mut decoder = Base64::default();
    let mut out = vec_with(input.len() / 4 * 3 + 3)?;
    decoder.push(input, &mut out)?;
    decoder.finish(&mut out)?;
    Ok(out)
}

/// Base64 that comes in pieces: `push` as many bytes as there are; what
/// does not make a whole group waits for the next piece.
#[derive(Debug, Default, Clone)]
pub struct Base64 {
    bits: u32,
    count: u8,
    done: bool,
}

impl Base64 {
    /// Decodes `input` onto `out`.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; the group in hand is lost with it.
    pub fn push(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), Error> {
        for &byte in input {
            if self.done {
                return Ok(());
            }
            if byte == b'=' {
                self.done = true;
                return Ok(());
            }
            let Some(value) = base64_value(byte) else {
                continue;
            };
            self.bits = (self.bits << 6) | u32::from(value);
            self.count += 1;
            if self.count == 4 {
                let group = [
                    (self.bits >> 16) as u8,
                    (self.bits >> 8) as u8,
                    self.bits as u8,
                ];
                self.bits = 0;
                self.count = 0;
                push_bytes(out, &group)?;
            }
        }
        Ok(())
    }

    /// What the last, incomplete group gives (a stray digit gives none).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn finish(&mut self, out: &mut Vec<u8>) -> Result<(), Error> {
        let (bits, count) = (self.bits, self.count);
        self.bits = 0;
        self.count = 0;
        match count {
            2 => push_byte(out, (bits >> 4) as u8),
            3 => push_bytes(out, &[(bits >> 10) as u8, (bits >> 2) as u8]),
            _ => Ok(()),
        }
    }
}

/// `input` as base64 onto `out`: in lines of at most 76 characters, each ended
/// by CRLF (`lines`), or on one line for a protocol's command.
///
/// # Errors
/// [`Error::NoMemory`]; `out` keeps what was written before.
pub fn encode_base64_into(input: &[u8], out: &mut String, lines: bool) -> Result<(), Error> {
    out.try_reserve(input.len() / 3 * 4 + 4 + if lines { input.len() / 57 * 2 + 2 } else { 0 })
        .map_err(|_| Error::NoMemory)?;
    let mut column = 0;
    for group in input.chunks(3) {
        let at = |i: usize| u32::from(group.get(i).copied().unwrap_or(0));
        let n = at(0) << 16 | at(1) << 8 | at(2);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= group.len() {
                let digit = BASE64
                    .get((n >> shift) as usize & 63)
                    .copied()
                    .unwrap_or(b'A');
                push_char(out, char::from(digit))?;
            } else {
                push_char(out, '=')?;
            }
        }
        column += 4;
        if lines && column == 76 {
            push_str(out, "\r\n")?;
            column = 0;
        }
    }
    if lines && column > 0 {
        push_str(out, "\r\n")?;
    }
    Ok(())
}

/// `input` as base64 in lines of at most 76 characters, each ended by
/// CRLF; `""` for no input.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn encode_base64(input: &[u8]) -> Result<String, Error> {
    let mut out = String::new();
    encode_base64_into(input, &mut out, true)?;
    Ok(out)
}

/// Base64 on one line, for a protocol's command (no line breaks).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn base64_line(input: &[u8]) -> Result<String, Error> {
    let mut out = String::new();
    encode_base64_into(input, &mut out, false)?;
    Ok(out)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Quoted-printable of a body, lines and all (`=XX`, a `=` ending a line
/// joins it to the next); `header`: as in an encoded word (RFC 2047),
/// where `_` is a space.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn decode_qp(input: &[u8], header: bool) -> Result<Vec<u8>, Error> {
    let mut out = vec_with(input.len())?;
    let mut i = 0;
    while let Some(&byte) = input.get(i) {
        if byte == b'_' && header {
            push_byte(&mut out, b' ')?;
        } else if byte == b'=' {
            let pair = (
                input.get(i + 1).copied().and_then(hex),
                input.get(i + 2).copied().and_then(hex),
            );
            if let (Some(high), Some(low)) = pair {
                push_byte(&mut out, high << 4 | low)?;
                i += 3;
                continue;
            }
            // A soft line break: `=` at the end of a line (white space
            // before the break is allowed).
            let mut j = i + 1;
            while matches!(input.get(j), Some(b' ' | b'\t')) {
                j += 1;
            }
            match (input.get(j), input.get(j + 1)) {
                (Some(b'\r'), Some(b'\n')) => i = j + 2,
                (Some(b'\n'), _) => i = j + 1,
                (None, _) => i = j,
                _ => {
                    push_byte(&mut out, b'=')?;
                    i += 1;
                }
            }
            continue;
        } else {
            push_byte(&mut out, byte)?;
        }
        i += 1;
    }
    Ok(out)
}

/// `text` as quoted-printable (UTF-8 bytes as `=XX`, lines of at most 76
/// characters, ended by CRLF). Line breaks of the text, LF or CRLF, stay
/// line breaks.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn encode_qp(text: &str) -> Result<String, Error> {
    let mut out = string_with(text.len() + text.len() / 8)?;
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let bytes = line.as_bytes();
        let mut column = 0;
        for (i, &byte) in bytes.iter().enumerate() {
            let last = i + 1 == bytes.len();
            // White space at the end of a line must be encoded.
            let plain = (byte == b' ' || byte == b'\t') && !last
                || (33..=126).contains(&byte) && byte != b'=';
            let width = if plain { 1 } else { 3 };
            if column + width > 75 {
                push_str(&mut out, "=\r\n")?;
                column = 0;
            }
            if plain {
                push_char(&mut out, char::from(byte))?;
            } else {
                let digit = |n: u8| char::from(b"0123456789ABCDEF"[usize::from(n & 15)]);
                push_char(&mut out, '=')?;
                push_char(&mut out, digit(byte >> 4))?;
                push_char(&mut out, digit(byte))?;
            }
            column += width;
        }
        push_str(&mut out, "\r\n")?;
    }
    // `split` of a text that ends in a line break gave one more empty line.
    if text.ends_with('\n') {
        out.truncate(out.len().saturating_sub(2));
    }
    Ok(out)
}

/// A charset of mail that can be turned into text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    Utf8,
    /// US-ASCII, and anything above it as is (Latin-1).
    Latin1,
    Latin9,
    Windows1252,
    Latin2,
    Windows1250,
    Windows1251,
    Koi8R,
}

impl Charset {
    /// The charset a name (of a header or a parameter) says; `None` for
    /// one that is not known. No allocation: the name is compared as it is.
    pub fn of(name: &str) -> Option<Self> {
        let name = name.trim().trim_matches('"');
        // RFC 2231's language suffix: `utf-8*de`.
        let name = name.split('*').next().unwrap_or("");
        let is = |names: &[&str]| names.iter().any(|n| name.eq_ignore_ascii_case(n));
        Some(if is(&["", "utf-8", "utf8"]) {
            Charset::Utf8
        } else if is(&[
            "us-ascii",
            "ascii",
            "ansi_x3.4-1968",
            "iso-8859-1",
            "iso8859-1",
            "latin1",
            "l1",
            "iso_8859-1",
        ]) {
            Charset::Latin1
        } else if is(&["iso-8859-15", "iso8859-15", "latin9", "latin-9"]) {
            Charset::Latin9
        } else if is(&["windows-1252", "cp1252", "x-cp1252"]) {
            Charset::Windows1252
        } else if is(&["iso-8859-2", "iso8859-2", "latin2"]) {
            Charset::Latin2
        } else if is(&["windows-1250", "cp1250"]) {
            Charset::Windows1250
        } else if is(&["windows-1251", "cp1251"]) {
            Charset::Windows1251
        } else if is(&["koi8-r", "koi8r"]) {
            Charset::Koi8R
        } else {
            return None;
        })
    }

    /// The character a byte above 127 stands for (the byte itself for
    /// the ASCII ones).
    fn char_of(self, byte: u8) -> char {
        if byte < 0x80 {
            return char::from(byte);
        }
        let high = |table: &[u16; 128]| {
            let unit = table
                .get(usize::from(byte - 0x80))
                .copied()
                .unwrap_or(0xFFFD);
            char::from_u32(u32::from(unit)).unwrap_or('\u{fffd}')
        };
        match self {
            Charset::Utf8 | Charset::Latin1 => char::from(byte),
            Charset::Latin9 => match byte {
                0xA4 => '€',
                0xA6 => 'Š',
                0xA8 => 'š',
                0xB4 => 'Ž',
                0xB8 => 'ž',
                0xBC => 'Œ',
                0xBD => 'œ',
                0xBE => 'Ÿ',
                _ => char::from(byte),
            },
            Charset::Windows1252 => {
                if (0x80..0xA0).contains(&byte) {
                    high(&CP1252)
                } else {
                    char::from(byte)
                }
            }
            Charset::Latin2 => high(&LATIN2),
            Charset::Windows1250 => high(&CP1250),
            Charset::Windows1251 => high(&CP1251),
            Charset::Koi8R => high(&KOI8R),
        }
    }
}

const CP1252: [u16; 128] = {
    let mut t = [0u16; 128];
    let mut i = 0;
    while i < 128 {
        t[i] = 0x80 + i as u16;
        i += 1;
    }
    let specials: [(usize, u16); 27] = [
        (0, 0x20AC),
        (2, 0x201A),
        (3, 0x0192),
        (4, 0x201E),
        (5, 0x2026),
        (6, 0x2020),
        (7, 0x2021),
        (8, 0x02C6),
        (9, 0x2030),
        (10, 0x0160),
        (11, 0x2039),
        (12, 0x0152),
        (14, 0x017D),
        (17, 0x2018),
        (18, 0x2019),
        (19, 0x201C),
        (20, 0x201D),
        (21, 0x2022),
        (22, 0x2013),
        (23, 0x2014),
        (24, 0x02DC),
        (25, 0x2122),
        (26, 0x0161),
        (27, 0x203A),
        (28, 0x0153),
        (30, 0x017E),
        (31, 0x0178),
    ];
    let mut k = 0;
    while k < 27 {
        t[specials[k].0] = specials[k].1;
        k += 1;
    }
    t
};

const LATIN2: [u16; 128] = [
    0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x8B, 0x8C, 0x8D, 0x8E, 0x8F,
    0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0x9B, 0x9C, 0x9D, 0x9E, 0x9F,
    0xA0, 0x104, 0x2D8, 0x141, 0xA4, 0x13D, 0x15A, 0xA7, 0xA8, 0x160, 0x15E, 0x164, 0x179, 0xAD,
    0x17D, 0x17B, 0xB0, 0x105, 0x2DB, 0x142, 0xB4, 0x13E, 0x15B, 0x2C7, 0xB8, 0x161, 0x15F, 0x165,
    0x17A, 0x2DD, 0x17E, 0x17C, 0x154, 0xC1, 0xC2, 0x102, 0xC4, 0x139, 0x106, 0xC7, 0x10C, 0xC9,
    0x118, 0xCB, 0x11A, 0xCD, 0xCE, 0x10E, 0x110, 0x143, 0x147, 0xD3, 0xD4, 0x150, 0xD6, 0xD7,
    0x158, 0x16E, 0xDA, 0x170, 0xDC, 0xDD, 0x162, 0xDF, 0x155, 0xE1, 0xE2, 0x103, 0xE4, 0x13A,
    0x107, 0xE7, 0x10D, 0xE9, 0x119, 0xEB, 0x11B, 0xED, 0xEE, 0x10F, 0x111, 0x144, 0x148, 0xF3,
    0xF4, 0x151, 0xF6, 0xF7, 0x159, 0x16F, 0xFA, 0x171, 0xFC, 0xFD, 0x163, 0x2D9,
];

const CP1250: [u16; 128] = [
    0x20AC, 0x81, 0x201A, 0x83, 0x201E, 0x2026, 0x2020, 0x2021, 0x88, 0x2030, 0x160, 0x2039, 0x15A,
    0x164, 0x17D, 0x179, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x98,
    0x2122, 0x161, 0x203A, 0x15B, 0x165, 0x17E, 0x17A, 0xA0, 0x2C7, 0x2D8, 0x141, 0xA4, 0x104,
    0xA6, 0xA7, 0xA8, 0xA9, 0x15E, 0xAB, 0xAC, 0xAD, 0xAE, 0x17B, 0xB0, 0xB1, 0x2DB, 0x142, 0xB4,
    0xB5, 0xB6, 0xB7, 0xB8, 0x105, 0x15F, 0xBB, 0x13D, 0x2DD, 0x13E, 0x17C, 0x154, 0xC1, 0xC2,
    0x102, 0xC4, 0x139, 0x106, 0xC7, 0x10C, 0xC9, 0x118, 0xCB, 0x11A, 0xCD, 0xCE, 0x10E, 0x110,
    0x143, 0x147, 0xD3, 0xD4, 0x150, 0xD6, 0xD7, 0x158, 0x16E, 0xDA, 0x170, 0xDC, 0xDD, 0x162,
    0xDF, 0x155, 0xE1, 0xE2, 0x103, 0xE4, 0x13A, 0x107, 0xE7, 0x10D, 0xE9, 0x119, 0xEB, 0x11B,
    0xED, 0xEE, 0x10F, 0x111, 0x144, 0x148, 0xF3, 0xF4, 0x151, 0xF6, 0xF7, 0x159, 0x16F, 0xFA,
    0x171, 0xFC, 0xFD, 0x163, 0x2D9,
];

const CP1251: [u16; 128] = [
    0x402, 0x403, 0x201A, 0x453, 0x201E, 0x2026, 0x2020, 0x2021, 0x20AC, 0x2030, 0x409, 0x2039,
    0x40A, 0x40C, 0x40B, 0x40F, 0x452, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x98, 0x2122, 0x459, 0x203A, 0x45A, 0x45C, 0x45B, 0x45F, 0xA0, 0x40E, 0x45E, 0x408, 0xA4,
    0x490, 0xA6, 0xA7, 0x401, 0xA9, 0x404, 0xAB, 0xAC, 0xAD, 0xAE, 0x407, 0xB0, 0xB1, 0x406, 0x456,
    0x491, 0xB5, 0xB6, 0xB7, 0x451, 0x2116, 0x454, 0xBB, 0x458, 0x405, 0x455, 0x457, 0x410, 0x411,
    0x412, 0x413, 0x414, 0x415, 0x416, 0x417, 0x418, 0x419, 0x41A, 0x41B, 0x41C, 0x41D, 0x41E,
    0x41F, 0x420, 0x421, 0x422, 0x423, 0x424, 0x425, 0x426, 0x427, 0x428, 0x429, 0x42A, 0x42B,
    0x42C, 0x42D, 0x42E, 0x42F, 0x430, 0x431, 0x432, 0x433, 0x434, 0x435, 0x436, 0x437, 0x438,
    0x439, 0x43A, 0x43B, 0x43C, 0x43D, 0x43E, 0x43F, 0x440, 0x441, 0x442, 0x443, 0x444, 0x445,
    0x446, 0x447, 0x448, 0x449, 0x44A, 0x44B, 0x44C, 0x44D, 0x44E, 0x44F,
];

const KOI8R: [u16; 128] = [
    0x2500, 0x2502, 0x250C, 0x2510, 0x2514, 0x2518, 0x251C, 0x2524, 0x252C, 0x2534, 0x253C, 0x2580,
    0x2584, 0x2588, 0x258C, 0x2590, 0x2591, 0x2592, 0x2593, 0x2320, 0x25A0, 0x2219, 0x221A, 0x2248,
    0x2264, 0x2265, 0xA0, 0x2321, 0xB0, 0xB2, 0xB7, 0xF7, 0x2550, 0x2551, 0x2552, 0x451, 0x2553,
    0x2554, 0x2555, 0x2556, 0x2557, 0x2558, 0x2559, 0x255A, 0x255B, 0x255C, 0x255D, 0x255E, 0x255F,
    0x2560, 0x2561, 0x401, 0x2562, 0x2563, 0x2564, 0x2565, 0x2566, 0x2567, 0x2568, 0x2569, 0x256A,
    0x256B, 0x256C, 0xA9, 0x44E, 0x430, 0x431, 0x446, 0x434, 0x435, 0x444, 0x433, 0x445, 0x438,
    0x439, 0x43A, 0x43B, 0x43C, 0x43D, 0x43E, 0x43F, 0x44F, 0x440, 0x441, 0x442, 0x443, 0x436,
    0x432, 0x44C, 0x44B, 0x437, 0x448, 0x44D, 0x449, 0x447, 0x44A, 0x42E, 0x410, 0x411, 0x426,
    0x414, 0x415, 0x424, 0x413, 0x425, 0x418, 0x419, 0x41A, 0x41B, 0x41C, 0x41D, 0x41E, 0x41F,
    0x42F, 0x420, 0x421, 0x422, 0x423, 0x416, 0x412, 0x42C, 0x42B, 0x417, 0x428, 0x42D, 0x429,
    0x427, 0x42A,
];

/// Text in `charset` as it comes in pieces, whatever the pieces cut:
/// UTF-8 characters cut in two wait for the next piece.
#[derive(Debug, Clone)]
pub struct Decoder {
    charset: Charset,
    /// The start of a UTF-8 character that was cut.
    held: [u8; 4],
    held_len: u8,
}

impl Decoder {
    pub fn new(charset: Charset) -> Self {
        Self {
            charset,
            held: [0; 4],
            held_len: 0,
        }
    }

    /// Appends the text of `bytes` to `out`; bytes that are no text in
    /// the charset come as U+FFFD.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; the piece is lost with it.
    pub fn push(&mut self, bytes: &[u8], out: &mut String) -> Result<(), Error> {
        if self.charset != Charset::Utf8 {
            out.try_reserve(bytes.len()).map_err(|_| Error::NoMemory)?;
            for &byte in bytes {
                push_char(out, self.charset.char_of(byte))?;
            }
            return Ok(());
        }
        // A character cut at the end of the last piece is finished with the
        // front of this one.
        let held = usize::from(self.held_len).min(self.held.len());
        let mut joined: Vec<u8> = Vec::new();
        let mut input = bytes;
        if held > 0 {
            joined
                .try_reserve_exact(held + bytes.len())
                .map_err(|_| Error::NoMemory)?;
            joined.extend_from_slice(self.held.get(..held).unwrap_or(&[]));
            joined.extend_from_slice(bytes);
            self.held_len = 0;
            input = &joined;
        }
        loop {
            match core::str::from_utf8(input) {
                Ok(text) => return push_str(out, text),
                Err(error) => {
                    let (good, rest) = input.split_at(error.valid_up_to());
                    push_str(out, core::str::from_utf8(good).unwrap_or(""))?;
                    match error.error_len() {
                        Some(skip) => {
                            push_char(out, '\u{fffd}')?;
                            input = rest.get(skip..).unwrap_or(&[]);
                        }
                        None => {
                            // Cut at the end: wait for the rest.
                            let n = rest.len().min(4);
                            if let (Some(to), Some(from)) = (self.held.get_mut(..n), rest.get(..n))
                            {
                                to.copy_from_slice(from);
                            }
                            self.held_len = n as u8;
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    /// A character left cut at the very end is no character.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn finish(&mut self, out: &mut String) -> Result<(), Error> {
        if self.held_len > 0 {
            self.held_len = 0;
            push_char(out, '\u{fffd}')?;
        }
        Ok(())
    }
}

/// `bytes` in `charset` as text.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn text_of(charset: Charset, bytes: &[u8]) -> Result<String, Error> {
    let mut out = string_with(bytes.len())?;
    let mut decoder = Decoder::new(charset);
    decoder.push(bytes, &mut out)?;
    decoder.finish(&mut out)?;
    Ok(out)
}

/// The words of a header (RFC 2047: `=?utf-8?B?…?=`, `=?iso-8859-1?Q?…?=`)
/// as text; what is no encoded word stays as it is (8-bit text as UTF-8,
/// else as Latin-1). White space between two encoded words goes.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn decode_words(raw: &[u8]) -> Result<String, Error> {
    let latin;
    let text = match core::str::from_utf8(raw) {
        Ok(text) => text,
        Err(_) => {
            latin = text_of(Charset::Latin1, raw)?;
            latin.as_str()
        }
    };
    let mut out = string_with(text.len())?;
    let mut rest = text;
    let mut after_word = false;
    while !rest.is_empty() {
        if let Some((word, tail)) = encoded_word(rest)? {
            push_str(&mut out, &word)?;
            rest = tail;
            after_word = true;
            continue;
        }
        // White space between encoded words is not text.
        if after_word {
            let trimmed = rest.trim_start_matches([' ', '\t']);
            if trimmed.len() != rest.len() && encoded_word(trimmed)?.is_some() {
                rest = trimmed;
                continue;
            }
        }
        let mut chars = rest.chars();
        if let Some(c) = chars.next() {
            push_char(&mut out, c)?;
        }
        rest = chars.as_str();
        after_word = false;
    }
    Ok(out)
}

/// The encoded word at the start of `text`, decoded, and what follows.
fn encoded_word(text: &str) -> Result<Option<(String, &str)>, Error> {
    let Some(body) = text.strip_prefix("=?") else {
        return Ok(None);
    };
    let Some((charset, body)) = body.split_once('?') else {
        return Ok(None);
    };
    let Some((encoding, body)) = body.split_once('?') else {
        return Ok(None);
    };
    let Some((data, rest)) = body.split_once("?=") else {
        return Ok(None);
    };
    if data.contains([' ', '\t']) || charset.is_empty() || encoding.len() != 1 {
        return Ok(None);
    }
    let bytes = match encoding {
        "B" | "b" => decode_base64(data.as_bytes())?,
        "Q" | "q" => decode_qp(data.as_bytes(), true)?,
        _ => return Ok(None),
    };
    // A charset not known: the bytes as they are, if they are text.
    let charset = Charset::of(charset).unwrap_or(Charset::Utf8);
    Ok(Some((text_of(charset, &bytes)?, rest)))
}

/// `text` as a header's value: as it is where it is plain ASCII, else
/// as encoded words of UTF-8 (RFC 2047), each short enough for a line.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn encode_words(text: &str) -> Result<String, Error> {
    if text.is_ascii() && !text.contains(['\r', '\n']) {
        return super::mem::copy(text);
    }
    let mut out = String::new();
    let mut piece = String::new();
    for c in text.chars().filter(|c| *c != '\r' && *c != '\n') {
        // 30 bytes a word: 40 characters of base64, 52 with the frame.
        if piece.len() + c.len_utf8() > 30 {
            flush_word(&mut piece, &mut out)?;
        }
        push_char(&mut piece, c)?;
    }
    flush_word(&mut piece, &mut out)?;
    Ok(out)
}

fn flush_word(piece: &mut String, out: &mut String) -> Result<(), Error> {
    if piece.is_empty() {
        return Ok(());
    }
    if !out.is_empty() {
        push_str(out, "\r\n ")?;
    }
    push_str(out, "=?utf-8?B?")?;
    encode_base64_into(piece.as_bytes(), out, false)?;
    push_str(out, "?=")?;
    piece.clear();
    Ok(())
}

/// The credentials of `AUTH PLAIN` as bytes that wipe themselves
/// (`\0user\0password`).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn plain_credentials(user: &str, password: &str) -> Result<SecretBytes, Error> {
    let mut data = SecretBytes::with_capacity(user.len() + password.len() + 2)?;
    data.push_byte(0)?;
    data.push_bytes(user.as_bytes())?;
    data.push_byte(0)?;
    data.push_bytes(password.as_bytes())?;
    Ok(data)
}

/// `text` from bytes of any charset, as a lossy UTF-8 reading would.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn utf8_lossy(bytes: &[u8]) -> Result<String, Error> {
    lossy(bytes)
}
