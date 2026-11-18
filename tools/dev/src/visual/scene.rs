//! What one check looks at: a picture of a screen (or of a widget in its
//! cell) with what its drawing described, the interface's measures on
//! that device, and helpers that find ink in the picture.

use paperpaw_core::sdk::semantics::{Described, Role};
use paperpaw_platform::Rect;

use crate::image::Image;
use crate::report::Level;

/// How far a pixel's lightness must differ from the background to count
/// as ink: antialiased edges of glyphs are left out, strokes are not.
pub const INK: u8 = 60;

/// The kind of panel, for contrast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    /// Black and white only (the X4): grays are rounded or dithered.
    Mono,
    /// Grays (or colour).
    Gray,
}

/// The interface's measures on a device, as `UiStyle` computes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ui {
    pub dpi: u16,
    /// The margin of content at the screen's sides (`UiStyle::margin`).
    pub margin: u16,
    /// The least size of a touch target (`UiStyle::finger`, 7 mm); 0
    /// without a touch screen.
    pub finger: u16,
    /// Width of lines (`UiStyle::line`).
    pub line: u16,
    /// The standard gap between elements (`UiStyle::gap`, 2 mm).
    pub gap: u16,
}

impl Ui {
    /// Pixels for `tenths` tenths of a millimetre, as `Metrics` rounds.
    pub fn tenth_mm(&self, tenths: u32) -> u16 {
        ((tenths * u32::from(self.dpi) + 127) / 254).max(1) as u16
    }

    /// How far an edge may be off and still count as aligned: two pixels,
    /// or a quarter millimetre on dense screens (side bearings of glyphs
    /// differ by about that).
    pub fn tolerance(&self) -> u16 {
        ((u32::from(self.dpi) * 25 + 1270) / 2540).max(2) as u16
    }

    /// `pixels` in millimetres, for messages.
    pub fn mm(&self, pixels: u16) -> f32 {
        f32::from(pixels) * 25.4 / f32::from(self.dpi.max(1))
    }
}

/// Something a check found, before it becomes a report's finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub check: &'static str,
    pub level: Level,
    /// Where in the picture.
    pub area: Rect,
    pub message: String,
}

impl Issue {
    pub fn warning(check: &'static str, area: Rect, message: impl Into<String>) -> Self {
        Issue {
            check,
            level: Level::Warning,
            area,
            message: message.into(),
        }
    }

    pub fn error(check: &'static str, area: Rect, message: impl Into<String>) -> Self {
        Issue {
            check,
            level: Level::Error,
            area,
            message: message.into(),
        }
    }
}

/// A screen or widget as drawn.
#[derive(Debug)]
pub struct Scene<'a> {
    pub image: &'a Image,
    luma: Vec<u8>,
    /// What the screen (or widget) itself described, in drawing order:
    /// without the screen below an overlay and the row of button hints.
    pub described: Vec<Described>,
    /// The content area of the screen (without the row of hints).
    pub screen: Rect,
    pub ui: Ui,
    pub panel: Panel,
    /// A widget's cell.
    pub cell: Option<Rect>,
    /// Every label shortened with "…" is reported, wherever it ends (the
    /// UI kit: no block may shorten one).
    pub labels_whole: bool,
    /// Texts of the language drawn that hold a "…" of their own
    /// ("Loading…"): a line that ends at it is not shortened.
    pub ellipses: &'a [String],
    /// The screen is the keyboard's: a row of its keys may be as narrow
    /// as the screen lets that many keys be.
    pub keyboard: bool,
}

impl<'a> Scene<'a> {
    pub fn new(image: &'a Image, described: Vec<Described>, screen: Rect, ui: Ui) -> Self {
        let mut luma = Vec::with_capacity((image.width * image.height) as usize);
        for y in 0..image.height {
            for x in 0..image.width {
                luma.push(image.luma(x, y));
            }
        }
        Scene {
            image,
            luma,
            described,
            screen,
            ui,
            panel: Panel::Gray,
            cell: None,
            labels_whole: false,
            ellipses: &[],
            keyboard: false,
        }
    }

    pub fn keyboard(mut self, keyboard: bool) -> Self {
        self.keyboard = keyboard;
        self
    }

    pub fn ellipses(mut self, ellipses: &'a [String]) -> Self {
        self.ellipses = ellipses;
        self
    }

