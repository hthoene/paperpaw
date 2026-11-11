//! What the store asks, and how it reads the answers (slice 7, host-light):
//! the dialog of plugins another plugin asks for (events 77 and 78), the
//! first-run offer of a few plugins and the offer of Literata's bold and
//! italic. Each is a pure function of what the plugin read (the card, the
//! answer, a few facts of the device) giving data or a [`Next`]; the plugin
//! executes the host calls (`Ui::ask`, `answer_plugin`, the queue's start).
//! The interface-6 rules of `request.rs`, `setup.rs` and `styles.rs`, the same
//! words (`i18n/{en,de}.ini`), the same files (a Beta 3 card is read as it is).
//!
//! Texts come through [`Say`] (the host's catalog in [`Words`], a table in
//! tests). A requesting plugin and a library are untrusted: an id is checked
//! with `is_plugin_name` before it is a line of a file or part of a key.
//!
//! Interface 7 differences, on purpose: the answer to the one who asked is one
//! [`PluginOutcome`] for each id (`answer_plugin`) where interface 6 sent one
//! event with lists and an error text: installed, declined, failed (no
//! connection, not fetched) or not found (the library does not have it); a
//! request the host numbers each is its own (the same ask while its dialog is
//! open is [`Reply::Join`]: the plugin answers both when it ends).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Storage;
use paperpaw_sdk::install::is_plugin_name;
use paperpaw_sdk::native::{
    Error, Host, commands::PluginOutcome, events::AnswerOutcome, formatting::Arg, ui::Question,
};
use paperpaw_ui_kit::{
    Messages,
    mem::push,
    text::{concat, copy, decimal_u64},
};

use crate::native_index::Index;
use crate::native_model::Entry;
use crate::native_queue::{Mark, Queue, Report, join_names, read_text};
use crate::native_request::{self, Finished, Last, Request, Verdict};
use crate::native_screens::Next;

/// The texts of the store's catalog, as the host says them.
pub trait Say {
    /// The text of `key`; `None` where the catalog has none.
    ///
    /// # Errors
    /// `NoMemory`, or what the host refuses.
    fn lookup(&mut self, key: &str) -> Result<Option<String>, Error>;
    /// The text of `key` with `args` put in (the plural by `Arg::Count`).
    ///
    /// # Errors
    /// `NoMemory`, or what the host refuses.
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error>;
    /// The ASCII digits of `text` in the language's own digits.
    ///
    /// # Errors
    /// `NoMemory`, or what the host refuses.
    fn digits(&mut self, text: &str) -> Result<String, Error>;
    /// The text of `key`.
    ///
    /// # Errors
    /// `NotFound` where the catalog has none, else as [`lookup`](Self::lookup).
    fn text(&mut self, key: &str) -> Result<String, Error> {
        self.lookup(key)?.ok_or(Error::NotFound)
    }
}

/// The host's catalog: [`Say`] through the buffers of one [`Messages`].
#[derive(Debug)]
pub struct Words<'h, 'a> {
    host: &'h Host<'a>,
    messages: Messages,
}

impl<'h, 'a> Words<'h, 'a> {
    /// Words said through `host`.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(host: &'h Host<'a>) -> Result<Self, Error> {
        Ok(Self {
            host,
            messages: Messages::new()?,
        })
    }

    /// The buffers, for what the ui-kit says through a [`Messages`] itself.
    pub fn messages(&mut self) -> &mut Messages {
        &mut self.messages
    }
}

