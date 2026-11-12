//! The host's details job, the kernel's side (`docs/abi/library.md` 2.1): the
//! books of native format plugins are in the catalog from the walk on, but
//! the walk cannot read them (the kernel's stand-in format answers "not
//! ready", `native_book_formats`), so their lines name the file and have no
//! size. Once a walk has published, the host reads them itself, one book at a
//! time, through the format plugin (`doc_details_job`) and keeps what it read
//! (`library::keep_native_details`); the next walk makes their lines from it.
//!
//! **When.** Armed when a walk publishes or `core.catalog-changed` is heard
//! (`library_events`), disarmed when the catalog has no book left to read. A
//! book's job begins from the service pump, never in a callback, only once the
//! kernel's start has returned, when no background job of either kind runs (it
//! is the device's one slot, `doc_cover_job`), when no book is open for
//! reading or showing, and not in the quiet moment after a displacement: the
//! cover job's rules. Also never at the start's peak nor while the user is
//! busy (#2670): not before [`QUIET_MS`] after the start or any input, by a
//! timed wake. Nor beside another screen's or job's peak (#248): not while a
//! screen other than home's or the library's is on top (one opened for slot
//! `core.home` or `core.library`, or by its id where it is offered for one;
//! looked at again at the next input, also after a screen that closed on its
//! own: no timed wake), nor while network work
//! or a job of the install queue runs, nor while a first walk is under way
//! (its books published so far arm the job while it goes on); the last
//! three looked at again after 1 s, then 2, 4 … up to [`HOLD_WAIT_MAX_MS`]: a
//! walk cut short goes on only once a view of the library wants the
//! catalog, so its marks may stay on the card a long time. A reader's open takes its place at once (a displacement: the book
//! is looked at again later, and given up, as a failed book, after three in
//! a row); a cover that needs the slot makes it give way (`cover`: looked at
//! again later, counted as nothing); the device's sleep ends it, and nothing
//! starts one after the wake until a walk arms it again.
//!
//! **Which.** The catalog is read from a byte cursor, a line at a time (a 256
//! byte chunk and the SDK's line view on the stack, nothing kept): a book of a
//! native format's stand-in, of no size (its details unread), whose file is
//! there, and that did not fail in a job of this run or crash one before
//! (the library's ring of failed books). At most [`LINES_PER_PASS`] lines a
//! pass. The status log is not read (a removed book's file is gone, and so is
//! its job).
//!
//! **The catalog again.** Never once per book: when the catalog has no book
//! left, one `core.catalog-wanted` (made anew) where at least one book was
//! kept since the last; while it runs, one at most every two minutes, and also
//! only where something was kept. A walk that follows a pass that kept
//! nothing arms a pass that finds nothing and wants nothing: no loop.
//!
//! **Power.** Nothing here asks for another pass or holds the idle or sleep
//! timer off: the job's book prepares at the cover job's pace and under its
//! awake-time cap per book (`doc_pump`'s gap, `doc_cover_job`'s cap), and the
//! next look waits the same gap, a timed wake too ([`DetailsRun::wake_at`]),
//! or the end of the quiet moment, so that the job goes on on an idle device.
use super::{
    admitted_features::AdmittedFeatures, doc_service::HOST_JOB, doc_service::details::DetailsJob,
    host_services::Caller, lib_cover::path_key, native_declarations::NativeDeclarations,
    native_host_call::HostRoute, registry_service::Sources,
};
use crate::abi7::context::{Frame, registry_views::RegistryView};
use alloc::string::String;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_CANCELLED, ERR_DENIED, ERR_FULL, ERR_IO, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE,
    ERR_UNSUPPORTED,
};
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sdk::builtin::{events, slots};
use paperpaw_sdk::library::{self, catalog};

/// The most catalog lines one pass looks at.
const LINES_PER_PASS: usize = 32;
/// The bytes read at once when a line's end is looked for.
const CHUNK: usize = 256;
/// How long the device is left alone after the start or an input before a
/// book's job begins: an EPUB's open (its zip and XML, some 30 KiB) is never
/// added to the start's peak or to a screen the user works in (#2670).
pub(crate) const QUIET_MS: u64 = 1_000;
/// The longest wait between looks while the job is held (a first walk, a
/// network or install job).
pub(crate) const HOLD_WAIT_MAX_MS: u64 = 32_000;

/// The pump's frame for the details job's open: the kernel's settings,
/// declarations and registry, no callback (the format is called through the
/// pump's route, never through a frame).
struct PumpFrame<'a> {
    env: &'a paperpaw_sdk::Env<'a>,
    sources: Sources<'a>,
    directory: Option<&'a NativeDeclarations>,
    log: super::device_log::DeviceLog,
}

