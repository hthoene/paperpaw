//! One retained instance registry and a deduplicated outer-callback start queue.
//! Registrations retain leases from this registry, never a second plugin owner.
use super::{
    event_tags::EventTags,
    lease_slot::{Slot, Unpark},
    native_callbacks::{CallbackClock, CallbackRuntime, NativeCallbackEpoch},
    native_screen::Dispatch,
    owner_directory::OwnerDirectory,
    resources::OwnerResourceView,
    shared::{self, Shared},
};
use alloc::{boxed::Box, string::String, vec::Vec};
use core::cell::Cell;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_STATE};
use paperpaw_sdk::Id;

pub(crate) trait NativeInstance: Dispatch {
    fn running(&self) -> bool;
    fn stop(&self);
    fn callback_depth(&self) -> u8 {
        0
    }
    /// Whether the instance subscribed to the event `tag` (at the start, read
    /// from the committed registrations; no allocation).
    fn subscribed(&self, _tag: u32) -> bool {
        false
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(None);
    }
    /// Whether the owner holds nothing the host keeps for it now, so that
    /// dropping the instance ends nothing (#248, D3). `false` unless the
    /// instance can tell.
    fn holds_nothing(&self) -> bool {
        false
    }
    /// Deliver the one-record event `bytes` of a host service (`core.net`,
    /// `core.sockets`) to the callback the instance registered for its tag.
    ///
    /// # Errors
    /// `ERR_UNSUPPORTED` when the instance registered none for the tag, the
    /// callback's own status when it failed.
    fn service_event(&self, _bytes: &[u8], _env: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    }
    /// As [`Self::service_event`], the callback lent the command `stage` (every
    /// callback but `draw` gets one). An instance without a stage path takes
    /// the event without one.
    fn service_event_staged(
        &self,
        bytes: &[u8],
        env: &paperpaw_sdk::Env<'_>,
        _stage: &mut super::commands::CommandStage<'_>,
    ) -> Result<i32, i32> {
        self.service_event(bytes, env)
    }
    /// The `event` callback at `token` with the one-record `bytes`, lent the
    /// command `stage` (an action's `event.action_run`). An instance without a
    /// stage path takes it without one.
    fn event_staged(
        &self,
        token: u32,
        bytes: &[u8],
        env: &paperpaw_sdk::Env<'_>,
        _stage: &mut super::commands::CommandStage<'_>,
    ) -> Result<i32, i32> {
        self.event(token, bytes, env)
    }
    /// A widget's `input` callback at `token` with the one-record `bytes`
    /// (`input.widget`), lent the command `stage`. An instance without a
    /// stage path takes it without one.
    fn input_staged(
        &self,
        token: u32,
        bytes: &[u8],
        env: &paperpaw_sdk::Env<'_>,
        _stage: &mut super::commands::CommandStage<'_>,
    ) -> Result<i32, i32> {
        self.input(token, bytes, env)
    }
    /// Revoke before owner cleanup; the registry supplies the original key.
    fn retire(&self, _owner: u64) -> Result<(), i32> {
        self.stop();
        Ok(())
    }
}

pub(crate) struct NativeLease(Shared<Slot>);

