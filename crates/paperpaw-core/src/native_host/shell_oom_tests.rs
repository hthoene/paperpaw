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
        log: Default::default(),
        meter: crate::meter::Meter::new(None),
        directory: None,
        stack_rows: alloc::rc::Rc::new(Default::default()),
        font_removals: Rc::new(crate::native_host::font_removals::FontRemovals::new()),
        registry_generation: alloc::rc::Rc::new(
            crate::native_host::registry_generation::RegistryGeneration::new(),
        ),
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
        log: Default::default(),
        meter: crate::meter::Meter::new(None),
        directory: None,
        stack_rows: alloc::rc::Rc::new(Default::default()),
        font_removals: Rc::new(crate::native_host::font_removals::FontRemovals::new()),
        registry_generation: alloc::rc::Rc::new(
            crate::native_host::registry_generation::RegistryGeneration::new(),
        ),
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

#[test]
fn font_removal_map_or_id_oom_invalidates_older_handles_without_panicking() {
    use crate::native_host::font_removals::FontRemovals;
    let provider = Id::parse("owned-pack").unwrap();
    for allocation in [1, 2] {
        let tracker = FontRemovals::new();
        tracker.removed(&Id::new_static("existing"), 1);
        let before = tracker.generation().unwrap();
        arm_failure(allocation);
        tracker.removed(&provider, 1);
        arm_failure(0);
        assert!(!tracker.valid("existing", before));
        assert!(!tracker.valid("owned-pack", before));
        let after = tracker.generation().unwrap();
        assert!(tracker.valid("owned-pack", after));
    }
}

#[test]
fn service_budget_refusal_never_allocates_another_x4_page() {
    use crate::abi7::core_frame::CoreFrame;
    use crate::native_host::{HostResources, service_slots, shell::DeclaredServices};
    use core::pin::pin;
    use paperpaw_native::{
        host_glue::{ERR_BUSY, ERR_NO_MEMORY, ERR_NOT_FOUND, OK, PpBytes},
        metadata::Declared,
    };
    use paperpaw_platform::Instant;
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let view = owner.try_view().unwrap();
    let scope = pin!(EnvironmentScope::new());
    // SAFETY: the view outlives the scope's callbacks.
    unsafe { scope.expire_view(&view) };
    let declared =
        DeclaredServices::try_from_iter(["core.plugin", "core.usage"].into_iter().map(|key| {
            Declared {
                key,
                major: 1,
                semantic_revision: 0,
            }
        }))
        .unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let frame = CoreFrame {
        callback: None,
        instance: None,
        env: &env,
        meter: &kernel.shared.meter,
        depth: None,
        directory: None,
        stack_generation: kernel.shared.stack_rows.generation(),
        stack_rows: &kernel.shared.stack_rows,
        font_removals: &kernel.shared.font_removals,
        registry_generation: &kernel.shared.registry_generation,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
        log: Default::default(),
        ui: None,
    };
    let query = |text: &str, out: &mut u32| {
        // SAFETY: borrowed fixture inputs and scalar output are live and disjoint.
        unsafe {
            service_slots::query(
                &scope,
                &owner,
                &declared,
                PpBytes {
                    ptr: text.as_ptr(),
                    len: text.len(),
                },
                1,
                0,
                out,
            )
        }
    };
    scope
        .as_ref()
        .scope_core(&frame, || {
            let mut first = 0;
            assert_eq!(query("core.usage", &mut first), OK);
            for (key, expected) in [
                ("core.usage", ERR_BUSY),
                ("core.plugin/missing", ERR_NOT_FOUND),
            ] {
                let mut out = 0x9876;
                arm_failure(1);
                let result = query(key, &mut out);
                let no_attempt = super::FAIL_NTH.with(|n| n.get() == 1);
                arm_failure(0);
                assert_eq!(result, expected);
                assert_eq!(out, 0x9876);
                assert!(no_attempt, "query must refuse before allocating scratch");
            }
            assert_eq!(service_slots::release(&scope, &owner, first), OK);
            let mut out = 0x9876;
            arm_failure(1);
            let result = query("core.usage", &mut out);
            arm_failure(0);
            assert_eq!(result, ERR_NO_MEMORY);
            assert_eq!(out, 0x9876);
            assert_eq!(query("core.usage", &mut out), OK);
        })
        .unwrap();
}

#[test]
fn nested_plugin_query_shares_the_x4_blob_budget_until_outer_release() {
    use crate::abi7::core_frame::CoreFrame;
    use crate::native_host::{HostResources, service_slots, shell::DeclaredServices};
    use core::pin::pin;
    use paperpaw_native::{
        host_glue::{ERR_BUSY, OK, PpBytes},
        metadata::Declared,
    };
    use paperpaw_platform::Instant;
    let pool = HostResources::try_new().unwrap();
    let outer = pool.try_owner().unwrap();
    let inner = pool.try_owner().unwrap();
    let (outer_view, inner_view) = (outer.try_view().unwrap(), inner.try_view().unwrap());
    let outer_scope = pin!(EnvironmentScope::new());
    let inner_scope = pin!(EnvironmentScope::new());
    // SAFETY: the views outlive the scopes' callbacks.
    unsafe {
        outer_scope.expire_view(&outer_view);
        inner_scope.expire_view(&inner_view);
    }
    let declared = DeclaredServices::try_from_iter(core::iter::once(Declared {
        key: "core.usage",
        major: 1,
        semantic_revision: 0,
    }))
    .unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let frame = CoreFrame {
        callback: None,
        instance: None,
        env: &env,
        meter: &kernel.shared.meter,
        depth: None,
        directory: None,
        stack_generation: kernel.shared.stack_rows.generation(),
        stack_rows: &kernel.shared.stack_rows,
        font_removals: &kernel.shared.font_removals,
        registry_generation: &kernel.shared.registry_generation,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
        log: Default::default(),
        ui: None,
    };
    let query = |scope: &EnvironmentScope,
                 owner: &crate::native_host::resources::OwnerResources,
                 out: &mut u32| {
        // SAFETY: static readable key and live, exclusive scalar output.
        unsafe {
            service_slots::query(
                scope,
                owner,
                &declared,
                PpBytes {
                    ptr: b"core.usage".as_ptr(),
                    len: 10,
                },
                1,
                0,
                out,
            )
        }
    };
    outer_scope
        .as_ref()
        .scope_core(&frame, || {
            let mut outer_blob = 0;
            assert_eq!(query(&outer_scope, &outer, &mut outer_blob), OK);
            inner_scope
                .as_ref()
                .scope_core(&frame, || {
                    let mut inner_blob = 0x9876;
                    arm_failure(1);
                    let result = query(&inner_scope, &inner, &mut inner_blob);
                    let no_attempt = super::FAIL_NTH.with(|n| n.get() == 1);
                    arm_failure(0);
                    assert_eq!(result, ERR_BUSY);
                    assert_eq!(inner_blob, 0x9876);
                    assert!(no_attempt);
                    assert_eq!(service_slots::release(&outer_scope, &outer, outer_blob), OK);
                    assert_eq!(query(&inner_scope, &inner, &mut inner_blob), OK);
                })
                .unwrap();
            assert!(
                outer
                    .with_blob(outer_blob, outer_scope.callback_id(), |_| Ok(()))
                    .is_err()
            );
        })
        .unwrap();
}
