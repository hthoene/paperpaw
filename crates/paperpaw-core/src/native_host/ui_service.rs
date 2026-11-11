//! Host side of the service `core.ui` (`docs/abi/ui.md` 6): the questions and
//! answers (ops 8 `ask`, 2 `question`, 11 `answer`), the hand-over ops 16
//! `handover` and 17 `params` and the text ops 9 `text_write` and 10
//! `text_read`, over the one [`UiStore`]; op 3 `hints` (`ui_hints`) over the
//! registry's directory; ops 4, 5, 14 and 15 (widgets and
//! status, `ui_widget_ops`) over the handle table. The other ops answer
//! `ERR_UNSUPPORTED` until their handlers land.
//!
//! The shell wires the [`UiStore`] once; the callback epoch, the host-resolved
//! calling instance, the stack and the X4 flag come from each call's `Frame`.
//! A family without the store answers `ERR_UNSUPPORTED`. Texts and params may
//! be secret: they are never in a `Debug` or a log, and the buffers that held
//! a text are wiped.
use super::{
    host_services::Caller,
    registrations::{self as wire, Record},
    shared::Shared,
    ui_clipboard,
    ui_guard::{self, Ask, OpenRequests},
    ui_hints,
    ui_requests::Question,
    ui_store::{PARAMS_MAX, PART_MAX, Param, Stamp, UiStore},
    ui_widget_layout, ui_widget_ops,
};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use crate::native_host::codec;
use alloc::vec::Vec;
use paperpaw_native::host_glue::{
    ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_STATE, ERR_UNSUPPORTED,
};

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

/// What the shell wires under `core.ui`: the store. The stack's view (the
/// callback epoch, the calling instance, the rows, the X4 flag and the
/// queued dialog close) comes from the `Frame` of each call, which the host
/// builds per callback from host state, never from plugin bytes.
pub(crate) struct UiServices {
    pub(crate) store: Shared<UiStore>,
    /// The widget and status-handle tables and the status cache (ops 4, 5, 14, 15).
    pub(crate) widgets: super::ui_widget_ops::WidgetState,
}

impl UiServices {
    /// The service for a device: `x4` sizes the tables (the X4's small caps).
    pub(crate) fn new(store: Shared<UiStore>, x4: bool) -> Self {
        Self {
            store,
            widgets: super::ui_widget_ops::WidgetState::new(x4),
        }
    }
}

/// `text_write` flags: bit 0 `last`.
const LAST: u32 = 1;
/// `ui_ask.flags` bit 2 `secret` (kind 0).
const SECRET: u32 = 1 << 2;

/// The `core.ui` backend.
pub(crate) fn backend(
    frame: &dyn Frame,
    who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let ui = who.hosts.ui().ok_or(ERR_UNSUPPORTED)?;
    match op.id {
        1 => super::ui_stack_op::stack(frame, who, input, out),
        8 => ask(frame, ui, who, input, out),
        2 => question(frame, ui, input, out),
        18 => ui_clipboard::write(frame, ui, who, input).map(|()| 0),
        19 => ui_clipboard::read(frame, ui, input, out),
        3 => ui_hints::hints(frame, who, op, input, out),
        11 => answer(frame, ui, input).map(|()| 0),
        16 => handover(frame, ui, who, input, out),
        17 => params(frame, ui, input, out),
        4 => ui_widget_ops::widget_make(frame, ui, who, input, out),
        5 => ui_widget_ops::widget_drop(ui, who, input).map(|()| 0),
        6 => ui_widget_layout::widget_input(frame, ui, who, input).map(|()| 0),
        7 => ui_widget_layout::widget_targets(frame, ui, who, input, out),
        12 => ui_widget_layout::widget_measure(frame, ui, who, input, out),
        13 => ui_widget_layout::widget_place(frame, ui, who, input).map(|()| 0),
        14 => ui_widget_ops::status_values(frame, ui, who, input, out),
        15 => ui_widget_ops::status_line(frame, ui, who, input, out),
        9 => text_write(frame, ui, who, input).map(|()| 0),
        10 => text_read(frame, ui, who, input, out),
        _ => Err(ERR_UNSUPPORTED),
    }
}

