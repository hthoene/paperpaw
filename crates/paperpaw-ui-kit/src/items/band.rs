//! What the bands of one frame share (#333). A frame of the X4 is drawn in
//! bands, and each band called for the list in full: the look read from the
//! settings (a dozen host calls), the note under the rows wrapped, the header
//! laid out. They are the same in every band of a frame, so the first band
//! makes them, keeps them for the next band of the same frame and the last
//! band lets them go: nothing stays on the heap between frames. Kept behind
//! one pointer with the heights ([`Kept`]), as a field of the list's own grew
//! the frame of every plugin that carries one (#288).
use super::*;
use crate::cards::Cards;
use paperpaw_sdk::native::callbacks::FrameRect;

/// The look, the note and the header of a frame, from its first band.
pub(super) struct Band {
    /// The frame the band is made for, and the row its next band begins at.
    frame: (FrameRect, u32),
    next: i32,
    pub(super) style: Style,
    pub(super) cards: Option<Cards>,
    pub(super) note: Option<(Rect, Vec<String>)>,
    pub(super) header: Option<Page>,
    /// Whether the band is the first of a frame or follows one that kept it:
    /// only then may the next band of the frame use it.
    chained: bool,
}

impl Band {
    /// Reads the look and wraps the note for the frame (nothing is kept
    /// across frames: its first band always starts here).
    pub(super) fn read(
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        (display, font): (&snapshot::Snapshot, &Font<'_>),
        footer: Footer<'_>,
    ) -> Result<alloc::boxed::Box<Self>, Error> {
        let cards = Cards::read(host, display)?;
        let mut style = Style::read(host, display, font)?;
        style.describing = paint::asking(frame);
        let note = match footer {
            Footer::Note(text) => {
                let (width, height) = (display.display_width(), display.display_height());
                Some(super::note_block(font, &style, width, height, text)?)
            }
            _ => None,
        };
        crate::mem::boxed(Self {
            frame: (frame.rect, frame.id),
            next: 0,
            style,
            cards,
            note,
            header: None,
            chained: frame.clip_top == frame.rect.y,
        })
    }

    /// Lays out the header (the frame's first band only).
    pub(super) fn prepare_header(
        &mut self,
        host: &Host<'_>,
        font: &Font<'_>,
        title: &str,
        (display, page, pages): (&snapshot::Snapshot, usize, usize),
    ) -> Result<(), Error> {
        if self.header.is_some() {
            return Ok(());
        }
        let mut header = Page::default();
        let mut messages = Messages::new()?;
        let title = translated(&mut messages, host, title)?;
        header::prepare(
            &mut header,
            host,
            font,
            &self.style,
            &mut messages,
            &title,
            display.display_width(),
            display.has_touch(),
            page,
            pages,
        )?;
        // The header was the last to need the catalog: its buffers (4 KiB) go
        // (`messages`, here) before the rows are painted, the peak of drawing a
        // list (#248).
        self.header = Some(header);
        Ok(())
    }

    /// Whether this is the last band of the frame's pane.
    fn last(frame: &DrawFrame<'_>) -> bool {
        i64::from(frame.clip_bottom) >= i64::from(frame.rect.y) + i64::from(frame.rect.height)
    }

    /// Whether the band goes on for the next band of its frame.
    pub(super) fn goes_on(&mut self, frame: &DrawFrame<'_>) -> bool {
        self.next = frame.clip_bottom;
        self.chained && !Self::last(frame)
    }

    /// Whether the band was made for the band `frame` is.
    fn fits(&self, frame: &DrawFrame<'_>) -> bool {
        self.frame == (frame.rect, frame.id) && self.next == frame.clip_top
    }
}

/// What a list keeps between its draws: the heights of its items, and the
/// [`Band`] of a frame under way.
#[derive(Debug, Default)]
pub(super) struct Kept {
    /// What `heights` were measured under ([`measured_under`]).
    pub(super) under: u32,
    /// The heights of the items (empty while taken).
    pub(super) heights: Vec<u16>,
    band: Option<alloc::boxed::Box<Band>>,
}

impl Kept {
    pub(super) const fn new(heights: Vec<u16>, under: u32) -> Self {
        Self {
            under,
            heights,
            band: None,
        }
    }
}

impl core::fmt::Debug for Band {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Band")
    }
}

impl ItemList {
    /// The band the frame's previous band kept, where `frame` goes on from it.
    #[inline(never)]
    pub(super) fn take_band(&mut self, frame: &DrawFrame<'_>) -> Option<alloc::boxed::Box<Band>> {
        let band = self.kept.as_mut()?.band.take()?;
        band.fits(frame).then_some(band)
    }

    /// Keeps the band for the frame's next band.
    #[inline(never)]
    pub(super) fn keep_band(&mut self, band: alloc::boxed::Box<Band>) {
        if let Some(kept) = self.kept.as_mut() {
            kept.band = Some(band);
        }
    }
}
