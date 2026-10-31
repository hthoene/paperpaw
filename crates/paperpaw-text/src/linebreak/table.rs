//! The class table: a lookup of the line-break class of a character.
//!
//! The data is `table_data.rs`, made by `tools/linebreak-table.py` (the
//! format is described there): a flash-resident byte array read in place,
//! without parsing and without allocation.

use super::table_data::{CLASSES, Class, TABLE};

/// What the table knows of one character.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Entry {
    pub(crate) class: Class,
    /// East Asian width F, W or H (UAX #14's `$EastAsian`).
    pub(crate) wide: bool,
    /// UAX #14 decides near this character (the compat profile, see the
    /// design): CJK, Hangul, Thai and the like, U+200B.
    pub(crate) uax: bool,
}

/// The first Hangul syllable and the last: computed, not stored.
const HANGUL: (u32, u32) = (0xAC00, 0xD7A3);
/// The header: 4 magic, format, 3 version, class count, reserved, index and stream lengths.
const HEADER: usize = 14;

fn le16(at: usize) -> usize {
    usize::from(TABLE[at]) | usize::from(TABLE[at + 1]) << 8
}

fn le32(at: usize) -> u32 {
    u32::from(TABLE[at])
        | u32::from(TABLE[at + 1]) << 8
        | u32::from(TABLE[at + 2]) << 16
        | u32::from(TABLE[at + 3]) << 24
}

fn entry(byte: u8) -> Entry {
    let class = CLASSES
        .get(usize::from(byte & 0x3F))
        .copied()
        // An id this code does not know acts as a letter: no break inside,
        // a break at spaces (the design, section 4.4).
        .unwrap_or(Class::AL);
    Entry {
        class,
        wide: byte & 0x40 != 0,
        uax: byte & 0x80 != 0,
    }
}

/// The table's entry of `c`.
pub(crate) fn lookup(c: char) -> Entry {
    let cp = u32::from(c);
    let index_len = le16(10);
    let ascii = HEADER;
    if cp < 0x80 {
        return entry(TABLE[ascii + cp as usize]);
    }
    if (HANGUL.0..=HANGUL.1).contains(&cp) {
        return Entry {
            class: if (cp - HANGUL.0) % 28 == 0 {
                Class::H2
            } else {
                Class::H3
            },
            wide: true,
            uax: true,
        };
    }
    let index = ascii + 128;
    let stream = index + index_len * 6;
    // The last index entry that starts at or before `cp`.
    let (mut low, mut high) = (0, index_len);
    while high - low > 1 {
        let mid = (low + high) / 2;
        if le32(index + mid * 6) <= cp {
            low = mid;
        } else {
            high = mid;
        }
    }
    let mut start = le32(index + low * 6);
    let mut at = stream + le16(index + low * 6 + 4);
    loop {
        let byte = TABLE[at];
        at += 1;
        let mut length = 0u32;
        let mut shift = 0;
        loop {
            let b = TABLE[at];
            at += 1;
            length |= u32::from(b & 0x7F) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                break;
            }
        }
        if cp < start + length {
            return entry(byte);
        }
        start += length;
    }
}

/// The header, for the tests.
#[cfg(test)]
pub(crate) fn header() -> (u8, (u8, u8, u8), u8, usize, usize) {
    (
        TABLE[4],
        (TABLE[5], TABLE[6], TABLE[7]),
        TABLE[8],
        le16(10),
        le16(12),
    )
}
