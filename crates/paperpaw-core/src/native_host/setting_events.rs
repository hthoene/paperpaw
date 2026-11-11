//! `event.setting_changed` (64) and the hints 70-74 (`plugins_changed`,
//! `wifi_changed`, `power_changed`, `status_changed`, `charge_changed`) to the
//! plugins that subscribed to them (`core-events.md`): one bounded queue ([`Pending`]) per subscribed plugin,
//! filled where a kept value changes (`Kernel::native_setting_after`,
//! `native_setting_typed`), emptied by [`Kernel::native_setting_pump`] between
//! callbacks, through the same event wrapper as the services' events
//! ([`Kernel::native_event_call`]).
//!
//! * Only a plugin that registered for tag 64 has a queue; the others lose
//!   the edge, and nothing is kept for a plugin that stopped.
//! * What a plugin is told of the old value is what it could read with slot 5:
//!   a secret of another plugin and a grant are not announced at all, an own
//!   secret comes with `old` empty. The check is made
//!   again at delivery, so a value revoked in between does not leak.
//! * A change the plugin made itself is announced to it too: it is delivered
//!   after its callback returned, never inside it.
//! * A hint keeps only a pending bit in the same queue (a second one before
//!   the first was told is one), and its record is built from the state at
//!   delivery, not at the time it was raised.
//! * An edge that finds no room (16 held, 4,096 bytes a plugin, 8,192 the
//!   device, or no memory) is dropped and replaced by the **empty key**:
//!   "several settings changed, read again what you keep".
//! * Memory: a queue holds at most 16 edges and 4,096 bytes of them per
//!   plugin, and all queues together at most 8,192 bytes (the device-wide
//!   budget of [`EventBudget`]), besides each queue's small fixed part and one
//!   record built at delivery (at most 1 KiB, freed after the callback). That
//!   is the heap these events take on the device; a plugin that does not
//!   subscribe has no queue and costs nothing.
//! * The pump tells at most [`PER_OWNER`] events to one plugin a pass and
//!   starts one plugin further each pass, so no plugin starves another; it
//!   asks for another pass while events wait.
use super::{
    codec,
    event_pending::{EventBudget, Pending, Ready, Visibility},
    native_instances::{InstanceState, NativeInstances},
    registrations as wire,
};
use crate::abi7::settings_visibility::{SettingIdentity, visibility};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY};
use paperpaw_sdk::{Id, Settings, wifi::WifiState};

/// The tag of the event.
const TAG: u32 = wire::EVENT_SETTING_CHANGED_TAG;

/// The hint events a plugin can subscribe to.
const HINTS: [u32; 6] = [
    wire::EVENT_PLUGINS_CHANGED_TAG,
    wire::EVENT_WIFI_CHANGED_TAG,
    wire::EVENT_POWER_CHANGED_TAG,
    wire::EVENT_STATUS_CHANGED_TAG,
    wire::EVENT_CHARGE_CHANGED_TAG,
    wire::EVENT_PREVIEW_CHANGED_TAG,
];

/// Events told to one plugin in one pass.
pub(crate) const PER_OWNER: usize = 4;

/// The queues of the subscribed plugins, by owner.
#[derive(Default)]
pub(crate) struct SettingEvents {
    budget: RefCell<Option<EventBudget>>,
    queues: RefCell<Vec<(u64, Pending)>>,
    cursor: Cell<usize>,
}

impl SettingEvents {
    /// Gives back the room the queues kept while plugins were hosted.
    pub(crate) fn shrink_to_fit(&self) {
        if let Ok(mut queues) = self.queues.try_borrow_mut() {
            super::shrink::shrink(&mut queues);
        }
    }

    /// Whether any running plugin subscribed to the event (else nothing is
    /// captured for it).
    pub(crate) fn wanted(instances: &NativeInstances) -> bool {
        let mut n = 0;
        while let Some((owner, _)) = instances.running_nth(n) {
            if instances.subscribed(owner, TAG) {
                return true;
            }
            n += 1;
        }
        false
    }

    /// Whether `owner` subscribed to any event of these queues.
    fn listens(instances: &NativeInstances, owner: u64) -> bool {
        instances.subscribed(owner, TAG)
            || HINTS.iter().any(|&tag| instances.subscribed(owner, tag))
            || instances.subscribed(owner, wire::EVENT_REGISTRY_CHANGED_TAG)
            || super::core_events::listens(instances, owner)
    }

