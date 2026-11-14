//! Borrow a bounded command stage only during a synchronous foreign callback.
//! The owner keeps this pinned slot, not a stage, registry or queue reference.
//! Accepted commands stay host-private until the host drains the stage after
//! the callback; the kernel applies them, so the next callback reads them back.

use super::Error;
use super::image_service::{
    self,
    commands::{ImageAdmit, ImageCommand},
};
pub use crate::abi7::commands_wire::{
    COMMAND_MAX_BYTES, COMMAND_MAX_KEY_BYTES, COMMAND_MAX_RECORDS, COMMAND_SET_SETTING_TAG,
    Command as WireCommand, CommandBatch, SecretText, commands_decode, commands_encode,
};
use crate::abi7::settings_wire::SettingValueRef;
use alloc::{borrow::Cow, string::String, vec::Vec};
use core::{cell::RefCell, fmt, marker::PhantomPinned, pin::Pin, ptr::NonNull};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_STATE, ERR_UNSUPPORTED, OK,
    PpBytes, buffers,
};
use paperpaw_sdk::{Command, Id, SettingSpec, SettingValue, Settings};

#[allow(
    dead_code,
    reason = "native screen dispatch installs UI command authority next"
)]
#[path = "commands_ui.rs"]
mod ui;
/// A redraw a stage kept for its screen (run by the kernel after the callback).
pub(crate) use ui::Redraw;

#[path = "commands_items.rs"]
mod items;
#[path = "commands_redraw.rs"]
mod redraw;
pub(crate) use items::ItemAdmit;
#[cfg_attr(
    not(test),
    allow(
        unused_imports,
        reason = "the shell's command runner names it; that runner is wired with the shell"
    )
)]
pub(crate) use items::ItemRoom;
pub(crate) use redraw::{Area, FOLLOWUP_ID, Followup};

#[path = "commands_answers.rs"]
mod answers;
#[path = "commands_lookup.rs"]
mod lookup;
#[path = "commands_presets.rs"]
mod preset;
#[cfg(test)]
pub(crate) use preset::{covers as preset_covers, plan as preset_plan, walk as preset_walk};
#[path = "commands_nav.rs"]
mod nav;
pub(crate) use nav::PolicyChange;

#[path = "commands_open.rs"]
mod open;
pub(crate) use open::Opens;

#[path = "commands_dev.rs"]
pub(crate) mod dev;

/// Host-resolved authority for guarded settings, shared with the drain-time filter.
#[derive(Clone, Copy, Debug)]
pub struct CommandAuthority {
    guarded_exempt: bool,
}

