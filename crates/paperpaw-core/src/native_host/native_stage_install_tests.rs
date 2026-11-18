//! Stage, subscription and setup-step declarations installed with a native
//! plugin's screen.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;

#[test]
fn a_native_plugins_stage_and_subscription_are_admitted_and_go_with_it() {
    reset(NORMAL);
    STAGES.with(|value| value.set(1));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let result = install(&mut kernel, 0);
    STAGES.with(|value| value.set(0));
    result.unwrap();
    let directory = kernel.shared.registry.native_declarations.as_ref().unwrap();
    let store = directory.store.borrow();
    let stage = store
        .stage("factory.insert")
        .expect("the stage is declared");
    assert_eq!(
        (stage.max_input, stage.max_reply, stage.max_records),
        (16, 16, 1)
    );
    // Its subscription to another plugin's stage is kept, and inert.
    let kept = store.stage_subscribers("other.insert").unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(
        (kept[0].plugin, kept[0].token, kept[0].owner),
        ("factory", 91, stage.owner)
    );
    drop(kept);
    drop(store);
    // The owner leaves: stage and subscription with it.
    kernel.unload_plugin(&Id::new_static("factory"));
    let directory = kernel.shared.registry.native_declarations.as_ref().unwrap();
    let store = directory.store.borrow();
    assert!(store.stage("factory.insert").is_none());
    assert!(store.stage_subscribers("other.insert").unwrap().is_empty());
}

#[test]
fn a_subscription_to_the_plugins_own_stage_refuses_the_install_and_publishes_nothing() {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    STAGES.with(|value| value.set(2));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let before = kernel.shared.registry.screens.list().iter().count();
    let result = install(&mut kernel, 0);
    STAGES.with(|value| value.set(0));
    assert!(matches!(result, Err(Error::Status(host_glue::ERR_INVALID))));
    assert_eq!(kernel.shared.registry.screens.list().iter().count(), before);
    assert_eq!(DESTROYED.with(Cell::get), 1);
    let stage = kernel
        .shared
        .registry
        .native_declarations
        .as_ref()
        .map(|directory| directory.store.borrow().stage("factory.insert"));
    assert!(stage.flatten().is_none());
}

#[test]
fn a_native_plugins_setup_steps_are_admitted_and_go_with_it() {
    reset(NORMAL);
    STAGES.with(|value| value.set(3));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let result = install(&mut kernel, 0);
    STAGES.with(|value| value.set(0));
    result.unwrap();
    let kind = crate::native_host::declaration_store::Kind::SetupStep;
    let directory = kernel.shared.registry.native_declarations.as_ref().unwrap();
    for name in ["setup", "second"] {
        assert!(
            directory
                .store
                .borrow()
                .find("factory", kind, name, &mut |_| {})
        );
    }
    assert!(
        !directory
            .store
            .borrow()
            .find("factory", kind, "screen", &mut |_| {})
    );
    kernel.unload_plugin(&Id::new_static("factory"));
    let directory = kernel.shared.registry.native_declarations.as_ref().unwrap();
    assert!(
        !directory
            .store
            .borrow()
            .find("factory", kind, "setup", &mut |_| {})
    );
}

#[test]
fn a_setup_step_named_like_the_plugins_screen_refuses_the_install_and_publishes_nothing() {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    STAGES.with(|value| value.set(4));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let before = kernel.shared.registry.screens.list().iter().count();
    let result = install(&mut kernel, 0);
    STAGES.with(|value| value.set(0));
    assert!(matches!(result, Err(Error::Status(host_glue::ERR_INVALID))));
    assert_eq!(kernel.shared.registry.screens.list().iter().count(), before);
    assert_eq!(DESTROYED.with(Cell::get), 1);
}
