//! The weather symbols (the sun, clouds, rain, snow, fog, thunder) drawn as
//! [`Paint`] operations, line or solid. Owner: the *Weather now* port.
//! The interface-6 code is `symbol.rs`.
//!
//! As there, a symbol is a few layers of shapes (discs, round-ended lines),
//! the front one first, laid out in a square 1000 units wide; a cloud before
//! the sun hides what lies behind it, with a little room around it. Only ink
//! is drawn, so the symbol sits on any frame, light or dark.
//!
//! The native canvas has no mask call: the pixels of the symbol are tested
//! one at a time in whole numbers (four samples a pixel; a pixel is inked
//! where half of them are) and joined into spans one pixel high, as the
//! toolkit's `shape::spans` does. Interface 6 drew the edges in grays; the
//! spans have none.
use alloc::vec::Vec;
use paperpaw_sdk::{Color, Rect, native::Error, round::sin_cos};
use paperpaw_ui_kit::{Paint, widget::shape::spans};

use crate::native_weather::Symbol;

/// The size of the square the shapes are laid out in.
const UNITS: i32 = 1000;

/// How a symbol is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Style {
    /// Outlines.
    Line,
    /// Filled shapes.
    Solid,
}

/// A shape: a disc, or a line with round ends (`w` half its width).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Disc {
        x: i32,
        y: i32,
        r: i32,
    },
    Line {
        ax: i32,
        ay: i32,
        bx: i32,
        by: i32,
        w: i32,
    },
}

impl Shape {
    /// Whether the point `x`, `y` lies in the shape grown by `grow` units
    /// (smaller where it is negative).
    fn holds(self, x: i32, y: i32, grow: i32) -> bool {
        match self {
            Shape::Disc { x: cx, y: cy, r } => {
                let reach = r + grow;
                // Most points lie far off: told apart without multiplying.
                if (x - cx).abs() > reach || (y - cy).abs() > reach {
                    return false;
                }
                let r = i64::from(reach.max(0));
                let (dx, dy) = (i64::from(x - cx), i64::from(y - cy));
                dx * dx + dy * dy <= r * r
            }
            Shape::Line { ax, ay, bx, by, w } => {
                let reach = w + grow;
                if x < ax.min(bx) - reach
                    || x > ax.max(bx) + reach
                    || y < ay.min(by) - reach
                    || y > ay.max(by) + reach
                {
                    return false;
                }
                let w = i64::from(reach.max(0));
                let (px, py) = (i64::from(x - ax), i64::from(y - ay));
                let (vx, vy) = (i64::from(bx - ax), i64::from(by - ay));
                let along = px * vx + py * vy;
                let length = vx * vx + vy * vy;
                let distance = if along <= 0 || length == 0 {
                    px * px + py * py
                } else if along >= length {
                    let (qx, qy) = (i64::from(x - bx), i64::from(y - by));
                    qx * qx + qy * qy
                } else {
                    px * px + py * py - along * along / length
                };
                distance <= w * w
            }
        }
    }

    /// The box the shape takes: left, top, right, bottom.
    fn bounds(self) -> (i32, i32, i32, i32) {
        match self {
            Shape::Disc { x, y, r } => (x - r, y - r, x + r, y + r),
            Shape::Line { ax, ay, bx, by, w } => (
                ax.min(bx) - w,
                ay.min(by) - w,
                ax.max(bx) + w,
                ay.max(by) + w,
            ),
        }
    }
}

/// A part of a symbol: shapes together, less some (`cut`).
#[derive(Debug, Clone, Default)]
struct Layer {
    shapes: Vec<Shape>,
    cut: Vec<Shape>,
    /// Only its outline is drawn, `thickness` wide.
    outline: bool,
    /// It hides the layers behind it, with this much room around it.
    hides: Option<i32>,
}

impl Layer {
    fn filled(shapes: Vec<Shape>) -> Self {
        Self {
            shapes,
            ..Self::default()
        }
    }

    fn holds(&self, x: i32, y: i32, grow: i32) -> bool {
        self.shapes.iter().any(|s| s.holds(x, y, grow))
            && !self.cut.iter().any(|s| s.holds(x, y, -grow))
    }

