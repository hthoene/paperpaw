//! Makes Mobipocket books for the tests: the Palm database, the PalmDOC and
//! MOBI headers, EXTH records, text records (compressed or not), pictures.

#![allow(dead_code, reason = "each test uses some of it")]

pub mod io;

pub struct Builder {
    pub text: Vec<u8>,
    pub compress: bool,
    pub utf8: bool,
    pub title: String,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub subjects: Vec<String>,
    pub pictures: Vec<Vec<u8>>,
    pub cover: Option<u32>,
    pub compression: Option<u16>,
    pub encryption: u16,
    pub version: u32,
    /// Text records end with their trailing data (flags bit 0 and 1).
    pub trailing: bool,
    pub record_size: usize,
}

impl Builder {
    pub fn new(html: &str) -> Self {
        Builder {
            text: html.as_bytes().to_vec(),
            compress: true,
            utf8: true,
            title: String::from("A Test Book"),
            authors: vec![String::from("Ann Author")],
            language: Some(String::from("en")),
            subjects: Vec::new(),
            pictures: Vec::new(),
            cover: None,
            compression: None,
            encryption: 0,
            version: 6,
            trailing: false,
            record_size: 4096,
        }
    }

    pub fn bytes(text: &[u8]) -> Self {
        let mut builder = Self::new("");
        builder.text = text.to_vec();
        builder
    }

    fn exth(&self) -> Vec<u8> {
        let mut records: Vec<(u32, Vec<u8>)> = Vec::new();
        for author in &self.authors {
            records.push((100, author.as_bytes().to_vec()));
        }
        for subject in &self.subjects {
            records.push((105, subject.as_bytes().to_vec()));
        }
        records.push((503, self.title.as_bytes().to_vec()));
        if let Some(language) = &self.language {
            records.push((524, language.as_bytes().to_vec()));
        }
        if let Some(cover) = self.cover {
            records.push((201, cover.to_be_bytes().to_vec()));
        }
        let mut body = Vec::new();
        for (kind, data) in &records {
            body.extend_from_slice(&kind.to_be_bytes());
            body.extend_from_slice(&((data.len() + 8) as u32).to_be_bytes());
            body.extend_from_slice(data);
        }
        let mut exth = b"EXTH".to_vec();
        exth.extend_from_slice(&((body.len() + 12) as u32).to_be_bytes());
        exth.extend_from_slice(&(records.len() as u32).to_be_bytes());
        exth.extend_from_slice(&body);
        while !exth.len().is_multiple_of(4) {
            exth.push(0);
        }
        exth
    }

