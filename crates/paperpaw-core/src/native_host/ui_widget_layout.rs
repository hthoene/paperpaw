//! `core.ui` ops 6 `widget_input`, 7 `widget_targets`, 12 `widget_measure` and
//! 13 `widget_place` (`docs/abi/ui.md` 5.2 and 6): the showing screen lays out
//! a widget instance it made, forwards input to it and reads its targets.
//!
//! - **Who.** The instance's maker only (another plugin's instance is
//!   `ERR_DENIED`, an unknown or dropped one `ERR_NOT_FOUND`); instance 0, an
//!   own widget (op 4 `flags` bit 0), is `ERR_BUSY` without a call: the plugin
//!   measures, places and draws its own.
//! - **Guard.** While another plugin guards, an instance that is private or
//!   was made by a screen at or below the guard is `ERR_DENIED` for all four
//!   (ui.md 4.3), as slot 35 is. A stack the frame cannot read hides it.
//! - **Nested.** Ops 7, 12 and 13 run the owner's callback inside the caller's
//!   through [`Frame::widget_call`] (one `core.widget` stage call each, depth 1
//!   only, `ERR_UNSUPPORTED` on the X4 until its stack gate passes). A call the
//!   owner fails, a reply its codec refuses or one without the expected row
//!   comes back as nothing: op 12 height 0 ("nothing to show"), op 7 no
//!   targets, op 13 `OK`; the owner is told it was dropped and the caller goes
//!   on (as op 14 does).
//! - **Op 6** is `ERR_UNSUPPORTED` until Core's dispatcher drains the queue
//!   ([`UiServices::widget_input_drained`]). It runs nothing inside the
//!   caller: the tap is queued for the owner's
//!   `input` callback ([`WidgetInputs`], at most 8 entries, 2 on the X4, one
//!   fallible allocation made at the first tap; `ERR_FULL` when it is full).
//!   Core's dispatcher drains it like the widget drops
//!   ([`UiServices::next_widget_input`]); an entry whose instance was dropped
//!   meanwhile, or whose owner stopped, is never delivered.
//!
//! An instance of the caller's own widget is `ERR_BUSY` for ops 7, 12 and 13
//! too, before any call.
//!
//! Memory: no op allocates beyond the one reply buffer of the call (`native_widget`);
//! op 7's rows go straight to the reply (32 x 25 bytes and the `end`).
use super::{
    registrations::{self as wire, Record},
    stages_wire::core_widget_v1 as widget,
    ui_compose_host::StackGuard,
    ui_handles::{GuardState, WidgetRef},
    ui_service::UiServices,
};
use crate::abi7::context::Frame;
use crate::native_host::codec;
use alloc::vec::Vec;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED,
};

use super::host_services::Caller;

/// `ui_widget_input.kind`: 0 tap, 1 long press.
const KINDS: u32 = 2;
/// Most targets one owner returns (`widget.target` rows).
const TARGETS_MAX: usize = 32;
/// Queued taps, and on the X4.
pub(crate) const INPUTS_MAX: usize = 8;
const INPUTS_MAX_X4: usize = 2;

/// One tap or long press waiting for its owner's `input` callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PendingInput {
    /// The plugin whose screen forwarded it (op 6's caller).
    pub(crate) who: u64,
    pub(crate) owner: u64,
    /// The owner's widget token (the `input` callback's).
    pub(crate) token: u32,
    pub(crate) instance: u32,
    pub(crate) target: u32,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) kind: u32,
}

/// The queue of op 6.
#[derive(Debug)]
pub(crate) struct WidgetInputs {
    queue: Vec<PendingInput>,
    cap: usize,
    /// Core's dispatcher drains the queue. Until it says so (`false`) op 6
    /// is `ERR_UNSUPPORTED`: a tap nothing delivers must not be accepted.
    drained: bool,
}

impl WidgetInputs {
    pub(crate) const fn new(x4: bool) -> Self {
        Self {
            queue: Vec::new(),
            cap: if x4 { INPUTS_MAX_X4 } else { INPUTS_MAX },
            drained: false,
        }
    }

    /// Room for `input`, in order: `ERR_FULL` at the cap, `ERR_NO_MEMORY` when
    /// the heap refuses the (one) allocation; nothing is queued then.
    pub(crate) fn push(&mut self, input: PendingInput) -> Result<(), i32> {
        if self.queue.len() >= self.cap {
            return Err(ERR_FULL);
        }
        if self.queue.capacity() == 0 {
            self.queue
                .try_reserve_exact(self.cap)
                .map_err(|_| ERR_NO_MEMORY)?;
        }
        self.queue.push(input);
        Ok(())
    }

    /// The owner stopped: what waits for it is gone.
    pub(crate) fn drop_owner(&mut self, owner: u64) {
        self.queue.retain(|input| input.owner != owner);
    }
}

