//! Keeping the lists in step with a CalDAV server, one request at a time, with
//! no host: what is asked of the network and the card goes through [`Wire`],
//! so the same code runs against the device and against a server in a test.
//!
//! A run: the lists are looked up (`PROPFIND`), each list that changed is read
//! whole (`REPORT`, saved to a file and read as it streams), then what was
//! changed on the reader is sent, a task at a time.
//!
//! The rules, which `PORTING-7.md` says in full:
//! - A task is sent with the version the reader last saw (`If-Match`), a new
//!   one with `If-None-Match: *`: nothing on the server is written over
//!   unseen.
//! - A task changed on both sides: **the server's version wins, and the
//!   reader's is kept as a new task** beside it. The same where the server
//!   removed a task the reader changed.
//! - A task changed is fetched, changed in its own text and sent back, so
//!   what the reader does not show (notes, alarms) is kept.
//! - Whatever fails stays marked on the task and is sent in a later run.
use alloc::{string::String, vec::Vec};

use paperpaw_caldav::{
    dav::{self, Item, Multistatus, Visit},
    ical::{Lines, NewTodo, Todo, TodoReader, write_todo},
    patch::{Change, Patcher},
    xml::Scanner,
};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    files::Volume,
    mem::{buffer, push},
    text::{concat, copy},
};

use crate::keep::{self, Writer};
use crate::model::{
    ETAG_MAX, FIRST, HREF_MAX, LIST_HREF_MAX, List, MAX_LISTS, MAX_TASKS, NAME_MAX, Seed, Store,
    TITLE_MAX, Task, UID_MAX, clean, flag, plain,
};

/// Who wrote a task made on the reader.
pub const PRODID: &str = "-//PaperPaw//Tasks//EN";
/// The file an answer with many parts is saved to and read from.
pub const ANSWER: &str = "sync.xml";
/// The file a task is fetched to, and the one its changed text is sent from.
pub const ITEM: &str = "item.ics";
pub const PUT: &str = "put.ics";
/// The most bytes of an answer and of one task's text.
pub const ANSWER_MAX: u64 = 2 * 1024 * 1024;
pub const ITEM_MAX: u64 = 256 * 1024;
/// How much of a file is read at a time.
const CHUNK: usize = 2048;
/// The longest body sent with the request itself; a longer one is a file.
const INLINE_MAX: usize = 2048;

/// An error of the reader or of the shared CalDAV code, as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused(pub Error);

impl From<Error> for Refused {
    fn from(error: Error) -> Self {
        Self(error)
    }
}

impl From<paperpaw_caldav::Error> for Refused {
    fn from(error: paperpaw_caldav::Error) -> Self {
        Self(match error {
            paperpaw_caldav::Error::NoMemory => Error::NoMemory,
            _ => Error::Invalid,
        })
    }
}

fn dav<T>(result: Result<T, paperpaw_caldav::Error>) -> Result<T, Error> {
    result.map_err(|error| Refused::from(error).0)
}