    fn ink(&self, x: i32, y: i32, thickness: i32) -> bool {
        self.holds(x, y, 0) && !(self.outline && self.holds(x, y, -thickness))
    }
}

/// A symbol as layers, the front one first.
#[derive(Debug, Clone, Default)]
struct Scene {
    layers: Vec<Layer>,
    thickness: i32,
}

impl Scene {
    fn ink(&self, x: i32, y: i32) -> bool {
        for layer in &self.layers {
            if layer.ink(x, y, self.thickness) {
                return true;
            }
            if let Some(room) = layer.hides
                && layer.holds(x, y, room)
            {
                return false;
            }
        }
        false
    }
}

/// `item` pushed onto `items`, refusing rather than aborting.
fn keep<T>(items: &mut Vec<T>, item: T) -> Result<(), Error> {
    items.try_reserve(1).map_err(|_| Error::NoMemory)?;
    items.push(item);
    Ok(())
}

/// `shapes` in a list of their own.
fn list<const N: usize>(shapes: [Shape; N]) -> Result<Vec<Shape>, Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(N).map_err(|_| Error::NoMemory)?;
    out.extend(shapes);
    Ok(out)
}

/// The shapes `make` gives for `0..count`.
fn each(count: i32, mut make: impl FnMut(i32) -> Shape) -> Result<Vec<Shape>, Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(usize::try_from(count).unwrap_or(0))
        .map_err(|_| Error::NoMemory)?;
    for i in 0..count {
        out.push(make(i));
    }
    Ok(out)
}

/// The parts of a cloud `w` units wide whose top left is at `x`, `y`;
/// it is about 0.62 `w` high.
fn cloud(x: i32, y: i32, w: i32) -> Result<Vec<Shape>, Error> {
    let base = w / 5;
    let bottom = y + w * 62 / 100 - base;
    list([
        Shape::Line {
            ax: x + base,
            ay: bottom,
            bx: x + w - base,
            by: bottom,
            w: base,
        },
        Shape::Disc {
            x: x + w * 58 / 100,
            y: y + w * 30 / 100,
            r: w * 30 / 100,
        },
        Shape::Disc {
            x: x + w * 30 / 100,
            y: y + w * 41 / 100,
            r: w / 5,
        },
    ])
}

/// A line from `a` to `b`, `w` units wide.
fn line((ax, ay): (i32, i32), (bx, by): (i32, i32), w: i32) -> Shape {
    Shape::Line {
        ax,
        ay,
        bx,
        by,
        w: (w / 2).max(1),
    }
}

/// The point `r` units from `x`, `y` towards `degrees` (clockwise from
/// twelve).
fn toward(x: i32, y: i32, r: i32, degrees: i32) -> (i32, i32) {
    let (sin, cos) = sin_cos(degrees);
    (x + r * sin / 10_000, y - r * cos / 10_000)
}

struct Builder {
    style: Style,
    thickness: i32,
    scene: Scene,
}

impl Builder {
    fn push(&mut self, layer: Layer) -> Result<(), Error> {
        keep(&mut self.scene.layers, layer)
    }

    /// A shape filled, or outlined in the line style.
    fn body(&mut self, shapes: Vec<Shape>, cut: Vec<Shape>, hides: bool) -> Result<(), Error> {
        self.push(Layer {
            shapes,
            cut,
            outline: self.style == Style::Line,
            hides: hides.then_some(self.thickness * 3 / 4),
        })
    }

    /// A cloud before what follows.
    fn cloud(&mut self, x: i32, y: i32, w: i32) -> Result<(), Error> {
        self.body(cloud(x, y, w)?, Vec::new(), true)
    }

    /// The sun: a disc (or ring) `r` units round with eight rays from
    /// `from` to `to` units out.
    fn sun(&mut self, (x, y): (i32, i32), r: i32, from: i32, to: i32) -> Result<(), Error> {
        self.body(list([Shape::Disc { x, y, r }])?, Vec::new(), false)?;
        let thickness = self.thickness;
        let rays = each(8, |i| {
            let d = i * 45;
            line(toward(x, y, from, d), toward(x, y, to, d), thickness)
        })?;
        self.push(Layer::filled(rays))
    }

