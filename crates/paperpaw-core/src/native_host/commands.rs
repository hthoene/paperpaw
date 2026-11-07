//! Borrow a bounded command stage only during a synchronous foreign callback.
//! The owner keeps this pinned slot, not a stage, registry or queue reference.
//! Accepted commands stay host-private until the host drains the stage after
//! the callback; the kernel applies them, so the next callback reads them back.

use super::Error;
pub use crate::abi7::commands_wire::{
    COMMAND_MAX_BYTES, COMMAND_MAX_KEY_BYTES, COMMAND_MAX_RECORDS, COMMAND_SET_SETTING_TAG,
    Command as WireCommand, CommandBatch, commands_decode, commands_encode,
};
use crate::abi7::settings_wire::SettingValueRef;
use alloc::{borrow::Cow, string::String, vec::Vec};
use core::{cell::RefCell, fmt, marker::PhantomPinned, pin::Pin, ptr::NonNull};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_STATE, ERR_UNSUPPORTED, OK,
    PpBytes, buffers,
};
use paperpaw_sdk::{Command, Id, SettingSpec, SettingValue, Settings};

/// Host-resolved authority for guarded settings, shared with the drain-time filter.
#[derive(Clone, Copy, Debug)]
pub struct CommandAuthority {
    guarded_exempt: bool,
}

impl crate::Kernel {
    /// Resolve command authority for a host-known submitting identity.
    /// Resolve anew when creating each callback's stage; never retain this token
    /// across callbacks. BindSlot or uninstall can change the settings editor.
    #[must_use]
    pub fn command_settings_authority(&self, owner: &Id) -> CommandAuthority {
        let reserved = crate::commands::Reserved::of(&self.shared.settings, &self.shared.registry);
        CommandAuthority {
            guarded_exempt: reserved.guarded_exempt(owner.as_str()),
        }
    }
}

/// Host staging for one callback: the submitting plugin's identity, the live
/// settings registry used for validation, at most `limit` accepted commands,
/// and at most `byte_limit` bytes of copied keys and text across them.
/// Construction allocates nothing. Each accepted batch reserves queue space and
/// copies text fallibly; setting ids are copied with the global allocator
/// (bounded by the key cap and counted in `byte_limit`).
pub struct CommandStage<'a> {
    owner: &'a Id,
    settings: &'a Settings,
    guarded_exempt: bool,
    staged: Vec<Command>,
    limit: usize,
    byte_limit: usize,
    bytes: usize,
}

impl<'a> CommandStage<'a> {
    /// Stage for `owner`, the host-known plugin identity, never a plugin claim.
    /// `authority` comes from `Kernel::command_settings_authority` for this owner.
    /// `None` exempts nobody: every foreign guarded setting is refused,
    /// including for an editor or kernel identity without host authority.
    /// `byte_limit` bounds the heap one callback can retain through staged keys
    /// and text; a profile chooses it within its boundary memory budget.
    #[must_use]
    pub fn new(
        owner: &'a Id,
        settings: &'a Settings,
        authority: Option<CommandAuthority>,
        limit: usize,
        byte_limit: usize,
    ) -> Self {
        Self {
            owner,
            settings,
            guarded_exempt: authority.is_some_and(|authority| authority.guarded_exempt),
            staged: Vec::new(),
            limit,
            byte_limit,
            bytes: 0,
        }
    }

    /// Commands accepted so far, in submission order.
    #[must_use]
    pub fn staged(&self) -> &[Command] {
        &self.staged
    }

    /// Hand accepted commands to the host's queue after the callback returns.
    #[must_use]
    pub fn into_commands(self) -> Vec<Command> {
        self.staged
    }

