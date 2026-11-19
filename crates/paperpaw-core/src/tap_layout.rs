//! Where a tap on the reading screen turns pages or opens the menu: the
//! tap zone presets of Settings › Controls (`core.tap-layout`), mirrored
//! by `core.tap-swap`. They bind the nine tap zones like a key map preset;
//! the user's own bindings still win over them.

use alloc::string::String;
use core::fmt::Write as _;

use paperpaw_platform::Point;
use paperpaw_sdk::setting::{Setting, Text};

use crate::keymap::Zone;

/// Where taps turn pages or open the menu while reading.
pub(crate) const LAYOUT: Setting<Text> = Setting::new("core.tap-layout");
/// Swaps the sides of [`LAYOUT`]: left goes on, right goes back.
pub(crate) const SWAP: Setting<bool> = Setting::new("core.tap-swap");

/// Option of `core.tap-layout`: the left and right thirds turn pages, the
/// middle opens the menu.
pub(crate) const THIRDS: &str = "core.tap-thirds";
/// Option: the left half goes back, the right half on; a strip across the
/// top opens the menu.
pub(crate) const HALVES: &str = "core.tap-halves";
/// Option: for one hand: the right edge and the bottom edge, where the
/// thumb reaches, go on; the upper left goes back.
pub(crate) const L_SHAPE: &str = "core.tap-l-shape";
/// Option: the top third goes back, the bottom third on, the middle opens
/// the menu.
pub(crate) const TOP_BOTTOM: &str = "core.tap-top-bottom";

/// All options, the first is the default.
pub(crate) const OPTIONS: &[&str] = &[THIRDS, HALVES, L_SHAPE, TOP_BOTTOM];

const PREV: &str = "core.prev-page";
const NEXT: &str = "core.next-page";
const MENU: &str = "core.menu";

/// Which areas of the screen the nine zones are.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum TapLayout {
    /// Three columns by three rows.
    #[default]
    Grid,
    /// Two columns below a strip across the top.
    Halves,
}

impl TapLayout {
    /// The zone `point` is in on a `width`×`height` screen.
    pub(crate) fn zone(self, point: Point, width: u16, height: u16) -> Zone {
        match self {
            TapLayout::Grid => Zone::of(point, width, height),
            TapLayout::Halves => {
                let (x, y) = (i64::from(point.x), i64::from(point.y));
                let (w, h) = (i64::from(width.max(1)), i64::from(height.max(1)));
                if y * 3 < h {
                    Zone::Top
                } else if x * 2 < w {
                    Zone::Left
                } else {
                    Zone::Right
                }
            }
        }
    }
}

/// The geometry of the option `name`.
pub(crate) fn layout_of(name: &str) -> TapLayout {
    if name == HALVES {
        TapLayout::Halves
    } else {
        TapLayout::Grid
    }
}

/// What each zone does for the option `name`, in the order top left, top,
/// top right, left, centre, right, bottom left, bottom, bottom right.
fn actions(name: &str) -> [&'static str; 9] {
    match name {
        HALVES => [MENU, MENU, MENU, PREV, MENU, NEXT, PREV, MENU, NEXT],
        L_SHAPE => [PREV, MENU, NEXT, PREV, MENU, NEXT, NEXT, NEXT, NEXT],
        TOP_BOTTOM => [PREV, PREV, PREV, MENU, MENU, MENU, NEXT, NEXT, NEXT],
        _ => [PREV, MENU, NEXT, PREV, MENU, NEXT, PREV, MENU, NEXT],
    }
}

/// The key map source of the option `name`, its sides swapped where
/// `swap`; `None` for the standard layout, which the default key map
/// already holds.
pub(crate) fn source(name: &str, swap: bool) -> Option<String> {
    if name != HALVES && name != L_SHAPE && name != TOP_BOTTOM && !swap {
        return None;
    }
    let actions = actions(name);
    let mut out = String::from("[keymap.global]\n");
    for (index, zone) in Zone::ALL.into_iter().enumerate() {
        let from = if swap { zone.mirrored() } else { zone };
        let from = Zone::ALL.iter().position(|z| *z == from).unwrap_or(index);
        let _ = writeln!(out, "tap.{} = {}", zone.id(), actions[from]);
    }
    // The reader's bottom tap opens font and layout where the menu is
    // there: a page action there goes before it.
    let bottom = actions[7];
    if bottom != MENU {
        let _ = writeln!(out, "[keymap.core.reader]\ntap.bottom = {bottom}");
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(layout: TapLayout, x: i32, y: i32) -> Zone {
        layout.zone(Point { x, y }, 300, 400)
    }

    #[test]
    fn halves_are_two_columns_below_a_strip() {
        assert_eq!(at(TapLayout::Halves, 10, 10), Zone::Top);
        assert_eq!(at(TapLayout::Halves, 290, 100), Zone::Top);
        assert_eq!(at(TapLayout::Halves, 149, 200), Zone::Left);
        assert_eq!(at(TapLayout::Halves, 150, 200), Zone::Right);
        assert_eq!(at(TapLayout::Grid, 150, 200), Zone::Center);
    }

    #[test]
    fn the_standard_layout_needs_no_source_and_a_swap_mirrors() {
        assert!(source(THIRDS, false).is_none());
        let swapped = source(THIRDS, true).unwrap();
        assert!(swapped.contains("tap.left = core.next-page"));
        assert!(swapped.contains("tap.right = core.prev-page"));
        assert!(swapped.contains("tap.center = core.menu"));
        let l = source(L_SHAPE, false).unwrap();
        assert!(l.contains("tap.bottom = core.next-page"));
        assert!(l.contains("[keymap.core.reader]\ntap.bottom = core.next-page"));
        let l = source(L_SHAPE, true).unwrap();
        assert!(l.contains("tap.top-right = core.prev-page"));
        assert!(source(TOP_BOTTOM, false).unwrap().contains("core.reader"));
        assert!(!source(L_SHAPE, true).unwrap().is_empty());
    }
}
