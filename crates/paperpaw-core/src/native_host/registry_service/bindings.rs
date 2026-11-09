//! Op 13 `bindings` (`registry.md` 2.2): the key bindings in effect, per layer.
//! The environment's `KeyBindings` walks its layers and each layer's bindings
//! without a list (`each_layer`, `each_binding`), so an empty scope reaches
//! every layer, whatever a binding names it (a screen, a slot, `global`, a
//! layer of its own).
use super::{Row, sources::Sources};
use crate::native_host::registrations::{self as wire, Record};
use core::ops::ControlFlow;
use paperpaw_sdk::keys::BindingOrigin;

/// `reg_binding.origin` is interface 6's `BindingOrigin` in order.
fn origin(origin: BindingOrigin) -> u32 {
    match origin {
        BindingOrigin::Device => 1,
        BindingOrigin::Preset => 2,
        BindingOrigin::User => 3,
        _ => 0,
    }
}

/// One layer's bindings. Nothing is allocated (`each_binding` names the
/// inputs on the stack).
fn layer(
    sources: &Sources<'_>,
    layer: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> ControlFlow<()> {
    sources
        .env
        .keys()
        .each_binding(layer, &mut |input, action, from| {
            let row = Record::ServiceRegBinding(wire::ServiceRegBinding {
                layer,
                input,
                action: action.map_or("", |id| id.as_str()),
                origin: origin(from),
            });
            visit(&row)
        })
}

/// Op 13: layer `scope`, or with an empty scope every layer that has a
/// binding, in the keymap's stable (sorted) order; a layer nothing binds in
/// gives no row.
pub(super) fn rows(
    sources: &Sources<'_>,
    scope: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let _end = if scope.is_empty() {
        sources
            .env
            .keys()
            .each_layer(&mut |name| layer(sources, name, visit))
    } else {
        layer(sources, scope, visit)
    };
    Ok(())
}
