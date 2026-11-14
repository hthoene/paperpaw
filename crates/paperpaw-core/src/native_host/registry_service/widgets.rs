//! Op 19 `widgets` (`registry.md` 2.2 and 3): the widgets the `core.widget`
//! stage can make, those code plugins added (`Widgets`) and those native
//! plugins declared (`widget.spec`), with the kind's icon, the cells as
//! declared and, in `flags` bit 0, whether some place that shows now lists
//! them (`Widgets::shown`: a `widget.place` whose `shown_if` holds names the
//! widget, or a placement of it, in one of its `list_keys`). Widgets that
//! preview a group of settings are no widgets to add: only `reg_query.flags`
//! bit 0 lists them, those that preview the group `scope`, in this op's order
//! (the environment's first, then the declarations in the store's order); a
//! settings page shows the last, as interface 6's `Widgets::preview` did.
use super::{
    Row,
    fixed::{Fixed, caps, translated},
    ordered, rules,
    sources::Sources,
};
use crate::native_host::{
    declaration_store::{DeclKey, DeclarationStore, Kind},
    registrations::{self as wire, Record},
};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID};
use paperpaw_sdk::{Env, widgets::base};

/// `reg_widget.flags`.
const SHOWN: u32 = 1;
const PRIVATE: u32 = 1 << 1;
const SQUARE: u32 = 1 << 2;

/// `reg_query.flags` bit 0: `scope` is a settings group, list its previews.
const PREVIEWS: u32 = 1;

/// Which widgets a query lists.
#[derive(Clone, Copy)]
enum Wanted<'a> {
    /// Widgets to add, of kind `.0` (empty: every kind).
    Kind(&'a str),
    /// The widgets that preview settings group `.0`.
    Previews(&'a str),
}

impl Wanted<'_> {
    fn takes(self, category: &str, previews: &str) -> bool {
        match self {
            Self::Kind(kind) => previews.is_empty() && (kind.is_empty() || category == kind),
            Self::Previews(group) => previews == group,
        }
    }
}

/// The cells of a code plugin's widget as `widget.spec.cells` has them: columns
/// in bits 0-7, rows 8-15, the least columns 16-23 and the least rows 24-31.
fn cells(span: paperpaw_sdk::widgets::CellSpan) -> u32 {
    u32::from_le_bytes([span.w, span.h, span.min_w, span.min_h])
}

pub(super) fn rows(
    sources: &Sources<'_>,
    scope: &str,
    flags: u32,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let wanted = if flags & PREVIEWS == 0 {
        Wanted::Kind(scope)
    } else if scope.is_empty() {
        return Err(ERR_INVALID);
    } else {
        Wanted::Previews(scope)
    };
    let env = sources.env;
    let widgets = env.widgets();
    let preview = match wanted {
        Wanted::Kind(_) => None,
        Wanted::Previews(group) => widgets.preview(group),
    };
    let listed = widgets
        .iter()
        .filter(|(id, _)| wanted.takes(widgets.category(id.as_str()).unwrap_or(""), ""))
        .map(|(id, title)| (id, title, ""));
    let previewing =
        preview.and_then(|id| widgets.title(id.as_str()).map(|title| (id, title, scope)));
    for (id, title, previews) in listed.chain(previewing) {
        let kind = widgets.category(id.as_str()).unwrap_or("");
        let span = widgets.cells(id.as_str());
        let flags = if widgets.shown(env, id.as_str()) {
            SHOWN
        } else {
            0
        } | if span.is_some_and(|span| span.square) {
            SQUARE
        } else {
            0
        };
        let row = Record::ServiceRegWidget(wire::ServiceRegWidget {
            id: id.as_str(),
            plugin: id.namespace(),
            kind,
            icon: widgets.category_icon(kind),
            flags,
            title: translated(env.i18n(), title, caps::TITLE),
            cells: span.map_or(0, cells),
            previews,
        });
        if visit(&row).is_break() {
            return Ok(());
        }
    }
    let Some(directory) = sources.directory else {
        return Ok(());
    };
    let store = directory.store.try_borrow().map_err(|_| ERR_BUSY)?;
    let mut stopped = false;
    store.keys(Kind::Widget, &mut |key| {
        if !stopped {
            stopped = declared(env, &store, key, wanted, visit).is_break();
        }
    });
    Ok(())
}

