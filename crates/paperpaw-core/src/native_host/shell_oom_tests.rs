//! Every actual Core adapter allocation after a fake successful make can fail.
use super::{NORMAL, arm_failure, reset};
use crate::native_host::environment::EnvironmentScope;
use crate::native_host::{
    DispatchDepth, Error, FlatHost,
    flat::{DropReason, FlatDepth, FlatError, FlatInstance, FlatPlugin, FlatRegistration},
    shared,
};
use alloc::{boxed::Box, rc::Rc};
use core::{cell::Cell, pin::Pin};
use paperpaw_sdk::{Id, Manifest, Version};

struct Made {
    scope: EnvironmentScope,
    destroyed: Rc<Cell<u32>>,
}
impl Drop for Made {
    fn drop(&mut self) {
        self.destroyed.set(self.destroyed.get() + 1);
    }
}
impl FlatInstance for Made {
    fn registrations(&self, _: &mut dyn FnMut(FlatRegistration)) {}
    fn environment(&self) -> Pin<&EnvironmentScope> {
        // SAFETY: the fake instance is boxed before this view can be requested.
        unsafe { Pin::new_unchecked(&self.scope) }
    }
    fn dispatch_event(
        &self,
        _: usize,
        _: &[u8],
        _: &mut [u8],
        _: &DispatchDepth,
    ) -> Result<i32, Error> {
        Ok(0)
    }
    fn event_dropped(&self, _: u32, _: DropReason) {
        panic!("unexpected callback");
    }
}

fn construct(
    made: Box<dyn FlatInstance>,
    device: Option<&FlatDepth>,
    host: &FlatHost,
    id: &Id,
) -> Result<Box<FlatPlugin>, FlatError> {
    let depth = match device {
        Some(depth) => depth.try_clone().ok_or(FlatError::Allocation)?,
        None => FlatDepth::try_new()?,
    };
    let manifest = FlatPlugin::try_manifest(id, id.as_str(), Version::new(1, 0, 0))?;
    let plugin = FlatPlugin::new(manifest, made, depth, host)?;
    shared::try_box(plugin).ok_or(FlatError::Allocation)
}

#[test]
fn first_and_later_plugin_refuse_each_post_make_allocation_and_destroy_once() {
    reset(NORMAL);
    let host = FlatHost {
        meter: crate::meter::Meter::new(None),
    };
    for owned in [false, true] {
        let text = alloc::string::String::from("a").repeat(96);
        let id = if owned {
            Id::parse(&text).unwrap()
        } else {
            Id::new_static("flat")
        };
        for existing in [false, true] {
            let device = existing.then(|| FlatDepth::try_new().unwrap());
            let steps = if existing { 3 } else { 4 } + u32::from(owned);
            for nth in 1..=steps + 1 {
                let destroyed = Rc::new(Cell::new(0));
                let made = Box::new(Made {
                    scope: EnvironmentScope::new(),
                    destroyed: Rc::clone(&destroyed),
                });
                // Fake make succeeded. ID setup and tracking cells already exist.
                // Refuse each actual depth/id/name/shared/final-box allocation.
                arm_failure(nth);
                let result = construct(made, device.as_ref(), &host, &id);
                arm_failure(0);
                assert_eq!(
                    result.is_err(),
                    nth <= steps,
                    "owned={owned}, existing={existing}, nth={nth}"
                );
                if let Err(error) = &result {
                    assert_eq!(*error, FlatError::Allocation);
                }
                drop(result);
                assert_eq!(
                    destroyed.get(),
                    1,
                    "owned={owned}, existing={existing}, nth={nth}"
                );
                assert_eq!(id.as_str(), if owned { text.as_str() } else { "flat" });
            }
        }
    }
}