impl CommandAuthority {
    /// The authority of a plugin that may (`true`) or may not change the
    /// guarded settings.
    pub(crate) const fn new(guarded_exempt: bool) -> Self {
        Self { guarded_exempt }
    }
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

/// The most commands one callback may stage (the stage's `limit`): enough for a
/// profile apply in one callback. Each is submitted in its
/// own batch (the wire batch cap, `COMMAND_MAX_RECORDS`, is separate and
/// unchanged); the staged commands live in a fallibly reserved heap `Vec`.
/// A callback that returns an error drops the whole stage, so a profile apply
/// stays atomic.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the production command path takes it with the shell wiring"
    )
)]
pub const CALLBACK_COMMANDS: usize = 64;
/// The most bytes of copied keys and text one callback may stage.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the production command path takes it with the shell wiring"
    )
)]
pub const CALLBACK_COMMAND_BYTES: usize = 4096;

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
    ui: Option<ui::UiContext<'a>>,
    dev: Option<dev::DevStage<'a>>,
    handovers: Option<open::Handovers<'a>>,
    opens: Opens,
    redraw: Option<u32>,
    /// `command.redraw_area`s of the callback (`commands_redraw.rs`).
    areas: redraw::Areas,
    accepted: usize,
    /// `image_prepare` / `image_release` admission, once the host gave it.
    images: Option<ImageAdmit>,
    /// Picture requests of the plugin held elsewhere (kept for a retry or
    /// failed, not yet told): they count against its room.
    images_held: usize,
    /// Texts of the plugin the kernel's speech holds (being spoken or waiting).
    speech_held: usize,
    /// Plugin requests open device-wide (`plugin_requests.rs`).
    requests_open: usize,
    /// How many of them are this plugin's (at most [`super::plugin_requests::MAX_PER_PLUGIN`]
    /// may be open).
    own_requests: u8,
    /// What the kernel knows of the plugin requests and of the submitter's
    /// right to answer them (`commands_answers.rs`).
    answer_view: super::plugin_requests::AnswerView,
    /// The answers to apply after the callback (`commands_answers.rs`).
    staged_answers: Vec<super::plugin_requests::Answer>,
    /// The ready lookups to apply after the callback, as their asks' hashes
    /// (`commands_lookup.rs`).
    staged_lookups: Vec<u64>,
    /// The callback belongs to a work run (`work.md` section 2, "background
    /// work is quiet"): commands that show something are refused.
    quiet: bool,
    /// The admitted image commands, applied by the host after the callback.
    staged_images: Vec<ImageCommand>,
    /// `command.set_item_state` once the host gave it (`commands_items.rs`),
    /// and the admitted states, applied by the host after the callback.
    items: Option<ItemAdmit<'a>>,
    staged_items: Vec<super::item_state::ItemPush>,
    /// The presets to apply after the callback (`commands_presets.rs`).
    staged_presets: Vec<alloc::boxed::Box<str>>,
    /// Entries of room this callback holds in the item-state table.
    room: core::cell::RefCell<Option<items::ItemRoom>>,
    /// The checked `command.set_hint` copies, applied after the callback.
    staged_hints: Vec<super::hint_labels::HintSet>,
    /// The checked `command.widget_changed` instances, applied after the
    /// callback.
    staged_widgets: Vec<u32>,
    staged_policies: Vec<nav::PolicyChange>,
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
            ui: None,
            dev: None,
            handovers: None,
            opens: Opens::default(),
            redraw: None,
            areas: redraw::Areas::default(),
            accepted: 0,
            images: None,
            quiet: false,
            images_held: 0,
            speech_held: 0,
            requests_open: 0,
            own_requests: 0,
            answer_view: super::plugin_requests::AnswerView::default(),
            staged_answers: Vec::new(),
            staged_lookups: Vec::new(),
            staged_images: Vec::new(),
            items: None,
            staged_items: Vec::new(),
            staged_presets: Vec::new(),
            room: core::cell::RefCell::new(None),
            staged_hints: Vec::new(),
            staged_widgets: Vec::new(),
            staged_policies: Vec::new(),
        }
    }

    /// Admits `command.image_prepare` and `command.image_release` for this
    /// callback (otherwise they are `ERR_UNSUPPORTED`): see
    /// [`ImageAdmit`]. The admitted ones come back from
    /// [`Self::take_image_commands`].
    #[must_use]
    #[allow(
        dead_code,
        reason = "the staged command path (with_native_commands) is wired with the shell's production command stage"
    )]
    pub(crate) fn with_images(mut self, images: ImageAdmit) -> Self {
        self.images = Some(images);
        self
    }

    /// Marks the callback as one of a work run: a command not on the list of
    /// [`allowed_in_a_run`] is `ERR_STATE` at submit.
    pub(crate) fn set_quiet(&mut self, quiet: bool) {
        self.quiet = quiet;
    }

    /// The picture requests of the plugin that wait outside the table.
    pub(crate) fn set_images_held(&mut self, held: usize) {
        self.images_held = held;
    }

    /// The plugin requests open device-wide.
    pub(crate) fn set_requests_open(&mut self, open: usize, own: usize) {
        self.requests_open = open;
        self.own_requests = u8::try_from(own).unwrap_or(u8::MAX);
    }

    /// The texts of the plugin that the kernel's speech holds.
    pub(crate) fn set_speech_held(&mut self, held: usize) {
        self.speech_held = held;
    }

    /// As [`Self::with_images`], on a stage already lent (the instance's
    /// own features are known only inside its callback scope).
    pub(crate) fn admit_images(&mut self, mut images: ImageAdmit) {
        images.room = images.room.saturating_sub(self.images_held);
        self.images = Some(images);
    }

    /// The admitted image commands, in order, for the host to apply after
    /// the callback (call it before [`Self::into_commands`]).
    #[must_use]
    pub(crate) fn take_image_commands(&mut self) -> Vec<ImageCommand> {
        core::mem::take(&mut self.staged_images)
    }

    /// Commands accepted so far, in submission order.
    #[must_use]
    pub fn staged(&self) -> &[Command] {
        &self.staged
    }

    /// Hand accepted commands to the host's queue after the callback returns.
    #[must_use]
    pub fn into_commands(mut self) -> Vec<Command> {
        core::mem::take(&mut self.staged)
    }

    /// The setting's spec, if this plugin may set it through slot 15.
    /// Registered, non-secret settings may belong to any plugin. Foreign
    /// guarded settings require the host-known editor or kernel identity.
    fn admitted(&self, key: &str) -> Result<&'a SettingSpec, i32> {
        // No room to read what its plugin describes of it is told as that.
        let spec = self.settings.try_spec(key).map_err(|_| ERR_NO_MEMORY)?;
        let spec = spec.ok_or(ERR_INVALID)?;
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
    pub(crate) fn stage(&mut self, batch: &CommandBatch<'_>) -> i32 {
        if self.accepted.saturating_add(batch.len()) > self.limit {
            return ERR_NO_SPACE;
        }
        if let Some(status) = self.refused_before_checks(batch) {
            return status;
        }
        if let Err(status) = self
            .validate_open_budget(batch)
            .and_then(|()| self.validate_handovers(batch))
        {
            return status;
        }
        // Wi-Fi and power commands (`commands_dev.rs`): features, limits,
        // rules; their copied bytes count against the budget.
        let dev_bytes = match self.validate_dev(batch) {
            Ok(bytes) => bytes,
            Err(status) => return status,
        };
        // The lists kept apart from the `Command`s ([`Kept`]).
        let kept = match self.validate_kept(batch) {
            Ok(kept) => kept,
            Err(status) => return status,
        };
        // Registration, scalar ranges and the byte budget, without allocating.
        let mut bytes = self
            .bytes
            .saturating_add(dev_bytes)
            .saturating_add(kept.bytes);
        if bytes > self.byte_limit {
            return ERR_NO_SPACE;
        }
        let mut images = self.images;
        if let Err(error) = self.validate_each(batch, &mut images, &mut bytes) {
            return error;
        }
        let image_commands = self.image_count(batch, images.is_some());
        let stored = Self::stored_count(batch) - image_commands;
        if self.staged.try_reserve(stored).is_err()
            || self.staged_images.try_reserve(image_commands).is_err()
            || self.reserve_kept(&kept).is_err()
        {
            return ERR_NO_MEMORY;
        }
        let start_hints = self.staged_hints.len();
        if let Err(error) = self.reserve_hints(batch) {
            return error;
        }
        let start_widgets = self.staged_widgets.len();
        if let Err(error) = self.reserve_widgets(batch) {
            return error;
        }
        let start_policies = self.staged_policies.len();
        if let Err(error) = self.reserve_policies(batch) {
            return error;
        }
        if let Err(error) = self
            .dev_reserve(batch)
            .and_then(|()| self.reserve_opens(batch))
            .and_then(|()| self.reserve_areas(batch))
        {
            return error;
        }
        // Everything fallible is reserved: the opens' hand-overs are claimed.
        let claims = match self.claim_handovers(batch) {
            Ok(claims) => claims,
            Err(error) => return error,
        };
        let (start, dev_start) = (self.staged.len(), self.dev_len());
        let opens = self.opens.at.len();
        let start_images = self.staged_images.len();
        if let Err(error) = self.push_batch(batch, dev_start, images.is_some()) {
            self.staged.truncate(start);
            self.staged_images.truncate(start_images);
            self.staged_hints.truncate(start_hints);
            self.staged_widgets.truncate(start_widgets);
            self.staged_policies.truncate(start_policies);
            self.opens.at.truncate(opens);
            self.dev_rollback(dev_start);
            self.unclaim(claims);
            return error;
        }
        // Capacity is reserved, so this cannot reallocate.
        self.keep(kept);
        self.keep_claims(claims);
        self.push_redraws(batch);
        self.accepted += batch.len();
        self.bytes = bytes;
        self.images = images;
        OK
    }

    /// The lists of a batch kept apart from the `Command`s, checked and
    /// copied: item states (`commands_items.rs`), presets
    /// (`commands_presets.rs`), answers to plugin requests
    /// (`commands_answers.rs`) and ready lookups (`commands_lookup.rs`).
    #[inline(never)]
    fn validate_kept(&self, batch: &CommandBatch<'_>) -> Result<Kept, i32> {
        let (item_states, item_bytes) = self.validate_item_states(batch)?;
        let (presets, preset_bytes) = self.validate_presets(batch)?;
        let (answers, answer_bytes) = self.validate_answers(batch)?;
        let lookups = self.validate_lookup_readies(batch)?;
        Ok(Kept {
            item_states,
            presets,
            answers,
            lookups,
            bytes: item_bytes
                .saturating_add(preset_bytes)
                .saturating_add(answer_bytes),
        })
    }

    /// Room for `kept` in the stage's lists.
    fn reserve_kept(&mut self, kept: &Kept) -> Result<(), ()> {
        self.staged_items
            .try_reserve(kept.item_states.len())
            .and_then(|()| self.staged_presets.try_reserve(kept.presets.len()))
            .and_then(|()| self.staged_answers.try_reserve(kept.answers.len()))
            .and_then(|()| self.staged_lookups.try_reserve(kept.lookups.len()))
            .map_err(|_| ())
    }

    /// Keeps `kept` in the stage's lists (room reserved by [`Self::reserve_kept`]).
    fn keep(&mut self, kept: Kept) {
        self.staged_items.extend(kept.item_states);
        self.staged_presets.extend(kept.presets);
        self.staged_answers.extend(kept.answers);
        self.staged_lookups.extend(kept.lookups);
    }

    /// Room for the instances of the batch's `command.widget_changed`.
    #[inline(never)]
    fn reserve_widgets(&mut self, batch: &CommandBatch<'_>) -> Result<(), i32> {
        let count = batch
            .iter()
            .filter(|command| matches!(command, WireCommand::WidgetChanged { .. }))
            .count();
        self.staged_widgets
            .try_reserve(count)
            .map_err(|_| ERR_NO_MEMORY)
    }

    /// Every command of a batch but the item states and the dev commands:
    /// image admission, scalar ranges, settings, and the byte budget.
    #[inline(never)]
    fn validate_each(
        &self,
        batch: &CommandBatch<'_>,
        images: &mut Option<ImageAdmit>,
        bytes: &mut usize,
    ) -> Result<(), i32> {
        for command in batch.iter() {
            if let Some(admit) = images.as_mut()
                && let Some(result) = admit.admit_counted(&command, bytes, self.byte_limit)
            {
                result?;
                continue;
            }
            let WireCommand::SetSetting { key, value } = command else {
                let checked = if dev::is_dev(&command)
                    || Self::is_item_state(&command)
                    || Self::is_preset(&command)
                    || Self::is_answer(&command)
                    || Self::is_lookup_ready(&command)
                {
                    Ok(0)
                } else {
                    self.validate_ui(command)
                };
                *bytes = bytes.saturating_add(checked?);
                if *bytes > self.byte_limit {
                    return Err(ERR_NO_SPACE);
                }
                continue;
            };
            self.validate_setting(key, value, bytes)?;
        }
        self.validate_speech_room(batch)?;
        self.validate_plugin_requests(batch)
    }

    /// A batch refused whole, whatever else it holds, the stage as it was.
    #[inline(never)]
    fn refused_before_checks(&self, batch: &CommandBatch<'_>) -> Option<i32> {
        // A write of a grant is refused for every plugin (`refused_outright`),
        // `answer_plugin` for one that did not declare `uses.install`.
        if let Some(status) = refused_outright(batch).or_else(|| self.answers_denied(batch)) {
            return Some(status);
        }
        // In a work run's callback only the listed commands are staged (the
        // callback sees the refusal and what it staged before stays valid).
        (self.quiet && batch.iter().any(|command| !allowed_in_a_run(&command))).then_some(ERR_STATE)
    }

    /// How many of `batch` go to the staged `Command`s (before the images).
    fn stored_count(batch: &CommandBatch<'_>) -> usize {
        batch
            .iter()
            .filter(|command| {
                !matches!(
                    command,
                    WireCommand::Redraw { .. }
                        | WireCommand::RedrawArea { .. }
                        | WireCommand::SetHint { .. }
                        | WireCommand::WidgetChanged { .. }
                        | WireCommand::ScreenPolicy { .. }
                        | WireCommand::ScreenReady {}
                ) && !dev::is_dev(command)
                    && !Self::is_item_state(command)
                    && !Self::is_preset(command)
                    && !Self::is_answer(command)
                    && !Self::is_lookup_ready(command)
            })
            .count()
    }

    /// One `set_setting` of a batch: its bytes counted against the budget, the
    /// setting registered and this plugin's to set, a scalar in its range.
    fn validate_setting(
        &self,
        key: &str,
        value: SettingValueRef<'_>,
        bytes: &mut usize,
    ) -> Result<(), i32> {
        let text = match value {
            SettingValueRef::Text(text) => text.len(),
            _ => 0,
        };
        *bytes = bytes.saturating_add(key.len()).saturating_add(text);
        if *bytes > self.byte_limit {
            return Err(ERR_NO_SPACE);
        }
        let spec = self.admitted(key)?;
        let scalar = match value {
            SettingValueRef::Bool(value) => Some(SettingValue::Bool(value)),
            SettingValueRef::Int(value) => Some(SettingValue::Int(value)),
            _ => None,
        };
        if scalar.is_some_and(|scalar| !spec.accepts(&scalar)) {
            return Err(ERR_INVALID);
        }
        Ok(())
    }

    /// How many of `batch` are image commands the host admitted.
    #[inline(never)]
    fn image_count(&self, batch: &CommandBatch<'_>, images: bool) -> usize {
        if !images {
            return 0;
        }
        batch
            .iter()
            .filter(|command| image_service::commands::owned_kind(command))
            .count()
    }

    /// Keep every command of a validated batch in order (capacity is reserved)
    /// and reserve the request entries of its DEV commands, last.
    #[inline(never)]
    fn push_batch(
        &mut self,
        batch: &CommandBatch<'_>,
        dev_start: usize,
        images: bool,
    ) -> Result<(), i32> {
        for command in batch.iter() {
            if matches!(
                command,
                WireCommand::Redraw { .. } | WireCommand::RedrawArea { .. }
            ) || Self::is_item_state(&command)
                || Self::is_preset(&command)
                || Self::is_answer(&command)
                || Self::is_lookup_ready(&command)
            {
                continue;
            }
            // Copied and listed ahead (`reserve_hints`).
            if matches!(command, WireCommand::SetHint { .. }) {
                continue;
            }
            // Listed for the table, not a kernel command; room is reserved.
            if let WireCommand::WidgetChanged { instance } = command {
                self.staged_widgets.push(instance);
                continue;
            }
            // Kept apart, applied before the others (`stage_policy`).
            if matches!(
                command,
                WireCommand::ScreenPolicy { .. } | WireCommand::ScreenReady {}
            ) {
                self.stage_policy(&command)?;
                continue;
            }
            if dev::is_dev(&command) {
                self.dev_push(&command)?;
                continue;
            }
            // A close of the root and a `back_to` with nothing above change
            // nothing: `OK`, nothing staged.
            if self.nav_no_effect(&command) {
                continue;
            }
            if images && self.stage_image(&command)? {
                continue;
            }
            self.stage_owned(&command)?;
        }
        self.dev_commit(dev_start)
    }

    /// Keeps an image command in its own list; `false` for another command.
    /// Its own frame: the copy of an image command is big, and is not held
    /// beside the others on the frame of `push_batch` (the S3 limit).
    #[inline(never)]
    fn stage_image(&mut self, command: &WireCommand<'_>) -> Result<bool, i32> {
        let Some(image) = image_service::commands::owned(command) else {
            return Ok(false);
        };
        // Capacity is reserved, so this push cannot reallocate.
        self.staged_images.push(image?);
        Ok(true)
    }

    /// Converts and keeps one ordinary command (and records an open).
    #[inline(never)]
    fn stage_owned(&mut self, command: &WireCommand<'_>) -> Result<(), i32> {
        let owned = self.owned(*command)?;
        if let WireCommand::Open { mode, handover, .. } = *command {
            self.record_open(self.staged.len(), mode, handover)?;
        }
        // Capacity is reserved, so this push cannot reallocate.
        self.staged.push(owned);
        Ok(())
    }

    /// Convert one admitted command, copying text fallibly and checking it.
    #[inline(never)]
    fn owned(&self, command: WireCommand<'_>) -> Result<Command, i32> {
        let WireCommand::SetSetting { key, value } = command else {
            return self.owned_ui(command);
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
            spec.id.try_clone().ok_or(ERR_NO_MEMORY)?
        } else {
            ui::copy_id(key)?
        };
        Ok(Command::SetSetting(id, value))
    }
}

