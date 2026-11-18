//! The lists and their tasks, as the plugin keeps them in memory and writes
//! them to its file: no host, no clock, no card. A task is one small piece of
//! memory (its numbers and one text of its id, its place on the server, its
//! version there and its title), and there are at most [`MAX_TASKS`] of them
//! in at most [`MAX_LISTS`] lists, so the plugin's memory is bounded.
//!
//! What was changed on the reader and not yet sent is marked on the task
//! ([`flag`]): that is the queue of a reader that is offline.
use alloc::{string::String, vec::Vec};
use core::cmp::Ordering;

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::mem::{push, stable_sort_by};

/// The most lists kept.
pub const MAX_LISTS: usize = 12;
/// The most tasks kept, in all lists together.
pub const MAX_TASKS: usize = 200;
/// The most bytes of a task's title.
pub const TITLE_MAX: usize = 160;
/// The most bytes of a list's name.
pub const NAME_MAX: usize = 60;
/// The most bytes of a task's id, of its place on the server and of its
/// version there; a task of a server with more is not kept.
pub const UID_MAX: usize = 120;
pub const HREF_MAX: usize = 240;
/// The most bytes of the path of a list on the server: a task made on the
/// reader is put below it, and that path must fit too.
pub const LIST_HREF_MAX: usize = HREF_MAX - 64;
/// What marks, where a list of the server has its change tag, the list the
/// reader made by itself so that a task can be added at once. It goes when
/// the account's lists come and nothing was put into it.
pub const FIRST: &str = "first";
pub const ETAG_MAX: usize = 96;
/// The first line of the file.
pub const HEADER: &str = "tasks 1";

/// What is marked on a task.
pub mod flag {
    /// It is done.
    pub const DONE: u8 = 1;
    /// Made on the reader, not yet on the server.
    pub const NEW: u8 = 1 << 1;
    /// Removed on the reader, not yet on the server: it shows nowhere.
    pub const DELETED: u8 = 1 << 2;
    /// Changed on the reader and not yet on the server: its title, its day,
    /// its priority, whether it is done.
    pub const TITLE: u8 = 1 << 3;
    pub const DUE: u8 = 1 << 4;
    pub const PRIORITY: u8 = 1 << 5;
    pub const STATE: u8 = 1 << 6;
    /// It repeats: it is changed where it was made, not on the reader.
    pub const FIXED: u8 = 1 << 7;
    /// Any change that waits.
    pub const CHANGED: u8 = TITLE | DUE | PRIORITY | STATE;
    /// Anything that waits for the server.
    pub const PENDING: u8 = NEW | DELETED | CHANGED;
}

/// `text` on one line without blanks around it, at most `max` bytes: what a
/// title or a name is kept as. Tabs and line breaks become blanks, so a line
/// of the file is one thing.
///
/// # Errors
/// `NoMemory`.
pub fn clean(text: &str, max: usize) -> Result<String, Error> {
    let mut out = String::new();
    for word in text.split(|c: char| c.is_whitespace() || c.is_control()) {
        if word.is_empty() {
            continue;
        }
        let gap = usize::from(!out.is_empty());
        if out.len() + gap + word.len() > max {
            // The part of the word that still fits, never half a character.
            let room = max.saturating_sub(out.len() + gap);
            let end = (0..=room.min(word.len()))
                .rev()
                .find(|at| word.is_char_boundary(*at))
                .unwrap_or(0);
            if end > 0 {
                out.try_reserve(gap + end).map_err(|_| Error::NoMemory)?;
                out.push_str(if gap == 1 { " " } else { "" });
                out.push_str(word.get(..end).unwrap_or(""));
            }
            break;
        }
        out.try_reserve(gap + word.len())
            .map_err(|_| Error::NoMemory)?;
        out.push_str(if gap == 1 { " " } else { "" });
        out.push_str(word);
    }
    Ok(out)
}

/// Whether `text` can be a field of the file: no tab, no line break, no
/// other control character.
#[must_use]
pub fn plain(text: &str) -> bool {
    !text.chars().any(char::is_control)
}

/// A task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// The id of its list.
    pub list: u8,
    /// See [`flag`].
    pub flags: u8,
    /// 0 for none, 1 the highest, 9 the lowest (as iCalendar counts).
    pub priority: u8,
    /// The day it is due, in days since 1970 by the reader's clock.
    pub due: Option<i32>,
    /// When it was done, in seconds since 1970; 0 if not known.
    pub done_at: i64,
    /// Seen in the answer being read (while the lists are compared).
    pub seen: bool,
    /// Not sent in this run any more (it failed, or changed meanwhile).
    pub skip: bool,
    /// Counts the changes made on the reader, so that a change made while the
    /// task was being sent is not taken for sent.
    pub touch: u8,
    /// Its id, its place on the server, its version there and its title, with
    /// a tab between them: one piece of memory.
    text: String,
}

