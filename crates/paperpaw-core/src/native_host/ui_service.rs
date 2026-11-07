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
    ui_store::{PARAMS_MAX, PART_MAX, Param, Stamp, UiStore},
};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
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
}

/// What the shell wires under `core.ui`: the store and the stack's view.
pub(crate) struct UiServices {
    pub(crate) store: Rc<UiStore>,
    pub(crate) host: Rc<dyn UiHost>,
}

/// `text_write` flags: bit 0 `last`.
const LAST: u32 = 1;

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
    let id = ui.store.make(stamp, &staged)?;
    wire::record_encode_one(
        &Record::ServiceUiHandle(wire::ServiceUiHandle { handle: id }),
        out,
    )
}

/// Op 17: the params of the calling instance's hand-over, then `end`.
fn params(ui: &UiServices, who: &Caller<'_>, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let Record::ServiceUiHandle(handle) = wire::records_decode_single(input)? else {
        return Err(ERR_INVALID);
    };
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
    };
    let reserved = wire::record_framed_len(&Record::ServiceEnd(end))?;
    let room = out.len().checked_sub(reserved).ok_or(ERR_NO_SPACE)?;
    let used = ui.store.params(instance, handle.handle, &mut out[..room])?;
    let length = wire::record_encode_one(&Record::ServiceEnd(end), &mut out[used..])?;
    Ok(used + length)
}

/// Op 9: a part of a hand-over's text, by its maker in the callback that made it.
fn text_write(ui: &UiServices, who: &Caller<'_>, input: &[u8]) -> Result<(), i32> {
    let Record::ServiceUiText(part) = wire::records_decode_single(input)? else {
        return Err(ERR_INVALID);
    };
    if part.flags & !LAST != 0 || part.data.len() > PART_MAX {
        return Err(ERR_INVALID);
    }
    let offset = usize::try_from(part.offset).map_err(|_| ERR_INVALID)?;
    ui.store.text_write(
        stamp(ui, who)?,
        part.text,
        offset,
        part.data.as_bytes(),
        part.flags & LAST != 0,
    )
}

/// Op 10: a part of a hand-over's text, for the owning instance only.
fn text_read(
    ui: &UiServices,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let Record::ServiceUiTextAt(at) = wire::records_decode_single(input)? else {
        return Err(ERR_INVALID);
    };
    let instance = ui.host.calling_instance(who.owner).ok_or(ERR_STATE)?;
    let offset = usize::try_from(at.offset).map_err(|_| ERR_INVALID)?;
    let mut part = Vec::new();
    part.try_reserve_exact(PART_MAX)
        .map_err(|_| ERR_NO_MEMORY)?;
    part.resize(PART_MAX, 0_u8);
    let result = ui
        .store
        .text_read(instance, at.text, offset, &mut part)
        .and_then(|(total, read)| {
            let data = core::str::from_utf8(&part[..read]).map_err(|_| ERR_INVALID)?;
            let last = offset + read >= total;
            let record = Record::ServiceUiText(wire::ServiceUiText {
                text: at.text,
                offset: at.offset,
                total: u32::try_from(total).map_err(|_| ERR_INVALID)?,
                flags: u32::from(last),
                data,
            });
            wire::record_encode_one(&record, out)
        });
    part.fill(0);
    core::hint::black_box(&mut part);
    result
}

#[cfg(test)]
#[path = "ui_service_tests.rs"]
mod tests;
