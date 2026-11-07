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
use paperpaw_sdk::image::{ColorWriter, Fit, Format, GrayWriter, ImageError, Source};

/// Waiting requests a plugin may have.
pub(crate) const WAITING: usize = 4;
/// The directory of prepared images.
pub(super) const DIR: &str = ".paperpaw/cache/images";

/// `event.image_ready` `status`.
pub(crate) const READY: u32 = 0;
pub(crate) const UNSUPPORTED: u32 = 1;
pub(crate) const DAMAGED: u32 = 2;
pub(crate) const TOO_LARGE: u32 = 3;
pub(crate) const CARD: u32 = 4;
pub(crate) const NO_MEMORY: u32 = 5;

impl super::Inner {
    /// How many more requests `owner` may queue: every answer not yet told
    /// counts with the waiting ones (4 at most), and the prepared images, the
    /// waiting ones and the failures to tell together are at most 8.
    pub(super) fn room(&self, owner: u64) -> usize {
        let waiting = self.waiting.iter().filter(|w| w.owner == owner).count();
        let told = self.ready.iter().filter(|r| r.owner == owner);
        let (undelivered, failures) = told.fold((0, 0), |(all, fails), r| {
            (all + 1, fails + usize::from(r.image == 0))
        });
        let prepared = self
            .entries
            .iter()
            .filter(|e| e.owner == owner && e.origin == super::Origin::Prepared)
            .count();
        WAITING
            .saturating_sub(waiting + undelivered)
            .min(PREPARED_IMAGES.saturating_sub(prepared + waiting + failures))
    }
}

/// What a decode runs within: the device's.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Env {
    /// The longest side `side` 0 stands for (the screen's).
    pub(crate) screen: u32,
    /// Whether the screen keeps colour at all.
    pub(crate) colour: bool,
    /// The longest side a prepared picture may have (1,600 on the X4).
    pub(crate) max_side: u32,
    /// The most bytes of a PNG's source row (8 KiB on the X4, `usize::MAX`
    /// elsewhere): a wider PNG is status 3 and is not decoded.
    pub(crate) png_row_max: usize,
}

/// Whether the file at `path` is a PNG whose source row (its width in bits
/// a pixel, rounded up to bytes) is over `max`, from its first 33 bytes.
fn png_row_over(storage: &dyn Storage, path: &str, max: usize) -> bool {
    let mut head = [0_u8; 33];
    if storage.read_at(path, 0, &mut head).ok() != Some(head.len())
        || &head[..8] != b"\x89PNG\r\n\x1a\n"
        || &head[12..16] != b"IHDR"
    {
        return false;
    }
    let width = u64::from(u32::from_be_bytes([head[16], head[17], head[18], head[19]]));
    let depth = u64::from(head[24]);
    let channels = match head[25] {
        0 | 3 => 1,
        4 => 2,
        2 => 3,
        6 => 4,
        _ => return false,
    };
    (width * depth * channels).div_ceil(8) > max as u64
}

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

/// The card path of the prepared image of `owner` numbered `seq`:
/// `<dir>/<owner hex>-<seq hex>.img`, at most 52 bytes, made in room taken
/// first.
///
/// # Errors
/// `ERR_NO_MEMORY`.
pub(super) fn file_name(owner: u64, seq: u32) -> Result<String, i32> {
    let mut path = String::new();
    path.try_reserve_exact(64).map_err(|_| ERR_NO_MEMORY)?;
    let _ = core::fmt::Write::write_fmt(&mut path, format_args!("{DIR}/{owner:x}-{seq:x}.img"));
    Ok(path)
}

