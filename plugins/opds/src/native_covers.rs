//! The covers of a feed's entries, where the device has the memory for them
//! (not a small one) and the setting is on: loaded one after the other for the
//! entries on the page shown, kept in a small cache of files so they show at
//! once the next time, and decoded by the host (`PrepareImage`, an image id of
//! the plugin: at most 8, so at most [`MAX_LIVE`] are held). This is the
//! state of the covers of one feed and the cache's index, without host calls;
//! the screen does what [`Covers::pick`] says.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    digest::md5::Md5,
    native::{Error, image::ImageId, net::RequestId},
};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

use crate::native_files::{self as files, Files};

/// Images held at a time: the host allows a plugin 8, and a few ids are left
/// for the host's own use.
pub const MAX_LIVE: usize = 6;
/// Files the cache keeps: the oldest goes past this.
pub const MAX_CACHED: usize = 48;
/// The cache's index: the names of its files, oldest first.
const INDEX: &str = "covers.txt";
const INDEX_CAP: usize = 8 * 1024;
/// The longest side a cover is decoded to, in pixels.
pub const SIDE: u32 = 160;

/// What the cover of one entry is up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Slot {
    /// Not asked for.
    #[default]
    Idle,
    /// Being downloaded.
    Fetching,
    /// Being decoded by the host.
    Preparing,
    /// Decoded: the image and its size.
    Ready(ImageId, u32, u32),
    /// Could not be had: not tried again for this feed.
    Failed,
}

/// The covers of one feed's entries.
#[derive(Debug, Default)]
pub struct Covers {
    /// Told apart from the covers of the feed before: an answer for an older
    /// feed is dropped.
    generation: u16,
    slots: Vec<Slot>,
    /// The download going on, and which entry's it is.
    fetching: Option<(RequestId, usize)>,
}

impl Covers {
    /// The covers of a feed of `entries` entries (the `generation`'th shown).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(generation: u16, entries: usize) -> Result<Self, Error> {
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(entries)
            .map_err(|_| Error::NoMemory)?;
        slots.resize(entries, Slot::Idle);
        Ok(Self {
            generation,
            slots,
            fetching: None,
        })
    }

    /// The feed these covers are of (see [`Covers::new`]).
    #[must_use]
    pub const fn generation(&self) -> u16 {
        self.generation
    }

    /// Images held (being decoded or decoded).
    fn live(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| matches!(s, Slot::Preparing | Slot::Ready(..)))
            .count()
    }

    /// The entry whose cover is next to load, among those `visible`: the first
    /// that `wants` one and has none yet, unless a download is going on or the
    /// images held are at [`MAX_LIVE`].
    pub fn pick(
        &self,
        visible: impl IntoIterator<Item = usize>,
        wants: impl Fn(usize) -> bool,
    ) -> Option<usize> {
        if self.fetching.is_some() || self.live() >= MAX_LIVE {
            return None;
        }
        visible
            .into_iter()
            .find(|i| self.slots.get(*i) == Some(&Slot::Idle) && wants(*i))
    }

    /// Entry `i`'s cover is being downloaded by request `id`.
    pub fn fetching(&mut self, i: usize, id: RequestId) {
        if let Some(slot) = self.slots.get_mut(i) {
            *slot = Slot::Fetching;
            self.fetching = Some((id, i));
        }
    }

    /// Entry `i`'s cover is being decoded.
    pub fn preparing(&mut self, i: usize) {
        if let Some(slot) = self.slots.get_mut(i) {
            *slot = Slot::Preparing;
        }
    }

    /// Entry `i`'s cover cannot be had.
    pub fn failed(&mut self, i: usize) {
        if let Some(slot) = self.slots.get_mut(i) {
            *slot = Slot::Failed;
        }
    }

    /// The entry the download `id` was for, which is no longer waited for.
    pub fn fetched(&mut self, id: RequestId) -> Option<usize> {
        match self.fetching {
            Some((waited, i)) if waited == id => {
                self.fetching = None;
                Some(i)
            }
            _ => None,
        }
    }

    /// The number the host gives back with a decoded picture for entry `i`.
    #[must_use]
    pub fn cookie(&self, i: usize) -> u32 {
        u32::from(self.generation) << 16 | (u32::try_from(i).unwrap_or(0) & 0xffff)
    }

    /// A picture is decoded (or not: `image` is `None`) for `cookie`. The image
    /// of a feed shown before, or of an entry that is gone, is handed back to be
    /// released.
    pub fn ready(&mut self, cookie: u32, image: Option<(ImageId, u32, u32)>) -> Option<ImageId> {
        let (generation, i) = (cookie >> 16, usize::try_from(cookie & 0xffff).ok()?);
        let current = u32::from(self.generation) == generation;
        match (self.slots.get_mut(i), current, image) {
            (Some(slot), true, Some((id, w, h))) if *slot == Slot::Preparing => {
                *slot = Slot::Ready(id, w, h);
                None
            }
            (Some(slot), true, None) if *slot == Slot::Preparing => {
                *slot = Slot::Failed;
                None
            }
            (_, _, image) => image.map(|(id, ..)| id),
        }
    }

    /// The picture of entry `i`, with its size, where it is decoded.
    #[must_use]
    pub fn image(&self, i: usize) -> Option<(ImageId, u32, u32)> {
        match self.slots.get(i) {
            Some(Slot::Ready(id, w, h)) => Some((*id, *w, *h)),
            _ => None,
        }
    }

    /// Lets go of every picture (the feed is left): their ids, to be released.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn release_all(&mut self) -> Result<Vec<ImageId>, Error> {
        let mut ids = Vec::new();
        for slot in &mut self.slots {
            if let Slot::Ready(id, ..) = *slot {
                push(&mut ids, id)?;
            }
            *slot = Slot::Failed;
        }
        self.fetching = None;
        Ok(ids)
    }
}