/// What a request sends as its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body<'a> {
    None,
    Text(&'a str),
    /// A file of the data folder.
    File(&'a str),
}

/// A request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Call<'a> {
    pub method: &'a str,
    pub url: &'a str,
    /// `Depth: 1`.
    pub deep: bool,
    /// `If-Match`, in quotes.
    pub if_match: Option<&'a str>,
    /// `If-None-Match: *`: only if nothing is there.
    pub only_new: bool,
    /// The body's `Content-Type`.
    pub content_type: &'a str,
    pub body: Body<'a>,
    /// The file of the data folder the answer is saved to, and the most
    /// bytes of it.
    pub save_to: Option<(&'a str, u64)>,
}

impl<'a> Call<'a> {
    const fn new(method: &'a str, url: &'a str) -> Self {
        Self {
            method,
            url,
            deep: false,
            if_match: None,
            only_new: false,
            content_type: "",
            body: Body::None,
            save_to: None,
        }
    }
}

/// What the engine asks of the device.
pub trait Wire {
    /// The plugin's data folder.
    fn files(&self) -> &dyn Volume;
    /// Starts a request (with who asks: the account's name and password);
    /// its number, which its end is told with.
    ///
    /// # Errors
    /// What the host says.
    fn send(&mut self, call: &Call<'_>) -> Result<u32, Error>;
    /// The `ETag` of the answer of the request `id` that just ended.
    ///
    /// # Errors
    /// What the host says.
    fn etag(&mut self, id: u32) -> Result<String, Error>;
    /// Seconds since 1970, if the clock is set.
    fn now(&mut self) -> Option<i64>;
    /// How far the reader's clock is east of UTC at `unix`, in seconds.
    fn zone(&mut self, unix: i64) -> i32;
    /// What a new task's id is made of.
    fn seed(&mut self) -> Seed;
}

/// How a request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// An answer came, with this status.
    Status(u32),
    /// There is no network.
    Offline,
    /// It failed otherwise (a name not found, a certificate, a full card).
    Failed,
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Outcome {
    /// The lists are in step (as far as [`Report::failed`] says).
    #[default]
    Synced,
    /// No address is set.
    NoAccount,
    /// No network: what waits is sent later.
    Offline,
    /// The server did not take the name and password.
    Denied,
    /// Nothing is at the address.
    NotFound,
    /// The address has no list of tasks.
    NoLists,
    /// The server answered with this status, or (0) something else failed.
    Failed(u32),
}

/// What a run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Report {
    pub outcome: Outcome,
    /// Tasks sent: made, changed or removed on the server.
    pub sent: u16,
    /// Tasks changed on both sides: the server's version was taken and the
    /// reader's kept as a new task.
    pub conflicts: u16,
    /// Tasks or lists that could not be read or sent this time.
    pub failed: u16,
    /// Tasks not kept: more than the reader holds, or with an id or address
    /// longer than it keeps.
    pub skipped: u16,
}

/// What an engine call came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// A request is under way (or nothing is to tell).
    Waiting,
    /// The run is over.
    Ended(Report),
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Idle,
    /// The lists are being looked up at `url`.
    Discover {
        url: String,
        hops: u8,
    },
    /// A list is being read; its change tag as it was before.
    Pull {
        list: u8,
        ctag: String,
    },
    /// The task `current` is being made, removed, fetched or sent.
    Create,
    Delete,
    Fetch,
    Put,
}

/// The engine.
#[derive(Debug, Default)]
pub struct Sync {
    state: State,
    request: u32,
    /// The lists still to read, with their change tags.
    queue: Vec<(u8, String)>,
    /// The lists to read once more at the end (a task of theirs was changed
    /// on both sides).
    again: Vec<u8>,
    second: bool,
    /// The id and the list of the task being sent, and its count of changes
    /// then.
    current: String,
    list: u8,
    touch: u8,
    /// The address the account is set to.
    base: String,
    report: Report,
    keep_days: i32,
    blocked: bool,
    dirty: bool,
}

/// A calendar found at the address.
#[derive(Debug)]
struct Calendar {
    href: String,
    name: String,
    ctag: String,
}

/// Collects the lists of tasks an answer names, and where to look instead.
#[derive(Debug, Default)]
struct Found {
    calendars: Vec<Calendar>,
    home: String,
    principal: String,
    skipped: u16,
}

impl Visit for Found {
    type Error = Refused;

    fn data(&mut self, _: &[u8]) -> Result<(), Refused> {
        Ok(())
    }

    fn item(&mut self, item: &Item<'_>) -> Result<(), Refused> {
        if item.cut {
            return Ok(());
        }
        if self.home.is_empty() {
            self.home = copy(item.home)?;
        }
        if self.principal.is_empty() {
            self.principal = copy(item.principal)?;
        }
        let href = dav::path(item.href);
        if !(item.calendar && item.todos) || href.is_empty() {
            return Ok(());
        }
        if self.calendars.len() >= MAX_LISTS || href.len() > LIST_HREF_MAX || !plain(href) {
            self.skipped += 1;
            return Ok(());
        }
        let tag = item.ctag;
        let ctag = if plain(tag) && tag.len() <= ETAG_MAX {
            tag
        } else {
            ""
        };
        let calendar = Calendar {
            href: copy(href)?,
            name: clean(item.name, NAME_MAX)?,
            ctag: copy(ctag)?,
        };
        push(&mut self.calendars, calendar)?;
        Ok(())
    }
}

/// Compares the tasks of one list with what the server's answer has.
struct Merge<'a> {
    store: &'a mut Store,
    list: u8,
    zone: i32,
    today: i32,
    keep_days: i32,
    seed: Seed,
    lines: Lines,
    reader: TodoReader,
    todo: Option<Todo>,
    conflicts: u16,
    skipped: u16,
}