    /// The setting's spec, if this plugin may set it through slot 15.
    /// Registered, non-secret settings may belong to any plugin. Foreign
    /// guarded settings require the host-known editor or kernel identity.
    fn admitted(&self, key: &str) -> Result<&'a SettingSpec, i32> {
        let spec = self.settings.spec(key).ok_or(ERR_INVALID)?;
        if spec.extra().secret {
            return Err(ERR_UNSUPPORTED);
        }
        if spec.guarded
            && !self.guarded_exempt
            && key.split('.').next() != Some(self.owner.as_str())
        {
            return Err(ERR_INVALID);
        }
        Ok(spec)
    }

    /// Validate an entire decoded batch, then stage it, or stage nothing.
    fn stage(&mut self, batch: &CommandBatch<'_>) -> i32 {
        if self.staged.len().saturating_add(batch.len()) > self.limit {
            return ERR_NO_SPACE;
        }
        // `answer_plugin` needs the install feature (`uses.install`, DEV), which
        // no catalog defines yet: refused for every plugin, wherever it sits in
        // the batch (`docs/abi/registry.md` section 5).
        if batch
            .iter()
            .any(|command| matches!(command, WireCommand::AnswerPlugin { .. }))
        {
            return ERR_DENIED;
        }
        // Registration, scalar ranges and the byte budget, without allocating.
        let mut bytes = self.bytes;
        for command in batch.iter() {
            // Commands 56-59 are decoded but not yet run by this host: the
            // whole batch is refused before anything is staged.
            let WireCommand::SetSetting { key, value } = command else {
                return ERR_UNSUPPORTED;
            };
            let text = match value {
                SettingValueRef::Text(text) => text.len(),
                _ => 0,
            };
            bytes = bytes.saturating_add(key.len()).saturating_add(text);
            if bytes > self.byte_limit {
                return ERR_NO_SPACE;
            }
            let spec = match self.admitted(key) {
                Ok(spec) => spec,
                Err(error) => return error,
            };
            let scalar = match value {
                SettingValueRef::Bool(value) => Some(SettingValue::Bool(value)),
                SettingValueRef::Int(value) => Some(SettingValue::Int(value)),
                _ => None,
            };
            if scalar.is_some_and(|scalar| !spec.accepts(&scalar)) {
                return ERR_INVALID;
            }
        }
        if self.staged.try_reserve(batch.len()).is_err() {
            return ERR_NO_MEMORY;
        }
        let start = self.staged.len();
        for command in batch.iter() {
            match self.owned(command) {
                // Capacity is reserved, so this push cannot reallocate.
                Ok(command) => self.staged.push(command),
                Err(error) => {
                    self.staged.truncate(start);
                    return error;
                }
            }
        }
        self.bytes = bytes;
        OK
    }

    /// Convert one admitted command, copying text fallibly and checking it.
    fn owned(&self, command: WireCommand<'_>) -> Result<Command, i32> {
        let WireCommand::SetSetting { key, value } = command else {
            return Err(ERR_UNSUPPORTED);
        };
        let spec = self.admitted(key)?;
        let value = match value {
            SettingValueRef::Bool(value) => SettingValue::Bool(value),
            SettingValueRef::Int(value) => SettingValue::Int(value),
            SettingValueRef::Text(text) => {
                let mut owned = String::new();
                owned
                    .try_reserve_exact(text.len())
                    .map_err(|_| ERR_NO_MEMORY)?;
                owned.push_str(text);
                SettingValue::Text(Cow::Owned(owned))
            }
            SettingValueRef::Absent => return Err(ERR_INVALID),
        };
        if !spec.accepts(&value) {
            return Err(ERR_INVALID);
        }
        // Keys are at most COMMAND_MAX_KEY_BYTES; a placement key is parsed.
        let id = if key == spec.id.as_str() {
            spec.id.clone()
        } else {
            Id::parse(key).map_err(|_| ERR_INVALID)?
        };
        Ok(Command::SetSetting(id, value))
    }
}

impl fmt::Debug for CommandStage<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandStage")
            .field("owner", self.owner)
            .field("staged", &self.staged.len())
            .field("limit", &self.limit)
            .field("bytes", &self.bytes)
            .field("byte_limit", &self.byte_limit)
            .finish_non_exhaustive()
    }
}

/// Allocation-free command slot inside a pinned host context.
/// This single-threaded slot lends a stage only for one callback.
#[non_exhaustive]
pub struct CommandScope {
    current: RefCell<Option<NonNull<CommandStage<'static>>>>,
    _pin: PhantomPinned,
}

