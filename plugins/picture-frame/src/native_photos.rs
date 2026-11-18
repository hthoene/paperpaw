//! Which photo is shown, without the host: the pictures of the card's folder
//! (their names, sorted, within a cap) and those cached from the server, the
//! choice of the next one (at each sleep, or by the day), how a photo is
//! fitted to the screen and the tone curve for e-ink.
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

use paperpaw_sdk::{Rect, native::Error};
use paperpaw_ui_kit::text::{concat, copy};

/// Most names kept of the card's folder, and the room their text may take.
pub const MOST: usize = 512;
pub const NAMES: usize = 24 * 1024;
/// The longest path a request to prepare a photo takes.
pub const PATH_MAX: usize = 512;
/// Where the server's photos are kept, in the plugin's data folder.
pub const CACHE: &str = "cache";

/// Whether a file of a folder is a photo: not a folder, not hidden, a JPEG
/// or a PNG by its name.
#[must_use]
pub fn is_photo(name: &str, is_dir: bool) -> bool {
    !is_dir && !name.starts_with('.') && extension(name).is_some()
}

/// `jpg`, `jpeg` or `png` where the name ends in one of them, in any case.
#[must_use]
pub fn extension(name: &str) -> Option<&'static str> {
    let (_, ext) = name.rsplit_once('.')?;
    [("jpg", "jpg"), ("jpeg", "jpg"), ("png", "png")]
        .iter()
        .find(|(known, _)| ext.eq_ignore_ascii_case(known))
        .map(|(_, ext)| *ext)
}

/// The order of names: without regard to case, then by their bytes.
pub fn by_name(a: &str, b: &str) -> Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
        .then_with(|| a.cmp(b))
}

/// The photos of a folder as its entries come: the alphabetically first
/// [`MOST`] within [`NAMES`] bytes, whatever the order the card lists them in.
#[derive(Debug, Default)]
pub struct Found {
    names: Vec<String>,
    bytes: usize,
    /// The smallest name dropped: nothing from there on is kept.
    cut: Option<String>,
}

impl Found {
    /// Considers one entry.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; what was kept stays.
    pub fn add(&mut self, name: &str, is_dir: bool) -> Result<(), Error> {
        if !is_photo(name, is_dir)
            || self
                .cut
                .as_deref()
                .is_some_and(|cut| by_name(name, cut) != Ordering::Less)
        {
            return Ok(());
        }
        let at = match self.names.binary_search_by(|kept| by_name(kept, name)) {
            Ok(_) => return Ok(()),
            Err(at) => at,
        };
        self.names.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.names.insert(at, copy(name)?);
        self.bytes += name.len();
        while self.names.len() > MOST || self.bytes > NAMES {
            let Some(last) = self.names.pop() else { break };
            self.bytes -= last.len();
            self.cut = Some(last);
        }
        Ok(())
    }

    /// The names kept, sorted.
    #[must_use]
    pub fn finish(self) -> Vec<String> {
        self.names
    }
}

/// Where a photo is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The card's folder (the library root).
    Card,
    /// The plugin's cache of the server's photos (the data folder).
    Cache,
}

/// The photos there are: the card's, then the cached ones.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Photos {
    /// The card's folder, without slashes at its ends.
    pub folder: String,
    pub card: Vec<String>,
    /// Names in the cache folder.
    pub cached: Vec<String>,
}

impl Photos {
    /// How many photos.
    #[must_use]
    pub fn len(&self) -> usize {
        self.card.len() + self.cached.len()
    }

    /// Whether there is none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Where photo `index` is and its path there; `None` past the last or
    /// for a path too long.
    #[must_use]
    pub fn at(&self, index: usize) -> Option<(Source, String)> {
        let path = match self.card.get(index) {
            Some(name) if self.folder.is_empty() => copy(name).ok()?,
            Some(name) => concat(&[&self.folder, "/", name]).ok()?,
            None => concat(&[CACHE, "/", self.cached.get(index - self.card.len())?]).ok()?,
        };
        let source = if index < self.card.len() {
            Source::Card
        } else {
            Source::Cache
        };
        (path.len() <= PATH_MAX).then_some((source, path))
    }
}

/// When the photo changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// At each sleep: the next one.
    Sleep,
    /// Once a day.
    Daily,
}

impl Change {
    /// The setting's value as a mode; the first mode for anything else.
    #[must_use]
    pub fn of(value: &str) -> Self {
        if value == crate::declarations::CHANGE[1] {
            Self::Daily
        } else {
            Self::Sleep
        }
    }
}

/// The photo to show of `count`: the one after `cursor` at each sleep, the
/// day's by the day number (`day`, days since 1970 by the local time; 0 where
/// the time is not known). `None` where there is no photo.
#[must_use]
pub fn choose(count: usize, change: Change, cursor: usize, day: i64) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match change {
        Change::Sleep => cursor % count,
        Change::Daily => usize::try_from(day.rem_euclid(i64::try_from(count).ok()?)).ok()?,
    })
}

/// How a photo is fitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Whole, by its shape; where it is near the screen's shape it fills it.
    Auto,
    /// Whole, with an edge round it.
    Whole,
    /// Fills the screen, cropped.
    Fill,
}

impl Fit {
    /// The setting's value as a fit.
    #[must_use]
    pub fn of(value: &str) -> Self {
        match value {
            v if v == crate::declarations::FIT[1] => Self::Whole,
            v if v == crate::declarations::FIT[2] => Self::Fill,
            _ => Self::Auto,
        }
    }

    /// Whether a photo of `size` fills `room` (is cropped to it): always for
    /// [`Fit::Fill`], never for [`Fit::Whole`], and for [`Fit::Auto`] where
    /// the two shapes differ by less than a third (a landscape photo on a
    /// portrait screen stays whole, a near one loses only an edge).
    #[must_use]
    pub fn fills(self, size: (u32, u32), room: Rect) -> bool {
        match self {
            Self::Fill => true,
            Self::Whole => false,
            Self::Auto => {
                let (w, h) = (u64::from(size.0), u64::from(size.1));
                let (rw, rh) = (u64::from(room.width), u64::from(room.height));
                if w == 0 || h == 0 || rw == 0 || rh == 0 {
                    return false;
                }
                // The shapes (w/h and rw/rh) within 4:3 of each other.
                let (a, b) = (w * rh, h * rw);
                let (low, high) = (a.min(b), a.max(b));
                high * 3 <= low * 4
            }
        }
    }
}

/// The tone curve for e-ink: the contrast (`percent`, -50 to 50) about the
/// middle gray, each gray mapped through it.
#[must_use]
pub fn tone(percent: i64) -> [u8; 256] {
    let factor = 100 + percent.clamp(-50, 50);
    let mut table = [0u8; 256];
    for (value, slot) in (0i64..).zip(table.iter_mut()) {
        let mapped = 128 + (value - 128) * factor / 100;
        *slot = u8::try_from(mapped.clamp(0, 255)).unwrap_or(0);
    }
    table
}

#[cfg(test)]
#[path = "native_photos_tests.rs"]
mod tests;
