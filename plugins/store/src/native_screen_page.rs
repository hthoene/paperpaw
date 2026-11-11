//! A plugin's page (the Beta 3 `page.rs` on interface 7): what it does, who
//! made it and what it takes, then what can be done with it (install, stop,
//! update, remove, restart, switch on) and what there is to read (how it works,
//! its pictures, its source), with the progress of its install shown on the
//! page. The page's decisions are [`native_page`]'s (`rows`, `actions`); this
//! screen fills [`PageInput`] from what the list, the page and the installs
//! say, draws the rows (an [`ItemList`]) and turns the reader's choice into a
//! [`Next`] for the plugin to carry out.
//!
//! The decisions of Beta 3's `page.rs` that are not in `native_page` are pure
//! functions here, tested without a host: [`facts`] (which facts the page
//! states and in what order), [`go`] (what *Install* does: say no network or
//! another job, show the notice before the first code plugin, wait for the
//! page, start), [`note`] (the line under the rows), [`value_of`], [`title_key`],
//! [`short_key`], [`progress_text`], [`how_text`].
//!
//! **What differs on purpose** (an [`ItemList`] draws the page, not the text
//! and icons of Beta 3): the summary shows in two lines under the name, each
//! fact is a faint row of one line, the notice before the first code plugin
//! shows under its two rows; the room a code plugin needs is the host's check
//! (`Installs::no_room`), so the fact with its sizes is gone and *Not enough
//! room* says why through the install's own refusal text.
//!
//! Rows are built when the screen opens and when what they show changes
//! (`event`), never at the start of the plugin; the summary, name and facts
//! are borrowed from the entry and the page while they are made.
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
        events::Event,
        formatting::Arg,
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages,
    mem::{one, push},
    text::{concat, copy, decimal, decimal_u64},
};

use crate::native_item::More;
use crate::native_model::Entry;
use crate::native_page::{self, Installed, Kind, PageInput, Row};
use crate::native_refusal::{self, Outcome};
use crate::native_screen_shots as shots;
use crate::native_screens::{Ask, Ctx, Next, Reply, Screen, Step, Trouble, ids};

/// The parameters: the plugin's id, its name (shown while the page is not
/// here) and its place in the index.
pub const ID: &str = "id";
pub const TITLE: &str = "title";
pub const ENTRY: &str = "entry";
/// The parameters of the source's screen: the plugin's name and the address.
pub const SOURCE_TITLE: &str = "title";
pub const SOURCE_TEXT: &str = "text";
/// The file the reader is told it has been told what code plugins are in.
const TOLD: &str = "code-told";
/// The file *How it works* is written to for the reader to open.
pub const HOW_FILE: &str = "how.txt";
/// A plugin that takes more memory than this while it runs is said to be
/// tight on devices with little.
pub const TIGHT_HEAP: u64 = 32 * 1024;
/// The mark between the facts of the version line.
const DOT: &str = " \u{b7} ";

/// Whether the plugin's build was checked against its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Verified,
    Failed,
    Not,
}

/// A fact the page states of the plugin, in the order of Beta 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Fact<'a> {
    /// The kernel stopped it: its author's key was withdrawn.
    Revoked,
    /// Nobody reviewed it, and where it comes from (empty where not said).
    Unreviewed {
        repository: &'a str,
    },
    /// Whether it has code, and the PaperPaw it is for.
    Kind {
        code: bool,
        sdk: &'a str,
    },
    Check(Check),
    Official,
    /// Its author, and since when and how many plugins they publish.
    By {
        author: &'a str,
        since: Option<(u32, u32)>,
    },
    /// The key its code is signed with, whole (64 digits).
    Key(&'a str),
    Version {
        version: &'a str,
        size: u64,
        licence: &'a str,
        built_in: bool,
    },
    Heap {
        bytes: u64,
        tight: bool,
    },
    /// The version installed.
    Installed(&'a str),
}

/// What [`facts`] reads.
#[derive(Debug, Clone, Copy)]
pub struct FactInput<'a> {
    pub entry: &'a Entry,
    /// The plugin's page, where it is here.
    pub more: Option<&'a More>,
    pub installed: Option<&'a Installed<'a>>,
    /// The kernel stopped the plugin installed.
    pub revoked: bool,
    /// The device has little memory.
    pub small: bool,
}

/// Whether the plugin is a built-in one switched off: *Switch on* is what the
/// store offers for it, nothing is downloaded.
#[must_use]
pub fn switched_off(installed: Option<&Installed<'_>>) -> bool {
    installed.is_some_and(|i| i.kind == Kind::BuiltIn && !i.enabled)
}

