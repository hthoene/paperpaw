//! A taller band for a frame of many bands (#267): each band costs the screens
//! a pass over what they show, so a frame drawn in half the passes is drawn
//! with fewer host calls (Home on the emulator's X4: 349 -> about 210 slots).
//! The band is made taller only for the frame, where the heap has the room for
//! it and for a picture's block after it, and made as it was when the frame is
//! drawn. The shell is passed the rows in the pieces its own bands have.

use paperpaw_platform::FrameView;

use crate::{Kernel, Rect};

/// The heap that must stay as one block after the taller band is taken: a
/// picture's transient block (32 KiB on the X4) and some.
const SPARE_AFTER: usize = 36 * 1024;

impl Kernel {
    /// Makes the band half the screen where the frame to draw (`region`) has
    /// more than two bands of today's rows and the heap's largest block holds
    /// it and [`SPARE_AFTER`]: the rows to go back to (`None` where nothing
    /// changed: a room's leased band, a frame in one band, little heap) and
    /// the rows of the band now.
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
        let bytes = stride.checked_mul(usize::from(half))?;
        let largest = usize::try_from(self.shared.memory.read()?.largest_free?).ok()?;
        if largest < bytes.checked_add(SPARE_AFTER)? {
            return None;
        }
        self.framebuffer.set_rows(half).ok()?;
        Some(rows)
    }

    /// The frame ends, and the band is as it was before [`Kernel::widen_band`]
    /// (where there is no room to shrink it, it stays).
    pub(crate) fn end_frame_narrow(&mut self, rows: Option<u16>) {
        self.framebuffer.end_frame();
        if let Some(rows) = rows {
            let _ = self.framebuffer.set_rows(rows);
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