impl NativeLease {
    pub(crate) fn try_new(instance: impl NativeInstance + 'static) -> Result<Self, i32> {
        let instance: Box<dyn NativeInstance> = shared::try_box(instance).ok_or(ERR_NO_MEMORY)?;
        Shared::try_new(Slot::live_now(instance))
            .map(Self)
            .map_err(|_| ERR_NO_MEMORY)
    }
    /// A parked plugin's lease: running for everyone, made by `recipe` at
    /// the first call at a safe point of `clock` (`lease_slot`).
    pub(crate) fn parked(recipe: Box<dyn Unpark>, clock: CallbackRuntime) -> Result<Self, i32> {
        let slot = Slot::parked(recipe, clock).ok_or(ERR_NO_MEMORY)?;
        Shared::try_new(slot).map(Self).map_err(|_| ERR_NO_MEMORY)
    }
    pub(crate) fn try_clone(&self) -> Result<Self, i32> {
        self.0.try_clone().map(Self).ok_or(ERR_NO_MEMORY)
    }
    /// The one accessor of a call into the plugin: a parked plugin is made
    /// here at a safe point, else `ERR_BUSY` (asked to start on the next
    /// pass). `ERR_CANCELLED` for a stopped plugin.
    pub(crate) fn live(&self) -> Result<core::cell::Ref<'_, dyn NativeInstance>, i32> {
        self.0.live()
    }
    /// Its slot, for what only a parked plugin's lease does (`lease_slot`).
    pub(crate) fn slot(&self) -> &Slot {
        &self.0
    }
    /// Running, parked included: a parked plugin is running for everyone.
    pub(crate) fn running(&self) -> bool {
        self.0.running()
    }
    /// The live instance without making a parked one (state and metadata
    /// only; `None` while parked).
    pub(crate) fn peek(&self) -> Option<core::cell::Ref<'_, dyn NativeInstance>> {
        self.0.peek()
    }
    pub(crate) fn is_parked(&self) -> bool {
        self.0.is_parked()
    }
    /// Whether the plugin subscribed to `tag`, without making it.
    pub(crate) fn subscribed(&self, tag: u32) -> bool {
        self.0.running() && self.0.subscribed(tag)
    }
    /// The plugin's image, without making it.
    pub(crate) fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        self.0.with_image(visit);
    }
    pub(crate) fn take_start(&self) -> bool {
        self.0.take_start()
    }
    pub(crate) fn refused(&self) -> bool {
        self.0.refused()
    }
    /// The callback depth of the live instance, without making a parked
    /// one (0 while parked: a nested call into it answers `ERR_BUSY`).
    ///
    /// # Errors
    /// `ERR_CANCELLED` for a stopped plugin.
    pub(crate) fn checked_depth(&self) -> Result<u8, i32> {
        if !self.0.running() {
            return Err(paperpaw_native::host_glue::ERR_CANCELLED);
        }
        Ok(self
            .0
            .peek()
            .map_or(0, |instance| instance.callback_depth()))
    }
    /// `ERR_CANCELLED` for a stopped plugin, `Ok` for a running or parked
    /// one: a check that makes nothing.
    pub(crate) fn check_running(&self) -> Result<(), i32> {
        if self.0.running() {
            Ok(())
        } else {
            Err(paperpaw_native::host_glue::ERR_CANCELLED)
        }
    }
    pub(crate) fn stop(&self) {
        self.0.stop();
    }
    /// A service's event for the callback registered for its tag.
    pub(crate) fn service_event(
        &self,
        bytes: &[u8],
        env: &paperpaw_sdk::Env<'_>,
    ) -> Result<i32, i32> {
        self.live()?.service_event(bytes, env)
    }
    /// A widget's `input` callback, with its command stage.
    pub(crate) fn input_staged(
        &self,
        token: u32,
        bytes: &[u8],
        env: &paperpaw_sdk::Env<'_>,
        stage: &mut super::commands::CommandStage<'_>,
    ) -> Result<i32, i32> {
        self.live()?.input_staged(token, bytes, env, stage)
    }
    /// The `event` callback at `token`, lent the command `stage`.
    pub(crate) fn event_staged(
        &self,
        token: u32,
        bytes: &[u8],
        env: &paperpaw_sdk::Env<'_>,
        stage: &mut super::commands::CommandStage<'_>,
    ) -> Result<i32, i32> {
        self.live()?.event_staged(token, bytes, env, stage)
    }
    /// As [`Self::service_event`], with the callback's command stage.
    pub(crate) fn service_event_staged(
        &self,
        bytes: &[u8],
        env: &paperpaw_sdk::Env<'_>,
        stage: &mut super::commands::CommandStage<'_>,
    ) -> Result<i32, i32> {
        self.live()?.service_event_staged(bytes, env, stage)
    }
}

impl Dispatch for NativeLease {
    fn take_open_info(&self) -> Option<super::ui_store::Info> {
        self.live().ok()?.take_open_info()
    }
    fn event(&self, token: u32, bytes: &[u8], env: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        self.live()?.event(token, bytes, env)
    }
    fn input(&self, token: u32, bytes: &[u8], env: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        self.live()?.input(token, bytes, env)
    }
    fn draw(
        &self,
        token: u32,
        frame: &super::registrations::DrawFrame<'_>,
        env: &paperpaw_sdk::Env<'_>,
        canvas: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        self.live()?.draw(token, frame, env, canvas)
    }
    fn screen_callback(
        &self,
        instance: Option<paperpaw_sdk::ScreenKey>,
        token: u32,
        bytes: &[u8],
        env: &paperpaw_sdk::Env<'_>,
        input: bool,
    ) -> Result<i32, i32> {
        self.live()?
            .screen_callback(instance, token, bytes, env, input)
    }
    fn screen_draw(
        &self,
        instance: Option<paperpaw_sdk::ScreenKey>,
        token: u32,
        frame: &super::registrations::DrawFrame<'_>,
        env: &paperpaw_sdk::Env<'_>,
        canvas: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        self.live()?
            .screen_draw(instance, token, frame, env, canvas)
    }
    fn stage(
        &self,
        token: u32,
        bytes: &[u8],
        out: &mut [u8],
        env: &paperpaw_sdk::Env<'_>,
    ) -> Result<i32, i32> {
        self.live()?.stage(token, bytes, out, env)
    }
    fn dropped(&self, token: u32, status: i32) {
        // Told to the live instance only: a parked plugin was never called.
        if let Some(instance) = self.0.peek() {
            instance.dropped(token, status);
        }
    }
}

struct Entry {
    key: u64,
    id: Id,
    instance: Option<NativeLease>,
    enabled: bool,
    start: Cell<bool>,
    retiring: bool,
    /// The folder of the version that replaces it once it is retired: a box
    /// of its bytes, not a `String` (a word less in every row, #248 B).
    replacement: Option<Box<str>>,
    /// When a failed retirement is tried again (ms), with
    /// [`FAILURE_REPORTED`]: one field for both (#248 B: the row is 40
    /// bytes on the X4, was 48).
    retry_at: u64,
    /// It declared `uses.install` (it may answer plugin requests).
    installer: bool,
}

