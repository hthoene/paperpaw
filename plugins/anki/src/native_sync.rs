//! The sync with Anki on the computer, one request at a time, over a [`Link`]
//! (the plugin's data folder and the network) so that the decisions are tested
//! without a host:
//!
//! 1. the answers waiting (`answers.txt`) go to Anki with `answerCards`, at
//!    most [`ANSWERS_PER_REQUEST`] at a time; those Anki took leave the queue
//!    at once, and the cards of the last sync (their schedule has changed in
//!    Anki) are dropped;
//! 2. the cards due, then new ones up to the setting, are looked up
//!    (`findCards`, the first [`MAX_CARDS`]) and fetched in small batches
//!    (`cardsInfo`) into `cards.new`, which replaces `cards.txt` when all
//!    came. A batch Anki's answer for is too large for the host's buffer is
//!    halved down to one card, which is then left out.
//!
//! Anki decides what is due: nothing is scheduled here. What fails stays as it
//! was: the answers wait for the next sync, the old cards stay until a sync
//! of answers made them stale.
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::native::{
    Error,
    net::{Reason, RequestId},
};
use paperpaw_ui_kit::files::{Volume, for_each_line_on};
use paperpaw_ui_kit::mem::push;

use crate::native_anki::{
    ANSWERS_PER_REQUEST, Answered, Body, Cards, Fault, INFO_BATCH, Ids, MAX_CARDS, Names, Reply,
    Which, query,
};
use crate::native_cards::{self as store, ANSWERS, CARDS, NEW};
use crate::native_keep::Secret;

/// The plugin's own number of its one request, in its `http_done` event.
pub const REQUEST_TOKEN: u32 = 1;

/// What the sync needs of the world: the data folder and the network.
pub trait Link {
    /// The plugin's data folder.
    fn volume(&self) -> &dyn Volume;
    /// Sends `body` to AnkiConnect, the end told with token [`REQUEST_TOKEN`].
    ///
    /// # Errors
    /// What the network service says.
    fn send(&mut self, body: &Body) -> Result<RequestId, Error>;
    /// Gives the memory body of the request that ended, in pieces.
    ///
    /// # Errors
    /// What the service says, or what `feed` says.
    fn read(
        &mut self,
        done: &Done,
        feed: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
    ) -> Result<(), Error>;
    /// Ends the request in flight.
    fn cancel(&mut self);
}

/// The end of a request, as far as the sync reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Done {
    pub id: RequestId,
    pub token: u32,
    pub reason: Reason,
    pub status: u32,
    /// The body was cut at the buffer.
    pub truncated: bool,
    /// The memory body's length.
    pub body_len: u32,
}

/// What the sync is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Send the answers, fetch the cards.
    Sync,
    /// Fetch the names of the decks.
    Decks,
}

/// The account to sync with.
#[derive(Debug)]
pub struct Account {
    /// The key AnkiConnect asks for, if it does.
    pub key: Option<Secret>,
    /// The deck (and its subdecks); all decks where empty.
    pub deck: String,
    /// New cards a sync fetches at most.
    pub new: usize,
}

/// How a sync ended: a text of the catalog to tell, and what it brought.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Finish {
    pub key: &'static str,
    /// Answers Anki took.
    pub sent: usize,
    /// Cards fetched.
    pub fetched: usize,
    /// What Anki said, where it said something the plugin has no text for.
    pub said: Option<String>,
    /// The deck names ([`Mode::Decks`]).
    pub names: Option<Vec<String>>,
}

impl Finish {
    /// An ending that is only a text.
    #[must_use]
    pub fn said_key(key: &'static str) -> Self {
        Self::said(key)
    }

    fn said(key: &'static str) -> Self {
        Self {
            key,
            ..Self::default()
        }
    }
}

/// What a step came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Under way.
    Quiet,
    /// Ended.
    Ended(Finish),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Send,
    Find(Which),
    Info,
    Names,
}

