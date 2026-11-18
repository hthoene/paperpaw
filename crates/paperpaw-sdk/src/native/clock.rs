//! The host's monotonic microsecond clock, with no epoch shared with event time.
use super::{Error, Host, binding::status};

/// Read a nondecreasing microsecond count within this start of PaperPaw.
/// The counter may restart after boot or power-cutting sleep. It is not wall time.
///
/// # Errors
/// Returns the host refusal without substituting a guessed time.
pub fn micros(host: &Host<'_>) -> Result<u64, Error> {
    let mut out = 0;
    // SAFETY: entry admission retains the table/context; output is call-local.
    status(unsafe { (host.table.clock_micros)(host.ctx, &mut out) })?;
    Ok(out)
}
