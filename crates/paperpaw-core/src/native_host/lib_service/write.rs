//! Writes of `core.library`: ops 8-12, 14, 17-21, 23, 24, 26.
use super::{Caller, ServiceOp};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

/// Status, files, positions, staged annotation transactions, fields.
pub(crate) trait LibWrite {}

pub(super) fn run(
    _host: &dyn super::LibHost,
    _who: &Caller<'_>,
    _op: &ServiceOp,
    _input: &[u8],
    _out: &mut [u8],
) -> Result<usize, i32> {
    Err(ERR_UNSUPPORTED)
}