    /// The queue of `owner`, made on first use.
    fn queue_of<'a>(
        &self,
        queues: &'a mut Vec<(u64, Pending)>,
        owner: u64,
    ) -> Result<&'a mut Pending, i32> {
        if let Some(at) = queues.iter().position(|(key, _)| *key == owner) {
            return Ok(&mut queues[at].1);
        }
        let mut budget = self
            .budget
            .try_borrow_mut()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        let pending = match budget.as_ref() {
            Some(budget) => budget.pending()?,
            None => {
                let made = EventBudget::try_new()?;
                let pending = made.pending()?;
                *budget = Some(made);
                pending
            }
        };
        queues
            .try_reserve(1)
            .map_err(|_| paperpaw_native::host_glue::ERR_NO_MEMORY)?;
        queues.push((owner, pending));
        let last = queues.len() - 1;
        Ok(&mut queues[last].1)
    }

    /// Runs `visit` on the queue of `owner` (made on first use).
    pub(super) fn with_queue<R>(
        &self,
        owner: u64,
        visit: impl FnOnce(&mut Pending) -> R,
    ) -> Result<R, i32> {
        let mut queues = self
            .queues
            .try_borrow_mut()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        Ok(visit(self.queue_of(&mut queues, owner)?))
    }

    /// Announces one change (an empty `key`: read everything again) to every
    /// subscribed plugin, each according to what it may read.
    pub(crate) fn publish(
        &self,
        instances: &NativeInstances,
        settings: &Settings,
        now_ms: u64,
        key: &str,
        old: Result<&[u8], i32>,
    ) {
        let Ok(mut queues) = self.queues.try_borrow_mut() else {
            return;
        };
        let mut n = 0;
        while let Some((owner, _)) = instances.running_nth(n) {
            n += 1;
            if !instances.subscribed(owner, TAG) {
                continue;
            }
            let Some(id) = instances.id(owner) else {
                continue;
            };
            let seen = readable(settings, id, key);
            // A drop sets the reread bit inside `Pending::setting`; with no
            // queue to hold even that, the plugin has nothing to be told by.
            if let Ok(queue) = self.queue_of(&mut queues, owner) {
                let _ = queue.setting(now_ms, key, old, seen);
            }
        }
    }

    /// Announces that the state behind hint `tag` changed (a second one
    /// before it was told is the same event) to every plugin subscribed to it.
    pub(crate) fn publish_hint(&self, instances: &NativeInstances, tag: u32) {
        if !HINTS.contains(&tag) {
            return;
        }
        let Ok(mut queues) = self.queues.try_borrow_mut() else {
            return;
        };
        let mut n = 0;
        while let Some((owner, _)) = instances.running_nth(n) {
            n += 1;
            if !instances.subscribed(owner, tag) {
                continue;
            }
            if let Ok(queue) = self.queue_of(&mut queues, owner) {
                let _ = queue.hint(tag);
            }
        }
    }

    /// Announces that the registry changed in the `what` bits (OR-ed with the
    /// bits not yet told) to every plugin subscribed to `event.registry_changed`.
    pub(crate) fn publish_registry(&self, instances: &NativeInstances, what: u32) {
        if what == 0 {
            return;
        }
        let Ok(mut queues) = self.queues.try_borrow_mut() else {
            return;
        };
        let mut n = 0;
        while let Some((owner, _)) = instances.running_nth(n) {
            n += 1;
            if !instances.subscribed(owner, wire::EVENT_REGISTRY_CHANGED_TAG) {
                continue;
            }
            if let Ok(queue) = self.queue_of(&mut queues, owner) {
                queue.registry(what);
            }
        }
    }

    /// Tells `plugin` alone that `what` happened (bit 31, `dropped`): no other
    /// subscriber gets the bit, and a plugin that does not subscribe or does
    /// not run gets nothing.
    pub(crate) fn publish_registry_to(&self, instances: &NativeInstances, plugin: &Id, what: u32) {
        let Some(owner) = instances.find(plugin.as_str()) else {
            return;
        };
        if what == 0
            || !instances.subscribed(owner, wire::EVENT_REGISTRY_CHANGED_TAG)
            || instances.id(owner).is_none()
        {
            return;
        }
        let Ok(mut queues) = self.queues.try_borrow_mut() else {
            return;
        };
        if let Ok(queue) = self.queue_of(&mut queues, owner) {
            queue.registry(what);
        }
    }

    /// Forgets the queues of plugins that no longer subscribe (stopped or
    /// unloaded): what they held is dropped, never delivered.
    fn prune(&self, instances: &NativeInstances) {
        if let Ok(mut queues) = self.queues.try_borrow_mut() {
            queues.retain(|(owner, _)| Self::listens(instances, *owner));
        }
    }

    /// The next event of one plugin, detached from its queue.
    fn take(&self, owner: u64) -> Option<Ready> {
        let mut queues = self.queues.try_borrow_mut().ok()?;
        queues
            .iter_mut()
            .find(|(key, _)| *key == owner)
            .and_then(|(_, queue)| queue.take())
    }

    /// The plugin's queue lost an event that could not be told: read again.
    fn reread(&self, owner: u64, now_ms: u64) {
        if let Ok(mut queues) = self.queues.try_borrow_mut()
            && let Some((_, queue)) = queues.iter_mut().find(|(key, _)| *key == owner)
        {
            let _ = queue.setting(now_ms, "", Ok(&[]), Visibility::Readable);
        }
    }

    /// Whether `owner` has something waiting.
    pub(super) fn has_waiting(&self, owner: u64) -> bool {
        self.queues.try_borrow().is_ok_and(|queues| {
            queues
                .iter()
                .any(|(key, queue)| *key == owner && !queue.is_empty())
        })
    }

    /// The bytes the held edges charge the device budget (0 when none is held).
    #[cfg(test)]
    pub(crate) fn budget_used(&self) -> usize {
        self.budget
            .try_borrow()
            .ok()
            .and_then(|budget| budget.as_ref().map(EventBudget::used))
            .unwrap_or(0)
    }

    /// The owners with something waiting, in the pump's order.
    pub(super) fn waiting(&self) -> Vec<u64> {
        let Ok(queues) = self.queues.try_borrow() else {
            return Vec::new();
        };
        let mut owners = Vec::new();
        if owners.try_reserve_exact(queues.len()).is_err() {
            return owners;
        }
        owners.extend(
            queues
                .iter()
                .filter(|(_, queue)| !queue.is_empty())
                .map(|(owner, _)| *owner),
        );
        owners
    }
}

