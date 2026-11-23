//! Paints the list of books: a row of a book has its cover, title, author, a
//! bar of how far it is read and the badges plugins put on it; the book read
//! last stands on top as a larger card; a grid shows covers side by side.
//! The text and shapes are laid out first and replayed; the covers are drawn
//! by the host after them, into the places kept for them.
use crate::{
    native_covers::{Covers, Slot},
    native_list::{BookList, Cell, Metrics, Placed, Row, pages},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose},
        fonts::{Font, FontRole},
        image::ImageStyle,
        snapshot,
    },
};
use paperpaw_ui_kit::{
    Messages, Page, Paint, Style, choice_chip, header,
    list::{backdrop, columns, ink},
    mem::collect,
    paint::{self, frame_rect},
    text,
};

/// Asks the host for a book's cover while a page is drawn.
pub trait Covering {
    /// The state of the cover of the book at window index `at` after asking.
    fn ask(&mut self, at: usize) -> Slot;
}

/// What an empty list says: a title and a few lines.
#[derive(Clone, Copy, Debug, Default)]
pub struct Said<'a> {
    pub title: &'a str,
    pub text: &'a str,
}

/// The book's thumbnail rect inside a row `r`: the row less a gap, 3 to 4.
fn thumb(r: Rect, style: &Style) -> Rect {
    let high = r.height.saturating_sub(style.gap);
    let wide = u16::try_from(u32::from(high) * 3 / 4).unwrap_or(high);
    Rect::new(
        r.x.saturating_add(style.margin),
        r.y.saturating_add(style.gap / 2),
        wide,
        high,
    )
}

/// What stands where a book has no cover (yet): the one placeholder every
/// screen draws.
fn placeholder(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    at: Rect,
    title: &str,
    color: Color,
) -> Result<(), Error> {
    paperpaw_ui_kit::parts::placeholder(out, font, style, at, title, color)
}

/// A bar `percent` full inside `at`.
fn bar(
    out: &mut Vec<Paint>,
    style: &Style,
    at: Rect,
    percent: u8,
    color: Color,
) -> Result<(), Error> {
    paperpaw_ui_kit::parts::progress_bar(out, style, at, u16::from(percent.min(100)) * 10, color)
}

/// What a cell's drawing needs besides the cell.
struct Look<'a> {
    font: &'a Font<'a>,
    style: &'a Style,
    /// The height of a book's row with a line of title.
    base: u16,
}

