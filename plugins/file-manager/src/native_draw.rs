//! The browser's layout and drawing with the kit: the header, rows of name and
//! size, a line for what happened, and the bar of buttons below.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::callbacks::{DrawFrame, DrawPurpose};
use paperpaw_sdk::native::fonts::{Font, FontRole};
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::snapshot;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{Point, Rect};
use paperpaw_ui_kit::{Messages, Page, Paint, Style, header, list, paint, text};

use crate::browser::{Browser, Button, Note};
use crate::model::path;

/// Where the parts of the screen go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layout {
    /// The rows.
    pub area: Rect,
    pub row: u16,
    /// Entries a page shows.
    pub rows: usize,
    /// What happened, above the bar.
    pub note: Rect,
    pub bar: Rect,
}

impl Layout {
    pub(crate) fn of(style: &Style, width: u16, height: u16) -> Self {
        let top = style.header.saturating_add(style.gap);
        let bar_y = height.saturating_sub(style.row).saturating_sub(style.gap);
        let note_y = bar_y.saturating_sub(style.row);
        let row = style.row.max(1);
        let area = Rect::new(0, top, width, note_y.saturating_sub(top));
        Self {
            area,
            row,
            rows: usize::from(area.height / row).max(1),
            note: Rect::new(0, note_y, width, style.row),
            bar: Rect::new(0, bar_y, width, style.row),
        }
    }

    /// The button `index` of `count` along the bar.
    pub(crate) fn button(&self, style_margin: u16, gap: u16, index: usize, count: usize) -> Rect {
        let count = count.max(1) as u16;
        let gap = gap.saturating_mul(2);
        let width = self
            .bar
            .width
            .saturating_sub(style_margin.saturating_mul(2))
            .saturating_sub(gap.saturating_mul(count - 1))
            / count;
        Rect::new(
            style_margin.saturating_add((index as u16).saturating_mul(width.saturating_add(gap))),
            self.bar.y,
            width,
            self.bar.height,
        )
    }
}

/// The rows a page shows now, from the display and the text size.
///
/// # Errors
/// The host's refusal.
pub(crate) fn rows_now(host: &Host<'_>) -> Result<usize, Error> {
    let display = snapshot::capture_snapshot(host)?;
    let font = Font::acquire_role(host, FontRole::Ui)?;
    let style = Style::read(host, &display, &font);
    font.release()?;
    let layout = Layout::of(&style?, display.display_width(), display.display_height());
    Ok(layout.rows)
}

/// What the last frame had, for taps.
#[derive(Debug, Default)]
pub(crate) struct Hits {
    pub layout: Option<Layout>,
    pub margin: u16,
    pub gap: u16,
    pub header: Page,
    pub buttons: usize,
}

impl Hits {
    /// The entry number (of the folder) at `point`, if a row is there.
    pub(crate) fn entry_at(&self, point: Point, first: usize, held: usize) -> Option<usize> {
        let layout = self.layout?;
        if !layout.area.contains(point) {
            return None;
        }
        let slot = (point.y - i32::from(layout.area.y)) as usize / usize::from(layout.row);
        (slot < layout.rows && slot < held).then_some(first + slot)
    }

    /// The button at `point`.
    pub(crate) fn button_at(&self, point: Point) -> Option<usize> {
        let layout = self.layout?;
        (0..self.buttons).find(|i| {
            layout
                .button(self.margin, self.gap, *i, self.buttons)
                .contains(point)
        })
    }
}

/// The text of a note, in the language shown.
fn note_text(host: &Host<'_>, messages: &mut Messages, note: &Note) -> Result<String, Error> {
    let key = match note {
        Note::Picked { copy, name } => {
            let key = if *copy {
                "file-manager.picked-copy"
            } else {
                "file-manager.picked-move"
            };
            return messages.format(
                host,
                key,
                &[Arg::Text {
                    name: "name",
                    value: name,
                }],
            );
        }
        Note::OwnData => "file-manager.error.own-data",
        Note::BadName => "file-manager.error.invalid",
        Note::NotTold => "file-manager.not-told",
        Note::Failed(error) => match error {
            Error::NotFound => "file-manager.error.not-found",
            Error::Invalid => "file-manager.error.invalid",
            Error::Unsupported | Error::Denied => "file-manager.error.unsupported",
            Error::Full => "file-manager.error.full",
            Error::NoMemory => "file-manager.error.memory",
            Error::NoSpace => "file-manager.error.too-long",
            _ => "file-manager.error.io",
        },
    };
    messages.copy(host, key)
}

/// A size as people read it: `820 B`, `14 KB`, `2,3 MB`.
pub(crate) fn size_text(bytes: u64, mark: &str) -> Result<String, Error> {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    let mut digits = [0; 20];
    let mut out = String::new();
    let mut push = |text: &str| -> Result<(), Error> {
        out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
        out.push_str(text);
        Ok(())
    };
    if unit == 0 || value >= 10.0 {
        push(text::decimal_u64((value + 0.5) as u64, &mut digits))?;
    } else {
        let tenths = (value * 10.0 + 0.5) as u64;
        push(text::decimal_u64(tenths / 10, &mut digits))?;
        push(mark)?;
        push(text::decimal_u64(tenths % 10, &mut digits))?;
    }
    push(" ")?;
    push(UNITS.get(unit).copied().unwrap_or("B"))?;
    Ok(out)
}