/// What `plugin` may be told of a change of `key`, by the rule of slot 5
/// (`registry.md`), with the same key matching (a placed key such as
/// `plugin.widget@2.token` is the setting `plugin.widget.token`): a grant and
/// the secret of another plugin are not announced; an own secret comes without
/// its old value. A key nothing defines (the plugin that defined it is being
/// uninstalled) fails closed: hidden from every other plugin, and told to its
/// own without the old value.
fn readable(settings: &Settings, plugin: &Id, key: &str) -> Visibility {
    if key.is_empty() {
        return Visibility::Readable;
    }
    let own = key.split(['.', '@']).next() == Some(plugin.as_str());
    let secret = match SettingIdentity::new(key)
        .ok()
        .and_then(|identity| identity.definition(settings))
    {
        Some(spec) => spec.extra().secret,
        None if own => return Visibility::Redacted,
        None => return Visibility::Hidden,
    };
    match visibility(plugin, key, secret, None) {
        Visibility::Redacted if !own => Visibility::Hidden,
        other => other,
    }
}

impl crate::Kernel {
    /// The record of hint `tag` into `bytes`, from the state now (`what`: the
    /// bits of `event.registry_changed`).
    fn hint_bytes(
        &self,
        tag: u32,
        what: u32,
        now_ms: u64,
        bytes: &mut Vec<u8>,
    ) -> Result<usize, i32> {
        use super::event_producers::{ChargeState, Hint};
        let off = WifiState::Off;
        let shared = &self.shared;
        let hint = match tag {
            wire::EVENT_REGISTRY_CHANGED_TAG => Hint::Registry(what),
            wire::EVENT_PLUGINS_CHANGED_TAG => Hint::Plugins,
            wire::EVENT_WIFI_CHANGED_TAG => {
                Hint::Wifi(shared.wifi.as_ref().map_or(&off, |wifi| &wifi.state))
            }
            wire::EVENT_POWER_CHANGED_TAG => Hint::Power(&shared.power),
            wire::EVENT_STATUS_CHANGED_TAG => Hint::Status,
            wire::EVENT_PREVIEW_CHANGED_TAG => Hint::Preview,
            wire::EVENT_CHARGE_CHANGED_TAG => {
                Hint::Charge(ChargeState::from_status(&shared.status, None))
            }
            // Not a hint: nothing is encoded.
            _ => return Err(ERR_INVALID),
        };
        // The longest is the power mode's id (128 bytes) and its fixed fields.
        bytes.try_reserve_exact(256).map_err(|_| ERR_NO_MEMORY)?;
        bytes.resize(256, 0);
        hint.encode(now_ms, bytes)
    }

    /// Announces a hint to the plugins subscribed to it.
    pub(crate) fn native_hint_publish(&self, tag: u32) {
        let instances = &self.shared.native_instances;
        instances.setting_events.publish_hint(instances, tag);
    }

