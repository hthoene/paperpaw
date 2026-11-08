//! Fonts read from the storage: packs read where they are, other font
//! files read into memory.

use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::Storage;
use paperpaw_text::{Font, FontError, PackSource};

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

/// Reads a font file into memory: `Ok(None)` where it cannot be read or is
/// no font, [`FontError::OutOfMemory`] where the heap has no room for it.
pub(super) fn read_font(
    storage: &dyn Storage,
    path: &str,
    weight: Option<f32>,
) -> Result<Option<Font>, FontError> {
    let Ok(size) = storage.size(path) else {
        return Ok(None);
    };
    if size > super::MAX_FONT_FILE {
        return Ok(None);
    }
    let mut data: Vec<u8> = Vec::new();
    data.try_reserve_exact(size as usize)
        .map_err(|_| FontError::OutOfMemory)?;
    data.resize(size as usize, 0);
    let mut at = 0;
    while at < data.len() {
        match storage.read_at(path, at as u64, &mut data[at..]) {
            Ok(n) if n > 0 => at += n,
            _ => return Ok(None),
        }
    }
    Ok(match weight {
        Some(weight) => Font::from_data(data, &[("wght", weight)]).ok(),
        None => Font::from_data(data, &[]).ok(),
    })
}