impl Visit for Merge<'_> {
    type Error = Refused;

    fn data(&mut self, bytes: &[u8]) -> Result<(), Refused> {
        let (reader, todo) = (&mut self.reader, &mut self.todo);
        self.lines
            .feed(bytes, &mut |line: &[u8]| -> Result<(), Refused> {
                if todo.is_none() {
                    *todo = reader.line(line)?;
                }
                Ok(())
            })
    }

    fn item(&mut self, item: &Item<'_>) -> Result<(), Refused> {
        let (reader, todo) = (&mut self.reader, &mut self.todo);
        self.lines
            .finish(&mut |line: &[u8]| -> Result<(), Refused> {
                if todo.is_none() {
                    *todo = reader.line(line)?;
                }
                Ok(())
            })?;
        self.reader.reset();
        let Some(todo) = self.todo.take() else {
            return Ok(());
        };
        if item.cut || item.status == 404 {
            self.skipped += u16::from(item.cut);
            return Ok(());
        }
        Ok(self.apply(item.href, item.etag, &todo)?)
    }
}

impl<'a> Merge<'a> {
    fn new(store: &'a mut Store, list: u8, wire: &mut dyn Wire, keep_days: i32) -> Self {
        let now = wire.now().unwrap_or(0);
        let zone = wire.zone(now);
        let today = i32::try_from((now + i64::from(zone)).div_euclid(86_400)).unwrap_or(0);
        for task in &mut store.tasks {
            task.seen = false;
        }
        Self {
            store,
            list,
            zone,
            today,
            keep_days,
            seed: wire.seed(),
            lines: Lines::new(),
            reader: TodoReader::new(TITLE_MAX),
            todo: None,
            conflicts: 0,
            skipped: 0,
        }
    }

    /// The reader's own version of the task at `index`, kept as a new task.
    fn keep_copy(&mut self, index: usize) -> Result<(), Error> {
        if !keep_copy(self.store, index, self.seed)? {
            self.skipped += 1;
        }
        if let Some(copy) = self.store.tasks.last_mut() {
            copy.seen = true;
        }
        Ok(())
    }

    /// A task the server has: `href`, its version `etag` and what it says.
    fn apply(&mut self, href: &str, etag: &str, todo: &Todo) -> Result<(), Error> {
        let href = dav::path(href);
        let etag = if plain(etag) && etag.len() <= ETAG_MAX {
            etag
        } else {
            ""
        };
        let kept = !href.is_empty()
            && href.len() <= HREF_MAX
            && plain(href)
            && todo.uid.len() <= UID_MAX
            && plain(&todo.uid);
        if !kept {
            self.skipped += 1;
            return Ok(());
        }
        let title = clean(&todo.summary, TITLE_MAX)?;
        let due = todo.due.map(|due| due.local_day(self.zone));
        let done_at = todo.completed.unwrap_or(0);
        let state =
            if todo.done { flag::DONE } else { 0 } | if todo.recurring { flag::FIXED } else { 0 };
        let list = self.list;
        let found = self.store.tasks.iter().position(|task| {
            task.list == list
                && if task.href().is_empty() {
                    task.uid() == todo.uid
                } else {
                    dav::same_path(task.href(), href)
                }
        });
        let Some(index) = found else {
            // Done long ago (or nobody knows when): it stays on the server.
            let old =
                done_at.div_euclid(86_400) + i64::from(self.keep_days) < i64::from(self.today);
            if todo.done && (todo.completed.is_none() || old) {
                return Ok(());
            }
            if self.store.tasks.len() >= MAX_TASKS {
                self.skipped += 1;
                return Ok(());
            }
            let mut task = Task::new(list, &todo.uid, href, etag, &title)?;
            (task.flags, task.priority, task.due) = (state, todo.priority, due);
            (task.done_at, task.seen) = (done_at, true);
            return push(&mut self.store.tasks, task);
        };
        let Some(task) = self.store.tasks.get_mut(index) else {
            return Ok(());
        };
        task.seen = true;
        let same = task.says_the_same(&title, due, todo.priority, todo.done);
        if task.href().is_empty() {
            // Made here and already there: a sending whose answer never came.
            task.set_server(href, etag)?;
            task.flags &= !flag::PENDING;
            if !same {
                task.flags |= flag::CHANGED;
            }
            return Ok(());
        }
        if dav::etag_eq(task.etag(), etag) {
            return Ok(());
        }
        if task.pending() && task.etag().is_empty() && !same {
            // The version is not known (a sending that was not answered):
            // the reader's change is sent against the version there is now.
            return task.set_server(href, etag);
        }
        if task.is(flag::CHANGED) && !same {
            // Changed on both sides: the server's wins, the reader's is kept.
            self.keep_copy(index)?;
            self.conflicts += 1;
        }
        let Some(task) = self.store.tasks.get_mut(index) else {
            return Ok(());
        };
        task.set_server(href, etag)?;
        task.set_title(&title)?;
        (task.flags, task.priority, task.due, task.done_at) = (state, todo.priority, due, done_at);
        task.touch = task.touch.wrapping_add(1);
        Ok(())
    }

