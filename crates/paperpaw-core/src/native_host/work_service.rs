//! Host side of the service `core.work` (`docs/abi/work.md` section 1).
//!
//! Three ops over the scheduler ([`WorkSched`]): `declare` (add or replace by
//! key), `cancel` and `state`. The backend is stateless; the scheduler and the
//! device view it asks live in [`WorkServices`], wired once by the shell. Until
//! then the family answers `ERR_UNSUPPORTED` (the X4 profile wires nothing).
//!
//! A network work needs the instance's admitted `uses.network`
//! (`ERR_UNSUPPORTED` when the host has not wired it, `ERR_DENIED` when the
//! instance did not ask) and the user's network setting for the plugin
//! (`ERR_DENIED`). Memory: the call itself keeps nothing; `state` is one
//! reply of at most 4,096 bytes written into the dispatcher's scratch.
use super::shared::Shared;
use super::{
    host_services::Caller,
    registrations::{self as wire, Record},
    work_sched::{Spec, WorkEnv, WorkSched},
};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use crate::native_host::codec;
use alloc::boxed::Box;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_STATE, ERR_UNSUPPORTED};

/// `uses.network`: the feature a network work needs.
const USES_NETWORK: &str = "uses.network";

/// What the shell wires under `core.work`: the table and the device's view.
/// Held in the host's fallible shared pointer: made at the first call
/// (`HostServices::work_for_call`, #248), where no room is `ERR_NO_MEMORY`.
pub(crate) struct WorkServices {
    pub(crate) sched: Shared<WorkSched>,
    pub(crate) env: Shared<Box<dyn WorkEnv>>,
}

/// The `core.work` backend.
pub(crate) fn backend(
    _frame: &dyn Frame,
    who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let work = who.hosts.work_for_call()?;
    match op.id {
        1 => {
            let spec: wire::ServiceWorkSpec<'_> = codec::single(input)?;
            let spec = Spec {
                token: spec.token,
                needs: spec.needs,
                flags: spec.flags,
                moments: spec.moments,
                after_ms: spec.after_ms,
                within_ms: spec.within_ms,
                every_ms: spec.every_ms,
                at_ms: spec.at_ms,
                key: spec.key,
            };
            WorkSched::check_form(&spec)?;
            // Work whose `event.work_due` the plugin could not take is never
            // declared (`work.md` section 3, as `core.net` asks for `http_done`).
            if !work.env.subscribed(who.owner, wire::EVENT_WORK_DUE_TAG) {
                return Err(ERR_STATE);
            }
            if spec.needs & 1 != 0 {
                who.features.feature(USES_NETWORK)?;
                if !work.env.network_allowed(who.owner) {
                    return Err(super::work_sched::network_denied());
                }
            }
            work.sched.declare(who.owner, &spec, work.env.now_ms())?;
            Ok(0)
        }
        2 => {
            let key: wire::ServiceWorkKey<'_> = codec::single(input)?;
            work.sched.cancel(who.owner, key.key)?;
            Ok(0)
        }
        3 => {
            if !input.is_empty() {
                return Err(ERR_INVALID);
            }
            state(work, who.owner, out)
        }
        _ => Err(ERR_UNSUPPORTED),
    }
}

fn state(work: &WorkServices, owner: u64, out: &mut [u8]) -> Result<usize, i32> {
    let end = wire::ServiceEnd {
        flags: 0,
        total: 0,
        generation: 0,
        next: 0,
        cursor: None,
    };
    let reserved = codec::framed_len(&Record::ServiceEnd(end))?;
    let available = out.len().checked_sub(reserved).ok_or(ERR_NO_SPACE)?;
    let (mut used, mut total, mut failure) = (0_usize, 0_u32, None);
    work.sched.rows(owner, work.env.power(), &mut |row| {
        let record = Record::ServiceWorkRow(wire::ServiceWorkRow {
            token: row.token,
            state: row.state,
            reason: row.reason,
            due_ms: row.due_ms,
            key: row.key.as_str(),
        });
        match codec::encode_one(&record, &mut out[used..available]) {
            Ok(length) => {
                used += length;
                total += 1;
                true
            }
            Err(error) => {
                failure = Some(error);
                false
            }
        }
    })?;
    if let Some(error) = failure {
        return Err(error);
    }
    let end = wire::ServiceEnd { total, ..end };
    let length = codec::encode_one(&Record::ServiceEnd(end), &mut out[used..])?;
    Ok(used + length)
}

#[cfg(test)]
#[path = "work_service_tests.rs"]
mod tests;
