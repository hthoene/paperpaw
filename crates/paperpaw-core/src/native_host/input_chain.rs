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
//! names), a replaced pointer is clamped to the display, a replaced gesture
//! is the kernel's own gesture at the clamped point, and a `raise` names an
//! input or gesture the filter's plugin declared with `input.spec`
//! (`ui.md` 2 and 4.1): the layers shown run what they bind it to.
use super::{
    declaration_store::{DeclarationStore, Kind},
    registrations::{self as wire, Record},
    stage_broker::Subscription,
    stages_wire::core_input_filter_v1 as stage,
    ui_guard::UiStackView,
    ui_input::{BrokerRunner, Dispatched, FilterRunner, InputFilters, Policy, Routed, schema},
};
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};
use paperpaw_platform::{
    ButtonState, InputEvent, Point, PointerEvent, PointerPhase, PointerTool, buttons,
};
use paperpaw_sdk::{Direction, Gesture, GestureKind};

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
    /// Raise this input of the filter's plugin (`input.<plugin>.<name>` or
    /// `gesture.<plugin>.<name>`) as key maps write it: what the layers shown
    /// bind it to runs, and no filter sees it.
    Raise(String),
    /// A gesture in place of the input (a replace that made an
    /// `input.gesture`): the screens get it as the kernel's own.
    Gesture(Gesture),
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

/// A gesture the kernel recognized as the stage's `input.gesture` record;
/// `None` for the kinds the record has none for (a pinch, a double tap, two
/// fingers), which no filter sees.
pub(crate) fn gesture_record(gesture: &Gesture, now_ms: u64) -> Option<Record<'static>> {
    let kind = match gesture.kind {
        GestureKind::Tap => 0,
        GestureKind::LongPress => 1,
        GestureKind::Swipe(Direction::Left) => 2,
        GestureKind::Swipe(Direction::Right) => 3,
        GestureKind::Swipe(Direction::Up) => 4,
        GestureKind::Swipe(Direction::Down) => 5,
        _ => return None,
    };
    Some(Record::InputGesture(wire::InputGesture {
        now_ms,
        kind,
        x: gesture.start.x,
        y: gesture.start.y,
    }))
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
            let point = clamped(pointer.x, pointer.y, display);
            let id = u8::try_from(pointer.contact.unwrap_or(0)).ok()?;
            Some(InputEvent::Pointer(match *original {
                InputEvent::Pointer(was) => was.with_id(id).with_phase(phase).with_point(point),
                _ => PointerEvent::new(id, phase, point, PointerTool::Finger, 1000),
            }))
        }
        _ => None,
    }
}

/// `x`, `y` on the display. A display of no width or height (a shell that has
/// not sized it yet) has no last pixel: the point goes to 0 (`clamp` panics
/// when its upper bound is below its lower).
fn clamped(x: i32, y: i32, (width, height): (u16, u16)) -> Point {
    Point::new(
        x.clamp(0, (i32::from(width) - 1).max(0)),
        y.clamp(0, (i32::from(height) - 1).max(0)),
    )
}

/// The gesture a filter's `replace` made (`input.gesture`: 0 tap, 1 long
/// press, 2-5 swipe left, right, up, down), at a point on the display, with the
/// tool of the pointer it replaces; `None` for any other record or kind.
pub(crate) fn gesture_of(
    record: &Record<'_>,
    original: &InputEvent,
    display: (u16, u16),
) -> Option<Gesture> {
    let Record::InputGesture(made) = record else {
        return None;
    };
    let kind = match made.kind {
        0 => GestureKind::Tap,
        1 => GestureKind::LongPress,
        2 => GestureKind::Swipe(Direction::Left),
        3 => GestureKind::Swipe(Direction::Right),
        4 => GestureKind::Swipe(Direction::Up),
        5 => GestureKind::Swipe(Direction::Down),
        _ => return None,
    };
    let tool = match original {
        InputEvent::Pointer(was) => was.tool,
        _ => PointerTool::Finger,
    };
    let point = clamped(made.x, made.y, display);
    Some(Gesture::new(kind, point, point, tool))
}

