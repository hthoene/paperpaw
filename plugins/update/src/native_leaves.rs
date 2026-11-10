//! The plugins built into the firmware running that an update leaves out, without
//! the host: the choice to update without them (interface 6's `Update::leaves_out`).
//!
//! Two sources say it. An update on the card is checked by the device, which
//! lists what the image leaves out (`core.device` op 16, `docs/abi/device.md`
//! 5.1; the plugin reads the list at once after the check, as it stays only
//! until the next one). An update the server named is not on the card yet: the
//! answer lists the crates it has built in (`plugins`), and what is built in
//! now and not among them is left out (`of_offer`). Which of them come back as
//! files (fetched builds) is not carried (`PORTING-7.md`), so none is filtered
//! out: a built-in plugin is never dropped without being named.
//!
//! The words are the plugins' names, which the registry knows (the host part is
//! `native_plugin/leaves.rs`).
use crate::{native_online::Offer, native_waiting::CRATE_PREFIX};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{Error, device::DeviceOutcome};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

/// Plugins named at most: a device holds far fewer built in.
pub const MAX_LEAVES: usize = 128;

/// Pages of the device's list read at most in one go.
pub const MOST_PAGES: usize = 16;

/// Times the read starts again at page 0 after a cursor of an earlier check
/// (`ERR_STATE`, `docs/abi/device.md` 5.1) before the file counts as unfit.
pub const MOST_RESTARTS: u8 = 2;

/// Times the check of one file is submitted again after its result was not there
/// yet or gone (`Busy`, `NotFound` of op 16) before the file counts as unfit.
pub const MOST_RESUBMITS: u8 = 3;

/// Whether the error of an op-16 read says that the result of the check is not
/// there for this plugin's request: a check is running (`ERR_BUSY`) or a newer
/// check replaced it (`ERR_NOT_FOUND`). Op 6 and op 16 answer only for the
/// plugin's most recent firmware check (`docs/abi/device.md` 5.1), so the old
/// request is never read again: the file's check is submitted again, after the
/// running check ended for `Busy` ([`Retry::Wait`]), at once for `NotFound`
/// ([`Retry::Now`]).
#[must_use]
pub const fn is_stale(error: Error) -> bool {
    matches!(error, Error::Busy | Error::NotFound)
}

/// What to do after a stale read ([`Recheck::stale`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    /// A check is running: wait for it to end, then submit the file's check
    /// again ([`Turn::Submit`]).
    Wait,
    /// No check runs: submit the file's check again now (no end comes).
    Now,
    /// Submitted [`MOST_RESUBMITS`] times: the file is unfit.
    Unfit,
}

/// What the scan does when a check ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// Not the file's check and nothing is waited for.
    Skip,
    /// The file's check ended and the device accepted it: read op 6, then op 16.
    Read,
    /// The running check that was waited for ended: submit the file's check
    /// again (`check_file` for the same file, a new check).
    Submit,
    /// The file's check was refused: the file cannot be installed.
    Unfit(DeviceOutcome),
}

/// The re-submits of the check of one file (a new one for each file).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Recheck {
    waiting: bool,
    submits: u8,
}

impl Recheck {
    /// The read of the file's result was stale ([`is_stale`], `error` is what
    /// op 16 answered). `ERR_BUSY` means a check is running: [`Retry::Wait`] for
    /// it to end and then submit the check again. `ERR_NOT_FOUND` means a newer
    /// check replaced the result and none runs (the plugin checks one file at a
    /// time), so no end would come: [`Retry::Now`], submit at once and wait for
    /// nothing. Both count as a re-submit; after [`MOST_RESUBMITS`] of them
    /// [`Retry::Unfit`].
    pub const fn stale(&mut self, error: Error) -> Retry {
        if self.submits >= MOST_RESUBMITS {
            return Retry::Unfit;
        }
        if matches!(error, Error::NotFound) {
            self.waiting = false;
            self.submits = self.submits.saturating_add(1);
            return Retry::Now;
        }
        self.waiting = true;
        Retry::Wait
    }

