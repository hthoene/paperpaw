//! Op 13 `bindings` (`registry.md` 2.2): the key bindings in effect, per layer.
//! The environment's `KeyBindings` answers one layer at a time and does not
//! name its layers, so the layers read are the ones a binding can name
//! (`command.bind_key`: a screen or slot id, or `global`).
use super::{Row, sources::Sources};
use crate::native_host::registrations::{self as wire, Record};
use core::ops::ControlFlow;
use paperpaw_sdk::{builtin::GLOBAL_KEYMAP_LAYER, keys::BindingOrigin};

/// `reg_binding.origin` is interface 6's `BindingOrigin` in order.
fn origin(origin: BindingOrigin) -> u32 {
    match origin {
        BindingOrigin::Device => 1,
        BindingOrigin::Preset => 2,
        BindingOrigin::User => 3,
        _ => 0,
    }
}

/// One layer's bindings.
fn layer(
    sources: &Sources<'_>,
    layer: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> ControlFlow<()> {
    for (input, action, from) in sources.env.keys().bindings(layer) {
        let row = Record::ServiceRegBinding(wire::ServiceRegBinding {
            layer,
            input: &input,
            action: action.as_ref().map_or("", |id| id.as_str()),
            origin: origin(from),
        });
        visit(&row)?;
    }
    ControlFlow::Continue(())
}

/// Op 13: layer `scope`, or with an empty scope `global`, then the slots and
/// the screens; a layer nothing binds in gives no row.
pub(super) fn rows(
    sources: &Sources<'_>,
    scope: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let _end = walk(sources, scope, visit);
    Ok(())
}

fn walk(
    sources: &Sources<'_>,
    scope: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> ControlFlow<()> {
    if !scope.is_empty() {
        return layer(sources, scope, visit);
    }
    let env = sources.env;
    layer(sources, GLOBAL_KEYMAP_LAYER, visit)?;
    for slot in env.slots() {
        layer(sources, slot.as_str(), visit)?;
    }
    for (screen, _) in env.screens().iter() {
        // A screen can also be a slot's id: its layer is read once.
        if !env.slots().iter().any(|slot| slot == screen) {
            layer(sources, screen.as_str(), visit)?;
        }
    }
    ControlFlow::Continue(())
}