struct Reset<'a>(&'a CommandScope);
impl Drop for Reset<'_> {
    fn drop(&mut self) {
        // Every submission's private borrow guard has ended before scope exit,
        // including unwinding. No user code can retain a guard or stage view.
        *self.0.current.borrow_mut() = None;
    }
}

impl CommandScope {
    /// Create an empty slot before pinning the containing context.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current: RefCell::new(None),
            _pin: PhantomPinned,
        }
    }

    /// Exclusively lend a stage for one synchronous callback.
    /// No RefCell guard or submission borrow spans `call`. Errors and
    /// unwinding clear the slot before the stage borrow ends.
    ///
    /// # Errors
    /// Returns Busy for nested scopes or while a submission is active.
    pub fn scope<R>(
        self: Pin<&Self>,
        stage: &mut CommandStage<'_>,
        call: impl FnOnce() -> R,
    ) -> Result<R, Error> {
        let mut current = self.current.try_borrow_mut().map_err(|_| Error::Busy)?;
        if current.is_some() {
            return Err(Error::Busy);
        }
        // Erase only the stage's borrowed lifetime. The exclusive caller
        // borrow lasts through call and Reset; no pointer is exposed.
        *current = Some(NonNull::from(stage).cast::<CommandStage<'static>>());
        drop(current);
        let _reset = Reset(self.get_ref());
        Ok(call())
    }

    /// Slot-15 bridge: decode and validate a whole batch, then stage all of it.
    ///
    /// Returns OK, or an error with the stage unchanged: ERR_STATE outside a
    /// callback, ERR_BUSY during another submission, codec statuses for the
    /// bytes, ERR_INVALID for a foreign guarded, unregistered or unaccepted setting
    /// value, ERR_UNSUPPORTED for a secret, ERR_NO_SPACE beyond the stage
    /// limit or byte budget and ERR_NO_MEMORY if fallible staging cannot reserve memory.
    ///
    /// # Safety
    /// When non-null with nonzero length, `bytes` must designate initialized
    /// storage readable for `len` bytes, live, stable and unmodified until
    /// this call returns, and disjoint from host state. No pointer is retained.
    #[must_use]
    pub unsafe fn submit(&self, bytes: *const u8, len: usize) -> i32 {
        let Ok(mut current) = self.current.try_borrow_mut() else {
            return ERR_BUSY;
        };
        let Some(pointer) = current.as_mut() else {
            return ERR_STATE;
        };
        if len > COMMAND_MAX_BYTES {
            return ERR_NO_SPACE;
        }
        // SAFETY: the caller guarantees readable, stable storage; the helper
        // checks null and size bounds before constructing any nonempty slice.
        let batch = match unsafe { buffers::bytes_in(PpBytes { ptr: bytes, len }, len) }
            .and_then(commands_decode)
        {
            Ok(batch) => batch,
            Err(error) => return error,
        };
        // SAFETY: scope retains the exclusive caller borrow of a live stage.
        // The RefMut excludes overlapping submissions; the reference exists
        // only for this call and is gone before Reset clears the pointer.
        let stage = unsafe { pointer.as_mut() };
        stage.stage(&batch)
    }
}

impl Default for CommandScope {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for CommandScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandScope").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_host_authority_refuses_foreign_guarded_settings_even_for_core() {
        let mut settings = Settings::new();
        let spec = SettingSpec::switch(
            "other.guarded",
            "guarded",
            false,
            paperpaw_sdk::builtin::groups::OTHER,
        )
        .guarded();
        assert!(settings.define(spec).is_ok());
        let owner = Id::new_static("core");
        let stage = CommandStage::new(&owner, &settings, None, 1, 40);
        assert_eq!(stage.admitted("other.guarded").err(), Some(ERR_INVALID));
        assert!(stage.staged().is_empty());
    }

    #[test]
    fn overlapping_submission_refuses_without_panicking() {
        let scope = CommandScope::new();
        let _borrow = scope.current.borrow_mut();
        // SAFETY: a zero-length submission reads no storage.
        assert_eq!(unsafe { scope.submit(core::ptr::null(), 0) }, ERR_BUSY);
    }
}
