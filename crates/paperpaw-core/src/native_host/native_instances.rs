//! One retained instance registry and a deduplicated outer-callback start queue.
//! Registrations retain leases from this registry, never a second plugin owner.
use super::{
    native_callbacks::{CallbackClock, CallbackRuntime, NativeCallbackEpoch},
    native_screen::Dispatch,
    shared::{self, Shared},
};
use alloc::{boxed::Box, vec::Vec};
use core::cell::Cell;
use paperpaw_native::host_glue::{ERR_CANCELLED, ERR_INVALID, ERR_NO_MEMORY, ERR_STATE};
use paperpaw_sdk::Id;

pub(crate) trait NativeInstance: Dispatch {
    fn running(&self) -> bool;
    fn stop(&self);
}

pub(crate) struct NativeLease(Shared<Box<dyn NativeInstance>>);

impl NativeLease {
    pub(crate) fn try_new(instance: impl NativeInstance + 'static) -> Result<Self, i32> {
        let instance: Box<dyn NativeInstance> = shared::try_box(instance).ok_or(ERR_NO_MEMORY)?;
        Shared::try_new(instance)
            .map(Self)
            .map_err(|_| ERR_NO_MEMORY)
    }
    pub(crate) fn try_clone(&self) -> Result<Self, i32> {
        self.0.try_clone().map(Self).ok_or(ERR_NO_MEMORY)
    }
    pub(crate) fn live(&self) -> Result<&dyn NativeInstance, i32> {
        if self.0.running() {
            Ok(&**self.0)
        } else {
            Err(ERR_CANCELLED)
        }
    }
    pub(crate) fn stop(&self) {
        self.0.stop();
    }
}

impl Dispatch for NativeLease {
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
        self.0.dropped(token, status);
    }
}

struct Entry {
    id: Option<Id>,
    instance: Option<NativeLease>,
    enabled: bool,
    start: Cell<bool>,
}

#[derive(Default)]
pub(crate) struct NativeInstances {
    entries: Vec<Entry>,
    callbacks: CallbackClock,
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

impl NativeInstances {
    /// Only fallible storage growth precedes publication; indices never move.
    pub(crate) fn insert(
        &mut self,
        id: Id,
        instance: Option<NativeLease>,
        enabled: bool,
    ) -> Result<usize, i32> {
        if self
            .entries
            .iter()
            .any(|entry| entry.id.as_ref() == Some(&id))
        {
            return Err(ERR_INVALID);
        }
        self.entries.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        let key = self.entries.len();
        self.entries.push(Entry {
            id: Some(id),
            instance,
            enabled,
            start: Cell::new(false),
        });
        Ok(key)
    }

    pub(crate) fn id(&self, key: usize) -> Option<&Id> {
        self.entries.get(key).and_then(|entry| entry.id.as_ref())
    }

    pub(crate) fn find(&self, id: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.id.as_ref().is_some_and(|value| value.as_str() == id))
    }

    pub(crate) fn state(&self, key: usize) -> Result<InstanceState, i32> {
        let entry = self.entries.get(key).ok_or(ERR_STATE)?;
        if !entry.enabled {
            return Ok(InstanceState::Stopped);
        }
        match &entry.instance {
            Some(instance) if instance.live().is_ok() => {
                Ok(InstanceState::Running(instance.try_clone()?))
            }
            Some(_) => Ok(InstanceState::Stopped),
            None => Ok(InstanceState::Lazy),
        }
    }

    /// A frozen run may retain the returned lease after this entry stops.
    /// Revocation makes that old lease CANCELLED without invoking foreign code.
    pub(crate) fn stop(&mut self, key: usize) -> Result<(), i32> {
        let entry = self.entries.get_mut(key).ok_or(ERR_STATE)?;
        entry.enabled = false;
        entry.start.set(false);
        if let Some(instance) = entry.instance.take() {
            instance.stop();
        }
        Ok(())
    }

    pub(crate) fn request_start(&self, key: usize) -> Result<(), i32> {
        let entry = self.entries.get(key).ok_or(ERR_STATE)?;
        if entry.enabled && entry.instance.is_none() {
            entry.start.set(true);
        }
        Ok(())
    }

    /// Zero allocation, deduplicated by entry; never flush inside a callback.
    pub(crate) fn take_start(&self) -> Option<usize> {
        if self.callbacks.depth() != 0 {
            return None;
        }
        self.entries
            .iter()
            .position(|entry| entry.enabled && entry.start.replace(false))
    }