    /// The answer is read. `complete`: to its end, so what it did not name is
    /// not on the server any more.
    fn finish(self, complete: bool) -> Result<(u16, u16), Error> {
        let Self {
            store,
            list,
            mut conflicts,
            skipped,
            seed,
            ..
        } = self;
        if complete {
            for index in 0..store.tasks.len() {
                let changed = store.tasks.get(index).is_some_and(|task| {
                    task.list == list
                        && !task.seen
                        && !task.href().is_empty()
                        && task.is(flag::CHANGED)
                });
                if changed {
                    // Removed there, changed here: kept as a new task.
                    store.renew(index, seed)?;
                    conflicts += 1;
                }
            }
            store
                .tasks
                .retain(|task| task.list != list || task.seen || task.href().is_empty());
        }
        store.purge(self.today, self.keep_days);
        Ok((conflicts, skipped))
    }
}

/// Keeps what the reader has of the task at `index` as a new task of the same
/// list; whether there was room.
fn keep_copy(store: &mut Store, index: usize, seed: Seed) -> Result<bool, Error> {
    if store.tasks.len() >= MAX_TASKS {
        return Ok(false);
    }
    let uid = store.new_uid(seed)?;
    let Some(task) = store.tasks.get(index) else {
        return Ok(false);
    };
    let mut copy = Task::new(task.list, &uid, "", "", task.title())?;
    copy.flags = flag::NEW | (task.flags & flag::DONE);
    (copy.priority, copy.due, copy.done_at) = (task.priority, task.due, task.done_at);
    push(&mut store.tasks, copy)?;
    Ok(true)
}

/// Reads the saved answer through `visit`; whether it was read to its end.
fn read_answer<V: Visit<Error = Refused>>(
    files: &dyn Volume,
    visit: V,
) -> Result<(V, bool), Error> {
    let mut chunk = buffer(CHUNK)?;
    let mut scanner = Scanner::new();
    let mut reader = Multistatus::new(visit);
    let mut at = 0u64;
    loop {
        let n = files.read_at(ANSWER, at, &mut chunk)?;
        let Some(part) = chunk.get(..n).filter(|part| !part.is_empty()) else {
            break;
        };
        scanner
            .feed(part, &mut reader)
            .map_err(|refused| refused.0)?;
        at += n as u64;
    }
    let complete = reader.complete();
    Ok((reader.visit, complete))
}

/// Writes the fetched task with `change` applied to the file that is sent;
/// whether it held a task.
fn patch_file(files: &dyn Volume, change: Change<'_>) -> Result<bool, Error> {
    let mut chunk = buffer(CHUNK)?;
    let mut patcher = Patcher::new(change);
    let mut writer = Writer::new(files, PUT)?;
    let mut out = |bytes: &[u8]| -> Result<(), Refused> { Ok(writer.put(bytes)?) };
    let mut at = 0u64;
    loop {
        let n = files.read_at(ITEM, at, &mut chunk)?;
        let Some(part) = chunk.get(..n).filter(|part| !part.is_empty()) else {
            break;
        };
        patcher.feed(part, &mut out).map_err(|refused| refused.0)?;
        at += n as u64;
    }
    patcher.finish(&mut out).map_err(|refused| refused.0)?;
    writer.end()?;
    Ok(patcher.patched())
}

/// What the task's marks say was changed.
fn change_of(task: &Task, now: Option<i64>) -> Change<'_> {
    Change {
        summary: task.is(flag::TITLE).then(|| task.title()),
        due: task.is(flag::DUE).then_some(task.due),
        priority: task.is(flag::PRIORITY).then_some(task.priority),
        done: task.is(flag::STATE).then(|| task.done()),
        completed: (task.done_at != 0).then_some(task.done_at),
        now,
    }
}

/// The ETag of an answer as it is kept: none where it cannot be.
fn kept_etag(wire: &mut dyn Wire, id: u32) -> String {
    wire.etag(id)
        .ok()
        .filter(|etag| plain(etag) && etag.len() <= ETAG_MAX)
        .unwrap_or_default()
}

impl Sync {
    /// Whether a run is under way.
    #[must_use]
    pub const fn running(&self) -> bool {
        !matches!(self.state, State::Idle)
    }

    /// Whether the server refused the name and password: no run starts by
    /// itself until the account changes.
    #[must_use]
    pub const fn blocked(&self) -> bool {
        self.blocked
    }

    /// The account was changed.
    pub fn account_changed(&mut self) {
        self.blocked = false;
    }

    /// Whether the lists or tasks changed since this was asked last.
    pub fn take_dirty(&mut self) -> bool {
        core::mem::take(&mut self.dirty)
    }

