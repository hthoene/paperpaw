//! A list as a sheet at the bottom of the screen, over what is open: as high
//! as its items need, up to the screen's header, with a border at the top and
//! a pager when the items need more than a page (a menu).
use crate::{
    Messages, Paint, Style,
    items::{Item, ItemList, draw_row, heading, item_height, paginate},
    paint::{self},
    text,
    widget::Frame,
};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::canvas::PartRole;
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose},
        fonts::Font,
        fonts::FontRole,
        formatting::Arg,
        snapshot,
    },
};

/// What a sheet is laid out with, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    /// A row: a sheet is never lower than one.
    pub row: u16,
    /// The gap inside the sheet, above and below its items.
    pub gap: u16,
    /// The line of the border (the border is twice as thick, at least 2).
    pub line: u16,
    /// The header of the screen below, which stays in sight.
    pub header: u16,
    /// The pager bar at the bottom, when the items need more than a page.
    pub pager: u16,
    /// The display.
    pub width: u16,
    /// The display.
    pub height: u16,
}

/// Where the sheet lies and where its items go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    /// The whole sheet, at the bottom of the display.
    pub region: Rect,
    /// The border along its top edge.
    pub border: Rect,
    /// The items' area inside it.
    pub list: Rect,
    /// The pager bar, if the items need more than a page.
    pub pager: Option<Rect>,
}

/// The sheet for items of `heights`: as high as they need, up to the header
/// of the screen below; with a pager where they need more.
#[must_use]
pub fn geometry(heights: &[u16], m: &Metrics) -> Geometry {
    let thick = m.line.saturating_mul(2).max(2);
    let border = u32::from(thick);
    let gap = u32::from(m.gap);
    let total = heights
        .iter()
        .map(|h| u32::from(*h))
        .sum::<u32>()
        .max(u32::from(m.row));
    let wanted = total + 2 * gap + border;
    let most = u32::from(m.height).saturating_sub(u32::from(m.header));
    let height = wanted.min(most) as u16;
    let region = Rect::new(0, m.height - height, m.width, height);
    let mut list = Rect::new(
        0,
        region.y.saturating_add((border + gap) as u16),
        m.width,
        height.saturating_sub((border + 2 * gap) as u16),
    );
    let mut pager = None;
    if wanted > u32::from(height) {
        let bar = m.pager.min(list.height);
        list.height -= bar;
        pager = Some(Rect::new(0, region.y + region.height - bar, m.width, bar));
    }
    Geometry {
        region,
        border: Rect::new(region.x, region.y, region.width, thick.min(region.height)),
        list,
        pager,
    }
}

impl ItemList {
    /// Draws the page the focus is on as a sheet at the bottom of the screen.
    /// The tap targets are those of the last completed frame; a tap on the
    /// pager's arrows reports the page action as [`crate::ItemStep::Header`].
    ///
    /// # Errors
    /// Propagates allocation, font, catalog and drawing errors.
    pub fn draw_sheet(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        shown: bool,
    ) -> Result<Rect, Error> {
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let mut style = Style::read(host, &display, &font)?;
        style.describing = matches!(frame.purpose, DrawPurpose::Screen) && paint::asking(frame);
        // The frame the theme chose for all (a line along the top edge unless).
        let plate = style.plate(Frame::Line);
        style.ink = plate.ink;
        style.paper = plate.paper;
        let (width, height) = (display.display_width(), display.display_height());
        let mut heights = Vec::new();
        heights
            .try_reserve_exact(self.items.len())
            .map_err(|_| Error::NoMemory)?;
        for item in &self.items {
            heights.push(item_height(&font, &style, width, item)?);
        }
        let metrics = Metrics {
            row: style.row,
            gap: style.gap,
            line: style.line,
            header: style.header,
            pager: style.row,
            width,
            height,
        };
        let sheet = geometry(&heights, &metrics);
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            font.release()?;
            return Ok(sheet.region);
        }
        let starts = paginate(&self.items, &heights, sheet.list.height)?;
        let page = starts
            .iter()
            .rposition(|start| *start <= self.focus)
            .unwrap_or(0);
        let end = starts.get(page + 1).copied().unwrap_or(self.items.len());
        let mut body = Vec::new();
        paint::describe(
            &mut body,
            style.describing,
            PartRole::Dialog,
            sheet.region,
            false,
            false,
        )?;
        let fill = plate.fill.unwrap_or(plate.paper);
        paint::push(&mut body, Paint::Fill(sheet.region, fill))?;
        if plate.outline {
            paint::push(&mut body, Paint::Fill(sheet.border, plate.line))?;
        }
        let mut hits = Vec::new();
        let mut steps = Vec::new();
        let mut y = sheet.list.y;
        for (index, &high) in heights.iter().enumerate().take(end).skip(starts[page]) {
            let rect = Rect::new(0, y, width, high);
            let focused = shown && index == self.focus;
            match &self.items[index] {
                Item::Heading(title) => heading(&mut body, &font, &style, rect, title)?,
                Item::Row(row) => {
                    if let Some((minus, plus)) =
                        draw_row(&mut body, &font, &style, rect, row, focused)?
                    {
                        steps.try_reserve(1).map_err(|_| Error::NoMemory)?;
                        steps.push((minus, plus, index));
                    }
                    hits.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    hits.push((rect, index));
                }
            }
            y = y.saturating_add(high);
        }
        let mut header = crate::Page::default();
        if let (Some(bar), true) = (sheet.pager, starts.len() > 1) {
            let label = page_label(host, page, starts.len())?;
            let touch = display.has_touch();
            pager(
                &mut body,
                &mut header,
                &font,
                &style,
                bar,
                &label,
                page,
                starts.len(),
                touch,
            )?;
        }
        paint::replay(frame, &font, body.iter())?;
        font.release()?;
        self.header = header;
        self.starts = starts;
        self.hits = hits;
        self.steps = steps;
        Ok(sheet.region)
    }
}

