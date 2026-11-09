//! The `core.input-filter` chain on the kernel's input path (`docs/abi/ui.md`
//! 4.1 and 4.3): [`crate::Kernel::handle`] asks the native filters first, then
//! the interface-6 ones, then screens, gestures and key maps.
//!
//! The chain itself (verdicts, captures, the guard rule) is `ui_input.rs`; this
//! is what is around it: an input event as the stage's `input.key` or
//! `input.pointer` record, the subscriptions read from the declaration store
//! (at most 4, in stage order), the runner over the stage broker at depth 0,
//! and the verdict as something `handle` can act on. Power is never filtered;
//! a failed or invalid filter passes the input (fail open, logged by the
//! count of refusals); a replaced key whose name is not one of the device's or
//! platform's known buttons passes unchanged (the key path takes `&'static`
//! names), a replaced pointer is clamped to the display, and a replaced
//! gesture, like a `raise`, waits for the plugins' gesture declarations
//! (`BLOCKED UIX-input-spec`: the host does not admit `input.spec` yet, so no
//! plugin has an input or gesture to raise).
use super::{
    declaration_store::DeclarationStore,
    registrations::{self as wire, Record},
    stage_broker::Subscription,
    stages_wire::core_input_filter_v1 as stage,
    ui_guard::UiStackView,
    ui_input::{BrokerRunner, Dispatched, FilterRunner, InputFilters, Policy, Routed, schema},
};
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_platform::{
    ButtonState, InputEvent, Point, PointerEvent, PointerPhase, PointerTool, buttons,
};

/// What the native filters made of one input.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Native {
    /// On to the interface-6 filters, screens, gestures and key maps.
    Pass,
    /// This input in its place.
    Replaced(InputEvent),
    /// Nothing else sees it.
    Taken,
    /// Nothing else sees it, and what it began is forgotten.
    Cancelled,
    /// Run this action instead (an action id).
    Run(String),
}

/// The buttons the platform names: the only key names a filter can make.
const PLATFORM_BUTTONS: [&str; 11] = [
    buttons::PAGE_FORWARD,
    buttons::PAGE_BACK,
    buttons::UP,
    buttons::DOWN,
    buttons::LEFT,
    buttons::RIGHT,
    buttons::CONFIRM,
    buttons::BACK,
    buttons::MENU,
    buttons::HOME,
    buttons::POWER,
];

/// `input.key.state`: pressed, released.
const KEY_PRESSED: u32 = 0;
const KEY_RELEASED: u32 = 1;

/// The input as the stage's input record; `None` for what is not filtered (a
/// hovering pen, and Power: its escape and power actions come from the
/// physical button alone).
pub(crate) fn record_of(event: &InputEvent, now_ms: u64) -> Option<Record<'static>> {
    match *event {
        InputEvent::Button { name, state } => {
            (name != buttons::POWER).then_some(Record::InputKey(wire::InputKey {
                now_ms,
                key: name,
                state: match state {
                    ButtonState::Pressed => KEY_PRESSED,
                    ButtonState::Released => KEY_RELEASED,
                    _ => return None,
                },
            }))
        }
        InputEvent::Pointer(pointer) => {
            let phase = match pointer.phase {
                PointerPhase::Down => 0,
                PointerPhase::Move => 1,
                PointerPhase::Up => 2,
                PointerPhase::Cancel => 3,
                _ => return None,
            };
            Some(Record::InputPointer(wire::InputPointer {
                now_ms,
                x: pointer.point.x,
                y: pointer.point.y,
                phase,
                contact: Some(u32::from(pointer.id)),
            }))
        }
        _ => None,
    }
}

