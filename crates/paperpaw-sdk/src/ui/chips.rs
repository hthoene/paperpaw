//! A row of chips, one of them chosen: a few filters or options side by
//! side, each as wide as its word needs ([`Chips`]). Where the choices
//! should share the width evenly, use [`Segments`](super::Segments).

use alloc::borrow::Cow;
use alloc::vec::Vec;

use paperpaw_platform::{Point, Rect};

use crate::Id;
use crate::builtin::actions;
use crate::env::Env;

use super::UiStyle;

/// A row of chips, one of them chosen: each a button
/// ([`Ui::button`](super::Ui::button)) as wide as its label needs with
/// the room a button keeps at its sides
/// ([`button_width`](super::button_width)), at least as wide as high, the
/// style's gap apart, from the left of the area. A chip that does not fit
/// beside the others goes onto the next row: words are never shortened
/// (only a word wider than the whole area is); [`Chips::height_for`] says
/// how tall the rows are. With the buttons, left and right move the
/// focus, *Select* chooses.
///
/// Each label is a translation key (or text that is none, shown as it
/// is), translated where it is measured and drawn: a key the plugin has
/// as a constant is kept without a copy. Made once with the layout (it
/// measures the labels); drawing and hitting allocate nothing. The
/// default is a row of no chips.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chips {
    /// One per label; empty for those not shown.
    cells: Vec<Rect>,
    labels: Vec<Cow<'static, str>>,
}

impl Chips {
    /// The chips of `labels` in `area`, measured in the UI font at the
    /// size [`Ui::button`](super::Ui::button) sets them, centred in the
    /// area's height.
    pub fn new<L: Into<Cow<'static, str>>>(
        env: &Env<'_>,
        style: &UiStyle,
        area: Rect,
        labels: impl IntoIterator<Item = L>,
    ) -> Self {
        let labels: Vec<Cow<'static, str>> = labels.into_iter().map(Into::into).collect();
        let cells = Self::from_widths(style, area, widths(env, style, labels.iter())).cells;
        Self { cells, labels }
    }

    /// The chips in `area` with the label widths `widths` already known
    /// (the room at their sides included): as [`Chips::new`], without
    /// labels.
    pub(crate) fn from_widths(
        style: &UiStyle,
        area: Rect,
        widths: impl IntoIterator<Item = u16>,
    ) -> Self {
        let cells = place(style, area, widths);
        // The rows in the middle of the area's height, as a row of
        // buttons is.
        let rows_height = cells
            .iter()
            .map(|c| c.bottom())
            .max()
            .map_or(0, |b| b as u16);
        let below = (area.y + area.height).saturating_sub(rows_height) / 2;
        // From the start side of the area: the right where the UI runs right
        // to left.
        let cells = cells
            .into_iter()
            .map(|c| style.flip(area, Rect::new(c.x, c.y + below, c.width, c.height)))
            .collect();
        Self {
            cells,
            labels: Vec::new(),
        }
    }

    /// The height of a row of chips: a button's in a row that can be
    /// touched, as [`Segments`](super::Segments) are.
    pub fn height(style: &UiStyle) -> u16 {
        super::Segments::height(style)
    }

    /// Where chip `index` is; none for one not shown (past the area).
    pub fn cell(&self, index: usize) -> Option<Rect> {
        self.cells.get(index).copied().filter(|c| c.width > 0)
    }

    /// The chip at `point`.
    pub fn hit(&self, point: Point) -> Option<usize> {
        self.cells.iter().position(|c| c.contains(point))
    }

    /// How many chips there are (every one is shown).
    pub fn shown(&self) -> usize {
        self.cells.iter().take_while(|c| c.width > 0).count()
    }

    /// Where the focus goes from `focus` with `action`: left and right
    /// (and previous and next), staying among the chips shown. `None` for
    /// other actions.
    pub fn step(&self, focus: usize, action: &Id) -> Option<usize> {
        let last = self.shown().checked_sub(1)?;
        // The second chip left of the first, on the same row: the chips
        // run from the right, so left goes on.
        let mirrored = matches!(
            (self.cells.first(), self.cells.get(1)),
            (Some(first), Some(second)) if second.y == first.y && second.x < first.x
        );
        let (back, on) = (focus.saturating_sub(1).min(last), (focus + 1).min(last));
        if action == &actions::PREVIOUS {
            Some(back)
        } else if action == &actions::NEXT {
            Some(on)
        } else if action == &actions::LEFT {
            Some(if mirrored { on } else { back })
        } else if action == &actions::RIGHT {
            Some(if mirrored { back } else { on })
        } else {
            None
        }
    }

    /// Draws the chips shown, `chosen` on and `focus` (where the focus is
    /// on them) focused.
    pub fn draw(&self, ui: &mut super::Ui<'_>, chosen: usize, focus: Option<usize>) {
        let (env, style) = (ui.env(), ui.style());
        let canvas = ui.canvas();
        for (i, (cell, label)) in self.cells.iter().zip(&self.labels).enumerate() {
            if cell.width == 0 {
                break;
            }
            // Outside the band drawn now, its focus too: drawn in its own.
            if !super::shows(canvas, super::button_reach(style, *cell)) {
                continue;
            }
            super::draw_button(
                canvas,
                env,
                style,
                *cell,
                env.i18n().tr(label),
                i == chosen,
                focus == Some(i),
            );
        }
    }
}

