//! Host-validated submission of callback registrations and declarations during make.
use super::{Error, Host, binding::status};
use paperpaw_native::host_glue::PpBytes;

/// Submit a record batch to the construction scope. For a declaration
/// builder, use `|b| submit(host, b).map_err(i32::from)` as its submission sink.
/// The host validates the entire batch before committing any registration.
///
/// # Errors
/// Returns the host's refusal. Every batch, including an oversized or malformed
/// one, reaches the host so it can poison construction if its error is ignored.
/// Use the builder/encoder to produce canonical input within the host's limits.
pub fn submit(host: &Host<'_>, batch: &[u8]) -> Result<(), Error> {
    // SAFETY: callback binding and borrowed batch remain live for this call.
    status(unsafe {
        (host.table.registrations_submit)(
            host.ctx,
            PpBytes {
                ptr: batch.as_ptr(),
                len: batch.len(),
            },
        )
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
    use super::*;
    use core::{cell::Cell, ffi::c_void, ptr};
    use paperpaw_native::host_glue::{self as abi, OK};
    unsafe extern "C" fn submit_slot(ctx: *mut c_void, bytes: PpBytes) -> i32 {
        // SAFETY: test context is a live Cell; wrapper supplies readable bytes.
        unsafe {
            let called = &*ctx.cast::<Cell<u32>>();
            called.set(called.get() + 1);
            if bytes.len > 4096 {
                return abi::ERR_NO_SPACE;
            }
        }
        OK
    }
    #[test]
    fn forwards_oversized_and_malformed_batches_to_the_host() {
        let mut table = abi::host_table();
        table.registrations_submit = submit_slot;
        let mut called = Cell::new(0_u32);
        let host = Host {
            table: &table,
            ctx: ptr::from_mut(&mut called).cast(),
        };
        assert_eq!(submit(&host, &[0; 4097]), Err(Error::NoSpace));
        assert_eq!(called.get(), 1);
        // Malformed input is forwarded so the host can poison construction.
        submit(&host, &[1]).unwrap();
        assert_eq!(called.get(), 2);
        let bytes = [0];
        submit(&host, &bytes).unwrap();
        let table = abi::host_table();
        let host = Host {
            table: &table,
            ctx: ptr::from_mut(&mut called).cast(),
        };
        assert_eq!(submit(&host, &bytes), Err(Error::Unsupported));
    }
}