impl Task {
    /// A task of `list` with these texts.
    ///
    /// # Errors
    /// `NoMemory`; `Invalid` for a text that cannot be kept.
    pub fn new(list: u8, uid: &str, href: &str, etag: &str, title: &str) -> Result<Self, Error> {
        let mut task = Self {
            list,
            flags: 0,
            priority: 0,
            due: None,
            done_at: 0,
            seen: false,
            skip: false,
            touch: 0,
            text: String::new(),
        };
        task.set_text(uid, href, etag, title)?;
        Ok(task)
    }

    fn part(&self, n: usize) -> &str {
        self.text.splitn(4, '\t').nth(n).unwrap_or("")
    }

    /// Its id (`UID`).
    #[must_use]
    pub fn uid(&self) -> &str {
        self.part(0)
    }

    /// Its place on the server (a path); empty before it is there.
    #[must_use]
    pub fn href(&self) -> &str {
        self.part(1)
    }

    /// Its version on the server (the ETag); empty when not known.
    #[must_use]
    pub fn etag(&self) -> &str {
        self.part(2)
    }

    /// What is to do.
    #[must_use]
    pub fn title(&self) -> &str {
        self.part(3)
    }

    /// Sets its texts.
    ///
    /// # Errors
    /// `NoMemory`; `Invalid` for a text with a control character, an empty
    /// id or one of them too long.
    pub fn set_text(
        &mut self,
        uid: &str,
        href: &str,
        etag: &str,
        title: &str,
    ) -> Result<(), Error> {
        let fits = !uid.is_empty()
            && uid.len() <= UID_MAX
            && href.len() <= HREF_MAX
            && etag.len() <= ETAG_MAX
            && title.len() <= TITLE_MAX;
        if !fits || ![uid, href, etag, title].into_iter().all(plain) {
            return Err(Error::Invalid);
        }
        let mut text = String::new();
        text.try_reserve_exact(uid.len() + href.len() + etag.len() + title.len() + 3)
            .map_err(|_| Error::NoMemory)?;
        for (n, part) in [uid, href, etag, title].into_iter().enumerate() {
            if n > 0 {
                text.push('\t');
            }
            text.push_str(part);
        }
        self.text = text;
        Ok(())
    }

    /// Sets its place and version on the server.
    ///
    /// # Errors
    /// As [`Task::set_text`].
    pub fn set_server(&mut self, href: &str, etag: &str) -> Result<(), Error> {
        let old = core::mem::take(&mut self.text);
        let mut parts = old.splitn(4, '\t');
        let uid = parts.next().unwrap_or("");
        let title = parts.nth(2).unwrap_or("");
        let set = self.set_text(uid, href, etag, title);
        if set.is_err() {
            self.text = old;
        }
        set
    }

    /// Sets its title.
    ///
    /// # Errors
    /// As [`Task::set_text`].
    pub fn set_title(&mut self, title: &str) -> Result<(), Error> {
        let old = core::mem::take(&mut self.text);
        let mut parts = old.splitn(4, '\t');
        let (uid, href, etag) = (
            parts.next().unwrap_or(""),
            parts.next().unwrap_or(""),
            parts.next().unwrap_or(""),
        );
        let set = self.set_text(uid, href, etag, title);
        if set.is_err() {
            self.text = old;
        }
        set
    }

    /// Whether `flag` is marked.
    #[must_use]
    pub const fn is(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    /// Whether it is done.
    #[must_use]
    pub const fn done(&self) -> bool {
        self.is(flag::DONE)
    }

    /// Whether it shows: not removed.
    #[must_use]
    pub const fn shows(&self) -> bool {
        !self.is(flag::DELETED)
    }

    /// Whether it shows and is not done.
    #[must_use]
    pub const fn open(&self) -> bool {
        self.shows() && !self.done()
    }

    /// Whether something of it waits for the server.
    #[must_use]
    pub const fn pending(&self) -> bool {
        self.is(flag::PENDING)
    }

    /// Whether what it says is the same as `other` says.
    #[must_use]
    pub fn says_the_same(&self, title: &str, due: Option<i32>, priority: u8, done: bool) -> bool {
        self.title() == title && self.due == due && self.priority == priority && self.done() == done
    }
}

/// A list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct List {
    /// Its number in the file, which its tasks name.
    pub id: u8,
    pub name: String,
    /// The path of its collection on the server; empty for a list that is
    /// only on the reader.
    pub href: String,
    /// The collection's change tag when its tasks were read last.
    pub ctag: String,
}