    /// A check ended: `ours` when it is the file's own, `accepted` when the
    /// device did not refuse it, `outcome` its verdict. A refusal of the
    /// re-submitted check is the same as of the first: the file is unfit.
    pub const fn ended(&mut self, ours: bool, accepted: bool, outcome: DeviceOutcome) -> Turn {
        if self.waiting {
            self.waiting = false;
            self.submits = self.submits.saturating_add(1);
            return Turn::Submit;
        }
        if !ours {
            return Turn::Skip;
        }
        if accepted {
            return Turn::Read;
        }
        Turn::Unfit(match outcome {
            DeviceOutcome::Ok => DeviceOutcome::Other,
            other => other,
        })
    }
}

/// Reads the device's list of what the checked image leaves out, page by page,
/// into the ids it returns. `read` gets the cursor of the page (`start` for
/// the first), appends the page's ids to the list it is given and answers the
/// cursor of the next page, or `None` at the end.
///
/// A cursor of an earlier check (`Error::State`) starts the read again at
/// `start` with the list empty, at most [`MOST_RESTARTS`] times. A check still
/// running (`Error::Busy`) or a result a newer check replaced
/// (`Error::NotFound`, [`is_stale`]) is not an answer: it is returned, each as
/// it is, for the caller to submit the file's check again ([`Recheck`]: after
/// the running check ended for `Busy`, at once for `NotFound`), never as a list (the image must not be offered without
/// knowing what it drops). Any other error is returned as it is: a foreign
/// image and the rest make the file unfit, where `Error::NoMemory` is retried
/// as before.
///
/// # Errors
/// What `read` answers, as above; [`Error::State`] once the restarts are spent.
pub fn read_all<C: Copy>(
    start: C,
    mut read: impl FnMut(C, &mut Vec<String>) -> Result<Option<C>, Error>,
) -> Result<Vec<String>, Error> {
    let mut ids: Vec<String> = Vec::new();
    let mut restarts = 0;
    let mut cursor = start;
    let mut pages = 0;
    while pages < MOST_PAGES {
        pages += 1;
        match read(cursor, &mut ids) {
            Ok(Some(next)) => cursor = next,
            Ok(None) => break,
            Err(Error::State) if restarts < MOST_RESTARTS => {
                restarts += 1;
                ids.clear();
                cursor = start;
                pages = 0;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(ids)
}

/// Why the server says a crate is missing from an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// It is not in the plugin library.
    Unknown,
    /// It is not built for the update yet.
    NotReady,
}

impl Why {
    /// The text that says it.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Unknown => "update.missing.unknown",
            Self::NotReady => "update.missing.not-ready",
        }
    }
}

/// The ids of the plugins built in now (`builtins`, a plugin's id each) that
/// the update `offer` has no build of: those whose crate (`paperpaw-plugin-<id>`)
/// is not among the crates the answer lists. An answer that lists none says
/// nothing of what the update has; the device's check of the file says it once
/// the update is on the card.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn of_offer<'a>(
    builtins: impl IntoIterator<Item = &'a str>,
    offer: &Offer,
) -> Result<Vec<String>, Error> {
    let mut crates = offer
        .plugins
        .lines()
        .filter_map(|line| line.trim().split_once(' ').map(|(name, _)| name))
        .peekable();
    if crates.peek().is_none() {
        return Ok(Vec::new());
    }
    let mut left = Vec::new();
    for id in builtins {
        let named = offer.plugins.lines().any(|line| {
            line.trim()
                .split_once(' ')
                .and_then(|(name, _)| name.strip_prefix(CRATE_PREFIX))
                == Some(id)
        });
        if !named && !left.iter().any(|kept: &String| kept == id) && left.len() < MAX_LEAVES {
            push(&mut left, copy(id)?)?;
        }
    }
    Ok(left)
}

/// What the answer says of why the plugin `id` is missing, if it says.
#[must_use]
pub fn why(offer: &Offer, id: &str) -> Option<Why> {
    offer.missing.lines().find_map(|line| {
        let (name, word) = line.split_once(' ')?;
        if name.strip_prefix(CRATE_PREFIX) != Some(id) {
            return None;
        }
        match word.trim() {
            "unknown" => Some(Why::Unknown),
            "not-ready" => Some(Why::NotReady),
            _ => None,
        }
    })
}

/// `name` followed by `reason` in brackets (`Stats (not ready yet)`).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn with_reason(name: &str, reason: &str) -> Result<String, Error> {
    concat(&[name, " (", reason, ")"])
}

#[cfg(test)]
#[path = "native_leaves_tests.rs"]
mod tests;
