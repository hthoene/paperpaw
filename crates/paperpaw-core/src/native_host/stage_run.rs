//! Slot 32, `stage.run`, from a native plugin: the owner of a stage asks one
//! of its subscribers, by cursor, and gets one `stage.reply` back
//! (`docs/abi/stages-services.md` section 1.2).
//!
//! The subscribers are the live `registration.stage` rows of the declaration
//! store, ordered by `(order, plugin id, registration index)`; the one at the
//! cursor is called through its retained lease at depth 2, with the store's
//! borrow released first. Everything that can refuse does so before any
//! subscriber runs, a subscriber that fails or answers with something that is
//! no record batch only costs its own reply a status, and the caller's `out`
//! is written once, whole, at the end.
use super::{
    codec,
    declaration_store::DeclarationStore,
    native_declarations::NativeDeclarations,
    native_screen::Dispatch,
    registrations as wire,
    registrations::Record,
    stage_members::{self, Member},
    stage_wire,
};
use crate::abi7::{context::Frame, core_frame::CoreFrame};
use alloc::vec::Vec;
use core::mem::MaybeUninit;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_CANCELLED, ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND,
    ERR_STATE, ERR_UNSUPPORTED,
};
use paperpaw_sdk::SettingValue;

/// The most subscribers a stage has active (the first 32 in its order).
const SUBSCRIBERS: usize = 32;
/// The longest subscriber id a `stage.reply` names.
const ID_BYTES: usize = 64;
/// The most payload bytes of a `stage.call` or a `stage.reply`.
const PAYLOAD_MAX: usize = 3840;
/// A whole `stage.call` batch fits this.
const CALL_MAX: usize = 4096;
/// Bit 0 of `stage.reply.flags`: the user granted the subscriber the right to
/// override the reading style (`core.grant.style.<subscriber>`, decision 131).
const STYLE_GRANTED: u32 = 1;

/// A subscriber as chosen, copied out of the store so that no borrow of it
/// crosses the call.
struct Chosen {
    owner: u64,
    token: u32,
    signature: u64,
    id: [u8; ID_BYTES],
    id_len: usize,
}

impl Chosen {
    fn id(&self) -> &str {
        // The bytes are a copy of a `&str` made whole.
        core::str::from_utf8(&self.id[..self.id_len]).unwrap_or("")
    }
}

fn scratch(capacity: usize) -> Result<Vec<u8>, i32> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| ERR_NO_MEMORY)?;
    bytes.resize(capacity, 0);
    Ok(bytes)
}

/// The largest `stage.reply` a stage with `max_reply` payload bytes makes: a
/// 64-byte subscriber id and the whole payload.
fn largest_reply(max_reply: usize) -> Result<usize, i32> {
    const ZEROS: [u8; PAYLOAD_MAX] = [0; PAYLOAD_MAX];
    const ID: &str = "................................................................";
    codec::framed_len(&Record::StageReply(wire::StageReply {
        subscriber: ID,
        index: 0,
        status: 0,
        flags: 0,
        signature: 0,
        payload: ZEROS.get(..max_reply).ok_or(ERR_INVALID)?,
    }))
}

/// Runs the subscriber at `cursor` of `stage` for its owner `caller`.
///
/// # Errors
/// `ERR_STATE` outside a callback or from a nested one (a subscriber cannot
/// run a stage), `ERR_UNSUPPORTED` on a small-memory device (the X4, until its
/// stack gate passes) or without a declaration directory,
/// `ERR_NOT_FOUND` for a stage nobody declared or a cursor past the last
/// subscriber, `ERR_DENIED` for a stage that is not the caller's,
/// `ERR_NO_SPACE` for an input over the stage's bound or an `out` too small
/// for its largest reply, `ERR_NO_MEMORY`. `out` is untouched on every error.
pub(crate) fn run(
    frame: &CoreFrame<'_>,
    stage: &str,
    cursor: u32,
    input: &[u8],
    out: &mut [MaybeUninit<u8>],
) -> Result<usize, i32> {
    let epoch = frame.callback_epoch()?;
    let caller = frame.caller()?;
    // A subscriber is itself in a callback, at depth 2: it cannot run a stage.
    if frame.callback_depth()? != 1 {
        return Err(ERR_STATE);
    }
    // The subscriber is a nested callback, one level deeper than any frame the
    // X4 has measured: closed until its stack gate passes (as `ui.compose`).
    if super::ui_store::is_x4(frame.env.device().memory) {
        return Err(ERR_UNSUPPORTED);
    }
    let directory = frame.directory.ok_or(ERR_UNSUPPORTED)?;
    let (limits, chosen) = choose(
        directory,
        (stage, epoch),
        caller,
        cursor,
        (input.len(), out.len()),
    )?;
    ask(
        frame,
        directory,
        stage,
        &limits,
        &chosen,
        (cursor, input),
        out,
    )
}