impl UiServices {
    /// Core's dispatcher drains the taps of op 6 ([`Self::next_widget_input`]
    /// and [`Self::widget_input_delivered`]): op 6 is served from now on.
    /// Its delivery must apply the guard rule of ui.md 4.3 again at the time
    /// of delivery (a guard may have come up since the tap was queued).
    #[allow(dead_code, reason = "Core's dispatcher drains the taps")]
    pub(crate) fn widget_input_drained(&self) -> Result<(), i32> {
        self.widgets
            .inputs
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .drained = true;
        Ok(())
    }

    /// The owner of the oldest tap whose instance is still made, if any
    /// (entries of dropped instances are discarded here).
    pub(crate) fn next_widget_input_owner(&self) -> Result<Option<u64>, i32> {
        let handles = self.widgets.handles.try_borrow().map_err(|_| ERR_BUSY)?;
        let mut inputs = self.widgets.inputs.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        inputs
            .queue
            .retain(|input| handles.has_widget(input.instance));
        Ok(inputs.queue.first().map(|input| input.owner))
    }

    /// Whether `input` may still be delivered: the guard rule of ui.md 4.3
    /// as `locate` applies it to the forwarding screen's plugin, read again
    /// at the time of delivery. A tap whose instance is gone or is no longer
    /// the forwarder's to reach is not.
    pub(crate) fn widget_input_allowed(
        &self,
        input: &PendingInput,
        guard: &dyn GuardState,
    ) -> bool {
        let Ok(handles) = self.widgets.handles.try_borrow() else {
            return false;
        };
        let Ok(found) = handles.widget_of(input.who, input.instance) else {
            return false;
        };
        // The forwarding screen must still be on the stack, guard or none.
        guard.on_stack(found.screen)
            && !(guard.guard_plugin().is_some_and(|g| g != input.who)
                && (found.private || guard.below_guard(found.screen)))
    }

    /// The next tap for `owner` whose instance is still made, as the
    /// `input.widget` record for its token, in `out`: the length, with the
    /// entry to give back to [`Self::widget_input_delivered`]. `None` when
    /// there is none. Entries of dropped instances are discarded here.
    #[allow(dead_code, reason = "Core's dispatcher drains the taps")]
    pub(crate) fn next_widget_input(
        &self,
        owner: u64,
        now_ms: u64,
        out: &mut [u8],
    ) -> Result<Option<(PendingInput, usize)>, i32> {
        let handles = self.widgets.handles.try_borrow().map_err(|_| ERR_BUSY)?;
        let mut inputs = self.widgets.inputs.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        inputs
            .queue
            .retain(|input| handles.has_widget(input.instance));
        let Some(input) = inputs.queue.iter().find(|input| input.owner == owner) else {
            return Ok(None);
        };
        let record = Record::InputWidget(wire::InputWidget {
            now_ms,
            instance: input.instance,
            target: input.target,
            x: input.x,
            y: input.y,
            kind: input.kind,
        });
        let length = codec::encode_batch(&[record], out)?;
        Ok(Some((*input, length)))
    }

    /// `input` leaves the queue: it is being delivered (once), or discarded.
    #[allow(dead_code, reason = "Core's dispatcher drains the taps")]
    pub(crate) fn widget_input_delivered(&self, input: &PendingInput) -> Result<(), i32> {
        let mut inputs = self.widgets.inputs.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if let Some(at) = inputs.queue.iter().position(|queued| queued == input) {
            inputs.queue.remove(at);
        }
        Ok(())
    }
}

/// The nested-callback gate of ops 7, 12 and 13.
fn nested_gate(frame: &dyn Frame) -> Result<(), i32> {
    frame.callback_epoch()?;
    if frame.callback_depth()? != 1 {
        return Err(ERR_BUSY);
    }
    if frame.nested_closed()? {
        return Err(ERR_UNSUPPORTED);
    }
    Ok(())
}

/// The instance `id` as `who` may lay it out under a nested call (ops 7, 12,
/// 13): one of the caller's own widgets is `ERR_BUSY` up front, as instance 0
/// is (the owner cannot be called from inside itself).
fn locate_nested(
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    id: u32,
) -> Result<WidgetRef, i32> {
    let found = locate(frame, ui, who, id)?;
    if found.owner == who.owner {
        return Err(ERR_BUSY);
    }
    Ok(found)
}

/// The instance `id` as `who` may lay it out.
fn locate(frame: &dyn Frame, ui: &UiServices, who: &Caller<'_>, id: u32) -> Result<WidgetRef, i32> {
    let found = ui
        .widgets
        .handles
        .try_borrow()
        .map_err(|_| ERR_BUSY)?
        .widget_of(who.owner, id)?;
    let guard = StackGuard(frame);
    if guard.guard_plugin().is_some_and(|g| g != who.owner)
        && (found.private || guard.below_guard(found.screen))
    {
        return Err(ERR_DENIED);
    }
    Ok(found)
}

fn area(input: &[u8]) -> Result<wire::ServiceUiWidgetArea, i32> {
    codec::single(input)
}

