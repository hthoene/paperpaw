//! Host side of the service `core.ui` (`docs/abi/ui.md` 6): the hand-over ops
//! 16 `handover` and 17 `params` and the text ops 9 `text_write` and 10
//! `text_read`, over the one [`UiStore`]. The other ops answer `ERR_UNSUPPORTED`
//! until their handlers land.
//!
//! The shell (the screen stack) wires a [`UiHost`] once: the callback epoch and
//! the host-resolved calling instance. A family without it answers
//! `ERR_UNSUPPORTED`; the X4 profile wires nothing yet. Texts and params may be
//! secret: they are never in a `Debug` or a log, and the buffers that held a
//! text are wiped.
use super::{
    host_services::Caller,
    registrations::{self as wire, Record},
    ui_guard::{self, Ask, OpenRequests, UiStackView},
    ui_requests::Question,
    ui_store::{PARAMS_MAX, PART_MAX, Param, Stamp, UiStore},
};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use crate::native_host::codec;
use alloc::{rc::Rc, vec::Vec};
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_STATE, ERR_UNSUPPORTED,
};

/// What the screen stack tells the service about the running callback.
pub(crate) trait UiHost {
    /// The epoch of the callback now running (0: none), a host counter bumped at
    /// every callback entry.
    fn callback_epoch(&self) -> u64;
    /// The instance whose callback runs for `owner` (the host resolves it,
    /// never the plugin), if a screen's callback is running.
    fn calling_instance(&self, owner: u64) -> Option<u32>;
    /// The stack, bottom to top, for the admission rules. `f` runs at once and
    /// the view does not outlive it.
    fn with_stack(&self, f: &mut dyn FnMut(&dyn UiStackView));
    /// Whether this is the X4 (one open request, not four).
    fn x4(&self) -> bool;
    /// Close the dialog instance of a request that was cancelled (queued: no
    /// plugin code runs inside the call).
    fn close_dialog(&self, instance: u32);
}

pub(crate) use crate::abi7::context::ui_views::{Declared, UiDirectory};

/// Op 14's call of the host-defined stage `core.status` to ONE owning plugin
/// (stage-local `status.ask` / `status.value`). Core implements it over its
/// stage broker (`Frame::stage_run`); `core.ui` encodes the ask, groups the
/// items per owner, adds the `own` rows itself and decodes the values. It
/// reads the owner's reply into `out` and returns the bytes used.
#[allow(dead_code, reason = "Core implements it; ops 4, 14 and 15 consume it")]
pub(crate) trait StatusAsk {
    fn ask(
        &self,
        owner: u64,
        token: u32,
        items: &[&str],
        context: &[u8],
        out: &mut [u8],
    ) -> Result<usize, i32>;
}

/// What the shell wires under `core.ui`: the store and the stack's view.
pub(crate) struct UiServices {
    pub(crate) store: Rc<UiStore>,
    pub(crate) host: Rc<dyn UiHost>,
}

/// `text_write` flags: bit 0 `last`.
const LAST: u32 = 1;
/// `ui_ask.flags` bit 2 `secret` (kind 0).
const SECRET: u32 = 1 << 2;

/// The `core.ui` backend.
pub(crate) fn backend(
    _frame: &dyn Frame,
    who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let ui = who.hosts.ui().ok_or(ERR_UNSUPPORTED)?;
    match op.id {
        8 => ask(ui, who, input, out),
        2 => question(ui, who, input, out),
        11 => answer(ui, who, input).map(|()| 0),
        16 => handover(ui, who, input, out),
        17 => params(ui, who, input, out),
        9 => text_write(ui, who, input).map(|()| 0),
        10 => text_read(ui, who, input, out),
        _ => Err(ERR_UNSUPPORTED),
    }
}

fn stamp(ui: &UiServices, who: &Caller<'_>) -> Result<Stamp, i32> {
    Stamp::new(who.owner, ui.host.callback_epoch())
}

