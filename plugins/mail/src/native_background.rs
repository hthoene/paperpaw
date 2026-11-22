//! The background check for mail (owner decision 52): the setting *Check for
//! new mail in the background* declares work (`core.work`) that comes due
//! every 15, 30 or 60 minutes; a due check runs on the host's background
//! work, one account after the other, one connection at a time, with the
//! calls the screens' check makes (folders and the inbox of each account),
//! and puts what it finds on the card, where the home widgets read it.
//!
//! The socket of a check belongs to the work run it was opened in (the host
//! ends it with the run, `docs/abi/sockets.md` section 1), also the socket of
//! the next account, opened in the callback of the last one's end. A run is
//! quiet: nothing here shows, asks or opens (the host answers `ERR_STATE` to a
//! command that would, `docs/abi/work.md` section 2.3); the unread count in the
//! widgets is told with `widget_changed`. A reader who opens Mail while a
//! check goes on is served first: the check is let go.
use crate::declarations::WORK_TOKEN;
use crate::native_model::{
    account::Values,
    background::{self, Round},
    browser::{self, Mode},
    job::Run,
};
use crate::native_plugin::NativeMail;
use crate::native_screens::fail_error;
use crate::native_values::{SettingsValues, switch};
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host, snapshot,
    work::{Work, WorkSpec},
};
use paperpaw_ui_kit::mem::{buffer, push};

/// The setting that turns the check on.
const ON: &str = "mail.background";
/// The setting that says how often.
const EVERY: &str = "mail.background-every";

impl NativeMail {
    /// Whether the reader turned the background check on.
    fn background_on(host: &Host<'_>) -> Result<bool, Error> {
        switch(host, ON, false)
    }

    /// Declares the work as the settings say (work lives for one start, so
    /// this is done at the start and when a setting changes), or takes it back
    /// when the check is off.
    ///
    /// # Errors
    /// The host's refusals: `Denied` where the plugin's network is turned off,
    /// `Full` and the like; `NoMemory`.
    pub(crate) fn declare_background(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let on = Self::background_on(host)?;
        let choice = SettingsValues(host).text(EVERY)?;
        if self.work.is_none() {
            self.work = Some(Work::open(host)?);
        }
        let (mut input, mut out) = (buffer(256)?, buffer(64)?);
        if !on {
            // A check that goes on is let go with its socket.
            if self.background.take().is_some() {
                self.cancel(host);
            }
            let work = self.work.as_ref().ok_or(Error::State)?;
            return work.cancel(host, background::KEY, &mut input, &mut out);
        }
        // Network work by time (no moment, no deadline: due when the time is
        // there, at the same distance each time; the host asks for the radio
        // if none is up, at most once in 15 minutes for all plugins), repeated
        // as long as the setting stays on.
        let spec = WorkSpec::new(background::KEY, WORK_TOKEN)?
            .network()
            .after_ms(background::FIRST_MS)
            .every_ms(background::every_ms(&choice));
        let work = self.work.as_ref().ok_or(Error::State)?;
        work.declare(host, &spec, &mut input, &mut out)
    }

    /// The work is due: a round over every account that is set up begins,
    /// unless the reader's own check is on.
    pub(crate) fn background_due(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.job.working() || self.background.is_some() || !Self::background_on(host)? {
            return Ok(());
        }
        let mut numbers: Vec<u8> = Vec::new();
        for account in self.accounts(host)? {
            push(&mut numbers, account.number)?;
        }
        self.background = Some(Round::new(&numbers)?);
        self.background_next(host)
    }

    /// Starts the check of the account at the front of the round. Whatever
    /// keeps it from starting ends the round (quietly, unless memory ran out):
    /// the next time is the work's.
    fn background_next(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let started = self.start_front(host);
        if !matches!(started, Ok(true)) {
            self.background = None;
        }
        started.map(|_| ())
    }

    /// Whether the check of an account of the round started; accounts that
    /// are not set up (any more) are passed by.
    fn start_front(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        while let Some(number) = self.background.as_ref().and_then(Round::front) {
            let fetch = self.fetch_count(host)?;
            let now = snapshot::capture_snapshot(host)?
                .unix_seconds()
                .unwrap_or(0);
            let tasks = browser::tasks(&Mode::Boxes, number, fetch, now).map_err(fail_error)?;
            match self.start_imap(host, number, tasks) {
                Ok(()) => return Ok(true),
                Err(Error::NotFound) => {
                    if let Some(round) = &mut self.background {
                        round.pass();
                    }
                }
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => return Ok(false),
            }
        }
        Ok(false)
    }

    /// The connection of the round moved on: once it ended, the next account
    /// (one that failed or whose server is not trusted holds up no other; a
    /// pinned key is asked about only on the screens).
    pub(crate) fn background_progressed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(round) = &mut self.background else {
            return Ok(());
        };
        if !round.checked(&self.job.run) {
            return Ok(());
        }
        self.job.run = Run::Idle;
        self.background_next(host)
    }

    /// A screen of Mail opens: the reader is served first, the check is let go
    /// (its socket closed, its connection reset).
    pub(crate) fn background_yields(&mut self, host: &Host<'_>) {
        if self.background.take().is_some() {
            self.cancel(host);
        }
    }
}