/// The facts the page states, as Beta 3 did: a stopped plugin and one nobody
/// reviewed first, then what it is, whether its build was checked, who made it,
/// its key, its version and size and licence, what it takes of memory, which
/// version is here.
///
/// # Errors
/// `NoMemory`.
pub fn facts<'a>(input: &FactInput<'a>) -> Result<Vec<Fact<'a>>, Error> {
    let entry = input.entry;
    let more = input.more;
    let mut out = Vec::new();
    out.try_reserve_exact(10).map_err(|_| Error::NoMemory)?;
    if input.revoked {
        out.push(Fact::Revoked);
    }
    if entry.unreviewed {
        let repository = more.map_or("", |m| m.repository.trim_start_matches("https://"));
        out.push(Fact::Unreviewed { repository });
    }
    out.push(Fact::Kind {
        code: entry.code,
        sdk: &entry.sdk,
    });
    match more.map(|m| m.verified.as_str()) {
        Some("verified") => out.push(Fact::Check(Check::Verified)),
        Some("failed") => out.push(Fact::Check(Check::Failed)),
        Some("unverified") => out.push(Fact::Check(Check::Not)),
        _ => {}
    }
    if entry.official {
        out.push(Fact::Official);
    } else if !entry.author.is_empty() {
        let since = more
            .filter(|m| m.author_since > 0)
            .map(|m| (m.author_since / 10000, m.author_plugins));
        out.push(Fact::By {
            author: &entry.author,
            since,
        });
    }
    if entry.code
        && !entry.official
        && let Some(key) = more
            .map(|m| m.author_key.as_str())
            .filter(|k| k.len() == 64)
    {
        out.push(Fact::Key(key));
    }
    let off = switched_off(input.installed);
    out.push(Fact::Version {
        version: &entry.version,
        size: entry.size,
        licence: &entry.licence,
        built_in: off,
    });
    if let Some(bytes) = more.and_then(|m| m.heap) {
        out.push(Fact::Heap {
            bytes,
            tight: input.small && bytes > TIGHT_HEAP,
        });
    }
    if let Some(installed) = input.installed.filter(|_| !off) {
        out.push(Fact::Installed(installed.version));
    }
    Ok(out)
}

/// The first 16 digits of a key of 64, in four groups: as the library's page
/// shows it whole.
///
/// # Errors
/// `NoMemory`.
pub fn short_key(key: &str) -> Result<Option<String>, Error> {
    if key.len() != 64 {
        return Ok(None);
    }
    let mut out = String::new();
    out.try_reserve_exact(19).map_err(|_| Error::NoMemory)?;
    for group in 0..4 {
        let Some(part) = key.get(group * 4..group * 4 + 4) else {
            return Ok(None);
        };
        if group > 0 {
            out.push(' ');
        }
        out.push_str(part);
    }
    Ok(Some(out))
}

/// The key of the text of `row`'s name.
#[must_use]
pub const fn title_key(row: Row) -> &'static str {
    match row {
        Row::Install => "store.install",
        Row::Stop => "store.stop",
        Row::Update => "store.update",
        Row::Remove => "store.remove",
        Row::Restart => "store.restart",
        Row::Understood => "store.code.understood",
        Row::Cancel => "store.not-now",
        Row::UseStore => "store.use-store",
        Row::SwitchOn => "store.switch-on",
        Row::NoRoom => "store.no-room",
        Row::NeedsNewer => "store.needs-newer",
        Row::NoBuild => "store.no-build",
        Row::Shots => "store.shots",
        Row::Source => "store.source",
        Row::How => "store.how",
    }
}

/// What a row says at its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Nothing,
    /// What it takes to download.
    Size,
    /// The version offered.
    Version,
    /// How many pictures, and an arrow.
    Shots,
    Arrow,
    /// How far the download is.
    Progress,
    /// The files are being asked for first: *Getting ready…*.
    Preparing,
}

/// What `row` says at its end; `waiting`: *Install* or *Update* was chosen
/// before the page's files were known.
#[must_use]
pub const fn value_of(row: Row, waiting: bool) -> Value {
    match row {
        Row::Install | Row::Update | Row::UseStore if waiting => Value::Preparing,
        Row::Install | Row::NoRoom => Value::Size,
        Row::Update | Row::UseStore => Value::Version,
        Row::Stop => Value::Progress,
        Row::Shots => Value::Shots,
        Row::Source | Row::How => Value::Arrow,
        _ => Value::Nothing,
    }
}

