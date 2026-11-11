//! A row of chips, one of them chosen: a few filters or options side by side,
//! each as wide as its word needs ([`Chips`]), as the Beta 3 files library
//! showed its filters on large screens.
//!
//! A chip is a button: outlined, or filled where it is the chosen one, with a
//! frame round it where the focus is. Chips run from the left of the area, a
//! gap apart; one that does not fit beside the others goes onto the next row
//! whole (a word is shortened only where it is wider than the whole area).
//! Left and right (and previous and next) move the focus from chip to chip,
//! *Select* chooses, a tap on a chip chooses it. Where the interface runs right
//! to left ([`Style::rtl`]) the chips run from the right and left and right
//! swap, as in Beta 3.
//!
//! Made once with the screen's layout (it measures the labels), it owns its
//! cells, its texts and the few colours it draws with, so nothing of the
//! style or the font is kept; [`Chips::prepare`] only pushes [`Paint`]
//! operations, and asking where a chip is or which one a tap meets allocates
//! nothing. Allocation is fallible and at most [`MAX_CHIPS`] chips are made.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::canvas::PartRole;
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, fonts::Font, snapshot::Snapshot},
};

use crate::{Paint, Style, paint, text};

/// The most chips one row of chips holds; more are refused, not cut short.
pub const MAX_CHIPS: usize = 32;

/// A row of chips laid out in an area, with what they draw with.
#[derive(Debug)]
pub struct Chips {
    /// One per label.
    cells: Vec<Rect>,
    labels: Vec<String>,
    /// The size of a label of one character, and of a longer one.
    sizes: (f32, f32),
    /// The room at each side of a word is a gap, or less in a narrow chip.
    gap: u16,
    /// The width of the outline, which the focus frame doubles.
    border: u16,
    /// The corners: half the height for pills, else this many pixels.
    corner: u32,
    pill: bool,
    /// The interface runs right to left: the chips start at the right.
    rtl: bool,
    /// Whether to queue what the chips tell about themselves.
    describing: bool,
    ink: Color,
    paper: Color,
}

impl Chips {
    /// How many pixels high a fingertip is on a touch screen, `0` on one that
    /// is not: what [`Chips::height`] keeps chips at least as high as. The
    /// figure [`Style::read`] holds rows to ([`crate::style::finger_px`]).
    #[must_use]
    pub fn finger(display: &Snapshot) -> u16 {
        crate::geometry::clamp_u16(i64::from(crate::style::finger_px(display)))
    }

    /// The height of a row of chips: seven tenths of a list row, at least a
    /// fingertip (`finger`, see [`Chips::finger`]) and no higher than the row.
    #[must_use]
    pub fn height(style: &Style, finger: u16) -> u16 {
        let row = u32::from(style.row);
        (row * 7 / 10).max(u32::from(finger)).min(row) as u16
    }

    /// The chips of `labels` (texts already translated) in `area`, measured
    /// in `font` at the sizes buttons are set in, the rows centred in the
    /// area's height.
    ///
    /// # Errors
    /// [`Error::NoSpace`] for more than [`MAX_CHIPS`] labels,
    /// [`Error::NoMemory`], or a refused font reply.
    pub fn new(
        font: &Font<'_>,
        style: &Style,
        finger: u16,
        area: Rect,
        labels: &[&str],
    ) -> Result<Self, Error> {
        let mut chips = Self::empty(style, labels.len())?;
        let widths = chips.widths(font, style, finger, labels)?;
        chips.set_cells(style, finger, area, &widths)?;
        for label in labels {
            crate::mem::push(&mut chips.labels, text::copy(label)?)?;
        }
        Ok(chips)
    }

    /// How tall the chips of `labels` are in an area `width` wide: their
    /// rows, a gap apart; `0` for none.
    ///
    /// # Errors
    /// As [`Chips::new`].
    pub fn height_for(
        font: &Font<'_>,
        style: &Style,
        finger: u16,
        width: u16,
        labels: &[&str],
    ) -> Result<u16, Error> {
        let chips = Self::empty(style, labels.len())?;
        let widths = chips.widths(font, style, finger, labels)?;
        let cells = place(style, finger, Rect::new(0, 0, width, 0), &widths)?;
        Ok(rows_bottom(&cells))
    }

    /// The chips with the label widths known (the room at their sides
    /// included), without labels: for laying out and for tests.
    ///
    /// # Errors
    /// As [`Chips::new`], without the font.
    pub fn from_widths(
        style: &Style,
        finger: u16,
        area: Rect,
        widths: &[u16],
    ) -> Result<Self, Error> {
        let mut chips = Self::empty(style, widths.len())?;
        chips.set_cells(style, finger, area, widths)?;
        Ok(chips)
    }

