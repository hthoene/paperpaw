//! A paged list screen: header, rows of title and value, focus, hit-testing.
use crate::{Messages, Page, Paint, Style, header, paint, text};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::canvas::PartRole;
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, FrameRect},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        snapshot,
    },
};

/// Retains scalar hit regions only; fonts and canvas never escape callbacks.
#[derive(Debug, Default)]
pub struct ListRenderer {
    area: Option<Rect>,
    row: u16,
    page: usize,
    rows: usize,
    header: Page,
    /// Where the last frame's region began on the screen.
    origin: (i32, i32),
}
impl ListRenderer {
    /// A renderer with the geometry of an earlier frame: the list area, the
    /// row height, the page shown and the rows a page holds. For tests of
    /// input handling, which needs no drawing.
    #[must_use]
    pub fn geometry(area: Rect, row: u16, page: usize, rows: usize) -> Self {
        Self {
            area: Some(area),
            row,
            page,
            rows,
            header: Page::default(),
            origin: (0, 0),
        }
    }
    /// First visible item, from the last successfully rendered frame.
    pub fn first(&self) -> usize {
        self.page.saturating_mul(self.rows)
    }
    /// Maximum visible rows in the last frame.
    pub fn rows(&self) -> usize {
        self.rows
    }
    /// Hit-test the same row rectangles used for drawing.
    pub fn hit(&self, point: Point, count: usize) -> Option<usize> {
        let area = self.area?;
        if !area.contains(point) || self.row == 0 {
            return None;
        }
        let slot = (point.y - i32::from(area.y)) as usize / usize::from(self.row);
        let index = self.first().checked_add(slot)?;
        (slot < self.rows && index < count).then_some(index)
    }
    /// Where row `index` was drawn in the last frame; `None` off its page.
    #[must_use]
    pub fn rect_of(&self, index: usize) -> Option<Rect> {
        let area = self.area?;
        let slot = index
            .checked_sub(self.first())
            .filter(|slot| *slot < self.rows)?;
        let y = usize::from(area.y).checked_add(slot.checked_mul(usize::from(self.row))?)?;
        Some(Rect::new(
            area.x,
            u16::try_from(y).ok()?,
            area.width,
            self.row,
        ))
    }

    /// Asks for the frame a focus step from row `from` to row `to` needs:
    /// the two rows alone where both are on the page drawn last, the screen
    /// where the step turns the page or the host refuses an area.
    ///
    /// # Errors
    /// Propagates command queueing errors.
    pub fn redraw_step(&self, host: &Host<'_>, from: usize, to: usize) -> Result<(), Error> {
        redraw_areas(host, self.origin, [self.rect_of(from), self.rect_of(to)])
    }

    /// Header navigation target, where touch arrows were drawn.
    pub fn header_action(&self, point: Point) -> Option<&'static str> {
        if self.header.back.is_some_and(|rect| rect.contains(point)) {
            return Some("core.back");
        }
        let (previous, _, next) = self.header.pager?;
        if previous.contains(point) {
            Some("core.prev-page")
        } else if next.contains(point) {
            Some("core.next-page")
        } else {
            None
        }
    }
    /// Render a page through typed font/canvas calls. No Env or host UI adapter.
    ///
    /// # Errors
    /// Propagates allocation, font, catalog and drawing errors; on failure the
    /// retained hit map stays at the last completed frame.
    #[allow(
        clippy::too_many_arguments,
        reason = "callback plus current list model"
    )]
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        title: &str,
        rows: &[(String, String)],
        focus: usize,
        shown: bool,
    ) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let mut style = Style::read(host, &display, &font)?;
        style.describing = paint::asking(frame);
        let width = display.display_width();
        let height = display.display_height();
        let top = style.header.saturating_add(style.gap);
        let area = Rect::new(0, top, width, height.saturating_sub(top));
        let per_page = (usize::from(area.height) / usize::from(style.row.max(1))).max(1);
        let count = rows.len().div_ceil(per_page).max(1);
        let index = (focus / per_page).min(count - 1);
        let mut header = Page::default();
        let mut messages = Messages::new()?;
        let title = messages.copy(host, title)?;
        header::prepare(
            &mut header,
            host,
            &font,
            &style,
            &mut messages,
            &title,
            width,
            display.has_touch(),
            index,
            count,
        )?;
        let mut body = Vec::new();
        body.try_reserve(1).map_err(|_| Error::NoMemory)?;
        body.push(Paint::Fill(style.page(area), style.paper));
        for (slot, (label, value)) in rows
            .iter()
            .skip(index * per_page)
            .take(per_page)
            .enumerate()
        {
            let rect = Rect::new(
                0,
                top.saturating_add((slot * usize::from(style.row)) as u16),
                width,
                style.row,
            );
            let focused = shown && index * per_page + slot == focus;
            row(&mut body, &font, &style, rect, label, value, focused)?;
        }

        paint::replay(frame, &font, header.paint.iter().chain(body.iter()))?;
        font.release()?;
        // Only hit geometry survives the draw callback.
        header.paint = Vec::new();
        self.header = header;
        self.area = Some(area);
        self.row = style.row;
        self.rows = per_page;
        self.page = index;
        self.origin = (frame.rect.x, frame.rect.y);
        Ok(())
    }
}

