//! The host's own question dialogs (`docs/abi/ui.md` 3.1 and 3.2), used when no
//! dialog plugin serves the slot: kind 1 (choice) and kind 2 (confirm). They
//! answer the request store directly, by the same path as op 11 (only the
//! instance the host opened may answer, the first final answer wins), and
//! send nothing as an event. *Back* and a close with no answer cancel, which
//! the stack's close tells the store.
//!
//! Kinds 0 (text) and 3 (folder) have no host dialog: without a dialog plugin
//! they end as cancelled, as the pump leaves them.
use super::{shared::Shared, ui_store::UiStore};
use alloc::{string::String, vec::Vec};

#[path = "dialogs/choice.rs"]
mod choice;
#[path = "dialogs/confirm.rs"]
mod confirm;

pub(crate) use choice::Choice;
pub(crate) use confirm::Confirm;

/// The question of a request, copied out of the store to make its dialog:
/// the lines as the asker wrote them (no split on commas, equal lines kept).
pub(crate) struct Asked {
    pub(crate) kind: u32,
    pub(crate) flags: u32,
    pub(crate) title: String,
    pub(crate) text: String,
    pub(crate) lines: Vec<String>,
}

/// Where a host dialog's answer goes: the request, and the one instance the
/// host opened for it.
pub(crate) struct Answerer {
    store: Shared<UiStore>,
    instance: u32,
    request: u32,
}

impl Answerer {
    pub(super) fn new(store: Shared<UiStore>, instance: u32, request: u32) -> Self {
        Self {
            store,
            instance,
            request,
        }
    }

    /// The final answer. Refused (the first answer won already, or the request
    /// is gone): the dialog closes all the same; an answer the store does not
    /// take with a `choice` is a cancel, noted for the log.
    pub(super) fn answer(&self, outcome: u32, choice: u32) {
        if self
            .store
            .answer(self.instance, self.request, outcome, choice)
            .is_err()
            && outcome == 0
        {
            self.store.note_refused();
            let _ = self.store.answer(self.instance, self.request, 1, 0);
        }
    }

    /// The line `index` of `lines` chosen: its own index; one the list does not
    /// hold is a cancel, noted for the log.
    pub(super) fn choose(&self, index: usize, lines: usize) {
        match u32::try_from(index).ok().filter(|_| index < lines) {
            Some(index) => self.answer(0, index),
            None => {
                self.store.note_refused();
                self.answer(1, 0);
            }
        }
    }
}
