//! Placing parts without computing positions by hand: a column of parts
//! one below the other ([`Stack`]), equal columns side by side
//! ([`columns`]), a grid of cells the focus moves through ([`CellGrid`]), a
//! group on a card or in a frame ([`Ui::card`](super::Ui::card)) and a row of segments,
//! one of them chosen, e.g. tabs over a page ([`Segments`]).
//!
//! ```
//! use paperpaw_sdk::ui::{Stack, columns};
//! use paperpaw_platform::Rect;
//!
//! let mut stack = Stack::new(Rect::new(0, 0, 480, 800), 12);
//! let title = stack.take(40);
//! let buttons = stack.take_bottom(60);
//! let text = stack.rest();
//! let [ok, cancel] = columns(buttons, 2, 12)[..] else { unreachable!() };
//! assert_eq!((title.y, text.y, buttons.y), (0, 52, 740));
//! assert_eq!((ok.width, cancel.x, cancel.right()), (234, 246, 480));
//! ```

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Rect};

use crate::widgets::Frame;

use super::{Ink, UiStyle};

/// Paints `frame` round `area`: the look's painter, else the usual.
pub(crate) fn paint_frame(canvas: &mut Canvas<'_>, style: &UiStyle, area: Rect, frame: Frame) {
    let part = super::look::FramePart::new(area, frame);
    if frame != Frame::None && style.look.frame(canvas, style, part) {
        return;
    }
    match frame {
        Frame::None => {}
        Frame::Line => super::stroke_round(
            canvas,
            style,
            area,
            super::button_border(style),
            Color::BLACK,
        ),
        Frame::Card if style.grays <= 2 => {
            // A light gray would be a heavy pattern on black and white:
            // the sparsest there is, in a thin line as cards in lists.
            let card = style.ink(Ink::CARD);
            let sparser = |c: u8| c + (255 - c) / 2;
            let pattern = Color::rgb(sparser(card.r), sparser(card.g), sparser(card.b));
            canvas.fill_round_shaded(area, style.corner, pattern);
            let line = (style.line / 2).max(1);
            canvas.stroke_round(area, line, style.corner, Color::BLACK);
        }
        Frame::Card => canvas.fill_round_shaded(area, style.corner, style.ink(Ink::CARD)),
        Frame::Dark => super::fill_round(canvas, style, area, Color::BLACK),
    }
}