/// Asks for the two `areas` of a focus step (in screen pixels; the frame's
/// region began at `origin`) to be drawn again, as interface 6's
/// `request_redraw_area`: what the focus leaves and what it reaches. The
/// whole screen where one is not known (`None`: off the page drawn last) or
/// the host refuses an area.
///
/// # Errors
/// Propagates command queueing errors.
pub fn redraw_areas(
    host: &Host<'_>,
    origin: (i32, i32),
    areas: [Option<Rect>; 2],
) -> Result<(), Error> {
    let mut scratch = [0; 64];
    redraw_step(&mut CommandSender::new(host, &mut scratch), origin, areas)
}

/// Where the redraws of a focus step go: the host's command stage, or a test's
/// record.
pub(crate) trait Redraws {
    /// Asks for `area` of the region to be drawn again.
    fn area(&mut self, area: FrameRect) -> Result<(), Error>;
    /// Asks for the whole screen to be drawn again.
    fn whole(&mut self) -> Result<(), Error>;
}

impl Redraws for CommandSender<'_, '_> {
    fn area(&mut self, area: FrameRect) -> Result<(), Error> {
        self.queue_redraw_area(RedrawMode::Partial, area)
    }
    fn whole(&mut self) -> Result<(), Error> {
        self.queue_redraw(RedrawMode::Partial)
    }
}

/// [`redraw_areas`] on `to`.
pub(crate) fn redraw_step(
    to: &mut impl Redraws,
    origin: (i32, i32),
    areas: [Option<Rect>; 2],
) -> Result<(), Error> {
    if let [Some(left), Some(reached)] = areas {
        let area = |rect: Rect| {
            FrameRect::new(
                i32::from(rect.x).saturating_sub(origin.0),
                i32::from(rect.y).saturating_sub(origin.1),
                u32::from(rect.width),
                u32::from(rect.height),
            )
        };
        if to
            .area(area(left))
            .and_then(|()| to.area(area(reached)))
            .is_ok()
        {
            return Ok(());
        }
    }
    to.whole()
}
/// Prepares one list row: focus, separator, title at the left and value at the right.
///
/// # Errors
/// Propagates allocation and font errors.
pub fn row(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    title: &str,
    value: &str,
    focused: bool,
) -> Result<(), Error> {
    backdrop(paint, style, rect, focused)?;
    paint::describe(paint, style.describing, PartRole::Row, rect, focused, false)?;
    columns(paint, font, style, rect, title, value, ink(style, focused))
}

/// The ink of text on a row that is focused or not.
#[must_use]
pub fn ink(style: &Style, focused: bool) -> Color {
    if focused { style.on_bar() } else { style.ink }
}

/// The focus round a row, or else the separator below it.
///
/// # Errors
/// [`Error::NoMemory`] when the paint list cannot grow.
pub fn backdrop(
    paint: &mut Vec<Paint>,
    style: &Style,
    rect: Rect,
    focused: bool,
) -> Result<(), Error> {
    if focused {
        let inset = rect.inset(
            (style.margin / 2).min(rect.width / 8),
            style.line.min(rect.height / 8),
        );
        // A pill only round a row of one band: round a taller one (a title
        // over two lines, a note) its ends would cut into the text.
        let radius = if inset.width < inset.height || !style.pill || inset.height > style.row {
            style.corner
        } else {
            u32::from(inset.height) / 2
        };
        paint::push(
            paint,
            Paint::Focus(
                inset,
                radius,
                style
                    .ring
                    .then_some(u32::from(style.line) * 2)
                    .map(|line| line.max(3)),
                style.bar(),
            ),
        )?;
    } else if style.separators != "core.separators.none" {
        let thickness = if style.separators == "core.separators.inset" {
            (style.line / 2).max(1)
        } else {
            style.line
        };
        paint::push(
            paint,
            Paint::Fill(
                Rect::new(
                    rect.x + style.margin,
                    rect.y + rect.height.saturating_sub(style.line),
                    rect.width.saturating_sub(2 * style.margin),
                    thickness,
                ),
                style.separator_ink,
            ),
        )?;
    }
    Ok(())
}