    /// A crescent moon `r` units round, filled in both styles: it is thin.
    fn moon(&mut self, (x, y): (i32, i32), r: i32) -> Result<(), Error> {
        self.push(Layer {
            shapes: list([Shape::Disc { x, y, r }])?,
            cut: list([Shape::Disc {
                x: x + r * 45 / 100,
                y: y - r * 32 / 100,
                r: r * 80 / 100,
            }])?,
            outline: false,
            hides: None,
        })
    }

    /// The sun by day, the moon by night, at `at`, `r` its disc's radius.
    fn sky(&mut self, day: bool, at: (i32, i32), r: i32) -> Result<(), Error> {
        if day {
            self.sun(at, r, r + r * 45 / 100, r + r * 105 / 100)
        } else {
            self.moon(at, r * 160 / 100)
        }
    }

    /// Rain in slanted lines below a cloud, from `top` to `bottom`.
    fn rain(&mut self, xs: &[i32], top: i32, bottom: i32) -> Result<(), Error> {
        let slant = (bottom - top) * 35 / 100;
        let mut lines = Vec::new();
        lines
            .try_reserve_exact(xs.len())
            .map_err(|_| Error::NoMemory)?;
        for &x in xs {
            lines.push(line((x, top), (x - slant, bottom), self.thickness));
        }
        self.push(Layer::filled(lines))
    }

    /// Drizzle: short dashes in two rows.
    fn drizzle(&mut self, xs: &[i32], top: i32) -> Result<(), Error> {
        let t = self.thickness;
        let mut dashes = Vec::new();
        dashes
            .try_reserve_exact(xs.len() * 2)
            .map_err(|_| Error::NoMemory)?;
        for &x in xs {
            dashes.push(line((x, top), (x - 30, top + 80), t));
            dashes.push(line((x - 90, top + 150), (x - 120, top + 230), t));
        }
        self.push(Layer::filled(dashes))
    }

    /// Snowflakes: three crossed lines each.
    fn snow(&mut self, flakes: &[(i32, i32)], arm: i32) -> Result<(), Error> {
        let t = self.thickness * 85 / 100;
        let mut lines = Vec::new();
        lines
            .try_reserve_exact(flakes.len() * 3)
            .map_err(|_| Error::NoMemory)?;
        for &(x, y) in flakes {
            for d in [0, 60, 120] {
                lines.push(line(toward(x, y, arm, d), toward(x, y, arm, d + 180), t));
            }
        }
        self.push(Layer::filled(lines))
    }

    /// Lightning: a zigzag.
    fn bolt(&mut self) -> Result<(), Error> {
        let t = self.thickness * 12 / 10;
        let points = [(570, 640), (450, 800), (590, 800), (470, 960)];
        let mut lines = Vec::new();
        lines.try_reserve_exact(3).map_err(|_| Error::NoMemory)?;
        for pair in points.windows(2) {
            if let [a, b] = pair {
                lines.push(line(*a, *b, t));
            }
        }
        self.push(Layer::filled(lines))
    }

    /// Hail: four stones below the cloud.
    fn hail(&mut self) -> Result<(), Error> {
        let r = self.thickness * 7 / 10;
        self.push(Layer::filled(list([
            Shape::Disc { x: 290, y: 740, r },
            Shape::Disc { x: 320, y: 890, r },
            Shape::Disc { x: 760, y: 750, r },
            Shape::Disc { x: 730, y: 900, r },
        ])?))
    }
}

