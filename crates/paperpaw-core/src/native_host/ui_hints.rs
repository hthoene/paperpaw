//! `core.ui` op 3 `hints` (`docs/abi/ui.md` 5 and 6): the `help.hint`
//! declarations of one screen, one `ui_hint` row each, then one `end`.
//!
//! - **Rows.** The `input` and the zone in thousandths exactly as declared
//!   (no zone: all 0), `text` the catalog key unchanged, `owner` the declaring
//!   plugin. Order: plugin order, then record order. No device filter: the
//!   help plugin chooses what to show.
//! - **Scope.** `reg_query.scope` is a screen id; one nobody declared a hint
//!   for is an empty page, not an error. `flags` must be 0
//!   (`ERR_UNSUPPORTED`), as in `core.registry`.
//! - **Pages.** At most 8 whole rows (`core.ui`'s page) and exactly one `end`; a cursor of an
//!   older generation is `ERR_STATE` ([`RegistryPage`]). The page counts the
//!   rows once for `total` and walks them again from the cursor; nothing is
//!   kept between calls.
//!
//! Memory (X4): the reply buffer the dispatcher lends, a few hundred bytes of
//! stack, and one fallible allocation of the matching keys (8 bytes each,
//! freed at the return).
use super::{
    codec,
    host_services::Caller,
    registrations::{self as wire, Record},
    registry_page::RegistryPage,
};
use crate::abi7::{
    context::{Frame, ui_views::HintView},
    services_wire::{self, ServiceOp},
};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_STATE, ERR_UNSUPPORTED};

/// The `ui_hint` row of a declared hint.
fn framed(hint: HintView<'_>) -> Record<'_> {
    Record::ServiceUiHint(wire::ServiceUiHint {
        input: hint.input,
        x: hint.x,
        y: hint.y,
        width: hint.width,
        height: hint.height,
        text: hint.text,
        owner: hint.owner,
    })
}

/// Op 3.
pub(super) fn hints(
    frame: &dyn Frame,
    _who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let query: wire::ServiceRegQuery<'_> = codec::single(input)?;
    if query.flags != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    let directory = frame.ui_directory()?;
    let generation = frame.registry()?.generation();
    // A row that cannot be framed (an owner id past its field) is skipped in
    // both walks, so `total`, the cursor and the rows agree.
    let mut total = 0_u32;
    directory.hints(query.scope, &mut |hint| {
        if codec::framed_len(&framed(hint)).is_ok() {
            total = total.saturating_add(1);
        }
        ControlFlow::Continue(())
    })?;
    let room = op.max_reply.min(out.len());
    let out = out.get_mut(..room).ok_or(ERR_INVALID)?;
    let mut page = RegistryPage::new(&query, generation, total, out)?
        .rows_at_most(services_wire::core_ui_v1::PAGE_MAX_ROWS);
    let mut index = 0_u32;
    let mut failure = None;
    directory.hints(query.scope, &mut |hint| {
        let row = framed(hint);
        if codec::framed_len(&row).is_err() {
            return ControlFlow::Continue(());
        }
        let at = index;
        index = index.saturating_add(1);
        if at < query.cursor {
            return ControlFlow::Continue(());
        }
        match page.push(&row) {
            Ok(flow) => flow,
            Err(error) => {
                failure = Some(error);
                ControlFlow::Break(())
            }
        }
    })?;
    if let Some(error) = failure {
        // A row past `total`: the tables changed between the two walks.
        return Err(if error == ERR_INVALID {
            ERR_STATE
        } else {
            error
        });
    }
    page.finish()
}

#[cfg(test)]
#[path = "ui_hints_tests.rs"]
mod tests;
