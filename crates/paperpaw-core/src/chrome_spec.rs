//! Bars that stay across the screens shown: tabs, a dock, a bar of
//! one's own along an edge (see [`ChromeSpec`]).
//!
//! A bar is an ordinary screen, made by its factory
//! for a strip along an edge of the screen; the screens below it get the
//! rest. It draws into its strip (its `(0, 0)` is the strip's corner, and
//! [`Env::content`](paperpaw_sdk::Env::content) is the strip), takes the taps,
//! long presses and swipes that begin in it, hears every event, and stays
//! as screens open and close. Whether it shows over a screen is its own
//! choice ([`ChromeSpec::shown`]), e.g. not over the reader; where a
//! screen guards the device (the lock), no bar shows. On devices without
//! touch, what a bar does is better bound to buttons as actions
//! (`tabs.next`), and the bar left to touch devices ([`ChromeSpec::need`]).

use core::fmt;

pub use paperpaw_sdk::ui::look::Edge;
use paperpaw_sdk::{Env, Id, MenuNeed, Priority, StackEntry};

/// A bar along an edge of the screen, made by its factory as a layer.
#[non_exhaustive]
pub struct ChromeSpec {
    pub id: Id,
    pub edge: Edge,
    /// Makes the bar when it is first shown, and again after it was not.
    pub factory: crate::LayerFactory,
    /// How wide it is across its edge, in pixels: never more than half of
    /// the screen.
    pub size: fn(&Env<'_>) -> u16,
    /// Whether it shows over the full screen shown (`entry`), e.g. not
    /// over the reader. By default always.
    pub shown: fn(&Env<'_>, &StackEntry) -> bool,
    /// What the device must have for it, e.g. a touch screen.
    pub need: Option<MenuNeed>,
    /// Bars along one edge lie the one of higher priority outermost.
    pub priority: Priority,
}

impl ChromeSpec {
    /// A bar along `edge`, made by `factory`, a centimetre wide
    /// until [`size`](Self::size) says otherwise, shown over every screen.
    pub fn new(id: Id, edge: Edge, factory: crate::LayerFactory) -> Self {
        Self {
            id,
            edge,
            factory,
            size: |env| env.metrics().mm(10) as u16,
            shown: |_, _| true,
            need: None,
            priority: Priority::DEFAULT,
        }
    }

    pub fn size(mut self, size: fn(&Env<'_>) -> u16) -> Self {
        self.size = size;
        self
    }

    pub fn shown(mut self, shown: fn(&Env<'_>, &StackEntry) -> bool) -> Self {
        self.shown = shown;
        self
    }

    pub fn need(mut self, need: MenuNeed) -> Self {
        self.need = Some(need);
        self
    }

    pub fn priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }
}

impl fmt::Debug for ChromeSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChromeSpec")
            .field("id", &self.id)
            .field("edge", &self.edge)
            .field("need", &self.need)
            .field("priority", &self.priority)
            .finish_non_exhaustive()
    }
}