fn stamp(frame: &dyn Frame, who: &Caller<'_>) -> Result<Stamp, i32> {
    Stamp::new(who.owner, frame.callback_epoch()?)
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
fn ask(
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    // A work run's callback is quiet (`work.md` section 2): no dialog.
    if who
        .hosts
        .work()
        .is_some_and(|work| work.sched.quiet(who.owner))
    {
        return Err(ERR_STATE);
    }
    let ask: wire::ServiceUiAsk<'_> = codec::single(input)?;
    let stamp = stamp(frame, who)?;
    let question = question_of(&ask)?;
    let instance = frame.calling_instance().ok();
    // A dialog of an open request asks under it (the phone keyboard).
    let parent = instance.and_then(|dialog| ui.store.dialog_of(dialog));
    let (total, has) = ui.store.open_requests(who.owner);
    let secret = ask.kind == 0 && ask.flags & SECRET != 0;
    let x4 = frame.ui_x4()?;
    let mut admitted = Err(ERR_STATE);
    frame.with_stack(&mut |view| {
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
            x4,
        );
    })?;
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
        // Queued, no plugin code runs. The request is cancelled already and
        // stays so whatever this answers: `ERR_STATE` is a stack that moved on
        // (the dialog is gone), and the queue (16 closes) cannot fill with at
        // most 4 requests open. A dialog left over is closed by the cleanup
        // sweep when its request is delivered.
        let _ = frame.close_dialog(dialog);
    }
    codec::encode_one(
        &Record::ServiceUiRequest(wire::ServiceUiRequest { request: id }),
        out,
    )
}

/// Op 2: the question, for the request's dialog instance only.
fn question(
    frame: &dyn Frame,
    ui: &UiServices,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let ask: wire::ServiceUiRequest = codec::single(input)?;
    let dialog = frame.calling_instance()?;
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
fn answer(frame: &dyn Frame, ui: &UiServices, input: &[u8]) -> Result<(), i32> {
    let answer: wire::ServiceUiAnswer = codec::single(input)?;
    let dialog = frame.calling_instance()?;
    ui.store
        .answer(dialog, answer.request, answer.outcome, answer.choice)
}

/// Op 16: stages the params, publishes the hand-over only after the whole
/// batch walked `OK`; a refusal leaves nothing.
fn handover(
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let stamp = stamp(frame, who)?;
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
    let id = match handed_on(frame, ui, &mut staged)? {
        Some(maker) => ui.store.handover_passing_on(stamp, &staged, maker),
        None => ui.store.handover_make(stamp, &staged),
    }?;
    codec::encode_one(
        &Record::ServiceUiHandle(wire::ServiceUiHandle { handle: id }),
        out,
    )
}

/// A hand-over whose `root` is `5` passes on the handed file of the calling
/// instance (a reader opening its table-of-contents screen of a book a
/// plugin handed over, `document.md` 1 root 5): the instance must hold one
/// and `path` must be its path (`ERR_DENIED` otherwise, as for a root of 5
/// before). Gives the plugin that handed the file first, and puts the file's
/// own root (`0`, `2`) in the params, so the screen reads the same hand-over
/// as the one the maker made.
fn handed_on(
    frame: &dyn Frame,
    ui: &UiServices,
    staged: &mut [Param<'_>],
) -> Result<Option<u64>, i32> {
    let Some(at) = staged
        .iter()
        .position(|param| param.key == "root" && param.value == "5")
    else {
        return Ok(None);
    };
    let instance = frame.calling_instance().map_err(|_| ERR_DENIED)?;
    let path = staged
        .iter()
        .find(|param| param.key == "path")
        .map(|param| param.value);
    let mut found = Err(ERR_DENIED);
    ui.store.handed_file(instance, &mut |file| {
        if Some(file.path) == path {
            found = Ok((file.maker, if file.root == 2 { "2" } else { "0" }));
        }
    })?;
    let (maker, root) = found?;
    staged[at].value = root;
    Ok(Some(maker))
}

/// Op 17: the params of the calling instance's hand-over, then `end`.
fn params(frame: &dyn Frame, ui: &UiServices, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let handle: wire::ServiceUiHandle = codec::single(input)?;
    let instance = frame.calling_instance()?;
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
fn text_write(
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    input: &[u8],
) -> Result<(), i32> {
    let part: wire::ServiceUiText<'_> = codec::single(input)?;
    if part.flags & !LAST != 0 || part.data.len() > PART_MAX {
        return Err(ERR_INVALID);
    }
    let offset = usize::try_from(part.offset).map_err(|_| ERR_INVALID)?;
    let (stamp, last) = (stamp(frame, who)?, part.flags & LAST != 0);
    if ui.store.is_request(part.text) {
        let instance = frame.calling_instance().ok();
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
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let at: wire::ServiceUiTextAt = codec::single(input)?;
    let instance = frame.calling_instance().ok();
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
