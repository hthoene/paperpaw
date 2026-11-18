//! What the two screens show, as rows of the kit's list: the lists' screen
//! (which list, *Add a task*, then the tasks: open ones by their day, done
//! ones last) and the account's (address, name, password, *Sync now*). The
//! rows are built from the tasks in memory; nothing here reads the card.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{Error, Host, formatting::Arg};
use paperpaw_ui_kit::{
    Item, ItemList, Messages,
    mem::push,
    text::{concat, copy},
};

use crate::model::{Store, Task, View as Tasks};
use crate::native_widgets::due_text;

/// What a row of a screen is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// Which list is shown: choosing it offers the others.
    Lists,
    /// *Add a task*.
    Add,
    /// The task at this place of the store.
    Task(u16),
    /// The account's rows.
    Address,
    User,
    Password,
    Sync,
}

impl Row {
    /// The key of what an account's row is for.
    #[must_use]
    pub const fn help(self) -> &'static str {
        match self {
            Row::Address => "tasks.setting.address.help",
            Row::User => "tasks.setting.user.help",
            Row::Password => "tasks.account.password.help",
            Row::Sync => "tasks.account.sync.help",
            _ => "",
        }
    }
}

/// A screen that is open.
#[derive(Debug, Default)]
pub struct View {
    pub list: ItemList,
    pub rows: Vec<Row>,
    /// What an empty list says, and what is said below the rows.
    pub empty: (String, String),
    pub hint: String,
}

impl View {
    /// Takes the rows built anew, the focus where it was.
    pub fn set(&mut self, built: Built) {
        let focus = self.list.focus();
        self.list = ItemList::new(built.items);
        self.list
            .set_focus(focus.min(built.rows.len().saturating_sub(1)));
        self.rows = built.rows;
        self.empty = built.empty;
        self.hint = built.hint;
    }

    /// The row of the task with the id `uid`, if it shows.
    #[must_use]
    pub fn row_of(&self, store: &Store, uid: &str) -> Option<usize> {
        self.rows.iter().position(|row| match row {
            Row::Task(at) => store
                .tasks
                .get(usize::from(*at))
                .is_some_and(|task| task.uid() == uid),
            _ => false,
        })
    }
}

/// The rows of a screen.
#[derive(Debug, Default)]
pub struct Built {
    pub items: Vec<Item>,
    pub rows: Vec<Row>,
    pub empty: (String, String),
    pub hint: String,
}

/// What the lists' screen shows: a list that is there, or what is due.
#[must_use]
pub fn shown(store: &Store, wanted: Tasks) -> Tasks {
    match wanted {
        Tasks::List(id) if store.list(id).is_some() => wanted,
        Tasks::Today => wanted,
        _ => store
            .lists
            .first()
            .map_or(Tasks::Today, |list| Tasks::List(list.id)),
    }
}

/// The name of what is shown.
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn view_name(
    host: &Host<'_>,
    say: &mut Messages,
    store: &Store,
    view: Tasks,
) -> Result<String, Error> {
    match view {
        Tasks::List(id) => copy(store.list(id).map_or("", |list| list.name.as_str())),
        _ => say.copy(host, "tasks.view.today"),
    }
}

/// What a task's row says at its end: that it is done, or its day, with a
/// mark before it where its priority is high.
fn task_value(
    host: &Host<'_>,
    say: &mut Messages,
    task: &Task,
    today: i32,
) -> Result<String, Error> {
    if task.done() {
        return say.copy(host, "tasks.done");
    }
    let urgent = (1..=4).contains(&task.priority);
    let when = match task.due {
        Some(due) => due_text(host, say, due, today)?,
        None => String::new(),
    };
    match (urgent, when.is_empty()) {
        (true, true) => copy("!"),
        (true, false) => concat(&["! ", &when]),
        (false, _) => Ok(when),
    }
}

/// The rows of the lists' screen showing `view`. `syncing`: a run is under
/// way, said where the count of open tasks is otherwise.
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn main_rows(
    host: &Host<'_>,
    store: &Store,
    view: Tasks,
    (today, syncing): (i32, bool),
) -> Result<Built, Error> {
    let mut say = Messages::new()?;
    let mut built = Built::default();
    let name = view_name(host, &mut say, store, view)?;
    let open = i64::try_from(store.count_open(view, today)).unwrap_or(i64::MAX);
    let state = if syncing {
        say.copy(host, "tasks.syncing")?
    } else {
        say.format(host, "tasks.open", &[Arg::Count(open)])?
    };
    push(&mut built.items, Item::row(&name, &state)?)?;
    push(&mut built.rows, Row::Lists)?;
    push(
        &mut built.items,
        Item::row(&say.copy(host, "tasks.add")?, "+")?,
    )?;
    push(&mut built.rows, Row::Add)?;
    let untitled = say.copy(host, "tasks.untitled")?;
    for at in store.order(view, today)? {
        let Some(task) = store.tasks.get(usize::from(at)) else {
            continue;
        };
        let title = match task.title() {
            "" => untitled.as_str(),
            title => title,
        };
        let value = task_value(host, &mut say, task, today)?;
        push(&mut built.items, Item::row(title, &value)?)?;
        push(&mut built.rows, Row::Task(at))?;
    }
    built.empty = (
        say.copy(host, "tasks.empty.title")?,
        say.copy(
            host,
            if view == Tasks::Today {
                "tasks.empty.today"
            } else {
                "tasks.empty.text"
            },
        )?,
    );
    built.hint = say.copy(host, "tasks.hint")?;
    Ok(built)
}

/// What the account's screen knows of the account.
#[derive(Debug, Clone, Copy)]
pub struct Account<'a> {
    pub address: &'a str,
    pub user: &'a str,
    pub has_password: bool,
    /// How the last run went, in words; empty until one has.
    pub message: &'a str,
    pub syncing: bool,
}

/// What a kept password is shown as.
const DOTS: &str = "••••••";

/// The rows of the account's screen.
///
/// # Errors
/// Host refusals, `NoMemory`.
pub fn account_rows(host: &Host<'_>, store: &Store, account: &Account<'_>) -> Result<Built, Error> {
    let mut say = Messages::new()?;
    let mut built = Built::default();
    let address = account
        .address
        .strip_prefix("https://")
        .unwrap_or(account.address);
    for (row, key, value) in [
        (Row::Address, "tasks.setting.address", address),
        (Row::User, "tasks.setting.user", account.user),
        (
            Row::Password,
            "tasks.account.password",
            if account.has_password { DOTS } else { "" },
        ),
    ] {
        push(&mut built.items, Item::row(&say.copy(host, key)?, value)?)?;
        push(&mut built.rows, row)?;
    }
    let waiting = i64::try_from(store.waiting()).unwrap_or(i64::MAX);
    let note = if account.syncing {
        say.copy(host, "tasks.syncing")?
    } else if !account.message.is_empty() {
        copy(account.message)?
    } else if waiting > 0 {
        say.format(host, "tasks.account.waiting", &[Arg::Count(waiting)])?
    } else {
        say.copy(host, "tasks.account.idle")?
    };
    let title = say.copy(host, "tasks.action.sync")?;
    push(&mut built.items, Item::noted(&title, "›", &note)?)?;
    push(&mut built.rows, Row::Sync)?;
    Ok(built)
}
