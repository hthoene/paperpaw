//! Slot 42 `canvas.describe`: what a native screen draws, told to whoever
//! asks (`Kernel::describe`: automation, reading aloud, tests), as a legacy
//! screen tells it with `Canvas::describe` (`screens-input.md` section 6.1, #243).
//!
//! Nothing is kept, copied or allocated where nobody asks: the label is
//! copied by the describing sink only, and only while it is there. One pass
//! keeps at most [`MAX_PARTS`] parts and [`MAX_LABELS`] label bytes, so a
//! screen describing or drawing text in a loop cannot use up the heap; past
//! them, or out of memory, a part is not told (slot 42 returns 0).

use super::canvas::CanvasScope;
use alloc::string::String;
use paperpaw_graphics::{Described, Role};
use paperpaw_native::host_glue::ERR_INVALID;
use paperpaw_platform::Rect;

/// Flag bit 0: the part has the focus.
pub(crate) const FOCUSED: u32 = 1;
/// Flag bit 1: the part is on or off (bit 2 says which); without it, neither.
pub(crate) const HAS_ON: u32 = 1 << 1;
/// Flag bit 2: on, where bit 1 is set.
pub(crate) const ON: u32 = 1 << 2;
/// Flag bit 3: the part does nothing now (also where the canvas says so).
pub(crate) const DISABLED: u32 = 1 << 3;
/// Every flag this build knows; any other bit is refused.
const KNOWN: u32 = FOCUSED | HAS_ON | ON | DISABLED;
/// The most parts one describe pass keeps (a full X4 list screen tells about 40).
pub(crate) const MAX_PARTS: usize = 128;
/// The most label bytes one describe pass keeps.
pub(crate) const MAX_LABELS: usize = 4096;

/// The role of wire number `role` (`screens-input.md` 6.1): 1 to 13 in the order of
/// [`Role`]; a later number this build does not know is told as text, as
/// [`Role::Other`] is drawn. 0 is no role.
fn role_of(role: u32) -> Option<Role> {
    Some(match role {
        0 => return None,
        1 => Role::Heading,
        2 => Role::Row,
        3 => Role::Button,
        4 => Role::Switch,
        5 => Role::Slider,
        6 => Role::Text,
        7 => Role::Check,
        8 => Role::Radio,
        9 => Role::Field,
        10 => Role::Image,
        11 => Role::Link,
        12 => Role::Dialog,
        13 => Role::Progress,
        _ => Role::Text,
    })
}

/// Whether a part in `role` is one to find and touch (`tap-on`): it needs an
/// area that is not empty.
fn touched(role: Role) -> bool {
    matches!(
        role,
        Role::Row
            | Role::Button
            | Role::Switch
            | Role::Field
            | Role::Check
            | Role::Radio
            | Role::Link
    )
}

/// The area `x`, `y`, `width`, `height` of the canvas's own coordinates as a
/// [`Rect`]: what lies left of or above `(0, 0)` or past 65,535 is cut off.
pub(crate) fn area(x: i64, y: i64, width: u64, height: u64) -> Rect {
    let width = i64::try_from(width).unwrap_or(i64::MAX);
    let height = i64::try_from(height).unwrap_or(i64::MAX);
    let fit = |v: i64| v.clamp(0, i64::from(u16::MAX));
    let (left, top) = (fit(x), fit(y));
    let (right, bottom) = (fit(x.saturating_add(width)), fit(y.saturating_add(height)));
    // In range by `fit`; the right and bottom edges are not left of the left
    // and top ones.
    let cut = |v: i64| u16::try_from(v).unwrap_or(u16::MAX);
    Rect::new(cut(left), cut(top), cut(right - left), cut(bottom - top))
}

impl CanvasScope {
    /// Slot 42: the part `role` (`role_of`) with `label` at the area of the
    /// canvas's own coordinates, `flags` saying more (`FOCUSED`, `HAS_ON`,
    /// `ON`, `DISABLED`). Returns 1 where it was told, 0 where nobody asks
    /// (a plugin may leave out the rest of this draw's parts), `ERR_INVALID`
    /// for role 0, an unknown flag or an empty area of a part to touch,
    /// `ERR_STATE` outside a draw callback, `ERR_BUSY` while another
    /// operation draws.
    #[allow(clippy::too_many_arguments, reason = "the ABI slot's scalar geometry")]
    #[must_use]
    pub fn describe(
        &self,
        role: u32,
        flags: u32,
        label: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> i32 {
        let Some(role) = role_of(role) else {
            return ERR_INVALID;
        };
        if flags & !KNOWN != 0 || (touched(role) && (width == 0 || height == 0)) {
            return ERR_INVALID;
        }
        let area = area(
            i64::from(x),
            i64::from(y),
            u64::from(width),
            u64::from(height),
        );
        self.with_canvas(|canvas| {
            let on = (flags & HAS_ON != 0).then_some(flags & ON != 0);
            // Disabled where the canvas says so too (`Canvas::disabling`).
            let part = Described::new(role, String::new(), area)
                .focused(flags & FOCUSED != 0)
                .on(on)
                .disabled(flags & DISABLED != 0);
            i32::from(canvas.try_describe(part, label, MAX_PARTS, MAX_LABELS))
        })
        .unwrap_or_else(|error| error)
    }
}

/// A native text draw told as text where somebody asks (`screens-input.md` 6.1): the
/// plugin's own parts take it in (a row's label) or it stands alone.
pub(crate) fn told_text(
    canvas: &mut paperpaw_graphics::Canvas<'_>,
    text: &str,
    x: f32,
    top: f32,
    width: f32,
    height: f32,
) {
    if !canvas.is_describing() {
        return;
    }
    let at = area(
        floor(x),
        floor(top),
        ceil(width).max(0).unsigned_abs(),
        ceil(height).max(0).unsigned_abs(),
    );
    let part = Described::new(Role::Text, String::new(), at);
    canvas.try_describe(part, text, MAX_PARTS, MAX_LABELS);
}

/// Float to whole pixels down, without `libm` (`no_std`): `as` truncates and
/// saturates, NaN gives 0.
fn floor(v: f32) -> i64 {
    let t = v as i64;
    if (t as f32) > v {
        t.saturating_sub(1)
    } else {
        t
    }
}

/// Float to whole pixels up; see [`floor`].
fn ceil(v: f32) -> i64 {
    let t = v as i64;
    if (t as f32) < v {
        t.saturating_add(1)
    } else {
        t
    }
}

#[cfg(test)]
#[path = "canvas_describe_tests.rs"]
mod tests;
