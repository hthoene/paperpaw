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
use alloc::{string::ToString, vec::Vec};
use paperpaw_sdk::{Params, builtin::params};

#[path = "dialogs/choice.rs"]
mod choice;
#[path = "dialogs/confirm.rs"]
mod confirm;

pub(crate) use choice::Choice;
pub(crate) use confirm::Confirm;

/// A comma inside a line of a choice would split it in two: this stands in for
/// it (a low quotation mark, which looks like one).
const COMMA: char = '\u{201a}';

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
    /// is gone): the dialog closes all the same.
    pub(super) fn answer(&self, outcome: u32, choice: u32) {
        let _ = self
            .store
            .answer(self.instance, self.request, outcome, choice);
    }
}

/// The question as params for the dialog of `kind` (1 choice, 2 confirm), the
/// form both dialogs read; `None` for any other kind.
pub(super) fn params_of(
    kind: u32,
    flags: u32,
    title: &str,
    text: &str,
    choices: &str,
) -> Option<Params> {
    let base = Params::new()
        .with(params::TITLE, title.to_string())
        .with(params::TEXT, text.to_string());
    match kind {
        1 => {
            let options: Vec<alloc::string::String> = choices
                .split('\n')
                .map(|line| line.replace(',', &COMMA.to_string()))
                .collect();
            Some(base.with(choice::OPTIONS, options.join(",")))
        }
        2 => {
            let mut params = base;
            let mut words = choices.split('\n');
            if let Some(yes) = words.next().filter(|w| !w.is_empty()) {
                params.set(confirm::YES_LABEL, yes.to_string());
            }
            if let Some(no) = words.next().filter(|w| !w.is_empty()) {
                params.set(confirm::NO_LABEL, no.to_string());
            }
            // `only_yes` (bit 8) and `focus_yes` (bit 9).
            if flags & (1 << 8) != 0 {
                params.set(confirm::ONLY_YES, "1");
            }
            if flags & (1 << 9) != 0 {
                params.set(confirm::FOCUS_YES, "1");
            }
            Some(params)
        }
        _ => None,
    }
}
