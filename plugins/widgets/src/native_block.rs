//! A block of text of a book widget: a small line above (a caption, a
//! percentage), the title as large as the size allows in as many lines as fit,
//! and small lines below it (the author, the series).
//!
//! Where the room is short the small lines go first: the last below the
//! title, then the others, then the one above; the title keeps at least two
//! small lines' room.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{Rect, native::Error};
use paperpaw_ui_kit::{
    Paint, geometry,
    widget::{
        Align, Cx, Look,
        text::{Wrapped, line as one_line},
    },
};

/// What a block says.
#[derive(Debug, Default)]
pub struct Block {
    /// The small line above the title.
    pub lead: Option<String>,
    pub title: Option<String>,
    /// The small lines below it, the most important first.
    pub notes: Vec<String>,
}

impl Block {
    /// Whether it says nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lead.is_none() && self.title.is_none() && self.notes.is_empty()
    }
}

/// What of a block fits a room, and how it is laid out.
struct Plan {
    lead: bool,
    notes: usize,
    title: Option<Wrapped>,
    /// The height of a small line with the gap after it.
    step: u16,
    row: u16,
    gap: u16,
}

impl Plan {
    fn small_lines(&self) -> u16 {
        u16::try_from(usize::from(self.lead) + self.notes)
            .unwrap_or(u16::MAX)
            .saturating_mul(self.step)
    }

    fn height(&self, cx: &Cx<'_>) -> Result<u16, Error> {
        let title = match &self.title {
            Some(title) => title.height(&cx.font)?.saturating_add(self.gap),
            None => 0,
        };
        Ok(title
            .saturating_add(self.small_lines())
            .saturating_sub(self.gap))
    }
}

fn plan(
    cx: &Cx<'_>,
    block: &Block,
    (width, height): (u16, u16),
    title_size: f32,
) -> Result<Plan, Error> {
    let style = cx.style();
    let (small, gap) = (style.small, style.gap / 2);
    let row = geometry::line_px(&cx.font, small)?;
    let step = row.saturating_add(gap);
    let mut lead = block.lead.is_some();
    let mut notes = block.notes.len();
    let least = row.saturating_mul(2);
    // The small lines go while the title would have less than a few words' room.
    loop {
        let count = u16::try_from(usize::from(lead) + notes).unwrap_or(u16::MAX);
        let taken = count.saturating_mul(step);
        if block.title.is_none() || height.saturating_sub(taken) >= least {
            break;
        }
        if notes > 0 {
            notes -= 1;
        } else if lead {
            lead = false;
        } else {
            break;
        }
    }
    let count = u16::try_from(usize::from(lead) + notes).unwrap_or(u16::MAX);
    let room = height.saturating_sub(count.saturating_mul(step));
    let title = match &block.title {
        Some(title) => Some(Wrapped::fitting(
            &cx.font,
            title,
            (title_size, small),
            (width, room),
            true,
        )?),
        None => None,
    };
    Ok(Plan {
        lead,
        notes,
        title,
        step,
        row,
        gap,
    })
}

/// The height `block` takes in a room `width` by `height` at most.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn height(cx: &Cx<'_>, block: &Block, room: (u16, u16), title_size: f32) -> Result<u16, Error> {
    plan(cx, block, room, title_size)?.height(cx)
}

/// Draws `block` in the middle of `room`, placed by `align`.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn draw(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    (room, align): (Rect, Align),
    (block, title_size): (&Block, f32),
) -> Result<(), Error> {
    let plan = plan(cx, block, (room.width, room.height), title_size)?;
    let small = cx.style().small;
    let total = plan.height(cx)?;
    let mut y = room.y.saturating_add(room.height.saturating_sub(total) / 2);
    let at = |y: u16, height: u16| Rect::new(room.x, y, room.width, height);
    if let (true, Some(lead)) = (plan.lead, &block.lead) {
        one_line(
            out,
            &cx.font,
            at(y, plan.row),
            align,
            (small, small),
            lead,
            look.faint,
        )?;
        y = y.saturating_add(plan.step);
    }
    if let Some(title) = &plan.title {
        let h = title.height(&cx.font)?;
        title.draw(out, &cx.font, at(y, h), align, look.ink)?;
        y = y.saturating_add(h).saturating_add(plan.gap);
    }
    for note in block.notes.iter().take(plan.notes) {
        one_line(
            out,
            &cx.font,
            at(y, plan.row),
            align,
            (small, small),
            note,
            look.faint,
        )?;
        y = y.saturating_add(plan.step);
    }
    Ok(())
}