impl Say for Words<'_, '_> {
    fn lookup(&mut self, key: &str) -> Result<Option<String>, Error> {
        match self.messages.copy(self.host, key) {
            Ok(text) => Ok(Some(text)),
            Err(Error::NotFound | Error::Invalid) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        self.messages.format(self.host, key, args)
    }

    fn digits(&mut self, text: &str) -> Result<String, Error> {
        self.messages.digits(self.host, text)
    }
}

/// A size as people read it: `820 B`, `14 KB`, `2,3 MB` (`mark` is the decimal
/// mark, the digits Latin; see [`shown_size`]). What `files::size` says in
/// interface 6.
///
/// # Errors
/// `NoMemory`.
pub fn size_text(bytes: u64, mark: &str) -> Result<String, Error> {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    let mut digits = [0; 20];
    let unit = UNITS.get(unit).copied().unwrap_or("B");
    if unit == "B" || value >= 10.0 {
        concat(&[decimal_u64((value + 0.5) as u64, &mut digits), " ", unit])
    } else {
        let tenths = (value * 10.0 + 0.5) as u64;
        let whole = copy(decimal_u64(tenths / 10, &mut digits))?;
        let rest = decimal_u64(tenths % 10, &mut digits);
        concat(&[&whole, mark, rest, " ", unit])
    }
}

/// A size as the language shown writes it: [`size_text`] with the language's
/// decimal mark (`core.decimal`, a point where it has none) and digits.
///
/// # Errors
/// `NoMemory`, or what the host refuses.
pub fn shown_size(say: &mut dyn Say, bytes: u64) -> Result<String, Error> {
    let mark = say.lookup("core.decimal")?.filter(|mark| !mark.is_empty());
    let text = size_text(bytes, mark.as_deref().unwrap_or("."))?;
    say.digits(&text)
}

/// The plugins' names (the library's, else their ids) of the first `most`,
/// joined with `, `; more end with `, …`.
///
/// # Errors
/// `NoMemory`.
pub fn names_up_to(list: &[(String, Option<Entry>)], most: usize) -> Result<String, Error> {
    fn name(item: &(String, Option<Entry>)) -> &str {
        item.1.as_ref().map_or(item.0.as_str(), |e| e.name.as_str())
    }
    let shown = join_names(list.iter().take(most).map(name), ", ")?;
    if list.len() <= most {
        return Ok(shown);
    }
    concat(&[&shown, ", …"])
}

// ---------------------------------------------------------------------------
// The requests of other plugins (events 77 and 78)
// ---------------------------------------------------------------------------

/// The text of a dialog's lines at most: a question's text holds 1,024 bytes.
const MAX_TEXT: usize = 1024;

/// The ids an ask names as the store takes them: those that are a plugin's name,
/// each once, none on the device already, at most
/// [`native_request::MAX_WANTED`].
///
/// # Errors
/// `NoMemory`.
pub fn request_ids<'a>(
    plugins: impl IntoIterator<Item = &'a str>,
    on_device: &dyn Fn(&str) -> bool,
) -> Result<Vec<String>, Error> {
    native_request::wanted(
        plugins.into_iter().filter(|id| is_plugin_name(id)),
        on_device,
    )
}

/// What the store does with an ask, before it looks at the library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Plan {
    /// Nothing to answer: everything asked for is on the device.
    Nothing,
    /// Answer every id with this outcome at once.
    Answer(PluginOutcome),
    /// The same ask whose dialog is still open or download going on.
    SameOpen,
    /// Look at the library and show the dialog; `close_stale` closes first the
    /// request a dialog closed without an answer left.
    Ask { close_stale: bool },
}

/// Decides an ask by `by` for `wanted` (see [`request_ids`]) at `now_ms`. A
/// request that is declined again, or while another is under way, is answered
/// *declined*; so is one by an id that is no plugin's name (what `Request`
/// could not read back).
#[must_use]
pub fn plan(
    by: &str,
    wanted: &[String],
    last: Option<&Last>,
    now_ms: u64,
    open: bool,
    requested_waiting: bool,
) -> Plan {
    let verdict = native_request::decide(by, wanted, last, now_ms, open, requested_waiting);
    match verdict {
        Verdict::NothingMissing => Plan::Nothing,
        _ if !is_plugin_name(by) => Plan::Answer(PluginOutcome::Declined),
        Verdict::DeclinedAgain | Verdict::Busy => Plan::Answer(PluginOutcome::Declined),
        Verdict::SameOpen => Plan::SameOpen,
        Verdict::Ask { close_stale } => Plan::Ask { close_stale },
    }
}

/// The text key of what is asked: the reader's own and the store's have words,
/// any other plugin gets the general one (the key is never built from a name).
#[must_use]
pub fn by_key(by: &str) -> &'static str {
    match by {
        "reader" => "store.request.by.reader",
        "store" => "store.request.by.store",
        _ => "store.request.by.other",
    }
}

/// The plugins `ids` as the library lists them, with their entries, in the order
/// of `ids`: where the library has an index a plugin it lacks is not offered
/// (left out), where there is none every id stands for itself (`index` is
/// `Index::kept`'s). What `request::entries` gives in interface 6.
///
/// # Errors
/// `NoMemory`.
pub fn listing(
    storage: &dyn Storage,
    index: Option<&Index>,
    ids: &[String],
) -> Result<Vec<(String, Option<Entry>)>, Error> {
    let mut list = Vec::new();
    let Some(index) = index else {
        for id in ids {
            push(&mut list, (copy(id)?, None))?;
        }
        return Ok(list);
    };
    let mut found: Vec<Entry> = Vec::new();
    index.scan(storage, &mut |_, entry| {
        if ids.contains(&entry.id) {
            push(&mut found, entry)?;
        }
        Ok(())
    })?;
    for id in ids {
        if let Some(at) = found.iter().position(|entry| entry.id == *id) {
            push(&mut list, (copy(id)?, Some(found.swap_remove(at))))?;
        }
    }
    Ok(list)
}