/// What *Install* (or *Update*, *Use the store's version*) does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Go {
    /// There is no network: say so.
    Offline,
    /// Another plugin is being installed: one at a time.
    Busy,
    /// Tell the reader what code plugins are first.
    Notice,
    /// Go ahead: the page's files are here.
    Start,
    /// The library says the plugin is gone.
    Gone,
    /// The page is awaited; `fetch`: ask for it now.
    Wait { fetch: bool },
}

/// What the page knows when *Install* is chosen.
#[derive(Debug, Clone, Copy)]
pub struct Choice {
    /// The plugin's page is here.
    pub here: bool,
    /// How the page stands where it is not here.
    pub state: Trouble,
    /// Another plugin's install goes on.
    pub other_job: bool,
    /// The plugin has code.
    pub code: bool,
    /// The reader was told what code plugins are.
    pub told: bool,
    /// The page was asked for.
    pub asked: bool,
}

/// What *Install* does, in Beta 3's order: no network, another job, the notice
/// before the first code plugin, then by the page.
#[must_use]
pub const fn go(choice: &Choice) -> Go {
    if !choice.here && matches!(choice.state, Trouble::Offline) {
        return Go::Offline;
    }
    if choice.other_job {
        return Go::Busy;
    }
    if choice.code && !choice.told {
        return Go::Notice;
    }
    if choice.here {
        return Go::Start;
    }
    match choice.state {
        Trouble::Failed => Go::Gone,
        _ => Go::Wait {
            fetch: !choice.asked,
        },
    }
}

/// The line under the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Note<'a> {
    Nothing,
    /// This plugin is downloading: the reader may leave the page.
    Downloading,
    /// Another plugin's install goes on.
    Busy(&'a str),
    /// A restart waits.
    Restart,
    /// How the last install ended: the text's key.
    Ended(&'static str),
}

/// The note: the job of this plugin, another's, a restart, or how the last
/// install ended (a refusal says why, with the plugin's name).
#[must_use]
pub fn note<'a>(first: Option<Row>, step: &Step, other: Option<&'a str>, flash: bool) -> Note<'a> {
    if first == Some(Row::Stop) {
        return Note::Downloading;
    }
    if let Some(name) = other {
        return Note::Busy(name);
    }
    if first == Some(Row::Restart) {
        return Note::Restart;
    }
    match *step {
        Step::Failed(reason) => match native_refusal::outcome(reason, flash) {
            Outcome::Said(key) => Note::Ended(key),
            _ => Note::Nothing,
        },
        _ => Note::Nothing,
    }
}

/// A size as people read it: `820 B`, `14 KB`, `2,3 MB`, in whole numbers (no
/// floats); `mark` is the decimal mark.
///
/// # Errors
/// `NoMemory`.
pub fn size_text(bytes: u64, mark: &str) -> Result<String, Error> {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    // Thousandths of the unit.
    let mut value = u128::from(bytes) * 1000;
    let mut unit = 0;
    while value >= 1_000_000 && unit + 1 < UNITS.len() {
        value /= 1024;
        unit += 1;
    }
    let mut digits = [0; 20];
    let name = UNITS.get(unit).copied().unwrap_or("B");
    if unit == 0 || value >= 10_000 {
        let whole = u64::try_from((value + 500) / 1000).unwrap_or(u64::MAX);
        concat(&[decimal_u64(whole, &mut digits), " ", name])
    } else {
        let tenths = u64::try_from((value + 50) / 100).unwrap_or(u64::MAX);
        let mut low = [0; 20];
        concat(&[
            decimal_u64(tenths / 10, &mut digits),
            mark,
            decimal_u64(tenths % 10, &mut low),
            " ",
            name,
        ])
    }
}

/// How far a download came: `1.2 MB / 3.4 MB`, or the bytes so far where the
/// total is not known.
///
/// # Errors
/// `NoMemory`.
pub fn progress_text(done: u32, total: u32, mark: &str) -> Result<String, Error> {
    let done = size_text(u64::from(done), mark)?;
    if total == 0 {
        return Ok(done);
    }
    concat(&[&done, " / ", &size_text(u64::from(total), mark)?])
}

/// The text *How it works* opens in the reader: the name, a blank line, the
/// description.
///
/// # Errors
/// `NoMemory`.
pub fn how_text(name: &str, description: &str) -> Result<String, Error> {
    concat(&[name, "\n\n", description, "\n"])
}

/// The tag of the question *Remove* asks.
pub const REMOVE: u8 = 1;

/// `step` without the bytes of a download: what the rows are made from.
#[must_use]
pub fn calm(step: Step) -> Step {
    match step {
        Step::Downloading { .. } => Step::Downloading { done: 0, total: 0 },
        other => other,
    }
}