/// The name of the cache file of the cover at `url`: by the digest of its
/// address.
///
/// # Errors
/// `NoMemory`.
pub fn cache_name(url: &str) -> Result<String, Error> {
    let mut hash = Md5::new();
    hash.update(url.as_bytes());
    let mut hex = [0u8; 32];
    let digest = hash.hex_into(&mut hex);
    concat(&["covers/", digest, ".img"])
}

/// Whether the file `name` is in the cache.
#[must_use]
pub fn cached(files: &dyn Files, name: &str) -> bool {
    files.size(name).is_ok_and(|size| size > 0)
}

/// Notes `name` as the newest file of the cache; the oldest go once there are
/// more than [`MAX_CACHED`]. A cache that cannot be noted is let go of (the
/// file stays until the next time).
///
/// # Errors
/// `NoMemory`.
pub fn remember(files: &dyn Files, name: &str) -> Result<(), Error> {
    let known = match files::read_text(files, INDEX, INDEX_CAP) {
        Ok(text) => text.unwrap_or_default(),
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        // Not readable: begun anew.
        Err(_) => String::new(),
    };
    let mut names: Vec<&str> = Vec::new();
    for line in known.lines().filter(|l| !l.is_empty() && *l != name) {
        push(&mut names, line)?;
    }
    push(&mut names, name)?;
    let extra = names.len().saturating_sub(MAX_CACHED);
    let mut text = String::new();
    for (at, kept) in names.iter().enumerate() {
        if at < extra {
            files::remove(files, kept);
            continue;
        }
        text.try_reserve(kept.len() + 1)
            .map_err(|_| Error::NoMemory)?;
        text.push_str(kept);
        text.push('\n');
    }
    match files.write(INDEX, text.as_bytes()) {
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Ok(()) | Err(_) => Ok(()),
    }
}

/// The cover cache's file names as a text copy, for a test.
///
/// # Errors
/// `NoMemory`.
pub fn index(files: &dyn Files) -> Result<String, Error> {
    match files::read_text(files, INDEX, INDEX_CAP)? {
        Some(text) => Ok(text),
        None => copy(""),
    }
}
