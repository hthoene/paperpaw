//! What the host's screens draw with: the sizes of the reader's interface
//! settings ([`Look`]) and a few parts (a line, a block of wrapped lines, a
//! bar, a button), each said to a describing canvas. The kernel's fonts and
//! the canvas alone, no SDK kit (#293).
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_graphics::{Canvas, Role};
use paperpaw_platform::{Color, DeviceInfo, Rect};
use paperpaw_sdk::builtin::{fonts, settings};
use paperpaw_sdk::{FontHandle, Fonts, Metrics, Settings};

/// The sizes the host's screens are drawn in, from the reader's interface
/// settings (text size, rows, margins, lines, corners).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Look {
    pub(crate) text: f32,
    pub(crate) small: f32,
    pub(crate) title: f32,
    pub(crate) row: u16,
    pub(crate) margin: u16,
    pub(crate) gap: u16,
    pub(crate) line: u32,
    pub(crate) corner: u32,
}

impl Look {
    pub(crate) fn of(device: &DeviceInfo, settings: &Settings, fonts: &Fonts) -> Self {
        let m = Metrics::new(device.display.dpi);
        let base = settings.read(&settings::UI_SIZE).clamp(6, 40) as f32;
        let text = m.pt(base);
        let line_height = fonts
            .role(fonts::UI.as_str())
            .map_or(text * 1.25, |font| font.line_metrics(text).line_height);
        let rows = settings.read(&settings::UI_ROW).clamp(120, 300) as f32 / 100.0;
        let finger = if device.input.touch { m.mm(7) } else { 0 };
        let corner = match settings::CORNERS
            .iter()
            .position(|c| *c == settings.text(settings::UI_CORNERS.as_str()))
        {
            Some(1) => m.tenth_mm(8),
            Some(2) => m.mm(2),
            Some(3) => m.tenth_mm(35),
            _ => 0,
        };
        Self {
            text,
            small: m.pt(base * 0.75),
            title: m.pt(base * 1.15),
            row: px(((line_height * rows) as u32).max(finger)),
            margin: px(m.mm(settings.read(&settings::UI_MARGIN).clamp(0, 15) as u32)),
            gap: px(m.mm(2)),
            line: m
                .tenth_mm(settings.read(&settings::UI_LINES).clamp(1, 10) as u32)
                .max(1),
            corner,
        }
    }

    /// The width of a button's frame and of a bar's.
    pub(crate) fn border(&self) -> u32 {
        self.line.max(2)
    }
}

fn px(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// The interface font.
pub(crate) fn font(fonts: &Fonts) -> Option<FontHandle<'_>> {
    fonts.role(fonts::UI.as_str())
}

/// Where the baseline of a line at `size` sits in the middle of `row`.
fn baseline(font: &FontHandle<'_>, size: f32, row: Rect) -> i32 {
    let metrics = font.line_metrics(size);
    i32::from(row.y) + ((f32::from(row.height) + metrics.ascent - metrics.descent) / 2.0) as i32
}

/// `text` shortened with "…" to `width`.
pub(crate) fn fit(font: &FontHandle<'_>, size: f32, text: &str, width: f32) -> String {
    let mut fitted = String::from(text);
    if font.measure(size, text) <= width {
        return fitted;
    }
    while let Some(last) = fitted.pop() {
        let _ = last;
        fitted.push('…');
        if font.measure(size, &fitted) <= width {
            return fitted;
        }
        fitted.pop();
    }
    fitted
}

/// `text` on one line in the middle of `area`, shortened where it does not
/// fit.
pub(crate) fn line(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    size: f32,
    text: &str,
    area: Rect,
    color: Color,
) {
    canvas.describe(Role::Text, text, area);
    let room = f32::from(area.width);
    let fitted;
    let shown = if font.measure(size, text) > room + 1.0 {
        fitted = fit(font, size, text, room);
        fitted.as_str()
    } else {
        text
    };
    let width = font.measure(size, shown);
    let x = f32::from(area.x) + ((room - width) / 2.0).max(0.0);
    font.draw(canvas, size, x, baseline(font, size, area), shown, color);
}

