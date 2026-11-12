//! The host's confirmation of a plugin install or removal (`docs/abi/device.md`
//! 5.0, 5.2): the words for a [`PluginQuestion`] and the pump that shows it on
//! the same guarding screen as the firmware's ([`super::firmware_confirm`]).
//!
//! Only the user answers: the answer comes back through the screen's own
//! nonce, `No` is reason 2, dismissal (Back, sleep, ten minutes, a screen that
//! cannot be shown) is reason 3, and neither is ever a yes.
use super::firmware_confirm::{Texts, clean, fingerprint, open_params};
use super::firmware_install::KIND_FIRMWARE;
use super::plugin_job::{KIND_REMOVAL, PluginQuestion, Relation};
use crate::Kernel;
use alloc::string::String;
use paperpaw_i18n::Arg;

/// `text` as an owned string, or `None` where the heap cannot take it.
fn own(text: &str) -> Option<String> {
    let mut owned = String::new();
    owned.try_reserve_exact(text.len()).ok()?;
    owned.push_str(text);
    Some(owned)
}

/// Adds `line` to `body` on a line of its own.
fn add(body: &mut String, line: &str) -> Option<()> {
    body.try_reserve(line.len() + 1).ok()?;
    if !body.is_empty() {
        body.push('\n');
    }
    body.push_str(line);
    Some(())
}

/// The `uses.*` keys shown, comma separated, and "…" where more were declared.
fn uses_list(question: &PluginQuestion) -> Option<String> {
    let shown = usize::from(question.uses.count).min(question.uses.keys.len());
    let mut list = String::new();
    for (at, key) in question.uses.keys[..shown].iter().enumerate() {
        let key = clean(key.as_str());
        list.try_reserve(key.len() + 2).ok()?;
        if at > 0 {
            list.push_str(", ");
        }
        list.push_str(&key);
    }
    if question.uses.more {
        list.try_reserve(4).ok()?;
        list.push_str(", …");
    }
    Some(list)
}

/// The words of `firmware_dual_boot` mode 1: who asked, and that the next
/// update erases the other firmware. Yes or No only; the focus starts on No.
fn dual_boot_texts(i18n: &paperpaw_i18n::Translator, asker: &str) -> Option<Texts> {
    let mut body = String::new();
    add(
        &mut body,
        &i18n.format(
            "core.firmware.asked-by",
            &[("name", Arg::Str(&clean(asker)))],
        ),
    )?;
    add(&mut body, i18n.tr("core.firmware.dual.what"))?;
    Some(Texts {
        title: own(i18n.tr("core.firmware.dual.title"))?,
        body,
        yes: own(i18n.tr("core.firmware.dual.yes"))?,
        no: own(i18n.tr("core.firmware.no"))?,
        data: None,
    })
}

/// The confirmation's words for `question`, asked by the plugin `asker`; the
/// text is built with fallible allocation (at most a few hundred bytes), and
/// `None` where the heap cannot take it: the question is then not asked, which
/// is a dismissal.
pub(crate) fn texts(
    i18n: &paperpaw_i18n::Translator,
    asker: &str,
    question: &PluginQuestion,
) -> Option<Texts> {
    let removal = question.kind == KIND_REMOVAL;
    if question.kind == KIND_FIRMWARE {
        // The one question of kind 1 is `firmware_dual_boot`'s (mode 1).
        return dual_boot_texts(i18n, asker);
    }
    let mut body = String::new();
    add(
        &mut body,
        &i18n.format(
            "core.firmware.asked-by",
            &[("name", Arg::Str(&clean(asker)))],
        ),
    )?;
    add(
        &mut body,
        &i18n.format(
            "core.plugin.what",
            &[
                ("name", Arg::Str(&clean(question.name.as_str()))),
                ("version", Arg::Str(&clean(question.version.as_str()))),
                ("id", Arg::Str(&clean(question.id.as_str()))),
            ],
        ),
    )?;
    if removal {
        add(&mut body, i18n.tr("core.plugin.remove-data"))?;
    } else {
        if !question.signer.as_str().is_empty() {
            add(
                &mut body,
                &i18n.format(
                    "core.firmware.signed",
                    &[("key", Arg::Str(&clean(question.signer.as_str())))],
                ),
            )?;
        } else if let Some(key) = &question.key {
            add(
                &mut body,
                &i18n.format(
                    "core.firmware.other-key",
                    &[("fingerprint", Arg::Str(&fingerprint(key)))],
                ),
            )?;
        } else {
            add(&mut body, i18n.tr("core.firmware.unsigned"))?;
        }
        if question.sideload {
            add(&mut body, i18n.tr("core.plugin.sideload"))?;
        }
        if question.relation != Relation::Newer {
            add(&mut body, i18n.tr("core.firmware.older"))?;
        }
        let keys = uses_list(question)?;
        if !keys.is_empty() {
            add(
                &mut body,
                &i18n.format("core.plugin.uses", &[("keys", Arg::Str(&keys))]),
            )?;
        }
    }
    let (title, yes) = if removal {
        ("core.plugin.title.remove", "core.plugin.yes.remove")
    } else {
        ("core.plugin.title", "core.plugin.yes")
    };
    Some(Texts {
        title: own(i18n.tr(title))?,
        body,
        yes: own(i18n.tr(yes))?,
        no: own(i18n.tr("core.firmware.no"))?,
        data: if removal {
            Some(own(i18n.tr("core.plugin.yes.remove-data"))?)
        } else {
            None
        },
    })
}

impl Kernel {
    /// Opens the confirmation of `question`, when nothing guards the device
    /// (never over the lock). A screen that cannot be shown, or a device
    /// without a random nonce, ends the question as dismissed. `true` while
    /// another pass has something to do.
    pub(crate) fn plugin_confirm_pump(
        &mut self,
        driver: &super::firmware_cell::DriverRef,
        question: &PluginQuestion,
    ) -> bool {
        if self.guard().is_some() {
            return false;
        }
        let dismiss = |driver: &super::firmware_cell::DriverRef| {
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
        let Some(said) = texts(&self.shared.translator, &asker, question) else {
            // No memory for the words: not asked, so not a yes.
            dismiss(driver);
            return true;
        };
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
