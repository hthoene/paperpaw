//! Vocabulary: the words looked up while reading, each with its sentence
//! and book, to go over later.
//!
//! The reader announces every word looked up
//! ([`events::WORD_LOOKED_UP`]); this plugin keeps it (setting
//! `vocabulary.collect`, on at first), newest first. *Vocabulary* in the
//! main menu lists them: a tap looks the word up again, holding it offers
//! to go to its place in the book or to delete it. The last row exports
//! all words for flash card programs such as Anki (word, sentence, book,
//! separated by tabs) into the `Notes` folder beside the library.
//!
//! With `vocabulary.mark` on (off at first; also in the reader's menu
//! *More*), the words are marked where they occur in books of their
//! language, in the role `vocabulary.learning` (a dotted line under the
//! text, as the reader likes it); a tap on one looks it up.
//!
//! *Flash cards* (in *Tools*) reviews cards with spaced repetition
//! (FSRS, as Anki does; see `fsrs`): each due card's front, then its
//! back and four answers, *Again*, *Hard*, *Good*, *Easy*, which decide
//! when it comes again. Decks and cards are made on a phone (*Edit with
//! your phone*: a page served on the local network while that screen is
//! open, see `phone`), typed, pasted as CSV or TSV, or taken from an Anki
//! deck. A widget says how many cards are due. *A flash card due* can be
//! the sleep screen (among its choices): the front of the next card,
//! answered on waking, as the review opens then with it; with
//! `vocabulary.review-on-wake` (off at first) the review opens on waking
//! whatever the sleep screen showed, where cards are due. With
//! `vocabulary.cards-from-lookups` (off at first) each word looked up
//! becomes a card of the deck *Looked up*, its sentence the example.
//!
//! Only the visible page of words, and of cards the state of those due,
//! is ever in memory.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod cards;
mod decks;
mod fsrs;
mod marks;
mod phone;
mod review;
mod sdcard;
mod sleep;
mod store;
mod sync;
mod table;
mod widget;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, groups, menus, params, slots};
use paperpaw_sdk::decorations::{Decoration, LineKind};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::Confirmed;
use paperpaw_sdk::ui::choice;
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Commands, Cx, Env, FixedAction, FixedScreen, Gesture, GestureKind,
    Handled, Id, Manifest, MenuEntry, Params, Plugin, Rect, RefreshMode, RegisterError, Registrar,
    Screen, ScreenSpec, SettingExtra, SettingSpec, StorageError,
};

use paperpaw_sdk::setting::{Setting, Text};
use store::Word;

pub const PLUGIN: Id = Id::new_static("vocabulary");
/// Opens the list of words.
pub const OPEN: Id = Id::new_static("vocabulary.open");
/// Setting: keep the words looked up.
pub const COLLECT: Setting<bool> = Setting::new("vocabulary.collect");
/// Setting: mark the words in books.
pub const MARK: Setting<bool> = Setting::new("vocabulary.mark");
/// Opens the flash cards.
pub const CARDS: Id = Id::new_static("vocabulary.flash-cards");
/// Setting: the chance to remember a card wanted at its review, in percent.
pub const RETENTION: Setting<i64> = Setting::new("vocabulary.retention");
/// Setting: new cards shown a day at most.
pub const NEW_PER_DAY: Setting<i64> = Setting::new("vocabulary.new-per-day");
/// Setting: the review opens as the device wakes, where cards are due.
pub const REVIEW_ON_WAKE: Setting<bool> = Setting::new("vocabulary.review-on-wake");
/// Setting: the folder of the card the decks' files are in.
pub const SD_FOLDER: Setting<Text> = Setting::new("vocabulary.sd-folder");
/// Setting: the decks are updated from the files when the flash cards open.
pub const SD_AUTO: Setting<bool> = Setting::new("vocabulary.sd-auto");
/// Reads the files of the folder anew: the decks follow them.
pub const SCAN: Id = Id::new_static("vocabulary.scan-card");
/// The sleep screen's mode of this plugin: a flash card due.
pub const SLEEP_CARD: &str = "vocabulary.sleep-card";
/// What the sleep screen shows (the `sleep` plugin's setting).
const SLEEP_SHOW: &str = "sleep.show";
/// Setting: each word looked up becomes a flash card.
pub const FROM_LOOKUPS: Setting<bool> = Setting::new("vocabulary.cards-from-lookups");
/// The role of the words marked in books.
pub const LEARNING: Id = Id::new_static("vocabulary.learning");
const SCREEN: Id = Id::new_static("vocabulary.list");
const CHOSEN: Event<choice::Picked> = Event::new("vocabulary.chosen");
/// The answer to whether to delete a word.
const DELETE: Event<Confirmed> = Event::new("vocabulary.delete");
/// The word and the language asked about, in the question's context.
const WORD: &str = "word";
const LANGUAGE: &str = "language";
const OPTION_LABELS: &str = "vocabulary.option.";
/// The dictionary's settings (see the `dictionary` plugin).
const DICTIONARY_SECTION: &str = "dictionary.section";
/// The dictionary's part of the reading settings.
static IN_DICTIONARY: SettingExtra = SettingExtra::new().section(DICTIONARY_SECTION);