/// Op 6: a tap or long press for the owner's `input` callback, queued.
pub(super) fn widget_input(
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    input: &[u8],
) -> Result<(), i32> {
    if !ui
        .widgets
        .inputs
        .try_borrow()
        .map_err(|_| ERR_BUSY)?
        .drained
    {
        return Err(ERR_UNSUPPORTED);
    }
    let tap: wire::ServiceUiWidgetInput = codec::single(input)?;
    if tap.kind >= KINDS {
        return Err(ERR_INVALID);
    }
    frame.callback_epoch()?;
    let found = locate(frame, ui, who, tap.instance)?;
    ui.widgets
        .inputs
        .try_borrow_mut()
        .map_err(|_| ERR_BUSY)?
        .push(PendingInput {
            who: who.owner,
            owner: found.owner,
            token: found.token,
            instance: tap.instance,
            target: tap.target,
            x: tap.x,
            y: tap.y,
            kind: tap.kind,
        })
}

/// Op 12: the owner's height for `width` and its least height.
pub(super) fn widget_measure(
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let area = area(input)?;
    nested_gate(frame)?;
    let found = locate_nested(frame, ui, who, area.instance)?;
    let call = widget::Record::WidgetMeasure(widget::WidgetMeasure::new(area.instance, area.width));
    let (mut height, mut least) = (0, 0);
    if let Some(reply) = ask(frame, &found, &call)? {
        for record in reply.records() {
            if let Ok(widget::Record::WidgetSize(size)) = record {
                (height, least) = (size.height, size.min_height);
                break;
            }
        }
    }
    codec::encode_one(
        &Record::ServiceUiSize(wire::ServiceUiSize {
            height,
            min_height: least,
        }),
        out,
    )
}

/// Op 13: tells the owner where its instance sits.
pub(super) fn widget_place(
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    input: &[u8],
) -> Result<(), i32> {
    let area = area(input)?;
    nested_gate(frame)?;
    let found = locate_nested(frame, ui, who, area.instance)?;
    let call = widget::Record::WidgetPlace(widget::WidgetPlace::new(
        area.instance,
        area.x,
        area.y,
        area.width,
        area.height,
    ));
    ask(frame, &found, &call)?;
    Ok(())
}

/// Op 7: the owner's targets, reading order, at most 32, then `end`.
pub(super) fn widget_targets(
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let area = area(input)?;
    nested_gate(frame)?;
    let found = locate_nested(frame, ui, who, area.instance)?;
    let call = widget::Record::WidgetTargets(widget::WidgetTargets::new(
        area.instance,
        area.x,
        area.y,
        area.width,
        area.height,
    ));
    let mut used = 0;
    let mut count = 0_u32;
    if let Some(reply) = ask(frame, &found, &call)? {
        for record in reply.records() {
            let Ok(widget::Record::WidgetTarget(target)) = record else {
                continue;
            };
            if count as usize == TARGETS_MAX {
                break;
            }
            used += codec::encode_one(
                &Record::ServiceUiTarget(wire::ServiceUiTarget {
                    target: target.target,
                    x: target.x,
                    y: target.y,
                    width: target.width,
                    height: target.height,
                }),
                out.get_mut(used..).ok_or(ERR_INVALID)?,
            )?;
            count += 1;
        }
    }
    let end = wire::ServiceEnd {
        flags: 0,
        total: count,
        generation: 0,
        next: 0,
        cursor: None,
    };
    Ok(used
        + codec::encode_one(
            &Record::ServiceEnd(end),
            out.get_mut(used..).ok_or(ERR_INVALID)?,
        )?)
}

/// What the owner answered, owned: the reply bytes of one stage call.
struct Reply {
    bytes: Vec<u8>,
}

impl Reply {
    fn records(&self) -> impl Iterator<Item = Result<widget::Record, i32>> + '_ {
        widget::reply_decode(&self.bytes).into_iter().flatten()
    }
}

/// One `core.widget` call to the owner of `found`. `None` is no answer: the
/// owner's call failed (it was told), or the call could not be made after the
/// gates (owner gone, busy); the gates themselves (`ERR_STATE`, `ERR_BUSY` at
/// depth 2, `ERR_UNSUPPORTED` on the X4) were checked before, and a refused
/// allocation is the caller's `ERR_NO_MEMORY`.
fn ask(frame: &dyn Frame, found: &WidgetRef, call: &widget::Record) -> Result<Option<Reply>, i32> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(widget::MAX_REPLY)
        .map_err(|_| ERR_NO_MEMORY)?;
    bytes.resize(widget::MAX_REPLY, 0);
    let mut payload = [0; 64];
    let length = widget::input_encode(core::slice::from_ref(call), &mut payload)?;
    match frame.widget_call(found.owner, found.token, &payload[..length], &mut bytes) {
        Ok(length) => {
            bytes.truncate(length);
            Ok(Some(Reply { bytes }))
        }
        // The host's own memory refusal is the caller's; any other failure
        // (the owner's, or it went away) is no answer.
        Err(ERR_NO_MEMORY) => Err(ERR_NO_MEMORY),
        Err(_) => Ok(None),
    }
}
