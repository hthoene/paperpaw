//! The interface-7 Vocabulary: its settings, the recovery at start (a word
//! list or a deck left half written by a power cut is put right), and the
//! flash cards: the decks' screen and the review. The word list, the phone
//! page, the SD card's screens, the marks in books, the sleep card and the
//! widget follow with their host pieces; see `PORTING-7.md`.
use alloc::string::String;

use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, DrawPurpose, Input},
    commands::{CommandSender, RedrawMode},
    events::{AnswerOutcome, Answered, Event, Incoming, kinds},
    formatting::Arg,
    registrations,
    reply::Reply,
    storage::DataStorage,
    ui::{HandoverId, Instance, Notify, Question, RequestId, Ui},
};
use paperpaw_sdk::{Handled, Storage};
use paperpaw_ui_kit::{
    Screens,
    mem::{SERVICE_BUFFER, buffer},
    read_event, text,
};

use crate::declarations::{
    CARDS_TOKEN, DECKS_TOKEN, DECORATE_STAGE, DECORATE_TOKEN, EVENTS_TOKEN, OPEN_TOKEN,
    RESUME_TOKEN, REVIEW_TOKEN, SD_CARD_TOKEN, SD_MENU_TOKEN, SLEEP_CARD, SLEEP_TOKEN, TICK_TOKEN,
    WORD_STAGE, WORD_TOKEN, WORDS_TOKEN, WORK_TOKEN,
};
use crate::files::DataFolder;
use crate::native_decks::{self, DecksView, Numbers};
use crate::native_goto;
use crate::native_list::{self, WordRows, WordsView};
use crate::native_review::{self, due_now};
use crate::native_review_screen::{HostTime, ReviewScreen, settings};
use crate::native_widgets::Tap;
use crate::native_words::{HostWords, Words};
use crate::store::Word;
use crate::{cards, store, sync};

mod image {
    include!(concat!(env!("OUT_DIR"), "/vocabulary_declarations.rs"));
}

/// What a question asked on the list of words is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    /// What to do with a word held: go to its book, or delete it.
    Options,
    /// Whether to delete the word.
    Delete,
}

/// Vocabulary's interface-7 plugin. At rest it is one pointer: the state
/// (the screens open, the widgets, the question asked) is made on first use,
/// so a device that never opens a vocabulary screen or widget keeps none of
/// it on the heap (#248).
#[derive(Debug, Default)]
pub struct NativeVocabulary {
    /// One element, so that it can be made without aborting where the heap is
    /// short (`Box::new` cannot refuse).
    state: Option<alloc::boxed::Box<[State]>>,
}

/// Vocabulary's interface-7 state: the screens open.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) ui: Option<Ui>,
    decks: Screens<DecksView>,
    reviews: Screens<ReviewScreen>,
    words: Screens<WordsView>,
    /// The sleep card's screen (one at a time).
    asleep: Screens<crate::native_sleep_card::Asleep>,
    /// The widgets screens show.
    widgets: crate::native_widgets::Widgets,
    /// The card files' menu, and the screens that update from them or save.
    pub(crate) sd_menus: Screens<crate::native_sd::MenuView>,
    pub(crate) sd_runs: Screens<crate::native_sd::Run>,
    /// The word a question about deleting is asked of, and the question.
    asked: Option<(RequestId, Word, Asked)>,
}

impl Plugin for NativeVocabulary {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::EVENTS)
            .chain(image::STAGES)
            .chain(image::WIDGETS)
            .chain(image::NAVIGATION)
            .chain(image::HELP)
            .chain(image::UI)
        {
            registrations::submit(host, batch)?;
        }
        // The services are opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        // What needs no state makes none: the start only puts right what a
        // power cut left half written, and a tick has nothing to do while no
        // screen or widget exists.
        match (self.state.is_none(), token) {
            (true, EVENTS_TOKEN) => State::default().event(host, token, incoming, reply),
            (true, TICK_TOKEN) => Ok(()),
            _ => self.state_of()?.event(host, token, incoming, reply),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, frame: DrawFrame<'_>) -> Result<(), Error> {
        self.state_of()?.draw(host, token, frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        self.state_of()?.input(host, token, input)
    }
}

