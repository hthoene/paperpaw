//! Reading a font file's family name and style from its header, without
//! loading the font: only the table directory and the small `name`,
//! `OS/2` and `head` tables are read, so a folder of large fonts can be
//! listed in little memory and time.

use alloc::string::String;
use alloc::vec::Vec;

/// What a font file says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FontInfo {
    /// The family, e.g. `Literata`; shared by the regular, bold and italic
    /// files of a family.
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    /// The weight, 100 (thin) to 900 (black); 400 is regular, 700 bold.
    pub weight: u16,
}

impl FontInfo {
    pub fn new(family: impl Into<String>, bold: bool, italic: bool, weight: u16) -> FontInfo {
        FontInfo {
            family: family.into(),
            bold,
            italic,
            weight,
        }
    }
}

/// Reads `buf.len()` bytes at an offset; returns how many were read.
pub type ReadAt<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> usize;

/// Largest `name` table read.
const MAX_NAME_TABLE: u32 = 64 * 1024;
/// Table records read at a time: the directory is not held whole, as its
/// size is only what the header claims.
const RECORDS: usize = 16;

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *data.get(at)?,
        *data.get(at + 1)?,
        *data.get(at + 2)?,
        *data.get(at + 3)?,
    ]))
}

fn read(read_at: ReadAt<'_>, offset: u64, len: u32) -> Option<Vec<u8>> {
    let len = len as usize;
    let mut buf = Vec::new();
    buf.try_reserve_exact(len).ok()?;
    buf.resize(len, 0);
    (read_at(offset, &mut buf) == len).then_some(buf)
}

/// The family and style of an OpenType/TrueType font (the first font of a
/// collection); `None` if it is none.
pub fn read_info(read_at: ReadAt<'_>) -> Option<FontInfo> {
    let header = read(read_at, 0, 12)?;
    let mut base = 0u64;
    let header = match &header[..4] {
        // A collection: use its first font.
        b"ttcf" => {
            let first = read(read_at, 12, 4)?;
            base = u64::from(u32_at(&first, 0)?);
            read(read_at, base, 12)?
        }
        [0, 1, 0, 0] | b"OTTO" | b"true" => header,
        _ => return None,
    };
    // Where the `name`, `OS/2` and `head` tables are (their first records).
    let tables = usize::from(u16_at(&header, 4)?);
    let (mut name, mut os2, mut head) = (None, None, None);
    let mut chunk = [0u8; RECORDS * 16];
    let mut done = 0;
    while done < tables {
        let records = &mut chunk[..(tables - done).min(RECORDS) * 16];
        if read_at(base + 12 + done as u64 * 16, records) != records.len() {
            return None;
        }
        for record in records.as_chunks::<16>().0 {
            let found = Some((u64::from(u32_at(record, 8)?), u32_at(record, 12)?));
            match &record[..4] {
                b"name" => name = name.or(found),
                b"OS/2" => os2 = os2.or(found),
                b"head" => head = head.or(found),
                _ => {}
            }
        }
        done += records.len() / 16;
    }

    let (offset, len) = name?;
    let name = read(read_at, offset, len.min(MAX_NAME_TABLE))?;
    let family = family_name(&name)?;

    // Style: OS/2 (weight and selection flags), else the head table.
    let (mut bold, mut italic) = (false, false);
    let mut weight = 400;
    if let Some((offset, len)) = os2.filter(|(_, len)| *len >= 64) {
        let os2 = read(read_at, offset, len.min(96))?;
        weight = u16_at(&os2, 4)?;
        let selection = u16_at(&os2, 62)?;
        italic = selection & 1 != 0;
        bold = selection & (1 << 5) != 0 || weight >= 600;
    } else if let Some((offset, _)) = head {
        let head = read(read_at, offset, 54)?;
        let style = u16_at(&head, 44)?;
        bold = style & 1 != 0;
        italic = style & 2 != 0;
        weight = if bold { 700 } else { 400 };
    }
    Some(FontInfo {
        family,
        bold,
        italic,
        weight,
    })
}

/// The family name (name id 1), else the typographic family (16); English
/// names first. Name 1 groups a regular, bold, italic and bold italic face
/// as systems do: a font's light or display cuts are families of their own
/// there, while name 16 would put all of them into one.
fn family_name(table: &[u8]) -> Option<String> {
    let count = usize::from(u16_at(table, 2)?);
    let strings = usize::from(u16_at(table, 4)?);
    let mut best: Option<(u8, String)> = None;
    for i in 0..count {
        let r = 6 + i * 12;
        let (platform, language, id) = (
            u16_at(table, r)?,
            u16_at(table, r + 4)?,
            u16_at(table, r + 6)?,
        );
        let (len, offset) = (
            usize::from(u16_at(table, r + 8)?),
            usize::from(u16_at(table, r + 10)?),
        );
        if id != 1 && id != 16 {
            continue;
        }
        let Some(bytes) = table.get(strings + offset..strings + offset + len) else {
            continue;
        };
        let text = match platform {
            // Windows and Unicode: UTF-16BE.
            0 | 3 => String::from_utf16_lossy(
                &bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect::<Vec<_>>(),
            ),
            // Macintosh: Roman.
            1 => bytes.iter().map(|&b| mac_roman(b)).collect(),
            _ => continue,
        };
        let english = language == 0x409 || (platform == 1 && language == 0);
        let rank = u8::from(id == 1) * 2 + u8::from(english);
        if !text.trim().is_empty() && best.as_ref().is_none_or(|(r, _)| rank > *r) {
            best = Some((rank, String::from(text.trim())));
        }
    }
    best.map(|(_, name)| name)
}

/// The upper half of Mac Roman, bytes 0x80 to 0xff.
const MAC_ROMAN: &str = "ÄÅÇÉÑÖÜáàâäãåçéèêëíìîïñóòôöõúùûü\
    †°¢£§•¶ß®©™´¨≠ÆØ∞±≤≥¥µ∂∑∏π∫ªºΩæø\
    ¿¡¬√ƒ≈∆«»…\u{a0}ÀÃÕŒœ\u{2013}\u{2014}“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ\
    ‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔ\u{f8ff}ÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";

/// A Mac Roman byte as a character; control characters, which have no
/// place in a name, become U+FFFD.
fn mac_roman(b: u8) -> char {
    match b {
        0x20..=0x7e => char::from(b),
        0x80..=0xff => MAC_ROMAN
            .chars()
            .nth(usize::from(b - 0x80))
            .unwrap_or(char::REPLACEMENT_CHARACTER),
        _ => char::REPLACEMENT_CHARACTER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(data: &[u8]) -> Option<FontInfo> {
        read_info(&mut |offset, buf: &mut [u8]| {
            let start = (offset as usize).min(data.len());
            let n = buf.len().min(data.len() - start);
            buf[..n].copy_from_slice(&data[start..start + n]);
            n
        })
    }

    #[test]
    fn reads_family_and_style() {
        let regular = info(include_bytes!(
            "../../../plugins/fonts-default/fonts/Literata.ttf"
        ));
        assert_eq!(
            regular,
            Some(FontInfo {
                family: String::from("Literata"),
                bold: false,
                italic: false,
                weight: 400,
            })
        );
        let italic = info(include_bytes!(
            "../../../plugins/fonts-default/fonts/Literata-Italic.ttf"
        ))
        .unwrap();
        assert_eq!(italic.family, "Literata");
        assert!(italic.italic && !italic.bold);
        assert_eq!(info(b"not a font at all"), None);
    }
}
