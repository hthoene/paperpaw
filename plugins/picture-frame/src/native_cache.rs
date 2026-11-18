//! The cache of the server's photos, without the host: what is kept
//! (`cache.txt`, a line a photo), and the plan of a sync: which photos of the
//! server fit the size cap, which of them are already there, which to fetch
//! and which cached ones to drop. When the server has more than the cap
//! holds, each sync takes the window of photos after the last one before, so
//! over the days every photo is shown.
use alloc::string::String;
use alloc::vec::Vec;

use core::ops::ControlFlow;
use paperpaw_sdk::native::Error;

use paperpaw_ui_kit::{
    files::{Volume, for_each_line_on, replace_on},
    mem::push,
    text::{concat, copy, decimal_u64},
};

use crate::native_photos::extension;

/// What a photo of unknown size (the server did not say) is counted as.
pub const UNKNOWN: u64 = 1 << 20;
/// The most photos kept, and the longest address a line holds.
pub const MOST: usize = 256;
pub const HREF_MAX: usize = 900;
/// The file the cache's list is kept in (in the plugin's data folder).
pub const MANIFEST: &str = "cache.txt";

/// A photo on the server: its address and its size (0: not told).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub href: String,
    pub size: u64,
}

/// A photo in the cache: its file name, its size and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    pub href: String,
}

/// The file name a photo of the server is kept as: `p`, eight hex digits of
/// its address and its extension; `None` for a name that is no photo.
#[must_use]
pub fn file_name(href: &str) -> Option<String> {
    let ext = extension(href.split(['?', '#']).next().unwrap_or(href))?;
    // FNV-1a over the address.
    let mut hash: u32 = 0x811c_9dc5;
    for byte in href.bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    let mut name = String::new();
    name.try_reserve_exact(14).ok()?;
    name.push('p');
    for shift in (0..8).rev() {
        let digit = char::from_digit((hash >> (shift * 4)) & 15, 16)?;
        name.push(digit);
    }
    name.push('.');
    name.push_str(ext);
    Some(name)
}

/// One line of the list: `name`, `size` and `href`, tab separated; `None` for
/// a line that is not one.
#[must_use]
pub fn parse_line(line: &str) -> Option<Entry> {
    let mut parts = line.splitn(3, '\t');
    let (name, size, href) = (parts.next()?, parts.next()?, parts.next()?);
    extension(name)?;
    Some(Entry {
        name: copy(name).ok()?,
        size: size.parse().ok()?,
        href: copy(href).ok()?,
    })
}

/// The list kept on `volume`, as far as it reads (an empty one where there is
/// none or it cannot be read).
#[must_use]
pub fn read(volume: &dyn Volume) -> Vec<Entry> {
    let mut entries = Vec::new();
    let read = for_each_line_on(volume, MANIFEST, &mut |line| {
        if let Some(entry) = parse_line(line) {
            push(&mut entries, entry)?;
        }
        Ok(if entries.len() >= MOST {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        })
    });
    if read.is_err() {
        entries.clear();
    }
    entries
}

/// Keeps the list.
///
/// # Errors
/// What writing says; [`Error::NoMemory`].
pub fn write(volume: &dyn Volume, entries: &[Entry]) -> Result<(), Error> {
    replace_on(volume, MANIFEST, render(entries)?.as_bytes())
}

/// The list as it is kept.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn render(entries: &[Entry]) -> Result<String, Error> {
    let mut out = String::new();
    for entry in entries {
        let mut digits = [0u8; 20];
        let size = decimal_u64(entry.size, &mut digits);
        let line = concat(&[&entry.name, "\t", size, "\t", &entry.href, "\n"])?;
        out.try_reserve(line.len()).map_err(|_| Error::NoMemory)?;
        out.push_str(&line);
    }
    Ok(out)
}

/// The bytes the entries take together.
#[must_use]
pub fn total(entries: &[Entry]) -> u64 {
    entries.iter().map(|entry| entry.size).sum()
}

/// What a sync does.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Cached and wanted: stay.
    pub keep: Vec<Entry>,
    /// To fetch: the index in the server's list and the file name.
    pub fetch: Vec<(usize, String)>,
    /// Cached files to remove.
    pub drop: Vec<String>,
    /// Where the next window starts (0: all photos fit).
    pub window: usize,
    /// How many photos of the server there are, and how many fit.
    pub photos: usize,
    pub fitting: usize,
}

/// The plan for `remote` (sorted by the caller) with `cached` in the cache and
/// `cap` bytes of room, the window starting at photo `window`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn plan(remote: &[Remote], cached: &[Entry], cap: u64, window: usize) -> Result<Plan, Error> {
    let count = remote.len();
    let mut chosen: Vec<usize> = Vec::new();
    let (mut used, mut last, mut all) = (0u64, None, true);
    for step in 0..count {
        let index = (window % count.max(1) + step) % count.max(1);
        let Some(photo) = remote.get(index) else {
            break;
        };
        let size = if photo.size == 0 { UNKNOWN } else { photo.size };
        if file_name(&photo.href).is_none() || photo.href.len() > HREF_MAX {
            continue;
        }
        if used.saturating_add(size) > cap {
            all = false;
            continue;
        }
        used += size;
        last = Some(index);
        push(&mut chosen, index)?;
    }
    let mut plan = Plan {
        photos: count,
        fitting: chosen.len(),
        window: if all {
            0
        } else {
            last.map_or(0, |index| (index + 1) % count.max(1))
        },
        ..Plan::default()
    };
    for &index in &chosen {
        let Some(photo) = remote.get(index) else {
            continue;
        };
        let Some(name) = file_name(&photo.href) else {
            continue;
        };
        let there = cached.iter().find(|entry| {
            entry.name == name
                && entry.href == photo.href
                && (photo.size == 0 || entry.size == photo.size)
        });
        match there {
            Some(entry) => push(&mut plan.keep, entry.clone())?,
            None => push(&mut plan.fetch, (index, name))?,
        }
    }
    for entry in cached {
        let wanted = plan.keep.iter().any(|kept| kept.name == entry.name)
            || plan.fetch.iter().any(|(_, name)| *name == entry.name);
        if !wanted {
            push(&mut plan.drop, copy(&entry.name)?)?;
        }
    }
    Ok(plan)
}

#[cfg(test)]
#[path = "native_cache_tests.rs"]
mod tests;