/// Whether words are kept and marked.
static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::switch(
        COLLECT.static_str(),
        "vocabulary.setting.collect",
        true,
        groups::READING,
    )
    .advanced()
    .with_extra(&IN_DICTIONARY)
    .description("vocabulary.setting.collect.help"),
    SettingSpec::switch(
        MARK.static_str(),
        "vocabulary.setting.mark",
        false,
        groups::READING,
    )
    .with_extra(&IN_DICTIONARY)
    .description("vocabulary.setting.mark.help"),
];

/// The flash cards' part of the reading settings.
static IN_CARDS: SettingExtra = SettingExtra::new().section("vocabulary.cards.title");
static IN_CARDS_PERCENT: SettingExtra = SettingExtra::new()
    .section("vocabulary.cards.title")
    .unit("vocabulary.unit.percent");

/// How the flash cards are reviewed, and whether words looked up become
/// cards.
static CARD_SETTINGS: [SettingSpec; 6] = [
    SettingSpec::number(
        NEW_PER_DAY.static_str(),
        "vocabulary.setting.new-per-day",
        0,
        200,
        20,
        groups::READING,
    )
    .with_extra(&IN_CARDS)
    .description("vocabulary.setting.new-per-day.help"),
    SettingSpec::number(
        RETENTION.static_str(),
        "vocabulary.setting.retention",
        70,
        97,
        90,
        groups::READING,
    )
    .advanced()
    .with_extra(&IN_CARDS_PERCENT)
    .description("vocabulary.setting.retention.help"),
    SettingSpec::switch(
        FROM_LOOKUPS.static_str(),
        "vocabulary.setting.cards-from-lookups",
        false,
        groups::READING,
    )
    .with_extra(&IN_CARDS)
    .description("vocabulary.setting.cards-from-lookups.help"),
    SettingSpec::switch(
        REVIEW_ON_WAKE.static_str(),
        "vocabulary.setting.review-on-wake",
        false,
        groups::READING,
    )
    .with_extra(&IN_CARDS)
    .description("vocabulary.setting.review-on-wake.help"),
    SettingSpec::switch(
        SD_AUTO.static_str(),
        "vocabulary.setting.sd-auto",
        true,
        groups::READING,
    )
    .with_extra(&IN_CARDS)
    .description("vocabulary.setting.sd-auto.help"),
    SettingSpec::text(
        SD_FOLDER.static_str(),
        "vocabulary.setting.sd-folder",
        sync::DEFAULT_FOLDER,
        groups::READING,
    )
    .with_extra(&IN_CARDS)
    .description("vocabulary.setting.sd-folder.help"),
];

/// How the words being learnt look in books.
static LEARNING_LOOK: [SettingSpec; 7] = paperpaw_sdk::role_settings!(
    "vocabulary.learning",
    "vocabulary.role.learning",
    Decoration::NONE.line(LineKind::Dotted)
);

