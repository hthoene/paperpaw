//! "Battery": the charge large beside a battery drawn to scale (lying or
//! standing) or inside a ring, a bolt while it charges.
//!
//! The battery plugin keeps how long the battery lasts to itself (no service
//! offers it), so no time left is shown.
use crate::native_battery_shapes::{battery, bolt, ring};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Point, Rect,
    native::{
        Error, Host,
        fonts::{Font, TextOrigin},
        settings::{self, SettingValue},
        snapshot::capture_snapshot,
    },
};
use paperpaw_ui_kit::widget::{
    Align, Cx, Look, TextSize,
    layout::{caption, caption_height},
    names::chosen_name,
    settings::Stack,
    text::{ink_baseline, largest_size},
};
use paperpaw_ui_kit::{Paint, geometry, paint, text};

/// The setting from which a charge counts as low, in percent.
const LOW: &str = "core.battery-low";
/// What it is without a setting.
const LOW_DEFAULT: i64 = 20;
/// How the battery is drawn: lying, standing, as a ring.
const STANDING: &str = "widgets.battery.standing";
const RING: &str = "widgets.battery.ring";
/// How wide the battery's body is for its height, lying (with the knob), in
/// hundredths.
const LYING: f32 = 212.0;

/// The height of the widget: the charge two titles large, the frame.
///
/// # Errors
/// Font refusals.
pub fn height(cx: &mut Cx<'_>, _width: u16) -> Result<u16, Error> {
    let style = cx.style();
    Ok(geometry::line_px(&cx.font, style.title * 2.0)?
        .saturating_add(style.gap)
        .saturating_add(cx.shape.frame_height()))
}

/// What the widget shows, to tell when it changed: the charge and whether it
/// charges; 1 where no battery is reported.
///
/// # Errors
/// Host refusals.
pub fn shows(host: &Host<'_>, _widget: &str) -> Result<u64, Error> {
    let snapshot = capture_snapshot(host)?;
    Ok(match snapshot.battery_percent() {
        Some(percent) => 2 + u64::from(percent.min(100)) + 1000 * u64::from(snapshot.is_charging()),
        None => 1,
    })
}

/// The charge as drawn: the number, a small percent sign after it, a bolt
/// before it while charging; measured as the font draws it.
struct Number<'a, 'f> {
    font: &'a Font<'f>,
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
    fn width(&self, size: f32) -> Result<f32, Error> {
        Ok(self.font.measure_text(size, &self.text)?
            + self.font.measure_text(size * 0.45, " %")?
            + size * self.bolt)
    }

    /// The height of a line's ascent at `size`, as drawn.
    fn ascent(&self, size: f32) -> Result<f32, Error> {
        Ok(geometry::line_metrics(self.font, size)?.0)
    }

    /// Where the digits' ink is at `size`: its top above the baseline
    /// (negative) and its height.
    fn ink(&self, size: f32) -> Result<(i32, i32), Error> {
        let (top, bottom) = match self.font.ink(size, &self.text)? {
            Some(ink) => geometry::check_ink(ink.top, ink.bottom)?,
            None => (-(self.ascent(size)? as i32), 0),
        };
        Ok((top, bottom - top))
    }

    /// The largest size from `max` down to the least at which `fits`.
    fn largest(
        &self,
        max: f32,
        fits: impl FnMut(f32) -> Result<bool, Error>,
    ) -> Result<Option<f32>, Error> {
        largest_size(max.max(self.least), self.least, fits)
    }
}

/// The charge at `size` from `x`, its digits' ink in the middle of `band` from
/// top to bottom: a bolt while charging, the number, the percent sign small
/// after it.
fn charge_text(
    out: &mut Vec<Paint>,
    look: &Look,
    number: &Number<'_, '_>,
    x: f32,
    (band, size): (Rect, f32),
) -> Result<(), Error> {
    let baseline = ink_baseline(number.font, size, &number.text, band)?;
    let (top, height) = number.ink(size)?;
    let mut x = x;
    if number.bolt > 0.0 {
        let h = (height as f32 * 1.15) as u16;
        let ink_top = baseline + top;
        let at = Rect::new(
            geometry::clamp_u16(x as i64),
            geometry::clamp_u16(i64::from(ink_top) - (i64::from(h) - i64::from(height)) / 2),
            (size * number.bolt * 0.75) as u16,
            h,
        );
        bolt(out, at, look.ink)?;
        x += size * number.bolt;
    }
    let digits = text::copy(&number.text)?;
    paint::push(
        out,
        Paint::Text {
            text: digits,
            size,
            origin: TextOrigin::new(x, baseline),
            color: look.ink,
            clip: None,
        },
    )?;
    let x = x + number.font.measure_text(size, &number.text)?;
    paint::push(
        out,
        Paint::Text {
            text: text::copy(" %")?,
            size: size * 0.45,
            origin: TextOrigin::new(x, baseline),
            color: look.faint,
            clip: None,
        },
    )
}

