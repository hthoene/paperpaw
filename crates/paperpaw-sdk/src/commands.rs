//! What a handler sends to the kernel ([`Commands`]).

use alloc::vec::Vec;

use crate::command::Command;
use crate::event::{Event, EventData};

/// What a handler sends to the kernel: the commands it queues, in order.
///
/// Every handler of a plugin gets one (actions, events, observers, work
/// steps, voices, widgets' input, installers). It is the part of a
/// handler's signature that grows: what is added to it later, such as
/// which plugin runs, its limits or small helpers, reaches every handler
/// through a method, without a new signature. That is why its fields are
/// private.
///
/// It reads as the slice of the commands queued so far
/// (`commands.iter()`, `commands[0]`, `commands.len()`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Commands {
    queued: Vec<Command>,
}

impl Commands {
    /// None queued yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues `command`.
    pub fn push(&mut self, command: Command) {
        self.queued.push(command);
    }

    /// Queues `event` with `data` ([`Command::Emit`]).
    pub fn emit<P: EventData>(&mut self, event: &Event<P>, data: P) {
        self.queued.push(event.emit(data));
    }

    /// Takes every command queued, leaving none (for the kernel, which
    /// runs them).
    #[cfg(feature = "kernel")]
    pub fn take(&mut self) -> Vec<Command> {
        core::mem::take(&mut self.queued)
    }
}

impl core::ops::Deref for Commands {
    type Target = [Command];

    fn deref(&self) -> &[Command] {
        &self.queued
    }
}

impl Extend<Command> for Commands {
    fn extend<I: IntoIterator<Item = Command>>(&mut self, commands: I) {
        self.queued.extend(commands);
    }
}

impl FromIterator<Command> for Commands {
    fn from_iter<I: IntoIterator<Item = Command>>(commands: I) -> Self {
        Self {
            queued: commands.into_iter().collect(),
        }
    }
}

impl From<Vec<Command>> for Commands {
    fn from(queued: Vec<Command>) -> Self {
        Self { queued }
    }
}

impl IntoIterator for Commands {
    type Item = Command;
    type IntoIter = alloc::vec::IntoIter<Command>;

    fn into_iter(self) -> Self::IntoIter {
        self.queued.into_iter()
    }
}

impl<'a> IntoIterator for &'a Commands {
    type Item = &'a Command;
    type IntoIter = core::slice::Iter<'a, Command>;

    fn into_iter(self) -> Self::IntoIter {
        self.queued.iter()
    }
}
