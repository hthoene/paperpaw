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
use paperpaw_sdk::native::Error;
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

/// Reads the device's list of what the checked image leaves out, page by page,
/// into the ids it returns. `read` gets the cursor of the page (`start` for
/// the first), appends the page's ids to the list it is given and answers the
/// cursor of the next page, or `None` at the end.
///
/// A cursor of an earlier check (`Error::State`) starts the read again at
/// `start` with the list empty, at most [`MOST_RESTARTS`] times. A check still
/// running (`Error::Busy`) is not an answer: it is returned for the caller to
/// ask again once the check has ended, never as a list (the image must not be
/// offered without knowing what it drops). Any other error is returned as it
/// is: no finished check, a foreign image and the rest make the file unfit,
/// where `Error::NoMemory` is retried as before.
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
