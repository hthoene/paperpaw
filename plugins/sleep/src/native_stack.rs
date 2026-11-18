//! Where the sleep screen puts its widgets, without the host: the widgets of
//! the list setting above each other between the margins (as the interface-6
//! `WidgetStack` did), the panel of a widget over the page, and when the
//! picture over the page is used. Allocation is fallible and sizes saturate.
use alloc::vec::Vec;
use paperpaw_sdk::{Rect, native::Error};

/// The free space in a list of widgets: it takes the room the others leave.
pub const FILL: &str = "core.fill";

/// The ids of a list setting: separated by commas, trimmed, none empty.
pub fn ids(list: &str) -> impl Iterator<Item = &str> {
    list.split(',').map(str::trim).filter(|id| !id.is_empty())
}

/// One entry of the list, once its widget was made and measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// A free space ([`FILL`]).
    Fill,
    /// A widget of this height (0: it has nothing to show).
    Widget(u16),
}

/// The widgets of `entries` in `area`, as high as they say, two gaps apart,
/// in the middle or, where there are free spaces, where those push them.
/// Those that would not fit below the others are left out, and the rest
/// after them, as are those with nothing to show. The result lists, for each
/// widget kept, its index in `entries` and its rectangle.
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot grow.
pub fn stack(
    area: Rect,
    margin: u16,
    gap: u16,
    entries: &[Entry],
) -> Result<Vec<(usize, Rect)>, Error> {
    let width = area.width.saturating_sub(margin.saturating_mul(2));
    let between = u32::from(gap) * 2;
    let (mut total, mut shown, mut fills) = (0u32, 0usize, 0usize);
    let mut kept = 0;
    for (at, entry) in entries.iter().enumerate() {
        match *entry {
            Entry::Fill => fills += 1,
            Entry::Widget(0) => {}
            Entry::Widget(height) => {
                let added = u32::from(height) + if shown == 0 { 0 } else { between };
                if total + added > u32::from(area.height) {
                    break;
                }
                total += added;
                shown += 1;
            }
        }
        kept = at + 1;
    }
    let mut placed = Vec::new();
    if shown == 0 {
        return Ok(placed);
    }
    placed
        .try_reserve_exact(shown)
        .map_err(|_| Error::NoMemory)?;
    let room = u32::from(area.height);
    let fill = if fills == 0 {
        0
    } else {
        (room.saturating_sub(total) / fills as u32).min(u32::from(u16::MAX))
    };
    let mut y = u32::from(area.y)
        + if fills == 0 {
            room.saturating_sub(total) / 2
        } else {
            0
        };
    for (at, entry) in entries.iter().enumerate().take(kept) {
        match *entry {
            Entry::Fill => y += fill,
            Entry::Widget(0) => {}
            Entry::Widget(height) => {
                if !placed.is_empty() {
                    y += between;
                }
                let rect = Rect::new(
                    area.x.saturating_add(margin),
                    y.min(u32::from(u16::MAX)) as u16,
                    width,
                    height,
                );
                placed.push((at, rect));
                y += u32::from(height);
            }
        }
    }
    Ok(placed)
}

/// `area` less `pad` all round.
#[must_use]
pub fn inside(area: Rect, pad: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(pad),
        area.y.saturating_add(pad),
        area.width.saturating_sub(pad.saturating_mul(2)),
        area.height.saturating_sub(pad.saturating_mul(2)),
    )
}

/// The values of `sleep.overlay-when`.
#[cfg(test)]
pub const ALWAYS: &str = "sleep.when.always";
pub const READING: &str = "sleep.when.reading";
pub const ELSEWHERE: &str = "sleep.when.elsewhere";

/// Whether the picture goes over the page now: as the setting says, by
/// whether a book is read on the topmost full screen (with or without a menu
/// over it); never where a screen guards the device, whose page is not to be
/// seen.
#[must_use]
pub fn used(when: &str, guarded: bool, reading: bool) -> bool {
    if guarded {
        return false;
    }
    match when {
        READING => reading,
        ELSEWHERE => !reading,
        _ => true,
    }
}

#[cfg(test)]
mod tests;