/// `text` wrapped to the width of `area`, each line centred, the block in
/// the middle of its height; as many lines as it holds, the last shortened.
pub(crate) fn lines(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    size: f32,
    text: &str,
    area: Rect,
    color: Color,
) {
    let height = font.line_metrics(size).line_height;
    let mut ranges = font.wrap(size, text, f32::from(area.width).max(1.0));
    let fits = ((f32::from(area.height) / height) as usize).max(1);
    let cut = ranges.len() > fits;
    ranges.truncate(fits);
    let block = height * ranges.len().max(1) as f32;
    let top = f32::from(area.y) + (f32::from(area.height) - block).max(0.0) / 2.0;
    canvas.describe(
        Role::Text,
        text,
        Rect::new(area.x, top as u16, area.width, block as u16),
    );
    let count = ranges.len();
    for (n, range) in ranges.into_iter().enumerate() {
        let row = Rect::new(
            area.x,
            (top + height * n as f32) as u16,
            area.width,
            height as u16,
        );
        let part = if cut && n + 1 == count {
            text.get(range.start..).unwrap_or_default()
        } else {
            text.get(range).unwrap_or_default()
        };
        let fitted = fit(font, size, part.trim(), f32::from(area.width));
        let width = font.measure(size, &fitted);
        let x = f32::from(row.x) + ((f32::from(row.width) - width) / 2.0).max(0.0);
        font.draw(canvas, size, x, baseline(font, size, row), &fitted, color);
    }
}

/// The lines `text` wraps to at `size` in `width`, each its own string.
pub(crate) fn wrapped(font: &FontHandle<'_>, size: f32, text: &str, width: f32) -> Vec<String> {
    font.wrap(size, text, width.max(1.0))
        .into_iter()
        .filter_map(|range| text.get(range))
        .map(|line| String::from(line.trim()))
        .collect()
}

/// A bar `permille` thousandths full: a frame with the look's corners, filled
/// inside as far as it has come.
pub(crate) fn bar(canvas: &mut Canvas<'_>, look: &Look, area: Rect, permille: u16, ink: Color) {
    let done = permille.min(1000);
    if canvas.is_describing() {
        let said = alloc::format!("{}%", done / 10);
        canvas.describe(Role::Progress, &said, area);
    }
    let border = look.border();
    let radius = look.corner.min(u32::from(area.height) / 2);
    canvas.stroke_round(area, border, radius, ink);
    let inset = u16::try_from(border * 2).unwrap_or(u16::MAX);
    let inner = Rect::new(
        area.x.saturating_add(inset),
        area.y.saturating_add(inset),
        area.width.saturating_sub(2 * inset),
        area.height.saturating_sub(2 * inset),
    );
    let filled = (u32::from(inner.width) * u32::from(done) / 1000) as u16;
    if filled == 0 || inner.height == 0 {
        return;
    }
    let mut cut = canvas.clipped(Rect::new(inner.x, inner.y, filled, inner.height));
    cut.fill_round(inner, radius.saturating_sub(u32::from(inset)), ink);
}

/// A button at `area` saying `label`: framed, or filled black with white
/// words where it has the focus.
pub(crate) fn button(
    canvas: &mut Canvas<'_>,
    font: Option<&FontHandle<'_>>,
    look: &Look,
    area: Rect,
    label: &str,
    focused: bool,
) {
    canvas.describe(Role::Button, label, area);
    let radius = look.corner.min(u32::from(area.height) / 2);
    let ink = if focused {
        canvas.fill_round(area, radius, Color::BLACK);
        Color::WHITE
    } else {
        canvas.stroke_round(area, look.border(), radius, Color::BLACK);
        Color::BLACK
    };
    if let Some(font) = font {
        let inner = area.inset(look.gap.min(area.width / 4), 0);
        let fitted = fit(font, look.small, label, f32::from(inner.width));
        let width = font.measure(look.small, &fitted);
        let x = f32::from(inner.x) + ((f32::from(inner.width) - width) / 2.0).max(0.0);
        font.draw(
            canvas,
            look.small,
            x,
            baseline(font, look.small, area),
            &fitted,
            ink,
        );
    }
}