/// What the widget draws in one go.
struct Plan {
    /// The style the battery is drawn in.
    style: Stack<48>,
    /// Whether the number shows.
    with_text: bool,
    /// The size asked for, where it is not fitted.
    fixed: Option<f32>,
    align: Align,
    /// The most the number grows to: calm, not loud.
    most: f32,
    low: bool,
    percent: u8,
    line: u16,
    gap: u16,
    /// The small text size of the interface.
    small: f32,
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    // The snapshot does not tell the gray levels: a card is taken to be drawn
    // on a panel with grays.
    let look = cx.shape.paint(out, area, 256)?;
    let Some(percent) = cx.snapshot.battery_percent() else {
        let message = cx.tr("widgets.battery.unknown")?;
        return crate::native_draw::message_in(cx, out, &look, &message);
    };
    let percent = percent.min(100);
    let charging = cx.snapshot.is_charging() && cx.settings.shows("charging")?;
    let mut area = look.area;
    if cx.settings.shows("caption")? {
        let (small, gap) = (cx.style().small, cx.style().gap);
        if area.height >= 3 * caption_height(&cx.font, small, gap)? {
            let title = cx.tr("widgets.battery.caption")?;
            area = caption(
                out,
                &cx.font,
                (small, gap),
                (area, cx.settings.align()?, look.faint),
                &title,
            )?;
        }
    }
    let fixed = match cx.settings.size()? {
        TextSize::Fit => None,
        other => Some(other.text(cx.style()) * 1.4),
    };
    let plan = Plan {
        style: chosen_name(cx, "style")?,
        with_text: cx.settings.shows("percent")?,
        fixed,
        align: cx.settings.align()?,
        most: fixed.map_or(cx.style().title * 4.0, |f| f * 1.2),
        low: i64::from(percent) <= low_charge(cx.host)?,
        percent,
        line: cx.style().line,
        gap: cx.style().gap,
        small: cx.style().small,
    };
    let digits = cx.number(i64::from(percent))?;
    let reference = cx.style().text;
    let number = Number {
        font: &cx.font,
        text: digits,
        bolt: if charging { 0.55 } else { 0.0 },
        least: cx.style().small * 0.6,
        ratio: geometry::line_metrics(&cx.font, reference)?.0 / reference.max(1.0),
    };
    if plan.style.as_str() == RING {
        draw_ring(out, &look, &number, &plan, area)
    } else {
        draw_battery(out, &look, &number, &plan, area)
    }
}

/// The charge from which the battery counts as low.
fn low_charge(host: &Host<'_>) -> Result<i64, Error> {
    let mut bytes = [0; 192];
    Ok(match settings::read_setting(host, LOW, &mut bytes)? {
        SettingValue::Int(value) => value,
        _ => LOW_DEFAULT,
    })
}

/// A ring with the number inside; beside it where the ring is too small for
/// it and there is room.
fn draw_ring(
    out: &mut Vec<Paint>,
    look: &Look,
    number: &Number<'_, '_>,
    plan: &Plan,
    area: Rect,
) -> Result<(), Error> {
    let (w, h) = (f32::from(area.width), f32::from(area.height));
    let d = area.width.min(area.height).min((plan.most * 1.6) as u16);
    let radius = u32::from(d / 2);
    let thick = (radius / 7).max(u32::from(plan.line) * 2).max(2);
    let inner = radius.saturating_sub(thick + (radius / 8).max(1)) as f32;
    let inside = number
        .largest(plan.fixed.unwrap_or(inner * 0.9), |s| {
            Ok(number.width(s)? <= inner * 1.5 && number.ascent(s)? <= inner)
        })?
        .unwrap_or(0.0);
    // Beside: as large as the room right of the ring allows.
    let room = (w - f32::from(d + plan.gap)).max(0.0);
    let beside_size = number
        .largest(plan.fixed.unwrap_or(h * 0.55), |s| {
            Ok(number.width(s)? <= room)
        })?
        .unwrap_or(0.0);
    let beside = plan.with_text && inside < plan.small && beside_size > 0.0;
    if plan.with_text && !beside && inside == 0.0 {
        // Too small for a ring: the number alone.
        let Some(size) = number.largest(plan.most, |s| {
            Ok(number.width(s)? <= w && geometry::line_height(number.font, s)? <= h)
        })?
        else {
            return Ok(());
        };
        let x = plan.align.x(area, number.width(size)?);
        return charge_text(out, look, number, x, (area, size));
    }
    let total = if beside {
        d + plan.gap + number.width(beside_size)? as u16
    } else {
        d
    };
    let x = plan.align.x(area, f32::from(total.min(area.width))) as u16;
    let square = Rect::new(x, area.y + (area.height - d) / 2, d, d);
    let centre = Point::new(
        i32::from(square.x) + radius as i32,
        i32::from(square.y) + radius as i32,
    );
    ring(
        out,
        look.ink,
        (centre, radius, thick),
        plan.percent,
        plan.line,
    )?;
    if !plan.with_text {
        return Ok(());
    }
    if beside {
        let tx = f32::from(x + d + plan.gap);
        charge_text(out, look, number, tx, (square, beside_size))
    } else {
        let tx = centre.x as f32 - number.width(inside)? / 2.0;
        charge_text(out, look, number, tx, (square, inside))
    }
}