/// The folder exports go to, beside the library (as the reader's notes).
const NOTES: &str = "Notes";
const EXPORT_NAME: &str = "Vocabulary.tsv";

/// The vocabulary plugin.
#[derive(Debug, Default)]
pub struct Vocabulary;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 3] = [
    FixedAction::new(OPEN, "vocabulary.title")
        .handler(|_, _, out| out.push(Command::OpenScreen(SCREEN, Params::new()))),
    FixedAction::new(CARDS, "vocabulary.cards.title")
        .handler(|_, _, out| out.push(Command::OpenScreen(decks::SCREEN, Params::new()))),
    FixedAction::new(SCAN, "vocabulary.sd.scan").handler(|_, _, out| {
        out.push(Command::OpenScreen(
            sdcard::SCREEN,
            Params::new().with(sdcard::FORCE, "1"),
        ));
    }),
];
static ENTRIES: [MenuEntry; 3] = [
    MenuEntry::new(menus::READER_MORE, Id::new_static(MARK.static_str())).order(40),
    MenuEntry::new(menus::TOOLS, OPEN).order(36),
    MenuEntry::new(menus::TOOLS, CARDS).order(36),
];
static ICONS: [(Id, &str); 3] = [
    (Id::new_static(MARK.static_str()), "vocabulary"),
    (OPEN, "vocabulary"),
    (CARDS, "stack-2"),
];

static SCREENS: [FixedScreen; 5] = [
    FixedScreen::new(decks::SCREEN, "vocabulary.cards.title", |env, _| {
        Box::new(decks::DecksScreen::new(env))
    }),
    FixedScreen::new(review::SCREEN, "vocabulary.review.title", |env, params| {
        Box::new(review::ReviewScreen::new(env, params))
    }),
    FixedScreen::new(
        Id::new_static(SLEEP_CARD),
        "vocabulary.sleep-card",
        |env, _| Box::new(sleep::SleepCard::new(env)),
    ),
    FixedScreen::new(phone::SCREEN, "vocabulary.phone.title", |env, _| {
        Box::new(phone::PhoneScreen::new(env))
    })
    .instances(paperpaw_sdk::Instances::SINGLE),
    FixedScreen::new(sdcard::SCREEN, "vocabulary.sd.title", |env, params| {
        Box::new(sdcard::SdScreen::new(env, params))
    })
    .instances(paperpaw_sdk::Instances::SINGLE),
];

impl Plugin for Vocabulary {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Vocabulary", paperpaw_sdk::package_version!())
            .description(
                "Keeps the words you look up, with their sentences, and reviews flash cards with spaced repetition. Make decks on your phone (Flash cards, Edit with your phone): type cards, or add many at once from a CSV, TSV or Anki file.",
            )
            // The page for editing cards on a phone.
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_settings(&CARD_SETTINGS)?;
        // A list or a deck left half written by a power cut is put right
        // first, and the answers of a review cut off go into the decks.
        r.on(events::STARTED, |env, _, _| {
            store::recover(env.storage());
            cards::recover(env.storage());
        })?;
        // The changes to the words, for the marks and the list.
        let changes = store::Changes::default();
        let recorded = changes.clone();
        r.on(events::WORD_LOOKED_UP, move |env, looked_up, out| {
            if record(env, looked_up, &recorded).is_err() {
                let message = env.i18n().tr("vocabulary.save-failed").to_string();
                out.push(Command::Notify(message));
            }
        })?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        r.add_settings(&LEARNING_LOOK)?;
        r.add_decorator(marks::decorator(&changes))?;
        r.on(events::DECORATION_TAPPED, marks::tapped)?;
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "vocabulary.title",
            Box::new(move |env, _| Box::new(WordsScreen::new(env, changes.clone()))),
        ))?;
        r.add_actions(&ACTIONS)?;
        r.add_screens(&SCREENS)?;
        // The first review: the answers, by touch or with the buttons.
        paperpaw_sdk::help::add(
            r,
            review::SCREEN.as_str(),
            &[
                paperpaw_sdk::help::HelpHint::any("vocabulary.help.answers"),
                paperpaw_sdk::help::HelpHint::touch("vocabulary.help.tap"),
                paperpaw_sdk::help::HelpHint::buttons("vocabulary.help.select"),
                paperpaw_sdk::help::HelpHint::any("vocabulary.help.back"),
            ],
        )?;
        r.add_choice(Id::new_static(SLEEP_SHOW), SLEEP_CARD)?;
        r.on(events::RESUME, |env, _, out| wake(env, out))?;
        widget::register(r)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// Keeps a word looked up, if wanted; `Err` where it could not be.