/// The bit of [`Entry::retry_at`] that says its failed retirement was said
/// already (a time never reaches it).
const FAILURE_REPORTED: u64 = 1 << 63;

impl Entry {
    /// When its failed retirement is tried again (ms; 0: at once).
    fn retry_at(&self) -> u64 {
        self.retry_at & !FAILURE_REPORTED
    }
}

#[derive(Default)]
pub(crate) struct NativeInstances {
    entries: Vec<Entry>,
    /// The plugin requests under way (`plugin_requests.rs`); nothing is
    /// allocated before the first request.
    pub(crate) plugin_requests: core::cell::RefCell<super::plugin_requests::PluginRequests>,
    /// The callbacks of plugins that failed (`event_failures.rs`).
    pub(crate) event_failures: super::event_failures::EventFailures,
    callbacks: CallbackClock,
    pool: Option<OwnerResourceView>,
    orphan_retry_at: Cell<u64>,
    orphan_failure_reported: Cell<bool>,
    /// The event pump left events for another pass (see `service_pump`).
    service_more: Cell<bool>,
    /// The pump's event buffer, made once (see `service_pump`), and when it
    /// may try again after a refusal.
    service_buffer: core::cell::RefCell<Vec<u8>>,
    service_retry: Cell<Option<u64>>,
    /// When the library's move job may be pumped again after a step that
    /// failed (`moves_pump`).
    moves_retry: Cell<Option<u64>>,
    /// When the document preparing loop may step again after a step that
    /// could not be made (`doc_pump`).
    doc_retry: Cell<Option<u64>>,
    /// How many steps of the preparing loop in a row could not be made (its
    /// back-off).
    doc_failures: Cell<u8>,
    /// How many steps in a row were refused (the back-off of `moves_pump`).
    moves_failures: Cell<u8>,
    /// The count of begun moves the pump last saw (`moves_pump`).
    moves_begun: Cell<u32>,
    /// Where the event pump goes on (the running plugin it starts at).
    service_cursor: Cell<usize>,
    /// What the work pump saw of the device at its last pass (see
    /// `work_pump`): bit 0 seen at all, bit 1 network up, bit 2 charging.
    work_seen: Cell<u8>,
    /// What the image pump keeps between passes (see `image_pump`).
    pub(super) image_pump: super::image_pump::ImagePump,
    /// The queues of `event.setting_changed` (see `setting_events`).
    pub(super) setting_events: super::setting_events::SettingEvents,
    /// The commands a callback staged, till the kernel runs them (see
    /// `event_call`).
    pub(super) outbox: super::event_call::Outbox,
    /// The actions run since the kernel last called its pump (see
    /// `native_action`); made when the first native action is declared.
    actions: core::cell::RefCell<Option<super::shared::Shared<super::native_action::ActionQueue>>>,
    /// The hand-over the staged `open` being applied names: `Some(None)` an
    /// open of none, `None` no staged open (see `Kernel::drain_native_opens`).
    pending_handover: Cell<Option<Option<u32>>>,
    /// The kernel handles every plugin's dispatch reads (meter, stack rows,
    /// declaration directory …): one for all, made with the first plugin
    /// (`Kernel::shared_flat_host`), so each keeps a handle, not a copy.
    pub(super) flat_host: Option<super::shared::Shared<super::FlatHost>>,
    #[cfg(test)]
    fixture_pool: Option<super::HostResources>,
}

pub(crate) enum InstanceState {
    Running(NativeLease),
    Lazy,
    Stopped,
}

pub(crate) struct CallbackEpoch<'a> {
    registry: &'a NativeInstances,
    epoch: u64,
}
impl CallbackEpoch<'_> {
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }
}
impl Drop for CallbackEpoch<'_> {
    fn drop(&mut self) {
        self.registry.callbacks.leave();
    }
}

/// The event tags an instance subscribed to.
fn subscriptions(lease: &NativeLease) -> EventTags {
    // A parked plugin's subscriptions are its recipe's: nothing is made.
    if lease.running() {
        EventTags::collect(|tag| lease.subscribed(tag))
    } else {
        EventTags::default()
    }
}

impl NativeInstances {
    /// The owner directory of the device pool, once an owner was admitted.
    fn directory(&self) -> Option<&Shared<OwnerDirectory>> {
        self.pool.as_ref()?.owners().ok()
    }

    /// Hands the plugin id of the enabled owner `key` to `visit`; `false`
    /// for an owner that is unknown or stopped, or while the directory is
    /// busy. Nothing is copied.
    pub(crate) fn with_id(&self, key: u64, visit: &mut dyn FnMut(&str)) -> bool {
        self.directory()
            .is_some_and(|directory| directory.with_id(key, visit).is_ok())
    }

    /// Whether the enabled plugin `id` has a screen on the stack
    /// ([`OwnerDirectory::plugin_has_screen`]).
    pub(crate) fn plugin_has_screen(&self, id: &str) -> bool {
        self.directory()
            .is_some_and(|directory| directory.plugin_has_screen(id))
    }

    /// How many owner-directory updates were skipped since the last call.
    pub(crate) fn take_directory_missed(&self) -> u32 {
        self.directory()
            .map_or(0, |directory| directory.take_missed())
    }

