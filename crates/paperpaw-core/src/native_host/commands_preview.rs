//! `command.preview_setting` (1537) and `command.preview_end` (1538),
//! `docs/abi/registry.md` 4.4: values shown in place of those kept while a
//! setting is chosen, never saved.
//!
//! The preview rows of one batch are one set. Each is checked as
//! `command.set_setting` checks its value (registered, not secret, this
//! plugin's to set, a value the setting takes) and copied fallibly: at most 8
//! rows (a batch) and [`PREVIEW_MAX_BYTES`] of keys and encoded values. The
//! set is staged as one [`Command::PreviewSettings`] after the batch's other
//! commands; an end is the empty set. The kernel applies it after the callback
//! (`Kernel::preview_settings`): the preview is the top screen's, and ends
//! with it, with its plugin and when the device sleeps.
use super::{CommandBatch, CommandStage, WireCommand};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_STATE};
use paperpaw_sdk::{Command, Id, SettingValue};

/// The most bytes of keys and encoded values of one preview set.
pub(crate) const PREVIEW_MAX_BYTES: usize = 2 * 1024;

/// `flags` bit 0 of `command.preview_setting`: shown as if it were not set.
const FLAG_NOT_SET: u32 = 1;

/// Whose the device's one preview is, as a callback's stage knows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PreviewHeld {
    /// No preview shows.
    #[default]
    Nobody,
    /// A screen of the submitting plugin shows one.
    Own,
    /// Another plugin's screen shows one.
    Other,
}

/// A checked preview set; empty, the preview ends.
pub(super) type PreviewSet = Vec<(Id, Option<SettingValue>)>;

impl CommandStage<'_> {
    /// Whether `command` shows or ends a preview.
    pub(super) fn is_preview(command: &WireCommand<'_>) -> bool {
        matches!(
            command,
            WireCommand::PreviewSetting { .. } | WireCommand::PreviewEnd { .. }
        )
    }

    /// Whose the preview is when the callback starts (the kernel's word;
    /// without it nobody's, so an end is accepted and ends nothing).
    pub(crate) fn set_preview_held(&mut self, held: PreviewHeld) {
        self.preview_held = held;
    }

    /// The preview of `batch`, checked and copied, with the bytes it counts
    /// against the stage's budget: `None` without preview commands, the empty
    /// set for an end.
    #[inline(never)]
    pub(super) fn validate_preview(
        &self,
        batch: &CommandBatch<'_>,
    ) -> Result<(Option<PreviewSet>, usize), i32> {
        let (mut rows, mut ends, mut bytes) = (0_usize, 0_usize, 0_usize);
        for command in batch.iter() {
            match command {
                WireCommand::PreviewSetting { key, value, flags } => {
                    // Only bit 0 is known, and "not set" carries no value.
                    if flags & !FLAG_NOT_SET != 0 || (flags != 0 && !value.is_empty()) {
                        return Err(ERR_INVALID);
                    }
                    rows += 1;
                    bytes = bytes.saturating_add(key.len()).saturating_add(value.len());
                }
                WireCommand::PreviewEnd { flags } => {
                    if flags != 0 {
                        return Err(ERR_INVALID);
                    }
                    ends += 1;
                }
                _ => {}
            }
        }
        if rows == 0 && ends == 0 {
            return Ok((None, 0));
        }
        // The rows are one set and an end is none: a batch says one of them.
        if rows != 0 && ends != 0 {
            return Err(ERR_INVALID);
        }
        if rows == 0 {
            // Only the submitter's own preview ends; with none, nothing does.
            if self.preview_held == PreviewHeld::Other {
                return Err(ERR_STATE);
            }
            return Ok((Some(Vec::new()), 0));
        }
        if bytes > PREVIEW_MAX_BYTES {
            return Err(ERR_NO_SPACE);
        }
        // The preview is a screen's: the submitter's own, on top.
        self.own_top()?;
        let mut set = Vec::new();
        set.try_reserve_exact(rows).map_err(|_| ERR_NO_MEMORY)?;
        for command in batch.iter() {
            if let WireCommand::PreviewSetting { key, value, flags } = command {
                set.push(self.preview_row(key, value, flags)?);
            }
        }
        Ok((Some(set), bytes))
    }

    /// One row of a set: the setting and the value shown for it (`None`: as
    /// if it were not set).
    fn preview_row(
        &self,
        key: &str,
        value: &[u8],
        flags: u32,
    ) -> Result<(Id, Option<SettingValue>), i32> {
        let spec = self.admitted(key)?;
        let shown = if flags & FLAG_NOT_SET != 0 {
            None
        } else {
            // A value that does not decode is not one of the setting.
            let value =
                crate::abi7::settings_wire::settings_decode(value).map_err(|_| ERR_INVALID)?;
            Some(Self::owned_value(spec, value)?)
        };
        Ok((Self::owned_key(spec, key)?, shown))
    }

    /// Keeps the checked preview of a batch as one command, after the batch's
    /// other commands (room is reserved: its rows count as stored commands).
    pub(super) fn keep_preview(&mut self, set: Option<PreviewSet>) {
        if let Some(set) = set {
            // A later batch of this callback sees whose the preview will be.
            self.preview_held = if set.is_empty() {
                PreviewHeld::Nobody
            } else {
                PreviewHeld::Own
            };
            self.staged.push(Command::PreviewSettings(set));
        }
    }
}

impl crate::Kernel {
    /// Whose the preview is for a callback of `owner`: the plugin of the
    /// screen that shows it. One whose screen is gone is nobody's (it ends at
    /// the kernel's next look, `check_preview_owner`).
    pub(crate) fn preview_held(&self, owner: &str) -> PreviewHeld {
        let shown_by = self
            .preview_owner
            .and_then(|serial| self.stack.iter().find(|screen| screen.serial == serial));
        match shown_by {
            None => PreviewHeld::Nobody,
            Some(screen) if screen.id.namespace() == owner => PreviewHeld::Own,
            Some(_) => PreviewHeld::Other,
        }
    }

    /// Whether a staged preview of `owner` still applies (`registry.md` 4.3,
    /// "checked again when applied"): a set where a screen of the plugin is
    /// on top, an end unless the preview has become another plugin's.
    pub(super) fn native_preview_applies(&self, owner: &Id, ends: bool) -> bool {
        if ends {
            return self.preview_held(owner.as_str()) != PreviewHeld::Other;
        }
        self.stack
            .last()
            .is_some_and(|top| top.id.namespace() == owner.as_str())
    }
}