    fn end(&mut self, outcome: Outcome) -> Step {
        self.state = State::Idle;
        self.queue = Vec::new();
        self.again = Vec::new();
        self.current = String::new();
        self.base = String::new();
        self.blocked |= outcome == Outcome::Denied;
        self.report.outcome = outcome;
        Step::Ended(self.report)
    }

    fn settle(&mut self, step: Result<Step, Error>) -> Step {
        match step {
            Ok(step) => step,
            Err(_) => self.end(Outcome::Failed(0)),
        }
    }

    fn save(&mut self, store: &Store, wire: &dyn Wire) -> Result<(), Error> {
        self.dirty = true;
        keep::save(wire.files(), store)
    }

    /// Starts a run with the server at `address`. Done tasks older than
    /// `keep_days` are not kept on the reader.
    pub fn start(
        &mut self,
        store: &mut Store,
        wire: &mut dyn Wire,
        address: &str,
        keep_days: i32,
    ) -> Step {
        if self.running() {
            return Step::Waiting;
        }
        self.report = Report::default();
        (self.second, self.keep_days) = (false, keep_days);
        if address.is_empty() {
            return self.end(Outcome::NoAccount);
        }
        for task in &mut store.tasks {
            task.skip = false;
        }
        let step = (|| {
            self.base = copy(address)?;
            let url = if store.home.is_empty() {
                copy(address)?
            } else {
                copy(&store.home)?
            };
            self.discover(wire, url, 0)
        })();
        self.settle(step)
    }

    fn discover(&mut self, wire: &mut dyn Wire, url: String, hops: u8) -> Result<Step, Error> {
        let mut call = Call::new("PROPFIND", &url);
        (call.deep, call.content_type) = (true, dav::XML);
        call.body = Body::Text(dav::PROPFIND_CALENDARS);
        call.save_to = Some((ANSWER, ANSWER_MAX));
        self.request = wire.send(&call)?;
        self.state = State::Discover { url, hops };
        Ok(Step::Waiting)
    }

    /// The request `id` ended.
    pub fn done(&mut self, store: &mut Store, wire: &mut dyn Wire, id: u32, ended: Ended) -> Step {
        if !self.running() || id != self.request {
            return Step::Waiting;
        }
        let state = core::mem::take(&mut self.state);
        let status = match ended {
            Ended::Status(401) => return self.end(Outcome::Denied),
            Ended::Status(status) => status,
            Ended::Offline => return self.end(Outcome::Offline),
            Ended::Failed => return self.end(Outcome::Failed(0)),
        };
        let step = match state {
            State::Idle => Ok(Step::Waiting),
            State::Discover { url, hops } => self.discovered(store, wire, url, hops, status),
            State::Pull { list, ctag } => self.pulled(store, wire, list, &ctag, status),
            State::Create => self.created(store, wire, id, status),
            State::Delete => self.deleted(store, wire, status),
            State::Fetch => self.fetched(store, wire, id, status),
            State::Put => self.was_put(store, wire, id, status),
        };
        self.settle(step)
    }

    fn discovered(
        &mut self,
        store: &mut Store,
        wire: &mut dyn Wire,
        url: String,
        hops: u8,
        status: u32,
    ) -> Result<Step, Error> {
        // The lists are no longer where they were found once: looked up
        // again from the address the account is set to.
        let remembered =
            hops == 0 && !store.home.is_empty() && url == store.home && url != self.base;
        let found = match status {
            207 => {
                let (found, complete) = read_answer(wire.files(), Found::default())?;
                if !complete {
                    return Ok(self.end(Outcome::Failed(status)));
                }
                found
            }
            403 => return Ok(self.end(Outcome::Denied)),
            404 | 410 if !remembered => return Ok(self.end(Outcome::NotFound)),
            404 | 410 => Found::default(),
            other => return Ok(self.end(Outcome::Failed(other))),
        };
        if found.calendars.is_empty() && remembered {
            store.home.clear();
            let base = copy(&self.base)?;
            return self.discover(wire, base, 1);
        }
        if found.calendars.is_empty() {
            // Not where the lists are: the server may say where they are.
            let next = [&found.home, &found.principal]
                .into_iter()
                .find(|href| !href.is_empty() && !dav::same_path(href, &url));
            return match next {
                Some(href) if hops < 3 => {
                    let next = dav(dav::resolve(&url, href))?;
                    self.discover(wire, next, hops + 1)
                }
                _ => Ok(self.end(Outcome::NoLists)),
            };
        }
        self.report.skipped += found.skipped;
        if store.home != url {
            store.home = url;
        }
        if !store.has_synced() {
            // The account's lists come for the first time: the list the
            // reader made by itself goes, if nothing was put into it.
            let unused: Vec<u8> = store
                .lists
                .iter()
                .filter(|list| list.ctag == FIRST && !list.synced())
                .filter(|list| !store.tasks.iter().any(|task| task.list == list.id))
                .map(|list| list.id)
                .collect();
            for id in unused {
                store.remove_list(id);
            }
        }
        self.queue = Vec::new();
        for calendar in &found.calendars {
            let known = store
                .lists
                .iter_mut()
                .find(|list| list.synced() && dav::same_path(&list.href, &calendar.href));
            let id = match known {
                Some(list) => {
                    if !calendar.name.is_empty() && list.name != calendar.name {
                        list.name = copy(&calendar.name)?;
                    }
                    if !calendar.ctag.is_empty() && calendar.ctag == list.ctag {
                        // Nothing changed there since it was read.
                        continue;
                    }
                    list.id
                }
                None => {
                    let name = match calendar.name.as_str() {
                        "" => calendar
                            .href
                            .trim_end_matches('/')
                            .rsplit('/')
                            .next()
                            .unwrap_or(""),
                        name => name,
                    };
                    match store.add_list(name, &calendar.href) {
                        Ok(id) => id,
                        Err(Error::Full) => {
                            self.report.skipped += 1;
                            continue;
                        }
                        Err(error) => return Err(error),
                    }
                }
            };
            push(&mut self.queue, (id, copy(&calendar.ctag)?))?;
        }
        Self::drop_gone(store, &found);
        self.save(store, wire)?;
        self.next(store, wire)
    }

