//! The host's confirmation of a plugin install or removal (`docs/abi/device.md`
//! 5.0, 5.2): the words for a [`PluginQuestion`] and the pump that shows it on
//! the same guarding screen as the firmware's ([`super::firmware_confirm`]).
//!
//! Only the user answers: the answer comes back through the screen's own
//! nonce, `No` is reason 2, dismissal (Back, sleep, ten minutes, a screen that
//! cannot be shown) is reason 3, and neither is ever a yes.
use super::firmware_confirm::{Texts, clean, fingerprint, open_params};
use super::plugin_job::{KIND_REMOVAL, PluginQuestion, Relation};
use crate::Kernel;
use alloc::{rc::Rc, string::String, string::ToString, vec::Vec};
use core::cell::RefCell;
use paperpaw_i18n::Arg;

/// The confirmation's words for `question`, asked by the plugin `asker`.
pub(crate) fn texts(
    i18n: &paperpaw_i18n::Translator,
    asker: &str,
    question: &PluginQuestion,
) -> Texts {
    let removal = question.kind == KIND_REMOVAL;
    let mut lines: Vec<String> = Vec::new();
    lines.push(i18n.format(
        "core.firmware.asked-by",
        &[("name", Arg::Str(&clean(asker)))],
    ));
    lines.push(i18n.format(
        "core.plugin.what",
        &[
            ("name", Arg::Str(&clean(question.name.as_str()))),
            ("version", Arg::Str(&clean(question.version.as_str()))),
            ("id", Arg::Str(&clean(question.id.as_str()))),
        ],
    ));
    if !removal {
        if !question.signer.as_str().is_empty() {
            lines.push(i18n.format(
                "core.firmware.signed",
                &[("key", Arg::Str(&clean(question.signer.as_str())))],
            ));
        } else if let Some(key) = &question.key {
            lines.push(i18n.format(
                "core.firmware.other-key",
                &[("fingerprint", Arg::Str(&fingerprint(key)))],
            ));
        } else {
            lines.push(String::from(i18n.tr("core.firmware.unsigned")));
        }
        if question.sideload {
            lines.push(String::from(i18n.tr("core.plugin.sideload")));
        }
        if question.relation != Relation::Newer {
            lines.push(String::from(i18n.tr("core.firmware.older")));
        }
        let shown = usize::from(question.uses.count).min(question.uses.keys.len());
        if shown > 0 {
            let keys: Vec<&str> = question.uses.keys[..shown]
                .iter()
                .map(|key| key.as_str())
                .collect();
            let more = if question.uses.more { ", …" } else { "" };
            lines.push(i18n.format(
                "core.plugin.uses",
                &[("keys", Arg::Str(&alloc::format!("{}{more}", clean(&keys.join(", ")))))],
            ));
        }
    }
    let (title, yes) = if removal {
        ("core.plugin.title.remove", "core.plugin.yes.remove")
    } else {
        ("core.plugin.title", "core.plugin.yes")
    };
    Texts {
        title: String::from(i18n.tr(title)),
        body: lines.join("\n"),
        yes: String::from(i18n.tr(yes)),
        no: String::from(i18n.tr("core.firmware.no")),
    }
}

impl Kernel {
    /// Opens the confirmation of `question`, when nothing guards the device
    /// (never over the lock). A screen that cannot be shown, or a device
    /// without a random nonce, ends the question as dismissed. `true` while
    /// another pass has something to do.
    pub(crate) fn plugin_confirm_pump(
        &mut self,
        driver: &Rc<RefCell<super::firmware_driver::Driver>>,
        question: &PluginQuestion,
    ) -> bool {
        if self.guard().is_some() {
            return false;
        }
        let dismiss = |driver: &Rc<RefCell<super::firmware_driver::Driver>>| {
            if let Ok(mut driver) = driver.try_borrow_mut() {
                driver.plugin_question_dismissed(question);
            }
        };
        let mut bytes = [0u8; 16];
        if !self.shared.random.fill(&mut bytes) {
            dismiss(driver);
            return true;
        }
        let nonce = u128::from_le_bytes(bytes);
        let asker = self.firmware_asker_name(question.owner);
        let said = texts(&self.shared.translator, &asker, question);
        let params = open_params(&said, nonce);
        let before = self.stack.len();
        self.open(super::firmware_confirm::SCREEN, None, &params);
        if self.stack.len() > before {
            if let Ok(mut driver) = driver.try_borrow_mut() {
                driver.plugin_question_shown(nonce, question);
            }
        } else {
            dismiss(driver);
        }
        true
    }
}

#[cfg(test)]
#[path = "plugin_confirm_tests.rs"]
mod tests;
