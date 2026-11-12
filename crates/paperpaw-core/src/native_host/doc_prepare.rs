//! The document host's preparing loop, and the cache upkeep it shares with
//! the opener's calls (`docs/abi/document.md` 2.1: "Preparing").
//!
//! A format whose `open` answered `state` 1 (preparing) is called again with
//! op 3 `prepare`, one document and one call a pass, by the kernel's pump
//! (`doc_pump`), between inputs: the pump runs only outside every callback
//! (instance depth 0), so no opener callback runs; the format is entered
//! through the retained instance registry, its own call being the one level
//! (depth 1), and the call is the format's own, metered to it. The format works for at most 50 ms of its
//! own clock and answers its progress; the answer is checked and recorded
//! (`doc_service::prepared`), the opener hears of a change with
//! `event.document_changed` (88, one per document, the latest values), and
//! when the format says done the host makes the cache stamp (`document_ready`).
//! A device that has two document places (the X4) prepares one book at a time,
//! in open order; elsewhere the preparing documents take turns.
//!
//! A step that cannot be made now (the format is not started, a table is
//! borrowed, no room for a buffer) is [`PrepareStep::Later`]; a format that
//! answers with an error or a malformed answer fails the document (`doc_state`
//! `state` 2 with the status, `ERR_INVALID` for a malformed one) and the
//! opener hears of it; `ERR_NO_SPACE` also removes the partial cache. Nothing
//! here allocates but one 4 KiB reply buffer a step, taken fallibly and freed
//! with it.
use super::{BookKind, DocState, FOLDER, NoImages, SIBLINGS};
use crate::native_host::{
    doc_service::{self, FAILED, FormatCall, FormatRef, Options, PrepareStep, READY, fmt},
    fmt_call::{Opener, Route, Stage},
    format_call::Target,
    format_scope::{FormatBook, Guard},
    host_services::HostServices,
};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_CANCELLED, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_TIMEOUT,
};

/// The reply room of one step: op 3 answers one `fmt.state`, as the open
/// does, so the open's room (not the stage's 3,840 bytes: it is held while the
/// format prepares, #248).
const REPLY_BYTES: usize = crate::native_host::doc_service::details::OPEN_REPLY;

impl DocState {
    /// Where the format of `format` is called, and how it reads its book.
    ///
    /// # Errors
    /// `ERR_BUSY` (the index is borrowed), `ERR_CANCELLED` (the format is
    /// gone).
    pub(super) fn format_target(&self, format: FormatRef) -> Result<(Target, BookKind), i32> {
        let index = self.index()?;
        let kind = if index.has_flag(format, FOLDER) {
            BookKind::Folder
        } else if index.has_flag(format, SIBLINGS) {
            BookKind::Siblings
        } else {
            BookKind::File
        };
        Ok((index.target(format).ok_or(ERR_CANCELLED)?, kind))
    }

