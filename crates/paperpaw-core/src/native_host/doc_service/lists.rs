//! The format and reading-app lists of `core.document` (ops 19 to 21).
use super::{DocHost, args, codec, encode, list, require, wire};
use paperpaw_native::host_glue::ERR_NOT_FOUND;

fn paged(input: &[u8], scope_allowed: bool) -> Result<wire::ServiceDocPage<'_>, i32> {
    let page: wire::ServiceDocPage<'_> = codec::single(input)?;
    // No document here, and `scope` is op 21's format filter alone.
    require(page.document == 0 && (scope_allowed || page.scope.is_empty()))?;
    args::page_state(page.cursor, page.generation)?;
    Ok(page)
}

pub(super) fn formats(
    host: &dyn DocHost,
    input: &[u8],
    rows: usize,
    out: &mut [u8],
) -> Result<usize, i32> {
    let page = paged(input, false)?;
    list(out, rows, |visit| {
        host.formats(page.cursor, page.generation, visit)
    })
}

pub(super) fn readers(
    host: &dyn DocHost,
    input: &[u8],
    rows: usize,
    out: &mut [u8],
) -> Result<usize, i32> {
    let page = paged(input, true)?;
    require(page.scope.is_empty() || args::id_valid(page.scope))?;
    list(out, rows, |visit| {
        host.readers(page.scope, page.cursor, page.generation, visit)
    })
}

pub(super) fn format_for(host: &dyn DocHost, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let path: wire::ServiceDocPath<'_> = codec::single(input)?;
    // Bit 0: sniff when no extension matches.
    require(
        path.root <= 1
            && path.flags & !1 == 0
            && args::path_valid(path.path)
            && args::reachable(path.root, path.path),
    )?;
    host.root_access(path.root)?;
    let mut written = Err(ERR_NOT_FOUND);
    host.format_for(path.root, path.path, path.flags & 1 != 0, &mut |record| {
        written = encode(&record, out);
    })?;
    written
}