impl Look<'_> {
    fn bar_height(&self) -> u16 {
        self.style.line.max(2).saturating_mul(2)
    }

    /// Title and value on one line, the note below, the bar at the bottom.
    fn words(
        &self,
        out: &mut Vec<Paint>,
        cell: &Cell,
        left: Rect,
        (focused, tall): (bool, bool),
    ) -> Result<(), Error> {
        let (font, style) = (self.font, self.style);
        let ink = ink(style, focused);
        let line = paperpaw_ui_kit::geometry::line_px(font, style.text)?;
        let small = paperpaw_ui_kit::geometry::line_px(font, style.small)?;
        let top = left.y.saturating_add(style.gap / 2);
        let (first, second) = if cell.card {
            // A word above the title: what the card is.
            let above = Rect::new(left.x, top, left.width, small);
            text::label(
                out,
                font,
                &cell.value,
                style.small,
                f32::from(left.x),
                above,
                if focused && !style.ring {
                    style.paper
                } else {
                    style.muted
                },
                Some(left),
            )?;
            let y = top.saturating_add(small);
            (
                Rect::new(left.x, y, left.width, line),
                Rect::new(left.x, y.saturating_add(line), left.width, small),
            )
        } else {
            // A title of two lines pushes the note down by one.
            let below = top.saturating_add(if tall { line.saturating_mul(2) } else { line });
            (
                Rect::new(left.x, top, left.width, line),
                Rect::new(left.x, below, left.width, small),
            )
        };
        if cell.card {
            text::label(
                out,
                font,
                &cell.title,
                style.text,
                f32::from(first.x),
                first,
                ink,
                Some(left),
            )?;
        } else {
            // The title and, at the right, how far it is read.
            let row = Rect::new(
                first.x.saturating_sub(style.margin),
                first.y,
                first.width.saturating_add(style.margin),
                first.height,
            );
            if tall {
                self.two_lines(out, cell, (row, left), ink)?;
            } else {
                columns(out, font, style, row, &cell.title, &cell.value, ink)?;
            }
        }
        let chips = self.chips(out, cell, left, second, ink)?;
        let room = Rect::new(
            second.x,
            second.y,
            second.width.saturating_sub(chips),
            second.height,
        );
        let color = if focused && !style.ring {
            style.paper
        } else {
            style.note
        };
        text::label(
            out,
            font,
            &cell.note,
            style.small,
            f32::from(room.x),
            room,
            color,
            Some(room),
        )
    }

    /// The cover and the room for the words of a book's row `r`. A row that
    /// took a second line of title keeps its cover's size, in the middle of it.
    fn parts(&self, r: Rect) -> (Rect, Rect) {
        let style = self.style;
        let usual = Rect::new(r.x, r.y, r.width, r.height.min(self.base));
        let mut cover = thumb(usual, style);
        cover.y = cover
            .y
            .saturating_add(r.height.saturating_sub(usual.height) / 2);
        let x = cover
            .x
            .saturating_add(cover.width)
            .saturating_add(style.gap);
        let left = Rect::new(
            x,
            r.y,
            r.x.saturating_add(r.width)
                .saturating_sub(x)
                .saturating_sub(style.margin),
            r.height,
        );
        (cover, left)
    }

    /// The filter row of a touch screen: the chip with the filter's name, and
    /// the lens. Where they are, for the taps.
    fn chip_row(
        &self,
        out: &mut Vec<Paint>,
        cell: &Cell,
        r: Rect,
        focused: bool,
        parts: &choice_chip::Parts,
    ) -> Result<(), Error> {
        backdrop(out, self.style, r, focused)?;
        choice_chip::draw(
            out,
            self.font,
            self.style,
            parts,
            &cell.note,
            ink(self.style, focused),
        )
    }

    /// How much of a row's first line the title may take: what `columns`
    /// leaves it beside the value.
    fn title_room(&self, wide: u16, value: &str) -> Result<f32, Error> {
        let style = self.style;
        let room = (f32::from(wide) - 3.0 * f32::from(style.margin)).max(0.0);
        let value = self.font.measure_text(style.text, value)?;
        Ok((room - value.min(room)).max(0.0))
    }

    /// Whether the title of `cell` needs more than one line in a row whose
    /// words are `wide` wide.
    fn wants_two(&self, cell: &Cell, wide: u16) -> Result<bool, Error> {
        let room = self.title_room(wide.saturating_add(self.style.margin), &cell.value)?;
        Ok(self.font.measure_text(self.style.text, &cell.title)? > room + 1.0)
    }

    /// A title on two lines, the value at the right of the first.
    fn two_lines(
        &self,
        out: &mut Vec<Paint>,
        cell: &Cell,
        (row, left): (Rect, Rect),
        ink: Color,
    ) -> Result<(), Error> {
        let (font, style) = (self.font, self.style);
        let room = self.title_room(row.width, &cell.value)?;
        let lines = text::wrap_with(|l| font.measure_text(style.text, l), &cell.title, room, 2)?;
        let line = paperpaw_ui_kit::geometry::line_px(font, style.text)?;
        let first = lines.first().map_or("", String::as_str);
        columns(out, font, style, row, first, &cell.value, ink)?;
        if let Some(more) = lines.get(1) {
            let at = Rect::new(left.x, row.y.saturating_add(line), left.width, line);
            text::label(
                out,
                font,
                more,
                style.text,
                f32::from(at.x),
                at,
                ink,
                Some(left),
            )?;
        }
        Ok(())
    }

    /// The badges as chips at the right of the note's line, as far as they
    /// fit; the width they take.
    fn chips(
        &self,
        out: &mut Vec<Paint>,
        cell: &Cell,
        left: Rect,
        second: Rect,
        ink: Color,
    ) -> Result<u16, Error> {
        let (font, style) = (self.font, self.style);
        let mut chips = 0u16;
        for chip in &cell.chips {
            let wide = font.measure_text(style.small, chip)?;
            let wide = u16::try_from(wide as u32 + 2 * u32::from(style.gap)).unwrap_or(u16::MAX);
            let used = chips.saturating_add(wide).saturating_add(style.gap);
            if used.saturating_add(left.width / 3) > left.width {
                break;
            }
            let at = Rect::new(
                left.x.saturating_add(left.width.saturating_sub(used)),
                second.y,
                wide,
                second.height,
            );
            paint::push(
                out,
                Paint::Focus(
                    at,
                    u32::from(at.height) / 2,
                    Some(u32::from(style.line).max(1)),
                    ink,
                ),
            )?;
            text::label(
                out,
                font,
                chip,
                style.small,
                f32::from(at.x) + f32::from(style.gap),
                at,
                ink,
                // The line of the text may be taller than the chip: the clip
                // keeps the sides of the chip but not its top and bottom.
                Some(Rect::new(
                    at.x,
                    at.y.saturating_sub(style.gap),
                    at.width,
                    at.height.saturating_add(2 * style.gap),
                )),
            )?;
            chips = used;
        }
        Ok(chips)
    }

    /// A book as a row of a list: cover, words, bar.
    fn book_row(
        &self,
        out: &mut Vec<Paint>,
        cell: &Cell,
        r: Rect,
        focused: bool,
    ) -> Result<Rect, Error> {
        let style = self.style;
        backdrop(out, style, r, focused)?;
        let tall = cell.two && r.height > self.base;
        let (cover, left) = self.parts(r);
        self.words(out, cell, left, (focused, tall))?;
        if let Some(percent) = cell.progress {
            let high = self.bar_height();
            let at = Rect::new(
                left.x,
                r.y.saturating_add(r.height)
                    .saturating_sub(style.gap / 2 + high + style.line),
                left.width,
                high,
            );
            bar(out, style, at, percent, ink(style, focused))?;
        }
        Ok(cover)
    }

    /// A book as a tile: the cover, a thin bar, the title below.
    fn tile(
        &self,
        out: &mut Vec<Paint>,
        cell: &Cell,
        r: Rect,
        cover_height: u16,
        focused: bool,
    ) -> Result<Rect, Error> {
        let (font, style) = (self.font, self.style);
        // A tile is no row: the theme's focus on the whole tile, and no
        // separator under the ones that are not chosen.
        if focused {
            paperpaw_ui_kit::focus::frame(out, style, r)?;
        }
        let inset = style.gap / 2;
        let cover = Rect::new(
            r.x.saturating_add(inset),
            r.y.saturating_add(inset),
            r.width.saturating_sub(2 * inset),
            cover_height.saturating_sub(inset),
        );
        if let Some(percent) = cell.progress {
            let high = self.bar_height() / 2;
            let at = Rect::new(
                cover.x,
                cover.y.saturating_add(cover.height).saturating_sub(high),
                cover.width,
                high.max(2),
            );
            bar(out, style, at, percent, ink(style, focused))?;
        }
        let label = Rect::new(
            r.x,
            r.y.saturating_add(cover_height),
            r.width,
            r.height.saturating_sub(cover_height),
        );
        text::label(
            out,
            font,
            &cell.title,
            style.small,
            f32::from(r.x) + f32::from(style.gap),
            label,
            ink(style, focused),
            Some(label),
        )?;
        Ok(cover)
    }
}