    /// The format's call for `document` begins: its roots 3, 4 and 6 are the
    /// document's, until the guard is dropped.
    ///
    /// # Errors
    /// `ERR_BUSY`, `ERR_CANCELLED` (no such book), `ERR_NO_MEMORY`, and what
    /// the scope says.
    pub(super) fn enter_scope<'h>(
        &self,
        hosts: &'h HostServices,
        owner: u64,
        document: u32,
        kind: BookKind,
        css: &str,
    ) -> Result<Guard<'h>, i32> {
        let books = self.books.try_borrow().map_err(|_| ERR_BUSY)?;
        let book = books
            .iter()
            .find(|book| book.document == document)
            .ok_or(ERR_CANCELLED)?;
        let opened = if kind == BookKind::Siblings {
            book.path.rsplit('/').next().unwrap_or("")
        } else {
            ""
        };
        hosts.format_scope().enter(
            owner,
            &FormatBook {
                path: &book.path,
                kind,
                opened,
                document,
                cache: book.cache.as_ref().map_or("", |cache| &cache.folder),
                css,
                cache_cap: self.cache_cap,
                quota: None,
            },
        )
    }

    /// The last document of a cache closes: what a format wrote after the
    /// stamp was made is put in the stamp (so a reopen counts it), and the
    /// folder's count goes (it is made again from the stamp at the next open).
    /// A cache another open document uses is left as it is.
    pub(super) fn finish_cache(&self, hosts: &HostServices, document: u32) {
        let (Some(storage), Ok(books)) = (self.storage.as_deref(), self.books.try_borrow()) else {
            return;
        };
        let Some(cache) = books
            .iter()
            .find(|book| book.document == document)
            .and_then(|book| book.cache.as_ref())
        else {
            return;
        };
        let shared = books.iter().any(|b| {
            b.document != document && b.cache.as_ref().is_some_and(|c| c.folder == cache.folder)
        });
        if shared {
            return;
        }
        let scope = hosts.format_scope();
        let bytes = scope.total(&cache.folder);
        if cache.finished && bytes != cache.stamped {
            let _ = super::super::doc_cache::complete(storage, &cache.identity(), bytes);
        }
        scope.reset(&cache.folder);
    }

    /// A call answered `ERR_NO_SPACE` while the cache was being made: the
    /// partial cache is removed (`document.md` 3, Cache quota), unless another
    /// open document uses the folder. What a bounded removal leaves is
    /// removed with the next open, as an unfinished folder.
    pub(super) fn discard_cache(&self, hosts: &HostServices, document: u32) {
        let (Some(storage), Ok(books)) = (self.storage.as_deref(), self.books.try_borrow()) else {
            return;
        };
        let Some(cache) = books
            .iter()
            .find(|book| book.document == document)
            .and_then(|book| book.cache.as_ref())
            // A `details` open's folder is not its own to remove.
            .filter(|cache| !cache.finished && !cache.details)
        else {
            return;
        };
        let shared = books.iter().any(|b| {
            b.document != document && b.cache.as_ref().is_some_and(|c| c.folder == cache.folder)
        });
        if !shared {
            let _ = super::super::doc_cache::clear(storage, &cache.folder);
            hosts.format_scope().reset(&cache.folder);
        }
    }

    /// The format answered that `document` is ready: the cache is finished,
    /// and the stamp says it (a cache found finished at the open keeps its
    /// stamp).
    pub(super) fn document_ready(&self, hosts: &HostServices, document: u32) {
        let Some(storage) = self.storage.as_deref() else {
            return;
        };
        let stamped = {
            let Ok(books) = self.books.try_borrow() else {
                return;
            };
            let Some(cache) = books
                .iter()
                .find(|book| book.document == document)
                .and_then(|book| book.cache.as_ref())
                // A finished cache found at the open keeps its stamp; a
                // `details` open makes none (`document.md` 2.1).
                .filter(|cache| !cache.finished && !cache.details)
            else {
                return;
            };
            let bytes = hosts.format_scope().total(&cache.folder);
            // A stamp that cannot be made leaves the cache unfinished: the next
            // open makes it again.
            if super::super::doc_cache::complete(storage, &cache.identity(), bytes).is_err() {
                return;
            }
            bytes
        };
        if let Ok(mut books) = self.books.try_borrow_mut()
            && let Some(cache) = books
                .iter_mut()
                .find(|book| book.document == document)
                .and_then(|book| book.cache.as_mut())
        {
            cache.finished = true;
            cache.stamped = stamped;
        }
    }

    /// One step of the preparing loop (see the module).
    #[inline(never)]
    pub(super) fn prepare_next(&self, hosts: &HostServices, route: &dyn Route) -> PrepareStep {
        let Some(document) = self.table.next_preparing(self.last_prepared.get()) else {
            return PrepareStep::Idle;
        };
        match self.prepare_one(hosts, route, &document) {
            Ok(()) => {
                self.last_prepared.set(document.id);
                PrepareStep::More
            }
            // The format is not started, or the plugin is its own format, or a
            // table is held, or there is no room for the buffer: later. The
            // turn still passes on, so where several documents take turns one
            // whose format cannot be called now does not hold up the others.
            Err(ERR_BUSY | ERR_NO_MEMORY | ERR_CANCELLED) => {
                self.last_prepared.set(document.id);
                PrepareStep::Later
            }
            Err(status) => {
                self.fail_prepared(hosts, &document, status);
                PrepareStep::More
            }
        }
    }

    /// `doc_service::pending`: the open's batch is kept with the book until
    /// the open is sent.
    pub(super) fn keep_open(&self, document: u32, batch: &[u8]) -> Result<(), i32> {
        let mut books = self.books.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let book = books
            .iter_mut()
            .find(|book| book.document == document)
            .ok_or(ERR_CANCELLED)?;
        book.open.clear();
        book.open
            .try_reserve_exact(batch.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        book.open.extend_from_slice(batch);
        Ok(())
    }

    /// The pending open reached the format: the document is the format's
    /// (it gets `close`), and the kept batch goes.
    fn open_sent(&self, document: u32) {
        self.table.open_sent(document);
        if let Ok(mut books) = self.books.try_borrow_mut()
            && let Some(book) = books.iter_mut().find(|book| book.document == document)
        {
            book.open = Vec::new();
        }
    }

    /// The document's preparation failed with `status`: it is `state` 2 and
    /// its opener hears of it.
    fn fail_prepared(&self, hosts: &HostServices, document: &doc_service::Document, status: i32) {
        // A status that cannot say "failed" is the format's malformed answer.
        let status = if status < 0 { status } else { ERR_INVALID };
        if self
            .table
            .set_prepared(
                document.id,
                FAILED,
                document.progress_ppm,
                status,
                document.flags,
            )
            .is_ok_and(|(moved, ..)| moved)
        {
            self.fresh.set(true);
        }
        if status == ERR_NO_SPACE {
            self.discard_cache(hosts, document.id);
        }
    }

    /// One call of `op` to the format of `document`, outside any callback,
    /// with the document's roots 3, 4 and 6 (as in an open) and its sides and
    /// conversion; the answer at the front of `out`, its length returned. The
    /// preparing loop's step and the cover job's cover (`doc_cover_make`).
    ///
    /// # Errors
    /// As `Stage::call`, `ERR_BUSY` for a book table held now.
    pub(super) fn call_in_scope(
        &self,
        hosts: &HostServices,
        route: &dyn Route,
        document: &doc_service::Document,
        (target, kind): (Target, BookKind),
        op: u32,
        out: &mut [u8],
    ) -> Result<usize, i32> {
        let (css, open) = {
            let books = self.books.try_borrow().map_err(|_| ERR_BUSY)?;
            let book = books
                .iter()
                .find(|book| book.document == document.id)
                .ok_or(ERR_NOT_FOUND)?;
            // A pending open (`doc_service::pending`) is sent as it was asked.
            let mut open = Vec::new();
            if op == fmt::OPEN {
                open.try_reserve_exact(book.open.len())
                    .map_err(|_| ERR_NO_MEMORY)?;
                open.extend_from_slice(&book.open);
            }
            let css = book.cache.as_ref().map_or("", |cache| &cache.css);
            (super::copy(css)?, open)
        };
        let mut call = FormatCall::new(document.format, op, document.id);
        call.conversion = document.conversion;
        call.image_side = document.image_side;
        call.cover_side = document.cover_side;
        if op == fmt::OPEN {
            call.flags = document.flags;
            call.options = Options::from_batch(&open);
        }
        // The format's roots 3, 4 and 6 are this call's, as in an open.
        let _scope = self.enter_scope(hosts, target.plugin, document.id, kind, &css)?;
        let stage = Stage {
            route,
            images: &NoImages,
        };
        let mut result = Err(ERR_NOT_FOUND);
        let mut run = |sibling: &str| {
            result = stage.call(
                Opener {
                    owner: document.owner,
                    sibling,
                },
                target,
                &call,
                out,
            );
        };
        if kind == BookKind::Siblings {
            self.sibling(document.id, &mut |name| run(name));
        } else {
            run("");
        }
        result
    }

    #[inline(never)]
    fn prepare_one(
        &self,
        hosts: &HostServices,
        route: &dyn Route,
        document: &doc_service::Document,
    ) -> Result<(), i32> {
        let (target, kind) = match self.format_target(document.format) {
            Ok(found) => found,
            // The format left the index: it will not be there again for this
            // document, so the document fails (retrying would never end).
            Err(ERR_CANCELLED) => {
                self.fail_prepared(hosts, document, ERR_CANCELLED);
                return Ok(());
            }
            Err(status) => return Err(status),
        };
        let mut out = Vec::new();
        out.try_reserve_exact(REPLY_BYTES)
            .map_err(|_| ERR_NO_MEMORY)?;
        out.resize(REPLY_BYTES, 0);
        let op = if document.open_pending {
            fmt::OPEN
        } else {
            fmt::PREPARE
        };
        let length = match self.call_in_scope(hosts, route, document, (target, kind), op, &mut out)
        {
            Ok(length) => length,
            // A pending open's format that is not made yet: later, until it
            // gave up for want of memory, which fails the document (Senior,
            // R2 on #1365). One that cannot be made fails it too; one that
            // answered an error has it (the open reached it: it gets `close`
            // from now on).
            Err(ERR_BUSY) if document.open_pending && route.gave_up(target.plugin) => {
                self.fail_prepared(hosts, document, ERR_NO_MEMORY);
                return Ok(());
            }
            Err(status @ (ERR_BUSY | ERR_NO_MEMORY)) => return Err(status),
            Err(status) if document.open_pending => {
                if status != ERR_CANCELLED {
                    self.open_sent(document.id);
                }
                self.fail_prepared(hosts, document, status);
                return Ok(());
            }
            Err(status) => return Err(status),
        };
        let answered = if document.open_pending {
            self.open_sent(document.id);
            doc_service::pending::opened(&self.table, document, &out[..length])
        } else {
            doc_service::prepared(&self.table, document, &out[..length])
        };
        let (state, status, moved) = answered?;
        if moved {
            self.fresh.set(true);
        }
        if state == READY {
            self.document_ready(hosts, document.id);
        } else if state == FAILED && (status == ERR_NO_SPACE || status == ERR_TIMEOUT) {
            // The format's partial cache goes (the same for a preparation
            // that stalled, `document.md` 2.1).
            self.discard_cache(hosts, document.id);
        }
        Ok(())
    }
}
