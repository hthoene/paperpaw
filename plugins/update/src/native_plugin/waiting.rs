//! An update that waits for plugins, on the host (the record, its rules and the
//! words are in `native_waiting`): which of what it waits for is still in use
//! or built in (`core.registry`, `core.device`), the record `waiting.txt`, the
//! question asked again once a day at a cheap moment (`core.work`), the page that
//! opens once after a week unseen, and the page's states *waits for plugins* and
//! *update anyway*.
use super::{
    Button, MOST_PAGES, NativeUpdate, Running, View,
    online::{CHECK_KEY, tolerate},
    plain, row,
};
use crate::{
    native_online::{OFFER, Offer},
    native_waiting::{CRATE_PREFIX, FILE, Holding, MAX_LEN, Waiting, unknown_crates},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    device::DeviceCursor,
    formatting::Arg,
    registry::{PageCursor, RegistryPluginKind},
    snapshot::capture_snapshot,
    storage::{self, Root},
    ui::Open,
    work::WorkSpec,
};
use paperpaw_ui_kit::{
    Item, Messages,
    files::{read_all, replace},
    mem::{SERVICE_BUFFER, buffer, join},
    text::copy,
};

/// A day, in milliseconds.
const DAY_MS: u32 = 86_400_000;
/// A plugin row's flag: switched on.
const ENABLED: u32 = 1;

/// An update that waits, as the page shows it: the plugins it waits for (code
/// plugins in use, then crates built in now it is not ready for) and those that
/// never come, by their names.
#[derive(Debug, PartialEq, Eq)]
pub struct Waited {
    pub offer: Offer,
    /// The code plugins in use it has no build for.
    stopping: String,
    /// The crates built in now it is not built for yet.
    crates: String,
    /// The crates built in now the server says are not in its plugin library.
    unknown: String,
    /// Every plugin built in now the update has no build of, as the answer
    /// lists its crates (what is gone after it; empty where it lists none).
    leaves: String,
}

/// The work that asks the server again: not before `after_ms`, within `within_ms`,
/// at a moment the network is up anyway.
fn spec(after_ms: u32, within_ms: u32) -> Result<WorkSpec<'static>, Error> {
    Ok(WorkSpec::when_cheap(CHECK_KEY, super::WORK_DUE)?
        .after_ms(after_ms)
        .within_ms(within_ms))
}

/// The record, if there is one: a damaged or too large file is none.
fn read_record(host: &Host<'_>) -> Result<Option<Waiting>, Error> {
    match read_all(host, Root::Data, FILE, MAX_LEN) {
        Ok(Some(bytes)) => match core::str::from_utf8(&bytes) {
            Ok(text) => Waiting::parse(text),
            Err(_) => Ok(None),
        },
        Ok(None) | Err(Error::Invalid | Error::NoSpace | Error::Io) => Ok(None),
        Err(error) => Err(error),
    }
}

fn write_record(host: &Host<'_>, waiting: &Waiting) -> Result<(), Error> {
    let text = waiting.to_text()?;
    tolerate(replace(host, Root::Data, FILE, text.as_bytes()))
}

/// Nothing waits (any more).
fn clear(host: &Host<'_>) {
    storage::remove(host, Root::Data, FILE).ok();
}

/// Unix seconds, where the clock is known.
fn now(host: &Host<'_>) -> Result<Option<i64>, Error> {
    Ok(capture_snapshot(host)?.unix_seconds())
}