/// Whether the answer to the question *Remove* asked (`tag`, `reply`) is
/// *Remove* (the first option); *Keep* and anything else leave the plugin.
#[must_use]
pub fn confirms_remove(tag: u8, reply: &Reply<'_>) -> bool {
    tag == REMOVE && matches!(reply, Reply::Choice(0))
}

/// What changes the rows: the page, how it stands, the install's step. The
/// bytes of a download are not in it: they move many times a second, and the
/// rows are not made again for them (only the progress row's value, `progress`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Sig {
    page: bool,
    state: Trouble,
    step: Step,
}

impl Sig {
    fn read(ctx: &Ctx<'_>, id: &str) -> Self {
        Self {
            page: ctx.loader.page(id).is_some(),
            state: ctx.loader.page_state(id),
            step: calm(ctx.installs.step(id)),
        }
    }
}

/// What the rows are made from, besides the plugin: the host, the entry and
/// page, the install's step and the decimal mark.
struct Look<'a, 'b> {
    ctx: &'a Ctx<'b>,
    entry: &'a Entry,
    more: Option<&'a More>,
    step: &'a Step,
    mark: &'a str,
}

/// What [`View::make`] made: the screen's rows and what goes with them.
struct Made {
    items: Vec<Item>,
    targets: Vec<Option<Row>>,
    below: Below,
    note: String,
    empty: (String, String),
    /// The header, where the plugin is known.
    title: Option<String>,
    sig: Sig,
}

/// What is under the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Below {
    Nothing,
    Note,
    /// The notice, or what is said while the page is not here.
    Empty,
}

/// The screen.
#[derive(Debug)]
pub struct View {
    /// Boxed: the screens share one enum, and the largest sets its size.
    list: Box<ItemList>,
    id: String,
    /// The header: the plugin's name.
    title: String,
    /// What the list says of the plugin, until its page is here.
    entry: Option<Entry>,
    /// What each item does when chosen; none for what is only read.
    targets: Vec<Option<Row>>,
    /// The notice before the first code plugin is shown.
    notice: bool,
    /// *Install* or *Update* was chosen before the files were known.
    waiting: Option<Row>,
    /// The page was asked for.
    asked: bool,
    sig: Sig,
    below: Below,
    note: String,
    empty: (String, String),
}

fn param<'a>(params: &[(&str, &'a str)], name: &str) -> Option<&'a str> {
    params.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

/// The place of the first row that does something (where the focus starts).
#[must_use]
pub fn first_action(targets: &[Option<Row>]) -> usize {
    targets.iter().position(Option::is_some).unwrap_or(0)
}

fn faint(item: Item) -> Item {
    match item {
        Item::Row(mut row) => {
            row.disabled = true;
            Item::Row(row)
        }
        other => other,
    }
}