/// The owner and number of a file name `ours` made.
fn parse_name(name: &str) -> Option<(u64, u32)> {
    let (owner, number) = name.strip_suffix(".img")?.split_once('-')?;
    Some((
        u64::from_str_radix(owner, 16).ok()?,
        u32::from_str_radix(number, 16).ok()?,
    ))
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
///
/// TEMPORARY: the decoders say "too large" in the text of `Unsupported`;
/// draft 25 gives them `ImageError::TooLarge`, which replaces the text match
/// before native activation.
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
        if inner.room(owner) == 0 {
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
    /// decodes the oldest request, within `env`. The answer is held for
    /// [`ImageTable::take_ready`]; true if a request was decoded.
    ///
    /// The request's own failure is its answer (a status), not an error.
    ///
    /// # Errors
    /// `ERR_BUSY`: the table is borrowed; nothing was taken, call again.
    pub(crate) fn prepare_step(&self, storage: &dyn Storage, env: Env) -> Result<bool, i32> {
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
        if let Some((owner, seq)) = garbage {
            match file_name(owner, seq) {
                // A file the card will not delete stays: nothing more to do.
                Ok(path) => drop(storage.remove(&path)),
                // No room for its name now: it stays listed for a later turn
                // (the room it left is still there).
                Err(_) => {
                    if let Ok(mut inner) = self.inner.try_borrow_mut() {
                        inner.garbage.insert(0, (owner, seq));
                    }
                }
            }
        }
        let Some(job) = job else {
            return Ok(false);
        };
        let answer = self.decode(storage, job, number, env);
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
        if inner.room(owner) == 0 {
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
    /// made there (`<owner>-<n>.img`) that no live image uses go, at most
    /// `max` in a call. Nothing outside the folder is touched, no subfolder is
    /// entered and any other name is left alone.
    ///
    /// `cursor` (start at 0, keep it between calls) counts the matching files
    /// the sweep had to leave (a live image's, or one the card will not
    /// delete); the next call skips that many, so the files behind them are
    /// reached and the sweep ends. `Ok(true)`: more candidates are behind
    /// the ones handled, call again. `Ok(false)`: every candidate was handled
    /// (what the cursor counts is still there). The card lists the whole
    /// folder in each call (`Storage::list` cannot stop early or resume), so
    /// run it from idle time.
    ///
    /// # Errors
    /// `ERR_IO` (the card cannot list the folder; a missing folder is
    /// fine), `ERR_NO_MEMORY`, `ERR_BUSY`.
    pub(crate) fn sweep_cache(
        &self,
        storage: &dyn Storage,
        max: usize,
        cursor: &mut usize,
    ) -> Result<bool, i32> {
        let mut names: Vec<String> = Vec::new();
        let mut failure = None;
        let (mut seen, mut more) = (0_usize, false);
        let skip = *cursor;
        let listed = storage.list(DIR, &mut |entry| {
            if failure.is_some() || entry.is_dir || !ours(entry.name) {
                return;
            }
            seen += 1;
            if seen <= skip {
                return;
            }
            if names.len() >= max {
                more = true;
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
        for name in &names {
            let live = name
                .rsplit('/')
                .next()
                .and_then(parse_name)
                .is_some_and(|(o, n)| {
                    inner
                        .entries
                        .iter()
                        .any(|e| e.origin == Origin::Prepared && e.owner == o && e.seq == n)
                });
            if live || storage.remove(name).is_err() {
                // Left in place: the next call goes past it.
                *cursor += 1;
            }
        }
        Ok(more)
    }

    fn decode(&self, storage: &dyn Storage, job: Waiting, number: u32, env: Env) -> Ready {
        let fail = |status| Ready {
            owner: job.owner,
            cookie: job.cookie,
            status,
            image: 0,
            width: 0,
            height: 0,
        };
        let side =
            if job.side == 0 { env.screen } else { job.side }.clamp(1, env.max_side.min(MAX_SIDE));
        let Ok(path) = file_name(job.owner, number) else {
            return fail(NO_MEMORY);
        };
        let colour = job.colour && env.colour;
        // A PNG whose source row is over the device's cap is not decoded.
        if png_row_over(storage, &job.source, env.png_row_max) {
            return fail(TOO_LARGE);
        }
        match run(storage, &job.source, &path, side, colour) {
            Ok(()) => {}
            Err(error) => {
                let _ = storage.remove(&path);
                return fail(status_of(&error));
            }
        }
        // What the decoder wrote must be whole: its size is its header's.
        let Some((_, width, height)) = paperpaw_sdk::ui::probe_picture(storage, &path) else {
            let _ = storage.remove(&path);
            return fail(CARD);
        };
        match self.add_prepared(job.owner, number, width, height, colour) {
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
