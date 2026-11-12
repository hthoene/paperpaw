//! Ops 11 `menus` and 12 `menu` (`registry.md` 2.2 and 2.4).
//!
//! Op 11 lists every menu id: those the environment's `Menus` knows (code
//! plugins, the host, the user's own definitions) and those natively declared
//! plugins named (`NativeDeclarations::menu_ids`), each once, with `flags`
//! bit 5 where the user has a definition (interface 6 `is_customized`).
//!
//! Op 12 lists the rows the host would show now. Its sources are the same two:
//! the rows `Menus` holds and the rows natively declared plugins put in the
//! declaration store (`menu_native`). Without a user definition the rows are
//! the declared ones in declared order (`Menus` order with the native rows by
//! order among them, those placed first before and those placed last after the
//! others); with one, the user's list. Query `flags` bit 0 (`all`) adds the
//! rows that could be in the menu and are not (hidden, removed); bit 1
//! (`declared`) reads the declared rows whatever the user chose.
use super::{
    Row,
    fixed::{Fixed, caps, clip, translated},
    menu_native::{BY_ORDER_END, Entry, FIRST_END, Native},
    ordered::Sort,
    sources::Sources,
};
use crate::native_host::item_state::ItemStates;
use crate::native_host::{
    native_menus::MENU_USER_DEFINED,
    registrations::{self as wire, Record},
};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_sdk::{Env, Id, ItemState, SettingKind};

/// `reg_query.flags` of op 12.
const ALL: u32 = 1;
const DECLARED: u32 = 1 << 1;

/// `reg_menu_row.flags`.
const CHECKED: u32 = 1;
const ENABLED: u32 = 1 << 1;
const HIDDEN: u32 = 1 << 2;
const REMOVED: u32 = 1 << 3;
const USER_MENU: u32 = 1 << 4;
const IN_DECLARED: u32 = 1 << 5;
const SWITCH: u32 = 1 << 6;

/// `reg_menu_row.kind`.
const ACTION: u32 = 0;
const SETTING: u32 = 1;
const SUBMENU: u32 = 2;
const SECTION: u32 = 3;

/// Op 11.
pub(super) fn menus(
    sources: &Sources<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let env = sources.env;
    let menus = env.menus();
    let translator = env.i18n();
    // Allocation: the SDK builds this list of references (4 or 8 bytes an
    // entry) as the kernel itself does for every menu it shows. Its size is
    // bounded by the entries the plugins declared, each of which the kernel
    // already holds in at least 64 bytes, so a failure here is a failure of
    // the kernel's own menus first; it is not a growth the caller controls.
    let ids = menus.menu_ids();
    let mut item = |id: &str, flags: u32| {
        let title = menus
            .title(id)
            .map_or("", |key| translated(translator, key, caps::TITLE));
        let plugin = id.split('.').next().unwrap_or("");
        visit(&Record::ServiceRegItem(wire::ServiceRegItem {
            id,
            plugin,
            flags,
            title,
        }))
    };
    let flags_of = |id: &str| {
        if menus.is_customized(id) {
            MENU_USER_DEFINED
        } else {
            0
        }
    };
    for id in &ids {
        if item(id.as_str(), flags_of(id.as_str())).is_break() {
            return Ok(());
        }
    }
    let Some(directory) = sources.directory else {
        return Ok(());
    };
    let mut stopped = false;
    // The user's own menus are `Menus`' already: `custom` is empty here.
    directory.menu_ids(
        &mut core::iter::empty(),
        &|menu| menus.is_customized(menu),
        &mut |menu, flags| {
            if stopped || ids.iter().any(|id| id.as_str() == menu) {
                return;
            }
            stopped = item(menu, flags).is_break();
        },
    )
}

/// What op 12 reads for one call.
struct Call<'a> {
    env: &'a Env<'a>,
    native: Option<Native<'a>>,
    menu: &'a str,
    /// The entries plugins contributed, as `Menus` has them (hides applied).
    contributed: alloc::vec::Vec<&'a Id>,
    customized: bool,
    /// The `declared` read: the user's definition is ignored.
    declared_view: bool,
    /// The states plugins pushed (`command.set_item_state`), read live.
    pushed: Option<core::cell::Ref<'a, ItemStates>>,
}

/// A declared row, wherever it comes from.
struct Item<'a> {
    id: &'a str,
    by: &'a str,
    order: i32,
    /// A native heading's message key.
    title: Option<&'a str>,
    section: bool,
    /// The plugin whose `menu.hide` takes it out.
    hidden_by: Option<&'a str>,
}