/// The scene of `symbol` (by day or night) in `style`, with lines
/// `thickness` units wide.
fn scene(symbol: Symbol, day: bool, style: Style, thickness: i32) -> Result<Scene, Error> {
    let mut b = Builder {
        style,
        thickness,
        scene: Scene {
            layers: Vec::new(),
            thickness,
        },
    };
    match symbol {
        Symbol::Clear => b.sky(day, (500, 500), if day { 200 } else { 225 })?,
        Symbol::FewClouds => {
            b.cloud(70, 560, 560)?;
            b.sky(day, (590, 420), 175)?;
        }
        Symbol::PartlyCloudy => {
            b.cloud(60, 420, 720)?;
            b.sky(day, (640, 350), 150)?;
        }
        Symbol::Cloudy => {
            b.cloud(60, 340, 800)?;
            b.cloud(430, 160, 520)?;
        }
        Symbol::Fog => {
            b.cloud(180, 110, 640)?;
            let t = thickness;
            b.push(Layer::filled(list([
                line((140, 650), (860, 650), t),
                line((230, 780), (770, 780), t),
                line((140, 910), (860, 910), t),
            ])?))?;
        }
        Symbol::Drizzle => {
            // The cloud above rain or snow.
            b.cloud(100, 90, 800)?;
            b.drizzle(&[400, 590, 780], 680)?;
        }
        Symbol::Rain => {
            b.cloud(100, 90, 800)?;
            b.rain(&[370, 560, 750], 690, 900)?;
        }
        Symbol::HeavyRain => {
            b.cloud(100, 90, 800)?;
            b.rain(&[310, 470, 630, 790], 680, 950)?;
        }
        Symbol::Sleet => {
            b.cloud(100, 90, 800)?;
            b.rain(&[360, 780], 690, 900)?;
            b.snow(&[(530, 820)], 80)?;
        }
        Symbol::Snow => {
            b.cloud(100, 90, 800)?;
            b.snow(&[(320, 760), (540, 870), (750, 760)], 80)?;
        }
        Symbol::Showers | Symbol::SnowShowers => {
            b.cloud(60, 220, 760)?;
            if symbol == Symbol::Showers {
                b.rain(&[330, 520, 710], 740, 940)?;
            } else {
                b.snow(&[(300, 800), (520, 880), (720, 790)], 75)?;
            }
            b.sky(day, (700, 270), 110)?;
        }
        Symbol::Thunder | Symbol::Hail => {
            b.cloud(100, 90, 800)?;
            b.bolt()?;
            if symbol == Symbol::Hail {
                b.hail()?;
            }
        }
    }
    Ok(b.scene)
}

/// The ink of `symbol` (by day or night) in a square of side 1,000, in
/// thousandths of the square's side: left, top, **right, bottom** (what
/// `symbol.rs`'s `ink_box` says). Taken from its shapes (what a cloud hides
/// of the sun is not taken off), for placing a symbol by its ink rather than
/// its square. Where memory fails for the shapes, the whole square.
#[must_use]
pub fn ink_box(symbol: Symbol, day: bool) -> (i32, i32, i32, i32) {
    let whole = (0, 0, UNITS, UNITS);
    let Ok(scene) = scene(symbol, day, Style::Solid, 60) else {
        return whole;
    };
    let mut ink = (UNITS, UNITS, 0, 0);
    for shape in scene.layers.iter().flat_map(|l| l.shapes.iter()) {
        let (left, top, right, bottom) = shape.bounds();
        ink = (
            ink.0.min(left),
            ink.1.min(top),
            ink.2.max(right),
            ink.3.max(bottom),
        );
    }
    let keep = |v: i32| v.clamp(0, UNITS);
    let (left, top, right, bottom) = (keep(ink.0), keep(ink.1), keep(ink.2), keep(ink.3));
    if right <= left || bottom <= top {
        return whole;
    }
    (left, top, right, bottom)
}

/// How wide the symbol's lines are, in units, for a symbol `size` pixels
/// large: about a sixteenth of it, never thinner than two pixels.
fn thickness(size: u32) -> i32 {
    let two_pixels = i32::try_from(2 * u64::from(UNITS.unsigned_abs()) / u64::from(size.max(1)))
        .unwrap_or(i32::MAX);
    60.max(two_pixels)
}

/// Whether the pixel `column`, `row` of a square `size` pixels wide shows
/// ink: half of its four samples do.
fn inked(scene: &Scene, size: i32, column: u32, row: u32) -> bool {
    let span = 4 * i64::from(size);
    let unit = |p: u32, sample: i64| -> i32 {
        i32::try_from((4 * i64::from(p) + sample) * i64::from(UNITS) / span).unwrap_or(i32::MAX)
    };
    let mut inside = 0;
    for (sx, sy) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
        if scene.ink(unit(column, sx), unit(row, sy)) {
            inside += 1;
        }
    }
    inside >= 2
}

