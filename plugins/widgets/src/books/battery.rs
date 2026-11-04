//! "Battery": the charge large beside a battery drawn to scale (lying or
//! standing) or inside a ring, a bolt while it charges.
//!
//! The battery plugin keeps how long the battery lasts to itself (no
//! service offers it), so no time left is shown.

use alloc::boxed::Box;
use alloc::string::String;

use paperpaw_sdk::builtin::settings::BATTERY_LOW;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{
    self, Align, FixedWidget, Frame, Padding, TextSize, Widget, WidgetLook,
};
use paperpaw_sdk::{Canvas, Color, Env, FontHandle, Id, Point, Rect, RegisterError, Registrar};

use super::{add_settings, caption, chosen, draw_message, draw_ring, frame_height};
use paperpaw_sdk::SettingSpec;

pub(crate) const ID: Id = Id::new_static(WIDGET);
/// Its id, as text.
const WIDGET: &str = "widgets.battery";
/// How the battery is drawn: lying, standing, as a ring.
const STYLES: [&str; 3] = [
    "widgets.battery.lying",
    "widgets.battery.standing",
    "widgets.battery.ring",
];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 13] = [
    widgets::align_spec("widgets.battery.align", ID, Align::Center),
    widgets::frame_spec("widgets.battery.frame", ID, Frame::None),
    widgets::padding_spec("widgets.battery.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.battery.paper", ID),
    widgets::ink_spec("widgets.battery.ink", ID),
    widgets::corners_spec("widgets.battery.corners", ID),
    widgets::frame_line_spec("widgets.battery.frame-line", ID),
    widgets::shadow_spec("widgets.battery.drop-shadow", ID),
    widgets::size_spec("widgets.battery.size", ID, TextSize::Fit, true),
    SettingSpec::choice(
        "widgets.battery.style",
        "widgets.books.setting.style",
        &STYLES,
        STYLES[0],
        ID,
    )
    .description("widgets.battery.style.help"),
    widgets::part_spec(
        "widgets.battery.percent",
        ID,
        "widgets.books.part.percent",
        true,
    ),
    widgets::part_spec(
        "widgets.battery.charging",
        ID,
        "widgets.books.part.charging",
        true,
    ),
    widgets::part_spec(
        "widgets.battery.caption",
        ID,
        "widgets.books.part.caption",
        false,
    ),
];

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    add_settings(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.battery");
            &SPEC
        },
        &SETTINGS,
    )?;
    static WIDGET: [FixedWidget; 1] =
        [FixedWidget::new(
            ID,
            "widgets.battery",
            |_| Box::new(BatteryWidget::default()),
        )
        .category(widgets::categories::DEVICE)];
    r.add_widgets(&WIDGET)
}

/// The charge in percent and whether it charges, as last read.
#[derive(Debug, Default)]
struct BatteryWidget {
    charge: Option<(u8, bool)>,
}

impl Widget for BatteryWidget {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let charge = env
            .status()
            .battery
            .map(|b| (b.percent.min(100), b.charging));
        let changed = charge != self.charge;
        self.charge = charge;
        changed
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        widgets::line_height(env, style.title() * 2.0) + style.gap() + frame_height(env, WIDGET)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, WIDGET, area);
        match self.charge {
            Some((percent, charging)) => draw(canvas, env, &look, percent, charging),
            None => draw_message(canvas, env, &look, "widgets.battery.unknown"),
        }
    }
}