/// The subscribers of `core.input-filter`, kept with the declaration store's
/// revision they were read at: read again only when the store changed, so a
/// key allocates nothing for them.
pub(crate) struct FilterCache {
    revision: u64,
    filters: Vec<Subscription<'static>>,
    /// How many inputs in a row each filter's call failed (the same order).
    failures: Vec<u8>,
}

/// A filter whose call failed this many inputs in a row is left out of the
/// list (until the declarations change), so that one that keeps failing costs
/// no call, no list read and no warning at every key. Only a failure the
/// plugin caused counts (a trap, a stopped plugin, a refused or bad reply): a
/// transient host refusal (`ERR_BUSY`, `ERR_NO_MEMORY`) does not, or three
/// keys under memory pressure would switch a filter off until the next start.
const GIVE_UP: u8 = 3;

impl FilterCache {
    fn new(revision: u64, filters: Vec<Subscription<'static>>) -> Result<Self, i32> {
        let mut failures = Vec::new();
        failures
            .try_reserve_exact(filters.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        failures.resize(filters.len(), 0);
        Ok(Self {
            revision,
            filters,
            failures,
        })
    }

    /// One input later: `failed` are the filters whose call failed. Counts
    /// them, clears the others' counts and drops any that reached
    /// [`GIVE_UP`]; says whether one was dropped.
    fn settle(&mut self, failed: &[(u64, u32)]) -> bool {
        let mut dropped = false;
        let mut at = 0;
        while let (Some(filter), Some(count)) = (self.filters.get(at), self.failures.get_mut(at)) {
            if failed.contains(&(filter.plugin, filter.token)) {
                *count = count.saturating_add(1);
            } else {
                *count = 0;
            }
            if *count >= GIVE_UP {
                self.filters.remove(at);
                self.failures.remove(at);
                dropped = true;
            } else {
                at += 1;
            }
        }
        dropped
    }
}

/// A runner that notes which filters' calls failed (at most
/// [`super::ui_input::MAX_FILTERS`]).
struct Watching<R> {
    inner: R,
    failed: [(u64, u32); super::ui_input::MAX_FILTERS],
    count: usize,
}

impl<R: FilterRunner> FilterRunner for Watching<R> {
    fn call(
        &mut self,
        filter: &Subscription<'_>,
        payload: &[u8],
        reply: &mut [u8],
    ) -> Result<usize, i32> {
        let answer = self.inner.call(filter, payload, reply);
        if let Err(error) = answer
            && error != ERR_BUSY
            && error != ERR_NO_MEMORY
            && let Some(slot) = self.failed.get_mut(self.count)
        {
            *slot = (filter.plugin, filter.token);
            self.count += 1;
        }
        answer
    }
}

/// The subscribers of `core.input-filter` in stage order, at most
/// [`super::ui_input::MAX_FILTERS`]. Reserved fallibly.
pub(crate) fn subscriptions(store: &DeclarationStore) -> Result<Vec<Subscription<'static>>, i32> {
    let mut found = store.stage_subscribers(stage::KEY)?;
    paperpaw_sort::sort_unstable_by_key(&mut found, |s| (s.order, s.plugin, s.index));
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

/// Whether `plugin` declared the input or gesture `name` (`input.<plugin>.<name>`
/// or `gesture.<plugin>.<name>`, as key maps write it) with `input.spec` of
/// the matching kind.
pub(crate) fn declared_input(
    store: &core::cell::RefCell<DeclarationStore>,
    plugin: &str,
    name: &str,
) -> bool {
    let (kind, rest) = match (name.strip_prefix("input."), name.strip_prefix("gesture.")) {
        (Some(rest), _) => (0, rest),
        (_, Some(rest)) => (1, rest),
        _ => return false,
    };
    let Some(own) = rest
        .strip_prefix(plugin)
        .and_then(|rest| rest.strip_prefix('.'))
    else {
        return false;
    };
    let Ok(store) = store.try_borrow() else {
        return false;
    };
    let mut found = false;
    store.find(plugin, Kind::InputSpec, own, &mut |record| {
        found |= matches!(record, Record::InputSpec(spec) if spec.kind == kind);
    });
    found
}

/// One input through the chain with `runner`: what it made of it, and how
/// many filters were refused (failed or invalid callbacks, replies and
/// verdicts: the input passed them).
#[cfg(test)]
pub(crate) fn run_chain(
    chain: &mut InputFilters,
    runner: &mut dyn FilterRunner,
    filters: &[Subscription<'_>],
    policy: &Policy<'_>,
    event: &InputEvent,
    context @ (now_ms, ..): (u64, (u16, u16), &[&'static str]),
) -> (Native, usize) {
    match record_of(event, now_ms) {
        Some(record) => run_record(chain, runner, filters, policy, &record, event, context),
        None => (Native::Pass, 0),
    }
}

/// [`run_chain`] for `record`, an input as the stage's record. `event` is the
/// input it stands for (what a replace changes: a gesture stands for the
/// finger that made it).
#[inline(never)] // own frame: the input buffers stay out of the caller's (X4 frame band)
pub(crate) fn run_record(
    chain: &mut InputFilters,
    runner: &mut dyn FilterRunner,
    filters: &[Subscription<'_>],
    policy: &Policy<'_>,
    input: &Record<'_>,
    event: &InputEvent,
    (_, display, device_keys): (u64, (u16, u16), &[&'static str]),
) -> (Native, usize) {
    let Ok(Dispatched {
        routed,
        mut refused,
    }) = chain.dispatch(runner, filters, policy, input)
    else {
        return (Native::Pass, 0);
    };
    let native = match routed {
        Routed::Pass => Native::Pass,
        Routed::Replaced(next) => match next.record().ok().and_then(|record| {
            gesture_of(&record, event, display)
                .map(Native::Gesture)
                // A gesture is replaced by a gesture only: a key or a finger
                // made from it is refused (the original goes on).
                .or_else(|| {
                    if matches!(input, Record::InputGesture(_)) {
                        return None;
                    }
                    event_of(&record, event, display, device_keys).map(Native::Replaced)
                })
        }) {
            Some(native) => native,
            None => {
                refused += 1;
                Native::Pass
            }
        },
        Routed::Taken => Native::Taken,
        Routed::Cancelled => Native::Cancelled,
        Routed::Run(action) => Native::Run(String::from(action.as_str())),
        // Only a name the filter's plugin declared gets here (`declared`).
        Routed::Raise { input, .. } => Native::Raise(String::from(input.as_str())),
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
        match record_of(event, now.millis()) {
            Some(record) => self.native_chain(now, &record, event),
            None => Native::Pass,
        }
    }

    /// The native filters' verdict on a gesture the kernel recognized.
    pub(crate) fn native_gesture(
        &mut self,
        now: paperpaw_platform::Instant,
        gesture: &Gesture,
    ) -> Native {
        let Some(record) = gesture_record(gesture, now.millis()) else {
            return Native::Pass;
        };
        // The finger that made it, for what a replace keeps of it.
        let finger = InputEvent::Pointer(PointerEvent::new(
            0,
            PointerPhase::Up,
            gesture.end,
            gesture.tool,
            1000,
        ));
        self.native_chain(now, &record, &finger)
    }

    fn native_chain(
        &mut self,
        now: paperpaw_platform::Instant,
        record: &Record<'_>,
        event: &InputEvent,
    ) -> Native {
        // The captures are the chain's only state: taken out for the call, so
        // that the rest of the kernel can be read meanwhile.
        let mut chain = core::mem::take(&mut self.shared.native_input);
        let mut cache = self.shared.native_filters.take();
        let (native, refused, failed) =
            self.native_input_run(&mut chain, &mut cache, now, record, event);
        self.shared.native_input = chain;
        // A filter that fails input after input is left out until the
        // declarations change.
        let gave_up = cache.as_mut().is_some_and(|cache| cache.settle(&failed));
        self.shared.native_filters = cache;
        if gave_up {
            self.warn_runtime("a native input filter failed at every input: left out");
        }
        // Told once while filters keep failing, not at every key.
        if refused > 0 && !self.shared.native_input_refused {
            self.warn_runtime("a native input filter failed or answered badly: its input passed");
        }
        self.shared.native_input_refused = refused > 0;
        native
    }

    #[inline(never)] // own frame: the input buffers stay out of the caller's (X4 frame band)
    fn native_input_run(
        &self,
        chain: &mut InputFilters,
        cache: &mut Option<FilterCache>,
        now: paperpaw_platform::Instant,
        record: &Record<'_>,
        event: &InputEvent,
    ) -> (Native, usize, Vec<(u64, u32)>) {
        let none = |native, refused| (native, refused, Vec::new());
        let shared = &self.shared;
        let (Some(directory), Some(depth)) = (
            shared.registry.native_declarations.as_deref(),
            shared.native_depth.as_ref(),
        ) else {
            return none(Native::Pass, 0);
        };
        // The store is released before any filter runs.
        let Ok(()) = directory
            .store
            .try_borrow()
            .map_err(|_| ERR_NO_MEMORY)
            .and_then(|store| match cache {
                Some(kept) if kept.revision == store.revision() => Ok(()),
                _ => {
                    let revision = store.revision();
                    subscriptions(&store)
                        .and_then(|filters| FilterCache::new(revision, filters))
                        .map(|made| *cache = Some(made))
                }
            })
        else {
            return none(Native::Pass, 1);
        };
        let Some(filters) = cache.as_ref().map(|kept| kept.filters.as_slice()) else {
            return none(Native::Pass, 0);
        };
        if filters.is_empty() {
            return none(Native::Pass, 0);
        }
        let env = shared.env(now);
        let stage_schema = schema();
        let mut runner = Watching {
            inner: BrokerRunner {
                schema: &stage_schema,
                depth: depth.as_depth(),
                registry: &shared.native_instances,
                env: &env,
            },
            failed: [(0, 0); super::ui_input::MAX_FILTERS],
            count: 0,
        };
        let stack: &dyn UiStackView = &*shared.stack_rows;
        // The interface-6 screens are not in the native stack rows: until they
        // go, a lock or a PIN screen of theirs is a guard too. Then only the
        // filters of its plugin run (by the id's namespace, as the
        // interface-6 filters do).
        let legacy = self.secret_owner();
        let runs = |plugin: u64| {
            super::ui_guard::filter_runs(stack, plugin)
                && legacy.as_deref().is_none_or(|owner| {
                    shared
                        .native_instances
                        .id(plugin)
                        .is_some_and(|id| id.namespace() == owner)
                })
        };
        let declared = |plugin: u64, name: &str| {
            shared
                .native_instances
                .id(plugin)
                .is_some_and(|id| declared_input(&directory.store, id.as_str(), name))
        };
        let policy = Policy {
            touch: shared.device.input.touch || shared.device.input.pen,
            secret: legacy.is_some() || super::ui_guard::top_is_secret(stack),
            runs: &runs,
            declared: &declared,
        };
        let display = (shared.device.display.width, shared.device.display.height);
        let (native, refused) = run_record(
            chain,
            &mut runner,
            filters,
            &policy,
            record,
            event,
            (now.millis(), display, shared.device.input.keys),
        );
        let failed = runner.failed.get(..runner.count).unwrap_or_default();
        let mut noted = Vec::new();
        // No room to note them: nothing is given up this time.
        if noted.try_reserve_exact(failed.len()).is_ok() {
            noted.extend_from_slice(failed);
        }
        (native, refused, noted)
    }
}

#[cfg(test)]
#[path = "input_chain_tests.rs"]
mod tests;
