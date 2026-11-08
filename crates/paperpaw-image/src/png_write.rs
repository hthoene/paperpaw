//! Writing gray PNG files row by row, e.g. screenshots: each row goes to
//! storage as it comes, uncompressed (stored deflate blocks), so writing
//! needs memory for one row only.

use alloc::vec::Vec;

use paperpaw_platform::Storage;

use crate::ImageError;

/// The CRC-32 of PNG chunks, over `data` continuing `crc`.
fn crc32(mut crc: u32, data: &[u8]) -> u32 {
    crc = !crc;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Writes a gray PNG to storage, a row at a time; the storage and path
/// are given with each call, so nothing is held between them.
#[derive(Debug)]
pub struct PngWriter {
    width: u32,
    rows_left: u32,
    /// The running Adler-32 of the image data.
    adler: (u32, u32),
}

impl PngWriter {
    /// Starts `path` for a `width`×`height` image.
    pub fn new(
        storage: &dyn Storage,
        path: &str,
        width: u32,
        height: u32,
    ) -> Result<Self, ImageError> {
        // A row and its filter byte make one stored deflate block, of at
        // most 0xffff bytes.
        if width == 0 || height == 0 || width >= 0xffff {
            return Err(ImageError::Unsupported("PNG size"));
        }
        // The signature, `IHDR` and the zlib header's `IDAT`, in one
        // fallible reservation.
        let mut head = Vec::new();
        head.try_reserve_exact(8 + 12 + 13 + 12 + 2)
            .map_err(|_| ImageError::OutOfMemory)?;
        head.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        let mut ihdr = [0_u8; 13];
        ihdr[..4].copy_from_slice(&width.to_be_bytes());
        ihdr[4..8].copy_from_slice(&height.to_be_bytes());
        // 8-bit gray, deflate, no filter method or interlace.
        ihdr[8..].copy_from_slice(&[8, 0, 0, 0, 0]);
        chunk(&mut head, b"IHDR", &ihdr);
        // The zlib header: deflate, no dictionary, fastest.
        chunk(&mut head, b"IDAT", &[0x78, 0x01]);
        storage.write(path, &head).map_err(ImageError::Storage)?;
        Ok(Self {
            width,
            rows_left: height,
            adler: (1, 0),
        })
    }

    /// The next row of `path`: `width` gray values (0 black, 255 white).
    pub fn row(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        gray: &[u8],
    ) -> Result<(), ImageError> {
        if self.rows_left == 0 || gray.len() < self.width as usize {
            return Err(ImageError::Invalid("PNG row"));
        }
        self.rows_left -= 1;
        let last = self.rows_left == 0;
        let len = (self.width + 1) as u16;
        // One fallible reservation per row for the whole of what is
        // appended: the block, its chunk frame and, last, the adler sum
        // and `IEND`.
        let mut out = Vec::new();
        out.try_reserve_exact(5 + usize::from(len) + 4 + 12 + 12)
            .map_err(|_| ImageError::OutOfMemory)?;
        out.extend_from_slice(&(u32::from(len) + 5 + if last { 4 } else { 0 }).to_be_bytes());
        out.extend_from_slice(b"IDAT");
        let mut block = out;
        block.push(u8::from(last));
        block.extend_from_slice(&len.to_le_bytes());
        block.extend_from_slice(&(!len).to_le_bytes());
        // Filter type: none.
        block.push(0);
        block.extend_from_slice(&gray[..self.width as usize]);
        for &byte in &block[8 + 5..] {
            self.adler.0 = (self.adler.0 + u32::from(byte)) % 65_521;
            self.adler.1 = (self.adler.1 + self.adler.0) % 65_521;
        }
        if last {
            let adler = (self.adler.1 << 16) | self.adler.0;
            block.extend_from_slice(&adler.to_be_bytes());
        }
        let crc = crc32(0, &block[4..]);
        block.extend_from_slice(&crc.to_be_bytes());
        if last {
            chunk(&mut block, b"IEND", &[]);
        }
        storage.append(path, &block).map_err(ImageError::Storage)
    }

    /// Whether every row was written.
    pub fn is_complete(&self) -> bool {
        self.rows_left == 0
    }
}

/// Appends a chunk of `kind` with `data` to `out`.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(crc32(0, kind), data);
    out.extend_from_slice(&crc.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    use alloc::vec::Vec;
    use core::cell::RefCell;
    use paperpaw_platform::{DirEntry, StorageError};

    /// One file in memory.
    #[derive(Default)]
    struct OneFile(RefCell<Vec<u8>>);

    impl Storage for OneFile {
        fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
            Ok(0)
        }
        fn size(&self, _: &str) -> Result<u64, StorageError> {
            Ok(self.0.borrow().len() as u64)
        }
        fn write(&self, _: &str, data: &[u8]) -> Result<(), StorageError> {
            *self.0.borrow_mut() = data.to_vec();
            Ok(())
        }
        fn append(&self, _: &str, data: &[u8]) -> Result<(), StorageError> {
            self.0.borrow_mut().extend_from_slice(data);
            Ok(())
        }
        fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            Ok(())
        }
        fn remove(&self, _: &str) -> Result<(), StorageError> {
            Ok(())
        }
    }

    /// Every chunk written has its length and CRC right (the decoder
    /// checks neither), and the file ends with `IEND`.
    #[test]
    fn every_chunk_has_its_length_and_crc() {
        let storage = OneFile::default();
        let mut png = PngWriter::new(&storage, "s.png", 5, 3).unwrap();
        for row in 0..3_u8 {
            png.row(&storage, "s.png", &[row * 40; 5]).unwrap();
        }
        assert!(png.is_complete());
        let data = storage.0.borrow();
        let mut at = 8;
        let mut kinds = Vec::new();
        while at < data.len() {
            let len = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
            let body = &data[at + 4..at + 8 + len];
            let crc = u32::from_be_bytes(data[at + 8 + len..at + 12 + len].try_into().unwrap());
            assert_eq!(crc32(0, body), crc, "chunk at {at}");
            kinds.push(<[u8; 4]>::try_from(&body[..4]).unwrap());
            at += 12 + len;
        }
        assert_eq!(at, data.len());
        assert_eq!(kinds.first(), Some(b"IHDR"));
        assert_eq!(kinds.last(), Some(b"IEND"));
        assert_eq!(kinds.iter().filter(|k| *k == b"IDAT").count(), 4);
    }

    #[test]
    fn crc_of_iend() {
        // The well-known CRC of an empty IEND chunk.
        assert_eq!(crc32(0, b"IEND"), 0xae42_6082);
    }
}
