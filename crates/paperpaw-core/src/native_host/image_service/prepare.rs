//! `command.image_prepare`: decoding a plugin's own picture into the host's
//! image cache in the background, one request per main-loop turn
//! (`document.md` 5). The command's admission (the caller's root 0 or 1
//! resolved to a card path, `uses.storage`) and the delivery of
//! `event.image_ready` are the shell's; this is the queue, the decode and the
//! ids.
use super::{ImageTable, Origin, PREPARED_IMAGES};
use super::{MAX_SIDE, wire};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_FULL, ERR_INVALID, ERR_NO_MEMORY};
use paperpaw_platform::Storage;
use paperpaw_sdk::image::{ColorWriter, Decoded, Fit, Format, GrayWriter, ImageError, Source};

/// Waiting requests a plugin may have.
pub(crate) const WAITING: usize = 4;
/// The directory of prepared images.
const DIR: &str = ".paperpaw/cache/images";

/// `event.image_ready` `status`.
pub(crate) const READY: u32 = 0;
pub(crate) const UNSUPPORTED: u32 = 1;
pub(crate) const DAMAGED: u32 = 2;
pub(crate) const TOO_LARGE: u32 = 3;
pub(crate) const CARD: u32 = 4;
pub(crate) const NO_MEMORY: u32 = 5;

/// A request not yet decoded.
pub(crate) struct Waiting {
    pub(crate) owner: u64,
    cookie: u32,
    side: u32,
    colour: bool,
    /// The card path of the source.
    source: String,
}

/// What `event.image_ready` tells, for `owner` only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ready {
    pub(crate) owner: u64,
    pub(crate) cookie: u32,
    pub(crate) status: u32,
    pub(crate) image: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl Ready {
    /// The record the plugin gets, `now_ms` the event's time.
    pub(crate) fn record(&self, now_ms: u64) -> wire::Record<'static> {
        wire::Record::EventImageReady(wire::EventImageReady {
            now_ms,
            cookie: self.cookie,
            status: self.status,
            image: self.image,
            width: self.width,
            height: self.height,
        })
    }
}

/// Whether `name` is one this host gives a prepared image: `<hex>-<hex>.img`.
fn ours(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".img") else {
        return false;
    };
    let Some((owner, number)) = stem.split_once('-') else {
        return false;
    };
    let hex = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_hexdigit());
    hex(owner) && hex(number)
}

/// A source file on the card.
struct FileSource<'a> {
    storage: &'a dyn Storage,
    path: &'a str,
    offset: u64,
}

impl Source for FileSource<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ImageError> {
        let n = self
            .storage
            .read_at(self.path, self.offset, buf)
            .map_err(|_| ImageError::Io)?;
        self.offset += n as u64;
        Ok(n)
    }
}

/// `status` for a decoder's refusal.
fn status_of(error: &ImageError) -> u32 {
    match error {
        ImageError::Unsupported(what) if what.contains("large") => TOO_LARGE,
        ImageError::Unsupported(_) => UNSUPPORTED,
        ImageError::Invalid(_) => DAMAGED,
        ImageError::OutOfMemory => NO_MEMORY,
        // The card, or a cause this build has no name for.
        _ => CARD,
    }
}