/// What a batch stages beside its `Command`s, checked before anything is
/// staged ([`CommandStage::validate_kept`]); `bytes` count against the budget.
struct Kept {
    item_states: Vec<super::item_state::ItemPush>,
    presets: Vec<alloc::boxed::Box<str>>,
    answers: Vec<super::plugin_requests::Answer>,
    lookups: Vec<u64>,
    bytes: usize,
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

/// Whether a callback of a work run may stage `command` (`work.md` section
/// 2, "background work is quiet"): a redraw of its own screens, settings,
/// pictures, widget values and item states, the network, a ready lookup (a
/// provider's fetch ends in its run, `document.md` 7) and a notify with the
/// `announce` bit (`ui.md` section 7: device messages and rules the user set
/// up, which the kernel shows at its next pump, in its queue of waiting
/// notices). Everything else is refused, so a command added later (a plain
/// notice, a preview, an install or firmware confirmation, Bluetooth pairing,
/// an `open`) shows nothing in the background until it is named here.
fn allowed_in_a_run(command: &WireCommand<'_>) -> bool {
    matches!(
        command,
        WireCommand::Redraw { .. }
            | WireCommand::RedrawArea { .. }
            | WireCommand::SetSetting { .. }
            | WireCommand::ResetSetting { .. }
            | WireCommand::SetItemState { .. }
            | WireCommand::WidgetChanged { .. }
            | WireCommand::ImagePrepare { .. }
            | WireCommand::ImageRelease { .. }
            | WireCommand::WifiOn {}
            | WireCommand::WifiOff {}
            | WireCommand::WifiScan {}
            | WireCommand::WifiJoin { .. }
            | WireCommand::WifiLeave {}
            | WireCommand::WifiHost { .. }
            | WireCommand::NetworkNone {}
            | WireCommand::GoOnline { .. }
            | WireCommand::LookupReady { .. }
    ) || matches!(command, WireCommand::Notify { flags, .. } if flags & ui::NOTIFY_ANNOUNCE != 0)
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

/// A batch no plugin may send: `ERR_DENIED` for the whole of it.
///
/// (`answer_plugin` needs the install feature, `uses.install`: refused for
/// the others in `commands_answers.rs`.)
///
/// User grants are the host's alone (`screens-input.md`, guarded grants): a
/// command that sets, resets or previews a `core.grant.*` setting refuses the
/// whole batch, for every plugin, the one the grant names and the settings
/// editor included.
fn refused_outright(batch: &CommandBatch<'_>) -> Option<i32> {
    batch.iter().any(touches_grant).then_some(ERR_DENIED)
}

/// Whether `command` writes a user grant (`core.grant.*`). `ApplyPreset` names
/// a preset by id only and writes nothing here; what a preset sets is checked
/// where presets resolve (REG), where a grant key must be dropped as well.
fn touches_grant(command: WireCommand<'_>) -> bool {
    match command {
        WireCommand::SetSetting { key, .. }
        | WireCommand::ResetSetting { key, .. }
        | WireCommand::PreviewSetting { key, .. } => key.starts_with("core.grant."),
        _ => false,
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
#[path = "commands_limit_tests.rs"]
mod limit_tests;

#[cfg(test)]
#[path = "commands_redraw_below_tests.rs"]
mod redraw_below_tests;

#[cfg(test)]
#[path = "commands_hint_tests.rs"]
mod hint_tests;

#[cfg(test)]
#[path = "commands_grant_tests.rs"]
mod grant_tests;

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

    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    #[test]
    fn a_setting_there_is_no_room_to_read_is_not_an_invalid_key() {
        use crate::native_host::described_settings::tests::{no_room_for_a_spec, probe};
        let (_shown, settings) = probe();
        let owner = Id::new_static("probe");
        let stage = CommandStage::new(&owner, &settings, None, 1, 40);
        no_room_for_a_spec();
        assert_eq!(stage.admitted("probe.one").err(), Some(ERR_NO_MEMORY));
        assert_eq!(stage.admitted("probe.none").err(), Some(ERR_INVALID));
        assert!(stage.admitted("probe.one").is_ok());
    }

    #[test]
    fn overlapping_submission_refuses_without_panicking() {
        let scope = CommandScope::new();
        let _borrow = scope.current.borrow_mut();
        // SAFETY: a zero-length submission reads no storage.
        assert_eq!(unsafe { scope.submit(core::ptr::null(), 0) }, ERR_BUSY);
    }
}
