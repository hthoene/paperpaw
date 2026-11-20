//! A format that asks before it reads a book (#242, `doc_service::asked`):
//! the host's side of the question (op 1 with `claimed`, root 3 the book),
//! the next format of the book's extension, and the pump's step for an
//! open whose format plugin was not made when it was to be asked. Also what
//! the cache of an open needs of the registry and the opener.
use super::{Bound, DocState, NoImages, copy, wire};
use crate::abi7::context::Frame;
use crate::native_host::{
    admitted_features::AdmittedFeatures,
    doc_service::{self, Document, FormatCall, FormatRef, HOST_JOB, PrepareStep},
    fmt_call::{Route, Stage},
    host_services::{Caller, HostServices},
};
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND};

impl DocState {
    /// The format that reads `document`'s book after those of `passed`
    /// (`FormatIndex::next_claimant`), by the book's extension.
    pub(super) fn next_after(&self, document: u32, passed: &[FormatRef]) -> Option<FormatRef> {
        let books = self.books.try_borrow().ok()?;
        let book = books.iter().find(|book| book.document == document)?;
        self.index().ok()?.next_claimant(&book.path, passed)
    }

    /// The pump's step for the first document whose format is still to be
    /// asked (`Document::asking`), at depth 0: `More` where it moved, `Later`
    /// while its plugin is still not made, `Idle` where none waits.
    pub(super) fn ask_waiting(
        &self,
        frame: &dyn Frame,
        hosts: &HostServices,
        route: &dyn Route,
    ) -> PrepareStep {
        let Some(document) = self.table.next_asking() else {
            return PrepareStep::Idle;
        };
        let who = Caller {
            owner: HOST_JOB,
            plugin: "",
            features: AdmittedFeatures::NONE,
            hosts,
        };
        let bound = Bound {
            state: self,
            frame,
            who: &who,
            route: Some(route),
        };
        match bound.step_asking(&document, route) {
            Ok(true) => PrepareStep::More,
            Ok(false) => PrepareStep::Later,
            Err(status) => {
                self.fail_prepared(hosts, &document, status);
                PrepareStep::More
            }
        }
    }
}

impl Bound<'_> {
    /// Op 1 with `claimed` to `format` for `document`, its roots those of
    /// the document (3 the book).
    pub(super) fn ask(&self, document: u32, format: FormatRef) -> Result<bool, i32> {
        let (target, kind) = self.state.format_target(format)?;
        let _scope = self
            .state
            .enter_scope(self.who.hosts, target.plugin, document, kind, "")?;
        let frame_route = super::FrameRoute(self.frame);
        let stage = Stage {
            route: self.route.unwrap_or(&frame_route),
            images: &NoImages,
        };
        let mut reply = [0_u8; 64];
        stage.ask(target, document, &mut reply)
    }

    /// One step of [`DocState::ask_waiting`] for `document`: `Ok(false)`
    /// while the plugin to ask is not made yet.
    fn step_asking(&self, document: &Document, route: &dyn Route) -> Result<bool, i32> {
        match self.ask(document.id, document.format) {
            Ok(true) => {
                // Its cache was made when the open came: the preparing loop
                // sends the open.
                self.state.table.set_asking(document.id, false)?;
                return Ok(true);
            }
            Err(ERR_BUSY) => {
                let (target, _) = self.state.format_target(document.format)?;
                if route.gave_up(target.plugin) {
                    return Err(ERR_NO_MEMORY);
                }
                return Ok(false);
            }
            Ok(false) | Err(_) => {}
        }
        // Passed on: its cache goes, the next format's is made with the
        // open's batch and CSS file as they were kept.
        let (batch, css) = {
            let books = self.state.books.try_borrow().map_err(|_| ERR_BUSY)?;
            let book = books
                .iter()
                .find(|book| book.document == document.id)
                .ok_or(ERR_NOT_FOUND)?;
            let mut batch = Vec::new();
            batch
                .try_reserve_exact(book.open.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            batch.extend_from_slice(&book.open);
            let css = book.cache.as_ref().map_or("", |cache| &cache.css);
            (batch, copy(css)?)
        };
        self.discard_cache(document.id);
        doc_service::asked::passed_on(self, document, &batch, &css)?;
        Ok(true)
    }

    /// The version of the plugin `plugin` as the registry says it (a new
    /// version of a format makes its caches stale).
    pub(super) fn format_version(&self, plugin: &str) -> Result<String, i32> {
        let asker = crate::native_host::registry_service::Asker {
            owner: self.who.owner,
            plugin: self.who.plugin,
        };
        // The row is encoded into a buffer of the heap, not of the opener's
        // stack (a frame on the X4 is small), and freed with this call.
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(1024).map_err(|_| ERR_NO_MEMORY)?;
        bytes.resize(1024, 0);
        let mut version = Err(ERR_NOT_FOUND);
        self.frame
            .registry()?
            .detail(6, plugin, &asker, &mut |row| {
                if version.is_ok() {
                    return Ok(());
                }
                let used = row.encode(&mut bytes)?;
                // One bare record, exactly (the frame is the whole encoding).
                let (raw, length) = wire::RawRecord::prefix(&bytes[..used])?;
                if length != used {
                    return Err(ERR_INVALID);
                }
                if raw.tag() == wire::SERVICE_REG_PLUGIN_TAG {
                    let row: wire::ServiceRegPlugin<'_> = raw.decode()?;
                    version = copy(row.version);
                }
                Ok(())
            })?;
        version
    }

    /// The card path of the user's CSS file of an open (`""`: none, or a call
    /// that does not read it), root 4 of the format's call.
    pub(super) fn css_card_path(&self, call: &FormatCall<'_>) -> Result<String, i32> {
        if call.op != crate::native_host::doc_service::fmt::OPEN || call.css_path.is_empty() {
            return Ok(String::new());
        }
        if call.css_root == 1 {
            let path = call.css_path.trim_start_matches('/');
            let mut copy = String::new();
            copy.try_reserve_exact(path.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            copy.push_str(path);
            return Ok(copy);
        }
        crate::native_host::owner_paths::OwnerPaths::join_for(
            self.who,
            call.css_root,
            call.css_path,
        )
    }
}
