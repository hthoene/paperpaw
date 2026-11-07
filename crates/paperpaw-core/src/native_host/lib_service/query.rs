//! Reads of `core.library`: ops 1-7, 13, 15, 16, 22, 25.
use super::{Caller, ServiceOp};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

/// The catalog, positions, annotations and fields as the service reads them.
pub(crate) trait LibQuery {}

pub(super) fn run(
    _host: &dyn super::LibHost,
    _who: &Caller<'_>,
    _op: &ServiceOp,
    _input: &[u8],
    _out: &mut [u8],
) -> Result<usize, i32> {
    Err(ERR_UNSUPPORTED)
}