impl List {
    /// Whether it is a list of the server.
    #[must_use]
    pub fn synced(&self) -> bool {
        !self.href.is_empty()
    }
}

/// Which tasks a view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The open tasks of all lists due today or before.
    Today,
    /// The tasks of one list, the done ones last.
    List(u8),
    /// The open tasks of all lists.
    Open,
}

/// What makes an id that no other task has: the times and a count.
#[derive(Debug, Clone, Copy, Default)]
pub struct Seed {
    /// Seconds since 1970, if the clock is set.
    pub unix: i64,
    /// The host's own clock.
    pub micros: u64,
}

/// The lists and tasks.
#[derive(Debug, Default)]
pub struct Store {
    pub lists: Vec<List>,
    pub tasks: Vec<Task>,
    /// The address the account's lists were found at.
    pub home: String,
    /// When the server was read last, in seconds since 1970.
    pub last_sync: i64,
    /// Counts the ids made.
    pub counter: u32,
}

/// A day as the file has it: its number, or `-` for none.
struct Day(Option<i32>);

impl core::fmt::Display for Day {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(day) => write!(f, "{day}"),
            None => f.write_str("-"),
        }
    }
}

/// Where a priority sorts: the highest first, none last.
const fn rank(priority: u8) -> u8 {
    if priority == 0 { 10 } else { priority }
}

impl Store {
    /// The list with `id`.
    #[must_use]
    pub fn list(&self, id: u8) -> Option<&List> {
        self.lists.iter().find(|list| list.id == id)
    }

    /// The list with `id`, to change.
    pub fn list_mut(&mut self, id: u8) -> Option<&mut List> {
        self.lists.iter_mut().find(|list| list.id == id)
    }

    /// The task with the id `uid`.
    #[must_use]
    pub fn find(&self, uid: &str) -> Option<usize> {
        self.tasks.iter().position(|task| task.uid() == uid)
    }

    /// The task of `list` with the id `uid`.
    #[must_use]
    pub fn find_in(&self, list: u8, uid: &str) -> Option<usize> {
        self.tasks
            .iter()
            .position(|task| task.list == list && task.uid() == uid)
    }

    /// The task at `index` becomes one the server never had: a new id (the
    /// old one may be another program's, and is the name of what the server
    /// removed), no place there, and all of it to send.
    ///
    /// # Errors
    /// `NotFound`; `NoMemory`.
    pub fn renew(&mut self, index: usize, seed: Seed) -> Result<(), Error> {
        let uid = self.new_uid(seed)?;
        let task = self.tasks.get_mut(index).ok_or(Error::NotFound)?;
        let title = paperpaw_ui_kit::text::copy(task.title())?;
        task.set_text(&uid, "", "", &title)?;
        task.flags = flag::NEW | (task.flags & flag::DONE);
        task.touch = task.touch.wrapping_add(1);
        Ok(())
    }

    /// Adds a list; its id.
    ///
    /// # Errors
    /// `Full` when there are [`MAX_LISTS`]; `NoMemory`; `Invalid`.
    pub fn add_list(&mut self, name: &str, href: &str) -> Result<u8, Error> {
        if self.lists.len() >= MAX_LISTS {
            return Err(Error::Full);
        }
        if !plain(href) || href.len() > LIST_HREF_MAX {
            return Err(Error::Invalid);
        }
        let id = (1..=u8::MAX)
            .find(|id| self.list(*id).is_none())
            .ok_or(Error::Full)?;
        let list = List {
            id,
            name: clean(name, NAME_MAX)?,
            href: paperpaw_ui_kit::text::copy(href)?,
            ctag: String::new(),
        };
        push(&mut self.lists, list)?;
        Ok(id)
    }

    /// Removes a list and its tasks.
    pub fn remove_list(&mut self, id: u8) {
        self.lists.retain(|list| list.id != id);
        self.tasks.retain(|task| task.list != id);
    }

    /// Whether the task at `index` is of a list of the server.
    fn synced(&self, index: usize) -> bool {
        self.tasks
            .get(index)
            .and_then(|task| self.list(task.list))
            .is_some_and(List::synced)
    }

