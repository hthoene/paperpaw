//! The kernel's own questions (`ask::choice` and `ask::confirm` of the
//! kernel's code, e.g. clean-up, pairing, the reading app): asked as a
//! plugin's are (`docs/abi/ui.md` 3.1), with the host as the asker, so that
//! the one dialog plugin on `core.choice` and `core.confirm` answers them.
//! The words are formatted here (an option's label, the buttons' words). What
//! the question was asked with (its reply, its context, a choice's options)
//! stays with the dialog's place on the stack, as it did with the kernel's
//! own dialogs: no table of the kernel's keeps it. When the dialog closes,
//! its answer is the reply event the question named, with that context, as
//! the stock screens sent it at their close: a [`Confirmed`] for yes or no, a
//! `Picked` (`path` and the option) for a choice. A cancel sends nothing.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::event::EventData as _;
use paperpaw_sdk::replies::Confirmed;
use crate::kit::ui::choice::{LABELS, OPTION, OPTIONS};
use crate::kit::ui::confirm::{FOCUS_YES, NO_LABEL, ONLY_YES, YES_LABEL};
use paperpaw_sdk::{Command, Id, Params};

use super::ui_requests::Question;
use super::ui_store::{HOST_OWNER, Stamp, UiStore};

/// `ask` flag bit 8 (`only_yes`) and bit 9 (`focus_yes`) of a confirm.
const ONLY_YES_FLAG: u32 = 1 << 8;
const FOCUS_YES_FLAG: u32 = 1 << 9;
/// The params of a dialog the pump opens: its request and the kind.
const REQUEST: &str = "core.request";
const KIND: &str = "core.kind";

/// The options of a choice, in the order the dialog answers by index.
fn options(params: &Params) -> impl Iterator<Item = &str> {
    params
        .get(OPTIONS)
        .unwrap_or_default()
        .split(',')
        .filter(|o| !o.is_empty())
}

impl crate::Kernel {
    /// `true` where `slot` with `params` is one of the kernel's own questions
    /// (an `ask::choice` or `ask::confirm` of its code, not the pump's open of
    /// a dialog, which names its request): asked through the ui store, its
    /// dialog opened at once. One that cannot be asked (no ui service, no
    /// memory, a form the store refuses, inside a plugin's callback) is
    /// dropped, and the log says so.
    pub(crate) fn ask_kernel(&mut self, slot: &Id, params: &Params) -> bool {
        let confirm = *slot == slots::CONFIRM;
        if !(confirm || *slot == slots::CHOICE) || params.get(REQUEST).is_some() {
            return false;
        }
        if self.ask_kernel_question(confirm, params).is_none() {
            self.warn_runtime("a question of the kernel's could not be asked");
        }
        true
    }

    fn ask_kernel_question(&mut self, confirm: bool, params: &Params) -> Option<()> {
        // The dialog opens in this call: never from inside a plugin's callback.
        if self.shared.native_instances.depth() != 0 {
            return None;
        }
        let store = self
            .shared
            .native_instances
            .ui_services()
            .and_then(|ui| ui.store.try_clone())?;
        let title = params.get(params::TITLE).unwrap_or_default();
        let (choices, flags, text) = if confirm {
            let words = match (params.get(YES_LABEL), params.get(NO_LABEL)) {
                (None, None) => String::new(),
                (yes, no) => alloc::format!("{}\n{}", yes.unwrap_or(""), no.unwrap_or("")),
            };
            let mut flags = 0;
            if params.get(ONLY_YES).is_some() {
                flags |= ONLY_YES_FLAG;
            }
            if params.get(FOCUS_YES).is_some() {
                flags |= FOCUS_YES_FLAG;
            }
            (words, flags, params.get(params::TEXT).unwrap_or_default())
        } else {
            let labels = params.get(LABELS).unwrap_or_default();
            let mut lines = String::new();
            for option in options(params) {
                if !lines.is_empty() {
                    lines.push('\n');
                }
                lines.push_str(&self.option_label(labels, option));
            }
            (lines, 0, "")
        };
        let question = Question {
            kind: if confirm { 2 } else { 1 },
            flags,
            remote: false,
            title,
            text,
            choices: &choices,
        };
        let who = Stamp::new(HOST_OWNER, 1).ok()?;
        let (request, _) = store.ask(who, None, &question, false, None, false).ok()?;
        // No callback asked it: it starts at once, and its dialog keeps what it
        // was asked with in its params.
        let start = store.host_started(request)?;
        self.dialog_open_with(&store, &start, params.clone());
        // A dialog that did not open (nothing on the slot, the stack full) is
        // no row to free the request at: freed now, cancelled.
        let id = alloc::format!("{request}");
        if !self
            .stack
            .iter()
            .any(|s| s.params.get(REQUEST) == Some(id.as_str()))
        {
            if store.take_delivery_of(HOST_OWNER, request).is_some() {
                let _ = store.delivered(request);
            }
            return None;
        }
        Some(())
    }

    /// What option `option` says: the translation of `labels` followed by it,
    /// or, without labels, an action's title where it names one (options that
    /// are actions, such as the other ways to open a book).
    fn option_label(&self, labels: &str, option: &str) -> String {
        let translator = &self.shared.translator;
        if labels.is_empty()
            && let Some(action) = self.shared.actions.get(option)
        {
            return String::from(translator.tr(action.title()));
        }
        String::from(translator.tr(&alloc::format!("{labels}{option}")))
    }

    /// The screen instance `instance` closes (`store` told already): where it
    /// was the dialog of one of the kernel's questions (its params name the
    /// request), the request is freed, and its answer goes out as the reply
    /// it named.
    // Its own frame: not on the stack of every screen's close.
    #[inline(never)]
    pub(crate) fn kernel_question_closed(&mut self, store: &UiStore, instance: u32) {
        let Some(screen) = self.stack.iter().find(|s| s.serial == instance) else {
            return;
        };
        let Some(request) = screen.params.get(REQUEST).and_then(|r| r.parse().ok()) else {
            return;
        };
        // Only the host's own question has an answer for the host here.
        let Some(delivery) = store.take_delivery_of(HOST_OWNER, request) else {
            return;
        };
        let _ = store.delivered(request);
        let reply = screen
            .params
            .get(params::REPLY)
            .and_then(|r| Id::parse(r).ok());
        let (Some(reply), 0) = (reply, delivery.outcome) else {
            return;
        };
        // The context as asked: without the dialog's own two params.
        let mut context = Params::new();
        for (key, value) in screen.params.iter() {
            if key != REQUEST && key != KIND {
                context.set(String::from(key), value);
            }
        }
        let answer = if delivery.kind == 2 {
            Confirmed::new(delivery.choice == 1, context).to_params()
        } else {
            let chosen: Vec<&str> = options(&screen.params).collect();
            let Some(option) = chosen.get(delivery.choice as usize) else {
                return;
            };
            Params::new()
                .with(params::PATH, context.get(params::PATH).unwrap_or_default())
                .with(OPTION, *option)
        };
        self.output.commands.push(Command::Emit(reply, answer));
    }
}
