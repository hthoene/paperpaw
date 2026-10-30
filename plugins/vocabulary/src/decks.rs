//! *Flash cards* (in *Tools*): *Review* with how many cards are due, *Edit
//! with your phone* (see [`phone`](crate::phone)), then the decks with
//! how many cards each has; a deck chosen is reviewed alone.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, Screen,
};

use crate::cards::{self, Day, Deck};
use crate::{NEW_PER_DAY, SD_AUTO, phone, review, sdcard, sdmenu, sync};

pub(crate) const SCREEN: Id = Id::new_static("vocabulary.cards");

/// Rows before the decks.
const REVIEW_ROW: usize = 0;
const PHONE_ROW: usize = 1;
const CARD_ROW: usize = 2;
const FIRST_DECK: usize = 3;

/// What the row of the SD card's files says: what there is to do.
pub(crate) fn files_note(env: &Env<'_>, folder: &str, files: sync::Pending) -> String {
    let folder = [("folder", Arg::Str(folder))];
    let i18n = env.i18n();
    if !files.folder {
        i18n.format("vocabulary.sd.row.none", &folder)
    } else if files.todo > 0 {
        i18n.plural("vocabulary.sd.row.todo", files.todo as i64, &folder)
    } else if files.failed > 0 {
        i18n.plural("vocabulary.sd.row.failed", files.failed as i64, &folder)
    } else if files.files == 0 {
        i18n.format("vocabulary.sd.row.empty", &folder)
    } else {
        i18n.format("vocabulary.sd.row.same", &folder)
    }
}

pub(crate) struct DecksScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    decks: Vec<(Deck, usize)>,
    due: usize,
    /// The files of the SD card's folder, listed.
    files: sync::Pending,
    folder: String,
    /// Whether the files were looked at for an update at opening.
    looked: bool,
}

/// The cards due now, and new ones to show today, as the widget and this
/// screen count them.
pub(crate) fn due_now(env: &Env<'_>) -> usize {
    let now = cards::now(env);
    let day = Day::now(env, now);
    let per_day = env.settings().read(&NEW_PER_DAY).clamp(0, 1000) as u32;
    cards::due_count(
        env.storage(),
        day.end,
        cards::new_left(env.storage(), day, per_day),
    )
}

impl DecksScreen {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let mut screen = Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            style,
            header,
            decks: Vec::new(),
            due: 0,
            files: sync::Pending::default(),
            folder: String::new(),
            looked: false,
        };
        screen.load(env);
        screen
    }

    fn load(&mut self, env: &Env<'_>) {
        crate::take_lookups(env);
        let sizes = cards::sizes(env.storage());
        self.decks = cards::decks(env.storage())
            .unwrap_or_default()
            .into_iter()
            .map(|d| {
                let n = sizes.get(&d.id).copied().unwrap_or(0);
                (d, n)
            })
            .collect();
        self.due = due_now(env);
        self.folder = sdcard::folder(env);
        self.files = sdcard::pending(env);
        // Rows as high as the phone's note needs (the largest text).
        let key = if self.decks.is_empty() {
            "vocabulary.cards.phone.first"
        } else {
            "vocabulary.cards.phone.help"
        };
        let width = self.list.area().width;
        let title = env.i18n().tr("vocabulary.cards.phone");
        let row =
            ui::Row::value(title, "›")
                .note(env.i18n().tr(key))
                .height(env, &self.style, width);
        let card_title = env.i18n().tr("vocabulary.sd.row");
        let card_row = ui::Row::value(card_title, "›")
            .note(&self.card_note(env))
            .height(env, &self.style, width);
        self.list.set_row(
            u32::from(row)
                .max(u32::from(card_row))
                .max(self.style.noted_row()),
        );
        let len = self.len();
        if self.list.focus() >= len {
            self.list.set_focus(len - 1);
        }
    }

    fn len(&self) -> usize {
        FIRST_DECK + self.decks.len()
    }

    /// What the row of the SD card's files says: what there is to do.
    fn card_note(&self, env: &Env<'_>) -> String {
        files_note(env, &self.folder, self.files)
    }

    fn open(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        let params = match row {
            REVIEW_ROW => Params::new(),
            PHONE_ROW => {
                cx.command(Command::OpenScreen(phone::SCREEN, Params::new()));
                return;
            }
            CARD_ROW => {
                cx.command(Command::OpenScreen(sdmenu::SCREEN, Params::new()));
                return;
            }
            row => match self.decks.get(row - FIRST_DECK) {
                Some((deck, _)) => Params::new().with(review::DECK, format!("{}", deck.id)),
                None => return,
            },
        };
        cx.command(Command::OpenScreen(review::SCREEN, params));
    }

    fn changed(&mut self, change: ListChange, cx: &mut Cx<'_, '_>) {
        change.request(cx);
    }
}

impl Screen for DecksScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.len();
        let title = env.i18n().tr("vocabulary.cards.title");
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, title, None, self.list.paging(len));
        let due = env
            .i18n()
            .plural("vocabulary.cards.due", self.due as i64, &[]);
        let empty = self.decks.is_empty();
        let card_note = self.card_note(env);
        let decks = &self.decks;
        self.list.draw(&mut ui, len, |ui, row, rect, focused| {
            let (title, note): (&str, String) = match row {
                REVIEW_ROW => (env.i18n().tr("vocabulary.cards.review"), due.clone()),
                PHONE_ROW => {
                    let key = if empty {
                        "vocabulary.cards.phone.first"
                    } else {
                        "vocabulary.cards.phone.help"
                    };
                    (
                        env.i18n().tr("vocabulary.cards.phone"),
                        env.i18n().tr(key).into(),
                    )
                }
                CARD_ROW => (env.i18n().tr("vocabulary.sd.row"), card_note.clone()),
                row => match decks.get(row - FIRST_DECK) {
                    Some((deck, n)) => (
                        deck.name.as_str(),
                        env.i18n().plural("vocabulary.cards.count", *n as i64, &[]),
                    ),
                    None => return,
                },
            };
            ui::Row::value(title, "›")
                .note(&note)
                .focused(focused)
                .draw(ui, rect);
        });
    }

    /// Back from a review or the phone: the counts as they are now.
    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.load(&env);
        // Opening the flash cards updates the decks from the files that
        // are new or changed, unless the setting says not to.
        if !self.looked {
            self.looked = true;
            if env.settings().read(&SD_AUTO) && self.files.todo > 0 {
                cx.command(Command::OpenScreen(
                    sdcard::SCREEN,
                    Params::new().with(sdcard::AUTO, "1"),
                ));
            }
        }
        cx.request_redraw();
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.len();
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
        let len = self.len();
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