#[test]
fn private_shared_refusal_drops_input_once_and_cloning_does_not_allocate() {
    struct Probe(Rc<Cell<u32>>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let destroyed = Rc::new(Cell::new(0));
    let input = Probe(Rc::clone(&destroyed));
    arm_failure(1);
    let result = shared::Shared::try_new(input);
    arm_failure(0);
    assert!(result.is_err());
    assert_eq!(destroyed.get(), 1);
    let value = shared::Shared::try_new(17).unwrap();
    arm_failure(1);
    let copy = value.try_clone();
    arm_failure(0);
    assert!(copy.is_some());
    assert_eq!(*copy.unwrap(), 17);
}

struct Routed {
    registry_oom: bool,
    made: Made,
    passes: Cell<u8>,
    calls: Rc<Cell<u32>>,
}
impl FlatInstance for Routed {
    fn registrations(&self, visit: &mut dyn FnMut(FlatRegistration)) {
        let pass = self.passes.get();
        self.passes.set(pass + 1);
        for index in 0..2 {
            let failure_index = if self.registry_oom { 0 } else { 1 };
            if pass > 0 && index == failure_index {
                // Handler allocation comes first, then registrar capacity.
                // Both event ids are static and need no allocation.
                arm_failure(if self.registry_oom { 2 } else { 1 });
            }
            visit(FlatRegistration::Event {
                event_tag: 32 + index as u32,
                index,
            });
        }
    }
    fn environment(&self) -> Pin<&EnvironmentScope> {
        self.made.environment()
    }
    fn dispatch_event(
        &self,
        _: usize,
        _: &[u8],
        _: &mut [u8],
        _: &DispatchDepth,
    ) -> Result<i32, Error> {
        self.calls.set(self.calls.get() + 1);
        Ok(0)
    }
    fn event_dropped(&self, _: u32, _: DropReason) {
        panic!("unexpected delivery");
    }
}

struct RegistrationProbe<'a> {
    plugin: &'a FlatPlugin,
    registry_oom: bool,
    refused: Cell<bool>,
}
impl paperpaw_sdk::Plugin for RegistrationProbe<'_> {
    fn manifest(&self) -> Manifest {
        self.plugin.manifest()
    }
    fn register(
        &self,
        registrar: &mut paperpaw_sdk::Registrar,
    ) -> Result<(), paperpaw_sdk::RegisterError> {
        let result = self.plugin.register(registrar);
        arm_failure(0);
        let expected = if self.registry_oom {
            "contribution allocation failed"
        } else {
            "flat handler allocation failed"
        };
        self.refused.set(matches!(&result,
            Err(paperpaw_sdk::RegisterError::Invalid { message }) if message == expected));
        // A handler refused by registrar drops immediately; an admitted first
        // subscription retains handles until the kernel rolls back the stage.
        let count = if self.registry_oom { 1 } else { 2 };
        assert_eq!(self.plugin.reference_counts(), (count, count));
        result
    }
}

#[test]
fn second_handler_oom_rolls_back_first_subscription_and_restores_shared_counts() {
    registration_rollback(false);
}

#[test]
fn registrar_capacity_oom_refuses_and_releases_the_handler_without_abort() {
    registration_rollback(true);
}

fn registration_rollback(registry_oom: bool) {
    use paperpaw_sdk::{Params, Plugin, PluginState, builtin::events};
    reset(NORMAL);
    let host = FlatHost {
        meter: crate::meter::Meter::new(None),
    };
    let destroyed = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let plugin = FlatPlugin::new(
        Manifest::new(Id::new_static("flat"), "Flat", Version::new(1, 0, 0)),
        Box::new(Routed {
            registry_oom,
            made: Made {
                scope: EnvironmentScope::new(),
                destroyed: Rc::clone(&destroyed),
            },
            passes: Cell::new(0),
            calls: Rc::clone(&calls),
        }),
        FlatDepth::try_new().unwrap(),
        &host,
    )
    .unwrap();
    let probe = RegistrationProbe {
        plugin: &plugin,
        registry_oom,
        refused: Cell::new(false),
    };
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel.register_plugin(&probe, probe.manifest());
    assert!(
        probe.refused.get(),
        "register returned the precise allocation refusal"
    );
    assert_eq!(
        kernel.plugins().get("flat").unwrap().state,
        PluginState::Failed
    );
    assert_eq!(
        plugin.reference_counts(),
        (1, 1),
        "kernel rollback released both handles"
    );
    kernel.emit(events::STARTED.id(), &Params::new());
    kernel.emit(events::RESUME.id(), &Params::new());
    assert_eq!(calls.get(), 0, "neither registration was published");
    assert_eq!(destroyed.get(), 0);
    drop(plugin);
    assert_eq!(destroyed.get(), 1);
    drop(kernel);
    assert_eq!(destroyed.get(), 1);
}

#[test]
fn resource_slot_growth_oom_creates_no_handle_and_can_retry() {
    use crate::native_host::{
        HostResources,
        resources::{FontKey, FontRole},
    };
    use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_STATE};
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    arm_failure(1);
    let refused = owner.insert_font(FontKey::Role(FontRole::Ui));
    arm_failure(0);
    assert_eq!(refused, Err(ERR_NO_MEMORY));
    assert_eq!(owner.with_font(1 << 10, |_| Ok(())), Err(ERR_STATE));
    let handle = owner.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
    assert_eq!(handle, 1 << 10);
    owner.release_all().unwrap();
    assert_eq!(owner.with_font(handle, |_| Ok(())), Err(ERR_STATE));
}
