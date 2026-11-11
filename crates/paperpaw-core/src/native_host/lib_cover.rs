//! `core.library` op 7 `cover` (`docs/abi/library.md` 2.1): the book's cover
//! as an image id of the caller, **made from the book's format on first use**
//! by `core.document` (`doc_service::cover`, DOC op 15) and kept in the
//! book's document cache, where the format's own `.complete` stamp, and the
//! removal that goes with a move or a delete of the book (`book_data_steps`),
//! already look after it.
//!
//! The first version makes the picture **inside the asking call**, in the
//! caller's frame (formats can be called nowhere else); a later call finds
//! the picture in the cache and decodes nothing. Making it in the background
//! (the work pump) is not in this version. `ERR_BUSY` means: no place in the
//! caller's document table now, or a format not running yet (its start is
//! requested). Only the second raises `event.library_changed` bit 6, so the
//! caller asks again; a full table raises nothing (it does not clear by
//! itself: the caller closes a document). The raises are counted by **one
//! counter for the whole library host** (not per book): after six in a row
//! it raises no more until a cover is made, so a start that never ends
//! cannot loop a caller that redraws on the event. A format whose `open` is still *preparing* is
//! `ERR_NOT_FOUND` (no cover now: the host's preparing loop does not run for
//! a library call, and a retry would spin); the book shows its outline until
//! the screen is built again, which finds the cover once the reader has
//! prepared the book.
//!
//! Nothing is kept between calls: the host's scratch (4 KiB, `doc_service::
//! cover`) and the book's path are freed on return.
use super::{
    doc_service::cover,
    host_services::Caller as HostCaller,
    lib_service::{Caller, One},
    registrations::Record,
};
use crate::abi7::context::Frame;
use alloc::string::String;
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_UNSUPPORTED};

/// What a `cover` call needs besides the book: the caller's frame (the format
/// is called through it) and the caller as the shell's services know it (the
/// document service and the image table are reached through it).
pub(crate) struct CoverCx<'a> {
    pub(crate) frame: &'a dyn Frame,
    pub(crate) caller: &'a HostCaller<'a>,
}

/// The cover of the book at `path` for `who` (`side` as in the contract).
///
/// # Errors
/// `ERR_UNSUPPORTED` where no document service is wired; as
/// [`cover::make`].
pub(crate) fn make(
    cx: &CoverCx<'_>,
    who: &Caller<'_>,
    path: &str,
    side: u32,
    visit: One<'_>,
) -> Result<(), i32> {
    if cx.caller.hosts.documents().is_none() {
        return Err(ERR_UNSUPPORTED);
    }
    debug_assert_eq!(cx.caller.owner, who.owner);
    let image = cover::make(cx.frame, cx.caller, path, side)?;
    visit(Record::ServiceDocImage(image));
    Ok(())
}

/// A book's root-1 path, owned (the catalog hands it out for the length of
/// one callback only).
pub(crate) fn owned(path: &str) -> Result<String, i32> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(path.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    owned.push_str(path);
    Ok(owned)
}