    /// Whether a running plugin wants `event.setting_changed`.
    pub(crate) fn native_setting_wanted(&self) -> bool {
        SettingEvents::wanted(&self.shared.native_instances)
    }

    /// Announces a kept change to the subscribed plugins (see the module).
    pub(crate) fn native_setting_publish(&self, key: &str, old: Result<&[u8], i32>) {
        let instances = &self.shared.native_instances;
        instances.setting_events.publish(
            instances,
            &self.shared.settings,
            self.now.millis(),
            key,
            old,
        );
    }

    /// Tells the waiting events, a few to a plugin in a pass; `true` when
    /// some wait (another pass). Never inside a callback.
    // Its own frame: the pump keeps the services' passes apart, so that no
    // frame adds their locals (the firmware's frame limit for host code).
    #[inline(never)]
    pub(crate) fn native_setting_pump(&mut self) -> bool {
        let events = &self.shared.native_instances.setting_events;
        events.prune(&self.shared.native_instances);
        let owners = events.waiting();
        if owners.is_empty() {
            return false;
        }
        if self.shared.native_instances.depth() > 0 {
            return true;
        }
        let start = events.cursor.get().checked_rem(owners.len()).unwrap_or(0);
        events.cursor.set(start + 1);
        for step in 0..owners.len() {
            let owner = owners[(start + step) % owners.len()];
            for _ in 0..PER_OWNER {
                // What a callback staged runs before the next one is called.
                self.native_outbox_run();
                if self.shared.native_instances.outbox.full() || !self.setting_tell(owner) {
                    break;
                }
            }
        }
        self.native_outbox_run();
        !self
            .shared
            .native_instances
            .setting_events
            .waiting()
            .is_empty()
    }

    /// Tells `owner` its oldest event; `false` when it had none.
    pub(super) fn setting_tell(&self, owner: u64) -> bool {
        let instances = &self.shared.native_instances;
        let Some(ready) = instances.setting_events.take(owner) else {
            return false;
        };
        let now = self.now.millis();
        let Ok(InstanceState::Running(lease)) = instances.state(owner) else {
            return true;
        };
        let Some(id) = instances.id(owner) else {
            return true;
        };
        // A held lifecycle or reading edge was encoded when it was held.
        if let Some((tag, held)) = ready.event_bytes() {
            if instances.subscribed(owner, tag) {
                // A failing callback does not get the event again.
                let _ = self.native_event_call(owner, &lease, held);
            }
            return true;
        }
        // A lifecycle bit is encoded on the stack: no allocation (the drain at
        // suspend and stop must not need any).
        if let Ready::Core(tag) = &ready {
            let mut record = [0_u8; 16];
            // Revoked since it was queued: nothing to tell.
            if instances.subscribed(owner, *tag)
                && let Ok(len) = super::core_events::scalar(*tag, now, &mut record)
            {
                let _ = self.native_event_call(owner, &lease, &record[..len]);
            }
            return true;
        }
        let mut bytes = Vec::new();
        let encoded = match &ready {
            Ready::Hint(tag) => {
                // Revoked since it was queued: nothing to tell.
                if !instances.subscribed(owner, *tag) {
                    return true;
                }
                self.hint_bytes(*tag, 0, now, &mut bytes)
            }
            Ready::Registry(what) => {
                // Revoked since it was queued: nothing to tell.
                if !instances.subscribed(owner, wire::EVENT_REGISTRY_CHANGED_TAG) {
                    return true;
                }
                self.hint_bytes(wire::EVENT_REGISTRY_CHANGED_TAG, *what, now, &mut bytes)
            }
            _ => {
                let settings = &self.shared.settings;
                match ready.setting_record(now, |key| readable(settings, id, key)) {
                    // Revoked since it was queued: nothing to tell.
                    Ok(None) => return true,
                    Ok(Some(record)) => {
                        let sized = codec::framed_len(&record)
                            .ok()
                            .and_then(|len| bytes.try_reserve_exact(1 + len).ok());
                        sized.ok_or(ERR_NO_MEMORY).and_then(|()| {
                            bytes.resize(bytes.capacity(), 0);
                            codec::encode_batch(&[record], &mut bytes)
                        })
                    }
                    Err(status) => Err(status),
                }
            }
        };
        let Ok(len) = encoded else {
            // A lost edge is replaced by the empty key; a hint or the empty key
            // itself is dropped (a retry would spin while memory is short).
            if matches!(ready, Ready::Edge(_)) {
                instances.setting_events.reread(owner, now);
            }
            return true;
        };
        // A failing callback does not get the event again.
        let _ = self.native_event_call(owner, &lease, &bytes[..len]);
        true
    }
}

#[cfg(test)]
#[path = "setting_events_tests.rs"]
mod tests;