/// The widgets to add that native plugins declared, for the kernel's own
/// screens that name widgets (`Widgets::set_declared_elsewhere`): each one's
/// id, the message key of its name and its kind; of widget `only` (or the
/// widget a placement shows) alone, where it names one. Nothing while the
/// store is being written.
pub(crate) fn declared_names(
    directory: &crate::native_host::native_declarations::NativeDeclarations,
    only: Option<&str>,
    each: &mut dyn FnMut(&str, &str, &str),
) {
    let Ok(store) = directory.store.try_borrow() else {
        return;
    };
    let only = only.map(base);
    store.keys(Kind::Widget, &mut |key| {
        let _read = ordered::with_plugin(&store, key, &mut |record, plugin| {
            if let Record::WidgetSpec(spec) = record
                && spec.previews.is_empty()
                && let Some(id) = Fixed::<{ caps::ID }>::of(&[plugin, ".", spec.name])
                && only.is_none_or(|only| only == id.as_str())
            {
                each(id.as_str(), spec.title, spec.category);
            }
            ControlFlow::Continue(())
        });
    });
}

/// The row of the declared widget `key`.
fn declared(
    env: &Env<'_>,
    store: &DeclarationStore,
    key: DeclKey,
    wanted: Wanted<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> ControlFlow<()> {
    // The id first: whether a place lists it scans the store again, which no
    // record visitor may do.
    let mut id = None;
    let _read = ordered::with_plugin(store, key, &mut |record, plugin| {
        if let Record::WidgetSpec(spec) = record {
            id = Fixed::<{ caps::ID }>::of(&[plugin, ".", spec.name]);
        }
        ControlFlow::Continue(())
    });
    let Some(id) = id else {
        return ControlFlow::Continue(());
    };
    let shown = listed(env, store, id.as_str());
    ordered::with_plugin(store, key, &mut |record, plugin| {
        let Record::WidgetSpec(spec) = record else {
            return ControlFlow::Continue(());
        };
        if !wanted.takes(spec.category, spec.previews) {
            return ControlFlow::Continue(());
        }
        let flags = if shown { SHOWN } else { 0 } | spec.flags & PRIVATE | spec.flags & SQUARE;
        visit(&Record::ServiceRegWidget(wire::ServiceRegWidget {
            id: id.as_str(),
            plugin,
            kind: spec.category,
            icon: env.widgets().category_icon(spec.category),
            flags,
            title: translated(env.i18n(), spec.title, caps::TITLE),
            cells: spec.cells,
            previews: spec.previews,
        }))
    })
}

/// Whether a place that shows now lists widget `id`: one of the environment's
/// places (`Widgets::shown`) or a native `widget.place`.
fn listed(env: &Env<'_>, store: &DeclarationStore, id: &str) -> bool {
    if env.widgets().shown(env, id) {
        return true;
    }
    let mut found = false;
    store.keys(Kind::WidgetPlace, &mut |key| {
        let _read = ordered::with_plugin(store, key, &mut |record, _| {
            if let Record::WidgetPlace(place) = record {
                found |= place_lists(env, &place, id);
            }
            ControlFlow::Continue(())
        });
    });
    found
}

fn place_lists(env: &Env<'_>, place: &wire::WidgetPlace<'_>, id: &str) -> bool {
    rules::value_holds(env.settings(), place.shown_if_key, place.shown_if_value)
        && place
            .list_keys
            .split('\n')
            .filter(|key| !key.is_empty())
            .any(|key| {
                env.settings()
                    .text(key)
                    .split(',')
                    .filter_map(|item| item.split_whitespace().next())
                    .any(|item| base(item) == id)
            })
}
