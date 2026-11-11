//! `command.image_prepare`: decoding a plugin's own picture into the host's
//! image cache in the background, one request per main-loop turn
//! (`document.md` 5). The command's admission (the caller's root 0 or 1
//! resolved to a card path, `uses.storage`) and the delivery of
//! `event.image_ready` are the shell's; this is the queue, the decode and the
//! ids.
use super::recipe::{self, Recipe, ToneSink};
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
/// The source file is not there (any root; a built-in plugin's install
/// folder is empty, so every path in it).
pub(crate) const NOT_FOUND: u32 = 6;

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
    /// The most bytes of one tile of a quarter turn (`turn`): 32 KiB on the
    /// X4 (the draw's own ceiling), 1 MiB elsewhere.
    pub(crate) turn_band: usize,
    /// The most short reads a quarter turn makes (16,384 on the X4, where a
    /// turn runs in one main-loop turn; `usize::MAX` elsewhere): a picture
    /// that would need more is status 3 and is not turned.
    pub(crate) turn_reads: usize,
}

/// Entries of the cache folder a sweep call looks at.
pub(crate) const SWEEP_SCAN: usize = 32;

/// Where a sweep of the cache folder is ([`ImageTable::sweep_cache`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SweepCursor {
    /// The place the next piece of the listing starts at.
    place: u64,
    /// Whether this pass removed a file.
    removed: bool,
}

impl Env {
    /// The X4's: the longest side a prepared picture may have is 1,600
    /// pixels and a PNG's source row at most 8 KiB.
    pub(crate) const fn x4(screen: u32, colour: bool) -> Self {
        Self {
            screen,
            colour,
            max_side: 1_600,
            png_row_max: 8 * 1024,
            turn_band: 32 * 1024,
            turn_reads: 16_384,
        }
    }
}

