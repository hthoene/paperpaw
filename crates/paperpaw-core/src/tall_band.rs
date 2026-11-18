//! A taller band for a frame of many bands (#267): each band costs the screens
//! a pass over what they show, so a frame drawn in fewer passes is drawn with
//! fewer host calls (Home on the emulator's X4: 349 -> about 210 slots). The
//! band is made taller only for the frame, where the heap has the room for it
//! and for a picture's block after it, and made as it was when the frame is
//! drawn. The shell is passed the rows in the pieces its own bands have.
//!
//! The band is half the screen, or as tall as the heap allows: a block a
//! little short of the room for half the screen still saves passes (#308: on
//! the X4 a largest block of 52 KB, 8 KB short, drew every frame in the usual
//! bands).
//!
//! The taller band's block is given back before the usual band is asked for
//! again (#308). The X4's heap takes the first block that fits, lowest first,
//! and moves a block to make it smaller: made smaller in one step, the usual
//! band was asked for while the taller one still lay at the start of the
//! largest block, came to lie right behind it where its old place was taken
//! meanwhile, and the 24 KB the taller one then gave back were a hole under
//! the band: 35 KB of the largest block gone for the run, and with them the
//! room for any taller band.

use paperpaw_platform::FrameView;

use crate::{Kernel, Rect};

/// The heap that must stay as one block after the taller band is taken: a
/// picture's transient block (32 KiB on the X4) and some.
const SPARE_AFTER: usize = 36 * 1024;

/// How many bands of `rows` rows the rows `first..last` of the screen lie in.
fn passes(first: u32, last: u32, rows: u32) -> u32 {
    let rows = rows.max(1);
    last.div_ceil(rows).saturating_sub(first / rows)
}

impl Kernel {
    /// Makes the band taller where the frame to draw (`region`) has more than
    /// two bands of today's rows and the heap's largest block holds a taller
    /// one and [`SPARE_AFTER`]: half the screen, or as many rows as the block
    /// allows where they still draw the frame in fewer passes. The rows to go
    /// back to (`None` where nothing changed: a room's leased band, a frame in
    /// one band, little heap) and the rows of the band now.
    pub(crate) fn widen_band(&mut self, region: Rect) -> (Option<u16>, u16) {
        let rows = self.framebuffer.rows();
        (self.widened(region, rows), self.framebuffer.rows())
    }

    fn widened(&mut self, region: Rect, rows: u16) -> Option<u16> {
        if self.lease.is_some() {
            return None;
        }
        let height = self.framebuffer.height();
        let align = self.frame.row_align.max(1);
        if rows >= height || u32::from(region.height) <= u32::from(rows) * 2 {
            return None;
        }
        let half = height.div_ceil(2).div_ceil(align).checked_mul(align)?;
        if half <= rows || half >= height {
            return None;
        }
        let stride = self
            .framebuffer
            .format()
            .stride(self.framebuffer.bounds().width);
        let largest = usize::try_from(self.shared.memory.read()?.largest_free?).ok()?;
        let room = largest.checked_sub(SPARE_AFTER)? / stride.max(1);
        let most = u16::try_from(room).unwrap_or(u16::MAX).min(half) / align * align;
        // The rows of the screen the frame is drawn in (`Kernel::render`).
        let first = u32::from(region.y - region.y % align);
        let last = region
            .bottom()
            .div_ceil(u32::from(align))
            .saturating_mul(u32::from(align))
            .min(u32::from(height));
        // The tallest of those with the fewest passes: half the screen
        // where the heap has the room, as a frame's bands were before.
        let usual = passes(first, last, u32::from(rows));
        let mut best = (usual, rows);
        let mut taller = rows.checked_add(align)?;
        while taller <= most {
            let count = passes(first, last, u32::from(taller));
            if count < usual && count <= best.0 {
                best = (count, taller);
            }
            taller = taller.checked_add(align)?;
        }
        if best.1 == rows {
            return None;
        }
        self.framebuffer.set_rows(best.1).ok()?;
        Some(rows)
    }

    /// The frame ends, and the band is as it was before [`Kernel::widen_band`]
    /// (where there is no room for it, it has fewer rows). The taller band's
    /// block goes first, the band down to its fewest rows, and only then is
    /// the usual band asked for: it takes the lowest block that fits with the
    /// taller one's free again, not a place behind it (see the module).
    pub(crate) fn end_frame_narrow(&mut self, rows: Option<u16>) {
        self.framebuffer.end_frame();
        if let Some(rows) = rows {
            let align = self.frame.row_align.max(1);
            let _ = self.framebuffer.set_rows(align);
            let mut rows = rows;
            while self.framebuffer.set_rows(rows).is_err() && rows > align {
                rows = (rows / 2 / align * align).max(align);
            }
        }
    }

    /// Passes a band's `part` to `out` in as many pieces as it has of the
    /// `old` rows (`widen_band`'s answer), evenly, each a multiple of the
    /// alignment; whole where the band was not made taller.
    pub(crate) fn pass_pieces(
        &self,
        part: Rect,
        old: Option<u16>,
        out: &mut dyn FnMut(u16, FrameView<'_>),
    ) {
        let align = self.frame.row_align.max(1);
        let count = old.map_or(1, |old| part.height.div_ceil(old.max(1)).max(1));
        let each = part.height.div_ceil(count).div_ceil(align) * align;
        let bottom = u32::from(part.y) + u32::from(part.height);
        for i in 0..count {
            let y = u32::from(part.y) + u32::from(i) * u32::from(each);
            let height = u32::from(each).min(bottom.saturating_sub(y));
            if height > 0 {
                let piece = Rect::new(part.x, y as u16, part.width, height as u16);
                out(piece.y, self.framebuffer.view_rows(piece));
            }
        }
    }
}