    /// The lists the server no longer has go, with their tasks; what was
    /// changed on the reader and never sent stays, in a list of the reader's
    /// own.
    fn drop_gone(store: &mut Store, found: &Found) {
        let gone: Vec<u8> = store
            .lists
            .iter()
            .filter(|list| {
                list.synced()
                    && !found
                        .calendars
                        .iter()
                        .any(|calendar| dav::same_path(&calendar.href, &list.href))
            })
            .map(|list| list.id)
            .collect();
        for id in gone {
            store.tasks.retain_mut(|task| {
                if task.list != id {
                    return true;
                }
                let keeps = task.is(flag::NEW | flag::CHANGED) && task.shows();
                if keeps {
                    let _ = task.set_server("", "");
                    task.flags &= flag::DONE;
                }
                keeps
            });
            if store.tasks.iter().any(|task| task.list == id) {
                if let Some(list) = store.list_mut(id) {
                    list.href.clear();
                    list.ctag.clear();
                }
            } else {
                store.remove_list(id);
            }
        }
    }

    /// The address of something on the server from its path.
    fn url(store: &Store, path: &str) -> Result<String, Error> {
        dav(dav::resolve(&store.home, path))
    }

    /// The next list to read, or the next task to send.
    fn next(&mut self, store: &mut Store, wire: &mut dyn Wire) -> Result<Step, Error> {
        while let Some((list, ctag)) = self.queue.pop() {
            let Some(found) = store.list(list).filter(|list| list.synced()) else {
                continue;
            };
            let url = Self::url(store, &found.href)?;
            let mut call = Call::new("REPORT", &url);
            (call.deep, call.content_type) = (true, dav::XML);
            call.body = Body::Text(dav::REPORT_TODOS);
            call.save_to = Some((ANSWER, ANSWER_MAX));
            self.request = wire.send(&call)?;
            self.state = State::Pull { list, ctag };
            return Ok(Step::Waiting);
        }
        self.send_next(store, wire)
    }

    fn pulled(
        &mut self,
        store: &mut Store,
        wire: &mut dyn Wire,
        list: u8,
        ctag: &str,
        status: u32,
    ) -> Result<Step, Error> {
        if status != 207 {
            self.report.failed += 1;
            return self.next(store, wire);
        }
        let merge = Merge::new(store, list, wire, self.keep_days);
        let (merge, complete) = read_answer(wire.files(), merge)?;
        let (conflicts, skipped) = merge.finish(complete)?;
        self.report.conflicts += conflicts;
        self.report.skipped += skipped;
        if !complete {
            self.report.failed += 1;
        }
        if let Some(list) = store.list_mut(list) {
            // Read again next time unless it was read whole.
            list.ctag = copy(if complete { ctag } else { "" })?;
        }
        self.save(store, wire)?;
        self.next(store, wire)
    }