    /// Lets the device services see the kernel's screen stack.
    pub(crate) fn attach_screens(&self, screens: &alloc::rc::Rc<super::stack_rows::StackRows>) {
        if let Some(directory) = self.directory() {
            directory.attach_screens(screens);
        }
    }

    /// The staged `open` about to be applied names `handover` (or none).
    pub(crate) fn set_pending_handover(&self, pending: Option<Option<u32>>) {
        self.pending_handover.set(pending);
    }

    /// Takes the hand-over the applied `open` was given: the first screen it
    /// reaches keeps it, no other.
    pub(crate) fn take_pending_handover(&self) -> Option<Option<u32>> {
        self.pending_handover.take()
    }

    /// What the next native `on_reopen` reports of the hand-over just attached.
    pub(crate) fn set_open_info(&self, info: Option<super::ui_store::Info>) {
        self.callbacks.set_open_info(info);
    }

    /// Whether a native screen's `draw` callback returned `OK` since the last
    /// call (a trap, a failure status, a frame the host cut short and a screen
    /// that was not called at all give `false`); clears it.
    pub(crate) fn take_drew(&self) -> bool {
        self.callbacks.take_drew()
    }

    /// Whether a staged `open` is being applied (it names a hand-over or none).
    pub(crate) fn pending_handover(&self) -> Option<Option<u32>> {
        self.pending_handover.get()
    }

    pub(crate) fn ui_store(&self) -> Option<&super::ui_store::UiStore> {
        self.pool.as_ref()?.ui_store()
    }

    /// The `core.ui` service of the device pool (widget and status tables).
    pub(crate) fn ui_services(&self) -> Option<&super::ui_service::UiServices> {
        self.pool.as_ref()?.ui()
    }

    /// What the screens' callbacks stage for the kernel, once the pool is
    /// adopted (a kernel with no native plugin has none).
    pub(crate) fn screen_stage(&self) -> Option<&super::screen_stage::ScreenStage> {
        Some(&self.pool.as_ref()?.hosts().screen_stage)
    }

    /// Notes that the instance of `key` declared `uses.install`.
    pub(crate) fn set_installer(&mut self, key: u64, installer: bool) {
        if let Some(entry) = self.entry_mut(key) {
            entry.installer = installer;
        }
    }

    /// Whether the running instance of `key` may answer plugin requests.
    pub(crate) fn is_installer(&self, key: u64) -> bool {
        self.entry(key)
            .is_some_and(|entry| entry.enabled && entry.installer)
    }

    /// The host owner key of the running plugin `id`, if it has an instance.
    pub(crate) fn key_of(&self, id: &str) -> Option<u64> {
        self.entries
            .iter()
            .find(|entry| entry.enabled && entry.id.as_str() == id)
            .map(|entry| entry.key)
    }

    /// Retain the device pool before any fallible admission work, including
    /// the first failed factory. Its orphan cleanup must remain reachable.
    pub(crate) fn adopt_pool(&mut self, identity: OwnerResourceView) -> Result<(), i32> {
        if self
            .pool
            .as_ref()
            .is_some_and(|pool| !pool.same_pool(&identity))
        {
            return Err(ERR_STATE);
        }
        self.pool.get_or_insert(identity);
        Ok(())
    }

    /// Keys never repeat; storage rows may move when failed admission is reclaimed.
    pub(crate) fn insert(
        &mut self,
        identity: OwnerResourceView,
        id: Id,
        instance: Option<NativeLease>,
        enabled: bool,
    ) -> Result<u64, i32> {
        let key = identity.owner_id();
        let directory = identity.owners()?.try_clone().ok_or(ERR_NO_MEMORY)?;
        self.adopt_pool(identity)?;
        if self
            .entries
            .iter()
            .any(|entry| entry.id == id || entry.key == key)
        {
            return Err(ERR_INVALID);
        }
        self.entries.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        if enabled {
            directory.admit(key, &id)?;
            if let Some(lease) = &instance {
                directory.subscribe(key, subscriptions(lease));
            }
        }
        self.entries.push(Entry {
            key,
            id,
            instance,
            enabled,
            start: Cell::new(false),
            retiring: false,
            replacement: None,
            retry_at: 0,
            installer: false,
        });
        Ok(key)
    }

    /// The queue the actions of native plugins wait in, made on the first
    /// call.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` when it cannot be made.
    pub(crate) fn action_queue(
        &self,
    ) -> Result<super::shared::Shared<super::native_action::ActionQueue>, i32> {
        let mut slot = self.actions.try_borrow_mut().map_err(|_| ERR_STATE)?;
        if slot.is_none() {
            *slot = Some(
                super::shared::Shared::try_new(super::native_action::ActionQueue::default())
                    .map_err(|_| ERR_NO_MEMORY)?,
            );
        }
        slot.as_ref()
            .and_then(super::shared::Shared::try_clone)
            .ok_or(ERR_NO_MEMORY)
    }