/// The pager's "page N of M" text.
///
/// # Errors
/// Propagates catalog and allocation errors.
fn page_label(host: &Host<'_>, page: usize, pages: usize) -> Result<String, Error> {
    let mut messages = Messages::new()?;
    messages.format(
        host,
        "core.page-of",
        &[
            Arg::Int {
                name: "page",
                value: page.saturating_add(1) as i64,
            },
            Arg::Int {
                name: "pages",
                value: pages as i64,
            },
        ],
    )
}

/// The pager bar: the page of pages in the middle, and on touch screens an
/// arrow at each end (faint where there is no page that way). The arrows'
/// tap regions go to `header`.
///
/// # Errors
/// Propagates font and allocation errors.
#[allow(clippy::too_many_arguments, reason = "geometry, look and paging")]
pub fn pager(
    body: &mut Vec<Paint>,
    header: &mut crate::Page,
    font: &Font<'_>,
    style: &Style,
    bar: Rect,
    label: &str,
    page: usize,
    pages: usize,
    touch: bool,
) -> Result<(), Error> {
    let size = style.small;
    let width = font.measure_text(size, label)?;
    let x = f32::from(bar.x) + (f32::from(bar.width) - width).max(0.0) / 2.0;
    text::label(body, font, label, size, x, bar, style.muted, Some(bar))?;
    if !touch {
        return Ok(());
    }
    let side = bar.height.max(style.row).min(bar.width / 4);
    // The first page's arrow at the start side: the right one where the
    // interface runs right to left, pointing the other way.
    let prev = style.flip(bar, Rect::new(bar.x, bar.y, side, bar.height));
    let next = style.flip(
        bar,
        Rect::new(bar.x + bar.width - side, bar.y, side, bar.height),
    );
    let (back, on_glyph) = if style.rtl {
        ("\u{203a}", "\u{2039}")
    } else {
        ("\u{2039}", "\u{203a}")
    };
    for (rect, glyph, on) in [(prev, back, page > 0), (next, on_glyph, page + 1 < pages)] {
        let color = if on { style.ink } else { style.disabled };
        let at = f32::from(rect.x)
            + (f32::from(rect.width) - font.measure_text(style.text, glyph)?) / 2.0;
        text::label(body, font, glyph, style.text, at, rect, color, Some(rect))?;
    }
    // The label between them does nothing.
    header.pager = Some((
        prev,
        Rect::new(
            bar.x + side,
            bar.y,
            bar.width.saturating_sub(2 * side),
            bar.height,
        ),
        next,
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> Metrics {
        Metrics {
            row: 60,
            gap: 8,
            line: 2,
            header: 80,
            pager: 50,
            width: 480,
            height: 800,
        }
    }

    #[test]
    fn the_sheet_is_as_high_as_its_items_and_lies_at_the_bottom() {
        let m = metrics();
        let s = geometry(&[60, 60, 60], &m);
        // 180 + 2 gaps of 8 + a border of 4.
        assert_eq!(s.region.height, 200);
        assert_eq!(s.region.y + s.region.height, 800);
        assert_eq!(s.border, Rect::new(0, 600, 480, 4));
        assert_eq!(s.list.y, s.region.y + 4 + 8);
        assert_eq!(s.list.height, 200 - 4 - 16);
        assert!(s.pager.is_none());
    }

    #[test]
    fn too_many_items_stop_under_the_header_and_get_a_pager() {
        let m = metrics();
        let s = geometry(&[60; 20], &m);
        assert_eq!(s.region.height, 720);
        assert_eq!(s.region.y, 80);
        let pager = s.pager.unwrap();
        assert_eq!(pager.y + pager.height, 800);
        assert_eq!(pager.height, 50);
        assert_eq!(s.list.height, 720 - 4 - 16 - 50);
        // Nothing in it: a sheet of one row.
        assert_eq!(geometry(&[], &m).region.height, 60 + 16 + 4);
    }

    #[test]
    fn a_thin_line_still_makes_a_border_of_two() {
        let m = Metrics {
            line: 0,
            ..metrics()
        };
        assert_eq!(geometry(&[60], &m).border.height, 2);
    }

    #[test]
    fn a_tiny_screen_does_not_underflow() {
        let m = Metrics {
            height: 100,
            header: 90,
            ..metrics()
        };
        let s = geometry(&[60; 5], &m);
        assert_eq!(s.region.height, 10);
        assert!(s.list.height <= 10);
    }
}
