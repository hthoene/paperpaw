//! The list of catalogs: *Sync catalogs* (while some are marked for it), the
//! catalogs, and *Add catalog*. The line at the bottom tells how a sync goes,
//! else how to reach a catalog's options.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{DrawFrame, Input},
    commands::CommandSender,
    formatting::Arg,
    ui::{Instance, Open, Question, TextMode},
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages,
    mem::{join, push},
    text::{concat, copy},
};

use super::{Ask, Opds, Pending, View};
use crate::native_catalogs::{self as catalogs, Catalog};
use crate::native_files::DataFiles;

/// A row of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Sync,
    Catalog(usize),
    Add,
}

/// The list screen: its rows, what each is, and the line below.
#[derive(Debug, Default)]
pub(super) struct ListView {
    list: ItemList,
    rows: Vec<Row>,
    note: String,
}

impl ListView {
    pub(super) fn draw(&mut self, host: &Host<'_>, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let title = Messages::new()?.copy(host, "opds.title")?;
        let footer = if self.note.is_empty() {
            Footer::None
        } else {
            Footer::Note(&self.note)
        };
        self.list.draw_with(host, &mut frame, &title, true, footer)
    }
}

impl Opds {
    /// The list screen opened.
    pub(super) fn opened_list(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        self.reload(host)?;
        self.list_note = None;
        let view = self.build_list(host, 0)?;
        self.screens.open(instance, View::List(view))
    }

    /// The rows and the line below, as the catalogs and the sync are now.
    fn build_list(&self, host: &Host<'_>, focus: usize) -> Result<ListView, Error> {
        let mut say = Messages::new()?;
        let mut rows = Vec::new();
        let mut items = Vec::new();
        if self.catalogs.iter().any(|c| c.sync) || self.sync_running() {
            let (title, help) = if self.sync_running() {
                ("opds.stop", "opds.stop.help")
            } else {
                ("opds.sync-all", "opds.sync-all.help")
            };
            push(
                &mut items,
                Item::noted(&say.copy(host, title)?, "", &say.copy(host, help)?)?,
            )?;
            push(&mut rows, Row::Sync)?;
        }
        let library = self.folder.as_str();
        for (i, catalog) in self.catalogs.iter().enumerate() {
            let at = paperpaw_sdk::web::host(&catalog.url);
            let note = if catalog.sync {
                let folder = catalog.shown_folder(library)?;
                say.format(
                    host,
                    "opds.synced-to",
                    &[
                        Arg::Text {
                            name: "host",
                            value: at,
                        },
                        Arg::Text {
                            name: "folder",
                            value: &folder,
                        },
                    ],
                )?
            } else {
                copy(at)?
            };
            push(&mut items, Item::noted(&catalog.name, "›", &note)?)?;
            push(&mut rows, Row::Catalog(i))?;
        }
        push(&mut items, Item::row(&say.copy(host, "opds.add")?, "+")?)?;
        push(&mut rows, Row::Add)?;
        let mut list = ItemList::new(items);
        list.set_focus(focus);
        Ok(ListView {
            list,
            rows,
            note: self.list_status(host, &mut say)?,
        })
    }

    /// The line at the bottom: a message, how a sync goes, or the hint.
    fn list_status(&self, host: &Host<'_>, say: &mut Messages) -> Result<String, Error> {
        if let Some(note) = &self.list_note {
            return copy(note);
        }
        if let Some(report) = self.sync_report_text(host, say)? {
            return Ok(report);
        }
        if self.unreadable {
            return say.copy(host, "opds.unreadable");
        }
        say.copy(
            host,
            if self.touch {
                "opds.hint"
            } else {
                "opds.hint.buttons"
            },
        )
    }