/// Draws the browser; returns where things are for taps.
///
/// # Errors
/// The host's refusals and a refused allocation.
pub(crate) fn draw(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    browser: &Browser,
    shown: bool,
) -> Result<Hits, Error> {
    let display = snapshot::capture_snapshot(host)?;
    let font = Font::acquire_role(host, FontRole::Ui)?;
    let drawn = draw_with(host, frame, &font, &display, browser, shown);
    font.release()?;
    drawn
}

fn draw_with(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    display: &snapshot::Snapshot,
    browser: &Browser,
    shown: bool,
) -> Result<Hits, Error> {
    let style = Style::read(host, display, font)?;
    let (width, height) = (display.display_width(), display.display_height());
    let layout = Layout::of(&style, width, height);
    let mut messages = Messages::new()?;
    let title = if browser.dir.is_empty() {
        messages.copy(host, "file-manager.device")?
    } else {
        text::copy(path::name(&browser.dir))?
    };
    let mut head = Page::default();
    let pages = browser.len().div_ceil(layout.rows).max(1);
    let page = (browser.cursor() / layout.rows).min(pages - 1);
    header::prepare(
        &mut head,
        host,
        font,
        &style,
        &mut messages,
        &title,
        width,
        display.has_touch(),
        page,
        pages,
    )?;
    let mut body: Vec<Paint> = Vec::new();
    let below = style.header;
    paint::push(
        &mut body,
        Paint::Fill(
            Rect::new(0, below, width, height.saturating_sub(below)),
            style.paper,
        ),
    )?;
    if browser.len() == 0 {
        let empty = messages.copy(host, "file-manager.empty")?;
        text::message(&mut body, font, &style, layout.area, &empty)?;
    } else {
        let mark = messages
            .copy(host, "core.decimal")
            .ok()
            .filter(|mark| !mark.is_empty())
            .map_or_else(|| text::copy("."), Ok)?;
        let on_list = shown && browser.bar().is_none();
        for (slot, entry) in browser.page().iter().enumerate().take(layout.rows) {
            let rect = Rect::new(
                0,
                layout
                    .area
                    .y
                    .saturating_add((slot as u16).saturating_mul(layout.row)),
                width,
                layout.row,
            );
            let value = if entry.is_dir {
                text::copy("›")?
            } else {
                let size = size_text(entry.size, &mark)?;
                messages.digits(host, &size)?
            };
            let focused = on_list && browser.first() + slot == browser.cursor();
            list::row(&mut body, font, &style, rect, &entry.name, &value, focused)?;
        }
    }
    if let Some(note) = &browser.note {
        let line = note_text(host, &mut messages, note)?;
        text::label(
            &mut body,
            font,
            &line,
            style.small,
            f32::from(style.margin),
            layout.note,
            style.note,
            None,
        )?;
    }
    let buttons = browser.buttons();
    draw_bar(
        host,
        &mut body,
        font,
        &style,
        &mut messages,
        &layout,
        browser,
        shown,
    )?;
    paint::replay(frame, font, head.paint.iter().chain(body.iter()))?;
    head.paint = Vec::new();
    Ok(Hits {
        layout: Some(layout),
        margin: style.margin,
        gap: style.gap,
        header: head,
        buttons: buttons.len(),
    })
}

/// The buttons along the bottom.
#[allow(clippy::too_many_arguments, reason = "the parts of one frame")]
fn draw_bar(
    host: &Host<'_>,
    body: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    messages: &mut Messages,
    layout: &Layout,
    browser: &Browser,
    shown: bool,
) -> Result<(), Error> {
    let buttons = browser.buttons();
    for (i, button) in buttons.iter().enumerate() {
        let label = messages.copy(
            host,
            match button {
                Button::NewFolder => "file-manager.new-folder",
                Button::Paste => "file-manager.paste",
                Button::Cancel => "file-manager.cancel",
            },
        )?;
        let rect = layout.button(style.margin, style.gap, i, buttons.len());
        let focused = shown && browser.bar() == Some(i);
        let ink = if focused { style.paper } else { style.ink };
        if focused {
            paint::push(body, Paint::Focus(rect, style.corner, None, style.ink))?;
        }
        paint::push(
            body,
            Paint::Focus(rect, style.corner, Some(u32::from(style.line)), style.ink),
        )?;
        let wide = font.measure_text(style.text, &label)?;
        let x = f32::from(rect.x) + (f32::from(rect.width) - wide).max(0.0) / 2.0;
        text::label(body, font, &label, style.text, x, rect, ink, Some(rect))?;
    }
    Ok(())
}

/// Whether the frame is the screen's own.
pub(crate) fn is_screen(frame: &DrawFrame<'_>) -> bool {
    matches!(frame.purpose, DrawPurpose::Screen)
}
