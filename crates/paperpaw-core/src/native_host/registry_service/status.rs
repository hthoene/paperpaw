//! Op 21 `status` (`registry.md` 2.2 and 3): the status items and lines
//! natively declared (`status.item`, `status.line`; `ui.md` 5.3), then the
//! kernel's own items (`host_status`, todo 598: plugin `core`, the drawn one
//! with its least width), `scope` empty or `item` or `line`. A scope that
//! names neither kind has an empty page (total 0, the last page).
use super::{
    Row,
    fixed::{Fixed, caps, translated},
    ordered,
    sources::Sources,
};
use crate::native_host::{
    declaration_store::Kind,
    registrations::{self as wire, Record},
    ui_widget_ops::host_status,
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
    let translator = sources.env.i18n();
    // The declared items, the kernel's, then the declared lines.
    if items {
        if let Some(directory) = sources.directory
            && declared(directory, translator, Kind::StatusItem, visit)?
        {
            return Ok(());
        }
        if kernels(sources, visit) {
            return Ok(());
        }
    }
    if lines && let Some(directory) = sources.directory {
        declared(directory, translator, Kind::StatusLine, visit)?;
    }
    Ok(())
}

/// The kernel's own items (`host_status`); `true` when `visit` stopped. An
/// item's row never has the drawn bit (`registry.md` 3: a line's only): that
/// the battery is drawn is what its value says (op 14, `flags` bit 0).
fn kernels(sources: &Sources<'_>, visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>) -> bool {
    let translator = sources.env.i18n();
    for (id, priority, size) in host_status::rows(sources.env) {
        let title = sources.env.status_items().title(id).unwrap_or(id);
        let row = Record::ServiceRegStatus(wire::ServiceRegStatus {
            id,
            plugin: host_status::PLUGIN,
            kind: ITEM,
            flags: 0,
            priority: u32::from(priority),
            size,
            tap: "",
            title: translated(translator, title, caps::TITLE),
        });
        if visit(&row).is_break() {
            return true;
        }
    }
    false
}

/// The natively declared items or lines (`kind`); `true` when `visit` stopped.
fn declared(
    directory: &crate::native_host::native_declarations::NativeDeclarations,
    translator: &paperpaw_i18n::Translator,
    kind: Kind,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<bool, i32> {
    let store = directory.store.try_borrow().map_err(|_| ERR_BUSY)?;
    let mut stopped = false;
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
    Ok(stopped)
}
