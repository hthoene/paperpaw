//! `command.set_item_state` (60) through a native callback: checked whole at
//! submit, stored after the callback, gone when the callback fails.
use super::*;
use crate::native_host::{
    commands::{WireCommand, commands_encode},
    item_state::{FAINT, HIDDEN, SWITCH},
};
use paperpaw_native::host_glue::{ERR_FULL, ERR_NOT_FOUND, ERR_UNSUPPORTED};

fn state(item: &str, flags: u32) -> WireCommand<'_> {
    WireCommand::SetItemState {
        item,
        flags,
        label: "",
        label_args: &[],
        badge: "",
        badge_args: &[],
    }
}

fn kernel(settings: usize, directory: bool) -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    const KEYS: [&str; 17] = [
        "fake.s0", "fake.s1", "fake.s2", "fake.s3", "fake.s4", "fake.s5", "fake.s6", "fake.s7",
        "fake.s8", "fake.s9", "fake.s10", "fake.s11", "fake.s12", "fake.s13", "fake.s14",
        "fake.s15", "fake.s16",
    ];
    for key in &KEYS[..settings] {
        kernel
            .shared
            .settings
            .define(paperpaw_sdk::SettingSpec::switch(
                key,
                "setting",
                false,
                paperpaw_sdk::builtin::groups::OTHER,
            ))
            .unwrap();
    }
    if directory {
        kernel.shared.registry.native_declarations = Some(
            crate::native_host::shared::Shared::try_new(
                crate::native_host::native_declarations::NativeDeclarations::default(),
            )
            .unwrap(),
        );
    }
    kernel
}

/// Submits `commands` in one batch from a callback that returns `returned`;
/// the submit status and the callback's result.
fn run(
    kernel: &mut crate::Kernel,
    commands: &[WireCommand<'_>],
    returned: i32,
) -> (i32, Result<i32, Error>) {
    reset(NORMAL);
    let hosted = fake();
    let batch = hosted
        .make_registered(hosted.begin().unwrap())
        .unwrap()
        .commit()
        .unwrap();
    let mut bytes = [0; 2048];
    let len = commands_encode(commands, &mut bytes).unwrap();
    let submitted = core::cell::Cell::new(i32::MIN);
    let result = kernel.with_native_commands(
        &batch,
        &DispatchDepth::new(),
        crate::native_host::shell::command_dispatch::CommandTarget {
            screen: None,
            hop: 0,
            result: crate::native_host::shell::command_dispatch::CommandResult::Status,
        },
        paperpaw_platform::Instant(0),
        |context, frame| {
            // SAFETY: the retained context structurally pins its environment.
            let scope = unsafe { context.map_unchecked(|value| &value.environment) };
            scope.scope_core(frame, || {
                // SAFETY: the encoded command is readable and disjoint from host state.
                submitted.set(unsafe { context.commands.submit(bytes.as_ptr(), len) });
                if returned == OK {
                    Ok(OK)
                } else {
                    Err(Error::Status(returned))
                }
            })?
        },
    );
    drop(batch);
    finish(&hosted);
    (submitted.get(), result)
}

fn pushed(kernel: &crate::Kernel, item: &str) -> Option<u32> {
    let directory = kernel.shared.registry.native_declarations.as_ref()?;
    let states = directory.items.borrow();
    states.get("fake", item).map(|p| p.flags)
}

#[test]
fn a_pushed_state_is_stored_after_the_callback_and_the_default_removes_it() {
    let mut kernel = kernel(1, true);
    let (submitted, result) = run(&mut kernel, &[state("s0", HIDDEN | FAINT)], OK);
    assert_eq!((submitted, result), (OK, Ok(OK)));
    assert_eq!(pushed(&kernel, "s0"), Some(HIDDEN | FAINT));
    let (submitted, _) = run(&mut kernel, &[state("s0", 0)], OK);
    assert_eq!(submitted, OK);
    assert_eq!(pushed(&kernel, "s0"), None, "the default state removes it");
}

#[test]
fn a_callback_that_fails_stores_nothing() {
    let mut kernel = kernel(1, true);
    let (submitted, result) = run(&mut kernel, &[state("s0", HIDDEN)], ERR_INVALID);
    assert_eq!(submitted, OK, "accepted at submit");
    assert!(result.is_err());
    assert_eq!(pushed(&kernel, "s0"), None);
}

#[test]
fn a_batch_with_a_bad_record_is_refused_whole() {
    let mut kernel = kernel(2, true);
    let refused = |kernel: &mut crate::Kernel, commands: &[WireCommand<'_>], status: i32| {
        let (submitted, _) = run(kernel, commands, OK);
        assert_eq!(submitted, status, "{commands:?}");
        assert_eq!(pushed(kernel, "s0"), None, "nothing of it is kept");
        assert_eq!(pushed(kernel, "s1"), None);
    };
    // An item that is none of the plugin's own.
    refused(
        &mut kernel,
        &[state("s0", HIDDEN), state("nobody", HIDDEN)],
        ERR_NOT_FOUND,
    );
    // Two records of one item in a batch.
    refused(
        &mut kernel,
        &[state("s0", HIDDEN), state("s0", FAINT)],
        ERR_INVALID,
    );
    // A switch on a setting: a Boolean setting shows its own value.
    refused(&mut kernel, &[state("s1", SWITCH)], ERR_INVALID);
    // A label key no catalog has.
    let unknown = WireCommand::SetItemState {
        item: "s0",
        flags: 0,
        label: "no.such.key",
        label_args: &[],
        badge: "",
        badge_args: &[],
    };
    refused(&mut kernel, &[unknown], ERR_NOT_FOUND);
}

#[test]
fn a_plugin_holds_16_states_and_the_net_result_decides() {
    let mut kernel = kernel(17, true);
    let names: alloc::vec::Vec<alloc::string::String> =
        (0..17).map(|n| alloc::format!("s{n}")).collect();
    let all: alloc::vec::Vec<_> = names.iter().map(|n| state(n, HIDDEN)).collect();
    // A batch holds 8 commands: two of them fill the plugin's 16.
    let (submitted, _) = run(&mut kernel, &all[..8], OK);
    assert_eq!(submitted, OK);
    let (submitted, _) = run(&mut kernel, &all[8..16], OK);
    assert_eq!(submitted, OK);
    assert_eq!(pushed(&kernel, "s15"), Some(HIDDEN));
    // Full: a 17th is refused, a change of one is not, and a batch that
    // clears one and adds another is.
    let (submitted, _) = run(&mut kernel, &all[16..], OK);
    assert_eq!(submitted, ERR_FULL);
    let (submitted, _) = run(&mut kernel, &[state("s0", FAINT)], OK);
    assert_eq!(submitted, OK);
    let (submitted, _) = run(&mut kernel, &[state("s1", 0), state("s16", HIDDEN)], OK);
    assert_eq!(submitted, OK);
    assert_eq!(
        (pushed(&kernel, "s1"), pushed(&kernel, "s16")),
        (None, Some(HIDDEN))
    );
}

#[test]
fn a_host_without_the_store_refuses_the_record_as_unsupported() {
    let mut kernel = kernel(1, false);
    let (submitted, _) = run(&mut kernel, &[state("s0", HIDDEN)], OK);
    assert_eq!(submitted, ERR_UNSUPPORTED);
}