    pub fn build(&self) -> Vec<u8> {
        let size = self.record_size;
        let chunks: Vec<&[u8]> = if self.text.is_empty() {
            vec![&[][..]]
        } else {
            self.text.chunks(size).collect()
        };
        let mut text_records: Vec<Vec<u8>> = Vec::new();
        for chunk in &chunks {
            let mut record = if self.compress {
                compress(chunk)
            } else {
                chunk.to_vec()
            };
            if self.trailing {
                // A byte for a character cut off (none: its low bits are 0),
                // then an entry of 3 bytes (its size, 0x83, comes last):
                // bits 0 and 1 of the flags.
                record.push(0);
                record.extend_from_slice(&[7, 7, 0x83]);
            }
            text_records.push(record);
        }
        let exth = self.exth();
        let header_len = 232usize;
        let mut first = vec![0u8; 16 + header_len];
        let compression = self
            .compression
            .unwrap_or(if self.compress { 2 } else { 1 });
        first[0..2].copy_from_slice(&compression.to_be_bytes());
        first[4..8].copy_from_slice(&(self.text.len() as u32).to_be_bytes());
        first[8..10].copy_from_slice(&(text_records.len() as u16).to_be_bytes());
        first[10..12].copy_from_slice(&(size as u16).to_be_bytes());
        first[12..14].copy_from_slice(&self.encryption.to_be_bytes());
        first[16..20].copy_from_slice(b"MOBI");
        first[20..24].copy_from_slice(&(header_len as u32).to_be_bytes());
        first[24..28].copy_from_slice(&2u32.to_be_bytes());
        let encoding: u32 = if self.utf8 { 65001 } else { 1252 };
        first[28..32].copy_from_slice(&encoding.to_be_bytes());
        first[36..40].copy_from_slice(&self.version.to_be_bytes());
        let name_at = (first.len() + exth.len()) as u32;
        first[84..88].copy_from_slice(&name_at.to_be_bytes());
        first[88..92].copy_from_slice(&(self.title.len() as u32).to_be_bytes());
        let first_image = if self.pictures.is_empty() {
            0xffff_ffffu32
        } else {
            1 + text_records.len() as u32
        };
        first[108..112].copy_from_slice(&first_image.to_be_bytes());
        first[128..132].copy_from_slice(&0x40u32.to_be_bytes());
        if self.trailing {
            first[0xf2..0xf4].copy_from_slice(&3u16.to_be_bytes());
        }
        first.extend_from_slice(&exth);
        first.extend_from_slice(self.title.as_bytes());
        while !first.len().is_multiple_of(4) {
            first.push(0);
        }
        let mut records = vec![first];
        records.extend(text_records);
        records.extend(self.pictures.iter().cloned());
        // The database.
        let list_end = 78 + records.len() * 8 + 2;
        let mut file = vec![0u8; 78];
        file[..4].copy_from_slice(b"Test");
        file[60..68].copy_from_slice(b"BOOKMOBI");
        file[76..78].copy_from_slice(&(records.len() as u16).to_be_bytes());
        let mut at = list_end;
        for record in &records {
            file.extend_from_slice(&(at as u32).to_be_bytes());
            file.extend_from_slice(&[0, 0, 0, 0]);
            at += record.len();
        }
        file.extend_from_slice(&[0, 0]);
        for record in &records {
            file.extend_from_slice(record);
        }
        file
    }
}

/// PalmDOC compression: literals, copies from the last 2 KB, and spaces
/// joined to the next letter.
pub fn compress(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < input.len() {
        // A copy of 3 to 10 bytes.
        let window = i.saturating_sub(2047);
        let mut best = (0usize, 0usize);
        for start in window..i {
            let mut n = 0;
            while n < 10 && i + n < input.len() && input[start + n] == input[i + n] {
                n += 1;
            }
            if n > best.0 {
                best = (n, i - start);
            }
        }
        if best.0 >= 3 {
            let pair = 0x8000u16 | ((best.1 as u16) << 3) | (best.0 as u16 - 3);
            out.extend_from_slice(&pair.to_be_bytes());
            i += best.0;
            continue;
        }
        let b = input[i];
        if b == b' ' && i + 1 < input.len() && (0x40..0x80).contains(&input[i + 1]) {
            out.push(input[i + 1] ^ 0x80);
            i += 2;
            continue;
        }
        if b == 0 || (0x09..0x80).contains(&b) {
            out.push(b);
            i += 1;
        } else {
            // A run of bytes that are not literals alone.
            let mut n = 1;
            while n < 8 && i + n < input.len() && !(0x09..0x80).contains(&input[i + n]) {
                n += 1;
            }
            out.push(n as u8);
            out.extend_from_slice(&input[i..i + n]);
            i += n;
        }
    }
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// A gray PNG of a gradient, with stored (not compressed) data.
pub fn png(width: u32, height: u32) -> Vec<u8> {
    let mut raw = Vec::new();
    for y in 0..height {
        raw.push(0);
        for x in 0..width {
            raw.push(((x * 255 / width.max(1)) ^ (y * 7)) as u8);
        }
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(60_000).peekable();
    while let Some(chunk) = chunks.next() {
        z.push(u8::from(chunks.peek().is_none()));
        z.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
        z.extend_from_slice(chunk);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let chunk = |kind: &[u8; 4], data: &[u8]| {
        let mut c = (data.len() as u32).to_be_bytes().to_vec();
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        c.extend_from_slice(&body);
        c.extend_from_slice(&crc32(&body).to_be_bytes());
        c
    };
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 0, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(chunk(b"IHDR", &header));
    png.extend(chunk(b"IDAT", &z));
    png.extend(chunk(b"IEND", &[]));
    png
}