/// The width of each label's chip: its word in the size buttons set it,
/// and half the chip's height at each side, as its rounded ends take that
/// much; rounded up, so the word keeps all its room.
fn widths<'a, L: AsRef<str> + 'a>(
    env: &'a Env<'_>,
    style: &'a UiStyle,
    labels: impl Iterator<Item = &'a L> + 'a,
) -> impl Iterator<Item = u16> + 'a {
    let font = style.font(env);
    labels.map(move |label| {
        let label = env.i18n().tr(label.as_ref());
        let single = label.chars().count() == 1;
        let size = if single { style.text } else { style.small };
        let text = match &font {
            Some(font) => font.measure(size, label),
            // No font: about half the size a character.
            None => label.chars().count() as f32 * size / 2.0,
        };
        let width =
            text + f32::from(Chips::height(style)).max(super::button_width(style, text) - text);
        width as u16 + u16::from(width > f32::from(width as u16))
    })
}

/// The chips of `widths` from the top left of `area`, the gap apart,
/// onto the next row where one does not fit beside the others.
fn place(style: &UiStyle, area: Rect, widths: impl IntoIterator<Item = u16>) -> Vec<Rect> {
    let height = Chips::height(style);
    let gap = style.gap as u16;
    let right = area.x.saturating_add(area.width);
    let (mut x, mut y) = (area.x, area.y);
    widths
        .into_iter()
        .map(|width| {
            let width = width.max(height).min(area.width);
            if x > area.x && x.saturating_add(width) > right {
                x = area.x;
                y = y.saturating_add(height + gap);
            }
            let cell = Rect::new(x, y, width, height);
            x = x.saturating_add(width).saturating_add(gap);
            cell
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::STYLE;

    #[test]
    fn chips_go_from_the_left_a_gap_apart_each_its_own_width() {
        let height = Chips::height(&STYLE);
        let gap = STYLE.gap as u16;
        let wide = height * 3;
        let area = Rect::new(10, 20, 1000, height + 10);
        let chips = Chips::from_widths(&STYLE, area, [wide, wide * 2]);
        assert_eq!(chips.cell(0), Some(Rect::new(10, 25, wide, height)));
        assert_eq!(
            chips.cell(1),
            Some(Rect::new(10 + wide + gap, 25, wide * 2, height))
        );
        assert_eq!(chips.cell(2), None);
        // A narrow word's chip is as wide as high.
        let narrow = Chips::from_widths(&STYLE, area, [1]);
        assert_eq!(narrow.cell(0).unwrap().width, height);
    }

    #[test]
    fn a_chip_that_does_not_fit_goes_onto_the_next_row_whole() {
        let height = Chips::height(&STYLE);
        let gap = STYLE.gap as u16;
        let wide = height * 2;
        let area = Rect::new(0, 0, 2 * wide + gap + wide / 2, 2 * height + gap);
        let chips = Chips::from_widths(&STYLE, area, [wide, wide, wide, wide]);
        assert_eq!(chips.shown(), 4);
        // The third begins the second row, as wide as its word needs.
        assert_eq!(
            chips.cell(2),
            Some(Rect::new(0, height + gap, wide, height))
        );
        assert_eq!(
            chips.cell(3),
            Some(Rect::new(wide + gap, height + gap, wide, height))
        );
        assert_eq!(chips.step(3, &actions::RIGHT), Some(3));
        assert_eq!(chips.step(2, &actions::LEFT), Some(1));
        // A word wider than the whole area alone is cut to it.
        let cut = Chips::from_widths(&STYLE, area, [area.width * 2]);
        assert_eq!(cut.cell(0).unwrap().width, area.width);
    }

    #[test]
    fn chips_hit_and_step_within_their_row() {
        let height = Chips::height(&STYLE);
        let area = Rect::new(0, 0, 1000, height);
        let chips = Chips::from_widths(&STYLE, area, [height * 2, height * 3, height]);
        let second = chips.cell(1).unwrap();
        assert_eq!(chips.hit(Point::new(i32::from(second.x) + 1, 1)), Some(1));
        // Between chips: none.
        assert_eq!(chips.hit(Point::new(i32::from(second.x) - 1, 1)), None);
        assert_eq!(chips.step(0, &actions::LEFT), Some(0));
        assert_eq!(chips.step(0, &actions::NEXT), Some(1));
        assert_eq!(chips.step(2, &actions::RIGHT), Some(2));
        assert_eq!(chips.step(1, &actions::UP), None);
        assert_eq!(
            Chips::from_widths(&STYLE, area, []).step(0, &actions::LEFT),
            None
        );
    }
}