/// The input a filter's `replace` made, as an event the kernel can route:
/// `None` where it cannot be one (an unknown key name, a gesture, a state or
/// phase that is none of ours), and the input then passes as it was.
/// `original` gives a pointer its tool and pressure; `display` is the width
/// and height a pointer is clamped to; `device_keys` are the device's buttons.
pub(crate) fn event_of(
    record: &Record<'_>,
    original: &InputEvent,
    display: (u16, u16),
    device_keys: &[&'static str],
) -> Option<InputEvent> {
    match record {
        Record::InputKey(key) => {
            let name = PLATFORM_BUTTONS
                .iter()
                .chain(device_keys)
                .copied()
                .find(|known| *known == key.key)?;
            // A filter never makes Power.
            if name == buttons::POWER {
                return None;
            }
            let state = match key.state {
                KEY_PRESSED => ButtonState::Pressed,
                KEY_RELEASED => ButtonState::Released,
                _ => return None,
            };
            Some(InputEvent::Button { name, state })
        }
        Record::InputPointer(pointer) => {
            let phase = match pointer.phase {
                0 => PointerPhase::Down,
                1 => PointerPhase::Move,
                2 => PointerPhase::Up,
                3 => PointerPhase::Cancel,
                _ => return None,
            };
            let (width, height) = display;
            let point = Point::new(
                pointer.x.clamp(0, i32::from(width).saturating_sub(1)),
                pointer.y.clamp(0, i32::from(height).saturating_sub(1)),
            );
            let id = u8::try_from(pointer.contact.unwrap_or(0)).ok()?;
            Some(InputEvent::Pointer(match *original {
                InputEvent::Pointer(was) => was.with_id(id).with_phase(phase).with_point(point),
                _ => PointerEvent::new(id, phase, point, PointerTool::Finger, 1000),
            }))
        }
        _ => None,
    }
}

/// The subscribers of `core.input-filter` in stage order, at most
/// [`super::ui_input::MAX_FILTERS`]. Reserved fallibly.
pub(crate) fn subscriptions(store: &DeclarationStore) -> Result<Vec<Subscription<'static>>, i32> {
    let mut found = store.stage_subscribers(stage::KEY)?;
    found.sort_unstable_by(|a, b| (a.order, a.plugin, a.index).cmp(&(b.order, b.plugin, b.index)));
    found.truncate(super::ui_input::MAX_FILTERS);
    let mut rows = Vec::new();
    rows.try_reserve_exact(found.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    rows.extend(found.iter().map(|subscriber| Subscription {
        stage: stage::KEY,
        plugin: subscriber.owner,
        token: subscriber.token,
        order: subscriber.order,
        index: subscriber.index,
        signature: subscriber.signature,
        enabled: true,
        style_granted: false,
    }));
    Ok(rows)
}

/// One input through the chain with `runner`: what it made of it, and how
/// many filters were refused (failed or invalid callbacks, replies and
/// verdicts: the input passed them).
pub(crate) fn run_chain(
    chain: &mut InputFilters,
    runner: &mut dyn FilterRunner,
    filters: &[Subscription<'_>],
    policy: &Policy<'_>,
    event: &InputEvent,
    (now_ms, display, device_keys): (u64, (u16, u16), &[&'static str]),
) -> (Native, usize) {
    let Some(record) = record_of(event, now_ms) else {
        return (Native::Pass, 0);
    };
    let Ok(Dispatched {
        routed,
        mut refused,
    }) = chain.dispatch(runner, filters, policy, &record)
    else {
        return (Native::Pass, 0);
    };
    let native = match routed {
        Routed::Pass => Native::Pass,
        Routed::Replaced(next) => match next
            .record()
            .ok()
            .and_then(|record| event_of(&record, event, display, device_keys))
        {
            Some(next) => Native::Replaced(next),
            None => {
                refused += 1;
                Native::Pass
            }
        },
        Routed::Taken => Native::Taken,
        Routed::Cancelled => Native::Cancelled,
        Routed::Run(action) => Native::Run(String::from(action.as_str())),
        // No plugin can declare an input or gesture yet, so `declared`
        // answers no and a raise never gets here.
        Routed::Raise { .. } => Native::Pass,
    };
    (native, refused)
}

impl crate::Kernel {
    /// The native filters' verdict on `event`, before everything else.
    /// Nothing runs when no native plugin is installed (no declaration
    /// directory, no shared depth) or for an input that is not filtered.
    pub(crate) fn native_input(
        &mut self,
        now: paperpaw_platform::Instant,
        event: &InputEvent,
    ) -> Native {
        if record_of(event, 0).is_none() {
            return Native::Pass;
        }
        // The captures are the chain's only state: taken out for the call, so
        // that the rest of the kernel can be read meanwhile.
        let mut chain = core::mem::take(&mut self.shared.native_input);
        let (native, refused) = self.native_input_run(&mut chain, now, event);
        self.shared.native_input = chain;
        // Told once while filters keep failing, not at every key.
        if refused > 0 && !self.shared.native_input_refused {
            self.warn_runtime("a native input filter failed or answered badly: its input passed");
        }
        self.shared.native_input_refused = refused > 0;
        native
    }

    fn native_input_run(
        &self,
        chain: &mut InputFilters,
        now: paperpaw_platform::Instant,
        event: &InputEvent,
    ) -> (Native, usize) {
        let shared = &self.shared;
        let (Some(directory), Some(depth)) = (
            shared.registry.native_declarations.as_deref(),
            shared.native_depth.as_ref(),
        ) else {
            return (Native::Pass, 0);
        };
        // The store is released before any filter runs.
        let Ok(filters) = directory
            .store
            .try_borrow()
            .map_err(|_| ERR_NO_MEMORY)
            .and_then(|store| subscriptions(&store))
        else {
            return (Native::Pass, 1);
        };
        if filters.is_empty() {
            return (Native::Pass, 0);
        }
        let env = shared.env(now);
        let stage_schema = schema();
        let mut runner = BrokerRunner {
            schema: &stage_schema,
            depth: depth.as_depth(),
            registry: &shared.native_instances,
            env: &env,
        };
        let stack: &dyn UiStackView = &*shared.stack_rows;
        let runs = |plugin: u64| super::ui_guard::filter_runs(stack, plugin);
        let declared = |_: u64, _: &str| false;
        let policy = Policy {
            touch: shared.device.input.touch || shared.device.input.pen,
            secret: super::ui_guard::top_is_secret(stack),
            runs: &runs,
            declared: &declared,
        };
        let display = (shared.device.display.width, shared.device.display.height);
        run_chain(
            chain,
            &mut runner,
            &filters,
            &policy,
            event,
            (now.millis(), display, shared.device.input.keys),
        )
    }
}

#[cfg(test)]
#[path = "input_chain_tests.rs"]
mod tests;
