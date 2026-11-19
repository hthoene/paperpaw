//! What the picture stage (`core.picture`, `docs/abi/document.md` 5) says of
//! a picture, kept with its image: a tone curve of 256 grays its rows go
//! through when they are drawn, and whether it is drawn in dots or in grays
//! where a draw names neither. The stage is asked once for a prepared
//! picture, before its plugin is told of it, never at a draw.
use super::{DOTS, GRAYS, ImageTable, Origin, prepare};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_platform::Storage;

/// The bytes of a tone curve.
pub(crate) const TONE_BYTES: usize = 256;
/// The gray bins of the histogram the stage is given.
pub(crate) const BINS: usize = 16;
/// The rows of a picture its histogram is counted from, spread over it.
const SAMPLED_ROWS: u32 = 16;
/// `picture.ask.place`: a picture on a page of a book, a page of a format
/// of whole pages (a comic), a book's cover, a small picture, a picture of
/// the sleep screen, a picture a plugin prepared for itself.
pub(crate) const PLACE_READER: u32 = 0;
pub(crate) const PLACE_COMIC: u32 = 1;
pub(crate) const PLACE_COVER: u32 = 2;
pub(crate) const PLACE_THUMBNAIL: u32 = 3;
pub(crate) const PLACE_SLEEP: u32 = 4;
pub(crate) const PLACE_UI: u32 = 5;
/// `picture.ask.flags`: a colour source, one with transparency.
const COLOUR: u32 = 1;
const SEE_THROUGH: u32 = 2;
/// `picture.tone.flags`: dots, grays (`canvas.draw_image` bits 3 and 4).
const TONE_DOTS: u32 = 1;
const TONE_GRAYS: u32 = 2;

/// A prepared picture the stage is to be asked for.
pub(crate) struct Wanted {
    pub(crate) owner: u64,
    pub(crate) id: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// Its cache file.
    pub(crate) path: String,
    /// A document's picture: the document (0: none, a cover the library
    /// made) and the format's number for it (`u32::MAX`: the cover).
    pub(crate) document: Option<(u32, u32)>,
    /// A part of its file (`fmt.image.part`).
    pub(crate) part: bool,
    /// It has been drawn already (its id was made and drawn in one frame).
    pub(crate) drawn: bool,
}

/// What the host knows of where a picture is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Shown {
    /// Its document's format reads whole pages (a comic, a folder of pictures).
    pub(crate) whole_pages: bool,
    /// Its plugin's screen is the sleep screen, on top now.
    pub(crate) sleeping: bool,
    /// The screen's long side in pixels.
    pub(crate) long_side: u32,
}

impl Wanted {
    /// `picture.ask.place` (`docs/abi/document.md` 5): a picture of a book's
    /// page first, then whose screen it is, then how small it is.
    pub(crate) fn place(&self, shown: Shown) -> u32 {
        match self.document {
            Some((document, index)) if document != super::LIBRARY_COVER && index != u32::MAX => {
                if shown.whole_pages {
                    PLACE_COMIC
                } else {
                    PLACE_READER
                }
            }
            _ if shown.sleeping => PLACE_SLEEP,
            _ if self.width.max(self.height).saturating_mul(4) <= shown.long_side => {
                PLACE_THUMBNAIL
            }
            Some(_) => PLACE_COVER,
            None => PLACE_UI,
        }
    }
}

impl ImageTable {
    /// Whether a picture may still wait for the stage: one flag, so a pass
    /// with nothing to ask costs nothing.
    pub(crate) fn tone_pending(&self) -> bool {
        self.unasked.get()
    }

    /// Whether the stage is known to have no subscriber in the declarations
    /// of `revision` ([`ImageTable::tones_unsubscribed`]).
    pub(crate) fn tones_unsubscribed_at(&self, revision: u64) -> bool {
        self.unsubscribed.get() == Some(revision)
    }

    /// The stage has no subscriber in the declarations of `revision`.
    pub(crate) fn tones_unsubscribed(&self, revision: u64) {
        self.unsubscribed.set(Some(revision));
    }

    /// The next picture the stage has not been asked for (a plugin's own, a
    /// document's, a cover); it counts as asked from now on, whatever comes
    /// of the call. `None` when there is none left (or the table is borrowed:
    /// asked at a later pass).
    pub(crate) fn tone_wanted(&self) -> Option<Wanted> {
        let mut inner = self.inner.try_borrow_mut().ok()?;
        loop {
            let Some(e) = inner.entries.iter_mut().find(|e| e.unasked) else {
                self.unasked.set(false);
                return None;
            };
            e.unasked = false;
            let (path, document) = match e.origin {
                Origin::Prepared => (prepare::file_name(e.owner, e.seq).ok(), None),
                Origin::Document { document, index } => {
                    let mut path = String::new();
                    let room = path.try_reserve_exact(e.path.len()).is_ok();
                    path.push_str(if room { &e.path } else { "" });
                    (room.then_some(path), Some((document, index)))
                }
            };
            // No room for its name: this picture is drawn as it is.
            let Some(path) = path else {
                continue;
            };
            return Some(Wanted {
                owner: e.owner,
                id: e.id,
                width: e.width,
                height: e.height,
                path,
                document,
                part: e.part.is_some(),
                drawn: e.drawn != 0,
            });
        }
    }