/// A title at the left and a value at the right of `rect`, shortened to fit.
///
/// # Errors
/// Propagates allocation and font errors.
pub fn columns(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    title: &str,
    value: &str,
    ink: Color,
) -> Result<(), Error> {
    let margin = f32::from(style.margin);
    let room = (f32::from(rect.width) - 2.0 * margin).max(0.0);
    let room = (room - margin).max(0.0);
    let title_width = font.measure_text(style.text, title)?.min(room * 0.6);
    let value = text::fit(font, style.text, value, room - title_width + 1.0)?;
    let value_width = font.measure_text(style.text, &value)?;
    let title_room = (room - value_width).max(0.0);
    let title = text::fit(font, style.text, title, title_room + 1.0)?;
    let title_area = Rect::new(rect.x, rect.y, (margin + title_room) as u16, rect.height);
    // Laid out left to right, mirrored in the row where the interface runs
    // right to left: the title at the right and the value at the left.
    text::label_in(
        paint,
        font,
        style.rtl,
        rect,
        &title,
        style.text,
        f32::from(rect.x) + margin,
        title_area,
        ink,
        Some(rect),
    )?;
    text::label_in(
        paint,
        font,
        style.rtl,
        rect,
        &value,
        style.text,
        f32::from(rect.x) + f32::from(rect.width) - margin - value_width,
        rect,
        ink,
        Some(rect),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hit_testing_uses_only_complete_rows_on_the_current_page() {
        let renderer = ListRenderer {
            area: Some(Rect::new(0, 50, 300, 210)),
            row: 40,
            page: 2,
            rows: 5,
            ..ListRenderer::default()
        };
        assert_eq!(renderer.hit(Point::new(2, 50), 14), Some(10));
        assert_eq!(renderer.hit(Point::new(2, 209), 14), Some(13));
        assert_eq!(renderer.hit(Point::new(2, 210), 14), None);
        assert_eq!(renderer.hit(Point::new(2, 251), 100), None);
        assert_eq!(renderer.hit(Point::new(-1, 50), 100), None);
    }
    #[test]
    fn header_hit_map_does_not_turn_a_label_into_a_page_action() {
        let renderer = ListRenderer {
            header: Page {
                paint: Vec::new(),
                back: Some(Rect::new(0, 0, 30, 40)),
                pager: Some((
                    Rect::new(100, 0, 30, 40),
                    Rect::new(130, 0, 60, 40),
                    Rect::new(190, 0, 30, 40),
                )),
            },
            ..ListRenderer::default()
        };
        assert_eq!(
            renderer.header_action(Point::new(15, 20)),
            Some("core.back")
        );
        assert_eq!(
            renderer.header_action(Point::new(115, 20)),
            Some("core.prev-page")
        );
        assert_eq!(renderer.header_action(Point::new(150, 20)), None);
        assert_eq!(
            renderer.header_action(Point::new(200, 20)),
            Some("core.next-page")
        );
    }

    #[test]
    fn a_row_is_where_it_was_drawn_and_none_off_its_page() {
        // The second page of rows of 40 px from y 50, five a page.
        let renderer = ListRenderer::geometry(Rect::new(0, 50, 300, 200), 40, 1, 5);
        assert_eq!(renderer.rect_of(5), Some(Rect::new(0, 50, 300, 40)));
        assert_eq!(renderer.rect_of(7), Some(Rect::new(0, 130, 300, 40)));
        assert_eq!(renderer.rect_of(9), Some(Rect::new(0, 210, 300, 40)));
        // The page before and the page after: a step there turns the page.
        assert_eq!(renderer.rect_of(4), None);
        assert_eq!(renderer.rect_of(10), None);
        // Nothing drawn yet: no row is known.
        assert_eq!(ListRenderer::default().rect_of(0), None);
    }

    #[test]
    fn the_focus_bar_is_the_themes_ink_so_its_paper_text_reads_on_it() {
        // #114: the bar was a literal black while its text is the theme's
        // paper: on a dark theme (light ink, dark paper) both were dark.
        let style = Style::themed(Color::rgb(235, 230, 220), Color::rgb(25, 30, 35));
        let mut paint = Vec::new();
        backdrop(&mut paint, &style, Rect::new(0, 0, 200, 60), true).unwrap();
        let bar = paint.iter().find_map(|op| match op {
            Paint::Focus(_, _, None, color) => Some(*color),
            _ => None,
        });
        assert_eq!(bar, Some(style.ink));
        assert_eq!(ink(&style, true), style.paper);
    }

    #[test]
    fn the_soft_bar_is_a_card_with_the_themes_text_on_it() {
        let mut style = Style::themed(Color::rgb(10, 20, 30), Color::rgb(240, 230, 220));
        style.soft = true;
        let mut paint = Vec::new();
        backdrop(&mut paint, &style, Rect::new(0, 0, 200, 60), true).unwrap();
        let bar = paint.iter().find_map(|op| match op {
            Paint::Focus(_, _, None, color) => Some(*color),
            _ => None,
        });
        assert_eq!(bar, Some(style.soft_bar()));
        // Of four grays (0, 85, 170, 255) it is not the paper's.
        assert!(style.soft_bar().luma().abs_diff(style.paper.luma()) > 40);
        assert_eq!(ink(&style, true), style.ink);
        // A frame keeps the text's ink too; a dark bar takes the paper.
        style.soft = false;
        style.ring = true;
        assert_eq!(ink(&style, true), style.ink);
        style.ring = false;
        assert_eq!(ink(&style, true), style.paper);
    }
}