    fn empty(style: &Style, count: usize) -> Result<Self, Error> {
        if count > MAX_CHIPS {
            return Err(Error::NoSpace);
        }
        let mut labels = Vec::new();
        labels
            .try_reserve_exact(count)
            .map_err(|_| Error::NoMemory)?;
        Ok(Self {
            cells: Vec::new(),
            labels,
            sizes: (style.text, style.small),
            gap: style.gap,
            border: style.line.max(2),
            corner: style.corner,
            pill: style.pill,
            rtl: style.rtl,
            describing: style.describing,
            ink: style.ink,
            paper: style.paper,
        })
    }

    /// The width of each label's chip: its word in the size buttons set it,
    /// and half the chip's height at each side (its rounded ends take that
    /// much), rounded up so the word keeps all its room.
    fn widths(
        &self,
        font: &Font<'_>,
        style: &Style,
        finger: u16,
        labels: &[&str],
    ) -> Result<Vec<u16>, Error> {
        let height = f32::from(Self::height(style, finger));
        let mut widths = Vec::new();
        widths
            .try_reserve_exact(labels.len())
            .map_err(|_| Error::NoMemory)?;
        for label in labels {
            let wide = font.measure_text(self.size_of(label), label)?;
            if !wide.is_finite() || !(0.0..=crate::geometry::MAX_PX).contains(&wide) {
                return Err(Error::Invalid);
            }
            let width = wide + height.max(self.button_width(wide) - wide);
            let whole = width as u16;
            widths.push(whole.saturating_add(u16::from(width > f32::from(whole))));
        }
        Ok(widths)
    }

    fn size_of(&self, label: &str) -> f32 {
        if label.chars().nth(1).is_none() {
            self.sizes.0
        } else {
            self.sizes.1
        }
    }

    /// How wide a button must be for a word `wide` pixels wide not to be
    /// shortened: a gap at each side, or a tenth of the width more where
    /// that is less.
    fn button_width(&self, wide: f32) -> f32 {
        let gap = f32::from(self.gap);
        if wide >= 10.0 * gap {
            wide + 2.0 * gap
        } else {
            wide * 1.2
        }
    }

    fn set_cells(
        &mut self,
        style: &Style,
        finger: u16,
        area: Rect,
        widths: &[u16],
    ) -> Result<(), Error> {
        let mut cells = place(style, finger, area, widths)?;
        // The rows in the middle of the area's height, as a row of buttons is.
        let below = (u32::from(area.y) + u32::from(area.height))
            .saturating_sub(u32::from(rows_bottom(&cells)))
            / 2;
        let below = crate::geometry::clamp_u16(i64::from(below));
        // From the start side of the area: the right where the interface runs
        // right to left.
        for cell in &mut cells {
            cell.y = cell.y.saturating_add(below);
            *cell = crate::style::flip_rect(self.rtl, area, *cell);
        }
        self.cells = cells;
        Ok(())
    }

    /// Where chip `index` is.
    #[must_use]
    pub fn cell(&self, index: usize) -> Option<Rect> {
        self.cells.get(index).copied()
    }

    /// The chip at `point`.
    #[must_use]
    pub fn hit(&self, point: Point) -> Option<usize> {
        self.cells.iter().position(|c| c.contains(point))
    }

    /// How many chips there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The text of chip `index`, as given.
    #[must_use]
    pub fn label(&self, index: usize) -> Option<&str> {
        self.labels.get(index).map(String::as_str)
    }

    /// Where the focus goes from chip `focus` with `action`: left and right
    /// (and previous and next), staying among the chips; `None` for other
    /// actions.
    #[must_use]
    pub fn step(&self, focus: usize, action: &str) -> Option<usize> {
        let last = self.cells.len().checked_sub(1)?;
        let (back, on) = (
            focus.saturating_sub(1).min(last),
            focus.saturating_add(1).min(last),
        );
        match action {
            "core.previous" => Some(back),
            "core.next" => Some(on),
            // The chips run from the right: left goes on.
            "core.left" => Some(if self.rtl { on } else { back }),
            "core.right" => Some(if self.rtl { back } else { on }),
            _ => None,
        }
    }

    /// What chip `index` draws on: its own cell and the frame the focus draws
    /// a little outside it. A screen that moves the focus between chips asks
    /// for these drawn again.
    #[must_use]
    pub fn reach(&self, index: usize) -> Option<Rect> {
        let cell = self.cell(index)?;
        let out = self.border.saturating_mul(3);
        Some(cell.grow(out))
    }

    /// The corners of a chip `height` high.
    fn radius(&self, height: u16) -> u32 {
        if self.pill {
            u32::from(height) / 2
        } else {
            self.corner
        }
    }

    /// Sets whether the chips tell what they are (`canvas.describe`): from
    /// [`paint::asking`] in the draw callback; off unless set.
    pub fn set_describing(&mut self, describing: bool) {
        self.describing = describing;
    }