/// The checks before any subscriber runs, and the subscriber at `cursor` of
/// the list this callback froze.
fn choose(
    directory: &NativeDeclarations,
    (stage, epoch): (&str, u64),
    caller: u64,
    cursor: u32,
    (input, out): (usize, usize),
) -> Result<(super::declaration_store::StageLimits, Chosen), i32> {
    let store = directory.store.try_borrow().map_err(|_| ERR_BUSY)?;
    let limits = store.stage(stage).ok_or(ERR_NOT_FOUND)?;
    if limits.owner != caller {
        return Err(ERR_DENIED);
    }
    if input > usize::try_from(limits.max_input).map_err(|_| ERR_INVALID)? {
        return Err(ERR_NO_SPACE);
    }
    let max_reply = usize::try_from(limits.max_reply).map_err(|_| ERR_INVALID)?;
    if out < largest_reply(max_reply)? {
        return Err(ERR_NO_SPACE);
    }
    // Only the first 32 are active; the rest stay inert.
    let cursor = usize::try_from(cursor).map_err(|_| ERR_NOT_FOUND)?;
    if cursor >= SUBSCRIBERS {
        return Err(ERR_NOT_FOUND);
    }
    let mut members = directory.members.try_borrow_mut().map_err(|_| ERR_BUSY)?;
    let picked = members
        .member(epoch, stage, cursor, || freeze(&store, stage))?
        .ok_or(ERR_NOT_FOUND)?;
    if picked.id.len() > ID_BYTES {
        return Err(ERR_INVALID);
    }
    let mut id = [0; ID_BYTES];
    id[..picked.id.len()].copy_from_slice(picked.id.as_bytes());
    let chosen = Chosen {
        owner: picked.owner,
        token: picked.token,
        signature: picked.signature,
        id,
        id_len: picked.id.len(),
    };
    Ok((limits, chosen))
}

