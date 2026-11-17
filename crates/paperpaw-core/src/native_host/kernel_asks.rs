//! The kernel's own questions (`ask::choice` and `ask::confirm` of the
//! kernel's code, e.g. clean-up, pairing, the reading app): asked as a
//! plugin's are (`docs/abi/ui.md` 3.1), with the host as the asker, so that
//! the one dialog plugin on `core.choice` and `core.confirm` answers them.
//! The words are formatted here (an option's label, the buttons' words); the
//! answer comes back as the reply event the question named, with the context
//! it was asked with, as the stock screens sent it: a [`Confirmed`] for yes or
//! no, a `Picked` (`core.path` and the option) for a choice. A cancel sends
//! nothing.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::event::EventData as _;
use paperpaw_sdk::replies::Confirmed;
use paperpaw_sdk::ui::choice::{LABELS, OPTION, OPTIONS};
use paperpaw_sdk::ui::confirm::{FOCUS_YES, NO_LABEL, ONLY_YES, YES_LABEL};
use paperpaw_sdk::{Command, Id, Params};

use super::ui_requests::{Delivery, Question};
use super::ui_store::{HOST_OWNER, Stamp, UiStore};

/// `ask` flag bit 8 (`only_yes`) and bit 9 (`focus_yes`) of a confirm.
const ONLY_YES_FLAG: u32 = 1 << 8;
const FOCUS_YES_FLAG: u32 = 1 << 9;

/// One question of the kernel's, open until its answer is delivered.
struct Asked {
    request: u32,
    reply: Id,
    /// The parameters it was asked with: they come back with the answer.
    context: Params,
    /// A choice's options, by the index the dialog answers.
    options: Vec<String>,
}

/// The kernel's open questions.
#[derive(Default)]
pub(crate) struct KernelAsks(Vec<Asked>);

#[cfg(test)]
impl KernelAsks {
    /// The context the first open question was asked with.
    pub(crate) fn context_of_first(&self) -> Option<&Params> {
        self.0.first().map(|asked| &asked.context)
    }
}

impl crate::Kernel {
    /// `true` where `slot` with `params` is one of the kernel's own questions
    /// (an `ask::choice` or `ask::confirm` of its code, not the pump's open of
    /// a dialog, which names its request): asked through the ui store. One
    /// that cannot be asked (no ui service, no memory, a form the store
    /// refuses) is dropped, and the log says so.
    pub(crate) fn ask_kernel(&mut self, slot: &Id, params: &Params) -> bool {
        let confirm = *slot == slots::CONFIRM;
        if !(confirm || *slot == slots::CHOICE) || params.get("core.request").is_some() {
            return false;
        }
        if self.ask_kernel_question(confirm, params).is_none() {
            self.warn_runtime("a question of the kernel's could not be asked");
        }
        true
    }

    fn ask_kernel_question(&mut self, confirm: bool, params: &Params) -> Option<()> {
        let store = self
            .shared
            .native_instances
            .ui_services()
            .and_then(|ui| ui.store.try_clone())?;
        let reply = params.get(params::REPLY).and_then(|r| Id::parse(r).ok());
        let title = params.get(params::TITLE).unwrap_or_default();
        let (options, choices, flags, text) = if confirm {
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
            let text = params.get(params::TEXT).unwrap_or_default();
            (Vec::new(), words, flags, text)
        } else {
            let options: Vec<String> = params
                .get(OPTIONS)
                .unwrap_or_default()
                .split(',')
                .filter(|o| !o.is_empty())
                .map(String::from)
                .collect();
            let labels = params.get(LABELS).unwrap_or_default();
            let mut lines = String::new();
            for option in &options {
                if !lines.is_empty() {
                    lines.push('\n');
                }
                lines.push_str(&self.option_label(labels, option));
            }
            (options, lines, 0, "")
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
        if let Some(reply) = reply {
            self.asks.0.try_reserve(1).ok()?;
            self.asks.0.push(Asked {
                request,
                reply,
                context: params.clone(),
                options,
            });
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

    /// The screen of the kernel's open question asked with `key` = `value`
    /// in its context (e.g. the pairing question by its number), to close it
    /// when what it asks about is over.
    pub(crate) fn kernel_question_dialog(&self, key: &str, value: &str) -> Option<u32> {
        let store = self.shared.native_instances.ui_store()?;
        let request = self
            .asks
            .0
            .iter()
            .find(|asked| asked.context.get(key) == Some(value))?
            .request;
        self.stack
            .iter()
            .map(|screen| screen.serial)
            .find(|serial| {
                store
                    .dialog_of(*serial)
                    .is_some_and(|(id, ..)| id == request)
            })
    }

    /// Hands the answers to the kernel's questions out as their reply events;
    /// each request is freed once its event ran.
    pub(crate) fn deliver_kernel_answers(&mut self, store: &UiStore) {
        while let Some(delivery) = store.take_delivery(HOST_OWNER) {
            if let Some(command) = self.kernel_answer(&delivery) {
                self.apply(command);
            }
            let _ = store.delivered(delivery.request);
            self.run_commands();
        }
    }

    /// The reply event of one answer, `None` for a cancel or a question that
    /// named no reply.
    fn kernel_answer(&mut self, delivery: &Delivery) -> Option<Command> {
        let at = self
            .asks
            .0
            .iter()
            .position(|asked| asked.request == delivery.request)?;
        let asked = self.asks.0.swap_remove(at);
        if delivery.outcome != 0 {
            return None;
        }
        let answer = if delivery.kind == 2 {
            Confirmed::new(delivery.choice == 1, asked.context).to_params()
        } else {
            let option = asked.options.get(delivery.choice as usize)?;
            Params::new()
                .with(
                    params::PATH,
                    asked.context.get(params::PATH).unwrap_or_default(),
                )
                .with(OPTION, option.as_str())
        };
        Some(Command::Emit(asked.reply, answer))
    }
}
