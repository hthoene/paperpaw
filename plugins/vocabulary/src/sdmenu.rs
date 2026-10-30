//! *Cards on the SD card*: the way into the files of the folder. One row
//! reads them into the decks (*Update*, see [`sdcard`](crate::sdcard)),
//! then for each deck a row that saves it to a file, and for each deck
//! that is linked to a file a switch: whether the cards a later update does
//! not find in the file are removed (off to begin with: they stay).

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, Screen,
};

use crate::cards::{self, Deck};
use crate::export::file_name_of;
use crate::{sdcard, sync};

pub(crate) const SCREEN: Id = Id::new_static("vocabulary.sd-menu");

/// What a row does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Update,
    /// Saves the deck of this number to its file.
    Save(u32),
    /// Switches the removal of the cards the deck's file lacks.
    Mirror(u32),
}

pub(crate) struct SdMenu {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    folder: String,
    files: sync::Pending,
    decks: Vec<Deck>,
    ledger: sync::Ledger,
    items: Vec<Item>,
}

impl SdMenu {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap();
        let display = env.device().display;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let mut menu = Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            style,
            header,
            folder: String::new(),
            files: sync::Pending::default(),
            decks: Vec::new(),
            ledger: sync::Ledger::default(),
            items: Vec::new(),
        };
        menu.load(env);
        menu
    }

    /// The decks, what the files say, and the rows, as they are now.
    fn load(&mut self, env: &Env<'_>) {
        let storage = env.storage();
        self.folder = sdcard::folder(env);
        self.files = sync::pending(storage, &self.folder);
        self.decks = cards::decks(storage).unwrap_or_default();
        self.ledger = sync::sources(storage).unwrap_or_default();
        self.items.clear();
        self.items.push(Item::Update);
        for deck in &self.decks {
            self.items.push(Item::Save(deck.id));
            if sync::link_of(&self.ledger, deck.id).is_some() {
                self.items.push(Item::Mirror(deck.id));
            }
        }
        let width = env.device().display.width;
        let high = (0..self.items.len())
            .map(|row| {
                let (title, value, note) = self.row(env, row);
                u32::from(ui::Row::value(&title, &value).note(&note).height(
                    env,
                    &self.style,
                    width,
                ))
            })
            .max()
            .unwrap_or(0)
            .max(self.style.noted_row());
        self.list.set_row(high);
        let len = self.items.len();
        if self.list.focus() >= len {
            self.list.set_focus(len.saturating_sub(1));
        }
    }

    fn deck(&self, id: u32) -> &str {
        self.decks
            .iter()
            .find(|d| d.id == id)
            .map_or("", |d| d.name.as_str())
    }

    /// The title, the value at the right and the note of a row.
    fn row(&self, env: &Env<'_>, row: usize) -> (String, String, String) {
        let i18n = env.i18n();
        match self.items.get(row).copied() {
            Some(Item::Update) => (
                i18n.tr("vocabulary.sd.update").into(),
                "›".into(),
                crate::decks::files_note(env, &self.folder, self.files),
            ),
            Some(Item::Save(id)) => {
                let deck = self.deck(id);
                let (key, file) = match sync::link_of(&self.ledger, id) {
                    Some(link) => ("vocabulary.sd.save.linked", link.name.clone()),
                    None => ("vocabulary.sd.save.new", file_name_of(deck)),
                };
                (
                    i18n.format("vocabulary.sd.save", &[("deck", Arg::Str(deck))]),
                    "›".into(),
                    i18n.format(
                        key,
                        &[
                            ("file", Arg::Str(&file)),
                            ("folder", Arg::Str(&self.folder)),
                        ],
                    ),
                )
            }
            Some(Item::Mirror(id)) => {
                let on = sync::link_of(&self.ledger, id).is_some_and(|s| s.mirror);
                let file = sync::link_of(&self.ledger, id).map_or("", |s| s.name.as_str());
                (
                    i18n.format("vocabulary.sd.mirror", &[("deck", Arg::Str(self.deck(id)))]),
                    i18n.tr(if on {
                        "vocabulary.sd.mirror.on"
                    } else {
                        "vocabulary.sd.mirror.off"
                    })
                    .into(),
                    i18n.format("vocabulary.sd.mirror.help", &[("file", Arg::Str(file))]),
                )
            }
            None => (String::new(), String::new(), String::new()),
        }
    }

    fn open(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match self.items.get(row).copied() {
            Some(Item::Update) => {
                cx.command(Command::OpenScreen(sdcard::SCREEN, Params::new()));
            }
            Some(Item::Save(id)) => {
                cx.command(Command::OpenScreen(
                    sdcard::SCREEN,
                    Params::new().with(sdcard::SAVE, alloc::format!("{id}")),
                ));
            }
            Some(Item::Mirror(id)) => {
                let on = sync::link_of(&self.ledger, id).is_some_and(|s| s.mirror);
                sync::set_mirror(env.storage(), id, !on);
                self.load(&env);
                cx.request_redraw();
            }
            None => {}
        }
    }

    fn changed(&mut self, change: ListChange, cx: &mut Cx<'_, '_>) {
        change.request(cx);
    }
}

impl Screen for SdMenu {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.items.len();
        let title = env.i18n().tr("vocabulary.sd.title");
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, title, None, self.list.paging(len));
        let menu = &*self;
        self.list.draw(&mut ui, len, |ui, row, rect, focused| {
            let (title, value, note) = menu.row(env, row);
            ui::Row::value(&title, &value)
                .note(&note)
                .focused(focused)
                .draw(ui, rect);
        });
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.load(&env);
        cx.request_redraw();
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::BACK {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        let len = self.items.len();
        let rows = self.list.rows() as isize;
        let delta = if let Some(delta) = ui::list_step(action, rows) {
            delta
        } else if *action == actions::CONFIRM {
            self.open(self.list.focus(), cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        let change = self.list.move_focus(delta, len);
        self.changed(change, cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let len = self.items.len();
        if gesture.kind == GestureKind::Tap {
            let paging = self.list.paging(len);
            if let Some(action) = self.header.tap(&env, &self.style, gesture.start, paging) {
                cx.command(Command::run(action));
                return Handled::Yes;
            }
        }
        let Some(row) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.set_focus(row);
        if gesture.kind == GestureKind::Tap {
            self.open(row, cx);
            return Handled::Yes;
        }
        Handled::No
    }
}