impl Frame for PumpFrame<'_> {
    fn settings(&self) -> &paperpaw_sdk::Settings {
        self.env.settings()
    }
    fn translator(&self) -> &paperpaw_i18n::Translator {
        self.env.i18n()
    }
    fn kernel_env(&self) -> Option<&paperpaw_sdk::Env<'_>> {
        Some(self.env)
    }
    fn storage(&self) -> Option<&dyn Storage> {
        Some(self.env.storage())
    }
    fn log(&self, line: core::fmt::Arguments<'_>) {
        self.log.line(line);
    }
    fn registry(&self) -> Result<&dyn RegistryView, i32> {
        Ok(&self.sources)
    }
    fn with_declarations(&self, visit: &mut dyn FnMut(&dyn core::any::Any)) -> Result<(), i32> {
        let directory = self.directory.ok_or(ERR_UNSUPPORTED)?;
        let store = directory.store.try_borrow().map_err(|_| ERR_BUSY)?;
        visit(&*store);
        Ok(())
    }
}

/// What a look at the catalog found.
enum Look {
    /// A book to read: its path, where its line begins and the next one.
    Book { path: String, line: u64, next: u64 },
    /// None in this pass's lines: the next look begins here.
    More(u64),
    /// None left.
    End,
}

/// What became of a pass.
enum Pass {
    Nothing,
    /// Nothing now; the next look at `at` (a timed wake).
    Wait(u64),
    End,
    Started,
    /// The open of the book at `line` (the next at `next`, its key `key`)
    /// answered `status`.
    Refused {
        status: i32,
        line: u64,
        next: u64,
        key: u64,
    },
}

