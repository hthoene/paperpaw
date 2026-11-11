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
use paperpaw_native::host_glue::{ERR_NOT_FOUND, ERR_STATE};

#[path = "dialogs/choice.rs"]
mod choice;
#[path = "dialogs/confirm.rs"]
mod confirm;
#[path = "dialogs/confirm_box.rs"]
mod confirm_box;

pub(crate) use choice::Choice;
pub(crate) use confirm::Confirm;
pub(crate) use confirm_box::{ConfirmBox, Touch};

/// The question of a request, copied out of the store to make its dialog:
/// the lines as the asker wrote them (no split on commas, equal lines kept).
pub(crate) struct Asked {
    pub(crate) kind: u32,
    pub(crate) flags: u32,
    pub(crate) title: String,
    pub(crate) text: String,
    pub(crate) lines: Vec<String>,
}

impl Asked {
    /// The question copied out of the store, every copy fallible: `None` when
    /// the host has no memory for it (the request then ends as not started).
    pub(crate) fn copy(
        kind: u32,
        flags: u32,
        title: &str,
        text: &str,
        choices: &str,
    ) -> Option<Self> {
        let mut lines = Vec::new();
        if !choices.is_empty() {
            lines.try_reserve_exact(choices.split('\n').count()).ok()?;
            for line in choices.split('\n') {
                lines.push(try_string(line)?);
            }
        }
        Some(Self {
            kind,
            flags,
            title: try_string(title)?,
            text: try_string(text)?,
            lines,
        })
    }
}

impl Asked {
    /// The dialog screen for this question: the choice sheet (kind 1) or the
    /// confirm box (kind 2), answering through `answer`.
    pub(crate) fn into_screen(
        self,
        env: &paperpaw_sdk::Env<'_>,
        answer: Answerer,
    ) -> alloc::boxed::Box<dyn paperpaw_sdk::Screen> {
        if self.kind == 1 {
            return alloc::boxed::Box::new(Choice::new(env, self.title, self.lines, answer));
        }
        let word = |at: usize| self.lines.get(at).map_or("", String::as_str);
        alloc::boxed::Box::new(Confirm::new(
            env,
            &self.title,
            &self.text,
            (word(0), word(1)),
            self.flags,
            answer,
        ))
    }
}

/// `text` copied without aborting when memory is short.
fn try_string(text: &str) -> Option<String> {
    let mut copy = String::new();
    copy.try_reserve_exact(text.len()).ok()?;
    copy.push_str(text);
    Some(copy)
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

    /// The final answer. Refused because the request is gone or the first
    /// answer won already: the dialog closes all the same, nothing is logged.
    /// An answer the store does not take as given (a `choice` it holds no line
    /// for) is a cancel, noted for the log.
    pub(super) fn answer(&self, outcome: u32, choice: u32) {
        match self
            .store
            .answer(self.instance, self.request, outcome, choice)
        {
            Err(status) if outcome == 0 && status != ERR_NOT_FOUND && status != ERR_STATE => {
                self.store.note_refused();
                let _ = self.store.answer(self.instance, self.request, 1, 0);
            }
            _ => {}
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
