//! A WebDAV folder as a list of photos: the listing a `PROPFIND` of depth 1
//! answers, read by the UI kit's shared reader (`paperpaw_ui_kit::dav`) as it
//! streams from the file it was saved to. Files with a photo's name become
//! [`Remote`]s with the address to fetch them from and their size; folders are
//! left out. A refused allocation ends the reading with what was read.
use alloc::vec::Vec;

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{dav::listing::read_each, files::Volume, mem::push};

use crate::native_cache::{HREF_MAX, MOST, Remote};
use crate::native_photos::{by_name, extension};

pub use paperpaw_ui_kit::dav::listing::{LISTING, PROPFIND};
pub use paperpaw_ui_kit::dav::url::{origin, path, resolve};

/// The photos of the listing saved as `name` on `volume`, for the folder at
/// `base`, sorted by address. At most [`MOST`]; the flag says there were more
/// or memory ran short.
///
/// # Errors
/// What reading the file says; [`Error::NoMemory`].
pub fn read(volume: &dyn Volume, name: &str, base: &str) -> Result<(Vec<Remote>, bool), Error> {
    let mut photos = Vec::new();
    let mut more = false;
    let short = read_each(volume, name, base, &mut |item| {
        if item.folder || extension(&item.name).is_none() || item.name.starts_with('.') {
            return Ok(());
        }
        if photos.len() >= MOST {
            more = true;
            return Ok(());
        }
        if item.url.len() > HREF_MAX {
            return Ok(());
        }
        push(
            &mut photos,
            Remote {
                href: item.url,
                size: item.size,
            },
        )
    })?;
    // The shared sorter: the std sort keeps a scratch the X4 refuses on the stack.
    paperpaw_sdk::sort::sort_by(&mut photos, |a, b| by_name(&a.href, &b.href));
    Ok((photos, more || short))
}

#[cfg(test)]
#[path = "native_dav_tests.rs"]
mod tests;
