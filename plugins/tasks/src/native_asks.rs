//! The questions the screens ask and what their answers do: a task's title on
//! the keyboard, its day and priority from a short list, what else can be
//! done with a task, which list to show, a list's name, and the account's
//! address, name and password. One question is open at a time; its answer
//! finds its task again by the task's id, not by its place.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    events::{AnswerOutcome, Answered},
    formatting::Arg,
    settings::SettingValue,
    ui::{Question, RequestId, TextMode, TextWrite},
};
use paperpaw_ui_kit::{
    Messages,
    mem::{SERVICE_BUFFER, buffer, push, wipe},
    text::{concat, copy},
};

use crate::declarations::{ACCOUNT_TOKEN, ADDRESS, SCREEN_TOKEN, USER};
use crate::model::{NAME_MAX, TITLE_MAX, View as Tasks, clean, flag};
use crate::native_plugin::{ACCOUNT, NativeTasks, address, clock_now, notice, notify, open, seed};
use crate::native_screens::{self as screens, Row};
use crate::native_secret::{self as secret, PASSWORD, Secret};
use crate::when::parse_day;

/// What the lists' menu offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pick {
    Today,
    List(u8),
    New,
    Rename(u8),
    Remove(u8),
    Sync,
    Account,
}

/// What can be done with a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    Toggle,
    Title,
    Due,
    Priority,
    Delete,
}

/// What a question was for.
#[derive(Debug)]
pub(crate) enum Asked {
    /// Which list to show, or what to do with the lists.
    Lists(Vec<Pick>),
    /// The title of a new task.
    Add,
    /// The name of a new list, or a new name for the list.
    NewList,
    Rename(u8),
    /// Whether to remove the list.
    Remove(u8),
    /// What to do with the task with this id.
    Options(String, Vec<Act>),
    /// Its new title, its day (from the list, or typed), its priority.
    Title(String),
    Due(String),
    DueText(String),
    Priority(String),
    /// A row of the account.
    Field(Row),
}

/// The priorities offered, as iCalendar counts them: none, low, medium, high.
const PRIORITIES: [u8; 4] = [0, 9, 5, 1];

impl NativeTasks {
    /// Asks to choose one of `labels`.
    fn ask_choice(
        &mut self,
        host: &Host<'_>,
        title: &str,
        labels: &[String],
        asked: Asked,
    ) -> Result<(), Error> {
        if self.asked.is_some() {
            return Ok(());
        }
        let choices = paperpaw_ui_kit::mem::join(labels, "\n")?;
        let question = Question::choice(title, "", &choices)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = self
            .handles
            .ui(host)?
            .ask(host, question, &mut input, &mut out)?;
        self.asked = Some((request, asked));
        Ok(())
    }