/// The battery with the number beside it or below it, whichever is larger.
fn draw_battery(
    out: &mut Vec<Paint>,
    look: &Look,
    number: &Number<'_, '_>,
    plan: &Plan,
    area: Rect,
) -> Result<(), Error> {
    let (w, h) = (f32::from(area.width), f32::from(area.height));
    let standing = plan.style.as_str() == STANDING;
    let gap = f32::from(plan.gap);
    // The battery's size: its height B (lying) or its width (standing), the
    // number as high as the battery beside it, or below it where the place is
    // high.
    let (bw, bh) = if standing {
        (100.0, 185.0)
    } else {
        (LYING, 100.0)
    };
    let text_size = |b: f32, share: f32| -> Result<f32, Error> {
        let wanted = plan.fixed.unwrap_or(b * share);
        Ok(number
            .largest(wanted, |s| {
                Ok(number.ascent(s)? <= number.ratio * wanted + 0.5)
            })?
            .unwrap_or(number.least))
    };
    let text_w = |b: f32, share: f32| -> Result<f32, Error> {
        if plan.with_text {
            number.width(text_size(b, share)?)
        } else {
            Ok(0.0)
        }
    };
    // Side by side: B from the height and from the width.
    let mut side = (h / (bh / 100.0)).min(plan.most);
    while side > 4.0 && side * bw / 100.0 + gap + text_w(side, 0.9)? > w {
        side *= 0.95;
    }
    // One above the other, the number's ink half a gap below the battery.
    let mut stacked = (w / (bw / 100.0)).min(plan.most);
    while stacked > 4.0 {
        let text_h = if plan.with_text {
            number.ascent(text_size(stacked, 0.6)?)?
        } else {
            0.0
        };
        if stacked * bh / 100.0 + gap / 2.0 + text_h <= h && text_w(stacked, 0.6)? <= w {
            break;
        }
        stacked *= 0.95;
    }
    let beside = side >= stacked || !plan.with_text;
    let b = if beside { side } else { stacked };
    let body = ((b * bw / 100.0) as u16, (b * bh / 100.0).min(h) as u16);
    let flags = (plan.percent, plan.low, standing);
    if beside {
        let size = text_size(b, 0.9)?;
        let tw = if plan.with_text {
            number.width(size)? as u16
        } else {
            0
        };
        let total = body.0 + if plan.with_text { plan.gap + tw } else { 0 };
        let x = plan.align.x(area, f32::from(total.min(area.width))) as u16;
        let y = area.y + (area.height - body.1) / 2;
        let at = Rect::new(x, y, body.0, body.1);
        battery(out, (look.ink, look.paper), at, flags, plan.line)?;
        if plan.with_text {
            charge_text(
                out,
                look,
                number,
                f32::from(x + body.0 + plan.gap),
                (at, size),
            )?;
        }
        return Ok(());
    }
    // The battery and the number's ink together in the middle.
    let size = text_size(b, 0.6)?;
    let ink = if plan.with_text {
        number.ink(size)?.1 as u16
    } else {
        0
    };
    let below = if plan.with_text {
        plan.gap / 2 + ink
    } else {
        0
    };
    let total = (body.1 + below).min(area.height);
    let top = area.y + (area.height - total) / 2;
    let x = plan.align.x(area, f32::from(body.0)) as u16;
    battery(
        out,
        (look.ink, look.paper),
        Rect::new(x, top, body.0, body.1),
        flags,
        plan.line,
    )?;
    if plan.with_text {
        let tx = plan.align.x(area, number.width(size)?);
        let band = Rect::new(area.x, top + body.1 + plan.gap / 2, area.width, ink);
        charge_text(out, look, number, tx, (band, size))?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::{LOW, LOW_DEFAULT};
    use paperpaw_core::Kernel;
    use paperpaw_platform::{
        DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
    };

    /// The charge from which the battery counts as low, where nobody set one,
    /// is the host's own default.
    #[test]
    fn a_low_charge_is_what_the_host_says_without_a_setting() {
        let device = DeviceInfo::new(
            "test",
            "Test device",
            DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
            InputCaps::new(true, false, false, &[]),
            MemoryClass::Small,
        );
        let mut kernel = Kernel::builder(device).build().unwrap();
        kernel.start(Instant(0));
        assert_eq!(kernel.settings().int(LOW), LOW_DEFAULT);
    }
}
