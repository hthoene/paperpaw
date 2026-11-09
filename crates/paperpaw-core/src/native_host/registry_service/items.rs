//! The list ops over the environment's registries: 4 `groups`, 7 `actions`,
//! 8 `screens`, 9 `slots`, 10 `slot_offers` and 14 `inputs` (`registry.md`
//! 2.2). Natively declared actions, screens, setting groups and slot offers
//! are merged into those registries when a plugin is installed, so they are
//! read from there; only the slot specs (a slot's name) stay in the
//! declaration store.
use super::{
    Row,
    fixed::{Fixed, caps, clip, translated},
    schema,
    sources::Sources,
};
use crate::native_host::{
    declaration_store::Kind,
    registrations::{self as wire, Record},
};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_UNSUPPORTED};
use paperpaw_sdk::{Id, keys::InputKind};

/// `reg_item.flags` of actions, screens and inputs.
const SEALED: u32 = 1 << 1;
const GESTURE: u32 = 1 << 2;
const DECLARED: u32 = 1 << 4;
/// `reg_slot.flags`: the user bound it.
const USER_BOUND: u32 = 1;

const ID: usize = caps::ID;
const TITLE: usize = caps::TITLE;

/// A `reg_item` row.
fn item<'a>(id: &'a str, plugin: &'a str, flags: u32, title: &'a str) -> Record<'a> {
    Record::ServiceRegItem(wire::ServiceRegItem {
        id,
        plugin,
        flags,
        title: clip(title, TITLE),
    })
}

/// The id, the plugin that owns it and the title as text: the part of the id
/// before its last dot is the plugin.
fn namespace_of(id: &Id) -> &str {
    id.namespace()
}

pub(super) fn rows(
    sources: &Sources<'_>,
    op: u32,
    scope: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    match op {
        schema::OP_GROUPS => groups(sources, scope, visit),
        schema::OP_ACTIONS => actions(sources, scope, visit),
        schema::OP_SCREENS => screens(sources, scope, visit),
        schema::OP_SLOTS => slots(sources, visit),
        schema::OP_SLOT_OFFERS => offers(sources, scope, visit),
        schema::OP_INPUTS => inputs(sources, visit),
        _ => Err(ERR_UNSUPPORTED),
    }
}

/// Op 4: every setting group; with a scope, the groups whose parent it is.
fn groups(
    sources: &Sources<'_>,
    scope: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let translator = sources.env.i18n();
    for group in sources.env.settings().groups() {
        let parent = group.parent.as_ref().map_or("", Id::as_str);
        if !scope.is_empty() && parent != scope {
            continue;
        }
        let row = Record::ServiceRegGroup(wire::ServiceRegGroup {
            id: group.id.as_str(),
            parent,
            page: group.page.as_ref().map_or("", Id::as_str),
            order: group.order,
            title: translated(translator, &group.title, caps::TITLE),
        });
        if visit(&row).is_break() {
            break;
        }
    }
    Ok(())
}

/// Op 7: the actions, those of plugin `scope` (empty: all), by id.
fn actions(
    sources: &Sources<'_>,
    scope: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let translator = sources.env.i18n();
    for action in sources.env.actions().iter() {
        let id = action.id();
        if !scope.is_empty() && namespace_of(id) != scope {
            continue;
        }
        let flags = if action.is_sealed() { SEALED } else { 0 };
        let title = translated(translator, action.title(), TITLE);
        if visit(&item(id.as_str(), namespace_of(id), flags, title)).is_break() {
            break;
        }
    }
    Ok(())
}

/// Op 8: the screens, those of plugin `scope` (empty: all).
fn screens(
    sources: &Sources<'_>,
    scope: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let translator = sources.env.i18n();
    for (id, title) in sources.env.screens().iter() {
        if !scope.is_empty() && namespace_of(id) != scope {
            continue;
        }
        let title = translated(translator, title, TITLE);
        if visit(&item(id.as_str(), namespace_of(id), 0, title)).is_break() {
            break;
        }
    }
    Ok(())
}

/// Op 10: the screens offered for slot `scope`, highest priority first
/// (nothing for a slot that is not there).
fn offers(
    sources: &Sources<'_>,
    scope: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    if scope.is_empty() {
        return Ok(());
    }
    let translator = sources.env.i18n();
    // The walk allocates nothing (`Env::each_slot_screen`); a break ends it.
    sources.env.each_slot_screen(scope, &mut |screen| {
        let title = sources
            .env
            .screen_title(screen)
            .map_or("", |key| translated(translator, key, TITLE));
        visit(&item(screen.as_str(), namespace_of(screen), 0, title)).is_continue()
    });
    Ok(())
}

/// The slot `id` as a row, its name from `declared` where the registry has none.
fn slot_row(
    sources: &Sources<'_>,
    id: &str,
    declared: Option<&str>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let env = sources.env;
    let bound = env.slot_screen(id).map_or("", Id::as_str);
    let choice = env.slot_choice(id);
    // Where the user chose, the default is the highest offer; else it is the
    // screen the slot shows.
    let default = if choice.is_some() {
        let mut first = "";
        env.each_slot_screen(id, &mut |screen| {
            first = screen.as_str();
            false
        });
        first
    } else {
        bound
    };
    let key = env.slot_title(id).or(declared).unwrap_or("");
    visit(&Record::ServiceRegSlot(wire::ServiceRegSlot {
        id,
        screen: bound,
        default,
        flags: if choice.is_some() { USER_BOUND } else { 0 },
        title: translated(env.i18n(), key, caps::TITLE),
    }))
}