/// The form of an `ask` (ui.md 3.1) before any admission: the slot, then
/// the kind, flags and choices (`Question::check`). Gives whether the
/// question goes to the phone keyboard. The one check of the host, which
/// hosts that stand in for it (the scripted host) call instead of copying.
///
/// # Errors
/// `ERR_INVALID` for a slot, kind, flags or choices that do not fit,
/// `ERR_NO_SPACE` for a choice or list over its limit.
pub fn check_ask(ask: &wire::ServiceUiAsk<'_>) -> Result<bool, i32> {
    question_of(ask).map(|question| question.remote)
}

/// The checked question `ask` brings.
fn question_of<'a>(ask: &wire::ServiceUiAsk<'a>) -> Result<Question<'a>, i32> {
    let remote = match ask.slot {
        "" => false,
        ui_guard::REMOTE_KEYBOARD if ask.kind == 0 => true,
        _ => return Err(ERR_INVALID),
    };
    let question = Question {
        kind: ask.kind,
        flags: ask.flags,
        remote,
        title: ask.title,
        text: ask.text,
        choices: ask.choices,
    };
    question.check()?;
    Ok(question)
}

/// Op 8: records a question for the caller and returns its request id; the
/// dialog opens after the caller's callback returns.
fn ask(ui: &UiServices, who: &Caller<'_>, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let ask: wire::ServiceUiAsk<'_> = codec::single(input)?;
    let stamp = stamp(ui, who)?;
    let question = question_of(&ask)?;
    let instance = ui.host.calling_instance(who.owner);
    // A dialog of an open request asks under it (the phone keyboard).
    let parent = instance.and_then(|dialog| ui.store.dialog_of(dialog));
    let (total, has) = ui.store.open_requests(who.owner);
    let secret = ask.kind == 0 && ask.flags & SECRET != 0;
    let mut admitted = Err(ERR_STATE);
    ui.host.with_stack(&mut |view| {
        admitted = ui_guard::ask_admission(
            view,
            &Ask {
                asker: who.owner,
                kind: ask.kind,
                slot: ask.slot,
                secret,
                parent: parent.map(|(_, secret, nested, has_nested)| ui_guard::Parent {
                    secret,
                    nested,
                    has_nested,
                }),
            },
            OpenRequests {
                total,
                asker_has_one: has,
            },
            ui.host.x4(),
        );
    });
    let admission = admitted?;
    let (id, cancelled) = ui.store.ask(
        stamp,
        instance,
        &question,
        admission.secret,
        parent.map(|(id, ..)| id),
        admission.cancels_asker_request,
    )?;
    if let Some(dialog) = cancelled.and_then(|c| c.dialog) {
        ui.host.close_dialog(dialog);
    }
    codec::encode_one(
        &Record::ServiceUiRequest(wire::ServiceUiRequest { request: id }),
        out,
    )
}

/// Op 2: the question, for the request's dialog instance only.
fn question(ui: &UiServices, who: &Caller<'_>, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let ask: wire::ServiceUiRequest = codec::single(input)?;
    let dialog = ui.host.calling_instance(who.owner).ok_or(ERR_STATE)?;
    ui.store.question(dialog, ask.request, |shown| {
        let record = Record::ServiceUiQuestion(wire::ServiceUiQuestion {
            kind: shown.kind,
            flags: shown.flags,
            max_bytes: shown.max_bytes,
            title: shown.title,
            text: shown.text,
            choices: shown.choices,
        });
        codec::encode_one(&record, out)
    })?
}

/// Op 11: the dialog's answer.
fn answer(ui: &UiServices, who: &Caller<'_>, input: &[u8]) -> Result<(), i32> {
    let answer: wire::ServiceUiAnswer = codec::single(input)?;
    let dialog = ui.host.calling_instance(who.owner).ok_or(ERR_STATE)?;
    ui.store
        .answer(dialog, answer.request, answer.outcome, answer.choice)
}