/// The live subscribers of `stage` in stage order, at most 32, as one list.
fn freeze(store: &DeclarationStore, stage: &str) -> Result<Vec<Member>, i32> {
    let mut subscribers = store.stage_subscribers(stage)?;
    subscribers
        .sort_unstable_by(|a, b| (a.order, a.plugin, a.index).cmp(&(b.order, b.plugin, b.index)));
    subscribers.truncate(SUBSCRIBERS);
    let mut members = Vec::new();
    members
        .try_reserve_exact(subscribers.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    for subscriber in &subscribers {
        if subscriber.plugin.len() > ID_BYTES {
            return Err(ERR_INVALID);
        }
        members.push(Member {
            owner: subscriber.owner,
            token: subscriber.token,
            signature: subscriber.signature,
            id: stage_members::boxed(subscriber.plugin)?,
        });
    }
    Ok(members)
}

/// Calls the chosen subscriber and frames its reply as one `stage.reply`
/// straight into `out`, once nothing can refuse any more (the reply buffer
/// lent to the subscriber is the only other one).
fn ask(
    frame: &CoreFrame<'_>,
    directory: &NativeDeclarations,
    stage: &str,
    limits: &super::declaration_store::StageLimits,
    subscriber: &Chosen,
    (cursor, input): (u32, &[u8]),
    out: &mut [MaybeUninit<u8>],
) -> Result<usize, i32> {
    let max_reply = usize::try_from(limits.max_reply).map_err(|_| ERR_INVALID)?;
    let max_records = limits.max_records;
    let (status, flags, payload) = call(
        frame,
        directory,
        stage,
        limits.schema,
        subscriber,
        input,
        (max_reply, max_records),
    )?;
    let granted = style_granted(frame, subscriber.id());
    let record = Record::StageReply(wire::StageReply {
        subscriber: subscriber.id(),
        index: cursor,
        status,
        flags: flags | if granted { STYLE_GRANTED } else { 0 },
        signature: subscriber.signature,
        payload: &payload,
    });
    let length = codec::framed_len(&record)?;
    let space = out.get_mut(..length).ok_or(ERR_NO_SPACE)?;
    for slot in space.iter_mut() {
        slot.write(0);
    }
    // SAFETY: every byte of `space` was written just above, and `u8` and
    // `MaybeUninit<u8>` have the same layout.
    let bytes = unsafe { &mut *(core::ptr::from_mut(space) as *mut [u8]) };
    if codec::encode_one(&record, bytes)? != length {
        return Err(ERR_INVALID);
    }
    Ok(length)
}

/// The subscriber's call: its status and payload, a refusal or a reply that
/// is no record batch costing only that status.
fn call(
    frame: &CoreFrame<'_>,
    directory: &NativeDeclarations,
    stage: &str,
    schema: u32,
    subscriber: &Chosen,
    input: &[u8],
    (max_reply, max_records): (usize, u32),
) -> Result<(i32, u32, Vec<u8>), i32> {
    let record = Record::StageCall(wire::StageCall {
        stage,
        schema,
        payload: input,
    });
    let capacity = codec::framed_len(&record)?
        .checked_add(1)
        .filter(|length| *length <= CALL_MAX)
        .ok_or(ERR_NO_SPACE)?;
    let mut call = scratch(capacity)?;
    if codec::encode_batch(&[record], &mut call)? != capacity {
        return Err(ERR_INVALID);
    }
    let mut reply = scratch(max_reply)?;
    // A subscriber that is not running (or no longer known) answers
    // `ERR_CANCELLED`: it is never started inside a stage run. The store's
    // borrow ends before foreign code runs.
    let lease = match directory
        .store
        .try_borrow()
        .map_err(|_| ERR_BUSY)?
        .native_lease(subscriber.owner)
    {
        Ok(lease) => lease,
        Err(_) => return Ok((ERR_CANCELLED, 0, Vec::new())),
    };
    let result = lease
        .stage(subscriber.token, &call, &mut reply, frame.env)
        .and_then(|length| {
            let length = usize::try_from(length).map_err(|_| length)?;
            let bytes = reply.get(..length).ok_or(ERR_INVALID)?;
            stage_wire::validate_reply(bytes, max_reply, max_records)?;
            Ok(length)
        });
    match result {
        Ok(length) => {
            reply.truncate(length);
            Ok((0, 0, reply))
        }
        // The subscriber's own refusal passes through; what the host cannot
        // take it is told with `dropped`.
        Err(status) => {
            if status == ERR_INVALID {
                lease.dropped(subscriber.token, status);
            }
            Ok((status, 0, Vec::new()))
        }
    }
}

/// Whether the user granted `subscriber` the right to override the reading
/// style: `core.grant.style.<subscriber>` is yes (decision 131).
fn style_granted(frame: &CoreFrame<'_>, subscriber: &str) -> bool {
    const KEY: &str = "core.grant.style.";
    let mut full = [0u8; KEY.len() + ID_BYTES];
    full[..KEY.len()].copy_from_slice(KEY.as_bytes());
    let end = KEY.len() + subscriber.len();
    let Some(tail) = full.get_mut(KEY.len()..end) else {
        return false;
    };
    tail.copy_from_slice(subscriber.as_bytes());
    let Ok(key) = core::str::from_utf8(&full[..end]) else {
        return false;
    };
    matches!(frame.settings().get(key), Some(SettingValue::Bool(true)))
}