    /// The queue of actions if one was made (no action was ever declared
    /// without it).
    pub(crate) fn action_queue_made(
        &self,
    ) -> Option<super::shared::Shared<super::native_action::ActionQueue>> {
        self.actions
            .try_borrow()
            .ok()?
            .as_ref()
            .and_then(super::shared::Shared::try_clone)
    }

    /// The ids of the interface-7 plugins known, enabled or not.
    pub(crate) fn plugin_ids(&self) -> impl Iterator<Item = &Id> {
        self.entries.iter().map(|entry| &entry.id)
    }

    /// A lease of the installed, enabled plugin `key` (`None` for none, or
    /// where memory refuses the clone).
    pub(crate) fn lease(&self, key: u64) -> Option<NativeLease> {
        self.entry(key)
            .filter(|entry| entry.enabled)
            .and_then(|entry| entry.instance.as_ref())
            .and_then(|lease| lease.try_clone().ok())
    }

    pub(crate) fn id(&self, key: u64) -> Option<&Id> {
        self.entry(key)
            .filter(|entry| entry.enabled)
            .map(|entry| &entry.id)
    }

    /// Whether the running instance of `key` subscribed to the event `tag`,
    /// asked of the instance itself (the services read the owner directory,
    /// which the install keeps; the work pump runs between callbacks and
    /// asks the instance).
    pub(crate) fn subscribed(&self, key: u64, tag: u32) -> bool {
        self.entry(key)
            .filter(|entry| entry.enabled)
            .and_then(|entry| entry.instance.as_ref())
            .is_some_and(|lease| lease.subscribed(tag))
    }

    pub(crate) fn find(&self, id: &str) -> Option<u64> {
        self.entries
            .iter()
            .find(|entry| entry.id.as_str() == id)
            .map(|entry| entry.key)
    }

    pub(crate) fn state(&self, key: u64) -> Result<InstanceState, i32> {
        let entry = self.entry(key).ok_or(ERR_STATE)?;
        if !entry.enabled {
            return Ok(InstanceState::Stopped);
        }
        match &entry.instance {
            Some(instance) if instance.running() => {
                Ok(InstanceState::Running(instance.try_clone()?))
            }
            Some(_) => Ok(InstanceState::Stopped),
            None => Ok(InstanceState::Lazy),
        }
    }

    /// A frozen run may retain the returned lease after this entry stops.
    /// Revocation makes that old lease CANCELLED without invoking foreign code.
    pub(crate) fn stop(&mut self, key: u64) -> Result<(), i32> {
        if let Some(directory) = self.directory() {
            directory.withdraw(key);
        }
        let entry = self.entry_mut(key).ok_or(ERR_STATE)?;
        entry.enabled = false;
        entry.start.set(false);
        entry.retiring = true;
        if let Some(instance) = &entry.instance {
            instance.stop();
        }
        Ok(())
    }

    pub(crate) fn request_start(&self, key: u64) -> Result<(), i32> {
        let entry = self.entry(key).ok_or(ERR_STATE)?;
        if entry.enabled && entry.instance.is_none() {
            entry.start.set(true);
        }
        Ok(())
    }

    /// A parked plugin whose start is due now (asked where it could not be
    /// made, its wait over) and whether its batch was refused: at a safe
    /// point only (`lease_slot`, #248).
    pub(crate) fn take_parked_start(&self) -> Option<(u64, bool)> {
        if !self.callbacks.safe_point() {
            return None;
        }
        self.entries
            .iter()
            .filter(|entry| entry.enabled && !entry.retiring)
            .find_map(|entry| {
                let lease = entry.instance.as_ref()?;
                lease.take_start().then(|| (entry.key, lease.refused()))
            })
    }
    /// The kernel's time, for a parked plugin's wait after no memory.
    pub(crate) fn set_now(&self, now_ms: u64) {
        self.callbacks.set_now(now_ms);
    }
    /// No parked plugin is made while the guard lives (a registration).
    pub(crate) fn hold_unparking(&self) -> Option<super::native_callbacks::Hold> {
        self.callbacks.hold()
    }
    /// The clock a parked plugin's lease reads its safe points from.
    pub(crate) fn unpark_clock(&mut self) -> Result<CallbackRuntime, i32> {
        self.callbacks.runtime()
    }

    /// Zero allocation, deduplicated by entry; never flush inside a callback.
    pub(crate) fn take_start(&self) -> Option<u64> {
        if self.callbacks.depth() != 0 {
            return None;
        }
        self.entries
            .iter()
            .find(|entry| entry.enabled && entry.start.replace(false))
            .map(|entry| entry.key)
    }

    pub(crate) fn install(&mut self, key: u64, instance: NativeLease) -> Result<(), i32> {
        if self.callbacks.depth() != 0 {
            return Err(ERR_STATE);
        }
        let subscribed = subscriptions(&instance);
        let entry = self.entry_mut(key).ok_or(ERR_STATE)?;
        if !entry.enabled || entry.instance.is_some() {
            return Err(ERR_STATE);
        }
        entry.instance = Some(instance);
        // A plugin that starts again may fail anew, and be said.
        self.event_failures.forget(key);
        if let Some(directory) = self.directory() {
            directory.subscribe(key, subscribed);
        }
        Ok(())
    }

