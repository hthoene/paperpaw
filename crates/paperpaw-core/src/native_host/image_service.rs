//! Slots 36 `image.info` and 37 `canvas.draw_image` (`docs/abi/document.md`
//! section 5): the host's decoded pictures, named by an image id per plugin and
//! drawn in bands from the card.
//!
//! An image id (`u32`, a checked counter, never 0) names a picture the host may
//! draw. A document's images come from the host's `fmt.image` translation, the
//! cover and op 16, and end with the document; a plugin's own come from
//! `image_prepare` and end with `image_release` or when the plugin stops. Ids
//! are per plugin: another plugin's id is `ERR_NOT_FOUND`.
//!
//! **Eight document image ids per plugin.** A ninth ends the plugin's least
//! recently drawn document image that is not protected. An id handed out is
//! protected until it has been drawn once, and the protection ends at the
//! plugin's next drawn frame that does not use it ([`ImageTable::end_frame`]) or
//! at `image_release`; with all eight protected the ninth is `ERR_BUSY`.
//! Prepared images have their own eight (`ERR_FULL`) and are never evicted.
//!
//! **Drawing** takes no heap of its own beyond the band drawer's row buffers
//! of the picture's width (a picture is at most [`MAX_SIDE`] pixels a side):
//! the file is read row by row from the card by the SDK's band drawer
//! (`paperpaw_sdk::ui::draw_image_over`), clipped by the canvas. A picture that
//! the card no longer holds is `ERR_IO` (nothing drawn); a file whose header
//! does not say the stated size is `ERR_INVALID`.
//!
//! Flags: bit 0 fill and crop, bit 1 small (not counted towards the e-paper
//! ghost full refresh), bits 8 to 15 transparency (255 minus the opacity); an
//! alpha cache image blends over what is below, all by the SDK's
//! `draw_image_over`. Other bits answer `ERR_UNSUPPORTED`.
use super::registrations::{self as wire, Record};
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_FULL, ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE,
    ERR_UNSUPPORTED, OK, PpBuffer, buffers,
};
use paperpaw_platform::{Rect, Storage};

/// Document image ids a plugin holds at once, and prepared ones.
pub(crate) const DOC_IMAGES: usize = 8;
pub(crate) const PREPARED_IMAGES: usize = 8;
/// The longest side of a picture the host keeps (cache images are at most
/// 1,600 pixels; the bound keeps the band drawer's rows small).
pub(crate) const MAX_SIDE: u32 = 4096;
/// The longest cache path.
pub(crate) const PATH_MAX: usize = 512;

/// A cache path of root 2 (`document.md` 5): relative, no `..`, `\`, NUL,
/// empty segments or leading `/`, and not the host's `.complete` or `.host/`.
fn cache_path_ok(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', '\0'])
        && path.split('/').all(|part| !part.is_empty() && part != "..")
        && path != ".complete"
        && path.split('/').next() != Some(".host")
}

/// `canvas.draw_image` flags.
const FILL: u32 = 1;
/// Flags bit 1: a small picture, no ghost full refresh for it.
const SMALL: u32 = 2;
/// Flags bits 8 to 15: 255 minus the opacity (0, opaque, by default).
const TRANSPARENCY_SHIFT: u32 = 8;
const KNOWN: u32 = FILL | SMALL | (0xff << TRANSPARENCY_SHIFT);

/// Where an image came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// A picture of an open document (`index`: the format's number for it,
    /// `u32::MAX` the cover).
    Document { document: u32, index: u32 },
    /// A plugin's own, from `image_prepare`.
    Prepared,
}

struct Entry {
    id: u32,
    owner: u64,
    origin: Origin,
    path: String,
    width: u32,
    height: u32,
    colour: bool,
    /// Never ended before it has been drawn once; see the module.
    protected: bool,
    /// The plugin drew it in its current frame.
    used_now: bool,
    /// When it was drawn last (a device counter), 0 for never.
    drawn: u64,
}

struct Inner {
    entries: Vec<Entry>,
    next_id: u32,
    stamp: u64,
}

