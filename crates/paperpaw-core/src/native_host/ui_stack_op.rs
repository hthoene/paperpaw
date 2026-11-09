//! `core.ui` op 1 `stack` (`docs/abi/ui.md` sections 4.3 and 6): the screen
//! stack, top first, as the plugin that asks may see it, paged like
//! `core.registry` with 8 rows a page.
//!
//! The rows come from the kernel's own stack: the instances, owners and flags
//! of the host-only mirror ([`ui_guard`]), the screen and slot ids of the call's
//! kernel `Env`. Which rows a plugin sees is [`ui_guard::visible_rows`]: all of
//! them, or while an instance guards the topmost guard alone for every plugin
//! but the guard's, which sees from the guard up (`service.end.flags` bit 2 says
//! rows were left out). The generation is a hash of the instances listed, so a
//! page read across a change is `ERR_STATE` (start again at 0).
use super::registrations::{self as wire, Record};
use super::ui_guard::{self, UiStackView};
use crate::abi7::context::Frame;
use crate::native_host::{codec, host_services::Caller};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_STATE};

/// Most rows a page holds (`core.ui` section 6).
const PAGE_ROWS: usize = crate::abi7::services_wire::core_ui_v1::PAGE_MAX_ROWS;

/// `service.end.flags` bit 0: the last page.
const END_LAST: u32 = 1;
/// `service.end.flags` bit 2: rows were left out for this plugin.
const END_LEFT_OUT: u32 = 1 << 2;

/// `ui_screen.flags`: bit 0 overlay, 1 guards, 2 covered, 4 secret. Bit 3
/// (waits for `screen_ready`) is not tracked yet and stays clear.
const OVERLAY: u32 = 1;
const GUARDS: u32 = 1 << 1;
const COVERED: u32 = 1 << 2;
const SECRET: u32 = 1 << 4;

/// `text` cut at a UTF-8 boundary to at most `limit` bytes.
fn clip(text: &str, limit: usize) -> &str {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The generation of the rows listed: never 0.
fn generation(view: &dyn UiStackView, start: usize, end: usize) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for index in start..end {
        for byte in view.row(index).map_or(0, |row| row.instance).to_le_bytes() {
            hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
        }
    }
    hash.max(1)
}

/// Op 1.
pub(crate) fn stack(
    frame: &dyn Frame,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let query: wire::ServiceRegQuery<'_> = codec::single(input)?;
    if query.flags != 0 {
        return Err(ERR_INVALID);
    }
    let env = frame.kernel_env().ok_or(ERR_STATE)?;
    let mut result = Err(ERR_STATE);
    frame.with_stack(&mut |view| {
        result = page(view, env.stack(), who.owner, &query, out);
    })?;
    result
}

// `covered` counts the LISTED rows only: what a viewer is not shown (rows above a
// guard it does not own, `service.end.flags` bit 2) must not leak through it.
fn page(
    view: &dyn UiStackView,
    entries: &[paperpaw_sdk::StackEntry],
    viewer: u64,
    query: &wire::ServiceRegQuery<'_>,
    out: &mut [u8],
) -> Result<usize, i32> {
    // The mirror and the kernel's list are made together: one that differs is a
    // stack that moved on.
    if entries.len() != view.len() {
        return Err(ERR_STATE);
    }
    let visible = ui_guard::visible_rows(view, viewer);
    let generation = generation(view, visible.start, visible.end);
    if query.generation != 0 && query.generation != generation
        || query.generation == 0 && query.cursor != 0
    {
        return Err(ERR_STATE);
    }
    // Only the empty scope names anything here.
    let total = if query.scope.is_empty() {
        u32::try_from(visible.end - visible.start).map_err(|_| ERR_STATE)?
    } else {
        0
    };
    if query.cursor > total {
        return Err(ERR_INVALID);
    }
    let end_len = codec::framed_len(&end_record(
        total,
        generation,
        query.cursor,
        visible.left_out,
    ))?;
    let mut used = 0;
    let mut next = query.cursor;
    // Top first: row `n` is the n-th instance from the top of the listed range.
    for rank in query.cursor..total {
        if usize::try_from(next - query.cursor).is_ok_and(|rows| rows == PAGE_ROWS) {
            break;
        }
        let index = visible.end - 1 - usize::try_from(rank).map_err(|_| ERR_STATE)?;
        let (Some(row), Some(entry)) = (view.row(index), entries.get(index)) else {
            return Err(ERR_STATE);
        };
        let flags = (u32::from(row.overlay) * OVERLAY)
            | (u32::from(row.guards) * GUARDS)
            | (u32::from(index + 1 < visible.end) * COVERED)
            | (u32::from(ui_guard::is_secret(view, index)) * SECRET);
        let record = Record::ServiceUiScreen(wire::ServiceUiScreen {
            screen: clip(
                entry.screen.as_str(),
                wire::SERVICE_UI_SCREEN_SCREEN_MAX_BYTES,
            ),
            instance: row.instance,
            slot: entry.slot.as_ref().map_or("", |slot| {
                clip(slot.as_str(), wire::SERVICE_UI_SCREEN_SLOT_MAX_BYTES)
            }),
            owner: clip(
                entry.screen.namespace(),
                wire::SERVICE_UI_SCREEN_OWNER_MAX_BYTES,
            ),
            flags,
        });
        let length = codec::framed_len(&record)?;
        if out.len() < used + length + end_len {
            if next == query.cursor {
                return Err(ERR_NO_SPACE);
            }
            break;
        }
        used += codec::encode_one(&record, out.get_mut(used..).ok_or(ERR_NO_SPACE)?)?;
        next += 1;
    }
    let record = end_record(total, generation, next, visible.left_out);
    used += codec::encode_one(&record, out.get_mut(used..).ok_or(ERR_NO_SPACE)?)?;
    Ok(used)
}

fn end_record(total: u32, generation: u32, next: u32, left_out: bool) -> Record<'static> {
    Record::ServiceEnd(wire::ServiceEnd {
        flags: (u32::from(next == total) * END_LAST) | (u32::from(left_out) * END_LEFT_OUT),
        total,
        generation,
        next,
        cursor: None,
    })
}

#[cfg(test)]
#[path = "ui_stack_op_tests.rs"]
mod tests;