/// The dialog that asks the reader, to be shown with [`Dialog::question`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Dialog {
    /// Who asked.
    pub by: String,
    /// The plugins the dialog names (those the library lists, as many as the
    /// question's text holds): the request keeps these and no others, so what
    /// is fetched is what the reader was shown.
    pub ids: Vec<String>,
    /// Asked for and listed by the library, but not named in the dialog (its
    /// text is full): answer each *declined* at once, they are not fetched.
    pub declined: Vec<String>,
    /// Asked for and not in the library: answer each *not found* at once.
    pub not_found: Vec<String>,
    /// What is asked.
    pub title: String,
    /// The plugins with what they take, a line each.
    pub text: String,
    /// The two buttons, `Download` and `Not now`, as the question takes them.
    pub labels: String,
}

impl Dialog {
    /// The confirmation to ask with (`Ui::ask`), the focus on *Download*.
    ///
    /// # Errors
    /// `NoSpace` where a text is over its field.
    pub fn question(&self) -> Result<Question<'_>, Error> {
        Question::confirm(&self.title, &self.text)?
            .confirm_labels(&self.labels)?
            .focus_yes()
    }
}

/// What the plugin does for an ask, once the store has looked.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reply {
    /// Nothing.
    Nothing,
    /// The same ask is open: this request is answered with it, when it ends.
    Join,
    /// Answer each of `ids` with `outcome` (`answer_plugin`).
    Answer {
        ids: Vec<String>,
        outcome: PluginOutcome,
    },
    /// Answer every id the ask named (the plugin holds them) with this outcome:
    /// the store could not go on (memory, the card, the host) and no id may stay
    /// without an outcome, because the host keeps a request open until each has
    /// one and allows only a few open. Needs no memory.
    AnswerAll(PluginOutcome),
    /// Show the dialog; the request is kept on the card. Answer
    /// [`Dialog::not_found`] and [`Dialog::declined`] at once.
    Dialog(Dialog),
    /// The plugins are queued as asked for (start the queue) and this screen
    /// shows their download.
    Fetch(Next),
}

fn answer(ids: &[String], outcome: PluginOutcome) -> Result<Reply, Error> {
    let mut all = Vec::new();
    all.try_reserve_exact(ids.len())
        .map_err(|_| Error::NoMemory)?;
    for id in ids {
        all.push(copy(id)?);
    }
    Ok(Reply::Answer { ids: all, outcome })
}

/// The dialog's lines: each plugin with its size, as many as fit the question's
/// text, and how many that is. A plugin that does not fit is not named, so the
/// caller does not ask for it (nothing is fetched that the reader did not see).
///
/// # Errors
/// `NoMemory`; `NoSpace` where not even the first line fits.
fn lines(say: &mut dyn Say, list: &[(String, Option<Entry>)]) -> Result<(String, usize), Error> {
    let mut text = String::new();
    let mut shown = 0;
    for (n, (id, entry)) in list.iter().enumerate() {
        let line = match entry {
            Some(entry) => {
                let size = shown_size(say, entry.size)?;
                let args = [
                    Arg::Text {
                        name: "name",
                        value: &entry.name,
                    },
                    Arg::Text {
                        name: "size",
                        value: &size,
                    },
                ];
                say.format("store.request.item", &args)?
            }
            None => copy(id)?,
        };
        let more = line.len() + usize::from(n > 0);
        if text.len() + more > MAX_TEXT {
            break;
        }
        text.try_reserve(more).map_err(|_| Error::NoMemory)?;
        if n > 0 {
            text.push('\n');
        }
        text.push_str(&line);
        shown += 1;
    }
    if shown == 0 {
        return Err(Error::NoSpace);
    }
    Ok((text, shown))
}

/// The ids of `wanted` that are not in `list`.
fn missing(wanted: &[String], list: &[(String, Option<Entry>)]) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    for id in wanted {
        if !list.iter().any(|(listed, _)| listed == id) {
            push(&mut out, copy(id)?)?;
        }
    }
    Ok(out)
}

