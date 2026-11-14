//! A bar chart for e-ink as prepared drawing: black bars on a thin base line, a
//! label below each, the value of the highest above it. The layout is plain
//! numbers ([`plan`]); [`bars`] measures the words and turns the plan into
//! [`Paint`], so nothing here holds a font or a canvas.

use alloc::vec::Vec;

use paperpaw_sdk::Rect;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::fonts::Font;
use paperpaw_ui_kit::{Paint, Style, paint, text};

/// Bars are never wider than those of a week.
const MIN_SLOTS: usize = 7;

/// One bar's place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Bar {
    /// The left edge of the bar and its width.
    pub x: f32,
    pub width: f32,
    /// The bar's height in pixels (0: nothing is drawn).
    pub height: i32,
    /// Whether it is the highest.
    pub highest: bool,
    /// Whether its label is shown.
    pub labelled: bool,
}

/// Where everything of a chart goes.
#[derive(Debug, PartialEq)]
pub(crate) struct Plan {
    /// The room for labels above and below the bars.
    pub label_height: u16,
    /// The area of the bars; the base line is at its bottom.
    pub chart: Rect,
    /// The base line's thickness.
    pub line: u16,
    pub bars: Vec<Bar>,
}

/// The layout of `count` values in `area`: `widest` is the widest label and
/// `small` the label size. A few values keep the width of a week's bars, at
/// the right (the newest); labels wider than their bar's room are shown for
/// every other bar, counted from the last.
///
/// # Errors
/// [`Error::NoMemory`] when the bars cannot be listed.
pub(crate) fn plan(
    area: Rect,
    values: &[u32],
    widest: f32,
    small: f32,
    line: u16,
) -> Result<Plan, Error> {
    let label_height = (small * 1.4) as u16;
    let chart = Rect::new(
        area.x,
        area.y.saturating_add(label_height),
        area.width,
        area.height.saturating_sub(label_height.saturating_mul(2)),
    );
    let line = line.max(1);
    let max = values.iter().copied().max().unwrap_or(0).max(1);
    let slots = values.len().max(MIN_SLOTS);
    let slot = f32::from(chart.width) / slots as f32;
    let shift = slot * (slots - values.len().min(slots)) as f32;
    let width = (slot * 0.6).max(2.0);
    let every = if widest > slot * 0.9 { 2 } else { 1 };
    let mut bars = Vec::new();
    bars.try_reserve_exact(values.len())
        .map_err(|_| Error::NoMemory)?;
    for (at, &value) in values.iter().enumerate() {
        let high = (f32::from(chart.height) * value as f32 / max as f32) as i32;
        bars.push(Bar {
            x: f32::from(chart.x) + shift + slot * at as f32 + (slot - width) / 2.0,
            width,
            // At least visible.
            height: if value > 0 {
                high.max(i32::from(line) * 2)
            } else {
                0
            },
            highest: value > 0 && value == max,
            labelled: (values.len() - 1 - at).is_multiple_of(every),
        });
    }
    Ok(Plan {
        label_height,
        chart,
        line,
        bars,
    })
}

/// Prepares `values` as bars in `area` with `labels` below them; the bar
/// `highlight` (e.g. today) is framed. `top` labels the highest bar.
///
/// # Errors
/// Propagates allocation and font errors.
pub(crate) fn bars(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    area: Rect,
    values: &[u32],
    labels: &[&str],
    (highlight, top): (Option<usize>, &str),
) -> Result<(), Error> {
    if values.is_empty() {
        return Ok(());
    }
    let mut widest = 0.0f32;
    for label in labels {
        widest = widest.max(font.measure_text(style.small, label)?);
    }
    let plan = plan(area, values, widest, style.small, style.line)?;
    let base = i32::from(plan.chart.y) + i32::from(plan.chart.height);
    let line = plan.line;
    paint::push(
        out,
        Paint::Fill(
            Rect::new(plan.chart.x, clamp(base), plan.chart.width, line),
            paperpaw_sdk::Color::gray(150),
        ),
    )?;
    for (at, bar) in plan.bars.iter().enumerate() {
        let x = bar.x;
        if bar.height > 0 {
            paint::push(
                out,
                Paint::Fill(
                    Rect::new(
                        clamp(x as i32),
                        clamp(base - bar.height),
                        bar.width as u16,
                        bar.height as u16,
                    ),
                    paperpaw_sdk::Color::BLACK,
                ),
            )?;
            if bar.highest && !top.is_empty() {
                let w = font.measure_text(style.small, top)?;
                let left = f32::from(area.x);
                let right = f32::from(area.x) + f32::from(area.width);
                // Not `f32::clamp`: its panic message formats floats, which brings core's 2,048 B
                // float formatter into the plugin (the S3 packer refuses it, #263).
                let at_x = (x + bar.width / 2.0 - w / 2.0).max(left).min((right - w).max(left));
                let row = Rect::new(
                    area.x,
                    clamp(base - bar.height - i32::from(plan.label_height)),
                    area.width,
                    plan.label_height,
                );
                text::label(out, font, top, style.small, at_x, row, style.note, None)?;
            }
        }
        if highlight == Some(at) {
            let pad = (bar.width / 2.0 / 2.0).max(0.0);
            paint::push(
                out,
                Paint::Focus(
                    Rect::new(
                        clamp((x - pad) as i32),
                        plan.chart.y,
                        (bar.width + 2.0 * pad) as u16,
                        plan.chart.height.saturating_add(line),
                    ),
                    0,
                    Some(u32::from(line)),
                    paperpaw_sdk::Color::gray(120),
                ),
            )?;
        }
        if let Some(label) = labels.get(at).filter(|_| bar.labelled) {
            let w = font.measure_text(style.small, label)?;
            let row = Rect::new(
                area.x,
                clamp(base + i32::from(line)),
                area.width,
                plan.label_height,
            );
            let color = if highlight == Some(at) {
                paperpaw_sdk::Color::BLACK
            } else {
                style.muted
            };
            text::label(
                out,
                font,
                label,
                style.small,
                x + bar.width / 2.0 - w / 2.0,
                row,
                color,
                None,
            )?;
        }
    }
    Ok(())
}