    /// Asks for a text on the keyboard, with `initial` to start with.
    fn ask_text(
        &mut self,
        host: &Host<'_>,
        title: &str,
        initial: &str,
        asked: Asked,
    ) -> Result<(), Error> {
        if self.asked.is_some() {
            return Ok(());
        }
        let question = match asked {
            Asked::Field(Row::Password) => Question::text(title, "")?.secret()?,
            Asked::Field(Row::Address) => Question::text(title, "")?.mode(TextMode::Url)?,
            _ => Question::text(title, "")?,
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.handles.ui(host)?;
        let request = ui.ask(host, question, &mut input, &mut out)?;
        // The question stays open when the filling fails (the field is then
        // empty); it is tracked either way so its answer is still taken.
        if !initial.is_empty() {
            let _ = ui.text_write(
                host,
                request.text(),
                0,
                initial,
                TextWrite::Last,
                &mut input,
                &mut out,
            );
        }
        self.asked = Some((request, asked));
        Ok(())
    }

    /// What was typed for the question `request`, wiped when let go.
    fn typed(&mut self, host: &Host<'_>, request: RequestId) -> Result<Secret, Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.handles.ui(host)?;
        let text = match ui.text_read(host, request.text(), 0, &mut input, &mut out) {
            Ok(part) if part.data().len() as u64 == u64::from(part.total()) => {
                Secret::copy(part.data())
            }
            Ok(_) => Err(Error::NoSpace),
            Err(error) => Err(error),
        };
        wipe(&mut input);
        wipe(&mut out);
        text
    }

    /// The lists' menu: which to show, a new one, what to do with this one,
    /// *Sync now* and the account.
    pub(crate) fn ask_lists(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if !self.usable(host)? {
            return Ok(());
        }
        let mut say = Messages::new()?;
        let (mut picks, mut labels) = (Vec::new(), Vec::new());
        push(&mut picks, Pick::Today)?;
        push(&mut labels, say.copy(host, "tasks.view.today")?)?;
        for list in &self.store.lists {
            push(&mut picks, Pick::List(list.id))?;
            push(&mut labels, copy(&list.name)?)?;
        }
        push(&mut picks, Pick::New)?;
        push(&mut labels, say.copy(host, "tasks.lists.new")?)?;
        let shown = screens::shown(&self.store, self.shown.unwrap_or(Tasks::List(0)));
        // A list of the reader's own can be named anew and removed; one of
        // the server is as the server has it.
        if let Tasks::List(id) = shown
            && self.store.list(id).is_some_and(|list| !list.synced())
        {
            push(&mut picks, Pick::Rename(id))?;
            push(&mut labels, say.copy(host, "tasks.lists.rename")?)?;
            push(&mut picks, Pick::Remove(id))?;
            push(&mut labels, say.copy(host, "tasks.lists.remove")?)?;
        }
        if !address(host)?.is_empty() {
            push(&mut picks, Pick::Sync)?;
            push(&mut labels, say.copy(host, "tasks.action.sync")?)?;
        }
        push(&mut picks, Pick::Account)?;
        push(&mut labels, say.copy(host, "tasks.lists.account")?)?;
        let title = say.copy(host, "tasks.lists.title")?;
        self.ask_choice(host, &title, &labels, Asked::Lists(picks))
    }

    /// *Add a task*: asks for its title.
    pub(crate) fn ask_add(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if !self.usable(host)? {
            return Ok(());
        }
        let title = Messages::new()?.copy(host, "tasks.add")?;
        self.ask_text(host, &title, "", Asked::Add)
    }

    /// What can be done with the task at `index`.
    pub(crate) fn offer(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        if !self.usable(host)? {
            return Ok(());
        }
        let Some(task) = self.store.tasks.get(index) else {
            return Ok(());
        };
        if task.is(flag::FIXED) {
            notice(host, "tasks.fixed");
            return Ok(());
        }
        let mut say = Messages::new()?;
        let (mut acts, mut labels) = (Vec::new(), Vec::new());
        for (act, key) in [
            (
                Act::Toggle,
                if task.done() {
                    "tasks.option.undo"
                } else {
                    "tasks.option.done"
                },
            ),
            (Act::Title, "tasks.option.title"),
            (Act::Due, "tasks.option.due"),
            (Act::Priority, "tasks.option.priority"),
            (Act::Delete, "tasks.option.remove"),
        ] {
            push(&mut acts, act)?;
            push(&mut labels, say.copy(host, key)?)?;
        }
        let title = match task.title() {
            "" => say.copy(host, "tasks.untitled")?,
            title => copy(title)?,
        };
        let asked = Asked::Options(copy(task.uid())?, acts);
        self.ask_choice(host, &title, &labels, asked)
    }

    /// Asks for the text of a row of the account.
    pub(crate) fn ask_field(&mut self, host: &Host<'_>, row: Row) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let (key, initial) = match row {
            Row::Address => (
                "tasks.setting.address",
                crate::native_plugin::text_setting(host, ADDRESS)?,
            ),
            Row::User => (
                "tasks.setting.user",
                crate::native_plugin::text_setting(host, USER)?,
            ),
            // A password is never shown in its field.
            _ => ("tasks.account.password", String::new()),
        };
        let title = say.copy(host, key)?;
        self.ask_text(host, &title, &initial, Asked::Field(row))
    }