/// What the library and the connection make of an ask the store takes: the
/// plugins it has are named in a dialog, none it has is *not found*, no network
/// is *failed*; a request the card refuses to keep is *declined*. The ask is
/// kept as the open request (`request.txt`) for the dialog.
///
/// # Errors
/// `NoMemory`, or what the host refuses.
pub fn offer(
    storage: &dyn Storage,
    say: &mut dyn Say,
    index: Option<&Index>,
    by: &str,
    wanted: &[String],
    online: bool,
) -> Result<Reply, Error> {
    let list = listing(storage, index, wanted)?;
    // The library has not what is asked for: not an error to tell the reader of.
    if list.is_empty() {
        return answer(wanted, PluginOutcome::NotFound);
    }
    // No network: every id asked for fails (those the library lacks too).
    if !online {
        return answer(wanted, PluginOutcome::Failed);
    }
    let not_found = missing(wanted, &list)?;
    let download = say.text("store.request.download")?;
    let not_now = say.text("store.request.not-now")?;
    let title = say.text(by_key(by))?;
    let (text, shown) = lines(say, &list)?;
    let mut ids: Vec<String> = Vec::new();
    let mut declined: Vec<String> = Vec::new();
    for (n, (id, _)) in list.iter().enumerate() {
        push(if n < shown { &mut ids } else { &mut declined }, copy(id)?)?;
    }
    match Request::start(storage, by, &ids) {
        Ok(()) => {}
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return answer(wanted, PluginOutcome::Declined),
    }
    Ok(Reply::Dialog(Dialog {
        by: copy(by)?,
        title,
        text,
        labels: concat(&[&download, "\n", &not_now])?,
        ids,
        declined,
        not_found,
    }))
}

/// What [`asked`] gives: the answer to the ask and what a stale request left.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Asked {
    /// The ids of a request that an earlier dialog left open on the card, closed
    /// now: answer each *declined* (the host keeps it open until each has an
    /// outcome).
    pub stale: Vec<String>,
    /// What to do for this ask.
    pub reply: Reply,
}

/// An ask for plugins (event 77): the whole decision. `last` is the last
/// request shown, which the plugin keeps from ask to ask; `open` says whether a
/// request is open on the card and `waiting` whether plugins asked for wait in
/// the queue (both read by the caller from `Request::open` and the queue).
///
/// Every id the ask names gets an outcome on every path: where the store cannot
/// go on (memory, the card, the host) the reply is
/// [`Reply::AnswerAll`]`(Failed)`; the request left by a closed dialog is
/// answered through [`Asked::stale`].
///
/// # Errors
/// None where the ask is answered; the host's refusal to read `now_ms` etc. is
/// the caller's.
#[allow(
    clippy::too_many_arguments,
    reason = "one ask: who, for what, when, and what the card and the device say of it"
)]
pub fn asked(
    storage: &dyn Storage,
    say: &mut dyn Say,
    index: Option<&Index>,
    last: &mut Option<Last>,
    (by, wanted): (&str, &[String]),
    now_ms: u64,
    (online, open, waiting): (bool, bool, bool),
) -> Result<Asked, Error> {
    let mut stale = Vec::new();
    let reply = match plan(by, wanted, last.as_ref(), now_ms, open, waiting) {
        Plan::Nothing => Reply::Nothing,
        Plan::Answer(outcome) => answer(wanted, outcome).unwrap_or(Reply::AnswerAll(outcome)),
        Plan::SameOpen => Reply::Join,
        Plan::Ask { close_stale } => {
            if close_stale {
                // What the closed dialog left: told *declined*, then closed.
                if let Ok(Some(left)) = Request::open(storage) {
                    stale = left.ids;
                }
                Request::close(storage);
            }
            match offer(storage, say, index, by, wanted, online) {
                Ok(Reply::Dialog(dialog)) => {
                    // Not remembering what was shown only asks again later.
                    *last = remember(by, &dialog.ids, now_ms, false);
                    Reply::Dialog(dialog)
                }
                Ok(other) => other,
                Err(_) => {
                    Request::close(storage);
                    Reply::AnswerAll(PluginOutcome::Failed)
                }
            }
        }
    };
    Ok(Asked { stale, reply })
}

fn remember(by: &str, ids: &[String], at_ms: u64, declined: bool) -> Option<Last> {
    Some(Last {
        by: copy(by).ok()?,
        ids: copy_all(ids).ok()?,
        at_ms,
        declined,
    })
}

fn copy_all(ids: &[String]) -> Result<Vec<String>, Error> {
    paperpaw_ui_kit::mem::copy_strings(ids)
}

/// What the reader said to the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Decision {
    /// *Download*.
    Download,
    /// *Not now*, or the dialog could not be shown.
    NotNow,
    /// The dialog was closed without an answer: nothing is told (no download
    /// starts); the next ask closes what it left.
    Ignore,
}

