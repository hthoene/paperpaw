//! The bookmark ribbon and the corner that sets it, as Beta 3 had them: a page
//! that has a bookmark shows a black ribbon with a notch at the top right of
//! the text, and with `reader.corner-bookmark` on a tap on the top right
//! corner sets or removes the page's bookmark.
//!
//! The starts of the book's bookmarks are read from the library once, when
//! the book opens (8 bytes each, at most [`MAX_MARKS`]), and follow the
//! toggles made here; the page asks no service as it turns.
use super::{Body, Error, Handled, Host, Input, Library, Open, Setup, View, flag};
use crate::native_bookmark::{self, Done, Range};
use alloc::vec::Vec;
use paperpaw_sdk::Color;
use paperpaw_sdk::native::{
    callbacks::{FrameRect, InputGestureKind},
    canvas::Canvas,
    library::{AnnotationCursor, AnnotationKind, BookRef},
};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};

/// The most bookmark starts kept for one book; a book with more shows the
/// ribbon on the first of them only.
pub const MAX_MARKS: usize = 256;

/// The ribbon's width and the corner's size, in tenths of a millimetre.
const RIBBON_WIDTH: u32 = 30;
const CORNER: u32 = 120;

/// Pixels of `tenths` tenths of a millimetre at `dpi`.
fn pixels(tenths: u32, dpi: u16) -> u32 {
    (tenths * u32::from(dpi) + 127) / 254
}

/// The ribbon over a content box whose top is `top` and right edge `right`,
/// in the region's pixels: `(x, y, width, height)`.
#[must_use]
pub fn ribbon_box(right: u32, top: u32, dpi: u16) -> (u32, u32, u32, u32) {
    let width = pixels(RIBBON_WIDTH, dpi).max(2);
    // Four fifths of the room above the text, at least as long as wide.
    let height = (top * 4 / 5).max(width);
    (right.saturating_sub(width), 0, width, height)
}

/// Whether `point` is in the corner of a screen `width` pixels wide where a tap
/// sets the bookmark (12 mm, at most a quarter of the width).
#[must_use]
pub fn in_corner((x, y): (i32, i32), width: u32, dpi: u16) -> bool {
    let size = pixels(CORNER, dpi).min(width / 4);
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return false;
    };
    x >= width.saturating_sub(size) && x < width && y < size
}

impl Open {
    /// Reads the starts of the book's bookmarks from the library (once, at the
    /// opening): a book the library cannot list shows no ribbon.
    pub(super) fn load_bookmarks(&mut self, host: &Host<'_>, library: Option<&Library>) {
        self.bookmarks = library
            .and_then(|library| read_marks(host, library, &self.path, self.conversion).ok())
            .unwrap_or_default();
    }

    /// Whether the page on show has a bookmark.
    pub(super) fn bookmarked(&self) -> bool {
        let Some(page) = &self.page else {
            return false;
        };
        let range = Range {
            start: (page.start.section, page.start.offset),
            end: (page.end.section, page.end.offset),
        };
        self.bookmarks.iter().any(|at| range.holds(*at))
    }

    /// The bookmarks after a toggle of the page `range` did `done`.
    pub(super) fn bookmarks_toggled(&mut self, range: Range, done: Done) {
        match done {
            Done::Removed => self.bookmarks.retain(|at| !range.holds(*at)),
            Done::Added if self.bookmarks.len() < MAX_MARKS => {
                if self.bookmarks.try_reserve(1).is_ok() {
                    self.bookmarks.push(range.start);
                }
            }
            Done::Added => {}
        }
    }

