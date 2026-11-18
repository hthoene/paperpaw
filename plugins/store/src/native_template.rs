//! Setup templates (#72): a ready-made set of plugins and settings the setup
//! guide offers right after the install, made by PaperPaw (no templates of
//! the reader's own yet).
//!
//! A template is a row of [`TEMPLATES`], a table in the program (a data plugin
//! would cost a plugin on every device, a file read at the first start and a
//! second admission; the table is read-only text in the image). It lists the
//! plugins of the library or built in that it needs and the settings it sets.
//!
//! Choosing one (the rules here, the page is
//! [`native_screen_templates`](crate::native_screen_templates)):
//! - its settings are written at once (`command.set_setting`, only settings
//!   the device has now: those of the reader, the home screen and the
//!   device; a plugin that is only added keeps its own defaults);
//! - built-in plugins it names are switched on, the others fetched from the
//!   library the way the first-run offer fetches them ([`choose`]);
//! - where the device is not online they are kept in [`PENDING`] and say so;
//!   the next contact with the library (the start while online, a list that
//!   came) puts them into the queue ([`release`]). Plugins they require come
//!   with them (the store's `requires` pass after each fetch).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Storage;
use paperpaw_sdk::install::is_plugin_name;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{mem::push, text::copy};

use crate::native_ask::{Chosen, Have, choose};
use crate::native_queue::{Mark, Queue, join_names, read_text};

/// The template chosen (its id), once chosen: the page shows it.
pub const CHOSEN: &str = "template-chosen.txt";
/// The plugins of a template waiting for a connection, an id a line.
pub const PENDING: &str = "template-pending.txt";
/// The most of [`PENDING`] read.
const MAX_PENDING: usize = 1024;

/// A setting's value, as a template sets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Value {
    /// A switch.
    Bool(bool),
    /// A text (a list's items, a choice's value).
    Text(&'static str),
}

/// A template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Template {
    /// Its id (the file `CHOSEN` keeps it).
    pub id: &'static str,
    /// The catalog keys of its name and its one line.
    pub name: &'static str,
    pub help: &'static str,
    /// The plugins it needs, by id.
    pub plugins: &'static [&'static str],
    /// The settings it sets: full key, value.
    pub settings: &'static [(&'static str, Value)],
}

/// The status line of the reader without its page number's company.
const CLEAN: &[(&str, Value)] = &[
    ("home.footer.pencil", Value::Bool(false)),
    ("home.footer.dots", Value::Bool(false)),
    ("reader.status.bottom-left", Value::Text("")),
    ("reader.status.bottom-center", Value::Text("")),
];

/// What reading statistics like to see: where one is and the time.
const TIMED: &[(&str, Value)] = &[
    (
        "reader.status.bottom-left",
        Value::Text("reader.status.chapter"),
    ),
    (
        "reader.status.bottom-center",
        Value::Text("core.status.clock"),
    ),
];

/// The templates, in the order of the page.
pub const TEMPLATES: [Template; 4] = [
    Template {
        id: "minimal",
        name: "store.template.minimal",
        help: "store.template.minimal.help",
        plugins: &[],
        settings: CLEAN,
    },
    Template {
        id: "stats",
        name: "store.template.stats",
        help: "store.template.stats.help",
        plugins: &["stats", "reading-goal", "end-of-book"],
        settings: TIMED,
    },
    Template {
        id: "language",
        name: "store.template.language",
        help: "store.template.language.help",
        plugins: &["vocabulary", "anki", "translate"],
        settings: &[],
    },
    Template {
        id: "notes",
        name: "store.template.notes",
        help: "store.template.notes.help",
        plugins: &["notes-vault", "notes-overview"],
        settings: &[],
    },
];

/// The template `id`, if it is one.
#[must_use]
pub fn find(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|template| template.id == id)
}