/// The reader's word as the host gave it: `choice` is 1 for *Download*, 0 for
/// *Not now*; a dialog the host could not start is *Not now*.
#[must_use]
pub fn decision(outcome: AnswerOutcome, choice: u32, not_started: bool) -> Decision {
    match outcome {
        _ if not_started => Decision::NotNow,
        AnswerOutcome::Answered if choice == 1 => Decision::Download,
        AnswerOutcome::Answered => Decision::NotNow,
        _ => Decision::Ignore,
    }
}

/// The reader's answer to the dialog (event 24): the plugins are queued as
/// asked for and their download is shown, or the one who asked is told
/// *Not now* (declined), or no network is *failed*. A dialog closed without an
/// answer is *declined* too (the request is closed and every id has its
/// outcome; nothing is remembered, so the ask may come again). Where the queue
/// cannot be kept (memory, the card) the ids are *failed*: no request is left
/// open without an answer. Nothing happens where no request is open.
///
/// # Errors
/// `NoMemory` where the open request cannot be read: it stays on the card and
/// the next ask answers it (`Asked::stale`).
pub fn answered(
    storage: &dyn Storage,
    last: &mut Option<Last>,
    now_ms: u64,
    online: bool,
    decision: Decision,
) -> Result<Reply, Error> {
    let Some(request) = Request::open(storage)? else {
        return Ok(Reply::Nothing);
    };
    match decision {
        Decision::Ignore => {
            Request::close(storage);
            Ok(Reply::Answer {
                ids: request.ids,
                outcome: PluginOutcome::Declined,
            })
        }
        Decision::NotNow => {
            // Not remembering what was declined only asks again sooner.
            *last = remember(&request.by, &request.ids, now_ms, true);
            Request::close(storage);
            Ok(Reply::Answer {
                ids: request.ids,
                outcome: PluginOutcome::Declined,
            })
        }
        // The connection went meanwhile.
        Decision::Download if !online => {
            Request::close(storage);
            Ok(Reply::Answer {
                ids: request.ids,
                outcome: PluginOutcome::Failed,
            })
        }
        Decision::Download => match start_download(storage, &request) {
            Ok(next) => Ok(Reply::Fetch(next)),
            Err(_) => {
                Request::close(storage);
                Ok(Reply::Answer {
                    ids: request.ids,
                    outcome: PluginOutcome::Failed,
                })
            }
        },
    }
}

/// Queues the plugins of `request` as asked for and gives the fetch screen to
/// open; the screen is made first, so nothing is queued where it cannot be.
fn start_download(storage: &dyn Storage, request: &Request) -> Result<Next, Error> {
    let open = crate::native_screen_fetch::open_next(&request.by, &request.ids)?;
    let mut queue = Queue::read(storage)?;
    queue.add(request.ids.iter().map(String::as_str), Mark::Requested)?;
    queue.keep(storage)?;
    Ok(open)
}

/// What the one who asked is told once the queue is empty: each id installed or
/// failed (`answer_plugin`), in the order of the request (installed first).
///
/// # Errors
/// `NoMemory`.
pub fn finished_answers(done: &Finished) -> Result<Vec<(&str, PluginOutcome)>, Error> {
    let mut all = Vec::new();
    all.try_reserve_exact(done.installed.len() + done.missing.len())
        .map_err(|_| Error::NoMemory)?;
    all.extend(finished_outcomes(done));
    Ok(all)
}

/// [`finished_answers`] as an iterator: needs no memory, so the end of a request
/// is always told (each id its outcome), also where memory is short.
pub fn finished_outcomes(done: &Finished) -> impl Iterator<Item = (&str, PluginOutcome)> {
    done.installed
        .iter()
        .map(|id| (id.as_str(), PluginOutcome::Installed))
        .chain(
            done.missing
                .iter()
                .map(|id| (id.as_str(), PluginOutcome::Failed)),
        )
}

// ---------------------------------------------------------------------------
// The first-run offer
// ---------------------------------------------------------------------------

/// The plugins offered, in this order: reading statistics, the widgets for the
/// home screen, Calibre, KOReader's progress sync.
pub const OFFERED: [&str; 4] = ["stats", "widgets", "calibre", "kosync"];
/// Where the plugins chosen on the page are kept, once added.
pub const CHOSEN: &str = "offer-chosen.txt";
/// The mark that the reader was told what code plugins are (the page of a
/// plugin shows the notice only once).
pub const CODE_TOLD: &str = "code-told";

/// What the device has of a plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Have {
    /// Not there.
    Absent,
    /// There and switched on.
    On,
    /// Built in and switched off.
    OffBuiltIn,
    /// A code plugin there but off (the plugin manager's).
    OffOther,
}