/// The device's images, all plugins'.
pub(crate) struct ImageTable {
    inner: RefCell<Inner>,
}

/// What `image.info` tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Info {
    pub(crate) image: u32,
    pub(crate) index: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) flags: u32,
}

impl Default for ImageTable {
    fn default() -> Self {
        Self::new()
    }
}

/// The part of the rectangle `(x, y, width, height)` inside `clip`, if any.
fn visible(clip: Rect, x: i32, y: i32, width: u32, height: u32) -> Option<Rect> {
    let left = i64::from(x).max(i64::from(clip.x));
    let top = i64::from(y).max(i64::from(clip.y));
    let right = (i64::from(x) + i64::from(width)).min(i64::from(clip.right()));
    let bottom = (i64::from(y) + i64::from(height)).min(i64::from(clip.bottom()));
    if right <= left || bottom <= top {
        return None;
    }
    Some(Rect::new(
        u16::try_from(left).ok()?,
        u16::try_from(top).ok()?,
        u16::try_from(right - left).ok()?,
        u16::try_from(bottom - top).ok()?,
    ))
}

impl ImageTable {
    /// An empty table.
    pub(crate) const fn new() -> Self {
        Self {
            inner: RefCell::new(Inner {
                entries: Vec::new(),
                next_id: 0,
                stamp: 0,
            }),
        }
    }

