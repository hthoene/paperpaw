//! Where a block's lines start and how wide each is.

use paperpaw_document::Block;

use crate::drop_cap::Cap;
use crate::{BookStyles, Style, block_em};

/// Where a block's lines start and how wide they are: the first line
/// (indented where the block starts, by half a line at most, perhaps
/// hanging out into the book's margin) and the others, within the book's
/// margins, each kept to a quarter of the width; and the lines a drop
/// capital spans, set in by the room it takes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Measure {
    pub(crate) first: (f32, f32),
    pub(crate) rest: (f32, f32),
    /// The lines beside a drop capital and the room it takes.
    cap: (usize, f32),
}

impl Measure {
    pub(crate) fn of(block: &Block, style: &Style, width: f32, indent: f32) -> Measure {
        let (start, end) = match style.book.has(BookStyles::MARGINS) {
            true => {
                let em = block_em(block, style);
                // Not `clamp`: what it says of bounds the wrong way round
                // brings `core`'s formatter of floats (#258).
                let side = |m: u16| (f32::from(m) / 100.0 * em).min(width / 4.0).max(0.0);
                (side(block.margins.0), side(block.margins.1))
            }
            false => (0.0, 0.0),
        };
        let rest = (start, (width - start - end).max(1.0));
        // Indented by half a line at most, so a narrow page keeps room.
        let first_x = (start + indent).min(start + rest.1 / 2.0).max(0.0);
        Measure {
            first: (first_x, (width - end - first_x).max(1.0)),
            rest,
            cap: (0, 0.0),
        }
    }

    /// The same, the first lines set in beside drop capital `cap`.
    pub(crate) fn beside(mut self, cap: Option<Cap>) -> Measure {
        if let Some(cap) = cap {
            self.cap = (cap.lines, cap.room);
        }
        self
    }

    /// Where line `n` starts and how wide it is.
    pub(crate) fn line(&self, n: usize) -> (f32, f32) {
        let (x, width) = if n == 0 { self.first } else { self.rest };
        match n < self.cap.0 {
            true => (x + self.cap.1, (width - self.cap.1).max(1.0)),
            false => (x, width),
        }
    }

    /// The same, the first line as wide as the others: a page that goes
    /// on with a block.
    pub(crate) fn continued(mut self) -> Measure {
        self.first = self.rest;
        self
    }
}