fn clamp(value: i32) -> u16 {
    value.clamp(0, i32::from(u16::MAX)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::new(10, 100, 280, 180)
    }

    #[test]
    fn a_week_fills_the_width_and_the_highest_is_marked() {
        let plan = plan(area(), &[0, 60, 120, 0, 30, 0, 90], 20.0, 14.0, 2).unwrap();
        assert_eq!(plan.label_height, 19);
        assert_eq!(plan.chart, Rect::new(10, 119, 280, 142));
        assert_eq!(plan.bars.len(), 7);
        assert_eq!(plan.bars[0].height, 0, "a day of nothing has no bar");
        assert!(!plan.bars[0].highest);
        assert!(plan.bars[2].highest);
        assert_eq!(plan.bars[2].height, 142);
        assert_eq!(plan.bars[1].height, 71);
        // 40 px a slot, bars 24 px wide, centred.
        assert_eq!(plan.bars[0].x, 18.0);
        assert_eq!(plan.bars[1].x, 58.0);
        assert!(plan.bars.iter().all(|bar| bar.labelled));
    }

    #[test]
    fn a_few_values_keep_the_width_of_a_week_at_the_right() {
        let plan = plan(area(), &[10, 20], 20.0, 14.0, 2).unwrap();
        assert_eq!(plan.bars[0].x, 10.0 + 40.0 * 5.0 + 8.0);
        assert_eq!(plan.bars[1].x, 10.0 + 40.0 * 6.0 + 8.0);
        assert_eq!(plan.bars[0].width, 24.0);
    }

    #[test]
    fn a_small_value_is_still_seen() {
        let plan = plan(area(), &[1, 100_000], 20.0, 14.0, 2).unwrap();
        assert_eq!(plan.bars[0].height, 4, "twice the line");
        assert!(!plan.bars[0].highest);
    }

    #[test]
    fn wide_labels_are_shown_for_every_other_bar_from_the_last() {
        let values = [1; 14];
        let plan = plan(area(), &values, 30.0, 14.0, 2).unwrap();
        // 20 px a slot, 30 px labels.
        let shown: Vec<bool> = plan.bars.iter().map(|b| b.labelled).collect();
        assert!(shown[13] && !shown[12] && shown[11]);
        assert_eq!(shown.iter().filter(|s| **s).count(), 7);
    }

    #[test]
    fn nothing_read_makes_no_bars_and_no_division_by_zero() {
        let plan = plan(area(), &[0, 0, 0], 20.0, 14.0, 0).unwrap();
        assert!(plan.bars.iter().all(|bar| bar.height == 0 && !bar.highest));
        assert_eq!(plan.line, 1);
        let empty = plan_of_none();
        assert!(empty.bars.is_empty());
    }

    fn plan_of_none() -> Plan {
        plan(Rect::new(0, 0, 0, 0), &[], 0.0, 14.0, 2).unwrap()
    }

    #[test]
    fn a_tiny_area_does_not_wrap_around() {
        let plan = plan(Rect::new(0, 0, 3, 10), &[5, 9], 4.0, 40.0, 2).unwrap();
        assert_eq!(plan.chart.height, 0);
        assert!(plan.bars.iter().all(|bar| bar.height >= 0));
    }
}
