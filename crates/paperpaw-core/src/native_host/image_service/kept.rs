//! The drawn rows of Home's small pictures, kept while Home is shown (#329).
//!
//! A draw reads a picture's whole decoded file from the card, scales it and
//! dithers it: a cover of 192 x 300 is 57 KB read, some 100 ms on the X4, at
//! every frame that draws its rows again. A focus move on Home drew the
//! covers of the card and the shelf anew each time (173 KB read for three),
//! though nothing of them had changed.
//!
//! So what a draw put into a black and white frame is kept, a bit a pixel (a
//! cover of 122 x 190: 3 KB), and the same draw again takes its rows from
//! there: no card, no scaling, no dithering, the same pixels.
//!
//! * Only small pictures ([`ONE`] bytes each, [`ALL`] together), only opaque
//!   ones in a frame of black and white, and only while the home screen is on
//!   top with nothing over it ([`super::ImageTable::tend_kept`]): a book, a
//!   list or a menu opened ends them all, before the screen that comes takes
//!   its memory.
//! * A draw only *wishes* for the room. It is taken between frames (the
//!   picture pump), never inside one: a block taken while a frame holds its
//!   taller band would come to lie behind it and leave a hole under it when
//!   the band goes (#308). The draw after that fills the rows as it draws
//!   them, band by band, and the ones after it read nothing.
//! * The rows are those of one way to draw the picture ([`Shape`]): another
//!   size, part, flag or look is another wish, and the old rows go.

use alloc::vec::Vec;

use super::Place;
use crate::kit::ui::picture_look::Steps;

/// The most bytes of one kept picture: 180 x 180 pixels or so.
pub(super) const ONE: usize = 4 * 1024;
/// The most bytes of all kept pictures.
pub(super) const ALL: usize = 12 * 1024;

/// What kept rows are the rows of: the picture as one draw places it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Shape {
    /// The size the picture is scaled to.
    placed: (u16, u16),
    /// The part of the scaled picture the draw's rectangle shows: its left
    /// column and top row in the scaled picture, its width and height.
    part: (u16, u16, u16, u16),
    /// The draw's flags (how it is placed, its tones).
    flags: u32,
    /// The picture look it is drawn with.
    look: Steps,
}

impl Shape {
    /// The shape of a draw into `(x, y, width, height)` that places the
    /// picture at `place`, and where its part lies on the canvas (left, top).
    /// `None` where nothing of the picture is in the rectangle or the part is
    /// more than [`ONE`] bytes.
    pub(super) fn of(
        place: &Place,
        (x, y, width, height): (i32, i32, u32, u32),
        flags: u32,
        look: Steps,
    ) -> Option<(Self, i32, i32)> {
        let (left, top) = (i64::from(place.left), i64::from(place.top));
        let from = (left.max(i64::from(x)), top.max(i64::from(y)));
        let to = (
            (left + i64::from(place.width)).min(i64::from(x) + i64::from(width)),
            (top + i64::from(place.height)).min(i64::from(y) + i64::from(height)),
        );
        let part = (
            u16::try_from(from.0 - left).ok()?,
            u16::try_from(from.1 - top).ok()?,
            u16::try_from(to.0 - from.0).ok().filter(|w| *w > 0)?,
            u16::try_from(to.1 - from.1).ok().filter(|h| *h > 0)?,
        );
        if usize::from(part.2).div_ceil(8) * usize::from(part.3) > ONE {
            return None;
        }
        let shape = Self {
            placed: (place.width, place.height),
            part,
            flags,
            look,
        };
        Some((
            shape,
            i32::try_from(from.0).ok()?,
            i32::try_from(from.1).ok()?,
        ))
    }

    /// The part's width and height.
    pub(super) const fn size(&self) -> (u16, u16) {
        (self.part.2, self.part.3)
    }
}

/// The kept rows of one picture, or the wish for them.
pub(super) struct Kept {
    shape: Shape,
    /// A bit a pixel (set: white), each row in whole bytes; empty until the
    /// room was taken ([`Kept::make_room`]).
    bits: Vec<u8>,
    /// The rows of the part that are kept: from this one, up to that one.
    rows: (u16, u16),
}

impl Kept {
    /// A wish for the rows of `shape`: no memory yet.
    pub(super) const fn wished(shape: Shape) -> Self {
        Self {
            shape,
            bits: Vec::new(),
            rows: (0, 0),
        }
    }

    /// Whether these are (to be) the rows of `shape`.
    pub(super) fn is(&self, shape: &Shape) -> bool {
        self.shape == *shape
    }

    fn stride(&self) -> usize {
        usize::from(self.shape.part.2).div_ceil(8)
    }

    /// The bytes it holds.
    pub(super) fn bytes(&self) -> usize {
        self.bits.len()
    }

    /// The bytes its rows take.
    pub(super) fn needs(&self) -> usize {
        self.stride() * usize::from(self.shape.part.3)
    }

    /// Whether it still waits for its room.
    pub(super) fn waits(&self) -> bool {
        self.bits.is_empty()
    }

    /// Takes the room for the rows; `false` where there is none now.
    pub(super) fn make_room(&mut self) -> bool {
        let needs = self.needs();
        if self.bits.try_reserve_exact(needs).is_err() {
            return false;
        }
        self.bits.resize(needs, 0);
        self.rows = (0, 0);
        true
    }

    /// A row a draw showed: `row` of the scaled picture, `grays` from its
    /// column `column` on. Kept where it holds the whole width of the part
    /// and goes on where the kept rows end (or lies in them); any other row
    /// begins the kept rows anew.
    pub(super) fn take(&mut self, row: usize, column: usize, grays: &[u8]) {
        let (left, top) = (
            usize::from(self.shape.part.0),
            usize::from(self.shape.part.1),
        );
        let (width, height) = (
            usize::from(self.shape.part.2),
            usize::from(self.shape.part.3),
        );
        if self.bits.is_empty()
            || row < top
            || row >= top + height
            || column > left
            || column + grays.len() < left + width
        {
            return;
        }
        let at = row - top;
        let (from, to) = (usize::from(self.rows.0), usize::from(self.rows.1));
        if at == to {
            self.rows.1 += 1;
        } else if at < from || at > to {
            self.rows = (at as u16, at as u16 + 1);
        }
        let stride = self.stride();
        let Some(line) = self.bits.get_mut(at * stride..(at + 1) * stride) else {
            return;
        };
        line.fill(0);
        for (x, gray) in grays[left - column..left - column + width]
            .iter()
            .enumerate()
        {
            if *gray >= 128 {
                line[x / 8] |= 0x80 >> (x % 8);
            }
        }
    }

    /// Whether the rows `from..to` of the part are kept.
    pub(super) fn has(&self, from: u16, to: u16) -> bool {
        !self.bits.is_empty() && from >= self.rows.0 && to <= self.rows.1
    }

    /// Row `row` of the part as grays (black 0, white 255) into `out`, which
    /// is as wide as the part.
    pub(super) fn row(&self, row: u16, out: &mut [u8]) {
        let stride = self.stride();
        let at = usize::from(row) * stride;
        let Some(line) = self.bits.get(at..at + stride) else {
            return;
        };
        for (x, gray) in out.iter_mut().enumerate() {
            *gray = if line[x / 8] & (0x80 >> (x % 8)) != 0 {
                255
            } else {
                0
            };
        }
    }
}