/// How a plugin can be added here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Way {
    /// Built in and switched off: switched on.
    SwitchOn,
    /// From the plugin library, as a code plugin.
    Install,
    /// There already, switched on.
    There,
    /// Not here: neither built in nor loadable as a file.
    No,
}

/// How a plugin the device has `have` of is added; `can_install` says that the
/// device loads code plugins and there is a network.
#[must_use]
pub fn way(have: Have, can_install: bool) -> Way {
    match have {
        Have::On | Have::OffOther => Way::There,
        Have::OffBuiltIn => Way::SwitchOn,
        Have::Absent if can_install => Way::Install,
        Have::Absent => Way::No,
    }
}

/// Whether plugin `id` can be added here (the step is left out where none can).
#[must_use]
pub fn can_add(have: &dyn Fn(&str) -> Have, can_install: bool, id: &str) -> bool {
    matches!(way(have(id), can_install), Way::SwitchOn | Way::Install)
}

/// The plugins offered here, in the order of [`OFFERED`].
///
/// # Errors
/// `NoMemory`.
pub fn rows(have: &dyn Fn(&str) -> Have, can_install: bool) -> Result<Vec<&'static str>, Error> {
    let mut rows = Vec::new();
    for id in OFFERED {
        if can_add(have, can_install, id) {
            push(&mut rows, id)?;
        }
    }
    Ok(rows)
}

/// The texts of an offered plugin's row: its name and what it is.
#[must_use]
pub fn row_keys(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "stats" => ("store.offer.stats", "store.offer.stats.help"),
        "widgets" => ("store.offer.widgets", "store.offer.widgets.help"),
        "calibre" => ("store.offer.calibre", "store.offer.calibre.help"),
        "kosync" => ("store.offer.kosync", "store.offer.kosync.help"),
        _ => return None,
    })
}

/// The hint under the rows: how to choose, and once some are chosen that they
/// are added on going on (`buttons`: a device with keys, no touch).
#[must_use]
pub fn hint_key(any_chosen: bool, buttons: bool) -> &'static str {
    match (any_chosen, buttons) {
        (true, _) => "store.offer.hint.chosen",
        (false, true) => "store.offer.hint.buttons",
        (false, false) => "store.offer.hint",
    }
}

/// What *Done* (or leaving the page) does with the plugins chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Chosen {
    /// Built in and off: switch them on (`set_plugin_enabled`).
    pub switch_on: Vec<&'static str>,
    /// To fetch from the library, quietly (see [`add_quietly`]).
    pub fetch: Vec<String>,
    /// What is kept for the setup's card (`CHOSEN`): the ids, a line each.
    pub kept: String,
    /// The notice to say once.
    pub notify: &'static str,
}

/// What becomes of the plugins `chosen` (ids of [`OFFERED`]).
///
/// # Errors
/// `NoMemory`.
pub fn choose(
    chosen: &[&'static str],
    have: &dyn Fn(&str) -> Have,
    can_install: bool,
) -> Result<Chosen, Error> {
    let (mut switch_on, mut fetch) = (Vec::new(), Vec::new());
    for id in chosen {
        match way(have(id), can_install) {
            Way::SwitchOn => push(&mut switch_on, *id)?,
            Way::Install => push(&mut fetch, copy(id)?)?,
            Way::There | Way::No => {}
        }
    }
    let kept = join_names(chosen.iter().copied(), "\n")?;
    let notify = if fetch.is_empty() {
        "store.offer.switched-on"
    } else {
        "store.offer.adding"
    };
    Ok(Chosen {
        switch_on,
        fetch,
        kept,
        notify,
    })
}

/// Puts `ids` in the queue quietly (behind those waiting, each once): the
/// first-run setup's, fetched without the store being opened and said once at
/// the end. The caller starts the queue.
///
/// # Errors
/// `NoMemory`; `Io` where the card refuses.
pub fn add_quietly(storage: &dyn Storage, ids: &[String]) -> Result<(), Error> {
    let mut queue = Queue::read(storage)?;
    queue.add(ids.iter().map(String::as_str), Mark::Quiet)?;
    queue.keep(storage)
}

/// Keeps what *Done* decided: the plugins chosen (for the setup's card) and,
/// where some are fetched, that the reader was told what code plugins are
/// (this page said what they are) and the queue.
///
/// # Errors
/// `NoMemory`; `Io` where the card refuses the queue (the other two are best
/// effort, as before: they only change what is said later).
pub fn keep_chosen(storage: &dyn Storage, chosen: &Chosen) -> Result<(), Error> {
    match storage.write(CHOSEN, chosen.kept.as_bytes()) {
        Ok(()) | Err(_) => {}
    }
    if chosen.fetch.is_empty() {
        return Ok(());
    }
    match storage.write(CODE_TOLD, b"") {
        Ok(()) | Err(_) => {}
    }
    add_quietly(storage, &chosen.fetch)
}

/// What the setup's card says of the plugins chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Badge {
    /// Nothing before any was chosen.
    None,
    /// This many are being added.
    BeingAdded(usize),
    /// They were added.
    Added,
}