/// How wide the battery's body is for its height, lying (with the
/// knob), in hundredths.
const LYING: u32 = 212;

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &WidgetLook, percent: u8, charging: bool) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let id = WIDGET;
    let area = caption(
        canvas,
        env,
        id,
        look,
        env.i18n().tr("widgets.battery.caption"),
    );
    let gap = style.gap();
    let charging = charging && widgets::shows(env, id, "charging");
    let low = i64::from(percent) <= env.settings().read(&BATTERY_LOW);
    let number = Number {
        font: &font,
        text: env.i18n().number(i64::from(percent)),
        bolt: if charging { 0.55 } else { 0.0 },
        least: style.small() * 0.6,
        ratio: font.line_metrics(style.text()).ascent / style.text().max(1.0),
    };
    let with_text = widgets::shows(env, id, "percent");
    let align = widgets::align(env, id);
    let fixed = match widgets::size(env, id) {
        TextSize::Fit => None,
        other => Some(other.text(&style) * 1.4),
    };
    let (w, h) = (f32::from(area.width), f32::from(area.height));
    let style_value = chosen(env, id, "style");
    // Not larger than this, however large the place: calm, not loud.
    let most = fixed.map_or(style.title() * 4.0, |f| f * 1.2);
    if style_value == STYLES[2] {
        // A ring with the number inside; beside it where the ring is
        // too small for it and there is room.
        let d = area.width.min(area.height).min((most * 1.6) as u16);
        let radius = u32::from(d / 2);
        let thick = (radius / 7).max(u32::from(style.line()) * 2).max(2);
        let inner = radius.saturating_sub(thick + (radius / 8).max(1)) as f32;
        let inside = number
            .largest(fixed.unwrap_or(inner * 0.9), |s| {
                number.width(s) <= inner * 1.5 && number.ascent(s) <= inner
            })
            .unwrap_or(0.0);
        // Beside: as large as the room right of the ring allows.
        let room = (w - f32::from(d + gap)).max(0.0);
        let beside_size = number
            .largest(fixed.unwrap_or(h * 0.55), |s| number.width(s) <= room)
            .unwrap_or(0.0);
        let beside = with_text && inside < style.small() && beside_size > 0.0;
        if with_text && !beside && inside == 0.0 {
            // Too small for a ring: the number alone.
            let Some(size) = number.largest(most, |s| {
                number.width(s) <= w && f32::from(widgets::line_height(env, s)) <= h
            }) else {
                return;
            };
            let tx = align.x(area, number.width(size));
            draw_charge_text(canvas, look, &number, tx, area, size);
            return;
        }
        let total = if beside {
            d + gap + number.width(beside_size) as u16
        } else {
            d
        };
        let x = super::place_x(area, total.min(area.width), align);
        let ring = Rect::new(x, area.y + (area.height - d) / 2, d, d);
        let center = Point::new(
            i32::from(ring.x) + radius as i32,
            i32::from(ring.y) + radius as i32,
        );
        draw_ring(canvas, env, look, center, radius, thick, percent);
        if !with_text {
            return;
        }
        if beside {
            let tx = f32::from(x + d + gap);
            draw_charge_text(canvas, look, &number, tx, ring, beside_size);
        } else {
            let tx = center.x as f32 - number.width(inside) / 2.0;
            draw_charge_text(canvas, look, &number, tx, ring, inside);
        }
        return;
    }
    let standing = style_value == STYLES[1];
    // The battery's size: its height B (lying) or its width (standing),
    // the number as high as the battery beside it, or below it where the
    // place is high. A font of a few sizes may draw it smaller.
    let (bw, bh) = if standing {
        (100.0, 185.0)
    } else {
        (LYING as f32, 100.0)
    };
    let text_size = |b: f32, share: f32| -> f32 {
        let wanted = fixed.unwrap_or(b * share);
        number
            .largest(wanted, |s| number.ascent(s) <= number.ratio * wanted + 0.5)
            .unwrap_or(number.least)
    };
    let text_w = |b: f32, share: f32| -> f32 {
        if with_text {
            number.width(text_size(b, share))
        } else {
            0.0
        }
    };
    // Side by side: B from the height and from the width.
    let side = {
        let mut b = (h / (bh / 100.0)).min(most);
        while b > 4.0 && b * bw / 100.0 + f32::from(gap) + text_w(b, 0.9) > w {
            b *= 0.95;
        }
        b
    };
    // One above the other, the number's ink half a gap below the battery.
    let stacked = {
        let mut b = (w / (bw / 100.0)).min(most);
        let text_h = |b: f32| {
            if with_text {
                number.ascent(text_size(b, 0.6))
            } else {
                0.0
            }
        };
        while b > 4.0 && (b * bh / 100.0 + f32::from(gap / 2) + text_h(b) > h || text_w(b, 0.6) > w)
        {
            b *= 0.95;
        }
        b
    };
    let beside = side >= stacked || !with_text;
    let b = if beside { side } else { stacked };
    let body = ((b * bw / 100.0) as u16, (b * bh / 100.0).min(h) as u16);
    if beside {
        let size = text_size(b, 0.9);
        let tw = if with_text {
            number.width(size) as u16
        } else {
            0
        };
        let total = body.0 + if with_text { gap + tw } else { 0 };
        let x = super::place_x(area, total.min(area.width), align);
        let y = area.y + (area.height - body.1) / 2;
        let at = Rect::new(x, y, body.0, body.1);
        draw_battery(canvas, env, look, at, percent, low, standing);
        if with_text {
            let tx = f32::from(x + body.0 + gap);
            draw_charge_text(canvas, look, &number, tx, at, size);
        }
    } else {
        // The battery and the number's ink together in the middle.
        let size = text_size(b, 0.6);
        let ink = if with_text {
            number.ink(size).1 as u16
        } else {
            0
        };
        let below = if with_text { gap / 2 + ink } else { 0 };
        let total = (body.1 + below).min(area.height);
        let top = area.y + (area.height - total) / 2;
        let x = super::place_x(area, body.0, align);
        draw_battery(
            canvas,
            env,
            look,
            Rect::new(x, top, body.0, body.1),
            percent,
            low,
            standing,
        );
        if with_text {
            let tx = align.x(area, number.width(size));
            let band = Rect::new(area.x, top + body.1 + gap / 2, area.width, ink);
            draw_charge_text(canvas, look, &number, tx, band, size);
        }
    }
}