impl View {
    /// The entry on show: the page's, else the list's.
    fn entry<'a>(&'a self, ctx: &'a Ctx<'_>) -> Option<&'a Entry> {
        ctx.loader
            .page(&self.id)
            .map(|page| &page.entry)
            .or(self.entry.as_ref())
    }

    /// Asks for the plugin's page, once, where it is not here: what the
    /// plugin does when the screen is shown.
    pub fn start(&mut self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        if self.asked || ctx.loader.page(&self.id).is_some() {
            return Ok(Next::None);
        }
        self.asked = true;
        Ok(Next::FetchPage(copy(&self.id)?))
    }

    /// The rows from what the list, the page and the installs say now; the
    /// focus stays on its row, or goes to the first that does something.
    fn build(&mut self, ctx: &Ctx<'_>, reset: bool) -> Result<(), Error> {
        let made = self.make(ctx)?;
        let focus = if reset {
            first_action(&made.targets)
        } else {
            self.list.focus()
        };
        self.sig = made.sig;
        self.below = made.below;
        self.note = made.note;
        self.empty = made.empty;
        if let Some(title) = made.title {
            self.title = title;
        }
        self.targets = made.targets;
        self.list.replace(made.items);
        self.list.set_focus(focus);
        Ok(())
    }

    /// What the screen shows now, made without changing it.
    fn make(&self, ctx: &Ctx<'_>) -> Result<Made, Error> {
        let mut say = Messages::new()?;
        let host = ctx.host;
        let sig = Sig::read(ctx, &self.id);
        let have = ctx.installs.have(&self.id);
        let page = ctx.loader.page(&self.id);
        let Some(entry) = page.map(|p| &p.entry).or(self.entry.as_ref()) else {
            return self.made_empty(ctx, &mut say, sig);
        };
        let more = page.map(|p| &p.more);
        let step = ctx.installs.step(&self.id);
        let small = ctx.loader.small_memory();
        let input = PageInput {
            entry,
            installed: have.as_ref(),
            own_job: matches!(step, Step::Downloading { .. } | Step::Installing),
            notice: self.notice,
            no_room: ctx.installs.no_room(&self.id),
            takes_betas: ctx.installs.takes_betas(),
            has_description: more.is_some_and(|m| !m.description.is_empty()),
            has_shots: more.is_some_and(|m| shots::count(&m.shots, small) > 0),
            has_source: more.is_some_and(|m| !m.source.is_empty()),
        };
        let rows = native_page::rows(&input)?;
        let mut items = Vec::new();
        let mut targets = Vec::new();
        let room = rows.len() + 12;
        items.try_reserve_exact(room).map_err(|_| Error::NoMemory)?;
        targets
            .try_reserve_exact(room)
            .map_err(|_| Error::NoMemory)?;
        let mark = say.copy(host, "core.decimal")?;
        let mut empty = (String::new(), String::new());
        let mut below = Below::Nothing;
        if self.notice {
            below = Below::Empty;
            empty = (copy(&entry.name)?, say.copy(host, "store.code.notice")?);
        } else {
            self.info(ctx, &mut say, &input, &mark, (&mut items, &mut targets))?;
        }
        let look = Look {
            ctx,
            entry,
            more,
            step: &step,
            mark: &mark,
        };
        for row in rows.iter().copied() {
            push(&mut items, self.row_item(&mut say, &look, row)?)?;
            push(&mut targets, Some(row))?;
        }
        let other = ctx.installs.other_job(&self.id);
        let first = rows.first().copied();
        let line = note(first, &step, other, ctx.installs.flash_hint());
        let note = Self::note_text(host, &mut say, &line, &entry.name)?;
        if !note.is_empty() && below == Below::Nothing {
            below = Below::Note;
        }
        Ok(Made {
            items,
            targets,
            below,
            note,
            empty,
            title: Some(copy(&entry.name)?),
            sig,
        })
    }

    /// Nothing of the plugin is known yet (or any more): what is said instead.
    fn made_empty(&self, ctx: &Ctx<'_>, say: &mut Messages, sig: Sig) -> Result<Made, Error> {
        let (title, text) = match ctx.loader.page_state(&self.id) {
            Trouble::Failed => ("store.gone", "store.gone.help"),
            Trouble::Offline => ("store.offline", "store.offline.page"),
            _ => ("store.loading-plugin", ""),
        };
        let empty = (
            say.copy(ctx.host, title)?,
            if text.is_empty() {
                String::new()
            } else {
                say.copy(ctx.host, text)?
            },
        );
        Ok(Made {
            items: Vec::new(),
            targets: Vec::new(),
            below: Below::Empty,
            note: String::new(),
            empty,
            title: None,
            sig,
        })
    }

    /// The faint rows above the actions: the name and what it does, then the
    /// facts.
    fn info(
        &self,
        ctx: &Ctx<'_>,
        say: &mut Messages,
        input: &PageInput<'_>,
        mark: &str,
        (items, targets): (&mut Vec<Item>, &mut Vec<Option<Row>>),
    ) -> Result<(), Error> {
        let entry = input.entry;
        let page = ctx.loader.page(&self.id);
        push(items, faint(Item::noted(&entry.name, "", &entry.summary)?))?;
        push(targets, None)?;
        let facts = facts(&FactInput {
            entry,
            more: page.map(|p| &p.more),
            installed: input.installed,
            revoked: ctx.installs.revoked(&self.id),
            small: ctx.loader.small_memory(),
        })?;
        for fact in &facts {
            let text = fact_text(ctx.host, say, fact, mark)?;
            push(items, faint(Item::row(&text, "")?))?;
            push(targets, None)?;
        }
        Ok(())
    }

    fn row_item(&self, say: &mut Messages, look: &Look<'_, '_>, row: Row) -> Result<Item, Error> {
        let (host, entry) = (look.ctx.host, look.entry);
        let title = say.copy(host, title_key(row))?;
        let value = match value_of(row, self.waiting.is_some()) {
            Value::Nothing => String::new(),
            Value::Size => say.digits(host, &size_text(entry.size, look.mark)?)?,
            Value::Version => copy(&entry.version)?,
            Value::Shots => {
                let small = look.ctx.loader.small_memory();
                let n = look.more.map_or(0, |m| shots::count(&m.shots, small));
                let mut digits = [0; 20];
                let n = decimal(n, &mut digits);
                concat(&[&say.digits(host, n)?, "  \u{203a}"])?
            }
            Value::Arrow => copy("\u{203a}")?,
            Value::Progress => match *look.step {
                Step::Downloading { done, total } => {
                    say.digits(host, &progress_text(done, total, look.mark)?)?
                }
                _ => say.copy(host, "store.preparing")?,
            },
            Value::Preparing => say.copy(host, "store.preparing")?,
        };
        Item::row(&title, &value)
    }

    /// The line under the rows, in words.
    fn note_text(
        host: &Host<'_>,
        say: &mut Messages,
        note: &Note<'_>,
        name: &str,
    ) -> Result<String, Error> {
        let named = |value| {
            [Arg::Text {
                name: "name",
                value,
            }]
        };
        match *note {
            Note::Nothing => Ok(String::new()),
            Note::Downloading => say.copy(host, "store.downloading"),
            Note::Busy(other) => say.format(host, "store.busy", &named(other)),
            Note::Restart => say.copy(host, "store.restart.help"),
            Note::Ended(key) => say.format(host, key, &named(name)),
        }
    }

    /// *Install*, *Update* or *Use the store's version* was chosen.
    fn install(&mut self, ctx: &Ctx<'_>, row: Row) -> Result<Next, Error> {
        let Some(entry) = self.entry(ctx) else {
            return Ok(Next::None);
        };
        let (code, name) = (entry.code, copy(&entry.name)?);
        let choice = Choice {
            here: ctx.loader.page(&self.id).is_some(),
            state: ctx.loader.page_state(&self.id),
            other_job: ctx.installs.other_job(&self.id).is_some(),
            code,
            told: ctx.data.exists(TOLD),
            asked: self.asked,
        };
        let id = copy(&self.id)?;
        Ok(match go(&choice) {
            Go::Offline => Next::Notify("store.offline"),
            Go::Busy => match ctx.installs.other_job(&self.id) {
                Some(other) => Next::Said {
                    key: "store.busy",
                    name: copy(other)?,
                },
                None => Next::None,
            },
            Go::Notice => {
                self.notice = true;
                self.build(ctx, true)?;
                Next::Redraw
            }
            Go::Start if row == Row::Update => Next::Update(id),
            Go::Start => Next::Install(id),
            Go::Gone => Next::Said {
                key: "store.refused.other",
                name,
            },
            Go::Wait { fetch } => {
                self.waiting = Some(row);
                self.asked = true;
                self.build(ctx, false)?;
                if fetch {
                    Next::FetchPage(id)
                } else {
                    Next::Redraw
                }
            }
        })
    }

    /// The page came (or was refused) while *Install* waited for it.
    fn resume(&mut self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        let Some(row) = self.waiting else {
            return Ok(Next::None);
        };
        if ctx.loader.page(&self.id).is_none()
            && ctx.loader.page_state(&self.id) == Trouble::Loading
        {
            return Ok(Next::None);
        }
        self.waiting = None;
        self.install(ctx, row)
    }

    fn activate(&mut self, ctx: &Ctx<'_>, row: Row) -> Result<Next, Error> {
        let id = || copy(&self.id);
        Ok(match row {
            Row::Install | Row::Update | Row::UseStore => self.install(ctx, row)?,
            Row::SwitchOn => Next::SwitchOn(id()?),
            Row::NeedsNewer => Next::Notify("store.needs-newer.help"),
            Row::NoBuild => Next::Notify("store.no-build.help"),
            Row::NoRoom => {
                let key = if ctx.installs.flash_hint() {
                    "store.refused.no-room-flash"
                } else {
                    "store.refused.no-room"
                };
                let name = self
                    .entry(ctx)
                    .map_or(Ok(String::new()), |e| copy(&e.name))?;
                Next::Said { key, name }
            }
            Row::Understood => {
                // A card that does not take the mark asks again next time.
                let _ = ctx.data.write(TOLD, b"");
                self.notice = false;
                self.install(ctx, Row::Install)?
            }
            Row::Cancel => {
                self.notice = false;
                self.build(ctx, true)?;
                Next::Redraw
            }
            Row::Stop => Next::Stop(id()?),
            Row::Restart => Next::Restart,
            Row::Remove => self.ask_remove(ctx)?,
            Row::Shots => Next::Open {
                screen: ids::SHOTS,
                params: one((shots::PARAM, id()?))?,
            },
            Row::Source => self.open_source(ctx)?,
            Row::How => self.open_how(ctx)?,
        })
    }

    /// *Remove* asks first, as Beta 3 did: remove or keep.
    fn ask_remove(&self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        let mut say = Messages::new()?;
        let name = self
            .entry(ctx)
            .map_or(self.id.as_str(), |e| e.name.as_str());
        let title = say.format(ctx.host, "store.remove-question", &[text("name", name)])?;
        let mut options = Vec::new();
        options.try_reserve_exact(2).map_err(|_| Error::NoMemory)?;
        options.push(say.copy(ctx.host, "store.option.remove")?);
        options.push(say.copy(ctx.host, "store.option.keep")?);
        Ok(Next::Ask {
            tag: REMOVE,
            ask: Ask::Choice { title, options },
        })
    }

    fn open_source(&self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        let (Some(entry), Some(page)) = (self.entry(ctx), ctx.loader.page(&self.id)) else {
            return Ok(Next::None);
        };
        let mut params = Vec::new();
        params.try_reserve_exact(3).map_err(|_| Error::NoMemory)?;
        params.push((ID, copy(&self.id)?));
        params.push((SOURCE_TITLE, copy(&entry.name)?));
        params.push((SOURCE_TEXT, copy(&page.more.source)?));
        Ok(Next::Open {
            screen: ids::SOURCE,
            params,
        })
    }

    /// *How it works*: the description is written to a file for the reader to
    /// open; where it cannot be (no memory, the card), the reader is told.
    fn open_how(&self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        let (Some(entry), Some(page)) = (self.entry(ctx), ctx.loader.page(&self.id)) else {
            return Ok(Next::None);
        };
        let written = how_text(&entry.name, &page.more.description).and_then(|text| {
            ctx.data
                .write(HOW_FILE, text.as_bytes())
                .map_err(|_| Error::Io)
        });
        Ok(match written {
            Ok(()) => Next::Read(copy(HOW_FILE)?),
            // The text says the memory may be full.
            Err(_) => Next::Notify("store.how-failed"),
        })
    }

    /// The rows are made anew; whether the screen needs a redraw is the
    /// answer's.
    fn settle(&mut self, ctx: &Ctx<'_>, always: bool) -> Result<Next, Error> {
        let sig = Sig::read(ctx, &self.id);
        let changed = always || sig != self.sig;
        let next = self.resume(ctx)?;
        let mut moved = false;
        if changed {
            self.build(ctx, false)?;
        } else if let Step::Downloading { done, total } = ctx.installs.step(&self.id) {
            moved = self.progress(ctx, done, total)?;
        }
        Ok(match next {
            Next::None if changed || moved => Next::Redraw,
            other => other,
        })
    }

    /// The bytes of the download moved: the value of the progress row is made
    /// again, in place, and nothing else (one row's texts, not the screen's).
    /// `false` where the page shows no *Stop* row (the one whose value is the progress).
    fn progress(&mut self, ctx: &Ctx<'_>, done: u32, total: u32) -> Result<bool, Error> {
        let Some(at) = self.targets.iter().position(|row| *row == Some(Row::Stop)) else {
            return Ok(false);
        };
        let mut say = Messages::new()?;
        let mark = say.copy(ctx.host, "core.decimal")?;
        let value = say.digits(ctx.host, &progress_text(done, total, &mark)?)?;
        Ok(self.list.set_value(at, value))
    }
}

