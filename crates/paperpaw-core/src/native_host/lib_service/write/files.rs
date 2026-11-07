//! Delete, move and import (`library.md` 2.3).
use super::{
    super::{Caller, args, wire::Record},
    need_storage, require, result_row,
};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_INVALID, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE, OK,
};

/// The most paths of one `can_delete` or `delete` (`lib_paths`, 7 x 512).
const MAX_PATHS: usize = 7;
/// FAT's name limit in UTF-16 units.
const SEGMENT_UNITS: usize = 255;
/// `lib_result.path` and every root-1 path.
const PATH_MAX: usize = 512;

/// What the host is asked to import. Names are checked already.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ImportRequest<'a> {
    /// The file in the caller's root 0.
    pub(crate) from: &'a str,
    /// A folder below the library folder, empty for the folder itself.
    pub(crate) folder: &'a str,
    pub(crate) name: &'a str,
    /// Replace the plugin's own earlier import of that name.
    pub(crate) replace: bool,
    /// Take a free name (`name (2).ext`) when it exists.
    pub(crate) free_name: bool,
}

/// `lib_paths.paths`: one per line, 1 to 7 valid paths. A line with a control
/// character cannot exist (the catalog has none), so nothing breaks the list.
fn each_path<'a>(
    host: &dyn super::super::LibHost,
    paths: &'a str,
    mut visit: impl FnMut(&'a str),
) -> Result<(), i32> {
    require(!paths.is_empty())?;
    let mut count = 0;
    for path in paths.split('\n') {
        require(args::root1_path(host, path))?;
        count += 1;
        if count > MAX_PATHS {
            return Err(ERR_NO_SPACE);
        }
        visit(path);
    }
    Ok(())
}

/// Ops 9 `can_delete` and 10 `delete`: one `lib_result` per path, in order.
pub(super) fn paths(
    host: &dyn super::super::LibHost,
    who: &Caller<'_>,
    delete: bool,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let Record::ServiceLibPaths(list) = args::single(input)? else {
        return Err(ERR_INVALID);
    };
    // Bit 0 `with_notes` belongs to `delete`.
    require(list.flags & !u32::from(delete) == 0)?;
    if delete {
        need_storage(who)?;
    }
    // Check the whole list first: a malformed line refuses all of it.
    each_path(host, list.paths, |_| ())?;
    let mut used = 0;
    let mut failure = None;
    each_path(host, list.paths, |path| {
        if failure.is_some() {
            return;
        }
        let (status, flags) = if delete {
            delete_one(host, path, list.flags & 1 != 0)
        } else {
            host.can_delete(path)
        };
        match result_row(path, status, flags, &mut out[used..]) {
            Ok(written) => used += written,
            Err(error) => failure = Some(error),
        }
    })?;
    failure.map_or(Ok(used), Err)
}

/// A book with annotations is `ERR_STATE` unless the caller says `with_notes`
/// (the view asked the user); nothing is removed then.
fn delete_one(host: &dyn super::super::LibHost, path: &str, with_notes: bool) -> (i32, u32) {
    // A book with an annotation transaction open is in use.
    match super::edit::book_busy(host, path) {
        Ok(false) => {}
        Ok(true) => return (ERR_BUSY, 0),
        Err(error) => return (error, 0),
    }
    let (status, facts) = host.can_delete(path);
    match status {
        OK if facts & 1 != 0 && !with_notes => (ERR_STATE, facts),
        OK | ERR_NOT_FOUND => host.delete(path),
        other => (other, facts),
    }
}

/// Op 11: `from` and `to` are root-1 paths; `flags` bit 0 replaces `to`.
pub(super) fn move_book(
    host: &dyn super::super::LibHost,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let Record::ServiceLibMove(request) = args::single(input)? else {
        return Err(ERR_INVALID);
    };
    require(
        request.flags & !1 == 0
            && args::root1_path(host, request.from)
            && args::root1_path(host, request.to)
            && request.from != request.to,
    )?;
    need_storage(who)?;
    // Neither the book that moves nor the one it replaces may be in use.
    for path in [request.from, request.to] {
        if super::edit::book_busy(host, path)? {
            return Err(ERR_BUSY);
        }
    }
    host.move_book(request.from, request.to, request.flags & 1 != 0)?;
    result_row(request.to, OK, 0, out)
}

/// Op 12: the name rules of `library.md` 2.3, then the host imports.
pub(super) fn import(
    host: &dyn super::super::LibHost,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let Record::ServiceLibImport(request) = args::single(input)? else {
        return Err(ERR_INVALID);
    };
    // Bit 0 replace, bit 1 take a free name; both is a contradiction.
    require(request.flags & !0b11 == 0 && request.flags != 0b11)?;
    require(args::root1_path(host, request.from))?;
    check_names(request.folder, request.name)?;
    let mut joined = Ok(());
    host.library_folder(&mut |library| {
        joined = check_join(host, library, request.folder, request.name);
    })?;
    joined?;
    let import = ImportRequest {
        from: request.from,
        folder: request.folder,
        name: request.name,
        replace: request.flags & 1 != 0,
        free_name: request.flags & 2 != 0,
    };
    let mut written = Err(ERR_NOT_FOUND);
    host.import(who, &import, &mut |path| {
        written = result_row(path, OK, 0, out);
    })?;
    written
}

/// One path segment: no control character or `"*:<>?|`, no trailing dot or
/// space, not a device name, at most 255 UTF-16 units, not `.` or `..`.
fn segment_ok(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.ends_with(['.', ' '])
        && !segment
            .chars()
            .any(|c| c.is_control() || "\"*:<>?|\\/".contains(c))
        && !paperpaw_platform::storage::is_reserved_name(segment)
        && segment.encode_utf16().count() <= SEGMENT_UNITS
}

/// The names come from outside (a feed, a server): `ERR_INVALID` before
/// anything is written.
fn check_names(folder: &str, name: &str) -> Result<(), i32> {
    require(segment_ok(name) && !name.starts_with(['.', '~']))?;
    require(folder.is_empty() || folder.split('/').all(segment_ok))
}

/// The joined path ends below the library folder and not in `.paperpaw/` or
/// `plugins/`, which a library folder at the card's root would reach
/// (`ERR_DENIED`), and fits a path (`ERR_NO_SPACE`).
fn check_join(
    host: &dyn super::super::LibHost,
    library: &str,
    folder: &str,
    name: &str,
) -> Result<(), i32> {
    let mut length = 0;
    let mut first = None;
    for part in [library, folder, name]
        .into_iter()
        .filter(|p| !p.is_empty())
    {
        if first.is_none() {
            first = part.split('/').next();
        }
        length += part.len() + usize::from(length != 0);
    }
    if first.is_some_and(|segment| args::excluded(host, segment)) {
        return Err(ERR_DENIED);
    }
    if length > PATH_MAX {
        return Err(ERR_NO_SPACE);
    }
    Ok(())
}
