//! The kernel's own status items (`core.status.clock`, `.date`, `.date-short`,
//! `.battery`, `.battery-low`; registered the interface-6 way in
//! `core_plugin.rs`) for native screens (#257, todo 598): op 14 answers them
//! without any plugin call, slot 35 draws the drawn ones, and REG op 21 lists
//! them beside the declared ones.
//!
//! - **Values.** A text (the time, the dates) is the row's text, as Beta 3
//!   wrote it (at most 128 bytes, cut on a character). A drawn value (the
//!   battery's symbol and percentage) is `flags` bit 0 with a status handle
//!   whose owner is [`HOST`]: no plugin ever has that owner id, so the
//!   handle never resolves to a plugin's code. No value hides the item.
//! - **Drawing.** Slot 35 kind 1 for such a handle draws the one item with
//!   the interface-6 renderer (`paperpaw_sdk::status::StatusLine`) in the
//!   place given, with its percentage, in the interface's font: the line's
//!   text size is the place's width over [`DRAWN_WIDTH`], the least width op
//!   21 lists for it, so it is drawn at the size the line was laid out with.
//! - **No nested call**, so the X4's gate does not hold them back.
//!
//! Memory: a value is the provider's own `String`, copied into op 14's row
//! and freed; the drawing allocates what the interface-6 line does (its
//! layout of one item), freed at the return.
use crate::abi7::context::{ComposeRequest, HostStatus};
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_sdk::{
    Color, Env, Params,
    builtin::fonts,
    status::{StatusLine, StatusValue},
};

/// The owner id of the host's own status handles: plugin owner ids start at
/// 1 (`HostResources::try_owner`, and a seat of owner 0 is refused).
pub(crate) const HOST: u64 = 0;

/// The kernel's items, by their token (the index): whether each is drawn.
/// The ids are `paperpaw_sdk::builtin::status_items`' (a test pins them).
const ITEMS: [(&str, bool); 5] = [
    ("core.status.clock", false),
    ("core.status.date", false),
    ("core.status.date-short", false),
    ("core.status.battery", true),
    ("core.status.battery-low", true),
];

/// The least width of a drawn item, in thousandths of the text's size (REG
/// op 21 `size`): the battery's symbol (1.3), the gap (0.3) and "100 %"
/// (about 2.6), as the interface-6 line measured them.
pub(crate) const DRAWN_WIDTH: u32 = 4_200;

/// The plugin REG op 21 names for the kernel's items.
pub(crate) const PLUGIN: &str = "core";

/// The token of the kernel's item `item`, if it is one.
pub(crate) fn token_of(item: &str) -> Option<u32> {
    ITEMS
        .iter()
        .position(|(id, _)| *id == item)
        .and_then(|at| u32::try_from(at).ok())
}

/// The value of the kernel's item `item` now: its text into `out` (cut to
/// `out`'s length on a character), or drawn. `None`: not the kernel's, or no
/// value (the item hides).
pub(crate) fn value(env: &Env<'_>, item: &str, out: &mut [u8]) -> Option<HostStatus> {
    let token = token_of(item)?;
    let value = env.status_items().value(item, env, &Params::default())?;
    match value {
        StatusValue::Text(text) => {
            let mut len = text.len().min(out.len());
            while !text.is_char_boundary(len) {
                len -= 1;
            }
            out.get_mut(..len)?
                .copy_from_slice(text.get(..len)?.as_bytes());
            Some(HostStatus::Text(len))
        }
        _ => Some(HostStatus::Drawn(token)),
    }
}

/// The rows REG op 21 lists for the kernel's items: (id, priority, least
/// width), in token order. Only those the kernel registered.
pub(crate) fn rows<'e>(env: &Env<'e>) -> impl Iterator<Item = (&'static str, u8, u32)> + 'e {
    let items = env.status_items();
    ITEMS.iter().filter_map(move |(id, drawn)| {
        items.title(id)?;
        let width = if *drawn { DRAWN_WIDTH } else { 0 };
        Some((*id, items.priority(id), width))
    })
}

/// Draws the kernel's item `token` into `request`'s place on `canvas`, as
/// the interface-6 line drew it. Its own frame (the X4's frame limit).
///
/// # Errors
/// `ERR_NOT_FOUND` for a token that is no item, `ERR_UNSUPPORTED` without the
/// interface's font, `ERR_INVALID` for a place with no size.
#[inline(never)]
pub(crate) fn draw(
    env: &Env<'_>,
    token: u32,
    request: &ComposeRequest,
    canvas: &mut Canvas<'_>,
) -> Result<(), i32> {
    let (item, _) = usize::try_from(token)
        .ok()
        .and_then(|at| ITEMS.get(at))
        .ok_or(ERR_NOT_FOUND)?;
    let Some(visible) = request.visible else {
        return Ok(());
    };
    let area = paperpaw_sdk::Rect::new(
        u16::try_from(request.x.max(0)).unwrap_or(u16::MAX),
        u16::try_from(request.y.max(0)).unwrap_or(u16::MAX),
        u16::try_from(request.width).unwrap_or(u16::MAX),
        u16::try_from(request.height).unwrap_or(u16::MAX),
    );
    if area.width == 0 || area.height == 0 {
        return Err(ERR_INVALID);
    }
    let font = env
        .fonts()
        .role(fonts::UI.as_str())
        .ok_or(ERR_UNSUPPORTED)?;
    // The size the line was laid out with: the place is the item's least width.
    let size = (f32::from(area.width) * 1000.0 / DRAWN_WIDTH as f32).min(f32::from(area.height));
    let mut canvas = canvas.clipped(visible);
    StatusLine::new(area, ["", "", item], size, Color::BLACK)
        .percent(true)
        .draw(&mut canvas, env, &font, &Params::default());
    Ok(())
}

#[cfg(test)]
mod tests {
    use paperpaw_sdk::builtin::status_items as ids;

    #[test]
    fn the_items_are_the_kernels_by_token() {
        let ours: alloc::vec::Vec<&str> = super::ITEMS.iter().map(|(id, _)| *id).collect();
        let kernels = [
            ids::CLOCK,
            ids::DATE,
            ids::DATE_SHORT,
            ids::BATTERY,
            ids::BATTERY_LOW,
        ];
        let kernels: alloc::vec::Vec<&str> = kernels.iter().map(|id| id.as_str()).collect();
        assert_eq!(ours, kernels);
        assert_eq!(super::token_of(ids::BATTERY.as_str()), Some(3));
        assert_eq!(super::token_of("core.status.gone"), None);
    }
}