impl ImageTable {
    /// `image_prepare`: queues the picture at the card path `source`.
    ///
    /// # Errors
    /// `ERR_FULL` (4 waiting, or 8 prepared and waiting together),
    /// `ERR_INVALID` (an empty or overlong source), `ERR_NO_MEMORY`,
    /// `ERR_BUSY` (the table is borrowed).
    pub(crate) fn prepare(
        &self,
        owner: u64,
        source: &str,
        side: u32,
        cookie: u32,
        colour: bool,
    ) -> Result<(), i32> {
        if source.is_empty() || source.len() > super::PATH_MAX {
            return Err(ERR_INVALID);
        }
        let mut inner = self.lock()?;
        let waiting = inner.waiting.iter().filter(|w| w.owner == owner).count()
            + inner
                .ready
                .iter()
                .filter(|r| r.owner == owner && r.image == 0)
                .count();
        let prepared = inner
            .entries
            .iter()
            .filter(|e| e.owner == owner && e.origin == Origin::Prepared)
            .count();
        if waiting >= WAITING || waiting + prepared >= PREPARED_IMAGES {
            return Err(ERR_FULL);
        }
        let mut owned = String::new();
        owned
            .try_reserve_exact(source.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        owned.push_str(source);
        inner.waiting.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        // Room for every answer that can be pending: waiting ones and these.
        let pending = inner.waiting.len() + 1;
        inner
            .ready
            .try_reserve(pending)
            .map_err(|_| ERR_NO_MEMORY)?;
        inner.waiting.push(Waiting {
            owner,
            cookie,
            side,
            colour,
            source: owned,
        });
        Ok(())
    }

    /// Any request waiting, an answer to tell or a cache file to delete.
    #[must_use]
    pub(crate) fn prepare_pending(&self) -> bool {
        self.inner.try_borrow().map_or(true, |i| {
            !i.waiting.is_empty() || !i.garbage.is_empty() || !i.ready.is_empty()
        })
    }

    /// One main-loop turn: deletes one cache file of an ended image and
    /// decodes the oldest request. `screen` is the longest side `side` 0
    /// stands for, `colour_screen` whether colour is kept at all. The answer
    /// is held for [`ImageTable::take_ready`]; true if a request was decoded.
    ///
    /// The request's own failure is its answer (a status), not an error.
    ///
    /// # Errors
    /// `ERR_BUSY`: the table is borrowed; nothing was taken, call again.
    pub(crate) fn prepare_step(
        &self,
        storage: &dyn Storage,
        screen: u32,
        colour_screen: bool,
    ) -> Result<bool, i32> {
        let (job, garbage, number) = {
            let mut inner = self.lock()?;
            // One file a turn; the list keeps the room it was given.
            let garbage = if inner.garbage.is_empty() {
                None
            } else {
                Some(inner.garbage.remove(0))
            };
            let job = if inner.waiting.is_empty() {
                None
            } else {
                Some(inner.waiting.remove(0))
            };
            let number = inner.seq;
            inner.seq = inner.seq.wrapping_add(1);
            (job, garbage, number)
        };
        if let Some(path) = garbage {
            // A file the card will not delete stays: nothing more to do.
            let _ = storage.remove(&path);
        }
        let Some(job) = job else {
            return Ok(false);
        };
        let answer = self.decode(storage, job, number, screen, colour_screen);
        // Nothing else holds the table here, and its room was reserved with
        // the request: this cannot fail or allocate.
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.ready.push(answer);
        }
        Ok(true)
    }

    /// A request that cannot even be queued for the decoder (its source did
    /// not resolve on the card): the plugin is told `status` at once.
    ///
    /// # Errors
    /// `ERR_FULL`, `ERR_NO_MEMORY`, `ERR_BUSY`.
    pub(crate) fn refuse(&self, owner: u64, cookie: u32, status: u32) -> Result<(), i32> {
        let mut inner = self.lock()?;
        let count = inner.waiting.iter().filter(|w| w.owner == owner).count()
            + inner
                .ready
                .iter()
                .filter(|r| r.owner == owner && r.image == 0)
                .count();
        if count >= WAITING {
            return Err(ERR_FULL);
        }
        let pending = inner.waiting.len() + 1;
        inner
            .ready
            .try_reserve(pending)
            .map_err(|_| ERR_NO_MEMORY)?;
        inner.ready.push(Ready {
            owner,
            cookie,
            status,
            image: 0,
            width: 0,
            height: 0,
        });
        Ok(())
    }

    /// Tells the oldest answer: `deliver` queues it for its plugin and
    /// returns `Ok`, and it is gone; an `Err` leaves it for the next try.
    /// `Ok(false)`: nothing to tell.
    ///
    /// # Errors
    /// `ERR_BUSY` (the table is borrowed), else `deliver`'s.
    pub(crate) fn take_ready(
        &self,
        deliver: &mut dyn FnMut(&Ready) -> Result<(), i32>,
    ) -> Result<bool, i32> {
        let mut inner = self.lock()?;
        let Some(first) = inner.ready.first() else {
            return Ok(false);
        };
        deliver(first)?;
        inner.ready.remove(0);
        Ok(true)
    }