/// Op 16: stages the params, publishes the hand-over only after the whole
/// batch walked `OK`; a refusal leaves nothing.
fn handover(ui: &UiServices, who: &Caller<'_>, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let stamp = stamp(ui, who)?;
    let mut staged: Vec<Param<'_>> = Vec::new();
    staged
        .try_reserve_exact(PARAMS_MAX)
        .map_err(|_| ERR_NO_MEMORY)?;
    wire::host_records_visit(input, PARAMS_MAX, &mut |record| {
        let Record::ServiceUiParam(param) = record else {
            return Err(ERR_INVALID);
        };
        // The walk is capped at the room reserved: no growth here.
        staged.push(Param {
            key: param.key,
            value: param.value,
        });
        Ok(())
    })?;
    let id = ui.store.handover_make(stamp, &staged)?;
    codec::encode_one(
        &Record::ServiceUiHandle(wire::ServiceUiHandle { handle: id }),
        out,
    )
}

/// Op 17: the params of the calling instance's hand-over, then `end`.
fn params(ui: &UiServices, who: &Caller<'_>, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let handle: wire::ServiceUiHandle = codec::single(input)?;
    let instance = ui.host.calling_instance(who.owner).ok_or(ERR_STATE)?;
    let info = ui.store.info(instance).ok_or(ERR_STATE)?;
    if info.handover != handle.handle {
        return Err(ERR_STATE);
    }
    let end = wire::ServiceEnd {
        flags: 0,
        total: info.params,
        generation: 0,
        next: 0,
        cursor: None,
    };
    let reserved = codec::framed_len(&Record::ServiceEnd(end))?;
    let room = out.len().checked_sub(reserved).ok_or(ERR_NO_SPACE)?;
    let used = ui.store.params(instance, handle.handle, &mut out[..room])?;
    let length = codec::encode_one(&Record::ServiceEnd(end), &mut out[used..])?;
    Ok(used + length)
}

/// Op 9: a part of a hand-over's text, by its maker in the callback that made it.
fn text_write(ui: &UiServices, who: &Caller<'_>, input: &[u8]) -> Result<(), i32> {
    let part: wire::ServiceUiText<'_> = codec::single(input)?;
    if part.flags & !LAST != 0 || part.data.len() > PART_MAX {
        return Err(ERR_INVALID);
    }
    let offset = usize::try_from(part.offset).map_err(|_| ERR_INVALID)?;
    let (stamp, last) = (stamp(ui, who)?, part.flags & LAST != 0);
    if ui.store.is_request(part.text) {
        let instance = ui.host.calling_instance(who.owner);
        return ui.store.request_text_write(
            stamp,
            instance,
            part.text,
            offset,
            part.data.as_bytes(),
            last,
        );
    }
    ui.store
        .text_write(stamp, part.text, offset, part.data.as_bytes(), last)
}

/// Op 10: a part of a hand-over's text, for the owning instance only.
fn text_read(
    ui: &UiServices,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let at: wire::ServiceUiTextAt = codec::single(input)?;
    let instance = ui.host.calling_instance(who.owner);
    let offset = usize::try_from(at.offset).map_err(|_| ERR_INVALID)?;
    let mut part = Vec::new();
    part.try_reserve_exact(PART_MAX)
        .map_err(|_| ERR_NO_MEMORY)?;
    part.resize(PART_MAX, 0_u8);
    let read = if ui.store.is_request(at.text) {
        ui.store
            .request_text_read(who.owner, instance, at.text, offset, &mut part)
    } else {
        instance
            .ok_or(ERR_STATE)
            .and_then(|instance| ui.store.text_read(instance, at.text, offset, &mut part))
    };
    let result = read.and_then(|(total, read)| {
        let data = core::str::from_utf8(&part[..read]).map_err(|_| ERR_INVALID)?;
        let last = offset + read >= total;
        let record = Record::ServiceUiText(wire::ServiceUiText {
            text: at.text,
            offset: at.offset,
            total: u32::try_from(total).map_err(|_| ERR_INVALID)?,
            flags: u32::from(last),
            data,
        });
        codec::encode_one(&record, out)
    });
    part.fill(0);
    core::hint::black_box(&mut part);
    result
}

#[cfg(test)]
#[path = "ui_service_tests.rs"]
mod tests;