/// Draws `symbol` (by day or night) into the square `area`, in `color`.
///
/// # Errors
/// `NoMemory` when a drawing operation cannot be kept.
pub fn draw(
    out: &mut Vec<Paint>,
    symbol: Symbol,
    day: bool,
    area: Rect,
    style: Style,
    color: Color,
) -> Result<(), Error> {
    let size = area.width.min(area.height);
    if size < 4 {
        return Ok(());
    }
    let scene = scene(symbol, day, style, thickness(u32::from(size)))?;
    let side = i32::from(size);
    let x = i32::from(area.x) + (i32::from(area.width) - side) / 2;
    let y = i32::from(area.y) + (i32::from(area.height) - side) / 2;
    spans(
        out,
        (x, y),
        (u32::from(size), u32::from(size)),
        color,
        |column, row| inked(&scene, side, column, row),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fail_alloc::sweep;

    const SYMBOLS: [Symbol; 14] = [
        Symbol::Clear,
        Symbol::FewClouds,
        Symbol::PartlyCloudy,
        Symbol::Cloudy,
        Symbol::Fog,
        Symbol::Drizzle,
        Symbol::Rain,
        Symbol::HeavyRain,
        Symbol::Sleet,
        Symbol::Snow,
        Symbol::Showers,
        Symbol::SnowShowers,
        Symbol::Thunder,
        Symbol::Hail,
    ];

    fn scene_of(symbol: Symbol, day: bool, style: Style) -> Scene {
        scene(symbol, day, style, 60).unwrap()
    }

    #[test]
    fn shapes_hold_their_points() {
        let disc = Shape::Disc {
            x: 500,
            y: 500,
            r: 100,
        };
        assert!(disc.holds(500, 500, 0) && disc.holds(600, 500, 0));
        assert!(!disc.holds(601, 500, 0) && disc.holds(601, 500, 5));
        assert!(!disc.holds(590, 500, -20));
        let line = Shape::Line {
            ax: 100,
            ay: 100,
            bx: 300,
            by: 100,
            w: 10,
        };
        assert!(line.holds(200, 105, 0) && line.holds(95, 100, 0));
        assert!(!line.holds(200, 115, 0) && !line.holds(315, 100, 0));
    }

    #[test]
    fn a_cloud_hides_the_sun_behind_it() {
        let solid = scene_of(Symbol::PartlyCloudy, true, Style::Solid);
        // The sun's middle shows; where the cloud lies before it, the cloud.
        assert!(solid.ink(640, 340));
        assert!(solid.ink(400, 700));
        // Outlined, the cloud is empty inside and hides the sun there.
        let lines = scene_of(Symbol::PartlyCloudy, true, Style::Line);
        assert!(!lines.ink(420, 700));
        assert!(!lines.ink(640, 340));
        // The night shows the moon instead.
        let night = scene_of(Symbol::Clear, false, Style::Line);
        assert!(night.ink(400, 600) && !night.ink(700, 300));
    }

    #[test]
    fn every_symbol_has_ink_and_none_reaches_out_of_its_square() {
        for symbol in SYMBOLS {
            for style in [Style::Line, Style::Solid] {
                for day in [true, false] {
                    let drawn = scene_of(symbol, day, style);
                    let inked = (0..50)
                        .flat_map(|y| (0..50).map(move |x| (x * 20, y * 20)))
                        .filter(|&(x, y)| drawn.ink(x, y))
                        .count();
                    assert!(inked > 40, "{symbol:?} {style:?}: {inked}");
                    for at in [-5, 1005] {
                        let out = (0..100).any(|i| drawn.ink(at, i * 10) || drawn.ink(i * 10, at));
                        assert!(!out, "{symbol:?} {style:?} reaches out");
                    }
                }
            }
        }
    }

    #[test]
    fn ink_boxes_lie_inside_the_square() {
        for symbol in SYMBOLS {
            for day in [true, false] {
                let (left, top, right, bottom) = ink_box(symbol, day);
                assert!(0 <= left && left < right && right <= UNITS, "{symbol:?}");
                assert!(0 <= top && top < bottom && bottom <= UNITS, "{symbol:?}");
            }
        }
    }

    #[test]
    fn lines_are_never_thinner_than_two_pixels() {
        assert_eq!(thickness(300), 60);
        assert_eq!(thickness(20), 100);
        assert_eq!(thickness(0), 2000);
    }

    /// The rectangles a symbol was drawn as.
    fn rects(out: &[Paint]) -> Vec<Rect> {
        out.iter()
            .map(|op| match op {
                Paint::Fill(rect, _) => *rect,
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn a_symbol_is_drawn_inside_its_square_in_its_colour() {
        let area = Rect::new(30, 40, 120, 90);
        for symbol in SYMBOLS {
            for style in [Style::Line, Style::Solid] {
                let mut out = Vec::new();
                draw(&mut out, symbol, true, area, style, Color::BLACK).unwrap();
                assert!(!out.is_empty(), "{symbol:?} {style:?}");
                // The square is 90 wide, centred in the 120 wide area.
                for op in &out {
                    let Paint::Fill(rect, color) = op else {
                        panic!("{op:?}")
                    };
                    assert_eq!(*color, Color::BLACK);
                    assert!(
                        rect.x >= 45
                            && rect.x + rect.width <= 135
                            && rect.y >= 40
                            && rect.y + rect.height <= 130,
                        "{symbol:?} {rect:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn solid_has_more_ink_than_line_and_follows_the_ink_box() {
        let area = Rect::new(0, 0, 200, 200);
        let covered = |style| {
            let mut out = Vec::new();
            draw(&mut out, Symbol::Rain, true, area, style, Color::BLACK).unwrap();
            let all = rects(&out);
            let ink: u32 = all
                .iter()
                .map(|r| u32::from(r.width) * u32::from(r.height))
                .sum();
            (ink, all)
        };
        let (line_ink, _) = covered(Style::Line);
        let (solid_ink, solid) = covered(Style::Solid);
        assert!(solid_ink > line_ink);
        let (left, top, right, bottom) = ink_box(Symbol::Rain, true);
        let px = |units: i32| units * 200 / 1000;
        let (x0, x1) = (
            solid.iter().map(|r| i32::from(r.x)).min().unwrap(),
            solid
                .iter()
                .map(|r| i32::from(r.x) + i32::from(r.width))
                .max()
                .unwrap(),
        );
        let (y0, y1) = (
            solid.iter().map(|r| i32::from(r.y)).min().unwrap(),
            solid
                .iter()
                .map(|r| i32::from(r.y) + i32::from(r.height))
                .max()
                .unwrap(),
        );
        // The pixels' ink lies within the box the shapes give (a pixel's
        // rounding apart) and fills most of it.
        assert!(x0 >= px(left) - 1 && x1 <= px(right) + 1, "{x0} {x1}");
        assert!(y0 >= px(top) - 1 && y1 <= px(bottom) + 1, "{y0} {y1}");
        assert!(x1 - x0 > (px(right) - px(left)) * 9 / 10);
        assert!(y1 - y0 > (px(bottom) - px(top)) * 9 / 10);
    }

    #[test]
    fn a_square_too_small_draws_nothing() {
        let mut out = Vec::new();
        for side in [0, 1, 3] {
            let area = Rect::new(5, 5, side, side);
            draw(
                &mut out,
                Symbol::Clear,
                true,
                area,
                Style::Solid,
                Color::BLACK,
            )
            .unwrap();
        }
        draw(
            &mut out,
            Symbol::Clear,
            true,
            Rect::new(5, 5, 400, 3),
            Style::Solid,
            Color::BLACK,
        )
        .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn a_symbol_at_the_far_corner_does_not_wrap() {
        let mut out = Vec::new();
        let area = Rect::new(u16::MAX - 20, u16::MAX - 20, 40, 40);
        draw(
            &mut out,
            Symbol::Snow,
            true,
            area,
            Style::Solid,
            Color::BLACK,
        )
        .unwrap();
        assert!(rects(&out).iter().all(|r| r.x >= u16::MAX - 20));
    }

    #[test]
    fn refused_memory_is_an_error_never_an_abort() {
        let area = Rect::new(0, 0, 24, 24);
        sweep(|| {
            let mut out = Vec::new();
            draw(
                &mut out,
                Symbol::Hail,
                true,
                area,
                Style::Line,
                Color::BLACK,
            )
        });
        sweep(|| {
            let mut out = Vec::new();
            draw(
                &mut out,
                Symbol::Sleet,
                false,
                area,
                Style::Solid,
                Color::BLACK,
            )
        });
    }
}