/// How many of the plugins offered wait in the queue.
#[must_use]
pub fn waiting_offered(queue: &Queue) -> usize {
    queue
        .lines()
        .iter()
        .filter(|line| OFFERED.contains(&line.id.as_str()))
        .count()
}

/// The setup's card: how many are being added, or that they were.
#[must_use]
pub fn badge(waiting: usize, chosen_kept: bool) -> Badge {
    if waiting > 0 {
        Badge::BeingAdded(waiting)
    } else if chosen_kept {
        Badge::Added
    } else {
        Badge::None
    }
}

/// The words of a badge.
///
/// # Errors
/// `NoMemory`, or what the host refuses.
pub fn badge_text(say: &mut dyn Say, badge: Badge) -> Result<Option<String>, Error> {
    Ok(match badge {
        Badge::None => None,
        Badge::BeingAdded(count) => Some(say.format(
            "store.offer.being-added",
            &[Arg::Count(i64::try_from(count).unwrap_or(i64::MAX))],
        )?),
        Badge::Added => Some(say.text("store.offer.added")?),
    })
}

/// Plugin `id`'s name where its page did not come: the setup's own for the
/// plugins it offers, else its id. The id is checked before it is part of a key.
///
/// # Errors
/// `NoMemory`, or what the host refuses.
pub fn name_of(say: &mut dyn Say, id: &str) -> Result<String, Error> {
    if is_plugin_name(id)
        && let Some(name) = say.lookup(&concat(&["store.offer.", id])?)?
    {
        return Ok(name);
    }
    copy(id)
}

// ---------------------------------------------------------------------------
// The styles offer (Literata's bold and italic)
// ---------------------------------------------------------------------------

/// The plugin with the faces.
pub const PACK: &str = "literata-styles";
/// How often the store may answer for it at most (it came, or it did not
/// install), over all starts.
pub const TRIES: u32 = 3;
/// How often the store answered for it, and that the hint was said.
pub const TRIES_FILE: &str = "styles-tries.txt";
pub const HINTED_FILE: &str = "styles-hinted.txt";

/// What the firmware has of the faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "four facts of the device, read apart"
)]
pub struct Faces {
    /// The firmware's regular face: where it is, Literata is the default.
    pub regular: bool,
    /// The faces built in (`fonts-default.literata-bold`).
    pub bold_built_in: bool,
    /// The faces of the pack (`literata-styles.bold`).
    pub bold_from_pack: bool,
    /// The pack is on the device.
    pub pack_installed: bool,
}

impl Faces {
    /// Whether this firmware has Literata but not its bold, and the plugin is not
    /// on the card.
    #[must_use]
    pub const fn lacks_styles(self) -> bool {
        self.regular && !self.bold_built_in && !self.bold_from_pack && !self.pack_installed
    }
}

/// How often the store answered for the pack (0 where nothing is kept).
///
/// # Errors
/// `NoMemory`.
pub fn tries(storage: &dyn Storage) -> Result<u32, Error> {
    Ok(read_text(storage, TRIES_FILE, 16)?
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0))
}

/// Puts the pack in the quiet queue if it is wanted at this start: the
/// firmware lacks it, the first-run setup is over (`setup_done`, the setup's
/// own folder), the network is there, the device and the library are known
/// (`device_known`), it is not queued and tried fewer than [`TRIES`] times.
/// Whether it was put in (then the caller starts the queue).
///
/// # Errors
/// `NoMemory`; `Io` where the card refuses the queue.
pub fn styles_offer(
    storage: &dyn Storage,
    faces: Faces,
    (setup_done, online, device_known): (bool, bool, bool),
) -> Result<bool, Error> {
    if !faces.lacks_styles() || !setup_done || !online || !device_known {
        return Ok(false);
    }
    let mut queue = Queue::read(storage)?;
    if queue.lines().iter().any(|line| line.id == PACK) || tries(storage)? >= TRIES {
        return Ok(false);
    }
    queue.add([PACK], Mark::Quiet)?;
    queue.keep(storage)?;
    Ok(true)
}

