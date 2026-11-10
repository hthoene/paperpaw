//! *Edit with your phone* on the native plugin (interface 6: `phone`): the
//! screen's state and what it draws. While the screen is open the device serves
//! a page to the local network (`core.net` `serve`) and shows the address as a
//! QR code and as text; the requests of the page are answered by
//! `native_phone_web`, the server and its events by `native_phone_shell`.
//! The address carries a key drawn anew each time the page is served from the
//! device's random source (`native_key`), so a safe key or no page.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::callbacks::{DrawFrame, Input};
use paperpaw_sdk::native::net::Reason;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::qr::QrCode;
use paperpaw_sdk::served::Guard;
use paperpaw_ui_kit::{Footer, Item, ItemList};

use crate::native_phone_web::Done;
use crate::native_words::Words;

/// The screen's title.
const TITLE: &str = "vocabulary.phone.title";

/// What the screen shows.
#[derive(Debug)]
pub(crate) enum State {
    /// Said instead of the address: starting, or why there is none.
    Message(String),
    /// Serving at `url`, with its code (none where it could not be made).
    Serving { url: String, qr: Option<QrCode> },
}

/// One open phone screen.
#[derive(Debug)]
pub(crate) struct PhoneView {
    state: State,
    /// What is said with the address: how to use it, and what the phone did
    /// last.
    list: ItemList,
    /// The key of this time of serving and the wrong ones tried.
    pub(crate) guard: Option<Guard>,
    /// Whether serving was asked for (once for each time the screen opens).
    pub(crate) started: bool,
    last: Option<String>,
}

impl PhoneView {
    /// The screen as opened: saying that it starts.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the catalog refuses.
    pub(crate) fn new(words: &mut impl Words) -> Result<Self, Error> {
        Ok(Self {
            state: State::Message(words.copy("vocabulary.phone.starting")?),
            list: ItemList::default(),
            guard: None,
            started: false,
            last: None,
        })
    }

    /// Whether the address is shown (and so a key is held).
    pub(crate) fn serving(&self) -> bool {
        matches!(self.state, State::Serving { .. })
    }

    /// Shows `state`, with the rows that go with it.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the catalog refuses; the screen is as it
    /// was then.
    pub(crate) fn show(&mut self, words: &mut impl Words, state: State) -> Result<(), Error> {
        let rows = self.rows(words, &state, self.last.as_deref())?;
        self.list.replace(rows);
        self.state = state;
        Ok(())
    }

    /// Says what the phone did last, under the address.
    ///
    /// # Errors
    /// As [`Self::show`].
    pub(crate) fn say(&mut self, words: &mut impl Words, done: &Done) -> Result<(), Error> {
        let said = said(words, done)?;
        let rows = self.rows(words, &self.state, Some(&said))?;
        self.list.replace(rows);
        self.last = Some(said);
        Ok(())
    }

    fn rows(
        &self,
        words: &mut impl Words,
        state: &State,
        last: Option<&str>,
    ) -> Result<Vec<Item>, Error> {
        let mut rows = Vec::new();
        if matches!(state, State::Serving { .. }) {
            rows.try_reserve_exact(2).map_err(|_| Error::NoMemory)?;
            rows.push(Item::noted(&words.copy("vocabulary.phone.hint")?, "", "")?);
            if let Some(last) = last {
                rows.push(Item::noted(last, "", "")?);
            }
        }
        Ok(rows)
    }

    /// Draws the screen under its header.
    ///
    /// # Errors
    /// What the host's fonts, canvas and texts refuse; `NoMemory`.
    pub(crate) fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let footer = match &self.state {
            State::Serving {
                url,
                qr: Some(code),
            } => Footer::Code { code, caption: url },
            State::Serving { url, qr: None } => Footer::Note(url),
            State::Message(words) => Footer::Empty {
                title: words,
                text: "",
            },
        };
        self.list.draw_with(host, frame, TITLE, true, footer)
    }

    /// Only back does anything here.
    ///
    /// # Errors
    /// What the host refuses.
    pub(crate) fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        Ok(ItemList::handled(self.list.input(host, input)?))
    }
}

/// What a request did, in the reader's words.
///
/// # Errors
/// [`Error::NoMemory`] and what the catalog refuses.
fn said(words: &mut impl Words, done: &Done) -> Result<String, Error> {
    use paperpaw_sdk::native::formatting::Arg;
    match done {
        Done::Deck(name) => words.format(
            "vocabulary.phone.deck-saved",
            &[Arg::Text {
                name: "name",
                value: name,
            }],
        ),
        Done::Card(front) => words.format(
            "vocabulary.phone.card-saved",
            &[Arg::Text {
                name: "front",
                value: front,
            }],
        ),
        Done::Deleted => words.copy("vocabulary.phone.deleted"),
        Done::Imported(n) => words.format(
            "vocabulary.phone.imported",
            &[Arg::Count(i64::try_from(*n).unwrap_or(i64::MAX))],
        ),
    }
}

/// The key of the text that says why the page is not served.
pub(crate) const fn reason_word(reason: Reason) -> &'static str {
    match reason {
        Reason::NoNetwork => "no network",
        Reason::Offline => "offline",
        Reason::Saving => "saving",
        Reason::NetworkOff => "network off",
        Reason::Refused => "refused",
        Reason::Ok => "ended",
        _ => "failed",
    }
}
