//! An update that waits for plugins, without the host: the update server named
//! a newer PaperPaw, but code plugins in use (installed, switched on, of the
//! version asked about) have no build for it yet (the answer's `waiting`), or
//! plugins built in now are not built for it yet (`missing … not-ready`). A
//! search in the background does not offer it; it is kept, shown on the update
//! page, and asked for again once a day at a cheap moment, until it no longer
//! waits. If the reader does not open the update page within a week, the page
//! opens once, at a start, and never again for that version.
//!
//! `waiting.txt` keeps it across starts (interface 6's file, small, version 1,
//! `docs/data-files.md`): the version, then `since <unix seconds>` (when it
//! began to wait, `-` while the clock is not known), `checked <unix seconds>`
//! (the last question asked, answered or not), lines `asked` (asked once while
//! the clock was not known), `seen` (the reader looked) and `told` (the page
//! opened by itself), and what it waits for: `plugin <id> <version>` and
//! `crate <name>` lines. The host's part (the registry, the work, the page) is
//! in `native_plugin/waiting.rs`.
use crate::{native_online::Offer, native_update::is_version};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{firmware::compare_versions, native::Error};
use paperpaw_ui_kit::{
    mem::push,
    text::{copy, write_to},
};

/// The data file.
pub const FILE: &str = "waiting.txt";
/// The most `waiting.txt` is read with: far more than its lines for every
/// plugin a device holds ([`MAX_ENTRIES`] each).
pub const MAX_LEN: usize = 32 * 1024;
/// Plugins and crates kept at most, each.
pub const MAX_ENTRIES: usize = 200;
/// How long an update waits before the page opens by itself.
pub const WEEK_SECONDS: i64 = 7 * 24 * 3600;
/// How often the server is asked again while an update waits.
pub const DAY_SECONDS: i64 = 24 * 3600;
/// The prefix of a first-party plugin's crate.
pub const CRATE_PREFIX: &str = "paperpaw-plugin-";

/// What an update waits for: code plugins (id, version) and crates built in now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Holding {
    pub plugins: Vec<(String, String)>,
    pub crates: Vec<String>,
}

impl Holding {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty() && self.crates.is_empty()
    }

    /// What the answer `offer` says an update waits for: the code plugins of the
    /// question it has no build for (`waiting`) and the crates it says are not
    /// ready (`missing … not-ready`). Not yet checked against what is in use.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn of_offer(offer: &Offer) -> Result<Self, Error> {
        let mut holding = Self::default();
        for line in offer.waiting.lines() {
            if holding.plugins.len() >= MAX_ENTRIES {
                break;
            }
            if let Some((id, version)) = line.trim().split_once(' ') {
                push(&mut holding.plugins, (copy(id)?, copy(version.trim())?))?;
            }
        }
        for line in offer.missing.lines() {
            if holding.crates.len() >= MAX_ENTRIES {
                break;
            }
            if let Some((name, why)) = line.split_once(' ')
                && why.trim() == "not-ready"
            {
                push(&mut holding.crates, copy(name)?)?;
            }
        }
        Ok(holding)
    }

    /// Keeps what still holds: `plugins` and `crates` say, entry by entry (in the
    /// order of the lists), which are still in use and still built in.
    pub fn keep(&mut self, plugins: &[bool], crates: &[bool]) {
        let mut flags = plugins.iter();
        self.plugins
            .retain(|_| flags.next().copied().unwrap_or(false));
        let mut flags = crates.iter();
        self.crates
            .retain(|_| flags.next().copied().unwrap_or(false));
    }
}

/// The crates of the answer's `missing` lines that never come: the server says
/// they are not in the plugin library (`unknown`), and the update has no build
/// of them (it does not name them among its `plugins`).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn unknown_crates(offer: &Offer) -> Result<Vec<String>, Error> {
    let mut names = Vec::new();
    for line in offer.missing.lines() {
        let Some((name, why)) = line.split_once(' ') else {
            continue;
        };
        let named = offer
            .plugins
            .lines()
            .any(|plugin| plugin.split(' ').next() == Some(name));
        if why.trim() == "unknown" && !named && names.len() < MAX_ENTRIES {
            push(&mut names, copy(name)?)?;
        }
    }
    Ok(names)
}

/// What `waiting.txt` keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Waiting {
    pub version: String,
    pub since: Option<i64>,
    pub checked: Option<i64>,
    pub asked: bool,
    pub seen: bool,
    pub told: bool,
    pub holding: Holding,
}

