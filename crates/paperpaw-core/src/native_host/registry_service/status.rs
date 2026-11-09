//! Op 21 `status` (`registry.md` 2.2 and 3): the status items and lines
//! natively declared (`status.item`, `status.line`; `ui.md` 5.3), `scope`
//! empty or `item` or `line`. Where no plugin declared one the page is empty
//! (total 0, the last page); so it is for a scope that names neither kind.
use super::{
    Row,
    fixed::{Fixed, caps, translated},
    ordered,
    sources::Sources,
};
use crate::native_host::{
    declaration_store::Kind,
    registrations::{self as wire, Record},
};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::ERR_BUSY;

/// `reg_status.flags`: drawn by its owner (always for a line), private.
const DRAWN: u32 = 1;
const PRIVATE: u32 = 1 << 1;
/// `reg_status.kind`.
const ITEM: u32 = 0;
const LINE: u32 = 1;

pub(super) fn rows(
    sources: &Sources<'_>,
    scope: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let (items, lines) = match scope {
        "" => (true, true),
        "item" => (true, false),
        "line" => (false, true),
        _ => return Ok(()),
    };
    let Some(directory) = sources.directory else {
        return Ok(());
    };
    let store = directory.store.try_borrow().map_err(|_| ERR_BUSY)?;
    let translator = sources.env.i18n();
    let mut stopped = false;
    for (kind, wanted) in [(Kind::StatusItem, items), (Kind::StatusLine, lines)] {
        if !wanted {
            continue;
        }
        store.keys(kind, &mut |key| {
            if stopped {
                return;
            }
            stopped = ordered::with_plugin(&store, key, &mut |record, plugin| {
                let (name, title, tap, kind, flags, priority, size) = match record {
                    Record::StatusItem(item) => (
                        item.name,
                        item.title,
                        item.tap,
                        ITEM,
                        item.flags & PRIVATE,
                        item.priority,
                        item.min_width,
                    ),
                    Record::StatusLine(line) => (
                        line.name,
                        line.title,
                        "",
                        LINE,
                        DRAWN | line.flags & PRIVATE,
                        0,
                        line.height,
                    ),
                    _ => return ControlFlow::Continue(()),
                };
                let Some(id) = Fixed::<{ caps::ID }>::of(&[plugin, ".", name]) else {
                    return ControlFlow::Continue(());
                };
                // The tap is one of the owner's own actions, named relative.
                let full: Fixed<{ caps::ID }> = if tap.is_empty() {
                    Fixed::new()
                } else if let Some(full) = Fixed::of(&[plugin, ".", tap]) {
                    full
                } else {
                    return ControlFlow::Continue(());
                };
                visit(&Record::ServiceRegStatus(wire::ServiceRegStatus {
                    id: id.as_str(),
                    plugin,
                    kind,
                    flags,
                    priority,
                    size,
                    tap: full.as_str(),
                    title: translated(translator, title, caps::TITLE),
                }))
            })
            .is_break();
        });
    }
    Ok(())
}