/// Keeps that the store answered for plugin `id` (with `status`, 0 where there
/// was no connection at all): only an answer is a try.
///
/// # Errors
/// `NoMemory`.
pub fn styles_answered(storage: &dyn Storage, id: &str, status: u16) -> Result<(), Error> {
    if id != PACK || status == 0 {
        return Ok(());
    }
    let mut digits = [0; 20];
    let next = tries(storage)?.saturating_add(1);
    let text = decimal_u64(u64::from(next), &mut digits);
    // A try not kept is none: the offer may be made once more.
    match storage.write(TRIES_FILE, text.as_bytes()) {
        Ok(()) | Err(_) => Ok(()),
    }
}

/// Whether the hint to connect once is to be said now: the first time only, as
/// it is kept.
pub fn hint_due(storage: &dyn Storage) -> bool {
    if storage.exists(HINTED_FILE) {
        return false;
    }
    match storage.write(HINTED_FILE, b"1") {
        Ok(()) | Err(_) => true,
    }
}

/// The names of `list` (joined with `, `) without `name`, if it is one.
///
/// # Errors
/// `NoMemory`.
pub fn without(list: &str, name: &str) -> Result<Option<String>, Error> {
    let mut found = false;
    let rest = join_names(
        list.split(", ").filter(|item| {
            let is = *item == name;
            found |= is;
            !is
        }),
        ", ",
    )?;
    Ok(found.then_some(rest))
}

/// What is said once no quiet plugin waits any more and the notes are read
/// (`native_queue::report`): the faces of Literata have a hint of their own
/// (what helps is the network, not the store; said once), then what was added
/// and what could not be.
///
/// # Errors
/// `NoMemory`, or what the host refuses.
pub fn quiet_notices(
    storage: &dyn Storage,
    say: &mut dyn Say,
    report: &Report,
) -> Result<Vec<String>, Error> {
    let mut notices = Vec::new();
    let mut not = copy(&report.not)?;
    let styles = name_of(say, PACK)?;
    if let Some(rest) = without(&not, &styles)? {
        not = rest;
        if hint_due(storage) {
            let name = Arg::Text {
                name: "name",
                value: &styles,
            };
            push(&mut notices, say.format("store.styles.later", &[name])?)?;
        }
    }
    for (key, names) in [
        ("store.offer.done", report.done.as_str()),
        ("store.offer.not-done", not.as_str()),
    ] {
        if !names.is_empty() {
            let arg = Arg::Text {
                name: "names",
                value: names,
            };
            push(&mut notices, say.format(key, &[arg])?)?;
        }
    }
    Ok(notices)
}

/// The native rules give what the interface-6 code did.
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    extern crate std;

    use super::*;
    use crate::native_parity_tests::old_entry;
    use proptest::prelude::*;

    #[test]
    fn the_constants_are_the_old_ones() {
        assert_eq!(OFFERED, crate::setup::OFFERED);
        assert_eq!(PACK, crate::styles::PACK);
        assert_eq!(native_request::DECLINED_MS, crate::request::DECLINED_MS);
        assert_eq!(MAX_TEXT, 1024);
    }

    proptest! {
        #[test]
        fn a_list_without_a_name_is_what_it_was(
            list in "[a-c, ]{0,24}",
            name in "[a-c]{0,3}",
        ) {
            prop_assert_eq!(
                without(&list, &name).unwrap(),
                crate::styles::without(&list, &name)
            );
        }

        #[test]
        fn names_are_joined_as_they_were(
            names in proptest::collection::vec("[a-z ]{0,8}", 0..6),
            listed in proptest::collection::vec(any::<bool>(), 6),
        ) {
            let native: Vec<(String, Option<Entry>)> = names
                .iter()
                .zip(listed.iter().chain(core::iter::repeat(&false)))
                .enumerate()
                .map(|(n, (name, listed))| {
                    let id = std::format!("id{n}");
                    let entry = listed.then(|| Entry {
                        id: id.clone(),
                        name: name.clone(),
                        ..Entry::default()
                    });
                    (id, entry)
                })
                .collect();
            let old: Vec<_> = native
                .iter()
                .map(|(id, e)| (id.clone(), e.as_ref().map(old_entry)))
                .collect();
            prop_assert_eq!(
                native_request::names(&native).unwrap(),
                crate::request::names(&old)
            );
            let all = names_up_to(&native, usize::MAX).unwrap();
            prop_assert_eq!(all, crate::request::names(&old));
        }

        #[test]
        fn sizes_are_written_as_they_were(bytes in any::<u64>(), mark in "[.,]") {
            prop_assert_eq!(
                size_text(bytes, &mark).unwrap(),
                paperpaw_sdk::files::size(bytes, &mark)
            );
        }
    }
}
