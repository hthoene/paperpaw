//! Generated-schema call services. Successful replies publish once from scratch.
use super::{
    declared_services::DeclaredServices,
    environment::EnvironmentScope,
    registrations,
    resources::{OwnerResources, ServiceKey},
    secret_bytes::wipe,
};
use crate::abi7::services_wire::ServiceOp;
use alloc::vec::Vec;
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_UNSUPPORTED, OK, PpBuffer, PpBytes, buffers,
};

/// Every service can return private data. Wipe the whole host scratch on all
/// exits, including a backend error or a malformed reply. Never format bytes.
struct ReplyScratch<'a>(&'a mut [u8]);

impl Drop for ReplyScratch<'_> {
    fn drop(&mut self) {
        wipe(self.0);
    }
}

fn input_pair(pair: PpBytes) -> Result<(), i32> {
    if pair.len > buffers::TEXT_MAX
        || pair.len != 0
            && (pair.ptr.is_null() || (pair.ptr as usize).checked_add(pair.len).is_none())
    {
        Err(ERR_INVALID)
    } else {
        Ok(())
    }
}
fn scalar_out(out: *mut u32) -> Result<(), i32> {
    if out.is_null()
        || !(out as usize).is_multiple_of(core::mem::align_of::<u32>())
        || (out as usize)
            .checked_add(core::mem::size_of::<u32>())
            .is_none()
    {
        Err(ERR_INVALID)
    } else {
        Ok(())
    }
}
fn output_pair(out: PpBuffer) -> Result<(), i32> {
    if out.cap > buffers::TEXT_MAX
        || out.cap != 0 && (out.ptr.is_null() || (out.ptr as usize).checked_add(out.cap).is_none())
    {
        Err(ERR_INVALID)
    } else {
        Ok(())
    }
}
fn key_valid(key: &str) -> bool {
    let mut segments = 0;
    key.split('.').all(|part| {
        segments += 1;
        (1..=32).contains(&part.len())
            && part.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    }) && segments >= 2
}

/// # Safety
/// The key and scalar output are live, aligned and disjoint for this call.
#[allow(
    clippy::too_many_arguments,
    reason = "host dependencies plus generated scalar slot arguments"
)]
pub(super) unsafe fn open(
    scope: &EnvironmentScope,
    owner: &OwnerResources,
    declared: &DeclaredServices,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    let result = (|| {
        input_pair(key)?;
        if key.len > 128 {
            return Err(ERR_INVALID);
        }
        scalar_out(out)?;
        scope.with_frame(|_| {
            // SAFETY: checked numeric bounds; the slot caller supplies key storage.
            let key = unsafe { buffers::text_in(key, 128) }?;
            if !key_valid(key) {
                return Err(ERR_INVALID);
            }
            let schema = owner.service_schema(key)?;
            let (declared_major, revision) = declared.declared(key).ok_or(ERR_UNSUPPORTED)?;
            if major != schema.major
                || major != declared_major
                || minor > revision
                || minor > schema.revision
            {
                return Err(ERR_UNSUPPORTED);
            }
            let handle = owner.insert_service(
                ServiceKey {
                    schema,
                    minor,
                    features: declared.features(),
                },
                declared.count(),
            )?;
            // SAFETY: only success publishes to the validated scalar output.
            unsafe {
                out.write(handle);
            }
            Ok(())
        })?
    })();
    result.map_or_else(|error| error, |()| OK)
}

fn validate_batch(bytes: &[u8], expected: &[u32], max_records: usize) -> Result<(), i32> {
    // The generated counted visitor enforces input_rows before any callback.
    registrations::host_records_visit(bytes, max_records, &mut |_| Ok(()))?;
    super::stage_wire::visit_batch(bytes, u32::MAX, |tag, _| {
        if expected.contains(&tag) {
            Ok(())
        } else {
            Err(ERR_UNSUPPORTED)
        }
    })
}