/// The charge as drawn: the number, a small percent sign after it, a
/// bolt before it while charging; measured as the font draws it.
struct Number<'a, 'f> {
    font: &'a FontHandle<'f>,
    text: String,
    /// The bolt's width, in sizes (0 without one).
    bolt: f32,
    /// The least size it is drawn at.
    least: f32,
    /// The font's ascent for its size, where it draws sizes as asked.
    ratio: f32,
}

impl Number<'_, '_> {
    /// Its width at `size`, the bolt and the sign included.
    fn width(&self, size: f32) -> f32 {
        self.font.measure(size, &self.text)
            + self.font.measure(size * 0.45, " %")
            + size * self.bolt
    }

    /// The height of a line's ascent at `size`, as drawn.
    fn ascent(&self, size: f32) -> f32 {
        self.font.line_metrics(size).ascent
    }

    /// Where the digits' ink is at `size`: its top above the baseline
    /// (negative) and its height.
    fn ink(&self, size: f32) -> (i32, i32) {
        let (top, bottom) = self.font.ink(size, &self.text).unwrap_or_else(|| {
            let metrics = self.font.line_metrics(size);
            (-(metrics.ascent as i32), 0)
        });
        (top, bottom - top)
    }

    /// The largest size from `max` down to the least at which `fits`.
    fn largest(&self, max: f32, fits: impl FnMut(f32) -> bool) -> Option<f32> {
        widgets::largest_size(max.max(self.least), self.least, fits)
    }
}

/// The charge at `size` from `x`, its digits' ink in the middle of
/// `band` from top to bottom: a bolt while charging, the number, the
/// percent sign small after it.
fn draw_charge_text(
    canvas: &mut Canvas<'_>,
    look: &WidgetLook,
    number: &Number<'_, '_>,
    x: f32,
    band: Rect,
    size: f32,
) {
    let font = number.font;
    let baseline = widgets::ink_baseline(font, size, &number.text, band);
    let (top, height) = number.ink(size);
    let mut x = x;
    if number.bolt > 0.0 {
        let h = (height as f32 * 1.15) as u16;
        let ink_top = baseline + top;
        let at = Rect::new(
            x as u16,
            (ink_top - (i32::from(h) - height) / 2).max(0) as u16,
            (size * number.bolt * 0.75) as u16,
            h,
        );
        draw_bolt(canvas, at, look.ink);
        x += size * number.bolt;
    }
    font.draw(canvas, size, x, baseline, &number.text, look.ink);
    let x = x + font.measure(size, &number.text);
    font.draw(canvas, size * 0.45, x, baseline, " %", look.faint);
}