    /// Sends the next task that waits, or ends the run.
    fn send_next(&mut self, store: &mut Store, wire: &mut dyn Wire) -> Result<Step, Error> {
        loop {
            let waits = |task: &Task, lists: &[List]| {
                task.pending()
                    && !task.skip
                    && lists
                        .iter()
                        .any(|list| list.id == task.list && list.synced())
            };
            let Some(index) = store
                .tasks
                .iter()
                .position(|task| waits(task, &store.lists))
            else {
                return self.finish(store, wire);
            };
            let Some(task) = store.tasks.get(index) else {
                return self.finish(store, wire);
            };
            self.current = copy(task.uid())?;
            (self.list, self.touch) = (task.list, task.touch);
            if task.is(flag::DELETED) {
                if task.href().is_empty() {
                    store.tasks.remove(index);
                    self.dirty = true;
                    continue;
                }
                let url = Self::url(store, task.href())?;
                let version = dav(dav::etag_header(task.etag()))?;
                let mut call = Call::new("DELETE", &url);
                call.if_match = (!task.etag().is_empty()).then_some(version.as_str());
                self.request = wire.send(&call)?;
                self.state = State::Delete;
            } else if task.is(flag::NEW) {
                self.create(store, wire, index)?;
            } else {
                let url = Self::url(store, task.href())?;
                let mut call = Call::new("GET", &url);
                call.save_to = Some((ITEM, ITEM_MAX));
                self.request = wire.send(&call)?;
                self.state = State::Fetch;
            }
            return Ok(Step::Waiting);
        }
    }

    /// Where a new task goes: its path, from its list's.
    fn new_href(store: &Store, task: &Task) -> Result<String, Error> {
        let list = store.list(task.list).ok_or(Error::NotFound)?;
        concat(&[list.href.trim_end_matches('/'), "/", task.uid(), ".ics"])
    }

    fn create(&mut self, store: &Store, wire: &mut dyn Wire, index: usize) -> Result<(), Error> {
        let task = store.tasks.get(index).ok_or(Error::NotFound)?;
        let text = dav(write_todo(&NewTodo {
            prodid: PRODID,
            uid: task.uid(),
            summary: task.title(),
            due: task.due,
            priority: task.priority,
            done: task.done(),
            completed: (task.done_at != 0).then_some(task.done_at),
            now: wire.now(),
        }))?;
        let url = Self::url(store, &Self::new_href(store, task)?)?;
        let mut call = Call::new("PUT", &url);
        (call.only_new, call.content_type) = (true, dav::CALENDAR);
        call.body = if text.len() <= INLINE_MAX {
            Body::Text(&text)
        } else {
            let mut writer = Writer::new(wire.files(), PUT)?;
            writer.put(text.as_bytes())?;
            writer.end()?;
            Body::File(PUT)
        };
        self.request = wire.send(&call)?;
        self.state = State::Create;
        Ok(())
    }

    /// The task being sent, if it is still there.
    fn current(&self, store: &Store) -> Option<usize> {
        store.find_in(self.list, &self.current)
    }

    fn created(
        &mut self,
        store: &mut Store,
        wire: &mut dyn Wire,
        id: u32,
        status: u32,
    ) -> Result<Step, Error> {
        let Some(index) = self.current(store) else {
            return self.send_next(store, wire);
        };
        let href = match store.tasks.get(index) {
            Some(task) => Self::new_href(store, task)?,
            None => String::new(),
        };
        let touch = self.touch;
        let Some(task) = store.tasks.get_mut(index) else {
            return self.send_next(store, wire);
        };
        match status {
            200..=299 => {
                task.set_server(&href, &kept_etag(wire, id))?;
                task.flags &= !flag::NEW;
                if task.touch != touch {
                    // Changed while it was sent: sent again, as a change.
                    task.flags |= flag::CHANGED;
                    task.skip = true;
                }
                self.report.sent += 1;
            }
            412 => {
                // It is there already (a sending that was never answered):
                // the next reading says which version, then it is compared.
                task.set_server(&href, "")?;
                task.flags = (task.flags & !flag::NEW) | flag::CHANGED;
                task.skip = true;
            }
            _ => {
                task.skip = true;
                self.report.failed += 1;
            }
        }
        self.save(store, wire)?;
        self.send_next(store, wire)
    }

    fn deleted(
        &mut self,
        store: &mut Store,
        wire: &mut dyn Wire,
        status: u32,
    ) -> Result<Step, Error> {
        let Some(index) = self.current(store) else {
            return self.send_next(store, wire);
        };
        match status {
            200..=299 | 404 | 410 => {
                store.tasks.remove(index);
                self.report.sent += 1;
            }
            412 => {
                // Changed there since: the server's version wins, it is back.
                self.server_wins(store, index, None)?;
            }
            _ => {
                if let Some(task) = store.tasks.get_mut(index) {
                    task.skip = true;
                }
                self.report.failed += 1;
            }
        }
        self.save(store, wire)?;
        self.send_next(store, wire)
    }