    /// Boot cleanup of the folder of prepared images: the files this host
    /// made there (`<owner>-<n>.img`) and no live image uses go, at most
    /// `max` in a call. Nothing outside the folder is touched, no
    /// subfolder is entered and any other name is left alone. `Ok(true)`: it
    /// may hold more, call again.
    ///
    /// # Errors
    /// `ERR_IO` (the card cannot list the folder; a missing folder is
    /// fine), `ERR_NO_MEMORY`, `ERR_BUSY`.
    pub(crate) fn sweep_cache(&self, storage: &dyn Storage, max: usize) -> Result<bool, i32> {
        let mut names: Vec<String> = Vec::new();
        let mut failure = None;
        let listed = storage.list(DIR, &mut |entry| {
            if failure.is_some() || names.len() >= max || entry.is_dir || !ours(entry.name) {
                return;
            }
            let mut name = String::new();
            if name
                .try_reserve_exact(DIR.len() + 1 + entry.name.len())
                .is_err()
                || names.try_reserve(1).is_err()
            {
                failure = Some(ERR_NO_MEMORY);
                return;
            }
            name.push_str(DIR);
            name.push('/');
            name.push_str(entry.name);
            names.push(name);
        });
        match listed {
            Ok(()) => {}
            Err(paperpaw_platform::StorageError::NotFound) => return Ok(false),
            Err(_) => return Err(paperpaw_native::host_glue::ERR_IO),
        }
        if let Some(error) = failure {
            return Err(error);
        }
        let inner = self
            .inner
            .try_borrow()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        let mut removed = 0;
        for name in &names {
            if inner.entries.iter().any(|e| &e.path == name) {
                continue;
            }
            if storage.remove(name).is_ok() {
                removed += 1;
            }
        }
        Ok(names.len() >= max && removed > 0)
    }

    fn decode(
        &self,
        storage: &dyn Storage,
        job: Waiting,
        number: u32,
        screen: u32,
        colour_screen: bool,
    ) -> Ready {
        let fail = |status| Ready {
            owner: job.owner,
            cookie: job.cookie,
            status,
            image: 0,
            width: 0,
            height: 0,
        };
        let side = if job.side == 0 { screen } else { job.side }.clamp(1, MAX_SIDE);
        let mut path = String::new();
        let name = alloc::format!("{DIR}/{:x}-{number:x}.img", job.owner);
        if path.try_reserve_exact(name.len()).is_err() {
            return fail(NO_MEMORY);
        }
        path.push_str(&name);
        let colour = job.colour && colour_screen;
        match run(storage, &job.source, &path, side, colour) {
            Ok(()) => {}
            Err(error) => {
                let _ = storage.remove(&path);
                return fail(status_of(&error));
            }
        }
        let Some((width, height)) = Decoded::peek(storage, &path).map(|d| d.size()) else {
            let _ = storage.remove(&path);
            return fail(CARD);
        };
        match self.add(job.owner, Origin::Prepared, &path, width, height, colour) {
            Ok(image) => Ready {
                owner: job.owner,
                cookie: job.cookie,
                status: READY,
                image,
                width,
                height,
            },
            Err(_) => {
                let _ = storage.remove(&path);
                fail(NO_MEMORY)
            }
        }
    }
}

/// Decodes the picture at `from` into the cache file `to`, at most `side`
/// pixels on the longest side.
fn run(
    storage: &dyn Storage,
    from: &str,
    to: &str,
    side: u32,
    colour: bool,
) -> Result<(), ImageError> {
    let mut head = [0_u8; 8];
    let n = storage
        .read_at(from, 0, &mut head)
        .map_err(|_| ImageError::Io)?;
    let format = Format::detect(&head[..n]).ok_or(ImageError::Unsupported("format"))?;
    let mut source = FileSource {
        storage,
        path: from,
        offset: 0,
    };
    let fit = Fit {
        short: side,
        long: side,
    };
    if colour {
        let mut writer = ColorWriter::new(storage, to);
        paperpaw_sdk::image::decode(format, &mut source, fit, &mut writer)
            .and_then(|()| writer.finish())
    } else {
        let mut writer = GrayWriter::new(storage, to);
        paperpaw_sdk::image::decode(format, &mut source, fit, &mut writer)
            .and_then(|()| writer.finish())
    }
}

#[cfg(test)]
#[path = "prepare_tests.rs"]
mod tests;
