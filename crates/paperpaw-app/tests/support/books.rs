//! Small books of the formats that are no text: a comic (CBZ) and a
//! pre-rendered XTC book, built in memory for the sleep tests (#248).

/// A 4×4 gray PNG, one comic page.
const PNG: [u8; 83] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x08, 0x00, 0x00, 0x00, 0x00, 0x8c, 0x9a, 0xc1,
    0xa2, 0x00, 0x00, 0x00, 0x1a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0xb0, 0xa9, 0xd8,
    0xc2, 0x20, 0x12, 0xd0, 0x73, 0x82, 0x41, 0x23, 0x65, 0xc1, 0x1d, 0x10, 0xef, 0x03, 0x00, 0x3e,
    0x94, 0x07, 0x81, 0x87, 0x5a, 0xf1, 0x08, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

/// A comic of four pages (stored ZIP entries).
pub fn cbz() -> Vec<u8> {
    let mut zip = paperpaw_zip::testing::ZipBuilder::new();
    for page in 1..=4 {
        zip = zip.stored(&format!("p{page}.png"), PNG.to_vec());
    }
    zip.build()
}

/// An XTC book of four 16×16 one-bit pages (`XTG\0`), with metadata and a
/// chapter per page, as `format-xtc`'s own tests build one.
pub fn xtc() -> Vec<u8> {
    const HEADER: u64 = 48;
    const METADATA: usize = 256;
    const CHAPTER: usize = 96;
    const INDEX_ENTRY: u64 = 16;
    let pages: Vec<Vec<u8>> = (0..4u8)
        .map(|at| {
            let data: Vec<u8> = (0..32u8).map(|b| b.wrapping_mul(at + 1)).collect();
            let mut page = b"XTG\0".to_vec();
            page.extend_from_slice(&16u16.to_le_bytes());
            page.extend_from_slice(&16u16.to_le_bytes());
            page.extend_from_slice(&[0, 0]);
            page.extend_from_slice(&(data.len() as u32).to_le_bytes());
            page.extend_from_slice(&[0; 8]);
            page.extend_from_slice(&data);
            page
        })
        .collect();
    let count = pages.len() as u64;
    let index = HEADER + (METADATA + CHAPTER * pages.len()) as u64;
    let data = index + INDEX_ENTRY * count;
    let mut out = b"XTC\0".to_vec();
    out.extend_from_slice(&0x0100u16.to_le_bytes());
    out.extend_from_slice(&(count as u16).to_le_bytes());
    out.extend_from_slice(&[0, 1, 0, 1]);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&HEADER.to_le_bytes());
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&data.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    let mut meta = vec![0u8; METADATA];
    meta[..5].copy_from_slice(b"Rooms");
    meta[128..131].copy_from_slice(b"Ann");
    meta[224..226].copy_from_slice(b"en");
    meta[244..246].copy_from_slice(&1u16.to_le_bytes());
    meta[246..248].copy_from_slice(&(count as u16).to_le_bytes());
    out.extend_from_slice(&meta);
    for at in 0..count {
        let mut chapter = vec![0u8; CHAPTER];
        let name = format!("Room {}", at + 1);
        chapter[..name.len()].copy_from_slice(name.as_bytes());
        chapter[80..82].copy_from_slice(&(at as u16).to_le_bytes());
        out.extend_from_slice(&chapter);
    }
    let mut at = data;
    for page in &pages {
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&(page.len() as u32).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        at += page.len() as u64;
    }
    for page in &pages {
        out.extend_from_slice(page);
    }
    out
}