impl NativeVocabulary {
    /// The state, made on first use.
    fn state_of(&mut self) -> Result<&mut State, Error> {
        if self.state.is_none() {
            let mut one = alloc::vec::Vec::new();
            one.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
            one.push(State::default());
            self.state = Some(one.into_boxed_slice());
        }
        self.state
            .as_mut()
            .and_then(|made| made.first_mut())
            .ok_or(Error::State)
    }
}

impl State {
    pub(crate) fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::Started, kinds::Tick, kinds::WidgetMade, kinds::WidgetDropped, kinds::Resume, kinds::ActionRun, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::Answered, kinds::WorkDue, kinds::StageCall)
        else {
            return Ok(());
        };
        match event {
            Event::Started { .. } if token == EVENTS_TOKEN => {
                recover(&DataStorage::new(host));
                Ok(())
            }
            Event::WidgetMade {
                widget, instance, ..
            } => self.widgets.made(host, widget, instance),
            Event::WidgetDropped { instance, .. } => {
                self.widgets.dropped(instance);
                Ok(())
            }
            Event::StageCall {
                stage: "core.widget",
                payload,
                ..
            } => self.widgets.stage(host, payload, reply),
            Event::Resume { .. } if token == RESUME_TOKEN => self.woke(host),
            Event::ScreenOpened { instance, .. } if token == SLEEP_TOKEN => {
                // A card that cannot be read leaves the screen saying that none
                // is due: the sleep screen must not be dead.
                let asleep = match crate::native_sleep_card::read(host) {
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    other => other.unwrap_or_default(),
                };
                self.asleep.open(instance, asleep)
            }
            Event::Tick { .. } if token == TICK_TOKEN => self.tick(host),
            Event::WorkDue(_) if token == WORK_TOKEN => self.sd_work(host),
            Event::StageCall {
                stage: WORD_STAGE,
                payload,
                ..
            } if token == WORD_TOKEN => self.word_looked_up(host, payload),
            Event::StageCall {
                stage: DECORATE_STAGE,
                payload,
                ..
            } if token == DECORATE_TOKEN => {
                crate::native_marks_stage::decorate(host, &mut self.ui, payload, reply)
            }
            Event::ActionRun {
                action: "vocabulary.flash-cards",
                ..
            } if token == CARDS_TOKEN => {
                CommandSender::new(host, &mut [0; 128]).open_screen(native_decks_screen())
            }
            Event::ActionRun {
                action: "vocabulary.open",
                ..
            } if token == OPEN_TOKEN => {
                CommandSender::new(host, &mut [0; 128]).open_screen(native_list::SCREEN)
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            Event::ScreenOpened { instance, .. } if token == WORDS_TOKEN => {
                self.opened_words(host, instance)
            }
            Event::ScreenOpened { instance, .. } if token == SD_MENU_TOKEN => {
                self.opened_sd_menu(host, instance)
            }
            Event::ScreenOpened {
                instance, handover, ..
            } if token == SD_CARD_TOKEN => self.opened_sd_card(host, instance, handover),
            Event::ScreenOpened { instance, .. } if token == DECKS_TOKEN => {
                self.opened_decks(host, instance)
            }
            Event::ScreenOpened {
                instance, handover, ..
            } if token == REVIEW_TOKEN => self.opened_review(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SD_MENU_TOKEN => self.shown_sd_menu(host, instance),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SD_CARD_TOKEN => self.sd_runs.shown(token, instance),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == DECKS_TOKEN || token == REVIEW_TOKEN || token == WORDS_TOKEN => {
                self.shown(host, token, instance)
            }
            Event::ScreenClosed { instance, .. } => {
                if let Some(review) = self.reviews.get(instance) {
                    let data = DataStorage::new(host);
                    review.close(&DataFolder(&data));
                }
                self.decks.close(instance);
                self.reviews.close(instance);
                self.words.close(instance);
                self.asleep.close(instance);
                self.closed_sd(host, instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub(crate) fn draw(
        &mut self,
        host: &Host<'_>,
        token: u32,
        mut frame: DrawFrame<'_>,
    ) -> Result<(), Error> {
        if frame.purpose == DrawPurpose::Widget {
            return self.widgets.draw(host, frame);
        }
        match token {
            SLEEP_TOKEN => crate::native_sleep_card::draw(
                host,
                &mut frame,
                self.asleep.top(token).ok_or(Error::State)?,
            ),
            DECKS_TOKEN => self
                .decks
                .top(token)
                .ok_or(Error::State)?
                .draw(host, &mut frame),
            REVIEW_TOKEN => self
                .reviews
                .top(token)
                .ok_or(Error::State)?
                .draw(host, &mut frame),
            SD_MENU_TOKEN | SD_CARD_TOKEN => self.draw_sd(host, token, &mut frame),
            WORDS_TOKEN => {
                let view = self.words.top(token).ok_or(Error::State)?;
                let data = DataStorage::new(host);
                view.draw(
                    host,
                    &mut frame,
                    &DataFolder(&data),
                    &mut HostWords::new(host)?,
                )
            }
            _ => Err(Error::State),
        }
    }

    pub(crate) fn input(
        &mut self,
        host: &Host<'_>,
        token: u32,
        input: Input<'_>,
    ) -> Result<Handled, Error> {
        // A tap on a widget opens the list of words, or the flash cards.
        if let Some(tap) = self.widgets.tapped(&input) {
            let screen = match tap {
                Tap::Words => native_list::SCREEN,
                Tap::Cards => native_decks_screen(),
            };
            CommandSender::new(host, &mut [0; 128]).open_screen(screen)?;
            return Ok(Handled::Yes);
        }
        match token {
            DECKS_TOKEN => {
                let Some(view) = self.decks.top(token) else {
                    return Ok(Handled::No);
                };
                let ui = self.ui.as_ref().ok_or(Error::State)?;
                view.input(host, ui, &input)
            }
            REVIEW_TOKEN => {
                let Some(screen) = self.reviews.top(token) else {
                    return Ok(Handled::No);
                };
                let data = DataStorage::new(host);
                screen.input(host, &DataFolder(&data), &input)
            }
            SD_MENU_TOKEN | SD_CARD_TOKEN => self.input_sd(host, token, &input),
            WORDS_TOKEN => {
                let Some(view) = self.words.top(token) else {
                    return Ok(Handled::No);
                };
                let step = view.input(host, &input)?;
                // The page the focus is on now is read before it is drawn. A
                // read that fails leaves the draw to read it, and the step
                // (a word held, *Export*) still runs.
                if step != native_list::Step::Ignored {
                    let data = DataStorage::new(host);
                    if view.prepare(host, &DataFolder(&data)).is_err() {
                        view.forget_page();
                    }
                }
                match step {
                    native_list::Step::Held(row) => self.held(host, row)?,
                    native_list::Step::Export => self.export(host)?,
                    native_list::Step::Tapped => {
                        // Looking the word up waits for its panel.
                        let said = HostWords::new(host)?.copy("vocabulary.hold-to-delete")?;
                        CommandSender::new(host, &mut [0; 256]).notify(Notify::new(&said)?)?;
                    }
                    _ => {}
                }
                Ok(native_list::handled(&step))
            }
            _ => Ok(Handled::No),
        }
    }
}

/// The decks' screen, as the action opens it.
fn native_decks_screen() -> &'static str {
    crate::native_decks::SCREEN
}

impl State {
    pub(crate) fn ensure(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    fn opened_words(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let data = DataStorage::new(host);
        let folder = DataFolder(&data);
        let count = store::count(&folder).map_err(native_review::storage_error)?;
        let mut view = WordsView::new(count);
        view.prepare(host, &folder)?;
        self.words.open(instance, view)
    }

    /// A word held: asks whether to delete it (a word deleted cannot be had
    /// back). One question at a time: the first is still open and its answer is
    /// the one that counts.
    fn held(&mut self, host: &Host<'_>, row: usize) -> Result<(), Error> {
        if self.asked.is_some() {
            return Ok(());
        }
        self.ensure(host)?;
        let count = self.words.top(WORDS_TOKEN).ok_or(Error::State)?.count();
        let data = DataStorage::new(host);
        let folder = DataFolder(&data);
        // Rows are numbered from the newest: a list that changed since the
        // screen counted it would name another word. Counted again instead.
        if store::count(&folder).map_err(native_review::storage_error)? != count {
            return self.recount(host);
        }
        let word = match WordRows::new(&folder, count).word(row) {
            Ok(word) => word,
            // The list changed under the screen: counted again.
            Err(Error::NotFound) => return self.recount(host),
            Err(error) => return Err(error),
        };
        // The book is there: go to it, or delete the word. Looking the word up
        // waits for the dictionary's panel (`PORTING-7.md`).
        if native_goto::book_there(host, &word)? {
            return self.ask_options(host, word);
        }
        self.ask_delete(host, word)
    }

    /// Asks what to do with a word whose book is on the card.
    fn ask_options(&mut self, host: &Host<'_>, word: Word) -> Result<(), Error> {
        let mut words = HostWords::new(host)?;
        let choices = text::concat(&[
            &words.copy("vocabulary.option.go-to")?,
            "\n",
            &words.copy("vocabulary.option.delete")?,
        ])?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let question = Question::choice(&word.word, "", &choices)?;
        let request = ui.ask(host, question, &mut input, &mut out)?;
        self.asked = Some((request, word, Asked::Options));
        Ok(())
    }

    /// Asks whether to delete `word` (a word deleted cannot be had back).
    fn ask_delete(&mut self, host: &Host<'_>, word: Word) -> Result<(), Error> {
        self.ensure(host)?;
        let texts = native_list::delete_texts(&mut HostWords::new(host)?, &word.word)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let request = ui.ask(host, texts.question()?, &mut input, &mut out)?;
        self.asked = Some((request, word, Asked::Delete));
        Ok(())
    }

    /// *Export* chosen: the words are written to `Notes/Vocabulary.tsv`, and the
    /// reader is told where, or that they could not be.
    fn export(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut words = HostWords::new(host)?;
        let said = match crate::native_export::run(host) {
            Ok(path) => words.format(
                "vocabulary.exported",
                &[Arg::Text {
                    name: "path",
                    value: &path,
                }],
            )?,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => words.copy("vocabulary.export-failed")?,
        };
        // The words are written (or not) either way: a notice the host refuses
        // is lost, not an error of the export.
        let Ok(notify) = Notify::new(&said) else {
            return Ok(());
        };
        match CommandSender::new(host, &mut [0; 1024]).notify(notify) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            _ => Ok(()),
        }
    }

    /// The answer to a question about a word: the book opens, the question
    /// about deleting is asked, or the word goes where it is yes; the list is
    /// counted again then.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        let Some((request, _, _)) = &self.asked else {
            return Ok(());
        };
        if *request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((_, word, asked)) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        match (asked, answer.choice()) {
            (Asked::Options, 0) => {
                self.ensure(host)?;
                let ui = self.ui.as_ref().ok_or(Error::State)?;
                native_goto::open_book(host, ui, &word)
            }
            (Asked::Options, 1) => self.ask_delete(host, word),
            (Asked::Delete, 1) => {
                let data = DataStorage::new(host);
                let (deleted, count) = native_list::delete(&DataFolder(&data), &word);
                if !deleted {
                    let said = HostWords::new(host)?.copy("vocabulary.delete-failed")?;
                    CommandSender::new(host, &mut [0; 256]).notify(Notify::new(&said)?)?;
                }
                self.recounted(host, count)
            }
            _ => Ok(()),
        }
    }

    /// The words are counted again and the list drawn again.
    fn recount(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let data = DataStorage::new(host);
        let count = store::count(&DataFolder(&data)).map_err(native_review::storage_error)?;
        self.recounted(host, count)
    }

    fn recounted(&mut self, host: &Host<'_>, count: usize) -> Result<(), Error> {
        if let Some(view) = self.words.top(WORDS_TOKEN) {
            view.recount(count);
            let data = DataStorage::new(host);
            view.prepare(host, &DataFolder(&data))?;
            CommandSender::new(host, &mut [0; 64]).queue_redraw(RedrawMode::Partial)?;
        }
        Ok(())
    }

    /// The decks and how many cards each has, with what is due now. The
    /// words looked up that wait become cards first.
    fn numbers(host: &Host<'_>) -> Result<Numbers, Error> {
        let data = DataStorage::new(host);
        let storage = DataFolder(&data);
        let _ = cards::take_lookups(&storage, &lookup_deck(host)?);
        let time = HostTime::read(host, &storage)?;
        let due = due_now(&storage, &time, settings(host)?.new_per_day);
        // The card's row is offered where the card's files can be read (the
        // service may be missing, or the card out).
        let folder = crate::native_sd_shell::folder(host)?;
        let files = crate::native_card::with_both(host, |both| {
            Ok(sync::pending(both, &folder).unwrap_or_default())
        });
        let pending = files.as_ref().copied().unwrap_or_default();
        let mut numbers =
            Numbers::read(&storage, due, &folder, pending).map_err(|_| Error::NoMemory)?;
        numbers.files = files.is_ok();
        Ok(numbers)
    }

    fn opened_decks(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        self.ensure(host)?;
        let numbers = Self::numbers(host)?;
        let view = DecksView::new(host, &numbers)?;
        self.decks.open(instance, view)?;
        // Opening the flash cards updates the decks from the files that are
        // new or changed, unless the setting says not to.
        if numbers.files && numbers.pending.todo > 0 && crate::native_sd_shell::auto(host)? {
            self.auto_update(host)?;
        }
        Ok(())
    }

    fn opened_review(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        let (deck, first) = match handover {
            Some(handover) => {
                let (mut input, mut out) = (buffer(4096)?, buffer(4096)?);
                let params = self
                    .ui
                    .as_ref()
                    .ok_or(Error::State)?
                    .params(host, handover, &mut input, &mut out)?;
                native_review::named(params.map(|param| param.map(|p| (p.key(), p.value()))))?
            }
            None => (None, None),
        };
        let data = DataStorage::new(host);
        let screen = ReviewScreen::new(host, &DataFolder(&data), deck, first)?;
        self.reviews.open(instance, screen)
    }

    /// A screen came on top: the decks are counted again (a review changed
    /// what is due).
    fn shown(&mut self, host: &Host<'_>, token: u32, instance: Instance) -> Result<(), Error> {
        if token == WORDS_TOKEN {
            // Back from a dialog: with the words looked up meanwhile.
            self.words.shown(token, instance)?;
            return self.recount(host);
        }
        self.decks_or_reviews(token, instance)?;
        if token == DECKS_TOKEN {
            let numbers = Self::numbers(host)?;
            let Some(old) = self.decks.top(token) else {
                return Ok(());
            };
            let focus = old.focus();
            let mut view = DecksView::new(host, &numbers)?;
            view.set_focus(focus);
            *old = view;
        }
        Ok(())
    }

    fn decks_or_reviews(&mut self, token: u32, instance: Instance) -> Result<(), Error> {
        if token == DECKS_TOKEN {
            self.decks.shown(token, instance)
        } else {
            self.reviews.shown(token, instance)
        }
    }

    /// A word was looked up: kept as the settings say; where it cannot be, the
    /// reader is told (the call itself is answered: there is nothing to reply).
    fn word_looked_up(&mut self, host: &Host<'_>, payload: &[u8]) -> Result<(), Error> {
        use crate::stages_subscribed::reader_word_looked_up_v1::WordLooked;
        let looked = WordLooked::decode(payload).map_err(|_| Error::Invalid)?;
        let mut scratch = [0u8; 64];
        let mut on = |key: &str, default: bool| match paperpaw_sdk::native::settings::read_setting(
            host,
            key,
            &mut scratch,
        )? {
            paperpaw_sdk::native::settings::SettingValue::Bool(value) => Ok::<bool, Error>(value),
            _ => Ok(default),
        };
        let collect = on("vocabulary.collect", true)?;
        let from_lookups = on("vocabulary.cards-from-lookups", false)?;
        let data = DataStorage::new(host);
        if crate::native_collect::keep(&DataFolder(&data), collect, from_lookups, &looked).is_err()
        {
            let said = HostWords::new(host)?.copy("vocabulary.save-failed")?;
            CommandSender::new(host, &mut [0; 256]).notify(Notify::new(&said)?)?;
        }
        // The widgets show the words: ask them again.
        self.widgets.changed(host);
        Ok(())
    }

    /// Waking: the review opens with the card the sleep screen showed, where
    /// *A flash card due* was the sleep screen, or where wanted
    /// (`vocabulary.review-on-wake`) and cards are due.
    fn woke(&mut self, host: &Host<'_>) -> Result<(), Error> {
        use paperpaw_sdk::native::settings::{SettingValue, read_setting};
        let data = DataStorage::new(host);
        let folder = DataFolder(&data);
        let mut scratch = [0u8; 64];
        // A setting that is absent or refused: the sleep card was not chosen.
        let asleep = matches!(
            read_setting(host, "sleep.show", &mut scratch),
            Ok(SettingValue::Text(chosen)) if chosen == SLEEP_CARD
        );
        let shown = if asleep {
            crate::native_sleep_card::shown(&folder)
        } else {
            None
        };
        if shown.is_none() {
            let wanted = matches!(
                read_setting(host, "vocabulary.review-on-wake", &mut scratch),
                Ok(SettingValue::Bool(true))
            );
            let time = HostTime::read(host, &folder)?;
            if !wanted || due_now(&folder, &time, settings(host)?.new_per_day) == 0 {
                return Ok(());
            }
        }
        self.ensure(host)?;
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        native_decks::open_review(host, ui, shown)
    }

    /// The minute tick: a card answered *Again* may be due in the review.
    fn tick(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.widgets.tick(host);
        let Some(instance) = self.reviews.top_instance(REVIEW_TOKEN) else {
            return Ok(());
        };
        self.ensure(host)?;
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let Some(screen) = self.reviews.get(instance) else {
            return Ok(());
        };
        let data = DataStorage::new(host);
        screen
            .tick(host, &DataFolder(&data), ui, instance)
            .map(|_| ())
    }
}

/// The name of the deck the words looked up go into.
fn lookup_deck(host: &Host<'_>) -> Result<String, Error> {
    HostWords::new(host)?.copy("vocabulary.cards.looked-up")
}

/// Puts right what a power cut left: the word list, then the decks. (Best
/// effort, as on interface 6: what cannot be put right now is tried at the
/// next start.)
pub(crate) fn recover(storage: &dyn Storage) {
    let folder = DataFolder(storage);
    store::recover(&folder);
    cards::recover(&folder);
}

#[cfg(test)]
mod tests;
