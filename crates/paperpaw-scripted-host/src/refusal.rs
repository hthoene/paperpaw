//! Which host slot a refusal or a call count is about, and the rules that make
//! the n-th call of a slot fail with a chosen status.
use std::cell::{Cell, RefCell};

/// A host slot a plugin reaches, as far as the scripted host fakes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Slot {
    /// `heap.alloc` and `heap.realloc`: a refusal is a null block with the status.
    Heap,
    /// `commands.submit`.
    Commands,
    /// `registrations.submit`.
    Registrations,
    /// `service.open`.
    ServiceOpen,
    /// `service.call`.
    ServiceCall,
    /// `settings.read`.
    SettingsRead,
    /// `env.snapshot`.
    EnvSnapshot,
}

pub(crate) const SLOTS: usize = 7;

impl Slot {
    pub(crate) fn index(self) -> usize {
        match self {
            Self::Heap => 0,
            Self::Commands => 1,
            Self::Registrations => 2,
            Self::ServiceOpen => 3,
            Self::ServiceCall => 4,
            Self::SettingsRead => 5,
            Self::EnvSnapshot => 6,
        }
    }
}

/// `slot`'s `nth` call (counted from 1 since the host was made or last
/// [`ScriptedHost::reset_counts`](crate::ScriptedHost::reset_counts)) answers
/// `status` and does nothing else; `every` makes all calls from the nth on fail.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rule {
    pub(crate) slot: Slot,
    pub(crate) nth: usize,
    pub(crate) every: bool,
    pub(crate) status: i32,
}

/// Call counters and refusal rules; shared between the slots through `&`.
#[derive(Default)]
pub(crate) struct Refusals {
    calls: [Cell<usize>; SLOTS],
    rules: RefCell<Vec<Rule>>,
}

impl Refusals {
    /// Counts the call, and the status it must answer instead of working.
    pub(crate) fn call(&self, slot: Slot) -> Option<i32> {
        let cell = &self.calls[slot.index()];
        cell.set(cell.get() + 1);
        let n = cell.get();
        self.rules
            .borrow()
            .iter()
            .find(|rule| rule.slot == slot && (rule.nth == n || (rule.every && rule.nth <= n)))
            .map(|rule| rule.status)
    }

    pub(crate) fn calls(&self, slot: Slot) -> usize {
        self.calls[slot.index()].get()
    }

    pub(crate) fn add(&self, rule: Rule) {
        self.rules.borrow_mut().push(rule);
    }

    pub(crate) fn clear_rules(&self) {
        self.rules.borrow_mut().clear();
    }

    pub(crate) fn reset_counts(&self) {
        for cell in &self.calls {
            cell.set(0);
        }
    }
}
