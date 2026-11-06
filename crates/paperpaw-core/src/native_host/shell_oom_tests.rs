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
) -> Result<Box<FlatPlugin>, FlatError> {
    let depth = match device {
        Some(depth) => depth.try_clone().ok_or(FlatError::Allocation)?,
        None => FlatDepth::try_new()?,
    };
    let manifest = Manifest::new(Id::new_static("flat"), "Flat", Version::new(1, 0, 0));
    let plugin = FlatPlugin::new(manifest, made, depth, host)?;
    shared::try_box(plugin).ok_or(FlatError::Allocation)
}

#[test]
fn first_and_later_plugin_refuse_each_post_make_allocation_and_destroy_once() {
    reset(NORMAL);
    let host = FlatHost {
        meter: crate::meter::Meter::new(None),
    };
    for existing in [false, true] {
        let device = existing.then(|| FlatDepth::try_new().unwrap());
        let steps = if existing { 2 } else { 3 };
        for nth in 1..=steps + 1 {
            let destroyed = Rc::new(Cell::new(0));
            let made = Box::new(Made {
                scope: EnvironmentScope::new(),
                destroyed: Rc::clone(&destroyed),
            });
            // The fake make succeeded. No test setup allocation occurs in the
            // checked construction window; its destructors only update cells.
            arm_failure(nth);
            let result = construct(made, device.as_ref(), &host);
            arm_failure(0);
            assert_eq!(
                result.is_err(),
                nth <= steps,
                "existing={existing}, nth={nth}"
            );
            if let Err(error) = &result {
                assert_eq!(*error, FlatError::Allocation);
            }
            drop(result);
            assert_eq!(destroyed.get(), 1, "existing={existing}, nth={nth}");
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
    made: Made,
    passes: Cell<u8>,
    calls: Rc<Cell<u32>>,
}
impl FlatInstance for Routed {
    fn registrations(&self, visit: &mut dyn FnMut(FlatRegistration)) {
        let pass = self.passes.get();
        self.passes.set(pass + 1);
        for index in 0..2 {
            if pass > 0 && index == 1 {
                // Construction already checked both tags. Fail the actual
                // second handler allocation, after the first subscribe's Vec.
                arm_failure(1);
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
        self.refused.set(
            matches!(&result, Err(paperpaw_sdk::RegisterError::Invalid { message })
            if message == "flat handler allocation failed"),
        );
        // The first subscription still owns its handles until kernel rollback.
        assert_eq!(self.plugin.reference_counts(), (2, 2));
        result
    }
}

#[test]
fn second_handler_oom_rolls_back_first_subscription_and_restores_shared_counts() {
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
        refused: Cell::new(false),
    };
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel.register_plugin(&probe, probe.manifest());
    assert!(
        probe.refused.get(),
        "register returned the second-handler OOM"
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