    /// The task at `index` was changed on the server too: its list is read
    /// again and the server's version taken; with `keep`, the reader's
    /// version is kept as a new task.
    fn server_wins(
        &mut self,
        store: &mut Store,
        index: usize,
        keep: Option<Seed>,
    ) -> Result<(), Error> {
        if let Some(seed) = keep {
            if !keep_copy(store, index, seed)? {
                self.report.skipped += 1;
            }
            self.report.conflicts += 1;
        }
        let Some(task) = store.tasks.get_mut(index) else {
            return Ok(());
        };
        let href = copy(task.href())?;
        task.set_server(&href, "")?;
        task.flags &= !flag::PENDING;
        task.skip = true;
        if !self.again.contains(&task.list) {
            push(&mut self.again, task.list)?;
        }
        Ok(())
    }

    fn fetched(
        &mut self,
        store: &mut Store,
        wire: &mut dyn Wire,
        id: u32,
        status: u32,
    ) -> Result<Step, Error> {
        let Some(index) = self.current(store) else {
            return self.send_next(store, wire);
        };
        match status {
            200..=299 => {
                if self.send_changed(store, wire, index, id)? {
                    return Ok(Step::Waiting);
                }
            }
            404 | 410 => {
                // Removed there, changed here: kept as a new task, sent next.
                store.renew(index, wire.seed())?;
                self.report.conflicts += 1;
            }
            _ => self.failed(store, index),
        }
        self.save(store, wire)?;
        self.send_next(store, wire)
    }

    /// The task at `index` could not be sent this time.
    fn failed(&mut self, store: &mut Store, index: usize) {
        if let Some(task) = store.tasks.get_mut(index) {
            task.skip = true;
        }
        self.report.failed += 1;
    }

    /// The task at `index` was fetched (request `id`): if the server still
    /// has the version the reader knows, its text is changed and sent back;
    /// whether it was sent.
    fn send_changed(
        &mut self,
        store: &mut Store,
        wire: &mut dyn Wire,
        index: usize,
        id: u32,
    ) -> Result<bool, Error> {
        let there = kept_etag(wire, id);
        let now = wire.now();
        let Some(task) = store.tasks.get(index) else {
            return Ok(false);
        };
        let known = task.etag();
        if !known.is_empty() && !there.is_empty() && !dav::etag_eq(known, &there) {
            self.server_wins(store, index, Some(wire.seed()))?;
            return Ok(false);
        }
        if !patch_file(wire.files(), change_of(task, now))? {
            // No task in what the server gave.
            self.failed(store, index);
            return Ok(false);
        }
        let version = if there.is_empty() { known } else { &there };
        let header = dav(dav::etag_header(version))?;
        let url = Self::url(store, task.href())?;
        let mut call = Call::new("PUT", &url);
        call.if_match = (!version.is_empty()).then_some(header.as_str());
        (call.content_type, call.body) = (dav::CALENDAR, Body::File(PUT));
        self.request = wire.send(&call)?;
        self.state = State::Put;
        Ok(true)
    }

    fn was_put(
        &mut self,
        store: &mut Store,
        wire: &mut dyn Wire,
        id: u32,
        status: u32,
    ) -> Result<Step, Error> {
        let Some(index) = self.current(store) else {
            return self.send_next(store, wire);
        };
        let touch = self.touch;
        let Some(task) = store.tasks.get_mut(index) else {
            return self.send_next(store, wire);
        };
        match status {
            200..=299 => {
                let href = copy(task.href())?;
                task.set_server(&href, &kept_etag(wire, id))?;
                if task.touch == touch {
                    task.flags &= !flag::CHANGED;
                } else {
                    // Changed again meanwhile: that is sent in the next run.
                    task.skip = true;
                }
                self.report.sent += 1;
            }
            412 => self.server_wins(store, index, Some(wire.seed()))?,
            _ => {
                task.skip = true;
                self.report.failed += 1;
            }
        }
        self.save(store, wire)?;
        self.send_next(store, wire)
    }

    /// Nothing waits any more (or what waits failed this time).
    fn finish(&mut self, store: &mut Store, wire: &mut dyn Wire) -> Result<Step, Error> {
        if !self.again.is_empty() && !self.second {
            // The lists with a task changed on both sides, once more: the
            // server's version of it is taken now.
            self.second = true;
            for list in core::mem::take(&mut self.again) {
                push(&mut self.queue, (list, String::new()))?;
            }
            return self.next(store, wire);
        }
        if let Some(now) = wire.now() {
            store.last_sync = now;
        }
        self.save(store, wire)?;
        Ok(self.end(Outcome::Synced))
    }
}