/// Op 12.
pub(super) fn menu(
    sources: &Sources<'_>,
    scope: &str,
    flags: u32,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let env = sources.env;
    let guard = match sources.directory {
        Some(directory) => Some(directory.store.try_borrow().map_err(|_| ERR_BUSY)?),
        None => None,
    };
    let pushed = match sources.directory {
        Some(directory) => Some(directory.items.try_borrow().map_err(|_| ERR_BUSY)?),
        None => None,
    };
    let menus = env.menus();
    // Allocation: the SDK builds this list of references (4 or 8 bytes an
    // entry) as the kernel itself does for every menu it shows. Its size is
    // bounded by the entries the plugins declared, each of which the kernel
    // already holds in at least 64 bytes, so a failure here is a failure of
    // the kernel's own menus first; it is not a growth the caller controls.
    let contributed = menus.contributed_ids(scope);
    let call = Call {
        env,
        native: guard.as_deref().map(|store| Native::new(store, env.caps())),
        menu: scope,
        contributed,
        customized: menus.is_customized(scope),
        declared_view: flags & DECLARED != 0,
        pushed,
    };
    let known = menus.menu_ids().iter().any(|id| id.as_str() == scope)
        || call
            .native
            .as_ref()
            .is_some_and(|native| native.names_menu(scope));
    if scope.is_empty() || !known {
        return Ok(());
    }
    let all = flags & ALL != 0;
    if call.customized && !call.declared_view {
        let _end = user_rows(&call, all, visit);
    } else {
        let _end = declared(&call, all, &mut |item| {
            // The default set carries no user flag; a hidden row is flagged.
            let hidden = item.hidden_by.is_some();
            let bits = if hidden { HIDDEN } else { IN_DECLARED };
            row(&call, item, bits, all, false, &mut *visit)
        });
    }
    Ok(())
}

/// The user's list, then (`all`) the declared rows it leaves out and the
/// hidden ones.
fn user_rows(
    call: &Call<'_>,
    all: bool,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let menus = call.env.menus();
    let listed = menus.ids(call.menu);
    for id in &listed {
        // Listed by the user: `no-touch` leaves it in, other needs do not
        // (`Menus::ids` applied its own).
        if call
            .native
            .as_ref()
            .is_some_and(|n| !n.need_met(id.as_str(), true))
        {
            continue;
        }
        let title = call
            .native
            .as_ref()
            .and_then(|n| n.section_title(id.as_str()));
        let item = Item {
            id: id.as_str(),
            by: id.namespace(),
            order: order_of(call, id.as_str()),
            title: title.as_ref().map(Fixed::as_str),
            section: title.is_some() || menus.is_section(id.as_str()),
            hidden_by: None,
        };
        let bits = if in_default_set(call, id.as_str()) {
            IN_DECLARED
        } else {
            0
        };
        row(call, &item, bits, all, true, visit)?;
    }
    if !all {
        return ControlFlow::Continue(());
    }
    declared(call, true, &mut |item| {
        if listed.iter().any(|id| id.as_str() == item.id) {
            return ControlFlow::Continue(());
        }
        let bits = if item.hidden_by.is_some() {
            HIDDEN
        } else {
            REMOVED | IN_DECLARED
        };
        row(call, item, bits, true, true, visit)
    })
}

/// The declared position of `item`, 0 where it has none.
fn order_of(call: &Call<'_>, item: &str) -> i32 {
    call.env
        .menus()
        .order_of(call.menu, item)
        .map_or(0, i32::from)
}

/// Whether `item` is among the rows the plugins declare into the menu and
/// show on this device (the editor's default set).
fn in_default_set(call: &Call<'_>, item: &str) -> bool {
    let env_hidden = call.env.menus().hidden_by(call.menu, item).is_some();
    let native = call.native.as_ref();
    let hidden = env_hidden || native.is_some_and(|n| n.hidden_by(call.menu, item).is_some());
    let declared = call.contributed.iter().any(|id| id.as_str() == item)
        || native.is_some_and(|n| n.declares(call.menu, item));
    declared
        && !hidden
        && call.env.menus().available(item)
        && native.is_none_or(|n| n.need_met(item, false))
}