fn record(
    env: &Env<'_>,
    looked_up: &events::WordLookedUp,
    changes: &store::Changes,
) -> Result<(), StorageError> {
    if !env.settings().read(&COLLECT) {
        return Ok(());
    }
    let word = looked_up.word.trim();
    if word.is_empty() {
        return Ok(());
    }
    let result = store::add(
        env.storage(),
        &Word {
            word: word.to_string(),
            language: looked_up.language.clone(),
            section: looked_up.section,
            offset: looked_up.offset,
            path: looked_up.path.clone(),
            title: looked_up.title.clone(),
            sentence: looked_up.text.clone(),
        },
    );
    changes.changed();
    // A card to be: a line appended, the deck not read now (see
    // `take_lookups`); the word is kept whether or not that works.
    if env.settings().read(&FROM_LOOKUPS) {
        let _ = cards::queue_lookup(env.storage(), word, &looked_up.text);
    }
    result
}

/// Waking: the review, with the card shown asleep first where the sleep
/// screen showed one (*A flash card due*), or where wanted
/// ([`REVIEW_ON_WAKE`]) and cards are due. Below a lock, as it opens
/// (the kernel keeps what others open below a screen that guards).
fn wake(env: &Env<'_>, out: &mut Commands) {
    let asleep = env.settings().text(SLEEP_SHOW) == SLEEP_CARD;
    let shown = asleep.then(|| sleep::shown(env)).flatten();
    if shown.is_none() && !(env.settings().read(&REVIEW_ON_WAKE) && decks::due_now(env) > 0) {
        return;
    }
    let params = match shown {
        Some((deck, id)) => Params::new().with(review::FIRST, format!("{deck} {id}")),
        None => Params::new(),
    };
    out.push(Command::OpenScreen(review::SCREEN, params));
}

/// The words looked up waiting to become cards: cards of the deck
/// *Looked up* now (each front once, its sentence the example). Where
/// there is no room (no more decks, a full deck) they wait on.
pub(crate) fn take_lookups(env: &Env<'_>) {
    let name = env.i18n().tr("vocabulary.cards.looked-up");
    let _ = cards::take_lookups(env.storage(), name);
}

/// The folder exports go to: `Notes` beside the library folder.
fn notes_dir(env: &Env<'_>) -> String {
    let library = paperpaw_sdk::library::folder(env);
    let parent = paperpaw_sdk::files::parent(library);
    if parent.is_empty() {
        String::from(NOTES)
    } else {
        format!("{parent}/{NOTES}")
    }
}

/// The list of words, newest first, and *Export* below them.
struct WordsScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    /// Words in all.
    count: usize,
    /// The words of the visible page: their row and number in the file.
    visible: Vec<(usize, usize, Word)>,
    /// The changes to the words they were read after
    /// ([`store::Changes`]): words looked up meanwhile show when the list
    /// does again.
    read_after: u32,
    changes: store::Changes,
}