/// Shared by dispatch and backend conformance tests; replies are bare streams.
pub(crate) fn validate_reply(bytes: &[u8], op: &ServiceOp) -> Result<(), i32> {
    if bytes.len() > op.max_reply {
        return Err(ERR_INVALID);
    }
    let mut rows = 0_u32;
    let mut ended = false;
    let mut next_tag = 0;
    super::stage_wire::visit_stream(bytes, |tag, framed| {
        if ended {
            return Err(ERR_INVALID);
        }
        let index = op
            .reply
            .iter()
            .position(|expected| *expected == tag)
            .ok_or(ERR_INVALID)?;
        // Only the end row's body is read; a row of another tag is framed and
        // listed by the op, and its body is the backend's own encoding.
        let raw = registrations::RawRecord::one(framed).map_err(|_| ERR_INVALID)?;
        if op.reply_rows == 0 {
            if tag == registrations::SERVICE_END_TAG || index < next_tag {
                return Err(ERR_INVALID);
            }
            next_tag = index + 1;
            rows += 1;
            return Ok(());
        }
        match tag {
            registrations::SERVICE_END_TAG => {
                let end: registrations::ServiceEnd = raw.decode().map_err(|_| ERR_INVALID)?;
                if !op.paged
                    && (end.total != rows || end.next != 0 || end.generation != 0 || end.flags != 0)
                {
                    return Err(ERR_INVALID);
                }
                ended = true;
            }
            _ => {
                rows += 1;
                if rows > u32::from(op.reply_rows) {
                    return Err(ERR_INVALID);
                }
            }
        }
        Ok(())
    })
    .map_err(|_| ERR_INVALID)?;
    if op.reply_rows != 0 && !ended {
        return Err(ERR_INVALID);
    }
    Ok(())
}

fn validate_op(
    key: ServiceKey,
    op: u32,
    input: &[u8],
    output: usize,
) -> Result<&'static ServiceOp, i32> {
    let op = key
        .schema
        .op(op)
        .filter(|op| op.since <= key.minor)
        .ok_or(ERR_UNSUPPORTED)?;
    // A gated op asks the opening instance's admitted features before the
    // sizes: `ERR_UNSUPPORTED` when the host has not wired the feature,
    // `ERR_DENIED` when this instance did not request it.
    if let Some(required) = op.feature {
        key.features.feature(required)?;
    }
    if input.len() > op.max_input || output < op.max_reply {
        return Err(ERR_NO_SPACE);
    }
    validate_batch(input, op.input, usize::from(op.input_rows))?;
    Ok(op)
}

/// # Safety
/// Input/output storage is live, disjoint and exclusive as required by the slot.
pub(super) unsafe fn call(
    scope: &EnvironmentScope,
    owner: &OwnerResources,
    plugin: &str,
    service: u32,
    op: u32,
    input: PpBytes,
    out: PpBuffer,
) -> i32 {
    let result = (|| {
        input_pair(input)?;
        output_pair(out)?;
        scope.with_frame(|frame| {
            let key = owner.service(service)?;
            // SAFETY: the slot caller supplies readable input after pair checks.
            let input = unsafe { buffers::bytes_in(input, buffers::TEXT_MAX) }?;
            let op = validate_op(key, op, input, out.cap)?;
            let running = owner.enter_service(key)?;
            let mut scratch = Vec::new();
            scratch
                .try_reserve_exact(op.max_reply)
                .map_err(|_| ERR_NO_MEMORY)?;
            scratch.resize(op.max_reply, 0);
            let scratch = ReplyScratch(&mut scratch);
            let caller = owner.caller(plugin, key.features);
            let length = running.run(frame, &caller, op, input, scratch.0)?;
            let reply = scratch.0.get(..length).ok_or(ERR_INVALID)?;
            validate_reply(reply, op)?;
            // SAFETY: output has the declared capacity; no output was written on
            // any earlier failure, including a service's own failure.
            let output = unsafe { buffers::buffer_out(out, buffers::TEXT_MAX) }?;
            for (slot, byte) in output.iter_mut().zip(reply) {
                slot.write(*byte);
            }
            Ok(length as i32)
        })?
    })();
    result.unwrap_or_else(|error| error)
}

#[cfg(test)]
#[path = "call_services/tests.rs"]
mod tests;