    /// An id for a new task.
    pub(crate) fn new_uid(&mut self, seed: Seed) -> Result<String, Error> {
        self.counter = self.counter.wrapping_add(1);
        paperpaw_ui_kit::text::format(format_args!(
            "pp-{:x}-{:x}-{:x}",
            seed.unix, seed.micros, self.counter
        ))
    }

    /// Adds an open task to `list`; where it is.
    ///
    /// # Errors
    /// `Full` when there are [`MAX_TASKS`]; `NotFound` for no such list;
    /// `NoMemory`.
    pub fn add_task(
        &mut self,
        list: u8,
        title: &str,
        due: Option<i32>,
        seed: Seed,
    ) -> Result<usize, Error> {
        if self.tasks.len() >= MAX_TASKS {
            return Err(Error::Full);
        }
        let synced = self.list(list).ok_or(Error::NotFound)?.synced();
        let uid = self.new_uid(seed)?;
        let mut task = Task::new(list, &uid, "", "", &clean(title, TITLE_MAX)?)?;
        task.due = due;
        if synced {
            task.flags = flag::NEW;
        }
        push(&mut self.tasks, task)?;
        Ok(self.tasks.len() - 1)
    }

    /// Marks `changed` on the task at `index`, if the server is to know.
    fn touched(&mut self, index: usize, changed: u8) {
        let synced = self.synced(index);
        if let Some(task) = self.tasks.get_mut(index) {
            task.touch = task.touch.wrapping_add(1);
            task.skip = false;
            if synced && !task.is(flag::NEW) {
                task.flags |= changed;
            }
        }
    }

    /// Ticks the task at `index` off, or on again; `now` in seconds since 1970
    /// (0 if the clock is not set).
    ///
    /// # Errors
    /// `NotFound`; `Unsupported` for a task that repeats.
    pub fn toggle(&mut self, index: usize, now: i64) -> Result<(), Error> {
        let task = self.tasks.get_mut(index).ok_or(Error::NotFound)?;
        if task.is(flag::FIXED) {
            return Err(Error::Unsupported);
        }
        task.flags ^= flag::DONE;
        task.done_at = if task.done() { now } else { 0 };
        self.touched(index, flag::STATE);
        Ok(())
    }

    /// Sets the title of the task at `index`.
    ///
    /// # Errors
    /// `NotFound`; `Unsupported` for a task that repeats; `NoMemory`.
    pub fn set_title(&mut self, index: usize, title: &str) -> Result<(), Error> {
        let task = self.tasks.get_mut(index).ok_or(Error::NotFound)?;
        if task.is(flag::FIXED) {
            return Err(Error::Unsupported);
        }
        let title = clean(title, TITLE_MAX)?;
        if task.title() != title {
            task.set_title(&title)?;
            self.touched(index, flag::TITLE);
        }
        Ok(())
    }

    /// Sets the day the task at `index` is due.
    ///
    /// # Errors
    /// `NotFound`; `Unsupported` for a task that repeats.
    pub fn set_due(&mut self, index: usize, due: Option<i32>) -> Result<(), Error> {
        let task = self.tasks.get_mut(index).ok_or(Error::NotFound)?;
        if task.is(flag::FIXED) {
            return Err(Error::Unsupported);
        }
        if task.due != due {
            task.due = due;
            self.touched(index, flag::DUE);
        }
        Ok(())
    }

    /// Sets the priority of the task at `index`.
    ///
    /// # Errors
    /// `NotFound`; `Unsupported` for a task that repeats.
    pub fn set_priority(&mut self, index: usize, priority: u8) -> Result<(), Error> {
        let task = self.tasks.get_mut(index).ok_or(Error::NotFound)?;
        if task.is(flag::FIXED) {
            return Err(Error::Unsupported);
        }
        if task.priority != priority.min(9) {
            task.priority = priority.min(9);
            self.touched(index, flag::PRIORITY);
        }
        Ok(())
    }

    /// Removes the task at `index`: at once where the server never had it,
    /// else it is marked and goes when the server has removed it.
    ///
    /// # Errors
    /// `NotFound`.
    pub fn delete(&mut self, index: usize) -> Result<(), Error> {
        let synced = self.synced(index);
        let task = self.tasks.get_mut(index).ok_or(Error::NotFound)?;
        if synced && !task.href().is_empty() {
            task.flags = (task.flags & !flag::CHANGED) | flag::DELETED;
            task.touch = task.touch.wrapping_add(1);
            task.skip = false;
        } else {
            self.tasks.remove(index);
        }
        Ok(())
    }