    pub fn labels_whole(mut self, whole: bool) -> Self {
        self.labels_whole = whole;
        self
    }

    pub fn panel(mut self, panel: Panel) -> Self {
        self.panel = panel;
        self
    }

    pub fn cell(mut self, cell: Option<Rect>) -> Self {
        self.cell = cell;
        self
    }

    /// The whole picture.
    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.image.width as u16, self.image.height as u16)
    }

    /// The pixel's lightness (white outside the picture).
    pub fn luma(&self, x: u32, y: u32) -> u8 {
        if x >= self.image.width || y >= self.image.height {
            return 255;
        }
        self.luma[(y * self.image.width + x) as usize]
    }

    /// Where `d` can be seen: its clip on the screen.
    pub fn visible(&self, d: &Described) -> Option<Rect> {
        d.clip
            .intersection(&self.screen)
            .and_then(|r| r.intersection(&self.bounds()))
    }

    /// The most common lightness in `area`: the paper, or a focus bar's
    /// black, behind what is drawn there.
    pub fn background(&self, area: Rect) -> u8 {
        let mut counts = [0u32; 32];
        let mut sums = [0u64; 32];
        let step = if area.area() > 40_000 { 2 } else { 1 };
        for y in (u32::from(area.y)..area.bottom()).step_by(step) {
            for x in (u32::from(area.x)..area.right()).step_by(step) {
                let l = self.luma(x, y);
                counts[usize::from(l / 8)] += 1;
                sums[usize::from(l / 8)] += u64::from(l);
            }
        }
        let (bucket, count) = counts
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| **c)
            .unwrap_or((31, &0));
        if *count == 0 {
            return 255;
        }
        (sums[bucket] / u64::from(*count)) as u8
    }

    /// The ink in `area` against `background`.
    pub fn ink(&self, area: Rect, background: u8) -> Ink {
        self.ink_with(area, background, INK)
    }

    /// The pixels in `area` differing from `background` by more than
    /// `threshold`: [`Scene::ink`] for faint drawing too.
    pub fn ink_with(&self, area: Rect, background: u8, threshold: u8) -> Ink {
        let area = area.intersection(&self.bounds()).unwrap_or_default();
        let (w, h) = (usize::from(area.width), usize::from(area.height));
        let mut cells = vec![false; w * h];
        for y in 0..h {
            for x in 0..w {
                let l = self.luma(u32::from(area.x) + x as u32, u32::from(area.y) + y as u32);
                cells[y * w + x] = l.abs_diff(background) > threshold;
            }
        }
        Ink { area, cells }
    }

    /// The ink in `area` against its own background.
    pub fn ink_in(&self, area: Rect) -> Ink {
        let background = self.background(area);
        self.ink(area, background)
    }

    /// The parts of `roles` described.
    pub fn parts<'s>(&'s self, roles: &'s [Role]) -> impl Iterator<Item = &'s Described> + 's {
        self.described
            .iter()
            .filter(move |d| roles.contains(&d.role))
    }
}

/// Whether a label says something (arrows and marks do not).
pub fn says_something(label: &str) -> bool {
    label.chars().any(char::is_alphanumeric)
}

/// A short form of a label for messages.
pub fn quote(label: &str) -> String {
    let label = label.trim();
    let short: String = label.chars().take(40).collect();
    if short.len() < label.len() {
        format!("\"{short}...\"")
    } else {
        format!("\"{short}\"")
    }
}