    /// Draws the ribbon where the page has a bookmark.
    pub(super) fn draw_ribbon(
        &self,
        canvas: &Canvas<'_>,
        rect: FrameRect,
        (area, dpi): (&crate::native_pages::Box, u16),
    ) -> Result<(), Error> {
        if !self.bookmarked() {
            return Ok(());
        }
        let right = u32::from(area.x) + u32::from(area.width);
        let (x, y, width, height) = ribbon_box(right, u32::from(area.y), dpi);
        let at = |dx: u32, dy: u32| {
            (
                rect.x
                    .saturating_add(i32::try_from(x + dx).unwrap_or(i32::MAX)),
                rect.y
                    .saturating_add(i32::try_from(y + dy).unwrap_or(i32::MAX)),
            )
        };
        let notch = width / 2;
        let body = height.saturating_sub(notch);
        let (bx, by) = at(0, 0);
        canvas.fill_rectangle(FrameRect::new(bx, by, width, body), Color::BLACK)?;
        // Both sides of the notch narrow towards the bottom.
        for row in 0..notch {
            let side = notch - row;
            let (lx, ly) = at(0, body + row);
            canvas.fill_rectangle(FrameRect::new(lx, ly, side, 1), Color::BLACK)?;
            let (rx, ry) = at(width - side, body + row);
            canvas.fill_rectangle(FrameRect::new(rx, ry, side, 1), Color::BLACK)?;
        }
        Ok(())
    }
}

impl View {
    /// A tap on the top right corner, where `reader.corner-bookmark` is on: the
    /// page's bookmark is set or removed. `None` for any other input.
    pub(super) fn corner_tap(
        &mut self,
        host: &Host<'_>,
        library: Option<&Library>,
        input: &Input<'_>,
    ) -> Result<Option<Handled>, Error> {
        let Input::Gesture {
            position,
            kind: InputGestureKind::Tap,
            ..
        } = *input
        else {
            return Ok(None);
        };
        if !matches!(self.body, Body::Open(_)) || !flag(host, "reader.corner-bookmark")? {
            return Ok(None);
        }
        let setup = Setup::read(host)?;
        let display = &setup.display;
        if !in_corner(
            (position.x, position.y),
            u32::from(display.display_width()),
            display.display_dpi(),
        ) {
            return Ok(None);
        }
        self.toggle_bookmark(host, library)?;
        Ok(Some(Handled::Yes))
    }
}

/// The starts of the bookmarks of the book at `path` in the conversion
/// `conversion` (or of none), at most [`MAX_MARKS`].
fn read_marks(
    host: &Host<'_>,
    library: &Library,
    path: &str,
    conversion: u64,
) -> Result<Vec<(u32, u32)>, Error> {
    let book = BookRef::path(path)?;
    let (mut input, mut out) = (buffer(1024)?, buffer(SERVICE_BUFFER)?);
    let mut marks = Vec::new();
    let mut cursor = AnnotationCursor::start();
    let mut seen = 0usize;
    loop {
        let page = library.annotations(host, book, cursor, &mut input, &mut out)?;
        for row in page.rows() {
            seen += 1;
            let row = row?;
            let mine = row.conversion().is_none_or(|c| c == conversion);
            if row.kind() == AnnotationKind::Bookmark && mine && marks.len() < MAX_MARKS {
                marks.try_reserve(1).map_err(|_| Error::NoMemory)?;
                marks.push((row.start_section(), row.start_offset()));
            }
        }
        match page.end().next() {
            Some(next) if seen < native_bookmark::MAX_ROWS => cursor = next,
            _ => return Ok(marks),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ribbon_hangs_from_the_top_right_of_the_text() {
        // 3 mm at 254 dpi is 30 pixels; four fifths of 100 pixels above.
        assert_eq!(ribbon_box(500, 100, 254), (470, 0, 30, 80));
        // No room above the text: as long as it is wide.
        assert_eq!(ribbon_box(500, 0, 254), (470, 0, 30, 30));
        // Never left of the screen.
        assert_eq!(ribbon_box(10, 0, 254), (0, 0, 30, 30));
    }

    #[test]
    fn the_corner_is_12_mm_at_most_a_quarter_of_the_width() {
        // 12 mm at 254 dpi: 120 pixels.
        assert!(in_corner((480, 0), 600, 254));
        assert!(in_corner((599, 119), 600, 254));
        assert!(!in_corner((479, 0), 600, 254));
        assert!(!in_corner((599, 120), 600, 254));
        assert!(!in_corner((600, 0), 600, 254));
        assert!(!in_corner((-1, 0), 600, 254));
        // A narrow screen: a quarter of it.
        assert!(in_corner((300, 0), 400, 254));
        assert!(!in_corner((299, 0), 400, 254));
    }
}
