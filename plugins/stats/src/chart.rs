//! A bar chart for e-ink: black bars on a thin base line, a label below
//! each, the value of the highest above it.

use paperpaw_sdk::ui;
use paperpaw_sdk::{Color, Rect};

/// Bars are never wider than those of a week.
const MIN_SLOTS: usize = 7;

/// Draws `values` as bars in `area` with `labels` below them; the bar
/// `highlight` (e.g. today) is framed. `top` labels the highest bar. A
/// few values keep the width of a week's bars, at the right (the newest).
pub(crate) fn bars(
    ui: &mut ui::Ui<'_>,
    area: Rect,
    values: &[u32],
    labels: &[&str],
    (highlight, top): (Option<usize>, &str),
) {
    let style = ui.style();
    let Some(font) = ui.font() else {
        return;
    };
    if values.is_empty() {
        return;
    }
    let label_h = (style.small() * 1.4) as u16;
    let top_h = label_h;
    let chart = Rect::new(
        area.x,
        area.y + top_h,
        area.width,
        area.height.saturating_sub(label_h + top_h),
    );
    let max = values.iter().copied().max().unwrap_or(0).max(1);
    let slots = values.len().max(MIN_SLOTS);
    let slot = f32::from(chart.width) / slots as f32;
    let shift = slot * (slots - values.len()) as f32;
    let bar_w = (slot * 0.6).max(2.0);
    let base_y = i32::from(chart.y + chart.height);
    let line = u32::from(style.line()).max(1);
    ui.canvas().fill_rect(
        i32::from(chart.x),
        base_y,
        u32::from(chart.width),
        line,
        Color::gray(150),
    );
    // Labels wider than their bar's room: every other one, counted from
    // the last (today).
    let widest = labels
        .iter()
        .map(|l| font.measure(style.small(), l))
        .fold(0.0f32, f32::max);
    let every = if widest > slot * 0.9 { 2 } else { 1 };
    for (i, &value) in values.iter().enumerate() {
        let x = f32::from(chart.x) + shift + slot * i as f32 + (slot - bar_w) / 2.0;
        let h = (f32::from(chart.height) * value as f32 / max as f32) as i32;
        if value > 0 {
            // At least visible.
            let h = h.max(line as i32 * 2);
            ui.canvas()
                .fill_rect(x as i32, base_y - h, bar_w as u32, h as u32, Color::BLACK);
            if value == max && !top.is_empty() {
                let w = font.measure(style.small(), top);
                let tx = (x + bar_w / 2.0 - w / 2.0)
                    .clamp(f32::from(area.x), f32::from(area.x + area.width) - w);
                let row = Rect::new(area.x, (base_y - h) as u16 - top_h, area.width, top_h);
                ui::Label::new(top)
                    .px(style.small())
                    .color(style.faint(ui::Ink::NOTE))
                    .draw_from(ui, tx, row);
            }
        }
        if highlight == Some(i) {
            let pad = (slot - bar_w) / 4.0;
            ui.canvas().stroke_rect(
                (x - pad) as i32,
                i32::from(chart.y),
                (bar_w + 2.0 * pad) as u32,
                u32::from(chart.height) + line,
                line,
                Color::gray(120),
            );
        }
        let shown = (values.len() - 1 - i).is_multiple_of(every);
        if let Some(label) = labels.get(i).filter(|_| shown) {
            let w = font.measure(style.small(), label);
            let lx = x + bar_w / 2.0 - w / 2.0;
            let row = Rect::new(area.x, base_y as u16 + line as u16, area.width, label_h);
            let color = if highlight == Some(i) {
                Color::BLACK
            } else {
                style.faint(ui::Ink::MUTED)
            };
            ui::Label::new(label)
                .px(style.small())
                .color(color)
                .draw_from(ui, lx, row);
        }
    }
}