impl WordsScreen {
    fn new(env: &Env<'_>, changes: store::Changes) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let mut screen = Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            style,
            header,
            count: 0,
            visible: Vec::new(),
            read_after: 0,
            changes,
        };
        screen.load(env);
        screen
    }

    /// Counts the words and reads the visible page.
    fn load(&mut self, env: &Env<'_>) {
        self.read_after = self.changes.count();
        self.count = store::count(env.storage());
        let len = self.len();
        if self.list.focus() >= len {
            self.list.set_focus(len.saturating_sub(1));
        }
        self.load_page(env);
    }

    fn load_page(&mut self, env: &Env<'_>) {
        let first = self.list.page() * self.list.rows();
        let last = (first + self.list.rows()).min(self.count);
        // Row `r` shows word number `count - 1 - r`.
        let (from, to) = (
            self.count.saturating_sub(last),
            self.count - first.min(self.count),
        );
        self.visible = store::range(env.storage(), from, to)
            .into_iter()
            .map(|(n, word)| (self.count - 1 - n, n, word))
            .collect();
    }

    /// The rows: the words, then *Export* if there are any.
    fn len(&self) -> usize {
        match self.count {
            0 => 0,
            n => n + 1,
        }
    }

    fn word(&self, row: usize) -> Option<&(usize, usize, Word)> {
        self.visible.iter().find(|(r, _, _)| *r == row)
    }

    /// A tap or confirm: looks the word up again, or exports.
    fn open(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        if row == self.count && row > 0 {
            return self.export(cx);
        }
        if let Some((_, _, word)) = self.word(row) {
            cx.command(Command::OpenSlot(
                slots::LOOKUP,
                Params::new()
                    .with(params::WORD, word.word.clone())
                    .with(params::LANGUAGE, word.language.clone()),
            ));
        }
    }

    /// Holding a word: what can be done with it.
    fn offer(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        let Some((_, n, word)) = self.word(row) else {
            return;
        };
        let mut options = alloc::vec!["look-up"];
        if !word.path.is_empty() && cx.env().storage().exists(&word.path) {
            options.push("go-to");
        }
        options.push("delete");
        cx.command(
            paperpaw_sdk::ask::choice(&word.word, &CHOSEN)
                .about(format!("{n}"))
                .options(OPTION_LABELS, &options)
                .command(),
        );
    }

    fn export(&self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let dir = notes_dir(&env);
        let _ = env.storage().create_dir(&dir);
        let file = format!("{dir}/{EXPORT_NAME}");
        let message = if store::export(env.storage(), &file) {
            env.i18n()
                .format("vocabulary.exported", &[("path", Arg::Str(&file))])
        } else {
            env.i18n().tr("vocabulary.export-failed").to_string()
        };
        cx.command(Command::Notify(message));
    }

    /// The answer to whether to delete a word: deleted where it is yes.
    fn delete(&mut self, answer: &Confirmed, cx: &mut Cx<'_, '_>) {
        let Some(word) = answer.context.get(WORD).filter(|_| answer.yes) else {
            return;
        };
        let env = *cx.env();
        let word = Word {
            word: word.to_string(),
            language: answer.context.get(LANGUAGE).unwrap_or("").to_string(),
            ..Word::default()
        };
        let removed = store::remove(env.storage(), &word);
        self.changes.changed();
        if removed.is_err() {
            let message = env.i18n().tr("vocabulary.delete-failed").to_string();
            cx.command(Command::Notify(message));
        }
        self.load(&env);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    fn changed(&mut self, change: ListChange, cx: &mut Cx<'_, '_>) {
        if change == ListChange::Page {
            let env = *cx.env();
            self.load_page(&env);
        }
        change.request(cx);
    }
}

