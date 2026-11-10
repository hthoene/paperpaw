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
        self.room(gray.len() * 2)?;
        for &g in gray {
            self.buffer.extend_from_slice(&[g, 255]);
        }
        Ok(())
    }

    fn row_alpha(&mut self, gray_alpha: &[u8]) -> Result<(), ImageError> {
        self.room(gray_alpha.len())?;
        self.buffer.extend_from_slice(gray_alpha);
        Ok(())
    }
}