    /// The frames of the chips: the chosen one filled, the others on paper,
    /// each outlined, and a frame outside the one `focus` is on.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn frames(
        &self,
        paint: &mut Vec<Paint>,
        chosen: usize,
        focus: Option<usize>,
    ) -> Result<(), Error> {
        self.frames_in(paint, chosen, focus, None)
    }

    /// Whether chip `index` (with the frame the focus draws round it) is in
    /// `band`, the part of the screen drawn now: `true` without a band.
    fn shows(&self, index: usize, band: Option<Rect>) -> bool {
        let Some(band) = band else {
            return true;
        };
        self.reach(index).is_some_and(|reach| {
            reach.y < band.y.saturating_add(band.height)
                && band.y < reach.y.saturating_add(reach.height)
        })
    }

    /// [`Chips::frames`] for the chips in `band` only, where the screen is
    /// drawn a band at a time (the draw callback's rows).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn frames_in(
        &self,
        paint: &mut Vec<Paint>,
        chosen: usize,
        focus: Option<usize>,
        band: Option<Rect>,
    ) -> Result<(), Error> {
        let border = u32::from(self.border);
        for (i, cell) in self.cells.iter().enumerate() {
            if !self.shows(i, band) {
                continue;
            }
            let back = if i == chosen { self.ink } else { self.paper };
            paint::describe(
                paint,
                self.describing,
                PartRole::Button,
                *cell,
                focus == Some(i),
                false,
            )?;
            let radius = self.radius(cell.height.min(cell.width));
            paint::push(paint, Paint::Focus(*cell, radius, None, back))?;
            paint::push(paint, Paint::Focus(*cell, radius, Some(border), self.ink))?;
            if focus == Some(i) {
                let out = self.border.saturating_mul(2);
                paint::push(
                    paint,
                    Paint::Focus(
                        cell.grow(out),
                        radius.saturating_add(u32::from(out)),
                        Some(border),
                        self.ink,
                    ),
                )?;
            }
        }
        Ok(())
    }

    /// Prepares the drawing of the chips: their frames ([`Chips::frames`]) and
    /// words, each word centred and shortened to its chip, white on the
    /// chosen one.
    ///
    /// # Errors
    /// [`Error::NoMemory`] or a refused font reply.
    pub fn prepare(
        &self,
        paint: &mut Vec<Paint>,
        font: &Font<'_>,
        chosen: usize,
        focus: Option<usize>,
    ) -> Result<(), Error> {
        self.prepare_in(paint, font, chosen, focus, None)
    }

    /// [`Chips::prepare`] for the chips in `band` only (see
    /// [`Chips::frames_in`]): a chip outside it, its focus frame too, makes no
    /// operation.
    ///
    /// # Errors
    /// [`Error::NoMemory`] or a refused font reply.
    pub fn prepare_in(
        &self,
        paint: &mut Vec<Paint>,
        font: &Font<'_>,
        chosen: usize,
        focus: Option<usize>,
        band: Option<Rect>,
    ) -> Result<(), Error> {
        self.frames_in(paint, chosen, focus, band)?;
        for (i, (cell, label)) in self.cells.iter().zip(&self.labels).enumerate() {
            if !self.shows(i, band) {
                continue;
            }
            let size = self.size_of(label);
            let pad = f32::from(self.gap).min(f32::from(cell.width) / 12.0);
            let room = (f32::from(cell.width) - 2.0 * pad).max(1.0);
            let fitted = text::fit(font, size, label, room)?;
            let wide = font.measure_text(size, &fitted)?;
            let x = f32::from(cell.x) + (f32::from(cell.width) - wide).max(0.0) / 2.0;
            let ink = if i == chosen { self.paper } else { self.ink };
            text::label(paint, font, &fitted, size, x, *cell, ink, Some(*cell))?;
        }
        Ok(())
    }
}

/// The chips of `widths` from the top left of `area`, a gap apart, onto the
/// next row where one does not fit beside the others; each as wide as its
/// width says, at least as wide as high and at most the area's width.
fn place(style: &Style, finger: u16, area: Rect, widths: &[u16]) -> Result<Vec<Rect>, Error> {
    let height = Chips::height(style, finger);
    let right = area.x.saturating_add(area.width);
    let mut cells = Vec::new();
    cells
        .try_reserve_exact(widths.len())
        .map_err(|_| Error::NoMemory)?;
    let (mut x, mut y) = (area.x, area.y);
    for width in widths {
        let width = (*width).max(height).min(area.width);
        if x > area.x && x.saturating_add(width) > right {
            x = area.x;
            y = y.saturating_add(height).saturating_add(style.gap);
        }
        cells.push(Rect::new(x, y, width, height));
        x = x.saturating_add(width).saturating_add(style.gap);
    }
    Ok(cells)
}

/// The bottom edge of the lowest chip: `0` for none.
fn rows_bottom(cells: &[Rect]) -> u16 {
    let bottom = cells.iter().map(Rect::bottom).max().unwrap_or(0);
    crate::geometry::clamp_u16(i64::from(bottom))
}

#[cfg(test)]
mod tests;
