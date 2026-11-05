//! The drawing core of the real "Battery" widget
//! (`plugins/widgets/src/books/battery.rs`: `draw_battery`, `draw_bolt`,
//! `fill_triangle`, `draw_charge_text`), written once against [`Gfx`], the
//! calls the widget makes on the canvas and the font. Used with either
//! table, it measures a plugin ported to the flat one. What the real
//! widget also takes from `Env` (settings, the look, sizes) is not here.

use paperpaw_platform::{Color, Point, Rect};

/// What the widget calls: the canvas and the font.
pub trait Gfx {
    fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: Color);
    fn draw_line(&mut self, from: Point, to: Point, width: u32, color: Color);
    fn fill_round(&mut self, area: Rect, radius: u32, color: Color);
    fn stroke_round(&mut self, area: Rect, thickness: u32, radius: u32, color: Color);
    fn measure(&mut self, size: f32, text: &str) -> f32;
    fn text(&mut self, size: f32, x: f32, baseline: i32, text: &str, color: Color) -> f32;
}

/// How a battery draws: `ink` and `paper`.
#[derive(Clone, Copy)]
pub struct Look {
    pub ink: Color,
    pub paper: Color,
    pub faint: Color,
}

/// The battery in `body`, lying, `percent` full, a bolt and the number
/// beside it while it `charging`.
#[inline(never)]
pub fn battery<G: Gfx>(g: &mut G, look: &Look, body: Rect, percent: u8, charging: bool) {
    draw_battery(g, look, body, percent);
    let size = f32::from(body.height) * 0.9;
    let mut digits = [0u8; 3];
    let text = number(percent.min(100), &mut digits);
    let mut x = f32::from(body.x + body.width) + 8.0;
    let baseline = i32::from(body.y) + i32::from(body.height) * 3 / 4;
    if charging {
        let h = body.height;
        draw_bolt(g, Rect::new(x as u16, body.y, h / 2, h), look.ink);
        x += f32::from(h) * 0.6;
    }
    g.text(size, x, baseline, text, look.ink);
    x += g.measure(size, text);
    g.text(size * 0.45, x, baseline, " %", look.faint);
}

fn number(value: u8, digits: &mut [u8; 3]) -> &str {
    let mut at = 3;
    let mut value = value;
    loop {
        at -= 1;
        digits[at] = b'0' + value % 10;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    core::str::from_utf8(&digits[at..]).unwrap_or("")
}

fn draw_battery<G: Gfx>(g: &mut G, look: &Look, body: Rect, percent: u8) {
    let short = body.width.min(body.height);
    if short < 6 {
        return;
    }
    let knob = (short * 12 / 100).max(2);
    let shell = Rect::new(body.x, body.y, body.width - knob, body.height);
    let h = body.height * 45 / 100;
    let knob_at = Rect::new(
        body.x + body.width - knob - 1,
        body.y + (body.height - h) / 2,
        knob + 1,
        h,
    );
    let line = u32::from(short / 11).max(2);
    let radius = u32::from(short / 5);
    g.fill_round(knob_at, u32::from(knob / 2), look.ink);
    g.fill_round(shell, radius, look.paper);
    g.stroke_round(shell, line, radius, look.ink);
    let pad = line as u16 + (short / 14).max(1);
    let inner = Rect::new(
        shell.x + pad,
        shell.y + pad,
        shell.width.saturating_sub(2 * pad),
        shell.height.saturating_sub(2 * pad),
    );
    let w = (u32::from(inner.width) * u32::from(percent.min(100)) / 100) as u16;
    let level = Rect::new(inner.x, inner.y, w, inner.height);
    if level.width > 0 && level.height > 0 {
        g.fill_round(level, radius.saturating_sub(u32::from(pad)).max(1), look.ink);
    }
}

fn draw_bolt<G: Gfx>(g: &mut G, at: Rect, color: Color) {
    let (x, y) = (i32::from(at.x), i32::from(at.y));
    let (w, h) = (i32::from(at.width), i32::from(at.height));
    if w < 3 || h < 4 {
        return;
    }
    let p = |fx: i32, fy: i32| Point::new(x + w * fx / 100, y + h * fy / 100);
    fill_triangle(g, p(70, 0), p(10, 58), p(56, 58), color);
    fill_triangle(g, p(44, 42), p(90, 42), p(30, 100), color);
}

fn fill_triangle<G: Gfx>(g: &mut G, a: Point, b: Point, c: Point, color: Color) {
    let top = a.y.min(b.y).min(c.y);
    let bottom = a.y.max(b.y).max(c.y);
    let edges = [(a, b), (b, c), (c, a)];
    for y in top..=bottom {
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
                g.fill_rect(l, y, (r - l) as u32, 1, color);
            }
        }
    }
}
