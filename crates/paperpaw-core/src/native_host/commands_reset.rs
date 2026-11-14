//! `command.reset_setting` (1536), `docs/abi/registry.md` 4.1: a setting, a
//! group, the submitter's own settings or what a placement set go back to
//! their defaults.
//!
//! A reset of one key is checked as `command.set_setting` checks its key. A
//! reset of many names the values kept now (and the active scope's) that it
//! covers, each one the submitter may write: what is kept on reset, secrets,
//! grants and another plugin's guarded settings are left as they are. Each
//! becomes one [`Command::ResetSetting`], in the batch's order; the room for
//! them is reserved before anything is staged. The two resets that need the
//! reader's consent (another plugin's settings, every setting) are refused:
//! the host has no consent dialog yet.
use super::{CommandBatch, CommandStage, WireCommand, ui};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED};
use paperpaw_sdk::{Command, Id, Settings};

/// `flags` bit 0: `key` is a group.
const GROUP: u32 = 1;
/// `flags` bit 1: every setting of plugin `key`.
const PLUGIN: u32 = 1 << 1;
/// `flags` bit 2: every setting (`key` empty).
const ALL: u32 = 1 << 2;
/// `flags` bit 3: every value placement `key` sets.
const PLACEMENT: u32 = 1 << 3;

/// Whose reset it is: what [`CommandStage::admitted`] goes by.
#[derive(Clone, Copy)]
struct Who<'a> {
    settings: &'a Settings,
    owner: &'a Id,
    guarded_exempt: bool,
}

impl Who<'_> {
    /// Whether a reset of many takes the kept value `id`: a setting still
    /// registered, not kept on reset (a secret is), no grant, and not another
    /// plugin's guarded one. `placed`: what a placement set goes with it,
    /// kept on reset or not.
    fn takes(&self, id: &str, placed: bool) -> Result<bool, i32> {
        if id.starts_with("core.grant.") {
            return Ok(false);
        }
        let spec = self.settings.try_spec(id).map_err(|_| ERR_NO_MEMORY)?;
        let Some(spec) = spec else {
            return Ok(false);
        };
        if spec.extra().secret || (!placed && spec.is_kept_on_reset()) {
            return Ok(false);
        }
        Ok(!spec.guarded || self.guarded_exempt || self.own(id))
    }

    /// Whether a setting of group `of` is shown in `group` or a part of it
    /// (one of a group nobody registered is shown with the others).
    fn shown_in(&self, of: &Id, group: &Id) -> bool {
        if self.settings.groups().any(|known| known.id == *of) {
            self.settings.within(of, group)
        } else {
            self.settings
                .within(&paperpaw_sdk::builtin::groups::OTHER, group)
        }
    }

    /// Whether `id` is a setting of the submitting plugin.
    fn own(&self, id: &str) -> bool {
        id.split('.').next() == Some(self.owner.as_str())
    }

    /// Calls `each` with the id of every setting the reset `key`, `flags`
    /// resets; a reset that is none, or not this plugin's, is its status.
    fn walk(
        &self,
        key: &str,
        flags: u32,
        each: &mut dyn FnMut(&str) -> Result<(), i32>,
    ) -> Result<(), i32> {
        let settings = self.settings;
        match flags {
            0 => {
                super::admitted(settings, self.owner, self.guarded_exempt, key)?;
                each(key)
            }
            PLACEMENT if key.contains('@') => {
                let placed = |id: &&Id| {
                    let rest = id.as_str().strip_prefix(key);
                    rest.is_some_and(|rest| rest.starts_with('.'))
                };
                for id in settings.valued().filter(placed) {
                    if self.takes(id.as_str(), true)? {
                        each(id.as_str())?;
                    }
                }
                Ok(())
            }
            GROUP => {
                let group = settings.groups().find(|group| group.id.as_str() == key);
                let group = &group.ok_or(ERR_INVALID)?.id;
                for id in settings.valued() {
                    let spec = settings.try_spec(id.as_str());
                    let Some(spec) = spec.map_err(|_| ERR_NO_MEMORY)? else {
                        continue;
                    };
                    if self.shown_in(&spec.group, group) && self.takes(id.as_str(), false)? {
                        each(id.as_str())?;
                    }
                }
                Ok(())
            }
            PLUGIN if key == self.owner.as_str() => {
                for id in settings.valued() {
                    if self.own(id.as_str()) && self.takes(id.as_str(), false)? {
                        each(id.as_str())?;
                    }
                }
                Ok(())
            }
            // Another plugin's settings, every setting: only with the
            // reader's consent, which the host cannot ask for yet.
            PLUGIN if is_plugin(key) => Err(ERR_UNSUPPORTED),
            ALL if key.is_empty() => Err(ERR_UNSUPPORTED),
            _ => Err(ERR_INVALID),
        }
    }
}

/// Whether `key` is formed as a plugin's id is: one segment.
fn is_plugin(key: &str) -> bool {
    !key.is_empty() && !key.contains(['.', '@'])
}

impl<'a> CommandStage<'a> {
    /// Whether `command` is a `command.reset_setting`.
    pub(super) fn is_reset(command: &WireCommand<'_>) -> bool {
        matches!(command, WireCommand::ResetSetting { .. })
    }

    fn who(&self) -> Who<'a> {
        Who {
            settings: self.settings,
            owner: self.owner,
            guarded_exempt: self.guarded_exempt,
        }
    }

    /// How many settings the resets of `batch` reset, every reset checked:
    /// the `Command`s they are staged as ([`Self::stage_reset`]).
    #[inline(never)]
    pub(super) fn reset_count(&self, batch: &CommandBatch<'_>) -> Result<usize, i32> {
        let mut count = 0_usize;
        for command in batch.iter() {
            if let WireCommand::ResetSetting { key, flags } = command {
                self.who().walk(key, flags, &mut |_| {
                    count = count.saturating_add(1);
                    Ok(())
                })?;
            }
        }
        Ok(count)
    }

    /// Keeps the resets of one checked `command.reset_setting`, in the room
    /// reserved for them ([`Self::reset_count`]): never more than that.
    #[inline(never)]
    pub(super) fn stage_reset(&mut self, key: &str, flags: u32) -> Result<(), i32> {
        let (who, staged) = (self.who(), &mut self.staged);
        who.walk(key, flags, &mut |id| {
            if staged.len() == staged.capacity() {
                return Err(ERR_NO_MEMORY);
            }
            staged.push(Command::ResetSetting(ui::copy_id(id)?));
            Ok(())
        })
    }
}
