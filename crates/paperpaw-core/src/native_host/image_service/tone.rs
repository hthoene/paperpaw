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
/// `picture.ask.place`: a picture a plugin prepared for itself.
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
}

impl ImageTable {
    /// Whether a prepared picture may still wait for the stage: one flag, so
    /// a pass with nothing to ask costs nothing.
    pub(crate) fn tone_pending(&self) -> bool {
        self.unasked.get()
    }

    /// The next prepared picture the stage has not been asked for; it counts
    /// as asked from now on, whatever comes of the call. `None` when there is
    /// none left (or the table is borrowed: asked at a later pass).
    pub(crate) fn tone_wanted(&self) -> Option<Wanted> {
        let mut inner = self.inner.try_borrow_mut().ok()?;
        let Some(e) = inner.entries.iter_mut().find(|e| e.unasked) else {
            self.unasked.set(false);
            return None;
        };
        e.unasked = false;
        if e.origin != Origin::Prepared {
            return None;
        }
        Some(Wanted {
            owner: e.owner,
            id: e.id,
            width: e.width,
            height: e.height,
            path: prepare::file_name(e.owner, e.seq).ok()?,
        })
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
    /// that is neither (nothing is kept), or with no room for the curve.
    pub(crate) fn tone_answer(&self, owner: u64, id: u32, tone: &[u8], flags: u32) -> bool {
        let forced = match flags {
            0 => 0,
            TONE_DOTS => DOTS,
            TONE_GRAYS => GRAYS,
            _ => return false,
        };
        let curve = match tone.len() {
            0 => None,
            TONE_BYTES => {
                // Room taken fallibly: without it the picture stays as it is.
                let mut copy = Vec::new();
                if copy.try_reserve_exact(TONE_BYTES).is_err() {
                    return false;
                }
                copy.extend_from_slice(tone);
                match Box::<[u8; TONE_BYTES]>::try_from(copy.into_boxed_slice()) {
                    Ok(boxed) => Some(boxed),
                    Err(_) => return false,
                }
            }
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
