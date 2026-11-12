//! Bounded implicit core.status calls to one declared owner and token.
use super::{
    native_declarations::NativeDeclarations,
    native_screen::Dispatch,
    registrations::{self as wire, Record},
    stages_wire::core_status_v1 as status,
    ui_service::{StatusAsk, UiDirectory},
};
use crate::abi7::{context::Frame, core_frame::CoreFrame};
use crate::native_host::codec;
use alloc::vec::Vec;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_UNSUPPORTED,
};

pub(crate) struct StatusCalls<'a> {
    pub(crate) frame: &'a CoreFrame<'a>,
    pub(crate) directory: &'a NativeDeclarations,
}
impl StatusAsk for StatusCalls<'_> {
    fn ask(
        &self,
        owner: u64,
        token: u32,
        items: &[&str],
        context: &[u8],
        out: &mut [u8],
    ) -> Result<usize, i32> {
        if super::ui_store::nested_closed(self.frame.env.device().memory) {
            return Err(ERR_UNSUPPORTED);
        }
        self.frame.callback_epoch()?;
        if self.frame.callback_depth()? != 1 {
            return Err(ERR_BUSY);
        }
        if self.frame.caller()? == owner {
            return Err(ERR_BUSY);
        }
        if items.is_empty() || items.len() > status::STATUS_ASK_ITEMS_MAX_ITEMS {
            return Err(ERR_INVALID);
        }
        for item in items {
            let declared = self.directory.status_item(item)?;
            if declared.owner != owner || declared.token != token {
                return Err(ERR_DENIED);
            }
        }
        let lease = self
            .directory
            .store
            .try_borrow()
            .map_err(|_| ERR_BUSY)?
            .native_lease(owner)?;
        if lease.live()?.callback_depth() != 1 {
            return Err(ERR_BUSY);
        }
        let input = input(items, context)?;
        let mut reply = scratch(status::MAX_REPLY)?;
        let result = lease
            .stage(token, &input, &mut reply, self.frame.env)
            .and_then(|length| {
                let length = usize::try_from(length).map_err(|_| length)?;
                let bytes = reply.get(..length).ok_or(ERR_INVALID)?;
                status::reply_decode(bytes)?;
                if length > out.len() {
                    return Err(ERR_NO_SPACE);
                }
                out[..length].copy_from_slice(bytes);
                Ok(length)
            });
        if let Err(error) = result {
            lease.dropped(token, error);
        }
        result
    }
}

pub(super) fn scratch(length: usize) -> Result<Vec<u8>, i32> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| ERR_NO_MEMORY)?;
    bytes.resize(length, 0);
    Ok(bytes)
}
fn input(items: &[&str], context: &[u8]) -> Result<Vec<u8>, i32> {
    let mut names = Vec::new();
    let length = items.iter().try_fold(0usize, |sum, item| {
        sum.checked_add(item.len())
            .and_then(|n| n.checked_add(1))
            .ok_or(ERR_NO_SPACE)
    })?;
    names.try_reserve_exact(length).map_err(|_| ERR_NO_MEMORY)?;
    for (index, item) in items.iter().enumerate() {
        if index != 0 {
            names.push(b'\n');
        }
        names.extend_from_slice(item.as_bytes());
    }
    let names = core::str::from_utf8(&names).map_err(|_| ERR_INVALID)?;
    let capacity = names
        .len()
        .checked_add(context.len())
        .and_then(|length| length.checked_add(32))
        .filter(|length| *length <= status::MAX_INPUT)
        .ok_or(ERR_NO_SPACE)?;
    let mut payload = scratch(capacity)?;
    let length = status::StatusAsk::new(names, context).encode(&mut payload)?;
    payload.truncate(length);
    let record = Record::StageCall(wire::StageCall {
        stage: status::KEY,
        schema: u32::from(status::MAJOR),
        payload: &payload,
    });
    let mut call = scratch(
        codec::framed_len(&record)?
            .checked_add(1)
            .ok_or(ERR_NO_SPACE)?,
    )?;
    codec::encode_batch(&[record], &mut call)?;
    Ok(call)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
    use super::*;
    #[test]
    fn owner_call_preserves_group_order_and_context_inside_stage_envelope() {
        let bytes = input(&["remote.second", "remote.first"], &[0, 9, 255]).unwrap();
        let Record::StageCall(call) = codec::decode_any_single(&bytes).unwrap() else {
            panic!("expected stage.call")
        };
        assert_eq!(
            (call.stage, call.schema),
            (status::KEY, u32::from(status::MAJOR))
        );
        let ask = status::StatusAsk::decode(call.payload).unwrap();
        assert_eq!(ask.items, "remote.second\nremote.first");
        assert_eq!(ask.context, &[0, 9, 255]);
    }
    #[test]
    fn owner_call_rejects_oversized_context_and_item_lists() {
        assert!(input(&["remote.item"], &[0; 1025]).is_err());
        assert!(input(&["remote.item"; 9], &[]).is_err());
    }
}

#[cfg(test)]
#[path = "native_status_tests.rs"]
mod owner_tests;
