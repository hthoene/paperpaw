//! Paints the list of books: a row of a book has its cover, title, author, a
//! bar of how far it is read and the badges plugins put on it; the book read
//! last stands on top as a larger card; a grid shows covers side by side.
//! The text and shapes are laid out first and replayed; the covers are drawn
//! by the host after them, into the places kept for them.
use crate::{
    native_covers::{Covers, Slot},
    native_list::{BookList, Cell, Metrics, Row, pages},
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
    Messages, Page, Paint, Style, header,
    list::{backdrop, columns, ink},
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

/// The title's first letter in capitals: what a book without a cover shows.
fn initial(title: &str) -> Result<String, Error> {
    let mut letter = String::new();
    letter.try_reserve(12).map_err(|_| Error::NoMemory)?;
    if let Some(c) = title.chars().find(|c| c.is_alphanumeric()) {
        for upper in c.to_uppercase().take(3) {
            letter.push(upper);
        }
    }
    Ok(letter)
}

/// An outline with the title's first letter, where a book has no cover (yet).
fn placeholder(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    at: Rect,
    title: &str,
    color: Color,
) -> Result<(), Error> {
    let radius = style.corner.min(u32::from(at.width.min(at.height)) / 8);
    paint::push(
        out,
        Paint::Focus(at, radius, Some(u32::from(style.line).max(1)), color),
    )?;
    let letter = initial(title)?;
    if letter.is_empty() {
        return Ok(());
    }
    let size = style.title * 1.3;
    let wide = font.measure_text(size, &letter)?;
    let x = f32::from(at.x) + (f32::from(at.width) - wide) / 2.0;
    text::label(out, font, &letter, size, x, at, color, Some(at))
}

/// A bar `percent` full inside `at`.
fn bar(
    out: &mut Vec<Paint>,
    style: &Style,
    at: Rect,
    percent: u8,
    color: Color,
) -> Result<(), Error> {
    paint::push(out, Paint::Fill(at, style.rule))?;
    let full = u32::from(at.width) * u32::from(percent.min(100)) / 100;
    let full = u16::try_from(full).unwrap_or(at.width);
    paint::push(
        out,
        Paint::Fill(Rect::new(at.x, at.y, full, at.height), color),
    )
}

/// What a cell's drawing needs besides the cell.
struct Look<'a> {
    font: &'a Font<'a>,
    style: &'a Style,
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
        focused: bool,
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
            (
                Rect::new(left.x, top, left.width, line),
                Rect::new(left.x, top.saturating_add(line), left.width, small),
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
            columns(
                out,
                font,
                style,
                Rect::new(
                    first.x.saturating_sub(style.margin),
                    first.y,
                    first.width.saturating_add(style.margin),
                    first.height,
                ),
                &cell.title,
                &cell.value,
                ink,
            )?;
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
                Some(at),
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
        let cover = thumb(r, style);
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
        self.words(out, cell, left, focused)?;
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
        backdrop(out, style, r, focused)?;
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
    let head = paperpaw_ui_kit::geometry::line_px(font, style.title)?;
    let small = paperpaw_ui_kit::geometry::line_px(font, style.small)?;
    let total = u32::from(head) + u32::from(style.gap) + u32::from(small) * lines.len() as u32;
    let total = u16::try_from(total).unwrap_or(u16::MAX);
    let mut y = area.y.saturating_add(area.height.saturating_sub(total) / 2);
    let title = text::fit(font, style.title, said.title, room)?;
    let x = (f32::from(area.width) - font.measure_text(style.title, &title)?).max(0.0) / 2.0;
    text::label(
        out,
        font,
        &title,
        style.title,
        x,
        Rect::new(0, y, area.width, head),
        style.ink,
        None,
    )?;
    y = y.saturating_add(head).saturating_add(style.gap);
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
        let laid = pages(self.cells(), &metrics, self.grid())?;
        let page = laid
            .iter()
            .rposition(|p| p.first().is_some_and(|first| first.cell <= self.focus()))
            .unwrap_or(0);
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
            },
            Vec::new(),
            Vec::new(),
        );
        let mut lowest = area.y;
        for placed in laid.get(page).map_or(&[][..], Vec::as_slice) {
            let Some(cell) = self.cells().get(placed.cell) else {
                continue;
            };
            let (r, focused) = (placed.rect, placed.cell == self.focus());
            lowest = lowest.max(r.y.saturating_add(r.height));
            hits.try_reserve(1).map_err(|_| Error::NoMemory)?;
            hits.push((r, placed.cell));
            let Row::Book(at) = cell.row else {
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
        self.starts = laid
            .iter()
            .filter_map(|p| p.first().map(|f| f.cell))
            .collect();
        self.hits = hits;
        self.band = Some(Rect::new(0, 0, width, style.header));
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