/// `status` for a card read that failed: a missing file is its own status.
fn card_status(error: paperpaw_platform::StorageError) -> u32 {
    if error == paperpaw_platform::StorageError::NotFound {
        NOT_FOUND
    } else {
        CARD
    }
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

/// The most pixels a source picture may have (8 Mpx, `document.md` 5).
pub(crate) const SOURCE_PIXELS_MAX: u64 = 8_000_000;

/// Frame headers a JPEG's size is looked for in before it is no picture.
const JPEG_SEGMENTS_MAX: usize = 256;

/// The pixel count the header of the JPEG, PNG or GIF at `path` declares,
/// from a few small reads and no allocation: `Ok(None)` for a format this
/// does not read (the decoder refuses it), `Err(status)` for a header that
/// cannot be read or has no size (`DAMAGED`; `NOT_FOUND` for a missing file; `CARD` where the card fails).
fn source_pixels(storage: &dyn Storage, path: &str) -> Result<Option<u64>, u32> {
    let mut head = [0_u8; 33];
    let n = storage.read_at(path, 0, &mut head).map_err(card_status)?;
    let head = &head[..n];
    let size = match Format::detect(head) {
        Some(Format::Png) => {
            if head.len() < 24 || &head[12..16] != b"IHDR" {
                return Err(DAMAGED);
            }
            paperpaw_sdk::image::dimensions(head)
        }
        Some(Format::Gif) => {
            if head.len() < 10 {
                return Err(DAMAGED);
            }
            // The decoder makes the first frame at the frame's own size, so
            // that is counted too, and a frame outside its screen is no GIF.
            let (frame_w, frame_h) = gif_frame(storage, path)?;
            let screen = paperpaw_sdk::image::dimensions(head).ok_or(DAMAGED)?;
            if frame_w > screen.0 || frame_h > screen.1 {
                return Err(DAMAGED);
            }
            let pixels = (u64::from(screen.0) * u64::from(screen.1))
                .max(u64::from(frame_w) * u64::from(frame_h));
            return Ok(Some(pixels));
        }
        Some(Format::Jpeg) => jpeg_size(storage, path)?,
        _ => return Ok(None),
    };
    let (width, height) = size.ok_or(DAMAGED)?;
    Ok(Some(u64::from(width) * u64::from(height)))
}

/// Blocks (and extension sub-blocks) walked to a GIF's first frame.
const GIF_BLOCKS_MAX: usize = 1024;

/// The size of a GIF's first frame, from its image descriptor, found by
/// walking the blocks before it with small reads; `DAMAGED` where the walk
/// ends first, goes on too long, or the frame has no size.
fn gif_frame(storage: &dyn Storage, path: &str) -> Result<(u32, u32), u32> {
    let mut flags = [0_u8; 1];
    if storage.read_at(path, 10, &mut flags).map_err(card_status)? != 1 {
        return Err(DAMAGED);
    }
    // The header, the screen descriptor and the global colour table.
    let mut at = 13_u64;
    if flags[0] & 0x80 != 0 {
        at += 3 << ((flags[0] & 7) + 1);
    }
    let mut steps = 0;
    while steps < GIF_BLOCKS_MAX {
        steps += 1;
        let mut block = [0_u8; 9];
        let n = storage.read_at(path, at, &mut block).map_err(card_status)?;
        match block.first() {
            // An image descriptor: `2C left(2) top(2) width(2) height(2)`.
            Some(0x2c) if n == 9 => {
                let width = u32::from(u16::from_le_bytes([block[5], block[6]]));
                let height = u32::from(u16::from_le_bytes([block[7], block[8]]));
                return if width > 0 && height > 0 {
                    Ok((width, height))
                } else {
                    Err(DAMAGED)
                };
            }
            // An extension: `21 label`, then sub-blocks of a length byte
            // and that many bytes, ended by a length of 0.
            Some(0x21) if n >= 2 => {
                at += 2;
                loop {
                    steps += 1;
                    let mut len = [0_u8; 1];
                    if steps > GIF_BLOCKS_MAX
                        || storage.read_at(path, at, &mut len).map_err(card_status)? != 1
                    {
                        return Err(DAMAGED);
                    }
                    at += 1 + u64::from(len[0]);
                    if len[0] == 0 {
                        break;
                    }
                }
            }
            _ => return Err(DAMAGED),
        }
    }
    Err(DAMAGED)
}

/// A JPEG's size from its first frame header, walking the segments before
/// it by small reads (a file with a long EXIF block puts it far from the
/// start); `DAMAGED` where the walk ends first or goes on too long.
fn jpeg_size(storage: &dyn Storage, path: &str) -> Result<Option<(u32, u32)>, u32> {
    let mut at = 2_u64;
    for _ in 0..JPEG_SEGMENTS_MAX {
        let mut seg = [0_u8; 9];
        let n = storage.read_at(path, at, &mut seg).map_err(card_status)?;
        if n < 2 || seg[0] != 0xff {
            return Err(DAMAGED);
        }
        match seg[1] {
            // Fill byte: the next byte may be the marker.
            0xff => at += 1,
            // Markers with no length.
            0x01 | 0xd0..=0xd8 => at += 2,
            // A frame header: `FF Cn len(2) precision height(2) width(2)`.
            0xc0..=0xcf if !matches!(seg[1], 0xc4 | 0xc8 | 0xcc) => {
                if n < 9 {
                    return Err(DAMAGED);
                }
                let height = u32::from(u16::from_be_bytes([seg[5], seg[6]]));
                let width = u32::from(u16::from_be_bytes([seg[7], seg[8]]));
                return Ok((width > 0 && height > 0).then_some((width, height)));
            }
            // Start of scan or end: no frame header came first.
            0xd9 | 0xda => return Err(DAMAGED),
            _ => {
                if n < 4 {
                    return Err(DAMAGED);
                }
                at += 2 + u64::from(u16::from_be_bytes([seg[2], seg[3]]));
            }
        }
    }
    Err(DAMAGED)
}

/// A request not yet decoded.
pub(crate) struct Waiting {
    pub(crate) owner: u64,
    cookie: u32,
    side: u32,
    colour: bool,
    /// What `image_prepare` asked besides the size and the colour.
    recipe: Recipe,
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
    /// The mean brightness of an 8 by 8 grid over the picture, for a ready
    /// one.
    pub(crate) luma: Option<[u8; recipe::LUMA_BYTES]>,
}

impl Ready {
    /// The record the plugin gets, `now_ms` the event's time.
    pub(crate) fn record(&self, now_ms: u64) -> wire::Record<'_> {
        wire::Record::EventImageReady(wire::EventImageReady {
            now_ms,
            cookie: self.cookie,
            status: self.status,
            image: self.image,
            width: self.width,
            height: self.height,
            luma: self.luma.as_ref().map_or(&[][..], |grid| &grid[..]),
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

/// `status` for a decoder's refusal: too large for good (3) is not the same
/// as not enough memory now (5, try again later).
fn status_of(error: &ImageError) -> u32 {
    match error {
        ImageError::TooLarge => TOO_LARGE,
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
        recipe: &Recipe,
    ) -> Result<(), i32> {
        if source.is_empty() || source.len() > super::PATH_MAX {
            return Err(ERR_INVALID);
        }
        let mut inner = self.lock()?;
        if inner.room(owner) == 0 {
            return Err(ERR_FULL);
        }
        let recipe = recipe.try_copy()?;
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
            recipe,
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
            inner.seq = inner.seq.wrapping_add(1) & 0x7fff_ffff;
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
            luma: None,
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
    /// `max` in a call and `SWEEP_SCAN` entries looked at. Nothing outside
    /// the folder is touched, no subfolder is entered and any other name is
    /// left alone.
    ///
    /// `cursor` (start at the default, keep it between calls) is where the
    /// listing goes on (the folder is read a piece at a time, not once a
    /// call). Files removed between pieces may move the
    /// places of the entries behind them, so a pass that removed anything is
    /// followed by another, and the sweep ends with a pass that removed
    /// nothing: no file it can delete is left behind a false end.
    /// `Ok(true)`: call again. `Ok(false)`: done. A `max` of 0 is `ERR_INVALID`:
    /// with no budget the cursor would never move.
    /// The listing is read with [`Storage::list_bounded`]: no entry past the
    /// one the call stops at, and a place that no longer holds starts at the
    /// folder's start (the pass that follows a removal finds what is left).
    ///
    /// # Errors
    /// `ERR_INVALID` (`max` is 0), `ERR_IO` (the card cannot list the folder; a
    /// missing folder is fine), `ERR_NO_MEMORY`, `ERR_BUSY`; the cursor is as
    /// it was.
    pub(crate) fn sweep_cache(
        &self,
        storage: &dyn Storage,
        max: usize,
        cursor: &mut SweepCursor,
    ) -> Result<bool, i32> {
        if max == 0 {
            return Err(ERR_INVALID);
        }
        let mut names: Vec<String> = Vec::new();
        let mut failure = None;
        let mut looked = 0_usize;
        let listed = storage.list_bounded(DIR, cursor.place, &mut |entry| {
            if failure.is_some() || looked >= SWEEP_SCAN || names.len() >= max {
                return false;
            }
            looked += 1;
            if entry.is_dir || !ours(entry.name) {
                return true;
            }
            let mut name = String::new();
            if name
                .try_reserve_exact(DIR.len() + 1 + entry.name.len())
                .is_err()
                || names.try_reserve(1).is_err()
            {
                failure = Some(ERR_NO_MEMORY);
                return false;
            }
            name.push_str(DIR);
            name.push('/');
            name.push_str(entry.name);
            names.push(name);
            true
        });
        let next = match listed {
            Ok(next) => next,
            Err(paperpaw_platform::StorageError::NotFound) => {
                *cursor = SweepCursor::default();
                return Ok(false);
            }
            Err(_) => return Err(paperpaw_native::host_glue::ERR_IO),
        };
        if let Some(error) = failure {
            return Err(error);
        }
        let inner = self
            .inner
            .try_borrow()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        let mut removed = false;
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
            // One the card will not delete stays: it ends no pass later.
            if !live && storage.remove(name).is_ok() {
                removed = true;
            }
        }
        cursor.removed |= removed;
        match next {
            Some(place) => {
                cursor.place = place;
                Ok(true)
            }
            None if cursor.removed => {
                *cursor = SweepCursor::default();
                Ok(true)
            }
            None => {
                *cursor = SweepCursor::default();
                Ok(false)
            }
        }
    }

    fn decode(&self, storage: &dyn Storage, job: Waiting, number: u32, env: Env) -> Ready {
        let fail = |status| Ready {
            owner: job.owner,
            cookie: job.cookie,
            status,
            image: 0,
            width: 0,
            height: 0,
            luma: None,
        };
        let side =
            if job.side == 0 { env.screen } else { job.side }.clamp(1, env.max_side.min(MAX_SIDE));
        let Ok(path) = file_name(job.owner, number) else {
            return fail(NO_MEMORY);
        };
        let colour = job.colour && env.colour;
        // A source over 8 Mpx is refused from its header, before a decoder
        // reads a byte of its data; a header that cannot be read is no
        // picture (`document.md` 5).
        match source_pixels(storage, &job.source) {
            Ok(Some(pixels)) if pixels > SOURCE_PIXELS_MAX => return fail(TOO_LARGE),
            Ok(_) => {}
            Err(status) => return fail(status),
        }
        // A PNG whose source row is over the device's cap is not decoded.
        if png_row_over(storage, &job.source, env.png_row_max) {
            return fail(TOO_LARGE);
        }
        if let Err(status) = make(storage, &job, (&path, number), side, colour, env) {
            return fail(status);
        }
        // What the decoder wrote must be whole: its size is its header's.
        let Ok((kind, width, height)) = paperpaw_sdk::ui::probe_picture(storage, &path) else {
            let _ = storage.remove(&path);
            return fail(CARD);
        };
        let luma = recipe::luma_grid(
            storage,
            &path,
            (
                width as usize,
                height as usize,
                if kind == paperpaw_sdk::ui::PictureKind::Color {
                    3
                } else {
                    1
                },
            ),
        );
        match self.add_prepared(job.owner, number, width, height, colour) {
            Ok(image) => Ready {
                owner: job.owner,
                cookie: job.cookie,
                status: READY,
                image,
                width,
                height,
                luma,
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
    (colour, tone): (bool, Option<&[u8]>),
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
        decode_into(format, &mut source, fit, &mut writer, tone)?;
        writer.finish()
    } else {
        let mut writer = GrayWriter::new(storage, to);
        decode_into(format, &mut source, fit, &mut writer, tone)?;
        writer.finish()
    }
}

/// The decode into `writer`, through the tone table where there is one.
fn decode_into<S: paperpaw_sdk::image::ImageSink>(
    format: Format,
    source: &mut FileSource<'_>,
    fit: Fit,
    writer: &mut S,
    tone: Option<&[u8]>,
) -> Result<(), ImageError> {
    match tone {
        Some(table) => {
            let mut toned = ToneSink::new(writer, table);
            paperpaw_sdk::image::decode(format, source, fit, &mut toned)
        }
        None => paperpaw_sdk::image::decode(format, source, fit, writer),
    }
}

/// The name of the file a turned picture is first decoded to: made like
/// [`file_name`] with the top bit of the number set, so that the boot sweep
/// removes one a power cut left.
fn first_name(owner: u64, number: u32) -> Result<String, i32> {
    file_name(owner, number | 0x8000_0000)
}

/// Decodes the request's picture to `path` (the final file) with its recipe:
/// the tone table while it is decoded, then the turn its JPEG's EXIF
/// orientation asks for. Nothing is left behind on a failure; the status is
/// the answer.
fn make(
    storage: &dyn Storage,
    job: &Waiting,
    (path, number): (&str, u32),
    side: u32,
    colour: bool,
    env: Env,
) -> Result<(), u32> {
    let tone = job.recipe.tone.as_deref();
    // The orientation is read before the decode, so that a picture that is
    // not turned is decoded straight into its file.
    let mut head = [0_u8; 8];
    let is_jpeg = storage
        .read_at(&job.source, 0, &mut head)
        .is_ok_and(|n| Format::detect(&head[..n]) == Some(Format::Jpeg));
    let orientation = if job.recipe.turn && is_jpeg {
        recipe::exif_orientation(storage, &job.source)
    } else {
        1
    };
    if !recipe::needs_turn(orientation) {
        return run(storage, &job.source, path, side, (colour, tone)).map_err(|error| {
            let _ = storage.remove(path);
            status_of(&error)
        });
    }
    let first = first_name(job.owner, number).map_err(|_| NO_MEMORY)?;
    let made = run(storage, &job.source, &first, side, (colour, tone)).and_then(|()| {
        recipe::turn_file(
            storage,
            (&first, path),
            orientation,
            if colour { 3 } else { 1 },
            (env.turn_band, env.turn_reads),
        )
    });
    let _ = storage.remove(&first);
    made.map_err(|error| {
        let _ = storage.remove(path);
        status_of(&error)
    })
}

#[cfg(test)]
#[path = "prepare_tests.rs"]
mod tests;
