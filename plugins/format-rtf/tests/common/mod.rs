//! Makes RTF documents for the tests: the framing, a hexadecimal picture, a PNG.

#![allow(dead_code, reason = "each test uses some of it")]

pub mod io;

/// `body` as an RTF document in the default code page.
pub fn rtf(body: &str) -> Vec<u8> {
    format!("{{\\rtf1\\ansi\\ansicpg1252\\deff0{{\\fonttbl{{\\f0 Serif;}}}}\n{body}}}").into_bytes()
}

/// Bytes as hexadecimal text, 64 digits a line.
pub fn hex(bytes: &[u8]) -> String {
    let digits: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let lines: Vec<&str> = digits
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    lines.join("\r\n")
}

/// A picture group of a PNG.
pub fn picture(width: u32, height: u32) -> String {
    format!(
        "{{\\pict\\pngblip\\picw{width}\\pich{height}\\picwgoal300\\pichgoal300 {}}}",
        hex(&png(width, height))
    )
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