/// Whether `inner` lies within `outer`.
pub fn inside(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

/// Ink in an area: which pixels differ from the background.
#[derive(Debug, Clone)]
pub struct Ink {
    pub area: Rect,
    cells: Vec<bool>,
}

impl Ink {
    fn w(&self) -> usize {
        usize::from(self.area.width)
    }

    /// Whether the pixel at `(x, y)`, relative to the area, is ink.
    pub fn at(&self, x: usize, y: usize) -> bool {
        x < self.w() && y < usize::from(self.area.height) && self.cells[y * self.w() + x]
    }

    pub fn count(&self) -> usize {
        self.cells.iter().filter(|c| **c).count()
    }

    /// Leaves out what lies in `area` (absolute).
    pub fn clear(&mut self, area: Rect) {
        let Some(r) = area.intersection(&self.area) else {
            return;
        };
        let w = self.w();
        for y in r.y - self.area.y..(r.bottom() as u16 - self.area.y) {
            for x in r.x - self.area.x..(r.right() as u16 - self.area.x) {
                self.cells[usize::from(y) * w + usize::from(x)] = false;
            }
        }
    }

    /// Leaves out lines across the area: rows with ink over most of its
    /// width (a frame, a rule, the edge of a bar), so that what is left
    /// is the text.
    pub fn clear_lines(&mut self) {
        let w = self.w();
        if w < 40 {
            return;
        }
        for (y, count) in self.rows().into_iter().enumerate() {
            if count * 10 >= w * 6 {
                self.cells[y * w..(y + 1) * w].fill(false);
            }
        }
    }

    /// Leaves out the sides of a frame round the area: ink that runs
    /// unbroken from its top edge to its bottom edge (an outline's side or
    /// a rounded end passing by), which no icon or letter drawn within
    /// the area does.
    pub fn clear_sides(&mut self) {
        let (w, h) = (self.w(), usize::from(self.area.height));
        if w == 0 || h == 0 {
            return;
        }
        // The shapes of ink touching the top edge, each pixel joined to
        // its eight neighbours.
        let mut shape = vec![usize::MAX; w * h];
        let mut stack = Vec::new();
        for start in 0..w {
            if !self.cells[start] || shape[start] != usize::MAX {
                continue;
            }
            shape[start] = start;
            stack.push(start);
            let mut reaches_bottom = false;
            let mut pixels = Vec::new();
            while let Some(i) = stack.pop() {
                pixels.push(i);
                let (x, y) = (i % w, i / w);
                reaches_bottom |= y + 1 == h;
                for (dx, dy) in NEIGHBOURS {
                    let (nx, ny) = (x as isize + dx, y as isize + dy);
                    if nx < 0 || ny < 0 || nx as usize >= w || ny as usize >= h {
                        continue;
                    }
                    let n = ny as usize * w + nx as usize;
                    if self.cells[n] && shape[n] == usize::MAX {
                        shape[n] = start;
                        stack.push(n);
                    }
                }
            }
            if reaches_bottom {
                for i in pixels {
                    self.cells[i] = false;
                }
            }
        }
    }

    /// A box filled with ink in the area (rows of it unbroken across
    /// three fifths of the area or more, above and below what is in it,
    /// half the area high or more): where it is (absolute), and a pixel
    /// of its fill.
    pub fn filled_box(&self) -> Option<(Rect, (u16, u16))> {
        let (w, h) = (self.w(), usize::from(self.area.height));
        let mut rows = Vec::new();
        for y in 0..h {
            let (mut start, mut best) = (0, (0, 0));
            for x in 0..=w {
                if x < w && self.at(x, y) {
                    continue;
                }
                if x - start > best.1 - best.0 {
                    best = (start, x);
                }
                start = x + 1;
            }
            if (best.1 - best.0) * 5 >= w * 3 {
                rows.push((y, best));
            }
        }
        // Rows above and below the words, as high as half the area or
        // more between them.
        let (top, bottom) = (rows.first()?.0, rows.last()?.0);
        if rows.len() < 3 || (bottom - top) * 2 < h {
            return None;
        }
        let left = rows.iter().map(|r| r.1.0).min()?;
        let right = rows.iter().map(|r| r.1.1).max()?;
        let (x, y) = (self.area.x + left as u16, self.area.y + top as u16);
        let boxed = Rect::new(x, y, (right - left) as u16, (bottom - top + 1) as u16);
        Some((boxed, (x, y)))
    }

    /// Whether the ink frames the area: an unbroken line along most of
    /// its top or bottom edge, or of its left or right one (a box drawn
    /// round a text), not letters that touch it.
    pub fn framed(&self) -> bool {
        let (w, h) = (self.w(), usize::from(self.area.height));
        if w == 0 || h == 0 {
            return false;
        }
        // The longest unbroken run of ink along an edge: three fifths of
        // it or more is a line drawn there, not letters (which leave gaps
        // between lines and words).
        let run = |cells: &mut dyn Iterator<Item = bool>| {
            let (mut best, mut now) = (0, 0);
            for ink in cells {
                now = if ink { now + 1 } else { 0 };
                best = best.max(now);
            }
            best
        };
        let across = |y: usize| run(&mut (0..w).map(|x| self.at(x, y))) * 5 >= w * 3;
        let down = |x: usize| run(&mut (0..h).map(|y| self.at(x, y))) * 5 >= h * 3;
        // At an edge or a pixel or two in from it (a frame drawn inside).
        (0..3.min(h)).any(|y| across(y) || across(h - 1 - y))
            || (0..3.min(w)).any(|x| down(x) || down(w - 1 - x))
    }

    /// Leaves out rows of ink set as dots, a quarter of the area or
    /// more, none of them longer than two pixels: a light line drawn as
    /// a pattern (a list's separators on black and white), not an icon.
    pub fn clear_dotted(&mut self) {
        let w = self.w();
        for y in 0..usize::from(self.area.height) {
            let row = &self.cells[y * w..(y + 1) * w];
            let count = row.iter().filter(|c| **c).count();
            let longest = row.split(|c| !*c).map(<[bool]>::len).max().unwrap_or(0);
            if count * 4 >= w && longest <= 2 {
                self.cells[y * w..(y + 1) * w].fill(false);
            }
        }
    }

    /// Leaves out ink connected to the area's corners: what a box's round
    /// corners leave there.
    pub fn clear_corners(&mut self) {
        let (w, h) = (self.w(), usize::from(self.area.height));
        let (right, bottom) = (w.saturating_sub(1), h.saturating_sub(1));
        let mut stack = vec![(0, 0), (right, 0), (0, bottom), (right, bottom)];
        self.flood(&mut stack);
    }

    /// Leaves out ink connected to the area's right edge: what reaches in
    /// from the right (a label's first letter, a line that runs on under
    /// it), not an icon before it.
    pub fn clear_right(&mut self) {
        let (w, h) = (self.w(), usize::from(self.area.height));
        let mut stack: Vec<(usize, usize)> = (0..h).map(|y| (w.saturating_sub(1), y)).collect();
        self.flood(&mut stack);
    }

    /// Leaves out ink connected to the area's edges: the frame of a
    /// button around a label (its round corners too), what reaches in
    /// from outside.
    pub fn clear_edges(&mut self) {
        let (w, h) = (self.w(), usize::from(self.area.height));
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for x in 0..w {
            stack.push((x, 0));
            stack.push((x, h.saturating_sub(1)));
        }
        for y in 0..h {
            stack.push((0, y));
            stack.push((w.saturating_sub(1), y));
        }
        self.flood(&mut stack);
    }

    /// Leaves out the ink connected to the points of `stack`.
    fn flood(&mut self, stack: &mut Vec<(usize, usize)>) {
        let w = self.w();
        while let Some((x, y)) = stack.pop() {
            if !self.at(x, y) {
                continue;
            }
            self.cells[y * w + x] = false;
            if x > 0 {
                stack.push((x - 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            stack.push((x + 1, y));
            stack.push((x, y + 1));
        }
    }

    /// Keeps the ink in `rows` only (as [`Ink::lines`] gives them).
    pub fn keep_rows(&mut self, rows: &[(usize, usize)]) {
        let w = self.w();
        for y in 0..usize::from(self.area.height) {
            if !rows
                .iter()
                .any(|(top, height)| (*top..top + height).contains(&y))
            {
                self.cells[y * w..(y + 1) * w].fill(false);
            }
        }
    }

    /// The smallest rectangle around the ink (absolute).
    pub fn bounds(&self) -> Option<Rect> {
        let (mut left, mut top, mut right, mut bottom) = (usize::MAX, usize::MAX, 0, 0);
        let w = self.w();
        for (i, c) in self.cells.iter().enumerate() {
            if *c {
                let (x, y) = (i % w, i / w);
                left = left.min(x);
                right = right.max(x);
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
        (left != usize::MAX).then(|| {
            Rect::new(
                self.area.x + left as u16,
                self.area.y + top as u16,
                (right - left + 1) as u16,
                (bottom - top + 1) as u16,
            )
        })
    }

    /// Ink pixels in each row.
    pub fn rows(&self) -> Vec<usize> {
        let w = self.w();
        (0..usize::from(self.area.height))
            .map(|y| {
                self.cells[y * w..(y + 1) * w]
                    .iter()
                    .filter(|c| **c)
                    .count()
            })
            .collect()
    }

    /// Ink pixels in each column.
    pub fn columns(&self) -> Vec<usize> {
        let w = self.w();
        let mut columns = vec![0; w];
        for (i, c) in self.cells.iter().enumerate() {
            if *c {
                columns[i % w] += 1;
            }
        }
        columns
    }

    /// Ink pixels without ink above, below, left or right of them: the
    /// dots of a dithered gray.
    pub fn isolated(&self) -> usize {
        let (w, h) = (self.w(), usize::from(self.area.height));
        let mut n = 0;
        for y in 0..h {
            for x in 0..w {
                if !self.at(x, y) {
                    continue;
                }
                let neighbour = (x > 0 && self.at(x - 1, y))
                    || self.at(x + 1, y)
                    || (y > 0 && self.at(x, y - 1))
                    || self.at(x, y + 1);
                if !neighbour {
                    n += 1;
                }
            }
        }
        n
    }

    /// Whether there is a solid block of ink, two pixels by two.
    pub fn has_solid(&self) -> bool {
        let (w, h) = (self.w(), usize::from(self.area.height));
        (0..h.saturating_sub(1)).any(|y| {
            (0..w.saturating_sub(1)).any(|x| {
                self.at(x, y) && self.at(x + 1, y) && self.at(x, y + 1) && self.at(x + 1, y + 1)
            })
        })
    }

    /// The lines of text in the ink: runs of rows with ink, split where
    /// at least `gap` rows have none; runs much lower than the tallest
    /// (lines, the dots of an umlaut) are left out. As `(top, height)`
    /// relative to the area.
    pub fn lines(&self, gap: usize) -> Vec<(usize, usize)> {
        let rows = self.rows();
        let mut runs: Vec<(usize, usize)> = Vec::new();
        let mut start: Option<usize> = None;
        let mut empty = 0;
        for (y, count) in rows.iter().enumerate() {
            if *count > 0 {
                if start.is_none() {
                    start = Some(y);
                }
                empty = 0;
            } else if let Some(s) = start {
                empty += 1;
                if empty >= gap {
                    runs.push((s, y + 1 - empty - s));
                    start = None;
                    empty = 0;
                }
            }
        }
        if let Some(s) = start {
            runs.push((s, rows.len() - empty - s));
        }
        let tallest = runs.iter().map(|r| r.1).max().unwrap_or(0);
        runs.retain(|r| r.1 * 10 >= tallest * 4 && r.1 >= 4);
        runs
    }

    /// The run of ink starting at the first ink column at or right of
    /// `from` (relative): it ends where `gap` columns in a row have no
    /// ink. As `(first, last)` columns, relative.
    pub fn run_from(&self, from: usize, gap: usize) -> Option<(usize, usize)> {
        let columns = self.columns();
        let first = (from..columns.len()).find(|x| columns[*x] > 0)?;
        let mut last = first;
        let mut empty = 0;
        for (x, count) in columns.iter().enumerate().skip(first) {
            if *count > 0 {
                last = x;
                empty = 0;
            } else {
                empty += 1;
                if empty > gap {
                    break;
                }
            }
        }
        Some((first, last))
    }
}

/// The eight neighbours of a pixel.
const NEIGHBOURS: [(isize, isize); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ink_lines_and_runs() {
        let mut image = Image::filled(40, 30, [230, 230, 230]);
        // Two "lines" of text, 6 rows high, 3 rows apart, and a thin line.
        for y in (2..8).chain(11..17) {
            for x in 5..20 {
                image.set_pixel(x, y, [20, 20, 20]);
            }
        }
        for x in 0..40 {
            image.set_pixel(x, 28, [20, 20, 20]);
        }
        let scene = Scene::new(&image, Vec::new(), Rect::new(0, 0, 40, 30), ui());
        assert_eq!(scene.background(Rect::new(0, 0, 40, 30)), 230);
        let ink = scene.ink_in(Rect::new(0, 0, 40, 30));
        assert_eq!(ink.lines(2), [(2, 6), (11, 6)]);
        assert_eq!(ink.bounds(), Some(Rect::new(0, 2, 40, 27)));
        let band = scene.ink_in(Rect::new(0, 0, 40, 10));
        assert_eq!(band.run_from(0, 3), Some((5, 19)));
        assert!(band.has_solid());
        assert_eq!(band.isolated(), 0);
    }

    pub(crate) fn ui() -> Ui {
        Ui {
            dpi: 220,
            margin: 26,
            finger: 0,
            line: 2,
            gap: 19,
        }
    }
}