    /// Makes the rows of the list on show anew (after a change of the
    /// catalogs or of the sync) and asks for a redraw.
    pub(super) fn refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(focus) = self.top_list().map(|view| view.list.focus()) else {
            return Ok(());
        };
        let view = self.build_list(host, focus)?;
        if let Some(top) = self.top_list() {
            *top = view;
        }
        Self::redraw(host)
    }

    fn top_list(&mut self) -> Option<&mut ListView> {
        match self.screens.top(crate::declarations::CATALOGS_SCREEN) {
            Some(View::List(view)) => Some(view),
            _ => None,
        }
    }

    pub(super) fn list_input(
        &mut self,
        host: &Host<'_>,
        token: u32,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let Some(View::List(view)) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        // With buttons the menu button offers what a hold does on a touch screen.
        if let Input::Action {
            action: "core.menu",
            ..
        } = *input
        {
            let focus = view.list.focus();
            self.offer(host, focus)?;
            return Ok(Handled::Yes);
        }
        let step = view.list.input(host, input)?;
        if matches!(step, ItemStep::Chosen(_) | ItemStep::Held(_)) {
            self.list_note = None;
        }
        match step {
            ItemStep::Chosen(index) => self.activate(host, index)?,
            ItemStep::Held(index) => self.offer(host, index)?,
            _ => {}
        }
        Ok(ItemList::handled(step))
    }

    /// The row at `index` of the list on show.
    fn row_at(&mut self, index: usize) -> Option<Row> {
        self.top_list()?.rows.get(index).copied()
    }

    /// A row was chosen.
    fn activate(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        match self.row_at(index) {
            Some(Row::Sync) => self.sync_row(host),
            Some(Row::Catalog(i)) => self.open_browse(host, i),
            Some(Row::Add) => {
                let title = Messages::new()?.copy(host, "opds.address")?;
                self.ask_text(
                    host,
                    &title,
                    Some("https://"),
                    Some(TextMode::Url),
                    false,
                    Ask::Address,
                )
            }
            None => Ok(()),
        }
    }

    /// Opens the browsing of the catalog at `index`.
    pub(super) fn open_browse(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        self.pending = Some(Pending::Browse(index));
        let mut scratch = [0; 128];
        CommandSender::new(host, &mut scratch).open(Open::screen("opds.browse")?)
    }

    /// Opens the editor of the catalog at `index`.
    pub(super) fn open_edit(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        self.pending = Some(Pending::Edit(index));
        let mut scratch = [0; 128];
        CommandSender::new(host, &mut scratch).open(Open::screen("opds.edit")?)
    }

    /// What can be done with row `index`: the options of a catalog.
    fn offer(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(Row::Catalog(i)) = self.row_at(index) else {
            return self.activate(host, index);
        };
        let Some(catalog) = self.catalogs.get(i) else {
            return Ok(());
        };
        let mut say = Messages::new()?;
        let mut labels = Vec::new();
        for key in [
            "opds.option.open",
            "opds.option.sync",
            "opds.option.edit",
            "opds.option.remove",
        ] {
            push(&mut labels, say.copy(host, key)?)?;
        }
        let choices = join(&labels, "\n")?;
        let title = copy(&catalog.name)?;
        let question = Question::choice(&title, "", &choices)?;
        self.ask(host, question, None, Ask::Offer(i))
    }

    /// The option chosen for the catalog at `i`.
    pub(super) fn offered(&mut self, host: &Host<'_>, i: usize, choice: u32) -> Result<(), Error> {
        if i >= self.catalogs.len() {
            return Ok(());
        }
        match choice {
            0 => self.open_browse(host, i),
            1 => self.sync_one(host, i),
            2 => self.open_edit(host, i),
            3 => self.remove(host, i),
            _ => Ok(()),
        }
    }

    /// Takes the catalog at `i` out of the list, with its password.
    fn remove(&mut self, host: &Host<'_>, i: usize) -> Result<(), Error> {
        let removed = self.catalogs.remove(i);
        if !self.save_list(host)? {
            self.catalogs.insert(i, removed);
        }
        self.refresh(host)
    }

    /// Keeps the list, and takes the passwords of catalogs no longer in it away;
    /// `false` where it could not be kept (said), and the list is as it was.
    pub(super) fn save_list(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        if self.unreadable {
            let note = Messages::new()?.copy(host, "opds.unreadable")?;
            self.list_note = Some(note);
            self.refresh(host)?;
            return Ok(false);
        }
        let files = DataFiles::new(host);
        match catalogs::save(&files, &self.catalogs) {
            Ok(()) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {
                Self::notice_key(host, "opds.not-kept")?;
                return Ok(false);
            }
        }
        catalogs::prune(&self.vault(host), &self.catalogs)?;
        Ok(true)
    }

    /// The address of a new catalog was typed.
    pub(super) fn address_typed(&mut self, host: &Host<'_>, typed: &str) -> Result<(), Error> {
        let text = typed.trim();
        if text.is_empty() || text == "https://" {
            return Ok(());
        }
        let url = if text.contains("://") {
            copy(text)?
        } else {
            concat(&["https://", text])?
        };
        if url.len() > crate::native_model::MAX_URL {
            return Self::notice_key(host, "opds.not-kept");
        }
        self.reload(host)?;
        push(&mut self.catalogs, Catalog::new(&url)?)?;
        if !self.save_list(host)? {
            self.catalogs.pop();
            return Ok(());
        }
        let last = self.catalogs.len().saturating_sub(1);
        self.open_edit(host, last)
    }
}