#[derive(Debug)]
struct Run {
    account: Account,
    phase: Phase,
    pending: Option<RequestId>,
    /// The answers in flight.
    batch: usize,
    sent: usize,
    ids: Vec<u64>,
    /// The next card of `ids` to fetch, and how many at a time.
    at: usize,
    step: usize,
    /// The cards in the batch in flight.
    asked: usize,
    fetched: usize,
}

/// The sync: at most one at a time.
#[derive(Debug, Default)]
pub struct Sync {
    run: Option<Run>,
}

impl Sync {
    /// Whether a sync is under way.
    #[must_use]
    pub const fn running(&self) -> bool {
        self.run.is_some()
    }

    /// Starts a sync (nothing happens where one runs).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn start(
        &mut self,
        link: &mut dyn Link,
        account: Account,
        mode: Mode,
    ) -> Result<Step, Error> {
        if self.run.is_some() {
            return Ok(Step::Quiet);
        }
        let mut run = Run {
            account,
            phase: Phase::Send,
            pending: None,
            batch: 0,
            sent: 0,
            ids: Vec::new(),
            at: 0,
            step: INFO_BATCH,
            asked: 0,
            fetched: 0,
        };
        let step = match mode {
            Mode::Decks => {
                run.phase = Phase::Names;
                let body = Body::deck_names(key(&run))?;
                Self::post(link, &mut run, &body)
            }
            Mode::Sync => Self::send_answers(link, &mut run),
        };
        self.settle(link, run, step)
    }

    /// Ends the sync under way (the plugin stops).
    pub fn cancel(&mut self, link: &mut dyn Link) {
        if self.run.take().is_some() {
            link.cancel();
            store::discard(link.volume(), NEW);
        }
    }

    /// The end of a request.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn done(&mut self, link: &mut dyn Link, done: &Done) -> Result<Step, Error> {
        let Some(mut run) = self.run.take() else {
            return Ok(Step::Quiet);
        };
        if done.token != REQUEST_TOKEN || run.pending != Some(done.id) {
            self.run = Some(run);
            return Ok(Step::Quiet);
        }
        run.pending = None;
        let step = match Self::advance(link, &mut run, done) {
            Ok(step) => step,
            Err(error) => {
                store::discard(link.volume(), NEW);
                return Err(error);
            }
        };
        self.settle(link, run, Ok(step))
    }

    fn settle(
        &mut self,
        link: &mut dyn Link,
        run: Run,
        step: Result<Option<Finish>, Error>,
    ) -> Result<Step, Error> {
        match step {
            Ok(None) => {
                self.run = Some(run);
                Ok(Step::Quiet)
            }
            Ok(Some(finish)) => Ok(Step::Ended(finish)),
            Err(error) => {
                store::discard(link.volume(), NEW);
                Err(error)
            }
        }
    }

    /// Sends `body`; where the network refuses, the sync ends.
    fn post(link: &mut dyn Link, run: &mut Run, body: &Body) -> Result<Option<Finish>, Error> {
        match link.send(body) {
            Ok(id) => {
                run.pending = Some(id);
                Ok(None)
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(Error::Denied) => Ok(Some(Self::over(link, run, "anki.denied"))),
            Err(Error::Unsupported) => Ok(Some(Self::over(link, run, "anki.no-network"))),
            Err(_) => Ok(Some(Self::over(link, run, "anki.unreachable"))),
        }
    }

    /// The end of a sync that failed.
    fn over(link: &mut dyn Link, run: &Run, key: &'static str) -> Finish {
        store::discard(link.volume(), NEW);
        Finish {
            key,
            sent: run.sent,
            ..Finish::default()
        }
    }

    fn fault(link: &mut dyn Link, run: &Run, fault: Fault) -> Finish {
        let mut finish = Self::over(link, run, "anki.bad-key");
        if let Fault::Said(text) = fault {
            finish.key = "anki.said";
            finish.said = Some(text);
        }
        finish
    }

    fn advance(link: &mut dyn Link, run: &mut Run, done: &Done) -> Result<Option<Finish>, Error> {
        if let Some(key) = ended_badly(done) {
            return Ok(Some(Self::over(link, run, key)));
        }
        match run.phase {
            Phase::Send => Self::sent(link, run, done),
            Phase::Find(which) => Self::found(link, run, done, which),
            Phase::Info => Self::info(link, run, done),
            Phase::Names => Self::named(link, run, done),
        }
    }

    /// The next answers go, or the cards are asked for where none wait.
    fn send_answers(link: &mut dyn Link, run: &mut Run) -> Result<Option<Finish>, Error> {
        let mut answers: Vec<(u64, u8)> = Vec::new();
        let read = for_each_line_on(link.volume(), ANSWERS, &mut |line| {
            if let Some(answer) = store::answer_of(line) {
                push(&mut answers, answer)?;
            }
            Ok(if answers.len() >= ANSWERS_PER_REQUEST {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            })
        });
        match read {
            Ok(()) | Err(Error::NotFound) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(Some(Finish::said("anki.unreadable"))),
        }
        if answers.is_empty() {
            return Self::find(link, run, Which::Due);
        }
        run.phase = Phase::Send;
        run.batch = answers.len();
        let body = Body::answer_cards(key(run), &answers)?;
        Self::post(link, run, &body)
    }

    fn sent(link: &mut dyn Link, run: &mut Run, done: &Done) -> Result<Option<Finish>, Error> {
        let mut reading = Answered::new();
        link.read(done, &mut |piece| reading.feed(piece))
            .or_else(forgive)?;
        match reading.finish()? {
            Reply::Ok(_) => {}
            Reply::Fault(fault) => return Ok(Some(Self::fault(link, run, fault))),
            Reply::Unknown => return Ok(Some(Self::over(link, run, "anki.bad-reply"))),
        }
        // Anki took them (a card it no longer has is not taken, and not kept).
        match store::drop_first(link.volume(), run.batch) {
            Ok(()) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(Some(Self::over(link, run, "anki.unreadable"))),
        }
        run.sent += run.batch;
        // What the cards were has changed in Anki.
        match store::remove(link.volume(), CARDS) {
            Ok(()) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(Some(Self::over(link, run, "anki.unreadable"))),
        }
        Self::send_answers(link, run)
    }

    fn find(link: &mut dyn Link, run: &mut Run, which: Which) -> Result<Option<Finish>, Error> {
        run.phase = Phase::Find(which);
        let search = query(&run.account.deck, which)?;
        let body = Body::find_cards(key(run), &search)?;
        Self::post(link, run, &body)
    }

    fn found(
        link: &mut dyn Link,
        run: &mut Run,
        done: &Done,
        which: Which,
    ) -> Result<Option<Finish>, Error> {
        let limit = match which {
            Which::Due => MAX_CARDS,
            Which::New => run.account.new.min(MAX_CARDS - run.ids.len()),
        };
        let mut reading = Ids::new(limit);
        link.read(done, &mut |piece| reading.feed(piece))
            .or_else(forgive)?;
        match reading.finish()? {
            Reply::Ok(ids) => {
                for id in ids {
                    if !run.ids.contains(&id) {
                        push(&mut run.ids, id)?;
                    }
                }
            }
            Reply::Fault(fault) => return Ok(Some(Self::fault(link, run, fault))),
            Reply::Unknown => return Ok(Some(Self::over(link, run, "anki.bad-reply"))),
        }
        if which == Which::Due && run.account.new > 0 && run.ids.len() < MAX_CARDS {
            return Self::find(link, run, Which::New);
        }
        if run.ids.is_empty() {
            // Nothing is due: the old cards are stale too.
            store::discard(link.volume(), CARDS);
            return Ok(Some(Self::ended(run)));
        }
        match store::remove(link.volume(), NEW) {
            Ok(()) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(Some(Self::over(link, run, "anki.unreadable"))),
        }
        Self::ask(link, run)
    }

    /// Asks for the next batch of cards, or puts the cards in place.
    fn ask(link: &mut dyn Link, run: &mut Run) -> Result<Option<Finish>, Error> {
        if run.at >= run.ids.len() {
            let placed = if run.fetched == 0 {
                store::remove(link.volume(), CARDS)
            } else {
                link.volume().rename(NEW, CARDS)
            };
            return match placed {
                Ok(()) => Ok(Some(Self::ended(run))),
                Err(Error::NoMemory) => Err(Error::NoMemory),
                Err(_) => Ok(Some(Self::over(link, run, "anki.unreadable"))),
            };
        }
        run.phase = Phase::Info;
        let end = (run.at + run.step).min(run.ids.len());
        run.asked = end - run.at;
        let body = Body::cards_info(key(run), &run.ids[run.at..end])?;
        Self::post(link, run, &body)
    }

    fn info(link: &mut dyn Link, run: &mut Run, done: &Done) -> Result<Option<Finish>, Error> {
        if done.truncated {
            // Too much for one answer: fewer cards at a time, or none for this one.
            if run.step > 1 {
                run.step = run.step.div_ceil(2);
            } else {
                run.at += 1;
            }
            return Self::ask(link, run);
        }
        let mut reading = Cards::new();
        link.read(done, &mut |piece| reading.feed(piece))
            .or_else(forgive)?;
        match reading.finish()? {
            Reply::Ok(cards) => {
                for card in &cards {
                    match store::append_new(link.volume(), card) {
                        Ok(()) => run.fetched += 1,
                        Err(Error::NoMemory) => return Err(Error::NoMemory),
                        Err(_) => return Ok(Some(Self::over(link, run, "anki.unreadable"))),
                    }
                }
            }
            Reply::Fault(fault) => return Ok(Some(Self::fault(link, run, fault))),
            Reply::Unknown => return Ok(Some(Self::over(link, run, "anki.bad-reply"))),
        }
        run.at += run.asked;
        Self::ask(link, run)
    }

    fn named(link: &mut dyn Link, run: &mut Run, done: &Done) -> Result<Option<Finish>, Error> {
        let mut reading = Names::new();
        link.read(done, &mut |piece| reading.feed(piece))
            .or_else(forgive)?;
        match reading.finish()? {
            Reply::Ok(names) => Ok(Some(Finish {
                key: "anki.decks-listed",
                names: Some(names),
                ..Finish::default()
            })),
            Reply::Fault(fault) => Ok(Some(Self::fault(link, run, fault))),
            Reply::Unknown => Ok(Some(Self::over(link, run, "anki.bad-reply"))),
        }
    }

    fn ended(run: &Run) -> Finish {
        Finish {
            key: if run.fetched == 0 {
                "anki.nothing-due"
            } else {
                "anki.synced"
            },
            sent: run.sent,
            fetched: run.fetched,
            ..Finish::default()
        }
    }
}

fn key(run: &Run) -> Option<&str> {
    run.account.key.as_ref().map(Secret::as_str)
}

/// A body that cannot be read is no answer (an unknown one); memory is
/// another thing.
fn forgive(error: Error) -> Result<(), Error> {
    match error {
        Error::NoMemory => Err(Error::NoMemory),
        _ => Ok(()),
    }
}

/// The text for a request that did not end in an answer of AnkiConnect.
fn ended_badly(done: &Done) -> Option<&'static str> {
    match done.reason {
        Reason::Ok => match done.status {
            200..=299 => None,
            401 | 403 => Some("anki.bad-key"),
            _ => Some("anki.bad-reply"),
        },
        Reason::NoNetwork | Reason::Offline | Reason::Saving => Some("anki.no-network"),
        Reason::NetworkOff => Some("anki.denied"),
        Reason::Cancelled | Reason::Replaced => Some("anki.cancelled"),
        _ => Some("anki.unreachable"),
    }
}