    /// An answer to a question the plugin asked.
    pub(crate) fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        if answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        if !matches!(&self.asked, Some((request, _)) if *request == answer.request()) {
            return Ok(());
        }
        let Some((request, asked)) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let choice = usize::try_from(answer.choice()).unwrap_or(usize::MAX);
        match asked {
            Asked::Lists(picks) => match picks.get(choice) {
                Some(pick) => self.picked(host, *pick),
                None => Ok(()),
            },
            Asked::Options(uid, acts) => match acts.get(choice) {
                Some(act) => self.act(host, &uid, *act),
                None => Ok(()),
            },
            Asked::Remove(id) => {
                if choice == 1 {
                    self.store.remove_list(id);
                    self.shown = None;
                    self.changed(host)?;
                }
                Ok(())
            }
            Asked::Due(uid) => self.due_chosen(host, uid, choice),
            Asked::Priority(uid) => {
                let (Some(index), Some(priority)) = (self.store.find(&uid), PRIORITIES.get(choice))
                else {
                    return Ok(());
                };
                let set = self.store.set_priority(index, *priority);
                self.after(host, set)
            }
            Asked::Field(row) => {
                let typed = self.typed(host, request)?;
                self.field(host, row, typed.as_str().trim())
            }
            typed_for => {
                let typed = self.typed(host, request)?;
                self.text_answered(host, typed_for, typed.as_str())
            }
        }
    }

    /// A change of a task was tried: kept and shown, or said why not.
    fn after(&mut self, host: &Host<'_>, set: Result<(), Error>) -> Result<(), Error> {
        match set {
            Ok(()) => self.changed(host),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(Error::Unsupported) => {
                notice(host, "tasks.fixed");
                Ok(())
            }
            Err(Error::Full) => {
                notice(host, "tasks.full");
                Ok(())
            }
            Err(_) => Ok(()),
        }
    }

    /// A choice of the lists' menu.
    fn picked(&mut self, host: &Host<'_>, pick: Pick) -> Result<(), Error> {
        let mut say = Messages::new()?;
        match pick {
            Pick::Today | Pick::List(_) => {
                self.shown = Some(match pick {
                    Pick::List(id) => Tasks::List(id),
                    _ => Tasks::Today,
                });
                self.refresh(host)?;
                // From the top of the list now shown.
                if let Some(top) = self.screens.top(SCREEN_TOKEN) {
                    top.list.set_focus(if top.rows.len() > 2 { 2 } else { 1 });
                }
                Ok(())
            }
            Pick::New => {
                let title = say.copy(host, "tasks.lists.name")?;
                self.ask_text(host, &title, "", Asked::NewList)
            }
            Pick::Rename(id) => {
                let title = say.copy(host, "tasks.lists.name")?;
                let name = copy(self.store.list(id).map_or("", |list| list.name.as_str()))?;
                self.ask_text(host, &title, &name, Asked::Rename(id))
            }
            Pick::Remove(id) => {
                let name = self.store.list(id).map_or("", |list| list.name.as_str());
                let title = say.format(
                    host,
                    "tasks.lists.remove.ask",
                    &[Arg::Text {
                        name: "name",
                        value: name,
                    }],
                )?;
                let labels = [
                    say.copy(host, "tasks.lists.remove.no")?,
                    say.copy(host, "tasks.lists.remove.yes")?,
                ];
                self.ask_choice(host, &title, &labels, Asked::Remove(id))
            }
            Pick::Sync => self.sync_now(host),
            Pick::Account => open(host, ACCOUNT),
        }
    }

    /// Something to do with the task with the id `uid`.
    fn act(&mut self, host: &Host<'_>, uid: &str, act: Act) -> Result<(), Error> {
        let Some(index) = self.store.find(uid) else {
            return Ok(());
        };
        let mut say = Messages::new()?;
        match act {
            Act::Toggle => self.toggle(host, index),
            Act::Delete => {
                let set = self.store.delete(index);
                self.after(host, set)
            }
            Act::Title => {
                let title = say.copy(host, "tasks.option.title")?;
                let old = copy(self.store.tasks.get(index).map_or("", |task| task.title()))?;
                self.ask_text(host, &title, &old, Asked::Title(copy(uid)?))
            }
            Act::Due => {
                let title = say.copy(host, "tasks.option.due")?;
                let mut labels = Vec::new();
                for key in [
                    "tasks.due.none",
                    "tasks.due.today",
                    "tasks.due.tomorrow",
                    "tasks.due.week",
                    "tasks.due.type",
                ] {
                    push(&mut labels, say.copy(host, key)?)?;
                }
                self.ask_choice(host, &title, &labels, Asked::Due(copy(uid)?))
            }
            Act::Priority => {
                let title = say.copy(host, "tasks.option.priority")?;
                let mut labels = Vec::new();
                for key in [
                    "tasks.priority.none",
                    "tasks.priority.low",
                    "tasks.priority.medium",
                    "tasks.priority.high",
                ] {
                    push(&mut labels, say.copy(host, key)?)?;
                }
                self.ask_choice(host, &title, &labels, Asked::Priority(copy(uid)?))
            }
        }
    }

    /// A day chosen from the list: none, today, tomorrow, in a week, or one
    /// to type.
    fn due_chosen(&mut self, host: &Host<'_>, uid: String, choice: usize) -> Result<(), Error> {
        let Some(index) = self.store.find(&uid) else {
            return Ok(());
        };
        let (now, today) = clock_now(host)?;
        let due = match choice {
            0 => None,
            4 => {
                let title = Messages::new()?.copy(host, "tasks.due.type.title")?;
                return self.ask_text(host, &title, "", Asked::DueText(uid));
            }
            _ if now.is_none() => {
                // Without a clock no day is today.
                notice(host, "tasks.no-clock");
                return Ok(());
            }
            1 => Some(today),
            2 => Some(today.saturating_add(1)),
            _ => Some(today.saturating_add(7)),
        };
        let set = self.store.set_due(index, due);
        self.after(host, set)
    }

    /// A text typed for a task or a list.
    fn text_answered(&mut self, host: &Host<'_>, asked: Asked, typed: &str) -> Result<(), Error> {
        match asked {
            Asked::Add => self.add(host, typed),
            Asked::NewList => {
                let name = clean(typed, NAME_MAX)?;
                if name.is_empty() {
                    return Ok(());
                }
                match self.store.add_list(&name, "") {
                    Ok(id) => {
                        self.shown = Some(Tasks::List(id));
                        self.changed(host)
                    }
                    Err(Error::Full) => {
                        notice(host, "tasks.lists.full");
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            }
            Asked::Rename(id) => {
                let name = clean(typed, NAME_MAX)?;
                let Some(list) = self.store.list_mut(id).filter(|_| !name.is_empty()) else {
                    return Ok(());
                };
                list.name = name;
                // Named by the reader's owner: theirs to keep.
                list.ctag.clear();
                self.changed(host)
            }
            Asked::Title(uid) => {
                let title = clean(typed, TITLE_MAX)?;
                let Some(index) = self.store.find(&uid).filter(|_| !title.is_empty()) else {
                    return Ok(());
                };
                let set = self.store.set_title(index, &title);
                self.after(host, set)
            }
            Asked::DueText(uid) => {
                let (now, today) = clock_now(host)?;
                let Some(index) = self.store.find(&uid) else {
                    return Ok(());
                };
                // A date with its year needs no clock; another does.
                let Some(day) =
                    parse_day(typed, today).filter(|_| now.is_some() || typed.len() > 6)
                else {
                    notice(host, "tasks.due.unknown");
                    return Ok(());
                };
                let set = self.store.set_due(index, Some(day));
                self.after(host, set)
            }
            _ => Ok(()),
        }
    }

    /// A new task with the title typed: in the list shown, or due today in
    /// the first list where what is due is shown.
    fn add(&mut self, host: &Host<'_>, typed: &str) -> Result<(), Error> {
        let title = clean(typed, TITLE_MAX)?;
        if title.is_empty() {
            return Ok(());
        }
        let (now, today) = clock_now(host)?;
        let shown = screens::shown(&self.store, self.shown.unwrap_or(Tasks::List(0)));
        let (list, due) = match shown {
            Tasks::List(id) => (Some(id), None),
            _ => (
                self.store.lists.first().map(|list| list.id),
                now.map(|_| today),
            ),
        };
        let Some(list) = list else {
            return Ok(());
        };
        let added = self.store.add_task(list, &title, due, seed(host));
        let uid = match added {
            Ok(index) => copy(self.store.tasks.get(index).map_or("", |task| task.uid()))?,
            Err(error) => return self.after(host, Err(error)),
        };
        self.changed(host)?;
        // The new task in sight.
        if let Some(top) = self.screens.top(SCREEN_TOKEN)
            && let Some(row) = top.row_of(&self.store, &uid)
        {
            top.list.set_focus(row);
        }
        Ok(())
    }

    /// What was typed for a row of the account: a setting, or the password.
    /// An empty answer for the password keeps what was kept (a confirm by
    /// mistake on the empty field loses nothing).
    fn field(&mut self, host: &Host<'_>, row: Row, typed: &str) -> Result<(), Error> {
        match row {
            Row::Address | Row::User => {
                let key = if row == Row::Address { ADDRESS } else { USER };
                let mut scratch = buffer(1200)?;
                CommandSender::new(host, &mut scratch)
                    .set_setting(key, SettingValue::Text(typed))?;
            }
            Row::Password if typed.is_empty() => return Ok(()),
            Row::Password => {
                self.keep_password(host, typed)?;
                self.account_changed();
            }
            _ => return Ok(()),
        }
        // On to the next row.
        if let Some(top) = self.screens.top(ACCOUNT_TOKEN) {
            let at = top.rows.iter().position(|r| *r == row).unwrap_or(0);
            top.list
                .set_focus((at + 1).min(top.rows.len().saturating_sub(1)));
        }
        self.refresh(host)
    }

    /// Keeps the password; said with the shared text where the device keeps
    /// no secrets.
    fn keep_password(&mut self, host: &Host<'_>, value: &str) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let label = say.copy(host, "tasks.secret")?;
        let kept = match self.handles.secrets(host) {
            Ok(secrets) => secret::keep(secrets, host, PASSWORD, &label, value),
            Err(error) => Err(error),
        };
        let key = match kept {
            Ok(()) => return Ok(()),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(Error::Unsupported | Error::Denied) => "core.secrets-unsupported",
            // A value too long, a full store: it was not kept.
            Err(_) => "tasks.account.not-kept",
        };
        let text = say.copy(host, key)?;
        notify(host, &concat(&[&text])?);
        Ok(())
    }
}