    pub(crate) fn callback(&self) -> Result<CallbackEpoch<'_>, i32> {
        let epoch = self.callbacks.enter_outer()?;
        Ok(CallbackEpoch {
            registry: self,
            epoch,
        })
    }

    /// Bind only an existing, enabled registry slot to an actual callback owner.
    pub(crate) fn identity(&mut self, key: u64) -> Result<NativeIdentity, i32> {
        if !self.entry(key).is_some_and(|entry| entry.enabled) {
            return Err(ERR_STATE);
        }
        let owner = key;
        Ok(NativeIdentity {
            owner,
            callbacks: self.callbacks.runtime()?,
            // No allocation: a refused share just leaves the handles to the
            // next grant or the plugin's stop (see `NativeCallbackEpoch`).
            pool: self.pool.as_ref().and_then(OwnerResourceView::try_pool),
        })
    }

    /// Retire a published owner. Cleanup is retryable under the same key;
    /// callbacks and broker lookup are revoked even when cleanup is busy.
    pub(crate) fn retire(&mut self, key: u64) -> Result<Option<String>, i32> {
        if let Some(directory) = self.directory() {
            directory.withdraw(key);
        }
        let entry = self.entry_mut(key).ok_or(ERR_STATE)?;
        entry.enabled = false;
        entry.retiring = true;
        entry.start.set(false);
        if let Some(instance) = &entry.instance {
            instance.0.retire(key)?;
        }
        let at = self
            .entries
            .iter()
            .position(|entry| entry.key == key)
            .ok_or(ERR_STATE)?;
        Ok(self.entries.swap_remove(at).replacement.map(String::from))
    }

    /// Reclaim a failed admission without reusing its key or moving any identity.
    pub(crate) fn abandon(&mut self, key: u64) -> Result<(), i32> {
        self.stop(key)?;
        let at = self
            .entries
            .iter()
            .position(|entry| entry.key == key)
            .ok_or(ERR_STATE)?;
        self.entries.swap_remove(at);
        Ok(())
    }

    pub(crate) fn defer_replacement(&mut self, key: u64, folder: String) -> Result<(), i32> {
        let entry = self.entry_mut(key).ok_or(ERR_STATE)?;
        if !entry.retiring || entry.replacement.is_some() {
            return Err(paperpaw_native::host_glue::ERR_BUSY);
        }
        // Exactly its bytes, asked for fallibly: the box then gives nothing
        // back (no reallocation that could fail).
        let mut exact = String::new();
        exact
            .try_reserve_exact(folder.len())
            .map_err(|_| paperpaw_native::host_glue::ERR_NO_MEMORY)?;
        exact.push_str(&folder);
        entry.replacement = Some(exact.into_boxed_str());
        Ok(())
    }

    pub(crate) fn replacement_pending(&self, id: &Id) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.id == *id && entry.replacement.is_some())
    }

    /// Orphaned Drop cleanup uses HostServices' one fixed retry queue. Rows
    /// retained here still own lifecycle/handle cleanup and replacement state;
    /// they are never enqueued as orphaned owners.
    pub(crate) fn retry_orphaned_retirements(&self, now: u64) -> Result<bool, i32> {
        if now < self.orphan_retry_at.get() {
            return Ok(true);
        }
        let result = self
            .pool
            .as_ref()
            .map_or(Ok(false), OwnerResourceView::retry_retirements);
        match result {
            Err(status) if status != paperpaw_native::host_glue::ERR_BUSY => {
                self.orphan_retry_at.set(now.saturating_add(5_000));
                if self.orphan_failure_reported.replace(true) {
                    Ok(true)
                } else {
                    Err(status)
                }
            }
            other => {
                self.orphan_retry_at.set(0);
                if other == Ok(false) {
                    self.orphan_failure_reported.set(false);
                }
                other
            }
        }
    }

    pub(crate) fn retirement_failed(&mut self, key: u64, status: i32, now: u64) -> bool {
        let Some(entry) = self.entry_mut(key) else {
            return false;
        };
        if status == paperpaw_native::host_glue::ERR_BUSY {
            entry.retry_at &= FAILURE_REPORTED;
            return false;
        }
        let reported = entry.retry_at & FAILURE_REPORTED != 0;
        entry.retry_at = now.saturating_add(5_000).min(!FAILURE_REPORTED) | FAILURE_REPORTED;
        !reported
    }

    pub(crate) fn retirement_ready(&self, key: u64, now: u64) -> bool {
        self.entry(key).is_some_and(|entry| entry.retry_at() <= now)
    }

    pub(crate) fn retirement_due(&self, now: u64) -> Option<u64> {
        let soon = now.saturating_add(50);
        let native = self
            .entries
            .iter()
            .filter(|entry| Self::needs_retirement(entry))
            .map(|entry| entry.retry_at().max(soon))
            .min();
        let orphan = self
            .pool
            .as_ref()
            .is_some_and(OwnerResourceView::retirements_pending)
            .then(|| self.orphan_retry_at.get().max(soon));
        native.into_iter().chain(orphan).min()
    }

    fn needs_retirement(entry: &Entry) -> bool {
        entry.retiring
            || entry
                .instance
                .as_ref()
                .is_some_and(|instance| !instance.0.running())
    }

    /// Also notices a Hosted::stop request made through a retained owner.
    pub(crate) fn pending_unload(&self, after: Option<u64>) -> Option<(u64, &Id)> {
        self.entries
            .iter()
            .filter(|entry| after.is_none_or(|key| entry.key > key))
            .filter(|entry| Self::needs_retirement(entry))
            .min_by_key(|entry| entry.key)
            .map(|entry| (entry.key, &entry.id))
    }

    fn entry(&self, key: u64) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.key == key)
    }

    fn entry_mut(&mut self, key: u64) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|entry| entry.key == key)
    }

    #[cfg(test)]
    pub(crate) fn test_insert(
        &mut self,
        id: Id,
        instance: Option<NativeLease>,
        enabled: bool,
    ) -> Result<u64, i32> {
        if self.fixture_pool.is_none() {
            self.fixture_pool = Some(super::HostResources::try_new().map_err(|_| ERR_NO_MEMORY)?);
        }
        let owner = self.fixture_pool.as_ref().ok_or(ERR_STATE)?.try_owner()?;
        self.insert(
            owner.try_view().ok_or(ERR_NO_MEMORY)?,
            id,
            instance,
            enabled,
        )
    }

    /// The host resources `test_insert` made, to wire services into.
    #[cfg(test)]
    pub(crate) fn test_resources(&self) -> Option<&super::HostResources> {
        self.fixture_pool.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn retained_rows(&self) -> (usize, usize) {
        (self.entries.len(), self.entries.capacity())
    }

    pub(crate) fn depth(&self) -> u8 {
        self.callbacks.depth()
    }

    /// Whether the last event pass left events for another.
    pub(crate) fn service_more(&self) -> bool {
        self.service_more.get()
    }

    pub(crate) fn service_buffer(&self) -> &core::cell::RefCell<Vec<u8>> {
        &self.service_buffer
    }

    pub(crate) fn service_retry(&self) -> Option<u64> {
        self.service_retry.get()
    }

    pub(crate) fn set_service_retry(&self, at: Option<u64>) {
        self.service_retry.set(at);
    }

    pub(crate) fn moves_retry(&self) -> Option<u64> {
        self.moves_retry.get()
    }

    pub(crate) fn set_moves_retry(&self, at: Option<u64>) {
        self.moves_retry.set(at);
    }

    pub(crate) fn doc_retry(&self) -> Option<u64> {
        self.doc_retry.get()
    }

    pub(crate) fn set_doc_retry(&self, at: Option<u64>) {
        self.doc_retry.set(at);
    }

    /// Counts one more step of the preparing loop that could not be made and
    /// returns how many in a row.
    pub(crate) fn doc_failed(&self) -> u8 {
        let failures = self.doc_failures.get().saturating_add(1);
        self.doc_failures.set(failures);
        failures
    }

    /// A step of the preparing loop was made, or none is due: the back-off
    /// starts over.
    pub(crate) fn doc_ok(&self) {
        self.doc_failures.set(0);
    }

    /// Counts one more refused step and returns how many in a row.
    pub(crate) fn moves_failed(&self) -> u8 {
        let failures = self.moves_failures.get().saturating_add(1);
        self.moves_failures.set(failures);
        failures
    }

    /// The pump saw the count of begun moves: when it differs from the last
    /// one, the back-off of earlier steps is dropped. It matters when no job
    /// is open (a move is refused as busy while one is, and a refused step
    /// keeps its job open): a journal that could not be read at the start, a
    /// job ended by the failure.
    pub(crate) fn moves_begun_seen(&self, begun: u32) {
        if self.moves_begun.replace(begun) != begun {
            self.moves_failures.set(0);
            self.moves_retry.set(None);
        }
    }

    /// A step was not refused: the back-off starts again.
    pub(crate) fn moves_ok(&self) {
        self.moves_failures.set(0);
    }

    pub(crate) fn work_seen(&self) -> u8 {
        self.work_seen.get()
    }

    pub(crate) fn set_work_seen(&self, seen: u8) {
        self.work_seen.set(seen);
    }

    /// The device's `core.work` host, once the shell wired it.
    pub(crate) fn work_services(&self) -> Option<&super::work_service::WorkServices> {
        self.pool.as_ref()?.hosts().work()
    }

    /// Gives back what the tables kept in reserve while the plugins were
    /// hosted one by one (every list grows by doubling): called once, when
    /// the shell has hosted them all ([`Kernel::start`](crate::Kernel::start)).
    pub(crate) fn shrink_to_fit(&mut self) {
        super::shrink::shrink(&mut self.entries);
        self.setting_events.shrink_to_fit();
        if let Some(directory) = self.directory() {
            directory.shrink_to_fit();
        }
        if let Some(hosts) = self.hosts() {
            hosts.shrink_to_fit();
        }
    }

    /// The device's wired host views (none before a pool is adopted).
    pub(crate) fn hosts(&self) -> Option<&super::host_services::HostServices> {
        self.pool.as_ref().map(|pool| pool.hosts())
    }

    pub(crate) fn service_cursor(&self) -> usize {
        self.service_cursor.get()
    }

    pub(crate) fn set_service_cursor(&self, at: usize) {
        self.service_cursor.set(at);
    }

    pub(crate) fn set_service_more(&self, more: bool) {
        self.service_more.set(more);
    }

    /// The device's services that raise events (`core.net`, `core.sockets`,
    /// `core.document`), those wired.
    pub(crate) fn event_services(&self) -> [Option<&dyn super::service_events::ServiceEvents>; 3] {
        self.pool
            .as_ref()
            .map_or([None, None, None], |pool| pool.hosts().event_services())
    }

    /// The device's network pool and the services' hosts, once a service
    /// made it.
    pub(crate) fn net_demand(
        &self,
    ) -> Option<(
        &alloc::rc::Rc<super::net_pool::NetPool>,
        &super::host_services::HostServices,
    )> {
        let hosts = self.pool.as_ref()?.hosts();
        Some((hosts.pool_made()?, hosts))
    }

    /// The closes the work scheduler could not take are asked again.
    pub(crate) fn retry_run_closes(&self, now_ms: u64) {
        if let Some(pool) = self.pool.as_ref() {
            pool.hosts().retry_run_closes(now_ms);
        }
    }

    /// The device's network policy, once `core.net` or `core.sockets` is wired.
    pub(crate) fn net_policy(&self) -> Option<alloc::rc::Rc<super::net_policy::NetPolicy>> {
        let hosts = self.pool.as_ref()?.hosts();
        (hosts.net().is_some() || hosts.sockets().is_some()).then(|| hosts.policy().clone())
    }

    /// A callback is on the stack, for the tests of what runs only between
    /// callbacks.
    #[cfg(test)]
    pub(crate) fn test_enter_callback(&self) {
        let _ = self.callbacks.enter_outer();
    }

    /// Ends the callback [`Self::test_enter_callback`] entered.
    #[cfg(test)]
    pub(crate) fn test_leave_callback(&self) {
        self.callbacks.leave();
    }

    /// The `index`th running instance, in order of installation: its key and
    /// its lease. `None` past the last.
    /// The first time a parked plugin asked to start may be made.
    pub(crate) fn parked_start_at(&self) -> Option<u64> {
        let leases = self
            .entries
            .iter()
            .filter_map(|entry| entry.instance.as_ref());
        leases.filter_map(|lease| lease.slot().start_at()).min()
    }
    pub(crate) fn running_nth(&self, index: usize) -> Option<(u64, &NativeLease)> {
        self.entries
            .iter()
            .filter(|entry| entry.enabled && !entry.retiring)
            .filter_map(|entry| Some((entry.key, entry.instance.as_ref()?)))
            .filter(|(_, lease)| lease.running())
            .nth(index)
    }
}