    /// A part of a file (a panel of a comic page) takes what another picture
    /// of the same file of its owner was answered, so the panels of a page
    /// look alike and nothing is read or asked for each. `None`: there is no
    /// such picture, the stage is asked; else whether that answer says
    /// something.
    pub(crate) fn tone_shared(&self, wanted: &Wanted) -> Option<bool> {
        if !wanted.part {
            return None;
        }
        let mut inner = self.inner.try_borrow_mut().ok()?;
        let (tone, forced) = {
            let like = inner.entries.iter().find(|e| {
                e.owner == wanted.owner
                    && e.id != wanted.id
                    && !e.unasked
                    && e.origin != Origin::Prepared
                    && e.path == wanted.path
            })?;
            let tone = match &like.tone {
                Some(curve) => Some(curve_of(&curve[..])?),
                None => None,
            };
            (tone, like.forced)
        };
        let says = tone.is_some() || forced != 0;
        let e = inner
            .entries
            .iter_mut()
            .find(|e| e.id == wanted.id && e.owner == wanted.owner)?;
        e.tone = tone;
        e.forced = forced;
        if says {
            e.kept = None;
        }
        Some(says)
    }

    /// No plugin subscribes to the stage: nothing waits for it.
    pub(crate) fn tones_skip(&self) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            for e in &mut inner.entries {
                e.unasked = false;
            }
            self.unasked.set(false);
        }
    }

    /// The stage's answer for the image `id` of `owner`: `tone` is empty or
    /// 256 bytes, `flags` bit 0 dots or bit 1 grays. `false` for an answer
    /// that is neither (nothing is kept), or with no room for the curve. A
    /// curve that changes no gray is kept as none: it takes no memory.
    pub(crate) fn tone_answer(&self, owner: u64, id: u32, tone: &[u8], flags: u32) -> bool {
        let forced = match flags {
            0 => 0,
            TONE_DOTS => DOTS,
            TONE_GRAYS => GRAYS,
            _ => return false,
        };
        let curve = match tone.len() {
            0 => None,
            TONE_BYTES if tone.iter().enumerate().all(|(v, &to)| v == usize::from(to)) => None,
            // Room taken fallibly: without it the picture stays as it is.
            TONE_BYTES => match curve_of(tone) {
                Some(boxed) => Some(boxed),
                None => return false,
            },
            _ => return false,
        };
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            return false;
        };
        let Some(e) = inner
            .entries
            .iter_mut()
            .find(|e| e.id == id && e.owner == owner)
        else {
            return false;
        };
        e.tone = curve;
        e.forced = forced;
        // Rows kept of a draw before the answer are not this picture's.
        e.kept = None;
        true
    }

    /// The curve of an image, taken out for a draw ([`ImageTable::give_tone`]
    /// puts it back).
    pub(super) fn take_tone(&self, owner: u64, id: u32) -> Option<Box<[u8; TONE_BYTES]>> {
        self.inner
            .try_borrow_mut()
            .ok()?
            .entries
            .iter_mut()
            .find(|e| e.id == id && e.owner == owner)?
            .tone
            .take()
    }

    /// The curve back with its image after a draw.
    pub(super) fn give_tone(&self, owner: u64, id: u32, tone: Option<Box<[u8; TONE_BYTES]>>) {
        if tone.is_some()
            && let Ok(mut inner) = self.inner.try_borrow_mut()
            && let Some(e) = inner
                .entries
                .iter_mut()
                .find(|e| e.id == id && e.owner == owner)
        {
            e.tone = tone;
        }
    }
}

/// A copy of the curve `tone` (256 bytes), its room taken fallibly.
fn curve_of(tone: &[u8]) -> Option<Box<[u8; TONE_BYTES]>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(tone.len()).ok()?;
    copy.extend_from_slice(tone);
    Box::<[u8; TONE_BYTES]>::try_from(copy.into_boxed_slice()).ok()
}

/// The histogram of the decoded picture at `path` and its `picture.ask`
/// flags: 16 gray bins (colour as luma) counted over up to 16 rows spread
/// over the picture, each bin 0 to 255 of the largest. One row is held at a
/// time. `None` where the file cannot be read or there is no room for a row.
pub(crate) fn histogram(storage: &dyn Storage, path: &str) -> Option<([u8; BINS], u32)> {
    use crate::kit::ui::PictureKind;
    let (kind, width, height) = crate::kit::ui::probe_picture(storage, path).ok()?;
    let (channels, flags) = match kind {
        PictureKind::Color => (3, COLOUR),
        PictureKind::Alpha => (2, SEE_THROUGH),
        _ => (1, 0),
    };
    let row_bytes = (width as usize).checked_mul(channels)?;
    let mut row = Vec::new();
    row.try_reserve_exact(row_bytes).ok()?;
    row.resize(row_bytes, 0);
    let mut bins = [0_u32; BINS];
    let rows = height.min(SAMPLED_ROWS);
    for i in 0..rows {
        // The middle row of each of `rows` equal bands.
        let y = u64::from(2 * i + 1) * u64::from(height) / u64::from(2 * rows);
        let at = 12 + y * row_bytes as u64;
        if storage.read_at(path, at, &mut row).ok() != Some(row_bytes) {
            return None;
        }
        for pixel in row.chunks_exact(channels) {
            let gray = if channels == 3 {
                paperpaw_sdk::image::luma(pixel[0].into(), pixel[1].into(), pixel[2].into())
            } else {
                pixel[0]
            };
            bins[usize::from(gray >> 4)] += 1;
        }
    }
    let most = u64::from(bins.iter().copied().max().unwrap_or(0).max(1));
    let mut out = [0_u8; BINS];
    for (out, bin) in out.iter_mut().zip(bins) {
        *out = (u64::from(bin) * 255 / most) as u8;
    }
    Some((out, flags))
}

#[cfg(test)]
#[path = "tone_tests.rs"]
mod tests;