/// A title and a few lines, centred in `area`.
fn draw_empty(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    area: Rect,
    said: Said<'_>,
) -> Result<(), Error> {
    let room = f32::from(area.width.saturating_sub(4 * style.margin)).max(1.0);
    let lines = text::wrap_with(|l| font.measure_text(style.small, l), said.text, room, 6)?;
    // A filter's words stand alone and may be a sentence or two: wrapped, as
    // in Beta 3, not cut.
    let titles = text::wrap_with(|l| font.measure_text(style.title, l), said.title, room, 4)?;
    let head = paperpaw_ui_kit::geometry::line_px(font, style.title)?;
    let small = paperpaw_ui_kit::geometry::line_px(font, style.small)?;
    let total = u32::from(head) * titles.len() as u32
        + u32::from(style.gap)
        + u32::from(small) * lines.len() as u32;
    let total = u16::try_from(total).unwrap_or(u16::MAX);
    let mut y = area.y.saturating_add(area.height.saturating_sub(total) / 2);
    for title in &titles {
        let x = (f32::from(area.width) - font.measure_text(style.title, title)?).max(0.0) / 2.0;
        text::label(
            out,
            font,
            title,
            style.title,
            x,
            Rect::new(0, y, area.width, head),
            style.ink,
            None,
        )?;
        y = y.saturating_add(head);
    }
    y = y.saturating_add(style.gap);
    for line in &lines {
        let x = (f32::from(area.width) - font.measure_text(style.small, line)?).max(0.0) / 2.0;
        text::label(
            out,
            font,
            line,
            style.small,
            x,
            Rect::new(0, y, area.width, small),
            style.muted,
            None,
        )?;
        y = y.saturating_add(small);
    }
    Ok(())
}