/// The title key a plugin's `slot.spec` gives slot `id`, where one does.
fn declared_title(sources: &Sources<'_>, id: &str, found: &mut dyn FnMut(&str)) -> Result<(), i32> {
    let Some(directory) = sources.directory else {
        return Ok(());
    };
    let Some((plugin, name)) = id.rsplit_once('.') else {
        return Ok(());
    };
    let store = directory.store.try_borrow().map_err(|_| ERR_BUSY)?;
    store.find(plugin, Kind::SlotSpec, name, &mut |record| {
        if let Record::SlotSpec(spec) = record {
            found(spec.title);
        }
    });
    Ok(())
}

/// Op 9: the slots some screen is bound to, then the declared slots nothing
/// is offered for yet.
fn slots(
    sources: &Sources<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    for slot in sources.env.slots() {
        let mut declared: Option<Fixed<{ caps::TITLE }>> = None;
        declared_title(sources, slot.as_str(), &mut |title| {
            declared = Fixed::of(&[clip(title, caps::TITLE)]);
        })?;
        let title = declared.as_ref().map(Fixed::as_str);
        if slot_row(sources, slot.as_str(), title, visit).is_break() {
            return Ok(());
        }
    }
    let Some(directory) = sources.directory else {
        return Ok(());
    };
    let store = directory.store.try_borrow().map_err(|_| ERR_BUSY)?;
    let mut stopped = false;
    store.keys(Kind::SlotSpec, &mut |key| {
        if stopped {
            return;
        }
        let Some(plugin) = store.plugin_of(key) else {
            return;
        };
        store.with_record(key, &mut |record| {
            let Record::SlotSpec(spec) = record else {
                return;
            };
            let Some(id) = Fixed::<ID>::of(&[plugin, ".", spec.name]) else {
                return;
            };
            if sources
                .env
                .slots()
                .iter()
                .any(|slot| slot.as_str() == id.as_str())
            {
                return;
            }
            stopped = slot_row(sources, id.as_str(), Some(spec.title), visit).is_break();
        });
    });
    Ok(())
}

/// The inputs a device with buttons or a touch screen makes, in the order of
/// [`paperpaw_sdk::keys::inputs`], then those the plugins declared that this
/// device meets.
fn inputs(
    sources: &Sources<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    // A visitor that breaks only ends the list.
    let _ended = inputs_flow(sources, visit);
    Ok(())
}

fn inputs_flow(
    sources: &Sources<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> ControlFlow<()> {
    use paperpaw_sdk::keys::{EDGES, PINCHES, SWIPES, ZONES};
    let env = sources.env;
    let caps = &env.device().input;
    let translator = env.i18n();
    let mut row = |id: &str, gesture: bool| {
        let flags = if gesture { GESTURE } else { 0 };
        visit(&item(id, "", flags, translated(translator, id, TITLE)))
    };
    let mut put = |parts: &[&str], gesture: bool| -> ControlFlow<()> {
        match Fixed::<ID>::of(parts) {
            Some(id) => row(id.as_str(), gesture),
            None => ControlFlow::Continue(()),
        }
    };
    for key in caps.keys {
        for kind in ["button", "long", "double"] {
            put(&[kind, ".", key], false)?;
        }
    }
    if caps.touch {
        for zone in ZONES {
            for kind in ["tap", "long-tap", "double-tap"] {
                put(&[kind, ".", zone], false)?;
            }
        }
        put(&["tap2"], true)?;
        for direction in SWIPES {
            put(&["swipe.", direction], true)?;
        }
        for edge in EDGES {
            put(&["edge.", edge], true)?;
        }
        for pinch in PINCHES {
            put(&["pinch.", pinch], true)?;
        }
    }
    // A plugin's own: `input.<id>` or `gesture.<id>`, by the owner.
    for spec in env.inputs() {
        if spec.need.is_some_and(|need| !need.met(env.caps())) {
            continue;
        }
        let gesture = spec.kind == InputKind::Gesture;
        let prefix = if gesture { "gesture." } else { "input." };
        let Some(id) = Fixed::<ID>::of(&[prefix, spec.id.as_str()]) else {
            continue;
        };
        let flags = DECLARED | if gesture { GESTURE } else { 0 };
        let title = translated(translator, &spec.title, TITLE);
        visit(&item(id.as_str(), namespace_of(&spec.id), flags, title))?;
    }
    native_inputs(sources, visit)
}

/// The inputs and gestures natively declared (`input.spec`, `ui.md` section
/// 2) that this device meets. A store in use leaves them out of this page.
fn native_inputs(
    sources: &Sources<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let Some(directory) = sources.directory else {
        return ControlFlow::Continue(());
    };
    let Ok(store) = directory.store.try_borrow() else {
        return ControlFlow::Continue(());
    };
    let env = sources.env;
    let mut flow = ControlFlow::Continue(());
    store.keys(Kind::InputSpec, &mut |key| {
        if flow.is_break() {
            return;
        }
        flow = super::ordered::with_plugin(&store, key, &mut |record, plugin| {
            let Record::InputSpec(spec) = record else {
                return ControlFlow::Continue(());
            };
            // Bit 0 `has_need`: the device meets it, or the row is not offered.
            if spec.flags & 1 != 0
                && !super::rules::need_met(spec.need, spec.capability, env.caps())
            {
                return ControlFlow::Continue(());
            }
            let gesture = spec.kind == 1;
            let prefix = if gesture { "gesture." } else { "input." };
            let Some(id) = Fixed::<ID>::of(&[prefix, plugin, ".", spec.name]) else {
                return ControlFlow::Continue(());
            };
            let flags = DECLARED | if gesture { GESTURE } else { 0 };
            let title = translated(env.i18n(), spec.title, TITLE);
            visit(&item(id.as_str(), plugin, flags, title))
        });
    });
    flow
}