    /// Lets go of the tasks done more than `keep_days` before `today` (days
    /// since 1970) that the server knows of or that are only on the reader;
    /// whether any went. One whose time is not known counts from today on.
    pub fn purge(&mut self, today: i32, keep_days: i32) -> bool {
        let before = self.tasks.len();
        let start = i64::from(today) * 86_400;
        for task in &mut self.tasks {
            if task.done() && task.done_at == 0 {
                task.done_at = start;
            }
        }
        self.tasks.retain(|task| {
            let day = task.done_at.div_euclid(86_400);
            !(task.done() && !task.pending() && day + i64::from(keep_days) < i64::from(today))
        });
        self.tasks.len() != before
    }

    /// How the tasks of a view follow each other: the open ones by their day
    /// (those with none last), then by priority, then by title; the done ones
    /// after them, the last done first.
    fn compare(a: &Task, b: &Task) -> Ordering {
        a.done().cmp(&b.done()).then_with(|| {
            if a.done() {
                return b.done_at.cmp(&a.done_at);
            }
            a.due
                .unwrap_or(i32::MAX)
                .cmp(&b.due.unwrap_or(i32::MAX))
                .then(rank(a.priority).cmp(&rank(b.priority)))
                .then_with(|| paperpaw_ui_kit::text::cmp_lowercase(a.title(), b.title()))
        })
    }

    fn in_view(task: &Task, view: View, today: i32) -> bool {
        task.shows()
            && match view {
                View::Today => !task.done() && task.due.is_some_and(|due| due <= today),
                View::List(id) => task.list == id,
                View::Open => !task.done(),
            }
    }

    /// The tasks of `view` in their order, as places in [`Store::tasks`].
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn order(&self, view: View, today: i32) -> Result<Vec<u16>, Error> {
        let mut found = Vec::new();
        for (index, task) in self.tasks.iter().enumerate() {
            if Self::in_view(task, view, today) {
                push(&mut found, u16::try_from(index).unwrap_or(u16::MAX))?;
            }
        }
        stable_sort_by(&mut found, |a, b| {
            match (
                self.tasks.get(usize::from(*a)),
                self.tasks.get(usize::from(*b)),
            ) {
                (Some(a), Some(b)) => Self::compare(a, b),
                _ => Ordering::Equal,
            }
        });
        Ok(found)
    }

    /// How many tasks `view` shows that are not done.
    #[must_use]
    pub fn count_open(&self, view: View, today: i32) -> usize {
        self.tasks
            .iter()
            .filter(|task| task.open() && Self::in_view(task, view, today))
            .count()
    }

    /// The task to do next: the first of the open ones.
    #[must_use]
    pub fn next(&self, today: i32) -> Option<usize> {
        self.tasks
            .iter()
            .enumerate()
            .filter(|(_, task)| Self::in_view(task, View::Open, today))
            .min_by(|(_, a), (_, b)| Self::compare(a, b))
            .map(|(index, _)| index)
    }

    /// How many tasks wait for the server.
    #[must_use]
    pub fn waiting(&self) -> usize {
        (0..self.tasks.len())
            .filter(|index| {
                self.synced(*index) && self.tasks.get(*index).is_some_and(Task::pending)
            })
            .count()
    }

    /// Whether any list is of the server.
    #[must_use]
    pub fn has_synced(&self) -> bool {
        self.lists.iter().any(List::synced)
    }

    /// The server is another one (or none): what was known of it is let go.
    /// The lists stay until the new server is read.
    pub fn forget_server(&mut self) {
        self.home.clear();
        for list in self.lists.iter_mut().filter(|list| list.synced()) {
            list.ctag.clear();
        }
    }

    // The file.

    /// Gives `out` the file's lines, each with its line break.
    ///
    /// # Errors
    /// `NoMemory`, or what `out` says.
    pub fn write(&self, out: &mut dyn FnMut(&str) -> Result<(), Error>) -> Result<(), Error> {
        use paperpaw_ui_kit::text::format;
        out(HEADER)?;
        out("\n")?;
        out(&format(format_args!(
            "S\t{}\t{}\t{}\n",
            self.last_sync, self.counter, self.home
        ))?)?;
        for list in &self.lists {
            out(&format(format_args!(
                "L\t{}\t{}\t{}\t{}\n",
                list.id, list.href, list.ctag, list.name
            ))?)?;
        }
        for task in &self.tasks {
            out(&format(format_args!(
                "T\t{}\t{}\t{}\t{}\t{}\t",
                task.list,
                task.flags,
                task.priority,
                Day(task.due),
                task.done_at
            ))?)?;
            out(&task.text)?;
            out("\n")?;
        }
        out(&format(format_args!(
            "E\t{}\n",
            self.lists.len() + self.tasks.len()
        ))?)
    }