impl Screen for WordsScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.len();
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(
            &mut ui,
            env.i18n().tr("vocabulary.title"),
            None,
            self.list.paging(len),
        );
        if len == 0 {
            ui.empty(
                self.list.area(),
                "vocabulary",
                env.i18n().tr("vocabulary.empty.title"),
                env.i18n().tr("vocabulary.empty"),
            );
            return;
        }
        let count = self.count;
        let visible = &self.visible;
        self.list.draw(&mut ui, len, |ui, row, rect, focused| {
            if row == count {
                let title = env.i18n().tr("vocabulary.export");
                let note = env.i18n().tr("vocabulary.export.help");
                ui::Row::value(title, "›")
                    .note(note)
                    .focused(focused)
                    .draw(ui, rect);
                return;
            }
            let Some((_, _, word)) = visible.iter().find(|(r, _, _)| *r == row) else {
                return;
            };
            let note = if word.sentence.is_empty() {
                word.title.as_str()
            } else {
                word.sentence.as_str()
            };
            ui::Row::plain(&word.word)
                .note(note)
                .focused(focused)
                .draw(ui, rect);
        });
    }

    /// Back from a book or a lookup: with the words looked up meanwhile.
    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if self.changes.count() != self.read_after {
            let env = *cx.env();
            self.load(&env);
            cx.request_redraw();
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.len();
        let rows = self.list.rows() as isize;
        let a = action;
        let delta = if let Some(delta) = ui::list_step(a, rows) {
            delta
        } else if *a == actions::CONFIRM {
            self.open(self.list.focus(), cx);
            return Handled::Yes;
        } else if *a == actions::MENU {
            self.offer(self.list.focus(), cx);
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
        match gesture.kind {
            GestureKind::Tap => self.open(row, cx),
            GestureKind::LongPress => self.offer(row, cx),
            _ => return Handled::No,
        }
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(answer) = DELETE.read(event, params) {
            return self.delete(&answer, cx);
        }
        let Some(picked) = CHOSEN.read(event, params) else {
            return;
        };
        let env = *cx.env();
        let Ok(n) = picked.path.parse::<usize>() else {
            return;
        };
        let Some((_, _, word)) = self.visible.iter().find(|(_, i, _)| *i == n).cloned() else {
            return;
        };
        match picked.option.as_str() {
            "look-up" => cx.command(Command::OpenSlot(
                slots::LOOKUP,
                Params::new()
                    .with(params::WORD, word.word.clone())
                    .with(params::LANGUAGE, word.language.clone()),
            )),
            "go-to" => {
                // The book, then the word's place in it.
                cx.command(paperpaw_sdk::readers::open(word.path.clone()));
                cx.command(events::GOTO.emit(events::Goto::to(
                    word.path.clone(),
                    paperpaw_sdk::document::Locator::new(word.section, word.offset),
                )));
            }
            // Asked first: a word deleted cannot be had back.
            "delete" => {
                let question = env.i18n().format(
                    "vocabulary.delete.question",
                    &[("word", Arg::Str(&word.word))],
                );
                cx.command(
                    paperpaw_sdk::ask::confirm(question, &DELETE)
                        .text(env.i18n().tr("vocabulary.delete.text"))
                        .yes(env.i18n().tr("vocabulary.option.delete"))
                        .no(env.i18n().tr("vocabulary.delete.keep"))
                        .with(WORD, word.word.clone())
                        .with(LANGUAGE, word.language.clone())
                        .command(),
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

    /// Marking is off at first, switches the decorator, which hears taps,
    /// and is in the reader's menu.
    #[test]
    fn marking_is_off_at_first() {
        use paperpaw_sdk::Contribution;
        use paperpaw_sdk::{Plugin, Registrar, SettingValue};

        let plugin = super::Vocabulary;
        let mut r = Registrar::new(plugin.manifest().id);
        assert!(plugin.register(&mut r).is_ok());
        let contributions = r.contributions();
        let mark = contributions.iter().find_map(|c| match c {
            Contribution::FixedSettings(list) => list.iter().find(|s| s.id == *super::MARK.id()),
            _ => None,
        });
        assert_eq!(mark.map(|s| &s.default), Some(&SettingValue::Bool(false)));
        let decorator = contributions.iter().find_map(|c| match c {
            Contribution::Decorator(d) => Some(d),
            _ => None,
        });
        assert!(decorator.is_some_and(|d| d.taps && d.switch.as_ref() == Some(super::MARK.id())));
        assert!(contributions.iter().any(|c| matches!(
            c,
            Contribution::FixedMenuEntries(list)
                if list.iter().any(|e| e.menu == paperpaw_sdk::builtin::menus::READER_MORE
                    && e.item == *super::MARK.id())
        )));
    }
}