fn text<'a>(name: &'a str, value: &'a str) -> Arg<'a> {
    Arg::Text { name, value }
}

/// The text of a fact.
fn fact_text(
    host: &Host<'_>,
    say: &mut Messages,
    fact: &Fact<'_>,
    mark: &str,
) -> Result<String, Error> {
    match *fact {
        Fact::Revoked => say.copy(host, "store.revoked"),
        Fact::Unreviewed { repository: "" } => say.copy(host, "store.unreviewed.fact"),
        Fact::Unreviewed { repository } => say.format(
            host,
            "store.unreviewed.from",
            &[text("repository", repository)],
        ),
        Fact::Kind { code: true, sdk } => say.format(host, "store.kind.code", &[text("sdk", sdk)]),
        Fact::Kind { code: false, .. } => say.copy(host, "store.kind.data"),
        Fact::Check(Check::Verified) => say.copy(host, "store.verified"),
        Fact::Check(Check::Failed) => say.copy(host, "store.verified.failed"),
        Fact::Check(Check::Not) => say.copy(host, "store.verified.not"),
        Fact::Official => say.copy(host, "store.official"),
        Fact::By {
            author,
            since: None,
        } => say.format(host, "store.by", &[text("author", author)]),
        Fact::By {
            author,
            since: Some((year, count)),
        } => {
            let mut digits = [0; 20];
            let year = decimal_u64(u64::from(year), &mut digits);
            say.format(
                host,
                "store.by-since",
                &[
                    text("author", author),
                    text("year", year),
                    Arg::Count(i64::from(count)),
                ],
            )
        }
        Fact::Key(key) => match short_key(key)? {
            Some(short) => say.format(host, "store.author-key", &[text("key", &short)]),
            None => Ok(String::new()),
        },
        Fact::Version {
            version,
            size,
            licence,
            built_in,
        } => {
            let line = if built_in {
                say.format(host, "store.version-built-in", &[text("version", version)])?
            } else {
                let size = say.digits(host, &size_text(size, mark)?)?;
                say.format(
                    host,
                    "store.version",
                    &[text("version", version), text("size", &size)],
                )?
            };
            if licence.is_empty() {
                return Ok(line);
            }
            let licence = say.format(host, "store.licence", &[text("licence", licence)])?;
            concat(&[&line, DOT, &licence])
        }
        Fact::Heap { bytes, tight } => {
            let size = say.digits(host, &size_text(bytes, mark)?)?;
            let key = if tight {
                "store.heap.tight"
            } else {
                "store.heap"
            };
            say.format(host, key, &[text("size", &size)])
        }
        Fact::Installed(version) => {
            say.format(host, "store.installed-version", &[text("version", version)])
        }
    }
}