    /// Takes a line of the file after its first; `Some(n)` for the last line,
    /// which says how many lists and tasks there were. A line that is not
    /// understood is passed over.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn read_line(&mut self, line: &str) -> Result<Option<usize>, Error> {
        let (kind, rest) = line.split_once('\t').unwrap_or((line, ""));
        match kind {
            "S" => {
                let mut parts = rest.splitn(3, '\t');
                self.last_sync = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                self.counter = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                self.home = paperpaw_ui_kit::text::copy(parts.next().unwrap_or(""))?;
            }
            "L" => {
                let mut parts = rest.splitn(4, '\t');
                let id = parts.next().and_then(|n| n.parse::<u8>().ok());
                let (href, ctag) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                let name = parts.next().unwrap_or("");
                if let Some(id) = id.filter(|id| *id > 0 && self.list(*id).is_none())
                    && self.lists.len() < MAX_LISTS
                {
                    let list = List {
                        id,
                        name: clean(name, NAME_MAX)?,
                        href: paperpaw_ui_kit::text::copy(href)?,
                        ctag: paperpaw_ui_kit::text::copy(ctag)?,
                    };
                    push(&mut self.lists, list)?;
                }
            }
            "T" => {
                if let Some(task) = Self::read_task(rest)?
                    && self.tasks.len() < MAX_TASKS
                {
                    push(&mut self.tasks, task)?;
                }
            }
            "E" => return Ok(Some(rest.trim().parse().unwrap_or(usize::MAX))),
            _ => {}
        }
        Ok(None)
    }

    fn read_task(rest: &str) -> Result<Option<Task>, Error> {
        let mut parts = rest.splitn(9, '\t');
        let mut number = || parts.next().and_then(|n| n.parse::<i64>().ok());
        let (Some(list), Some(flags), Some(priority)) = (number(), number(), number()) else {
            return Ok(None);
        };
        let due = parts.next().and_then(|n| n.parse::<i32>().ok());
        let done_at = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        let (uid, href, etag) = (
            parts.next().unwrap_or(""),
            parts.next().unwrap_or(""),
            parts.next().unwrap_or(""),
        );
        let title = parts.next().unwrap_or("");
        let (Ok(list), Ok(flags)) = (u8::try_from(list), u8::try_from(flags)) else {
            return Ok(None);
        };
        let mut task = match Task::new(list, uid, href, etag, title) {
            Ok(task) => task,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(None),
        };
        task.flags = flags;
        task.priority = u8::try_from(priority.clamp(0, 9)).unwrap_or(0);
        task.due = due;
        task.done_at = done_at;
        Ok(Some(task))
    }

    /// Drops what belongs to no list (a file changed by hand).
    pub fn tidy(&mut self) {
        let lists = &self.lists;
        self.tasks
            .retain(|task| lists.iter().any(|list| list.id == task.list));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;
    use alloc::vec;

    fn seed() -> Seed {
        Seed {
            unix: 1_795_000_000,
            micros: 12_345,
        }
    }

    fn store() -> Store {
        let mut store = Store::default();
        let local = store.add_list("Home", "").unwrap();
        let synced = store.add_list("Work", "/dav/work/").unwrap();
        assert_eq!((local, synced), (1, 2));
        store
    }

    fn titles(store: &Store, view: View, today: i32) -> Vec<String> {
        store
            .order(view, today)
            .unwrap()
            .iter()
            .map(|at| store.tasks[usize::from(*at)].title().into())
            .collect()
    }

    #[test]
    fn a_title_is_one_line_of_a_bounded_length() {
        assert_eq!(
            clean("  buy\tmilk \n and  bread ", 100).unwrap(),
            "buy milk and bread"
        );
        assert_eq!(clean("abc def", 5).unwrap(), "abc d");
        assert_eq!(
            clean("caf\u{e9}s", 4).unwrap(),
            "caf",
            "never half a character"
        );
        assert_eq!(clean("\u{7}\n", 10).unwrap(), "");
        assert!(plain("a b") && !plain("a\tb") && !plain("a\nb"));
    }

    #[test]
    fn a_task_keeps_its_four_texts_in_one() {
        let mut task = Task::new(1, "u1", "/c/u1.ics", "\"e1\"", "Buy milk").unwrap();
        assert_eq!(
            (task.uid(), task.href(), task.etag(), task.title()),
            ("u1", "/c/u1.ics", "\"e1\"", "Buy milk")
        );
        task.set_title("Buy bread").unwrap();
        task.set_server("/c/x.ics", "").unwrap();
        assert_eq!(
            (task.uid(), task.href(), task.etag(), task.title()),
            ("u1", "/c/x.ics", "", "Buy bread")
        );
        // What cannot be a field is refused and nothing changes.
        assert_eq!(task.set_title("a\tb"), Err(Error::Invalid));
        assert_eq!(
            task.set_server(&"x".repeat(HREF_MAX + 1), ""),
            Err(Error::Invalid)
        );
        assert_eq!(task.title(), "Buy bread");
        assert_eq!(Task::new(1, "", "", "", "x").unwrap_err(), Error::Invalid);
    }

    #[test]
    fn changes_wait_only_where_a_server_is_to_know() {
        let mut store = store();
        let local = store.add_task(1, "local", None, seed()).unwrap();
        let new = store.add_task(2, "new", None, seed()).unwrap();
        assert_ne!(store.tasks[local].uid(), store.tasks[new].uid());
        assert_eq!(store.tasks[local].flags, 0);
        assert_eq!(store.tasks[new].flags, flag::NEW);
        store.toggle(local, 5).unwrap();
        store.toggle(new, 5).unwrap();
        assert_eq!(store.tasks[local].flags, flag::DONE);
        assert_eq!(
            store.tasks[new].flags,
            flag::NEW | flag::DONE,
            "a new task is sent whole"
        );
        // One the server has: each change is marked.
        let mut known = Task::new(2, "k", "/dav/work/k.ics", "\"1\"", "known").unwrap();
        known.priority = 5;
        store.tasks.push(known);
        let known = store.tasks.len() - 1;
        store.toggle(known, 7).unwrap();
        store.set_due(known, Some(100)).unwrap();
        store.set_priority(known, 5).unwrap();
        assert_eq!(
            store.tasks[known].flags,
            flag::DONE | flag::STATE | flag::DUE
        );
        store.set_title(known, " renamed ").unwrap();
        store.set_priority(known, 1).unwrap();
        assert_eq!(store.tasks[known].title(), "renamed");
        assert!(store.tasks[known].is(flag::TITLE | flag::PRIORITY));
        assert_eq!(store.waiting(), 2);
        // Removed: the new one goes at once, the known one waits marked.
        store.delete(known).unwrap();
        assert_eq!(store.tasks[known].flags & flag::PENDING, flag::DELETED);
        assert!(!store.tasks[known].shows());
        store.delete(new).unwrap();
        store.delete(local).unwrap();
        assert_eq!(store.tasks.len(), 1);
    }

    #[test]
    fn a_task_that_repeats_is_not_changed_here() {
        let mut store = store();
        let mut task = Task::new(2, "r", "/dav/work/r.ics", "1", "daily").unwrap();
        task.flags = flag::FIXED;
        store.tasks.push(task);
        assert_eq!(store.toggle(0, 1), Err(Error::Unsupported));
        assert_eq!(store.set_title(0, "x"), Err(Error::Unsupported));
        assert_eq!(store.set_due(0, Some(1)), Err(Error::Unsupported));
        assert_eq!(store.tasks[0].flags, flag::FIXED);
    }

    #[test]
    fn the_order_is_day_priority_title_and_done_last() {
        let mut store = store();
        for (title, due, priority) in [
            ("no day", None, 0),
            ("tomorrow", Some(101), 0),
            ("today low", Some(100), 9),
            ("today high", Some(100), 1),
            ("overdue", Some(98), 0),
            ("Today high too", Some(100), 1),
        ] {
            let at = store.add_task(1, title, due, seed()).unwrap();
            store.set_priority(at, priority).unwrap();
        }
        let done = store.add_task(1, "done", Some(90), seed()).unwrap();
        store.toggle(done, 1000).unwrap();
        assert_eq!(
            titles(&store, View::List(1), 100),
            [
                "overdue",
                "today high",
                "Today high too",
                "today low",
                "tomorrow",
                "no day",
                "done"
            ]
        );
        assert_eq!(
            titles(&store, View::Today, 100),
            ["overdue", "today high", "Today high too", "today low"]
        );
        assert_eq!(store.count_open(View::List(1), 100), 6);
        assert_eq!(store.count_open(View::Today, 100), 4);
        assert_eq!(store.tasks[store.next(100).unwrap()].title(), "overdue");
        assert_eq!(titles(&store, View::List(2), 100), Vec::<String>::new());
    }

    #[test]
    fn done_tasks_go_after_their_days_unless_they_wait() {
        let mut store = store();
        let old = store.add_task(1, "old", None, seed()).unwrap();
        store.toggle(old, 10 * 86_400).unwrap();
        let fresh = store.add_task(1, "fresh", None, seed()).unwrap();
        store.toggle(fresh, 17 * 86_400).unwrap();
        let waits = store.add_task(2, "waits", None, seed()).unwrap();
        store.toggle(waits, 86_400).unwrap();
        let unknown = store.add_task(1, "no clock", None, seed()).unwrap();
        store.toggle(unknown, 0).unwrap();
        store.add_task(1, "open", None, seed()).unwrap();
        assert!(store.purge(18, 7));
        assert_eq!(
            store.tasks.iter().map(Task::title).collect::<Vec<_>>(),
            ["fresh", "waits", "no clock", "open"]
        );
        // The one without a time counts from the day it was first seen.
        assert!(!store.purge(24, 7));
        assert!(store.purge(26, 7));
        assert_eq!(store.tasks.len(), 2);
    }

    #[test]
    fn the_file_reads_back_as_it_was_written() {
        let mut store = store();
        store.home = "https://h/dav/".into();
        store.last_sync = 1_795_000_000;
        store.lists[1].ctag = "\"ct|1\"".into();
        let at = store
            .add_task(1, "Größe | mit Strich", Some(20_777), seed())
            .unwrap();
        store.set_priority(at, 5).unwrap();
        let mut known = Task::new(2, "k@x", "/dav/work/k.ics", "W/\"1\"", "").unwrap();
        known.flags = flag::DONE | flag::STATE;
        known.done_at = 1_795_000_001;
        store.tasks.push(known);
        for task in &mut store.tasks {
            task.touch = 0;
        }
        let mut text = String::new();
        store
            .write(&mut |piece| {
                text.push_str(piece);
                Ok(())
            })
            .unwrap();
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some(HEADER));
        let mut back = Store::default();
        let mut end = None;
        for line in lines {
            end = back.read_line(line).unwrap().or(end);
        }
        assert_eq!(end, Some(4));
        assert_eq!(back.lists, store.lists);
        assert_eq!(back.tasks, store.tasks);
        assert_eq!(
            (back.home.as_str(), back.last_sync, back.counter),
            ("https://h/dav/", 1_795_000_000, 1)
        );
    }

    #[test]
    fn lines_that_are_not_understood_are_passed_over() {
        let mut store = Store::default();
        for line in vec![
            "L\t1\t\t\tHome",
            "L\t1\t\t\tAgain the same number",
            "L\tx\t\t\tNo number",
            "T\t1\t0\t0\t-\t0\tu1\t\t\tgood",
            "T\t1\tx\t0\t-\t0\tu2\t\t\tbad flags",
            "T\t1\t0\t0\t-\t0\t\t\t\tno id",
            "T\t9\t0\t0\t-\t0\tu3\t\t\tno such list",
            "X\twhat",
            "",
        ] {
            assert_eq!(store.read_line(line).unwrap(), None);
        }
        store.tidy();
        assert_eq!(store.lists.len(), 1);
        assert_eq!(
            store.tasks.iter().map(Task::title).collect::<Vec<_>>(),
            ["good"]
        );
        assert_eq!(store.read_line("E\t2").unwrap(), Some(2));
    }

    #[test]
    fn the_lists_and_tasks_are_bounded() {
        let mut store = Store::default();
        for n in 0..MAX_LISTS {
            store.add_list(&alloc::format!("list {n}"), "").unwrap();
        }
        assert_eq!(store.add_list("one more", ""), Err(Error::Full));
        for n in 0..MAX_TASKS {
            store
                .add_task(1, &alloc::format!("task {n}"), None, seed())
                .unwrap();
        }
        assert_eq!(
            store.add_task(1, "one more", None, seed()),
            Err(Error::Full)
        );
        store.remove_list(1);
        assert!(store.tasks.is_empty());
        assert_eq!(
            store.add_list("again", "").unwrap(),
            1,
            "a free number is taken again"
        );
    }
}
