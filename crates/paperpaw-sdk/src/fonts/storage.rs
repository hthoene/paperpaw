//! Fonts read from the storage: packs read where they are, other font
//! files read into memory.

use alloc::rc::Rc;
use alloc::string::String;

use paperpaw_platform::Storage;
use paperpaw_text::{Font, PackSource};

/// A pack on the storage, read where it is.
pub(super) struct Stored {
    pub(super) storage: Rc<dyn Storage>,
    pub(super) path: String,
}

impl PackSource for Stored {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
        let mut done = 0;
        while done < buf.len() {
            match self
                .storage
                .read_at(&self.path, at + done as u64, &mut buf[done..])
            {
                Ok(0) | Err(_) => return false,
                Ok(n) => done += n,
            }
        }
        true
    }
}

/// Reads a font file into memory.
pub(super) fn read_font(storage: &dyn Storage, path: &str, weight: Option<f32>) -> Option<Font> {
    let size = storage.size(path).ok()?;
    if size > super::MAX_FONT_FILE {
        return None;
    }
    let mut data = alloc::vec![0u8; size as usize];
    let mut at = 0;
    while at < data.len() {
        let n = storage.read_at(path, at as u64, &mut data[at..]).ok()?;
        if n == 0 {
            return None;
        }
        at += n;
    }
    match weight {
        Some(weight) => Font::from_data(data, &[("wght", weight)]).ok(),
        None => Font::from_data(data, &[]).ok(),
    }
}