    pub(crate) fn install(&mut self, key: usize, instance: NativeLease) -> Result<(), i32> {
        if self.callbacks.depth() != 0 {
            return Err(ERR_STATE);
        }
        let entry = self.entries.get_mut(key).ok_or(ERR_STATE)?;
        if !entry.enabled || entry.instance.is_some() {
            return Err(ERR_STATE);
        }
        entry.instance = Some(instance);
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
    pub(crate) fn identity(&mut self, key: usize) -> Result<NativeIdentity, i32> {
        if !self
            .entries
            .get(key)
            .is_some_and(|entry| entry.enabled && entry.id.is_some())
        {
            return Err(ERR_STATE);
        }
        let owner = u64::try_from(key).map_err(|_| paperpaw_native::host_glue::ERR_FULL)?;
        Ok(NativeIdentity {
            owner,
            callbacks: self.callbacks.runtime()?,
        })
    }

    /// Failed admission leaves a tombstone: neither the slot nor its identity is reused.
    pub(crate) fn abandon(&mut self, key: usize) -> Result<(), i32> {
        self.stop(key)?;
        self.entries.get_mut(key).ok_or(ERR_STATE)?.id = None;
        Ok(())
    }

    pub(crate) fn depth(&self) -> u8 {
        self.callbacks.depth()
    }
}

pub(crate) struct NativeIdentity {
    owner: u64,
    callbacks: CallbackRuntime,
}
impl NativeIdentity {
    pub(crate) fn owner(&self) -> u64 {
        self.owner
    }
    pub(crate) fn callback(&self) -> Result<NativeCallbackEpoch, i32> {
        self.callbacks.enter()
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
        .insert(Id::try_parse(owner).unwrap(), None, true)
        .unwrap();
    registry.identity(key).unwrap()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
    use super::*;
    use alloc::rc::Rc;
    use paperpaw_sdk::Env;
    struct Fake {
        active: Cell<bool>,
        drops: Rc<Cell<u32>>,
    }
    impl Drop for Fake {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    impl NativeInstance for Fake {
        fn running(&self) -> bool {
            self.active.get()
        }
        fn stop(&self) {
            self.active.set(false);
        }
    }
    impl Dispatch for Fake {
        fn event(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
            panic!("not called after stop")
        }
        fn input(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
            panic!("not called after stop")
        }
        fn draw(
            &self,
            _: u32,
            _: &super::super::registrations::DrawFrame<'_>,
            _: &Env<'_>,
            _: &mut paperpaw_graphics::Canvas<'_>,
        ) -> Result<i32, i32> {
            panic!("not called after stop")
        }
        fn dropped(&self, _: u32, _: i32) {}
    }

    #[test]
    fn frozen_lease_keeps_instance_alive_but_cannot_call_after_registry_stop() {
        let drops = Rc::new(Cell::new(0));
        let instance = NativeLease::try_new(Fake {
            active: Cell::new(true),
            drops: drops.clone(),
        })
        .unwrap();
        let mut registry = NativeInstances::default();
        let key = registry
            .insert(Id::new_static("probe"), Some(instance), true)
            .unwrap();
        let InstanceState::Running(lease) = registry.state(key).unwrap() else {
            panic!("running");
        };
        registry.stop(key).unwrap();
        assert_eq!(lease.live().err(), Some(ERR_CANCELLED));
        let kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        let env = kernel.shared.env(paperpaw_platform::Instant(0));
        assert_eq!(lease.event(1, &[], &env), Err(ERR_CANCELLED));
        assert_eq!(drops.get(), 0, "frozen lease retains the old owner");
        drop(lease);
        assert_eq!(drops.get(), 1);
        assert!(matches!(registry.state(key), Ok(InstanceState::Stopped)));
    }

    #[test]
    fn lazy_start_deduplicates_and_flushes_only_after_outermost_epoch() {
        let mut registry = NativeInstances::default();
        let key = registry.insert(Id::new_static("lazy"), None, true).unwrap();
        let disabled = registry
            .insert(Id::new_static("disabled"), None, false)
            .unwrap();
        assert!(matches!(registry.state(key), Ok(InstanceState::Lazy)));
        let outer = registry.callback().unwrap();
        let epoch = outer.epoch();
        let inner = registry.callback().unwrap();
        assert_eq!(inner.epoch(), epoch);
        assert_eq!(registry.depth(), 2);
        registry.request_start(key).unwrap();
        registry.request_start(key).unwrap();
        registry.request_start(disabled).unwrap();
        assert_eq!(registry.take_start(), None);
        drop(inner);
        assert_eq!(registry.take_start(), None);
        drop(outer);
        assert_eq!(registry.take_start(), Some(key));
        assert_eq!(registry.take_start(), None);
        let next = registry.callback().unwrap();
        assert!(next.epoch() > epoch);
    }
    #[test]
    fn abandoned_admission_retries_with_a_new_owner_key() {
        let mut registry = NativeInstances::default();
        let first = registry
            .insert(Id::new_static("retry"), None, true)
            .unwrap();
        let identity = registry.identity(first).unwrap();
        let epoch = identity.callback().unwrap().epoch();
        registry.abandon(first).unwrap();
        assert!(registry.id(first).is_none());
        assert_eq!(registry.find("retry"), None);
        let second = registry
            .insert(Id::new_static("retry"), None, true)
            .unwrap();
        assert!(second > first);
        let next = registry.identity(second).unwrap();
        assert_ne!(identity.owner(), next.owner());
        assert!(next.callback().unwrap().epoch() > epoch);
    }
}