impl Screen for View {
    fn open(ctx: &Ctx<'_>, params: &[(&str, &str)]) -> Result<Self, Error> {
        let id = param(params, ID)
            .filter(|id| !id.is_empty())
            .ok_or(Error::Invalid)?;
        // What the list says of the plugin, while its own page is asked for.
        let entry = match (
            param(params, ENTRY).and_then(|n| n.parse::<usize>().ok()),
            ctx.loader.index(),
        ) {
            (Some(n), Some(index)) => match index.entry(ctx.data, n) {
                Ok(entry) => entry.filter(|entry| entry.id == id),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => None,
            },
            _ => None,
        };
        let mut view = Self {
            list: Box::default(),
            id: copy(id)?,
            title: copy(param(params, TITLE).unwrap_or(""))?,
            entry,
            targets: Vec::new(),
            notice: false,
            waiting: None,
            asked: false,
            sig: Sig {
                page: false,
                state: Trouble::Loading,
                step: Step::Idle,
            },
            below: Below::Nothing,
            note: String::new(),
            empty: (String::new(), String::new()),
        };
        view.build(ctx, true)?;
        Ok(view)
    }

    fn shown(&mut self, ctx: &Ctx<'_>) -> Result<(), Error> {
        // What the card and the device hold may have changed meanwhile.
        self.build(ctx, false)
    }

