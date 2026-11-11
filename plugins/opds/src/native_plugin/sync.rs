//! The sync of the catalogs marked for it (the engine is `native_sync`): from
//! the list's *Sync catalogs* row or a catalog's options (now), from the action
//! `opds.sync` (as declared work, at a good moment, as a rule's background
//! action asks for it), and the line that tells how it goes.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::{Error, Host, formatting::Arg, net::HttpDone, work::WorkSpec};
use paperpaw_ui_kit::{
    Messages,
    mem::{buffer, push},
};

use super::Opds;
use crate::native_catalogs as catalogs;
use crate::native_files::DataFiles;
use crate::native_sync::{Job, Report, Setup, Update};
use crate::native_web::{SYNC_BOOK, SYNC_FEED};

/// The key of the sync declared as work.
pub(super) const SYNC_WORK: &str = "opds.sync";
/// How long declared work may wait for a good moment: an hour, as in
/// interface 6.
const WITHIN_MS: u32 = 3_600_000;
/// Books a catalog gets at most in one sync, where the setting says nothing.
const DEFAULT_LIMIT: i64 = 50;

impl Opds {
    /// Whether a sync is going on.
    pub(super) fn sync_running(&self) -> bool {
        self.sync.running()
    }

    /// The jobs of `catalogs` (all those marked, or the one at `only`), each
    /// with its password.
    fn sync_jobs(&mut self, host: &Host<'_>, only: Option<usize>) -> Result<Vec<Job>, Error> {
        self.reload(host)?;
        let mut jobs = Vec::new();
        let vault = self.vault(host);
        for (at, catalog) in self.catalogs.iter().enumerate() {
            let wanted = match only {
                Some(only) => only == at,
                None => catalog.sync,
            };
            if wanted {
                let password = catalogs::password(&vault, catalog)?;
                push(&mut jobs, Job::new(catalog.try_clone()?, password))?;
            }
        }
        Ok(jobs)
    }

    /// Starts a sync of `only` (a catalog of the list) or of every catalog
    /// marked for it, unless one is going on.
    fn sync_begin(&mut self, host: &Host<'_>, only: Option<usize>) -> Result<(), Error> {
        if self.sync.running() {
            return Ok(());
        }
        if !self.web.available(host) {
            self.list_note = Some(Messages::new()?.copy(host, "opds.no-network")?);
            return self.refresh(host);
        }
        let jobs = self.sync_jobs(host, only)?;
        let limit = Self::number_setting(host, "opds.sync-limit", DEFAULT_LIMIT).max(1);
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        let folder = crate::native_files::DataFiles::new(host);
        let setup = Setup::new(&self.folder, &self.extensions, self.small, limit);
        let update = self
            .sync
            .start(host, &mut self.web, &folder, &setup, jobs)?;
        self.list_note = None;
        self.synced(host, update)
    }

    /// *Sync catalogs*: stops a sync that is going on, else starts one.
    pub(super) fn sync_row(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.sync.running() {
            let files = DataFiles::new(host);
            self.sync.stop(host, &mut self.web, &files);
            return self.refresh(host);
        }
        self.sync_begin(host, None)
    }

    /// *Sync now* in a catalog's options.
    pub(super) fn sync_one(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        self.sync_begin(host, Some(index))
    }

    /// The action `opds.sync`. The reader's own run (a menu, a key) syncs at
    /// once; a background run (a rule's, another plugin's) declares the sync as
    /// work, due at a good moment within the hour. Where there is no work
    /// service it runs at once.
    pub(super) fn sync_action(&mut self, host: &Host<'_>, background: bool) -> Result<(), Error> {
        self.open(host)?;
        if !background {
            return self.sync_begin(host, None);
        }
        let Some(work) = &self.work else {
            return self.sync_begin(host, None);
        };
        let spec =
            WorkSpec::when_cheap(SYNC_WORK, crate::declarations::WORK_TOKEN)?.within_ms(WITHIN_MS);
        let (mut input, mut out) = (buffer(256)?, buffer(64)?);
        match work.declare(host, &spec, &mut input, &mut out) {
            Ok(()) => Ok(()),
            // No room for it, no right to the network, no such work here: the
            // sync runs when the reader asks for it.
            Err(Error::Full | Error::Denied | Error::Unsupported | Error::State) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Declared work came due.
    pub(super) fn work_due(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        if key != SYNC_WORK {
            return Ok(());
        }
        self.open(host)?;
        self.sync_begin(host, None)
    }

    /// The end of a request of the sync.
    pub(super) fn sync_done(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        if !matches!(done.token(), SYNC_FEED | SYNC_BOOK) {
            return Ok(());
        }
        let files = DataFiles::new(host);
        let limit = Self::number_setting(host, "opds.sync-limit", DEFAULT_LIMIT).max(1);
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        let setup = Setup::new(&self.folder, &self.extensions, self.small, limit);
        let update = self.sync.done(
            host,
            &mut self.web,
            &files,
            self.library.as_ref(),
            &setup,
            done,
        );
        match update {
            Ok(update) => self.synced(host, update),
            Err(Error::NoMemory) => {
                // The sync ends with a line of its own.
                self.list_note = Some(Messages::new()?.copy(host, "opds.no-memory")?);
                self.refresh(host)
            }
            Err(error) => Err(error),
        }
    }

    /// A step of the sync: the list shows it, and its end is noticed.
    fn synced(&mut self, host: &Host<'_>, update: Option<Update>) -> Result<(), Error> {
        let Some(update) = update else {
            return Ok(());
        };
        if update == Update::Finished {
            let mut say = Messages::new()?;
            if let Some(text) = self.sync_report_text(host, &mut say)? {
                Self::notice(host, &text)?;
            }
        }
        self.refresh(host)
    }

    /// How a sync goes or went, as the line says it.
    pub(super) fn sync_report_text(
        &self,
        host: &Host<'_>,
        say: &mut Messages,
    ) -> Result<Option<String>, Error> {
        match self.sync.report() {
            Some(report) => report_text(host, say, report).map(Some),
            None => Ok(None),
        }
    }

    /// Stops the sync (the plugin stops, or its files go).
    pub(super) fn stop_sync(&mut self, host: &Host<'_>) {
        let files = DataFiles::new(host);
        self.sync.stop(host, &mut self.web, &files);
    }
}

/// How a sync goes or went.
fn report_text(host: &Host<'_>, say: &mut Messages, report: &Report) -> Result<String, Error> {
    let loaded = i64::from(report.loaded);
    let mut text = if report.running {
        say.format(
            host,
            "opds.syncing",
            &[
                Arg::Text {
                    name: "catalog",
                    value: &report.catalog,
                },
                Arg::Count(loaded),
            ],
        )?
    } else {
        say.format(host, "opds.synced", &[Arg::Count(loaded)])?
    };
    for (key, count) in [
        ("opds.failed", report.failed),
        ("opds.only-lists", report.only_lists),
        ("opds.unreachable", report.unreachable),
        ("opds.skipped", report.skipped),
    ] {
        if count > 0 {
            let more = say.format(host, key, &[Arg::Count(i64::from(count))])?;
            text.try_reserve(more.len() + 1)
                .map_err(|_| Error::NoMemory)?;
            text.push(' ');
            text.push_str(&more);
        }
    }
    Ok(text)
}
