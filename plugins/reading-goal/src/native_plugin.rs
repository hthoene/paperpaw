//! The interface-7 Reading goal: it asks the statistics for today's reading
//! (the stage `reading-goal.read-today`, which they answer from memory) when a
//! book closes or the device sleeps or stops, and writes it to `today.txt` in
//! the plugin's data folder. The widget and the sleep screen (the cat) wait for
//! images and the widget host call; they will ask on drawing and on the minute
//! tick. See `PORTING-7.md`.
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    events::{BookKind, Event},
    registrations,
    reply::Reply,
    stages::Stage,
    storage::DataStorage,
};

use crate::model::{Kept, parse_stage};
use crate::store;

mod image {
    include!(concat!(env!("OUT_DIR"), "/reading_goal_declarations.rs"));
}

/// The plugin's callback tokens (see `declarations.rs`).
const EVENTS: u32 = crate::declarations::EVENTS_TOKEN;
/// The stage this plugin owns and runs, by its full id.
const READ_TODAY: &str = "reading-goal.read-today";
/// What a reply of the stage can take (a day and the seconds: 8 bytes).
const REPLY: usize = Stage::out_len(8);

/// Reading goal's interface-7 state.
#[derive(Debug, Default)]
pub struct NativeReadingGoal {
    kept: Kept,
}

impl Plugin for NativeReadingGoal {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS.iter().chain(image::EVENTS) {
            registrations::submit(host, batch)?;
        }
        // A host that does not admit stages yet leaves the plugin without
        // the figure; the rest works.
        for batch in image::STAGES {
            match registrations::submit(host, batch) {
                Ok(()) | Err(Error::Unsupported) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        event: Event<'_>,
        _reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(edge) = edge(token, &event) else {
            return Ok(());
        };
        // Asked when the figure is about to be written, not at every page.
        if matches!(edge, Edge::BookClosed | Edge::Sleeping)
            && let Some((day, seconds)) = ask(host)
        {
            self.kept.apply(Edge::Told(day, seconds));
        }
        if self.kept.apply(edge) {
            self.flush(host);
        }
        Ok(())
    }
}

/// The plugin that answers: the reading statistics.
const STATISTICS: &str = "stats";

/// Asks the statistics for today's reading. Nothing where they do not answer:
/// not running yet (they start afterwards, the next ask is answered), no stage
/// for them, a day they do not know, or an answer that is not a figure. Every
/// subscriber is read to the end, and only the statistics' reply counts: any
/// other plugin on the stage is not heard.
fn ask(host: &Host<'_>) -> Option<(i32, u32)> {
    let stage = Stage::new(host, READ_TODAY).ok()?;
    let mut out = [0u8; REPLY];
    let mut run = stage.run(&[], &mut out);
    let mut figure_told = None;
    // At most 32 subscribers are active on a stage (docs/abi/stages-services.md).
    for _ in 0..33 {
        let Ok(Some(reply)) = run.next() else {
            break;
        };
        if let Some(told) = answer_of(reply.subscriber(), reply.status(), reply.payload()) {
            figure_told = Some(told);
        }
    }
    figure_told
}

/// The figure a subscriber's reply holds: only the statistics', and only a
/// successful, well-formed one.
pub(crate) fn answer_of(
    subscriber: &str,
    status: Result<(), Error>,
    payload: &[u8],
) -> Option<(i32, u32)> {
    if subscriber != STATISTICS {
        return None;
    }
    figure(status, payload)
}

/// The figure a reply holds.
pub(crate) fn figure(status: Result<(), Error>, payload: &[u8]) -> Option<(i32, u32)> {
    status.ok()?;
    parse_stage(payload)
}

/// What happened, as far as the figure is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edge {
    /// The statistics answered with today's seconds.
    Told(i32, u32),
    BookOpened,
    BookClosed,
    /// The device sleeps or stops.
    Sleeping,
}

/// The edge an event at `token` is, if it is one.
pub(crate) fn edge(token: u32, event: &Event<'_>) -> Option<Edge> {
    match (token, event) {
        (
            EVENTS,
            Event::Book {
                kind: BookKind::Opened,
                ..
            },
        ) => Some(Edge::BookOpened),
        (
            EVENTS,
            Event::Book {
                kind: BookKind::Closed,
                ..
            },
        ) => Some(Edge::BookClosed),
        (EVENTS, Event::Suspend { .. } | Event::Stopping { .. }) => Some(Edge::Sleeping),
        _ => None,
    }
}

impl Kept {
    /// Takes `edge` in; whether to write `today.txt` now. Not while pages turn:
    /// the card is slow to write to.
    pub(crate) fn apply(&mut self, edge: Edge) -> bool {
        match edge {
            Edge::Told(day, seconds) => self.read_today(day, seconds),
            Edge::BookOpened => {
                self.book_opened();
                false
            }
            Edge::BookClosed => self.book_closed(),
            Edge::Sleeping => self.must_flush(),
        }
    }
}

impl NativeReadingGoal {
    /// Writes what was told where the file holds something else. A failed
    /// write is not told: the figure is kept and written at the next edge, as
    /// on interface 6.
    fn flush(&self, host: &Host<'_>) {
        store::flush(&DataStorage::new(host), &self.kept).ok();
    }
}

#[cfg(test)]
mod tests;
