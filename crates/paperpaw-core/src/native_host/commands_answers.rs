//! `command.answer_plugin` (1545, `docs/abi/registry.md` section 5): checked
//! whole at submit, applied after the callback with the other commands (the
//! staged-presets way: a list kept apart from the kernel's `Command`s).
//!
//! Only a plugin that declared `uses.install` answers: any other gets
//! `ERR_DENIED` for the whole batch, wherever the command sits in it. At
//! submit the outcome must be one the host knows (`ERR_INVALID`), the plugin
//! a valid id (`ERR_INVALID`), and the request and id open and not answered
//! yet, also not by this stage or this batch (`ERR_STATE`: the first answer
//! wins). What the stage knows of the table is an [`AnswerView`]; the apply
//! checks again ([`Kernel::native_apply_answers`]).
//!
//! [`Kernel::native_apply_answers`]: crate::Kernel::native_apply_answers
use super::{CommandBatch, CommandStage, WireCommand, ui::copy_id};
use crate::native_host::plugin_requests::{Answer, AnswerView, OUTCOME_LAST, boxed};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_STATE};

impl CommandStage<'_> {
    /// Whether `command` answers a plugin request.
    pub(super) fn is_answer(command: &WireCommand<'_>) -> bool {
        matches!(command, WireCommand::AnswerPlugin { .. })
    }

    /// What the kernel knows of the requests and of the submitter's right to
    /// answer them.
    pub(crate) fn set_answer_view(&mut self, view: AnswerView) {
        self.answer_view = view;
    }

    /// `ERR_DENIED` for a batch that answers without the right to.
    pub(super) fn answers_denied(&self, batch: &CommandBatch<'_>) -> Option<i32> {
        (!self.answer_view.installer && batch.iter().any(|command| Self::is_answer(&command)))
            .then_some(ERR_DENIED)
    }

    /// Takes the admitted answers, in order, for the host to apply after the
    /// callback.
    #[must_use]
    pub(crate) fn take_answers(&mut self) -> Vec<Answer> {
        core::mem::take(&mut self.staged_answers)
    }

    /// The answers of `batch`, checked and copied; their bytes are the second
    /// part of the result.
    #[inline(never)]
    pub(super) fn validate_answers(
        &self,
        batch: &CommandBatch<'_>,
    ) -> Result<(Vec<Answer>, usize), i32> {
        let mut answers: Vec<Answer> = Vec::new();
        let count = batch.iter().filter(Self::is_answer).count();
        if count == 0 {
            return Ok((answers, 0));
        }
        answers
            .try_reserve_exact(count)
            .map_err(|_| ERR_NO_MEMORY)?;
        let mut bytes = 0_usize;
        for command in batch.iter() {
            let WireCommand::AnswerPlugin {
                request,
                plugin,
                outcome,
            } = command
            else {
                continue;
            };
            if outcome > OUTCOME_LAST {
                return Err(ERR_INVALID);
            }
            copy_id(plugin)?;
            let answered = |held: &Answer| held.request == request && &*held.plugin == plugin;
            if !self.answer_view.was_open(request, plugin)
                || self.staged_answers.iter().any(answered)
                || answers.iter().any(answered)
            {
                return Err(ERR_STATE);
            }
            answers.push(Answer {
                request,
                plugin: boxed(plugin)?,
                outcome,
            });
            bytes = bytes.saturating_add(plugin.len());
        }
        Ok((answers, bytes))
    }
}