/// The catalog's file, reserved fallibly.
fn catalog_file() -> Result<String, i32> {
    const NAME: &str = "/catalog.txt";
    let mut file = String::new();
    file.try_reserve_exact(library::DIR.len() + NAME.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    file.push_str(library::DIR);
    file.push_str(NAME);
    Ok(file)
}

/// Where the line after the one at `at` begins (the end of the file for a
/// last line without its newline); `None` where `at` is the end.
fn line_end(storage: &dyn Storage, file: &str, at: u64) -> Result<Option<u64>, i32> {
    let mut chunk = [0_u8; CHUNK];
    let mut offset = at;
    loop {
        let n = match storage.read_at(file, offset, &mut chunk) {
            Ok(n) => n,
            Err(StorageError::NotFound) => return Ok(None),
            Err(error) => return Err(super::storage::status(error)),
        };
        if n == 0 {
            return Ok((offset > at).then_some(offset));
        }
        if let Some(end) = chunk[..n].iter().position(|&byte| byte == b'\n') {
            return Ok(Some(offset + end as u64 + 1));
        }
        offset += n as u64;
    }
}

/// `at`, or the start of the next line where `at` is inside one (the catalog
/// was made again since the cursor was kept).
fn line_start(storage: &dyn Storage, file: &str, at: u64) -> Result<u64, i32> {
    if at == 0 {
        return Ok(0);
    }
    let mut before = [0_u8; 1];
    match storage.read_at(file, at - 1, &mut before) {
        Ok(1) if before[0] == b'\n' => Ok(at),
        Ok(_) => Ok(line_end(storage, file, at)?.unwrap_or(at)),
        Err(StorageError::NotFound) => Ok(at),
        Err(error) => Err(super::storage::status(error)),
    }
}

/// The next book to read from `from` (see the module): `wanted` says whether
/// a book of no size is one.
fn look(storage: &dyn Storage, from: u64, wanted: &dyn Fn(&str) -> bool) -> Result<Look, i32> {
    let file = catalog_file()?;
    // The status log is not read: a removed book's file is gone.
    let changes = catalog::Changes::default();
    let mut at = line_start(storage, &file, from)?;
    for _ in 0..LINES_PER_PASS {
        let Some(next) = line_end(storage, &file, at)? else {
            return Ok(Look::End);
        };
        let mut found = None;
        catalog::try_view_at_with(storage, &changes, at, &mut |view| {
            if view.size.is_none() && wanted(view.path) && storage.exists(view.path) {
                found = Some(super::lib_cover::owned(view.path));
            }
        })
        .map_err(super::lib_reads::fail)?;
        if let Some(path) = found {
            return Ok(Look::Book {
                path: path?,
                line: at,
                next,
            });
        }
        at = next;
    }
    Ok(Look::More(at))
}

impl crate::Kernel {
    /// The details job begins on the next book that needs one, where it may
    /// (see the module). Never asks for another pass.
    #[inline(never)]
    pub(crate) fn native_details_job_pump(&mut self) {
        let pass = self.native_details_pass();
        let wake = match pass {
            Pass::Nothing => None,
            Pass::Wait(at) => Some(at),
            Pass::End => {
                self.native_details_wanted(true);
                None
            }
            Pass::Started => {
                // What the format staged runs now; a book that is ready at
                // once is read in this pass. Soon the next look: the first
                // step of a book that prepares (this pass's preparing loop
                // came before it), or the next book.
                self.native_outbox_run();
                self.native_cover_job_pump();
                Some(self.now.millis().saturating_add(self.native_job_gap()))
            }
            Pass::Refused {
                status,
                line,
                next,
                key,
            } => {
                self.native_outbox_run();
                Some(self.native_details_refused(status, line, next, key))
            }
        };
        if let Some(at) = wake
            && let Some(job) = self.native_details_job()
        {
            job.run().wake_at(at);
        }
    }

    /// The details job, where the documents have one.
    fn native_details_job(&self) -> Option<&dyn DetailsJob> {
        self.shared
            .native_instances
            .hosts()?
            .documents()?
            .0
            .details_job()
    }

    /// The next look's timed wake, where one waits (`service_due`).
    pub(crate) fn native_details_due(&self) -> Option<u64> {
        self.native_details_job()?.run().wake()
    }

    /// One pass's look: the book whose job begins, if any.
    // Its own frame (as the refusal's): inlined, its early answers were
    // threaded into the pump's `match` as jumps the firmware's frame scan
    // cannot size (`frame-budget-ppn5.txt`).
    #[inline(never)]
    fn native_details_pass(&self) -> Pass {
        let instances = &self.shared.native_instances;
        // Never inside a callback, nor while something staged waits.
        if instances.depth() != 0 || instances.outbox.full() {
            return Pass::Nothing;
        }
        let Some(hosts) = instances.hosts() else {
            return Pass::Nothing;
        };
        let (Some(documents), Some(library)) = (hosts.documents(), hosts.library()) else {
            return Pass::Nothing;
        };
        let Some(job) = documents.0.details_job() else {
            return Pass::Nothing;
        };
        let run = job.run();
        let active = documents.0.cover_job_active();
        // A job that left the table without an end (an open took its place).
        if run.running() && !active {
            run.gone();
        }
        if !run.armed() || active || !run.due(self.now.millis()) {
            return Pass::Nothing;
        }
        match job.may_begin() {
            Ok(()) => {}
            Err(Some(at)) => return Pass::Wait(at),
            Err(None) => return Pass::Nothing,
        }
        let now = self.now.millis();
        let quiet = self.last_activity.millis().saturating_add(QUIET_MS);
        if now < quiet {
            return Pass::Wait(quiet);
        }
        let natives = &self.shared.registry.native_book_formats;
        if natives.is_empty() {
            run.disarm();
            return Pass::Nothing;
        }
        if !self.details_browsing() {
            return Pass::Nothing;
        }
        let env = self.shared.env(self.now);
        if self.details_busy() || catalog::is_growing(env.storage()) {
            let wait = run.hold_waits(QUIET_MS, HOLD_WAIT_MAX_MS);
            return Pass::Wait(now.saturating_add(wait));
        }
        run.hold_ended();
        let formats = &self.shared.formats;
        let wanted = |path: &str| {
            formats
                .for_path(path)
                .is_some_and(|spec| natives.has(spec.id.as_str()))
                && !library.0.book_failed(path_key(path))
        };
        let (path, line, next) = match look(env.storage(), run.cursor(), &wanted) {
            Ok(Look::Book { path, line, next }) => (path, line, next),
            Ok(Look::More(at)) => {
                run.set_cursor(at);
                return Pass::Wait(self.now.millis().saturating_add(self.native_job_gap()));
            }
            Ok(Look::End) => {
                run.disarm();
                return Pass::End;
            }
            // The card or the memory: a later pass looks again (no timed
            // wake, a card that fails is not tried in a loop).
            Err(_) => return Pass::Nothing,
        };
        let directory = self.shared.registry.native_declarations.as_deref();
        let frame = PumpFrame {
            env: &env,
            sources: Sources {
                env: &env,
                directory,
                generation: &self.shared.registry_generation,
            },
            directory,
            log: self.shared.device_log,
        };
        let who = Caller {
            owner: HOST_JOB,
            plugin: "",
            features: AdmittedFeatures::NONE,
            hosts,
        };
        let route = HostRoute {
            registry: instances,
            env: &env,
            meter: &self.shared.meter,
        };
        match job.start(&frame, &who, &route, &path) {
            Ok(()) => {
                run.started(line, next);
                Pass::Started
            }
            Err(status) => Pass::Refused {
                status,
                line,
                next,
                key: path_key(&path),
            },
        }
    }

    /// Whether the screen on top is home's or the library's: opened for slot
    /// `core.home` or `core.library`, or opened by its id where it is offered
    /// for one of them (see the module). `false` with no screen.
    fn details_browsing(&self) -> bool {
        let Some(top) = self.stack.last() else {
            return false;
        };
        let browsing = [slots::HOME, slots::LIBRARY];
        match &top.slot {
            Some(slot) => browsing.contains(slot),
            None => browsing.iter().any(|slot| {
                self.shared
                    .registry
                    .slot_screens(slot)
                    .any(|screen| *screen == top.id)
            }),
        }
    }

    /// Whether network work (a request, a server, a socket, work that waits
    /// for a network, a native service's) or a job of the install queue (an
    /// install, a check, a removal, firmware) runs or waits.
    fn details_busy(&self) -> bool {
        let network = self.io.http.busy()
            || self.io.servers.busy()
            || self.io.sockets.any()
            || self.network_work_waiting()
            || self
                .shared
                .native_instances
                .net_demand()
                .is_some_and(|(pool, _)| pool.wants() || pool.live());
        // A resting cell has no driver and so nothing to run: the driver
        // rests only at rest (`FirmwareCell::rest`, `Driver::at_rest`: no
        // queue, nothing running). A driver lent out counts as busy.
        let installing = self.firmware.as_ref().is_some_and(|cell| {
            cell.driver().is_some_and(|driver| {
                driver
                    .try_borrow()
                    .map_or(true, |driver| driver.waiting() > 0)
            })
        });
        network || installing
    }

    /// The open of a book answered `status`: tried again later where nothing
    /// was wrong with the book (after the preparing loop's wait for a step
    /// that could not be made, `doc_pump`), else passed by (and remembered as
    /// failed where the format failed it). When the next look is due.
    #[inline(never)]
    fn native_details_refused(&self, status: i32, line: u64, next: u64, key: u64) -> u64 {
        let soon = self.now.millis().saturating_add(self.native_job_gap());
        let Some(run) = self.native_details_job().map(DetailsJob::run) else {
            return soon;
        };
        match status {
            // The format not started yet, no place, no memory: again later.
            ERR_BUSY | ERR_FULL | ERR_NO_MEMORY => {
                let tries = run.later(line, next);
                return self
                    .now
                    .millis()
                    .saturating_add(super::doc_pump::wait(tries));
            }
            // No fault of the book (its file, the card, a format that left).
            ERR_NOT_FOUND | ERR_IO | ERR_CANCELLED | ERR_STATE | ERR_DENIED | ERR_UNSUPPORTED => {
                run.set_cursor(next);
            }
            // The format failed the book.
            _ => {
                run.set_cursor(next);
                if let Some(library) = self
                    .shared
                    .native_instances
                    .hosts()
                    .and_then(|h| h.library())
                {
                    library.0.cover_job_left(key);
                }
            }
        }
        soon
    }

    /// The details job ended (`cover_job_pump`): `filled` where its book's
    /// details were kept, `key` its book's where the format failed it.
    pub(crate) fn native_details_job_ended(&mut self, filled: bool, key: u64) {
        let Some(hosts) = self.shared.native_instances.hosts() else {
            return;
        };
        if key != 0
            && let Some(library) = hosts.library()
        {
            library.0.cover_job_left(key);
        }
        if let Some(run) = hosts
            .documents()
            .and_then(|documents| documents.0.details_job())
            .map(DetailsJob::run)
        {
            run.ended();
            if filled {
                run.filled();
            }
        }
        self.native_details_wanted(false);
    }

    /// `core.catalog-wanted` (made anew) where the run says it is due: the
    /// catalog's `end`, or two minutes since the last (see the module).
    fn native_details_wanted(&mut self, end: bool) {
        let now = self.now.millis();
        let due = self
            .shared
            .native_instances
            .hosts()
            .and_then(|hosts| hosts.documents())
            .and_then(|documents| documents.0.details_job())
            .is_some_and(|job| job.run().wanted(now, end));
        if due {
            self.output
                .commands
                .push(events::CATALOG_WANTED.emit(events::CatalogWanted::new(true)));
        }
    }

    /// A walk published, or the catalog changed: the details job looks at the
    /// catalog from its start (`library_events`).
    pub(crate) fn native_details_arm(&self) {
        if let Some(job) = self
            .shared
            .native_instances
            .hosts()
            .and_then(|hosts| hosts.documents())
            .and_then(|documents| documents.0.details_job())
        {
            job.run().arm(self.now.millis());
        }
    }
}

#[cfg(test)]
#[path = "details_job_pump_tests.rs"]
mod tests;