/// The declared rows in declared order: `Menus`' contributed entries, then
/// the native ones. Rows whose need the device does not meet are never
/// visited; hidden rows (`menu.hide`) only with `hidden`.
fn declared(
    call: &Call<'_>,
    hidden: bool,
    visit: &mut dyn FnMut(&Item<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let menus = call.env.menus();
    let native = call.native.as_ref();
    let hider =
        |item: &str| -> Option<Fixed<64>> { native.and_then(|n| n.hidden_by(call.menu, item)) };
    let usable =
        |item: &str| menus.available(item) && native.is_none_or(|n| n.need_met(item, false));
    // A native row: not one `Menus` has, usable here, hidden or not.
    let native_row = |entry: &Entry,
                      visit: &mut dyn FnMut(&Item<'_>) -> ControlFlow<()>|
     -> ControlFlow<()> {
        let Entry {
            id,
            by,
            order,
            section,
            title,
        } = entry;
        let id = id.as_str();
        if id.is_empty() || call.contributed.iter().any(|known| known.as_str() == id) || !usable(id)
        {
            return ControlFlow::Continue(());
        }
        let hider = menus
            .hidden_by(call.menu, id)
            .map(|by| Fixed::<64>::of(&[by.as_str()]).unwrap_or_else(Fixed::new))
            .or_else(|| hider(id));
        if hider.is_some() && !hidden {
            return ControlFlow::Continue(());
        }
        visit(&Item {
            id,
            by: by.as_str(),
            order: *order,
            title: section.then(|| title.as_str()),
            section: *section,
            hidden_by: hider.as_ref().map(Fixed::as_str),
        })
    };
    // Those placed first, then `Menus`' rows with the native ones by order
    // among them (a native row of the same order after), then those placed
    // last.
    let mut after: Option<Sort> = None;
    // The row read and not yet due: `next` scans the store, so it is kept.
    let mut pending: Option<(Sort, Entry)> = None;
    if let Some(native) = native {
        while let Some((sort, entry)) = native.next(call.menu, after) {
            if sort.0 >= FIRST_END {
                pending = Some((sort, entry));
                break;
            }
            after = Some(sort);
            native_row(&entry, &mut *visit)?;
        }
    }
    // The native rows by order that come before `limit` (all with `None`).
    let mut by_order = |limit: Option<i32>,
                        visit: &mut dyn FnMut(&Item<'_>) -> ControlFlow<()>|
     -> ControlFlow<()> {
        let Some(native) = native else {
            return ControlFlow::Continue(());
        };
        while let Some((sort, entry)) = pending.take().or_else(|| native.next(call.menu, after)) {
            let due = sort.0 < BY_ORDER_END && limit.is_none_or(|limit| entry.order < limit);
            if !due {
                pending = Some((sort, entry));
                break;
            }
            after = Some(sort);
            native_row(&entry, &mut *visit)?;
        }
        ControlFlow::Continue(())
    };
    // `contributed` lists the rows by order, sorted, then those placed
    // beside others: an order that falls marks where those begin, and the
    // native rows by order all come before them.
    let mut reached = i32::MIN;
    for id in &call.contributed {
        if !usable(id.as_str()) {
            continue;
        }
        let by = hider(id.as_str());
        if by.is_some() && !hidden {
            continue;
        }
        let here = order_of(call, id.as_str());
        let limit = (here >= reached).then_some(here);
        reached = if limit.is_some() { here } else { i32::MAX };
        by_order(limit, &mut *visit)?;
        visit(&Item {
            id: id.as_str(),
            by: id.namespace(),
            order: here,
            title: None,
            section: menus.is_section(id.as_str()),
            hidden_by: by.as_ref().map(Fixed::as_str),
        })?;
    }
    if let Some(native) = native {
        while let Some((sort, entry)) = pending.take().or_else(|| native.next(call.menu, after)) {
            after = Some(sort);
            native_row(&entry, &mut *visit)?;
        }
    }
    if hidden {
        env_hidden(call, &usable, visit)?;
    }
    ControlFlow::Continue(())
}

/// The entries `Menus` holds but plugins hid with `hide_menu_entry`; they are
/// not in `contributed_ids`, so they are found through the entries the menus
/// reference.
fn env_hidden(
    call: &Call<'_>,
    usable: &dyn Fn(&str) -> bool,
    visit: &mut dyn FnMut(&Item<'_>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let menus = call.env.menus();
    for (at, item) in menus.referenced_items().enumerate() {
        let Some(by) = menus.hidden_by(call.menu, item.as_str()) else {
            continue;
        };
        let first = menus.referenced_items().position(|other| other == item) == Some(at);
        let is_entry = menus.order_of(call.menu, item.as_str()).is_some();
        let listed = call.contributed.contains(&item);
        if !first || !is_entry || listed || !usable(item.as_str()) {
            continue;
        }
        visit(&Item {
            id: item.as_str(),
            by: by.as_str(),
            order: order_of(call, item.as_str()),
            title: None,
            section: menus.is_section(item.as_str()),
            hidden_by: Some(by.as_str()),
        })?;
    }
    ControlFlow::Continue(())
}

/// One `reg_menu_row` for `item`; `bits` the flags known from where the row
/// came from. A row the item's state hides is left out unless `all` asks for
/// it; one the user listed is never hidden by state.
fn row(
    call: &Call<'_>,
    item: &Item<'_>,
    mut bits: u32,
    all: bool,
    user_row: bool,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let env = call.env;
    let menus = env.menus();
    let translator = env.i18n();
    let key = item
        .title
        .or_else(|| menus.title(item.id))
        .or_else(|| screen_title(env, item.id));
    let Some(key) = key else {
        return ControlFlow::Continue(());
    };
    let mut state: ItemState = menus.state(env, item.id);
    // What the plugin pushed for its own item wins over the SDK's state.
    let push = call.pushed.as_deref().and_then(|states| {
        let (plugin, relative) = item.id.split_once('.')?;
        states.get(plugin, relative)
    });
    if let Some(push) = push {
        state.shown = !push.hidden();
        state.enabled = !push.faint();
        state.checked = push.switch();
    }
    let (mut label_text, mut badge_text) = ([0_u8; caps::TITLE], [0_u8; 64]);
    let pushed_label =
        push.and_then(|p| shown(translator, p.label(), p.label_args(), &mut label_text));
    let pushed_badge =
        push.and_then(|p| shown(translator, p.badge(), p.badge_args(), &mut badge_text));
    if !state.shown && !user_row {
        if !all {
            return ControlFlow::Continue(());
        }
        bits = bits & !IN_DECLARED | HIDDEN;
    }
    let spec = env.settings().spec(item.id);
    let switch_setting = spec.is_some_and(|spec| matches!(spec.kind, SettingKind::Bool));
    let on = if switch_setting {
        env.settings().bool(item.id)
    } else {
        state.checked == Some(true)
    };
    let kind = if item.section || menus.is_section(item.id) {
        SECTION
    } else if spec.is_some() {
        SETTING
    } else if is_menu(call, item.id) {
        SUBMENU
    } else {
        ACTION
    };
    let flags = bits
        | if on { CHECKED } else { 0 }
        | if state.enabled { ENABLED } else { 0 }
        | if call.customized && !call.declared_view {
            USER_MENU
        } else {
            0
        }
        | if switch_setting || state.checked.is_some() {
            SWITCH
        } else {
            0
        };
    let title = pushed_label.or(state.label.as_deref()).map_or_else(
        || translated(translator, key, caps::TITLE),
        |label| clip(label, caps::TITLE),
    );
    let badge = pushed_badge
        .or(state.badge.as_deref())
        .map_or("", |badge| clip(badge, 64));
    let native = call.native.as_ref();
    let native_icon = native.and_then(|n| n.icon(item.id));
    let icon = menus
        .icon(item.id)
        .or_else(|| native_icon.as_ref().map(Fixed::as_str))
        .unwrap_or("");
    let native_line = native.and_then(|n| n.describe(item.id));
    let describe = menus
        .description(item.id)
        .or_else(|| native_line.as_ref().map(Fixed::as_str))
        .map_or("", |key| translated(translator, key, caps::TITLE));
    let by = item.hidden_by.unwrap_or(item.by);
    visit(&Record::ServiceRegMenuRow(wire::ServiceRegMenuRow {
        item: item.id,
        kind,
        flags,
        order: item.order,
        by,
        title,
        icon,
        badge,
        describe,
    }))
}

/// A pushed label or badge as shown now: the catalog message of `key` with
/// its argument batch, in the user's language; `None` for none, or where it
/// cannot be resolved now (the item then shows its title and no badge).
fn shown<'b>(
    translator: &paperpaw_i18n::Translator,
    key: &str,
    args: &[u8],
    out: &'b mut [u8],
) -> Option<&'b str> {
    if key.is_empty() {
        return None;
    }
    let length = crate::native_host::formatting::format_into(translator, key, args, out).ok()?;
    core::str::from_utf8(out.get(..length)?)
        .ok()
        .filter(|text| !text.is_empty())
}

/// A screen's title key, for an item that is a screen.
fn screen_title<'a>(env: &Env<'a>, item: &str) -> Option<&'a str> {
    env.screens()
        .iter()
        .find(|(id, _)| id.as_str() == item)
        .map(|(_, title)| title)
}

/// Whether `item` names a menu that has rows: an action named like a menu is
/// shown as that submenu.
fn is_menu(call: &Call<'_>, item: &str) -> bool {
    let menus = call.env.menus();
    // `contributed_ids` allocates one reference an entry of that menu, as
    // above (and no more than the entries the kernel holds for it).
    menus.is_customized(item)
        || !menus.contributed_ids(item).is_empty()
        || call.native.as_ref().is_some_and(|n| n.names_menu(item))
}