/// A battery drawn in `body`: rounded, with its knob (to the right, or
/// on top where standing), filled as far as charged.
fn draw_battery(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &WidgetLook,
    body: Rect,
    percent: u8,
    low: bool,
    standing: bool,
) {
    let short = body.width.min(body.height);
    if short < 6 {
        return;
    }
    let style = UiStyle::new(env);
    let knob = (short * 12 / 100).max(2);
    let (shell, knob_at) = if standing {
        let shell = Rect::new(body.x, body.y + knob, body.width, body.height - knob);
        let w = body.width * 45 / 100;
        (
            shell,
            Rect::new(body.x + (body.width - w) / 2, body.y, w, knob + 1),
        )
    } else {
        let shell = Rect::new(body.x, body.y, body.width - knob, body.height);
        let h = body.height * 45 / 100;
        (
            shell,
            Rect::new(
                body.x + body.width - knob - 1,
                body.y + (body.height - h) / 2,
                knob + 1,
                h,
            ),
        )
    };
    let line = u32::from(short / 11)
        .max(u32::from(style.line()) * 2)
        .max(2);
    let radius = u32::from(short / 5);
    canvas.fill_round(knob_at, u32::from(knob / 2), look.ink);
    canvas.fill_round(shell, radius, look.paper);
    canvas.stroke_round(shell, line, radius, look.ink);
    // The charge, inside with a little room.
    let pad = line as u16 + (short / 14).max(1);
    let inner = Rect::new(
        shell.x + pad,
        shell.y + pad,
        shell.width.saturating_sub(2 * pad),
        shell.height.saturating_sub(2 * pad),
    );
    let percent = u32::from(percent.min(100));
    let level = if standing {
        let h = (u32::from(inner.height) * percent / 100) as u16;
        Rect::new(inner.x, inner.y + inner.height - h, inner.width, h)
    } else {
        let w = (u32::from(inner.width) * percent / 100) as u16;
        Rect::new(inner.x, inner.y, w, inner.height)
    };
    if level.width > 0 && level.height > 0 {
        let radius = radius.saturating_sub(u32::from(pad)).max(1);
        if low {
            // Low: shaded, so the outline stands out with little in it.
            canvas.fill_round_shaded(level, radius, Color::gray(110));
        } else {
            canvas.fill_round(level, radius, look.ink);
        }
    }
}

/// A bolt filling `at`: two triangles meeting in the middle.
pub(crate) fn draw_bolt(canvas: &mut Canvas<'_>, at: Rect, color: Color) {
    let (x, y) = (i32::from(at.x), i32::from(at.y));
    let (w, h) = (i32::from(at.width), i32::from(at.height));
    if w < 3 || h < 4 {
        return;
    }
    let p = |fx: i32, fy: i32| Point::new(x + w * fx / 100, y + h * fy / 100);
    fill_triangle(canvas, p(70, 0), p(10, 58), p(56, 58), color);
    fill_triangle(canvas, p(44, 42), p(90, 42), p(30, 100), color);
}

/// Fills the triangle `a`, `b`, `c` row by row.
fn fill_triangle(canvas: &mut Canvas<'_>, a: Point, b: Point, c: Point, color: Color) {
    let top = a.y.min(b.y).min(c.y);
    let bottom = a.y.max(b.y).max(c.y);
    let edges = [(a, b), (b, c), (c, a)];
    for y in top..=bottom {
        // Where the edges cross the middle of the row, in 256ths.
        let cy = y * 256 + 128;
        let (mut left, mut right) = (i32::MAX, i32::MIN);
        for (p, q) in edges {
            let (py, qy) = (p.y * 256, q.y * 256);
            if (py <= cy && cy < qy) || (qy <= cy && cy < py) {
                let x = p.x * 256 + (q.x - p.x) * 256 * (cy - py) / (qy - py);
                left = left.min(x);
                right = right.max(x);
            }
        }
        if left <= right {
            let (l, r) = ((left + 128) / 256, (right + 128) / 256);
            if r > l {
                canvas.fill_rect(l, y, (r - l) as u32, 1, color);
            }
        }
    }
}
