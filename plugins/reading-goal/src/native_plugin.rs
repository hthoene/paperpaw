//! The interface-7 Reading goal: what the statistics tell of today's reading is
//! kept, and written to `today.txt` in the plugin's data folder when a book
//! closes or the device sleeps or stops (not at every page turned). The widget
//! and the sleep screen (the cat) wait for images and the widget host call; see
//! `PORTING-7.md`.
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    events::{BookKind, Event},
    registrations,
    reply::Reply,
    storage::DataStorage,
};

use crate::model::{Kept, parse_stage};
use crate::store;

mod image {
    include!(concat!(env!("OUT_DIR"), "/reading_goal_declarations.rs"));
}

/// The plugin's callback tokens (see `declarations.rs`).
const EVENTS: u32 = crate::declarations::EVENTS_TOKEN;
const TODAY: u32 = crate::declarations::TODAY_TOKEN;
/// The stage's payload schema.
const SCHEMA: u32 = 1;

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
        // A host that does not admit stage subscriptions yet leaves the
        // plugin without the figure; the rest works.
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
        if let Some(edge) = edge(token, &event)
            && self.kept.apply(edge)
        {
            self.flush(host);
        }
        Ok(())
    }
}

/// What happened, as far as the figure is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edge {
    /// The statistics told today's seconds.
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
            TODAY,
            Event::StageCall {
                schema: SCHEMA,
                payload,
                ..
            },
        ) => parse_stage(payload).map(|(day, seconds)| Edge::Told(day, seconds)),
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