impl BookList {
    /// The cells of `page` (placed by [`pages`]) whose title is wider than
    /// one line of the row: the books of a list, not the card.
    fn long_titles(
        &self,
        font: &Font<'_>,
        style: &Style,
        page: &[Placed],
        metrics: &Metrics,
    ) -> Result<Vec<usize>, Error> {
        let look = Look {
            font,
            style,
            base: metrics.book_height(),
        };
        let mut wide = Vec::new();
        for placed in page {
            let Some(cell) = self.cells().get(placed.cell) else {
                continue;
            };
            if !matches!(cell.row, Row::Book(_)) || cell.card {
                continue;
            }
            let (_, left) = look.parts(Rect::new(
                placed.rect.x,
                placed.rect.y,
                placed.rect.width,
                look.base,
            ));
            if look.wants_two(cell, left.width)? {
                paperpaw_ui_kit::mem::push(&mut wide, placed.cell)?;
            }
        }
        Ok(wide)
    }

    /// Draws the page the focus is on under `title`; asks `covering` for the
    /// covers of the books on it and has the host draw the ones that are made.
    ///
    /// # Errors
    /// Allocation, font and drawing errors; the geometry kept is that of the
    /// last frame that completed. A cover that cannot be had is a placeholder,
    /// not an error.
    #[allow(
        clippy::too_many_lines,
        reason = "one frame: layout, the page's covers, painting"
    )]
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        title: &str,
        covers: &mut Covers,
        covering: &mut dyn Covering,
        said: Said<'_>,
    ) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let top = style.header.saturating_add(style.gap);
        let area = Rect::new(0, top, width, height.saturating_sub(top));
        let metrics = Metrics {
            area,
            row: style.row,
            gap: style.gap,
            margin: style.margin,
            columns: if self.grid() {
                Metrics::columns_for(width)
            } else {
                1
            },
        };
        let mut laid = pages(self.cells(), &metrics, self.grid())?;
        let page = laid
            .iter()
            .rposition(|p| p.first().is_some_and(|first| first.cell <= self.focus()))
            .unwrap_or(0);
        if !self.grid() {
            // The titles of this page that need a second line ask for it; the
            // page gives it to as many as it has room for.
            let wide = self.long_titles(
                &font,
                &style,
                laid.get(page).map_or(&[][..], Vec::as_slice),
                &metrics,
            )?;
            self.set_two(&wide);
            laid = pages(self.cells(), &metrics, false)?;
        }
        let mut head = Page::default();
        let mut messages = Messages::new()?;
        let title = match messages.copy(host, title) {
            Err(Error::NotFound | Error::Invalid) => text::copy(title)?,
            other => other?,
        };
        header::prepare(
            &mut head,
            host,
            &font,
            &style,
            &mut messages,
            &title,
            width,
            display.has_touch(),
            page,
            laid.len(),
        )?;
        let mut body = Vec::new();
        paint::push(&mut body, Paint::Fill(area, style.paper))?;
        let (look, mut hits, mut thumbs) = (
            Look {
                font: &font,
                style: &style,
                base: metrics.book_height(),
            },
            Vec::new(),
            Vec::new(),
        );
        let mut lowest = area.y;
        let touch = display.has_touch();
        let mut chip = None;
        // The rows of the band being drawn only are laid out: a frame is
        // drawn in bands, and what lies outside one is not painted by it.
        let band = (
            frame.clip_top.saturating_sub(frame.rect.y),
            frame.clip_bottom.saturating_sub(frame.rect.y),
        );
        let outside = |r: Rect| {
            band.1 > band.0
                && (i32::from(r.y.saturating_add(r.height)) <= band.0 || i32::from(r.y) >= band.1)
        };
        for placed in laid.get(page).map_or(&[][..], Vec::as_slice) {
            let Some(cell) = self.cells().get(placed.cell) else {
                continue;
            };
            let (r, focused) = (placed.rect, placed.cell == self.focus());
            lowest = lowest.max(r.y.saturating_add(r.height));
            hits.try_reserve(1).map_err(|_| Error::NoMemory)?;
            hits.push((r, placed.cell));
            let Row::Book(at) = cell.row else {
                let parts = if cell.row == Row::Filter && touch {
                    Some(choice_chip::layout(&font, &style, r, &cell.note, true)?)
                } else {
                    None
                };
                if parts.is_some() {
                    chip = parts;
                }
                if outside(r) {
                    continue;
                }
                if cell.row == Row::Note {
                    let line = Rect::new(r.x, r.y, r.width.saturating_sub(style.margin), r.height);
                    text::label(
                        &mut body,
                        &font,
                        &cell.title,
                        style.small,
                        f32::from(style.margin),
                        line,
                        style.muted,
                        Some(r),
                    )?;
                } else if let Some(parts) = &parts {
                    look.chip_row(&mut body, cell, r, focused, parts)?;
                } else {
                    paperpaw_ui_kit::list::row(
                        &mut body,
                        &font,
                        &style,
                        r,
                        &cell.title,
                        &cell.value,
                        focused,
                    )?;
                }
                continue;
            };
            // The page's covers are asked for as the rows are laid out.
            if covers.slot(at) == Slot::Idle && covers.has_room() {
                let slot = covering.ask(at);
                covers.set(at, slot);
            }
            if outside(r) {
                continue;
            }
            let cover = if self.grid() && !cell.card {
                look.tile(&mut body, cell, r, metrics.tile_cover(), focused)?
            } else {
                look.book_row(&mut body, cell, r, focused)?
            };
            if let Slot::Ready(_) = covers.slot(at) {
                thumbs.try_reserve(1).map_err(|_| Error::NoMemory)?;
                thumbs.push((at, cover));
            } else {
                placeholder(
                    &mut body,
                    &font,
                    &style,
                    cover,
                    &cell.title,
                    ink(&style, focused),
                )?;
            }
        }
        if !said.title.is_empty() && self.cells().iter().all(|c| !matches!(c.row, Row::Book(_))) {
            let rest = Rect::new(0, lowest, width, height.saturating_sub(lowest));
            draw_empty(&mut body, &font, &style, rest, said)?;
        }
        paint::replay(frame, &font, head.paint.iter().chain(body.iter()))?;
        font.release()?;
        self.draw_covers(frame, &thumbs, covers, covering)?;
        head.paint = Vec::new();
        self.header = head;
        self.starts = collect(laid.iter().filter_map(|p| p.first().map(|f| Ok(f.cell))))?;
        self.hits = hits;
        self.band = Some(Rect::new(0, 0, width, style.header));
        self.touch = touch;
        self.chip = chip;
        self.metrics = Some(metrics);
        Ok(())
    }

    /// The host draws the covers that are made into their places. One the host
    /// has ended is asked for again once; one that cannot be drawn is outlined.
    fn draw_covers(
        &self,
        frame: &mut DrawFrame<'_>,
        thumbs: &[(usize, Rect)],
        covers: &mut Covers,
        covering: &mut dyn Covering,
    ) -> Result<(), Error> {
        if thumbs.is_empty() {
            return Ok(());
        }
        let canvas = frame.draw_canvas()?;
        for &(at, rect) in thumbs {
            let Slot::Ready(mut image) = covers.slot(at) else {
                continue;
            };
            let mut drawn = canvas.draw_image(image, frame_rect(rect), ImageStyle::new());
            if drawn == Err(Error::NotFound) {
                covers.ended(at);
                let slot = covering.ask(at);
                covers.set(at, slot);
                if let Slot::Ready(again) = slot {
                    image = again;
                    drawn = canvas.draw_image(image, frame_rect(rect), ImageStyle::new());
                }
            }
            if drawn.is_err() {
                // Not drawn: a frame outline, and no more asks for this one.
                covers.set(at, Slot::Missing);
                canvas
                    .stroke_rounded_rectangle(frame_rect(rect), 0, 2, Color::BLACK)
                    .ok();
            }
        }
        Ok(())
    }
}
