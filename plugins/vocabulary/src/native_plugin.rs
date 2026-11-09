//! The interface-7 Vocabulary: its settings, the recovery at start (a word
//! list or a deck left half written by a power cut is put right), and the
//! flash cards: the decks' screen and the review. The word list, the phone
//! page, the SD card's screens, the marks in books, the sleep card and the
//! widget follow with their host pieces; see `PORTING-7.md`.
use alloc::string::String;

use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, Input},
    commands::CommandSender,
    events::{Event, Incoming, kinds},
    registrations,
    reply::Reply,
    storage::DataStorage,
    ui::{HandoverId, Instance, Ui},
};
use paperpaw_sdk::{Handled, Storage};
use paperpaw_ui_kit::{Screens, mem::buffer, read_event};

use crate::declarations::{CARDS_TOKEN, DECKS_TOKEN, EVENTS_TOKEN, REVIEW_TOKEN, TICK_TOKEN};
use crate::files::DataFolder;
use crate::native_decks::{DecksView, Numbers};
use crate::native_review::{self, due_now};
use crate::native_review_screen::{HostTime, ReviewScreen, settings};
use crate::{cards, store};

mod image {
    include!(concat!(env!("OUT_DIR"), "/vocabulary_declarations.rs"));
}

/// The folder of the SD card's decks is not shown until its screens are
/// ported: the decks' rows leave the phone and the card out.
const NO_FOLDER: &str = "";

/// Vocabulary's interface-7 state: the screens open.
#[derive(Debug, Default)]
pub struct NativeVocabulary {
    ui: Option<Ui>,
    decks: Screens<DecksView>,
    reviews: Screens<ReviewScreen>,
}

impl Plugin for NativeVocabulary {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS.iter().chain(image::EVENTS).chain(image::UI) {
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
        _reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::Started, kinds::Tick, kinds::ActionRun, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed)
        else {
            return Ok(());
        };
        match event {
            Event::Started { .. } if token == EVENTS_TOKEN => {
                recover(&DataStorage::new(host));
                Ok(())
            }
            Event::Tick { .. } if token == TICK_TOKEN => self.tick(host),
            Event::ActionRun {
                action: "vocabulary.flash-cards",
                ..
            } if token == CARDS_TOKEN => {
                CommandSender::new(host, &mut [0; 128]).open_screen(native_decks_screen())
            }
            Event::ScreenOpened { instance, .. } if token == DECKS_TOKEN => {
                self.opened_decks(host, instance)
            }
            Event::ScreenOpened {
                instance, handover, ..
            } if token == REVIEW_TOKEN => self.opened_review(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == DECKS_TOKEN || token == REVIEW_TOKEN => self.shown(host, token, instance),
            Event::ScreenClosed { instance, .. } => {
                if let Some(review) = self.reviews.get(instance) {
                    let data = DataStorage::new(host);
                    review.close(&DataFolder(&data));
                }
                self.decks.close(instance);
                self.reviews.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        match token {
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
            _ => Err(Error::State),
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
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
            _ => Ok(Handled::No),
        }
    }
}

/// The decks' screen, as the action opens it.
fn native_decks_screen() -> &'static str {
    crate::native_decks::SCREEN
}

impl NativeVocabulary {
    fn ensure(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
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
        Numbers::read(&storage, due, NO_FOLDER, Default::default()).map_err(|_| Error::NoMemory)
    }

    fn opened_decks(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        self.ensure(host)?;
        let view = DecksView::new(host, &Self::numbers(host)?)?;
        self.decks.open(instance, view)
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

    /// The minute tick: a card answered *Again* may be due in the review.
    fn tick(&mut self, host: &Host<'_>) -> Result<(), Error> {
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
    use crate::native_words::{HostWords, Words};
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