/// What choosing a template does besides its settings.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Plan {
    /// Built in and off: switch them on.
    pub switch_on: Vec<&'static str>,
    /// To fetch from the library (kept in the queue, or in [`PENDING`] where
    /// `later`).
    pub fetch: Vec<String>,
    /// The device is not online: the plugins to fetch wait for a connection.
    pub later: bool,
    /// The notice to say once; empty for none.
    pub notify: &'static str,
}

/// What becomes of `template` on a device that has `have` of each plugin;
/// `can_install` says it loads code plugins, `online` that it is connected.
///
/// # Errors
/// `NoMemory`.
pub fn plan(
    template: &Template,
    have: &dyn Fn(&str) -> Have,
    can_install: bool,
    online: bool,
) -> Result<Plan, Error> {
    let Chosen {
        switch_on, fetch, ..
    } = choose(template.plugins, have, can_install)?;
    let later = !fetch.is_empty() && !online;
    let notify = if later {
        "store.template.later"
    } else if !fetch.is_empty() {
        "store.template.adding"
    } else if !switch_on.is_empty() {
        "store.template.switched-on"
    } else {
        "store.template.applied"
    };
    Ok(Plan {
        switch_on,
        fetch,
        later,
        notify,
    })
}

/// Keeps what choosing `template` decided: the template chosen and the
/// plugins to fetch, in the queue (quietly, the caller starts it) or, where
/// `plan.later`, in [`PENDING`]. The first-run offer's note that code plugins
/// were explained is made too.
///
/// # Errors
/// `NoMemory`; `Io` where the card refuses the queue or the pending list.
pub fn keep(storage: &dyn Storage, template: &Template, plan: &Plan) -> Result<(), Error> {
    match storage.write(CHOSEN, template.id.as_bytes()) {
        Ok(()) | Err(_) => {}
    }
    if plan.fetch.is_empty() {
        return Ok(());
    }
    match storage.write(crate::native_ask::CODE_TOLD, b"") {
        Ok(()) | Err(_) => {}
    }
    if plan.later {
        let text = join_names(plan.fetch.iter().map(String::as_str), "\n")?;
        return storage
            .write(PENDING, text.as_bytes())
            .map_err(|_| Error::Io);
    }
    crate::native_ask::add_quietly(storage, &plan.fetch)
}

/// The template chosen earlier, if its file is there and names one.
///
/// # Errors
/// `NoMemory`.
pub fn chosen(storage: &dyn Storage) -> Result<Option<&'static Template>, Error> {
    Ok(read_text(storage, CHOSEN, 64)?.and_then(|id| find(id.trim())))
}

/// Whether plugins wait for a connection.
#[must_use]
pub fn waiting(storage: &dyn Storage) -> bool {
    storage.exists(PENDING)
}

/// The plugins that wait for a connection (ids, as written).
///
/// # Errors
/// `NoMemory`.
pub fn pending(storage: &dyn Storage) -> Result<Vec<String>, Error> {
    let mut ids = Vec::new();
    if let Some(text) = read_text(storage, PENDING, MAX_PENDING)? {
        for line in text.lines().filter(|line| is_plugin_name(line)) {
            push(&mut ids, copy(line)?)?;
        }
    }
    Ok(ids)
}

/// The device is online (or the library was reached): the plugins of
/// [`pending`] that are still `missing` go into the queue, quietly (one that
/// came another way is not fetched again), and the list is gone either way, so
/// a plugin the library does not have is tried once. `true` where the queue
/// holds quiet plugins now (the caller starts it).
///
/// # Errors
/// `NoMemory`; `Io` where the card refuses the queue.
pub fn release(storage: &dyn Storage, missing: &[&str]) -> Result<bool, Error> {
    let mut queue = Queue::read(storage)?;
    queue.add(missing.iter().copied(), Mark::Quiet)?;
    queue.keep(storage)?;
    match storage.remove(PENDING) {
        Ok(()) | Err(_) => {}
    }
    Ok(queue.has_quiet())
}