    fn lock(&self) -> Result<core::cell::RefMut<'_, Inner>, i32> {
        self.inner.try_borrow_mut().map_err(|_| ERR_BUSY)
    }

    /// Gives a picture an image id for `owner`: the id it already has for the
    /// same path in the same document, else a new one.
    ///
    /// # Errors
    /// `ERR_INVALID` (a side of 0 or over [`MAX_SIDE`], an empty or overlong
    /// path), `ERR_BUSY` (a document image over eight while all are
    /// protected), `ERR_FULL` (a ninth prepared image), `ERR_NO_MEMORY`,
    /// `ERR_NO_SPACE` (the id counter is spent).
    pub(crate) fn add(
        &self,
        owner: u64,
        origin: Origin,
        path: &str,
        width: u32,
        height: u32,
        colour: bool,
    ) -> Result<u32, i32> {
        if !(1..=MAX_SIDE).contains(&width)
            || !(1..=MAX_SIDE).contains(&height)
            || path.is_empty()
            || path.len() > PATH_MAX
        {
            return Err(ERR_INVALID);
        }
        let mut inner = self.lock()?;
        if let Origin::Document { document, .. } = origin
            && let Some(known) = inner.entries.iter().find(|e| {
                e.owner == owner
                    && matches!(e.origin, Origin::Document { document: d, .. } if d == document)
                    && e.path == path
            })
        {
            return Ok(known.id);
        }
        let held = |inner: &Inner, prepared: bool| {
            inner
                .entries
                .iter()
                .filter(|e| e.owner == owner && (e.origin == Origin::Prepared) == prepared)
                .count()
        };
        let mut evict = None;
        match origin {
            Origin::Prepared if held(&inner, true) >= PREPARED_IMAGES => return Err(ERR_FULL),
            Origin::Document { .. } if held(&inner, false) >= DOC_IMAGES => {
                evict = inner
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| {
                        e.owner == owner && e.origin != Origin::Prepared && !e.protected
                    })
                    .min_by_key(|(_, e)| e.drawn)
                    .map(|(i, _)| i);
                if evict.is_none() {
                    return Err(ERR_BUSY);
                }
            }
            _ => {}
        }
        // Everything that can fail comes before the first change.
        let id = inner.next_id.checked_add(1).ok_or(ERR_NO_SPACE)?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(path.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        owned.push_str(path);
        if evict.is_none() {
            inner.entries.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        }
        let entry = Entry {
            id,
            owner,
            origin,
            path: owned,
            width,
            height,
            colour,
            protected: origin != Origin::Prepared,
            used_now: false,
            drawn: 0,
        };
        inner.next_id = id;
        match evict {
            Some(index) => inner.entries[index] = entry,
            None => inner.entries.push(entry),
        }
        Ok(id)
    }

    /// A format's picture (`fmt.image`, whose `path` is below the cache
    /// root `cache`) as an image id of `owner` for `document` (`document.md`
    /// 5): the id the picture already has, else a new one.
    ///
    /// `Ok(None)`: the picture fails the check (a path outside the root-2
    /// rules or host-reserved, a cache file the card does not give or whose
    /// header does not say `width` x `height`), so it is left out of the
    /// stream, never failing it. `Ok(Some(0))`: all eight ids are handed
    /// out and none drawn, in a block stream (`in_block`) only.
    ///
    /// # Errors
    /// `ERR_BUSY` (the same, outside a block stream, or the table is
    /// borrowed), `ERR_NO_MEMORY`, `ERR_NO_SPACE` (the id counter is spent).
    #[allow(clippy::too_many_arguments, reason = "what the translation needs")]
    pub(crate) fn map_document(
        &self,
        storage: &dyn Storage,
        owner: u64,
        document: u32,
        cache: &str,
        path: &str,
        image: &wire::ServiceDocImage,
        in_block: bool,
    ) -> Result<Option<u32>, i32> {
        if !cache_path_ok(path) || cache.len().saturating_add(1 + path.len()) > PATH_MAX {
            return Ok(None);
        }
        let mut full = String::new();
        full.try_reserve_exact(cache.len() + 1 + path.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        full.push_str(cache);
        full.push('/');
        full.push_str(path);
        // Downloaded input: the header must say what the format says.
        let Some(decoded) = paperpaw_sdk::image::Decoded::peek(storage, &full) else {
            return Ok(None);
        };
        if decoded.size() != (image.width, image.height) {
            return Ok(None);
        }
        let origin = Origin::Document {
            document,
            index: image.index,
        };
        match self.add(
            owner,
            origin,
            &full,
            image.width,
            image.height,
            image.flags & 1 != 0,
        ) {
            Ok(id) => Ok(Some(id)),
            Err(ERR_INVALID) => Ok(None),
            Err(ERR_BUSY) if in_block => Ok(Some(0)),
            Err(error) => Err(error),
        }
    }

    /// `image_release`, or a reader giving a document image up.
    ///
    /// # Errors
    /// `ERR_NOT_FOUND` (not this plugin's, or ended), `ERR_BUSY`.
    pub(crate) fn release(&self, owner: u64, id: u32) -> Result<(), i32> {
        let mut inner = self.lock()?;
        let at = inner
            .entries
            .iter()
            .position(|e| e.id == id && e.owner == owner)
            .ok_or(ERR_NOT_FOUND)?;
        inner.entries.remove(at);
        Ok(())
    }

    /// `image.info`.
    ///
    /// # Errors
    /// `ERR_NOT_FOUND`, `ERR_BUSY`.
    pub(crate) fn info(&self, owner: u64, id: u32) -> Result<Info, i32> {
        let inner = self.inner.try_borrow().map_err(|_| ERR_BUSY)?;
        let e = inner
            .entries
            .iter()
            .find(|e| e.id == id && e.owner == owner)
            .ok_or(ERR_NOT_FOUND)?;
        Ok(Info {
            image: e.id,
            index: match e.origin {
                Origin::Document { index, .. } => index,
                Origin::Prepared => 0,
            },
            width: e.width,
            height: e.height,
            flags: u32::from(e.colour),
        })
    }

    /// The document ended: its images end with it.
    ///
    /// # Errors
    /// `ERR_BUSY`: a retry finishes it.
    pub(crate) fn document_closed(&self, document: u32) -> Result<(), i32> {
        self.lock()?
            .entries
            .retain(|e| !matches!(e.origin, Origin::Document { document: d, .. } if d == document));
        Ok(())
    }

    /// The plugin stopped: all its images end.
    ///
    /// # Errors
    /// `ERR_BUSY`: a retry finishes it.
    pub(crate) fn owner_stopped(&self, owner: u64) -> Result<(), i32> {
        self.lock()?.entries.retain(|e| e.owner != owner);
        Ok(())
    }

    /// The plugin's draw callback ended. If it drew a picture in it, the ids
    /// it handed out and did not use are no longer protected.
    ///
    /// # Errors
    /// `ERR_BUSY`: the frame's end is not lost, a retry applies it.
    pub(crate) fn end_frame(&self, owner: u64) -> Result<(), i32> {
        let mut inner = self.lock()?;
        let drew = inner.entries.iter().any(|e| e.owner == owner && e.used_now);
        for e in inner.entries.iter_mut().filter(|e| e.owner == owner) {
            if drew && !e.used_now {
                e.protected = false;
            }
            e.used_now = false;
        }
        Ok(())
    }

    /// `canvas.draw_image` on `canvas` (already inside the caller's `draw`):
    /// the picture scaled into `(x, y, width, height)` keeping its shape,
    /// centred (`FILL`: covering it and cropped to it), clipped by the canvas.
    ///
    /// # Errors
    /// `ERR_INVALID` (a drawn picture beyond 65,535 pixels, a file that does
    /// not say its stated size), `ERR_UNSUPPORTED` (reserved flag bits),
    /// `ERR_NOT_FOUND`, `ERR_IO` (the card cannot read it, nothing drawn),
    /// `ERR_BUSY`.
    #[allow(clippy::too_many_arguments, reason = "the slot's scalars")]
    pub(crate) fn draw(
        &self,
        owner: u64,
        id: u32,
        canvas: &mut Canvas<'_>,
        storage: &dyn Storage,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        flags: u32,
    ) -> Result<(), i32> {
        if flags & !KNOWN != 0 {
            return Err(ERR_UNSUPPORTED);
        }
        let opacity = 255 - u8::try_from((flags >> TRANSPARENCY_SHIFT) & 0xff).unwrap_or(0);
        let (path, w, h) = {
            let inner = self.inner.try_borrow().map_err(|_| ERR_BUSY)?;
            let e = inner
                .entries
                .iter()
                .find(|e| e.id == id && e.owner == owner)
                .ok_or(ERR_NOT_FOUND)?;
            let mut path = String::new();
            path.try_reserve_exact(e.path.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            path.push_str(&e.path);
            (path, e.width, e.height)
        };
        // The file is the card's, downloaded input: its header must say what
        // the id says, and it must be readable now.
        let (fw, fh) = paperpaw_sdk::image::Decoded::peek(storage, &path)
            .ok_or(ERR_IO)?
            .size();
        if (fw, fh) != (w, h) {
            return Err(ERR_INVALID);
        }
        let Some(place) = place(x, y, width, height, w, h, flags & FILL != 0)? else {
            return Ok(());
        };
        let Some(visible) = visible(canvas.clip(), x, y, width, height) else {
            // Nothing of the rectangle is on the canvas; still a draw.
            return self.drawn(owner, id);
        };
        let image = paperpaw_sdk::document::ImageRef {
            path,
            width: w,
            height: h,
        };
        let mut child = canvas.clipped(visible).offset(place.left, place.top);
        let area = Rect::new(0, 0, place.width, place.height);
        if flags & SMALL != 0 {
            child.small_pictures(|child| {
                paperpaw_sdk::ui::draw_image_over(child, storage, &image, area, opacity);
            });
        } else {
            paperpaw_sdk::ui::draw_image_over(&mut child, storage, &image, area, opacity);
        }
        self.drawn(owner, id)
    }

    /// Marks `id` drawn: unprotected from the next frame, and the newest.
    fn drawn(&self, owner: u64, id: u32) -> Result<(), i32> {
        let mut inner = self.lock()?;
        inner.stamp = inner.stamp.saturating_add(1);
        let stamp = inner.stamp;
        if let Some(e) = inner
            .entries
            .iter_mut()
            .find(|e| e.id == id && e.owner == owner)
        {
            e.used_now = true;
            // Drawn once: it may end now (the least recently drawn goes first).
            e.protected = false;
            e.drawn = stamp;
        }
        Ok(())
    }
}

/// Where a picture of `(sw, sh)` goes in `(x, y, width, height)`.
#[derive(Debug, PartialEq, Eq)]
struct Place {
    left: i32,
    top: i32,
    width: u16,
    height: u16,
}

/// The picture scaled to fit (or, `cover`, to cover) the rectangle, centred.
/// `None` for an empty rectangle. Computed wide: nothing overflows.
fn place(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    sw: u32,
    sh: u32,
    cover: bool,
) -> Result<Option<Place>, i32> {
    if width == 0 || height == 0 {
        return Ok(None);
    }
    let (tw, th) = (u64::from(width), u64::from(height));
    let (sw, sh) = (u64::from(sw), u64::from(sh));
    // Fit when the target is relatively wider than the picture, else the
    // other way; cover is the opposite choice.
    let width_limits = (tw * sh <= th * sw) != cover;
    let (dw, dh) = if width_limits {
        (tw, (tw * sh).div_ceil(sw).max(1))
    } else {
        ((th * sw).div_ceil(sh).max(1), th)
    };
    let (dw, dh) = (
        u16::try_from(dw).map_err(|_| ERR_INVALID)?,
        u16::try_from(dh).map_err(|_| ERR_INVALID)?,
    );
    let left = i64::from(x) + (i64::try_from(tw).unwrap_or(0) - i64::from(dw)) / 2;
    let top = i64::from(y) + (i64::try_from(th).unwrap_or(0) - i64::from(dh)) / 2;
    Ok(Some(Place {
        left: i32::try_from(left).map_err(|_| ERR_INVALID)?,
        top: i32::try_from(top).map_err(|_| ERR_INVALID)?,
        width: dw,
        height: dh,
    }))
}

/// `image.info` written into `out` as one `doc_image` record; the length.
///
/// # Errors
/// `ERR_NO_SPACE` (`out` is too small, nothing written).
pub(crate) fn encode_info(info: Info, out: &mut [u8]) -> Result<usize, i32> {
    wire::record_encode_one(
        &Record::ServiceDocImage(wire::ServiceDocImage {
            image: info.image,
            index: info.index,
            width: info.width,
            height: info.height,
            flags: info.flags,
        }),
        out,
    )
}

/// Slot 36: `image.info` into `out` (a `doc_image` record, at most 64 bytes).
/// The length written, or a negative status.
///
/// # Safety
/// `out` is live and writable for its capacity, disjoint from the context.
pub(super) unsafe fn info_slot(
    resources: &super::resources::OwnerResources,
    image: u32,
    out: PpBuffer,
) -> i32 {
    // SAFETY: as documented.
    let out = match unsafe { buffers::buffer_out(out, INFO_MAX) } {
        Ok(out) => out,
        Err(status) => return status,
    };
    let info = match resources.hosts().images().info(resources.owner_id(), image) {
        Ok(info) => info,
        Err(status) => return status,
    };
    let mut record = [0u8; INFO_MAX];
    match encode_info(info, &mut record) {
        Ok(len) => buffers::copy_required(&record[..len], out, INFO_MAX),
        Err(status) => status,
    }
}

/// Slot 37: `canvas.draw_image`, only inside the caller's own `draw`.
#[allow(clippy::too_many_arguments, reason = "the slot's scalars")]
pub(super) fn draw_slot(
    canvas: &super::canvas::CanvasScope,
    environment: &super::environment::EnvironmentScope,
    resources: &super::resources::OwnerResources,
    image: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    flags: u32,
) -> i32 {
    let owner = resources.owner_id();
    let table = resources.hosts().images();
    let result = canvas.with_canvas(|canvas| {
        environment
            .with_frame(|frame| {
                let storage = frame.storage().ok_or(ERR_STATE)?;
                table.draw(owner, image, canvas, storage, x, y, width, height, flags)
            })
            .and_then(core::convert::identity)
    });
    match result {
        Ok(Ok(())) => OK,
        Ok(Err(status)) | Err(status) => status,
    }
}

/// `image.info` answers one record of at most this many bytes (`abi.def`).
const INFO_MAX: usize = 64;

#[cfg(test)]
#[path = "image_service_tests.rs"]
mod tests;