impl Waiting {
    /// A record of the update `version` that waits for `holding`.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(version: &str, holding: Holding) -> Result<Self, Error> {
        Ok(Self {
            version: copy(version)?,
            holding,
            ..Self::default()
        })
    }

    /// What `text` keeps; a text that is no record, or whose first line is no
    /// version, is `Ok(None)`. Lines it does not know are passed over.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn parse(text: &str) -> Result<Option<Self>, Error> {
        let mut lines = text.lines();
        let Some(version) = lines.next().map(str::trim) else {
            return Ok(None);
        };
        if !is_version(version) {
            return Ok(None);
        }
        let mut waiting = Self {
            version: copy(version)?,
            ..Self::default()
        };
        let seconds = |rest: &str| rest.trim().parse::<i64>().ok();
        for line in lines {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("since ") {
                waiting.since = seconds(rest);
            } else if let Some(rest) = line.strip_prefix("checked ") {
                waiting.checked = seconds(rest);
            } else if let Some(rest) = line.strip_prefix("plugin ") {
                if let Some((id, version)) = rest.split_once(' ')
                    && waiting.holding.plugins.len() < MAX_ENTRIES
                {
                    push(
                        &mut waiting.holding.plugins,
                        (copy(id)?, copy(version.trim())?),
                    )?;
                }
            } else if let Some(rest) = line.strip_prefix("crate ") {
                if waiting.holding.crates.len() < MAX_ENTRIES {
                    push(&mut waiting.holding.crates, copy(rest.trim())?)?;
                }
            } else {
                match line {
                    "asked" => waiting.asked = true,
                    "seen" => waiting.seen = true,
                    "told" => waiting.told = true,
                    _ => {}
                }
            }
        }
        Ok(Some(waiting))
    }

    /// The text of the file.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn to_text(&self) -> Result<String, Error> {
        let mut text = String::new();
        let moment = |text: &mut String, name: &str, at: Option<i64>| match at {
            Some(at) => write_to(text, format_args!("{name} {at}\n")),
            None => write_to(text, format_args!("{name} -\n")),
        };
        write_to(&mut text, format_args!("{}\n", self.version))?;
        moment(&mut text, "since", self.since)?;
        moment(&mut text, "checked", self.checked)?;
        for (flag, set) in [
            ("asked", self.asked),
            ("seen", self.seen),
            ("told", self.told),
        ] {
            if set {
                write_to(&mut text, format_args!("{flag}\n"))?;
            }
        }
        for (id, version) in self.holding.plugins.iter().take(MAX_ENTRIES) {
            write_to(&mut text, format_args!("plugin {id} {version}\n"))?;
        }
        for name in self.holding.crates.iter().take(MAX_ENTRIES) {
            write_to(&mut text, format_args!("crate {name}\n"))?;
        }
        Ok(text)
    }

    /// Whether the update is newer than the firmware running (not any more where
    /// it was installed meanwhile).
    #[must_use]
    pub fn newer_than(&self, running: &str) -> bool {
        compare_versions(&self.version, running).is_gt()
    }

    /// A start at `now` (unix seconds, if known): times in the future (the
    /// clock set back) count from now. Whether to ask again (a day since the last
    /// question; while the clock is not known, once) and whether the page opens
    /// by itself (a week waited unseen, not before).
    pub fn start(&mut self, now: Option<i64>) -> (bool, bool) {
        if let Some(now) = now {
            if self.since.is_none_or(|since| since > now) {
                self.since = Some(now);
            }
            if self.checked.is_some_and(|checked| checked > now) {
                self.checked = None;
            }
        }
        let ask = match now {
            Some(now) => self
                .checked
                .is_none_or(|checked| now.saturating_sub(checked) >= DAY_SECONDS),
            None => !self.asked,
        };
        let open = !self.seen
            && !self.told
            && matches!((self.since, now), (Some(since), Some(now)) if now.saturating_sub(since) >= WEEK_SECONDS);
        self.told |= open;
        (ask, open)
    }

    /// A question asked at `now`: counted, answered or not.
    pub fn tried(&mut self, now: Option<i64>) {
        match now {
            Some(now) => self.checked = Some(now),
            None => self.asked = true,
        }
    }

    /// An answer naming this update, which waits for `holding`, at `now`: kept;
    /// the start of the waiting is the first answer's.
    pub fn answered(&mut self, holding: Holding, now: Option<i64>) {
        self.since = self.since.or(now);
        self.tried(now);
        self.holding = holding;
    }
}

#[cfg(test)]
#[path = "native_waiting_tests.rs"]
mod tests;