impl NativeUpdate {
    /// Which of the plugins `holding` names are installed, switched on and of the
    /// version asked about.
    fn plugin_flags(&mut self, host: &Host<'_>, holding: &Holding) -> Result<Vec<bool>, Error> {
        let mut flags = Vec::new();
        flags
            .try_reserve_exact(holding.plugins.len())
            .map_err(|_| Error::NoMemory)?;
        flags.resize(holding.plugins.len(), false);
        if flags.is_empty() {
            return Ok(flags);
        }
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let registry = self.online.registry(host)?;
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page = registry.plugins(host, cursor, &mut input, &mut out)?;
            for plugin in page.rows() {
                if plugin.kind() != RegistryPluginKind::Code || plugin.flags() & ENABLED == 0 {
                    continue;
                }
                for (flag, (id, version)) in flags.iter_mut().zip(&holding.plugins) {
                    *flag |= id == plugin.id() && version == plugin.version();
                }
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(flags)
    }

    /// Which of the crates `crates` are still built in.
    fn built_in_flags(&mut self, host: &Host<'_>, crates: &[String]) -> Result<Vec<bool>, Error> {
        let mut flags = Vec::new();
        flags
            .try_reserve_exact(crates.len())
            .map_err(|_| Error::NoMemory)?;
        flags.resize(crates.len(), false);
        if flags.is_empty() {
            return Ok(flags);
        }
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let dev = self.device(host)?;
        let mut cursor = DeviceCursor::start();
        for _ in 0..MOST_PAGES {
            let page = match dev.builtins(host, cursor, &mut input, &mut out) {
                Ok(page) => page,
                Err(Error::NotFound) => break,
                Err(error) => return Err(error),
            };
            for plugin in page.rows() {
                for (flag, name) in flags.iter_mut().zip(crates) {
                    *flag |= name.strip_prefix(CRATE_PREFIX) == Some(plugin.id());
                }
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(flags)
    }

    /// What of `holding` still holds here.
    fn still(&mut self, host: &Host<'_>, holding: &mut Holding) -> Result<(), Error> {
        let plugins = self.plugin_flags(host, holding)?;
        let crates = self.built_in_flags(host, &holding.crates)?;
        holding.keep(&plugins, &crates);
        Ok(())
    }

    /// The names the reader knows the plugins by, `, ` between: `wanted` is each
    /// one's text where nothing better is known and its id. A first-party crate is
    /// named after its plugin.
    fn shown(&mut self, host: &Host<'_>, wanted: &[(&str, &str)]) -> Result<String, Error> {
        let names = self.shown_each(host, wanted)?;
        join(&names, ", ")
    }

    /// The same, one name each (in the order of `wanted`).
    pub(super) fn shown_each(
        &mut self,
        host: &Host<'_>,
        wanted: &[(&str, &str)],
    ) -> Result<Vec<String>, Error> {
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        names
            .try_reserve_exact(wanted.len())
            .map_err(|_| Error::NoMemory)?;
        for (name, _) in wanted {
            names.push(copy(name)?);
        }
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let registry = self.online.registry(host)?;
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page = registry.plugins(host, cursor, &mut input, &mut out)?;
            for plugin in page.rows() {
                for (shown, (_, id)) in names.iter_mut().zip(wanted) {
                    if *id == plugin.id() && !plugin.name().is_empty() {
                        *shown = copy(plugin.name())?;
                    }
                }
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(names)
    }

    /// The page's view of the update `offer`: it waits where plugins in use have
    /// no build for it (or those built in now are not built for it yet); until the
    /// reader chose to update anyway.
    pub(super) fn offer_view(&mut self, host: &Host<'_>, offer: Offer) -> Result<View, Error> {
        let mut holding = Holding::of_offer(&offer)?;
        if !holding.is_empty() {
            self.still(host, &mut holding)?;
        }
        if holding.is_empty() {
            let leaves = self.left_by_offer(host, &offer, true)?;
            return Ok(View::Offer(offer, leaves));
        }
        let leaves = self.left_by_offer(host, &offer, false)?;
        let plugins: Vec<(&str, &str)> = {
            let mut list = Vec::new();
            list.try_reserve_exact(holding.plugins.len())
                .map_err(|_| Error::NoMemory)?;
            list.extend(
                holding
                    .plugins
                    .iter()
                    .map(|(id, _)| (id.as_str(), id.as_str())),
            );
            list
        };
        let stopping = self.shown(host, &plugins)?;
        let crates = self.crate_names(host, &holding.crates)?;
        let mut never = unknown_crates(&offer)?;
        let built_in = self.built_in_flags(host, &never)?;
        let mut flags = built_in.iter();
        never.retain(|_| flags.next().copied().unwrap_or(false));
        let unknown = self.crate_names(host, &never)?;
        let waited = Waited {
            offer,
            stopping,
            crates,
            unknown,
            leaves,
        };
        Ok(if self.anyway {
            View::Anyway(waited)
        } else {
            View::Waiting(waited)
        })
    }

    fn crate_names(&mut self, host: &Host<'_>, crates: &[String]) -> Result<String, Error> {
        let mut wanted = Vec::new();
        wanted
            .try_reserve_exact(crates.len())
            .map_err(|_| Error::NoMemory)?;
        wanted.extend(crates.iter().map(|name| {
            (
                name.as_str(),
                name.strip_prefix(CRATE_PREFIX).unwrap_or(name),
            )
        }));
        self.shown(host, &wanted)
    }

    /// Declares the question asked again, not before `after_ms`, within
    /// `within_ms`. A work that cannot be declared (a full table, the network
    /// turned off) costs this day's question.
    fn ask_again(&mut self, host: &Host<'_>, after_ms: u32, within_ms: u32) -> Result<(), Error> {
        let spec = spec(after_ms, within_ms)?;
        let (mut input, mut out) = (buffer(256)?, buffer(64)?);
        let declared = self
            .online
            .work(host)?
            .declare(host, &spec, &mut input, &mut out);
        match declared {
            Err(Error::Full | Error::Denied | Error::Unsupported) => Ok(()),
            other => other,
        }
    }

    /// The answer to the question named `offer`: where the update waits for
    /// plugins it is kept and the server asked again in a day; else nothing waits.
    pub(super) fn waiting_answered(&mut self, host: &Host<'_>, offer: &Offer) -> Result<(), Error> {
        let mut holding = Holding::of_offer(offer)?;
        if !holding.is_empty() {
            self.still(host, &mut holding)?;
        }
        if holding.is_empty() {
            clear(host);
            return Ok(());
        }
        let mut record = match read_record(host)? {
            Some(record) if record.version == offer.version => record,
            _ => Waiting::new(&offer.version, Holding::default())?,
        };
        record.answered(holding, now(host)?);
        write_record(host, &record)?;
        self.ask_again(host, DAY_MS, 2 * DAY_MS)
    }

    /// No update is named (or none is newer): nothing waits.
    pub(super) fn waiting_none(&mut self, host: &Host<'_>) {
        clear(host);
    }

    /// The server could not be reached while an update waits: asked again in a day.
    pub(super) fn waiting_failed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if read_record(host)?.is_some() {
            self.ask_again(host, DAY_MS, 2 * DAY_MS)?;
        }
        Ok(())
    }

    /// The server is asked: the next question not before a day (without a clock,
    /// not before it is known), where an update waits.
    pub(super) fn waiting_tried(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if let Some(mut record) = read_record(host)? {
            record.tried(now(host)?);
            write_record(host, &record)?;
        }
        Ok(())
    }

    /// The update is downloaded, whether it waited or not: no more questions.
    pub(super) fn waiting_done(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if read_record(host)?.is_none() {
            return Ok(());
        }
        clear(host);
        let (mut input, mut out) = (buffer(256)?, buffer(64)?);
        let work = self.online.work(host)?;
        tolerate(work.cancel(host, CHECK_KEY, &mut input, &mut out))
    }

    /// The reader looked at an update that waits.
    #[inline(never)]
    pub(super) fn waiting_seen(&mut self, host: &Host<'_>, view: &View) -> Result<(), Error> {
        if !matches!(view, View::Waiting(_)) {
            return Ok(());
        }
        match read_record(host)? {
            Some(mut record) if !record.seen => {
                record.seen = true;
                write_record(host, &record)
            }
            _ => Ok(()),
        }
    }

    /// At a start: whether the update page opens by itself, and the question asked
    /// again where it is due. What waits is kept small in `waiting.txt`; an update
    /// kept from before it was written (or with it lost) is looked at once and
    /// kept so. Nothing waits any more where the update is gone or what it waits
    /// for is no longer in use.
    pub(super) fn waiting_start(
        &mut self,
        host: &Host<'_>,
        running: &Running,
    ) -> Result<bool, Error> {
        if storage::size(host, Root::Data, OFFER).is_err() {
            clear(host);
            return Ok(false);
        }
        let (mut record, made) = match read_record(host)? {
            Some(record) => (record, false),
            None => {
                let Some(offer) = Self::offer(host, &running.version)? else {
                    return Ok(false);
                };
                let holding = Holding::of_offer(&offer)?;
                if holding.is_empty() {
                    return Ok(false);
                }
                (Waiting::new(&offer.version, holding)?, true)
            }
        };
        // Installed meanwhile (from the card) or no longer newer: nothing
        // waits, nor is the answer kept.
        if !record.newer_than(&running.version) {
            clear(host);
            storage::remove(host, Root::Data, OFFER).ok();
            return Ok(false);
        }
        // The text it is kept as: a copy that can fail (no clone of its strings).
        let before = record.to_text()?;
        self.still(host, &mut record.holding)?;
        if record.holding.is_empty() {
            clear(host);
            return Ok(false);
        }
        let (due, open) = record.start(now(host)?);
        if due {
            self.ask_again(host, 0, DAY_MS)?;
        }
        if made || record.to_text()? != before {
            write_record(host, &record)?;
        }
        Ok(open)
    }

    /// Opens the update page (a week unseen).
    pub(super) fn open_page(host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 128];
        CommandSender::new(host, &mut scratch).open(Open::screen("update.page")?)
    }
}

/// The rows of an update that waits: *Keep waiting* first (the way on), *Update
/// anyway*, *Check again*; chosen anyway, *Update later* first, then *Download
/// anyway* (which a second press does not pass by).
pub(super) fn rows(
    host: &Host<'_>,
    messages: &mut Messages,
    anyway: bool,
) -> Result<(Vec<Item>, Vec<Button>), Error> {
    let mut rows = (Vec::new(), Vec::new());
    if anyway {
        row(host, messages, &mut rows, "update.later", "", Button::Close)?;
        plain(
            host,
            messages,
            &mut rows,
            "update.download-anyway",
            Button::Download,
        )?;
    } else {
        plain(
            host,
            messages,
            &mut rows,
            "update.keep-waiting",
            Button::Close,
        )?;
        plain(host, messages, &mut rows, "update.anyway", Button::Download)?;
        plain(
            host,
            messages,
            &mut rows,
            "update.check-again",
            Button::Search,
        )?;
    }
    Ok((rows.0, rows.1))
}

/// The large line and the words under it: what the update waits for, and that
/// some cannot come at all; chosen anyway, what stops working and what is gone.
pub(super) fn texts(
    host: &Host<'_>,
    messages: &mut Messages,
    waited: &Waited,
    anyway: bool,
) -> Result<(String, String), Error> {
    let shown = super::shown_version(host, messages, &waited.offer.version)?;
    let version = [Arg::Text {
        name: "version",
        value: &shown,
    }];
    if anyway {
        let mut help = String::new();
        if !waited.stopping.is_empty() {
            help = messages.format(host, "update.stopping", &list(&waited.stopping))?;
        }
        // What is gone: all the update has no build of, where the answer says;
        // else the crates it names as not ready or not in the library.
        let gone = if waited.leaves.is_empty() {
            join_names(&waited.crates, &waited.unknown)?
        } else {
            copy(&waited.leaves)?
        };
        if !gone.is_empty() {
            let text = messages.format(host, "update.leaves.anyway", &list(&gone))?;
            help = if help.is_empty() {
                text
            } else {
                paperpaw_ui_kit::text::concat(&[&help, " ", &text])?
            };
        }
        return Ok((messages.format(host, "update.offer.title", &version)?, help));
    }
    let names = join_names(&waited.stopping, &waited.crates)?;
    let mut help = messages.format(host, "update.waiting.help", &list(&names))?;
    if !waited.unknown.is_empty() {
        let text = messages.format(host, "update.waiting.unknown", &list(&waited.unknown))?;
        help = paperpaw_ui_kit::text::concat(&[&help, " ", &text])?;
    }
    Ok((
        messages.format(host, "update.waiting.title", &version)?,
        help,
    ))
}

/// The argument `plugins` of a text.
pub(super) const fn list(text: &str) -> [Arg<'_>; 1] {
    [Arg::Text {
        name: "plugins",
        value: text,
    }]
}

/// Two lists of names as one.
fn join_names(first: &str, second: &str) -> Result<String, Error> {
    match (first.is_empty(), second.is_empty()) {
        (_, true) => copy(first),
        (true, false) => copy(second),
        (false, false) => paperpaw_ui_kit::text::concat(&[first, ", ", second]),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;
    use crate::native_waiting::DAY_SECONDS;

    #[test]
    fn the_question_asked_again_is_a_work_the_host_takes() {
        // At a start: within a day; after an answer: not before a day, within two.
        spec(0, DAY_MS).unwrap().check().unwrap();
        spec(DAY_MS, 2 * DAY_MS).unwrap().check().unwrap();
        assert_eq!(i64::from(DAY_MS) / 1000, DAY_SECONDS);
    }

    #[test]
    fn two_lists_of_names_are_one() {
        assert_eq!(join_names("a", "b").unwrap(), "a, b");
        assert_eq!(join_names("a", "").unwrap(), "a");
        assert_eq!(join_names("", "b").unwrap(), "b");
        assert_eq!(join_names("", "").unwrap(), "");
    }
}
