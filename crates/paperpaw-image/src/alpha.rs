//! Decoded images with transparency on storage: `PPGA`, width and height
//! (u32), rows of two bytes a pixel, gray and how opaque it is (0
//! transparent, 255 opaque).

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::Storage;

use crate::gray::read_all;
use crate::{ImageError, ImageSink};

const MAGIC: &[u8; 4] = b"PPGA";
const HEADER: u64 = 12;
/// Rows are appended in chunks of about this size.
const FLUSH: usize = 8192;

/// A decoded image with transparency in storage.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AlphaFile {
    pub path: String,
    pub width: u32,
    pub height: u32,
}

impl AlphaFile {
    /// Made of its parts.
    pub fn new(path: &str, width: u32, height: u32) -> Self {
        Self {
            path: String::from(path),
            width,
            height,
        }
    }

    /// Opens a decoded image with transparency, checking its size.
    pub fn open(storage: &dyn Storage, path: &str) -> Option<AlphaFile> {
        let mut header = [0u8; HEADER as usize];
        if !read_all(storage, path, 0, &mut header) || &header[..4] != MAGIC {
            return None;
        }
        let width = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let height = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
        let expected = HEADER + 2 * u64::from(width) * u64::from(height);
        (width > 0 && height > 0 && storage.size(path).ok()? == expected).then(|| AlphaFile {
            path: String::from(path),
            width,
            height,
        })
    }

    /// Reads rows from `y` on into `out` (two bytes a pixel), as many
    /// whole ones as fit and the image has, in one read. The rows read, 0
    /// if none could be.
    pub fn read_rows(&self, storage: &dyn Storage, y: u32, out: &mut [u8]) -> u32 {
        let row = self.width as usize * 2;
        let rows = (out.len() / row.max(1)).min(self.height.saturating_sub(y) as usize);
        let at = HEADER + u64::from(y) * row as u64;
        if rows == 0 || !read_all(storage, &self.path, at, &mut out[..rows * row]) {
            return 0;
        }
        rows as u32
    }
}

/// Writes a decoded image with transparency to storage as it is decoded;
/// rows of an image without it are kept opaque.
pub struct AlphaWriter<'a> {
    storage: &'a dyn Storage,
    path: &'a str,
    buffer: Vec<u8>,
}

impl core::fmt::Debug for AlphaWriter<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AlphaWriter")
            .field("path", &self.path)
            .finish()
    }
}

impl<'a> AlphaWriter<'a> {
    pub fn new(storage: &'a dyn Storage, path: &'a str) -> Self {
        Self {
            storage,
            path,
            buffer: Vec::new(),
        }
    }

    /// Writes what is buffered; call when decoding is complete.
    pub fn finish(mut self) -> Result<(), ImageError> {
        self.flush()
    }

    fn flush(&mut self) -> Result<(), ImageError> {
        if !self.buffer.is_empty() {
            self.storage
                .append(self.path, &self.buffer)
                .map_err(|_| ImageError::Io)?;
            self.buffer.clear();
        }
        Ok(())
    }

    fn room(&mut self, len: usize) -> Result<(), ImageError> {
        if self.buffer.len() + len > self.buffer.capacity() {
            self.flush()?;
        }
        Ok(())
    }
}

impl ImageSink for AlphaWriter<'_> {
    fn start(&mut self, width: u32, height: u32) -> Result<(), ImageError> {
        // On the stack: an allocation here aborted on a full heap.
        let mut header = [0_u8; HEADER as usize];
        header[..4].copy_from_slice(MAGIC);
        header[4..8].copy_from_slice(&width.to_le_bytes());
        header[8..].copy_from_slice(&height.to_le_bytes());
        self.storage
            .write(self.path, &header)
            .map_err(|_| ImageError::Io)?;
        self.buffer
            .try_reserve_exact(FLUSH.max(width as usize * 2))
            .map_err(|_| ImageError::OutOfMemory)
    }

    fn wants_alpha(&self) -> bool {
        true
    }

    fn row(&mut self, gray: &[u8]) -> Result<(), ImageError> {
        // A piece at a time that fits the room taken at the start: a row
        // longer than that is not grown into.
        for piece in gray.chunks((self.buffer.capacity() / 2).max(1)) {
            self.room(piece.len() * 2)?;
            for &g in piece {
                self.buffer.extend_from_slice(&[g, 255]);
            }
        }
        Ok(())
    }

    fn row_alpha(&mut self, gray_alpha: &[u8]) -> Result<(), ImageError> {
        self.room(gray_alpha.len())?;
        if gray_alpha.len() > self.buffer.capacity() {
            // Longer than the room taken at `start`: written as it is
            // rather than grown into.
            return self
                .storage
                .append(self.path, gray_alpha)
                .map_err(|_| ImageError::Io);
        }
        self.buffer.extend_from_slice(gray_alpha);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::tests::OneFile;

    /// Rows with and without transparency longer than the room taken at
    /// the start are written without growing it, and read back whole.
    #[test]
    fn longer_rows_are_written_without_growing() {
        let storage = OneFile::default();
        let mut alpha = AlphaWriter::new(&storage, "a.ppga");
        alpha.start(1, 4).unwrap();
        let room = alpha.buffer.capacity();
        let long = room / 2 + 1;
        let mut wide = alloc::vec![0u8; 2 * long];
        wide.iter_mut().enumerate().for_each(|(i, b)| *b = i as u8);
        let gray: Vec<u8> = (0..long).map(|i| i as u8).collect();
        alpha.row_alpha(&[1, 2]).unwrap();
        alpha.row_alpha(&wide).unwrap();
        alpha.row(&gray).unwrap();
        alpha.row(&[4]).unwrap();
        assert_eq!(alpha.buffer.capacity(), room);
        alpha.finish().unwrap();
        let data = storage.0.borrow();
        let opaque: Vec<u8> = gray.iter().flat_map(|&g| [g, 255]).collect();
        let rows = [&[1, 2][..], &wide, &opaque, &[4, 255]].concat();
        assert_eq!(&data[12..], &rows[..]);
    }
}
