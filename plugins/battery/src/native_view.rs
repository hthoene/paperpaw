//! Owned Battery screen, rendered through the flat native SDK only.
//!
//! Layout is prepared when the screen opens. Font leases, service records,
//! settings text and the canvas remain confined to their callback.

use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{DrawFrame, DrawPurpose, FrameRect, Input, InputGestureKind},
    commands::{CommandSender, RedrawMode},
    fonts::{Font, FontRole, TextOrigin},
    snapshot,
};
use paperpaw_sdk::{Color, Handled, Point, Rect};

use crate::log::Reading;

mod content;
mod header;
mod history;
mod messages;
mod rows;
mod style;
mod text;

use messages::Messages;
use style::Style;

enum Line {
    Heading(String),
    Value(String, String),
    Message(String),
}

/// Draw operations contain only owned text and scalar geometry.
#[derive(Debug)]
enum Paint {
    Fill(Rect, Color),
    Text {
        text: String,
        size: f32,
        origin: TextOrigin,
        color: Color,
        clip: Option<Rect>,
    },
}

#[derive(Debug, Default)]
struct Page {
    paint: Vec<Paint>,
    back: Option<Rect>,
    pager: Option<(Rect, Rect, Rect)>,
}

#[derive(Debug)]
pub(crate) struct NativeView {
    pages: Vec<Page>,
    page: usize,
}

impl NativeView {
    pub(crate) fn new(host: &Host<'_>, now: Reading) -> Result<Self, Error> {
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let mut messages = Messages::new(host)?;
        let (lines, footer) = content::lines(host, now, &mut messages)?;
        let starts = paginate(&lines, &style, display.display_height());
        let title = messages.copy(host, "battery.title")?;
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(starts.len())
            .map_err(|_| Error::NoMemory)?;
        for (index, &first) in starts.iter().enumerate() {
            let mut page = Page::default();
            header::prepare(
                &mut page,
                host,
                &font,
                &style,
                &mut messages,
                &title,
                display.display_width(),
                display.has_touch(),
                index,
                starts.len(),
            )?;
            let last = starts.get(index + 1).copied().unwrap_or(lines.len());
            prepare_body(
                &mut page.paint,
                &font,
                &style,
                &lines[first..last],
                &footer,
                display.display_width(),
                display.display_height(),
            )?;
            pages.push(page);
        }
        font.release()?;
        Ok(Self { pages, page: 0 })
    }

    pub(crate) fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let mut canvas = frame.draw_canvas()?;
        let page = self.pages.get(self.page).ok_or(Error::State)?;
        for operation in &page.paint {
            match operation {
                Paint::Fill(rect, color) => canvas.fill_rectangle(frame_rect(*rect), *color)?,
                Paint::Text {
                    text,
                    size,
                    origin,
                    color,
                    clip,
                } => {
                    if let Some(clip) = clip {
                        let region = canvas.clip_region(frame_rect(*clip))?;
                        font.render_text(&region, *size, *origin, *color, text)?;
                        region.restore()?;
                    } else {
                        font.render_text(&canvas, *size, *origin, *color, text)?;
                    }
                }
            }
        }
        font.release()
    }

    /// Page actions are handled at either end too, as in the original screen.
    /// Commit the new page only after the host accepts its redraw request.
    pub(crate) fn action(&mut self, host: &Host<'_>, action: &str) -> Result<Handled, Error> {
        let page = match action {
            "core.next-page" | "core.down" | "core.next" | "core.right" => self
                .page
                .saturating_add(1)
                .min(self.pages.len().saturating_sub(1)),
            "core.prev-page" | "core.up" | "core.previous" | "core.left" => {
                self.page.saturating_sub(1)
            }
            _ => return Ok(Handled::No),
        };
        if page != self.page {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
            self.page = page;
        }
        Ok(Handled::Yes)
    }

    pub(crate) fn input(&mut self, host: &Host<'_>, input: Input<'_>) -> Result<Handled, Error> {
        match input {
            Input::Action { action, .. } => self.action(host, action),
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => {
                let Some(action) = self.tap(position) else {
                    return Ok(Handled::No);
                };
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).run_action(action)?;
                Ok(Handled::Yes)
            }
            _ => Ok(Handled::No),
        }
    }

    /// The header's tap targets use the same geometry as its drawing.
    fn tap(&self, point: Point) -> Option<&'static str> {
        let page = self.pages.get(self.page)?;
        if page.back.is_some_and(|rect| rect.contains(point)) {
            return Some("core.back");
        }
        let (previous, label, next) = page.pager?;
        let before = point.x < i32::from(label.x) + i32::from(label.width) / 2;
        if previous.contains(point) || (label.contains(point) && before) {
            Some("core.prev-page")
        } else if next.contains(point) || label.contains(point) {
            Some("core.next-page")
        } else {
            None
        }
    }
}

fn frame_rect(rect: Rect) -> FrameRect {
    FrameRect::new(
        i32::from(rect.x),
        i32::from(rect.y),
        u32::from(rect.width),
        u32::from(rect.height),
    )
}

fn height(line: &Line, style: &Style) -> u16 {
    match line {
        Line::Heading(_) => style.heading,
        Line::Value(..) | Line::Message(_) => style.row,
    }
}

fn paginate(lines: &[Line], style: &Style, screen_height: u16) -> Vec<usize> {
    let (top, bottom) = (
        u32::from(style.header),
        u32::from(screen_height.saturating_sub(style.row)),
    );
    let mut pages = Vec::from([0]);
    let mut y = top;
    for (index, line) in lines.iter().enumerate() {
        let start = pages.last().copied().unwrap_or(0);
        if y + u32::from(height(line, style)) > bottom && index > start {
            let heading_last = index > start + 1 && matches!(lines[index - 1], Line::Heading(_));
            let first = if heading_last { index - 1 } else { index };
            pages.push(first);
            y = top;
            if heading_last {
                y += u32::from(height(&lines[first], style));
            }
        }
        y += u32::from(height(line, style));
    }
    pages
}

#[allow(
    clippy::too_many_arguments,
    reason = "layout inputs and owned output for one page"
)]
fn prepare_body(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    lines: &[Line],
    footer: &str,
    width: u16,
    screen_height: u16,
) -> Result<(), Error> {
    let mut y = style.header;
    let bottom = screen_height.saturating_sub(style.row);
    for line in lines {
        let high = height(line, style);
        if u32::from(y) + u32::from(high) > u32::from(bottom) {
            break;
        }
        let rect = Rect::new(0, y, width, high);
        match line {
            Line::Heading(title) => {
                let line = font.read_line_metrics(style.small)?.line_height as u16;
                let area = Rect::new(
                    0,
                    y + high.saturating_sub(line + style.gap / 2),
                    width.saturating_sub(style.margin),
                    line,
                );
                text::label(
                    paint,
                    font,
                    title,
                    style.small,
                    f32::from(style.margin),
                    area,
                    style.muted,
                    None,
                )?;
            }
            Line::Value(title, value) => rows::prepare(paint, font, style, rect, title, value)?,
            Line::Message(message) => text::label(
                paint,
                font,
                message,
                style.small,
                f32::from(style.margin),
                rect,
                style.muted,
                None,
            )?,
        }
        y += high;
    }
    text::label(
        paint,
        font,
        footer,
        style.small,
        f32::from(style.margin),
        Rect::new(0, bottom, width, style.row),
        style.note,
        None,
    )
}