pub(crate) struct NativeIdentity {
    owner: u64,
    callbacks: CallbackRuntime,
    /// The device pool, to end the callback's status handles with it: the
    /// pool alone, not an owner's view of it (#248).
    pool: Option<super::HostResources>,
}
impl NativeIdentity {
    /// The same identity again: a parked plugin's recipe keeps one for each
    /// time it makes the plugin's dispatch (#248).
    pub(crate) fn try_clone(&self) -> Option<Self> {
        Some(Self {
            owner: self.owner,
            callbacks: self.callbacks.try_clone()?,
            pool: match &self.pool {
                Some(pool) => Some(pool.try_clone()?),
                None => None,
            },
        })
    }
    pub(crate) fn owner(&self) -> u64 {
        self.owner
    }
    pub(crate) fn callback(&self) -> Result<NativeCallbackEpoch, i32> {
        let epoch = self.callbacks.enter()?;
        Ok(
            match self.pool.as_ref().and_then(super::HostResources::try_clone) {
                Some(pool) => epoch.ending_status_handles(pool, self.owner),
                None => epoch,
            },
        )
    }
    pub(crate) fn take_open_info(&self) -> Option<super::ui_store::Info> {
        self.callbacks.take_open_info()
    }
    /// A native screen's draw returned `OK` (read by `take_drew`).
    pub(crate) fn mark_drew(&self) {
        self.callbacks.mark_drew();
    }
    /// The device's `core.ui` service (widget handles), once a shell wired it.
    pub(crate) fn ui_services(&self) -> Option<&super::ui_service::UiServices> {
        self.pool.as_ref()?.hosts().ui()
    }

    /// What the screens' callbacks stage for the kernel (`screen_stage`).
    pub(crate) fn screen_stage(&self) -> Option<&super::screen_stage::ScreenStage> {
        Some(&self.pool.as_ref()?.hosts().screen_stage)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "fixture identity comes from a real temporary registry"
)]
pub(crate) fn test_identity(owner: &str) -> NativeIdentity {
    let mut registry = NativeInstances::default();
    let key = registry
        .test_insert(Id::try_parse(owner).unwrap(), None, true)
        .unwrap();
    registry.identity(key).unwrap()
}

// What parking a plugin again asks of the table (#248, D3).
#[path = "native_instances_repark.rs"]
mod repark;

#[cfg(test)]
#[path = "native_instances_tests.rs"]
mod tests;
