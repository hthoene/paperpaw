//! Generated-schema call services. A reply is built in the caller's buffer
//! and wiped there unless the call succeeds.
use super::{
    declared_services::DeclaredServices,
    environment::EnvironmentScope,
    registrations,
    resources::{OwnerResources, ServiceKey},
    secret_bytes::wipe,
};
use crate::abi7::services_wire::ServiceOp;
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_SPACE, ERR_UNSUPPORTED, OK, PpBuffer, PpBytes, buffers,
};

/// The reply room in the caller's buffer while a service fills it. Every
/// service can return private data: on every exit but a checked reply
/// (a backend error, a malformed reply) the room is wiped, so a failed call
/// leaves no part of a reply behind (`stages-services.md` 2.1). Never format
/// bytes.
struct ReplyOnFailure<'a>(&'a mut [u8]);

impl ReplyOnFailure<'_> {
    /// The reply of `length` bytes was checked: it stays for the caller, and
    /// whatever the service left past it in the room is wiped.
    fn keep(self, length: usize) {
        if let Some(rest) = self.0.get_mut(length..) {
            wipe(rest);
        }
        core::mem::forget(self);
    }
}

impl Drop for ReplyOnFailure<'_> {
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
/// Whether `input` and `out` share no byte. The reply is written into `out`
/// while the service reads `input`, so an overlapping pair is refused before
/// either slice exists (both pairs passed their own checks: no wrap-around).
fn apart(input: PpBytes, out: PpBuffer) -> bool {
    if input.len == 0 || out.cap == 0 {
        return true;
    }
    let (input, out) = (
        (input.ptr as usize, input.ptr as usize + input.len),
        (out.ptr as usize, out.ptr as usize + out.cap),
    );
    input.1 <= out.0 || out.1 <= input.0
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

/// How much of each reply row a reply check reads (todo 129).
///
/// A reply is built by a host backend with the generated encoders, so reading
/// every row by its record type is a self-check. It costs about 16 KB of
/// decoders in the X4 image, so a release build does it only where the rows
/// come from a plugin:
/// - **debug and test builds**: every row is decoded by its type (`All`);
/// - **release, `core.lookup`**: the provider's rows are a plugin's (`Lookup`):
///   the entry and end rows are decoded by their types;
/// - **release, every other service**: the framing of every row and the typed
///   end row (`Framing`). A row's body is its backend's own encoding, and the
///   asker decodes every row fallibly.
///
/// Whichever check reads a row, a row it refuses is `ERR_INVALID` (as the reply
/// check always answered).
///
/// All variants exist in every build (so each is compiled and tested); in a
/// release build `All` is never chosen by [`Rows::of`] and reads like `Framing`,
/// so the 93 row decoders it would need are not linked.
#[derive(Clone, Copy)]
enum Rows {
    All,
    Lookup,
    Framing,
}

impl Rows {
    /// The check a reply of `service` gets in this build.
    fn of(service: &str) -> Self {
        if cfg!(any(test, debug_assertions)) {
            Self::All
        } else if service == crate::abi7::services_wire::core_lookup_v1::KEY {
            Self::Lookup
        } else {
            Self::Framing
        }
    }

    /// The row's body, read by its type as far as this check goes.
    fn row(self, raw: &registrations::RawRecord<'_>) -> Result<(), i32> {
        match self {
            #[cfg(any(test, debug_assertions))]
            Self::All => rows::check(raw),
            Self::Lookup => rows::check_lookup(raw),
            #[cfg(any(test, debug_assertions))]
            Self::Framing => Ok(()),
            #[cfg(not(any(test, debug_assertions)))]
            Self::All | Self::Framing => Ok(()),
        }
    }
}

/// Shared by backend conformance tests: every row read by its type.
#[cfg(test)]
pub(crate) fn validate_reply(bytes: &[u8], op: &ServiceOp) -> Result<(), i32> {
    validate_reply_with(bytes, op, Rows::All)
}

fn validate_reply_with(bytes: &[u8], op: &ServiceOp, check: Rows) -> Result<(), i32> {
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
        // A bad row body is `ERR_INVALID` as far as `check` reads it.
        let raw = registrations::RawRecord::one(framed).map_err(|_| ERR_INVALID)?;
        check.row(&raw)?;
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
        if !apart(input, out) {
            return Err(ERR_INVALID);
        }
        scope.with_frame(|frame| {
            let key = owner.service(service)?;
            // SAFETY: the slot caller supplies readable input after pair checks.
            let input = unsafe { buffers::bytes_in(input, buffers::TEXT_MAX) }?;
            let op = validate_op(key, op, input, out.cap)?;
            let running = owner.enter_service(key)?;
            // SAFETY: the slot caller lends `out` for the call and nothing else
            // touches it while the call runs; it shares no byte with `input`
            // (`apart`, above). A refused call above wrote nothing.
            let output = unsafe { buffers::buffer_out(out, buffers::TEXT_MAX) }?;
            // The reply is built in the caller's own buffer, not in a host copy
            // of `max_reply` bytes (#248). It is initialised first, so the
            // service gets plain bytes.
            let output = output.get_mut(..op.max_reply).ok_or(ERR_NO_SPACE)?;
            for slot in output.iter_mut() {
                slot.write(0);
            }
            // SAFETY: every byte of `output` was written just above, and
            // `MaybeUninit<u8>` has the layout of `u8`.
            let output = unsafe { &mut *(core::ptr::from_mut(output) as *mut [u8]) };
            let reply = ReplyOnFailure(output);
            let caller = owner.caller(plugin, key.features);
            let length = running.run(frame, &caller, op, input, &mut *reply.0)?;
            let checked = reply.0.get(..length).ok_or(ERR_INVALID)?;
            validate_reply_with(checked, op, Rows::of(key.schema.key))?;
            reply.keep(length);
            Ok(length as i32)
        })?
    })();
    result.unwrap_or_else(|error| error)
}

#[path = "call_services/rows.rs"]
mod rows;

#[cfg(test)]
#[path = "call_services/tests.rs"]
mod tests;