    fn draw(&mut self, ctx: &Ctx<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let footer = match self.below {
            Below::Empty => Footer::Empty {
                title: &self.empty.0,
                text: &self.empty.1,
            },
            Below::Note => Footer::Note(&self.note),
            Below::Nothing => Footer::None,
        };
        self.list
            .draw_with(ctx.host, frame, &self.title, true, footer)
    }

    fn input(&mut self, ctx: &Ctx<'_>, input: &Input<'_>) -> Result<(Handled, Next), Error> {
        // Back from the notice to the plugin.
        if self.notice
            && let Input::Action {
                action: "core.back",
                ..
            } = *input
        {
            self.notice = false;
            self.build(ctx, true)?;
            return Ok((Handled::Yes, Next::Redraw));
        }
        let step = self.list.input(ctx.host, input)?;
        let next = match step {
            ItemStep::Chosen(index) => match self.targets.get(index).copied().flatten() {
                Some(row) => self.activate(ctx, row)?,
                None => Next::None,
            },
            _ => Next::None,
        };
        Ok((ItemList::handled(step), next))
    }

    fn answered(&mut self, _: &Ctx<'_>, tag: u8, reply: Reply<'_>) -> Result<Next, Error> {
        if confirms_remove(tag, &reply) {
            return Ok(Next::Remove(copy(&self.id)?));
        }
        Ok(Next::None)
    }

    fn event(&mut self, ctx: &Ctx<'_>, event: &Event<'_>) -> Result<Next, Error> {
        match event {
            Event::ScreenShown { .. } => {
                let fetch = self.start(ctx)?;
                let rest = self.settle(ctx, true)?;
                Ok(if fetch == Next::None { rest } else { fetch })
            }
            Event::PluginsChanged { .. } | Event::InstallDone(_) => self.settle(ctx, true),
            Event::HttpDone(_) | Event::HttpProgress(_) => self.settle(ctx, false),
            _ => Ok(Next::None),
        }
    }
}
