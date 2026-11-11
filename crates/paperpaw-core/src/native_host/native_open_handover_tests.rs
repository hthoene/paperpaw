//! `command.open` (1547): a staged open applies in its mode, names the
//! hand-over it claimed, and the instance it shows gets it: the selected text
//! of the reader's translate arrives as the hand-over's host text, never as a
//! param (`docs/abi/ui.md` 1.1).
use super::*;
use crate::native_host::{
    commands::{CommandScope, CommandStage, WireCommand, commands_encode},
    shared::Shared,
    ui_service::UiServices,
    ui_store::{Limits, Param, Stamp, UiStore},
};
use alloc::borrow::ToOwned;
use alloc::string::String;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NOT_FOUND, ERR_STATE};
use paperpaw_platform::Instant;

/// A kernel with the factory plugin installed (its screen is offered for
/// `core.translate`), the pool wired to a hand-over store.
fn wired() -> (crate::Kernel, Shared<UiStore>) {
    let store = Shared::try_new(UiStore::new(Limits::ELSEWHERE)).unwrap();
    let pool = crate::native_host::HostResources::try_new().unwrap();
    pool.hosts()
        .wire_ui(UiServices::new(store.try_clone().unwrap(), false))
        .unwrap();
    POOL.with(|slot| *slot.borrow_mut() = Some(pool));
    reset(NORMAL);
    OPENED.with(|opened| opened.borrow_mut().clear());
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    install_resources(&mut kernel, 8, None).unwrap();
    (kernel, store)
}

fn stamp() -> Stamp {
    Stamp::new(7, 1).unwrap()
}

/// Submits `commands` as the factory plugin's callback and drains what it
/// staged; the status of the submit.
fn submit(kernel: &mut crate::Kernel, store: &UiStore, commands: &[WireCommand<'_>]) -> i32 {
    let owner = Id::new_static("factory");
    // The callback of the screen on top.
    let caller = kernel
        .stack
        .last()
        .map(|active| paperpaw_sdk::ScreenKey::new(active.serial));
    let (status, staged, redraw, opens) = {
        let env = kernel.shared.env(Instant(0));
        let mut stage = CommandStage::new(&owner, env.settings(), None, 8, 4096)
            .with_ui(&env, caller, 0, 8)
            .with_handovers(store, stamp());
        let scope = core::pin::pin!(CommandScope::new());
        let mut bytes = [0; 512];
        let len = commands_encode(commands, &mut bytes).unwrap();
        let status = scope
            .as_ref()
            .scope(&mut stage, || {
                // SAFETY: the initialized input outlives the synchronous call.
                unsafe { scope.submit(bytes.as_ptr(), len) }
            })
            .unwrap();
        let (staged, redraw, _, opens) = stage.into_ui_output_with_opens();
        (status, staged, redraw, opens)
    };
    kernel.drain_native_opens(&owner, staged, redraw, opens);
    // The callback returned: what no open claimed is freed.
    store.end_callback(stamp()).unwrap();
    status
}

fn open(
    target: &'static str,
    mode: u32,
    slot: &'static str,
    handover: u32,
) -> WireCommand<'static> {
    WireCommand::Open {
        target,
        mode,
        slot,
        handover,
    }
}

fn made(store: &UiStore, params: &[(&str, &str)], text: &[u8]) -> u32 {
    let params: Vec<_> = params
        .iter()
        .map(|&(key, value)| Param { key, value })
        .collect();
    let id = store.handover_make(stamp(), &params).unwrap();
    if !text.is_empty() {
        store.text_write(stamp(), id, 0, text, true).unwrap();
    }
    id
}

fn read(store: &UiStore, instance: u32, handover: u32) -> String {
    let mut out = [0; 64];
    let (total, written) = store.text_read(instance, handover, 0, &mut out).unwrap();
    assert_eq!(total, written);
    String::from_utf8(out[..written].to_vec()).unwrap()
}

#[test]
fn the_selected_text_arrives_as_host_text_not_as_a_param() {
    let (mut kernel, store) = wired();
    // The reader is below (the root is never closed).
    let base = made(&store, &[("page", "1")], b"");
    assert_eq!(
        submit(&mut kernel, &store, &[open("factory.screen", 0, "", base)]),
        0
    );
    let text = "selected words, ünïcode";
    let id = made(&store, &[("language", "de")], text.as_bytes());
    assert_eq!(
        submit(&mut kernel, &store, &[open("core.translate", 1, "", id)]),
        0
    );
    let top = kernel.stack.last().unwrap();
    assert_eq!(top.id.as_str(), "factory.screen");
    let instance = top.serial;
    assert_eq!(
        OPENED.with(|opened| opened.borrow().clone()),
        [
            (kernel.stack[0].serial, base, 1, 0),
            (instance, id, 1, text.len() as u32)
        ]
    );
    assert_eq!(read(&store, instance, id), text);
    let mut params = [0; 128];
    let used = store.params(instance, id, &mut params).unwrap();
    let mut seen = Vec::new();
    let mut rest = &params[..used];
    while !rest.is_empty() {
        let (record, len) = crate::native_host::registrations::record_decode_prefix(rest).unwrap();
        let crate::native_host::registrations::Record::ServiceUiParam(param) = record else {
            panic!("a param record");
        };
        seen.push((param.key.to_owned(), param.value.to_owned()));
        rest = &rest[len..];
    }
    assert_eq!(seen, [("language".to_owned(), "de".to_owned())]);
    // It owns the hand-over now: nothing is left claimed, and closing the
    // instance frees it.
    assert_eq!(store.len(), 2);
    kernel.apply(Command::Close(paperpaw_sdk::ScreenKey::new(instance)));
    kernel.drain_deferred_ui_closes();
    assert_eq!(store.len(), 1);
}

#[test]
fn a_second_open_of_the_open_slot_passes_the_new_hand_over_and_frees_the_old() {
    let (mut kernel, store) = wired();
    let first = made(&store, &[("language", "de")], b"one");
    assert_eq!(
        submit(&mut kernel, &store, &[open("core.translate", 1, "", first)]),
        0
    );
    let instance = kernel.stack.last().unwrap().serial;
    let second = made(&store, &[("language", "de")], b"two words");
    assert_eq!(
        submit(
            &mut kernel,
            &store,
            &[open("core.translate", 1, "", second)]
        ),
        0
    );
    // The same params, a new text: the same instance, told again.
    assert_eq!(kernel.stack.len(), 1);
    assert_eq!(kernel.stack[0].serial, instance);
    assert_eq!(
        OPENED.with(|opened| opened.borrow().clone()),
        [(instance, first, 1, 3), (instance, second, 1, 9)]
    );
    assert_eq!(read(&store, instance, second), "two words");
    assert_eq!(store.len(), 1);
    // An open of none has other params than the instance has (it compares
    // them): opened anew in place of it, and its hand-over goes with it.
    assert_eq!(
        submit(&mut kernel, &store, &[open("core.translate", 1, "", 0)]),
        0
    );
    let fresh = kernel.stack.last().unwrap().serial;
    assert_ne!(fresh, instance);
    assert_eq!(
        OPENED.with(|opened| opened.borrow().last().copied()),
        Some((fresh, 0, 0, 0))
    );
    assert_eq!(store.len(), 0);
    // Another of none is the same params: the same instance, told again.
    assert_eq!(
        submit(&mut kernel, &store, &[open("core.translate", 1, "", 0)]),
        0
    );
    assert_eq!(kernel.stack.last().unwrap().serial, fresh);
    assert_eq!(
        OPENED.with(|opened| opened.borrow().last().copied()),
        Some((fresh, 0, 0, 0))
    );
    assert_eq!(OPENED.with(|opened| opened.borrow().len()), 4);
}

#[test]
fn the_other_modes_apply_and_an_open_that_does_not_apply_frees_its_claim() {
    let (mut kernel, store) = wired();
    // Mode 0: the screen by its id; mode 3: in a slot.
    let by_id = made(&store, &[("page", "1")], b"");
    assert_eq!(
        submit(&mut kernel, &store, &[open("factory.screen", 0, "", by_id)]),
        0
    );
    assert_eq!(kernel.stack.len(), 1);
    let in_slot = made(&store, &[("page", "2")], b"");
    assert_eq!(
        submit(
            &mut kernel,
            &store,
            &[open("factory.screen", 3, "core.lookup", in_slot)]
        ),
        0
    );
    assert!(
        kernel
            .stack
            .iter()
            .any(|s| s.slot.as_ref().is_some_and(|s| s.as_str() == "core.lookup"))
    );
    // Mode 2 replaces the submitter's own top instance, without showing the
    // one below in between.
    let (before, old_top) = (kernel.stack.len(), kernel.stack.last().unwrap().serial);
    let replaced = made(&store, &[("page", "3")], b"");
    assert_eq!(
        submit(
            &mut kernel,
            &store,
            &[open("factory.screen", 2, "", replaced)]
        ),
        0
    );
    assert_eq!(kernel.stack.len(), before);
    let top = kernel.stack.last().unwrap().serial;
    assert_ne!(top, old_top);
    assert_eq!(
        OPENED.with(|opened| opened.borrow().last().copied()),
        Some((top, replaced, 1, 0))
    );
    // Every hand-over is owned by a screen still open, none stays claimed.
    assert_eq!(store.len(), kernel.stack.len());
}

#[test]
fn the_form_of_an_open_and_its_hand_over_is_checked_before_anything_is_claimed() {
    let (mut kernel, store) = wired();
    let id = made(&store, &[], b"text");
    // Mode out of range, a slot where mode 3 is not, none where it is.
    for bad in [
        open("core.translate", 4, "", id),
        open("core.translate", 1, "core.lookup", id),
        open("factory.screen", 3, "", id),
    ] {
        assert_eq!(submit(&mut kernel, &store, &[bad]), ERR_INVALID);
    }
    assert_eq!(
        submit(&mut kernel, &store, &[open("nobody.screen", 0, "", 0)]),
        ERR_NOT_FOUND
    );
    assert_eq!(
        submit(&mut kernel, &store, &[open("core.nothing", 1, "", 0)]),
        ERR_NOT_FOUND
    );
    // A hand-over nobody made, one named twice, one of another plugin.
    assert_eq!(
        submit(&mut kernel, &store, &[open("core.translate", 1, "", 999)]),
        ERR_INVALID
    );
    let id = made(&store, &[], b"");
    let twice = [
        open("core.translate", 1, "", id),
        open("factory.screen", 0, "", id),
    ];
    assert_eq!(submit(&mut kernel, &store, &twice), ERR_INVALID);
    let foreign = store.handover_make(Stamp::new(8, 1).unwrap(), &[]).unwrap();
    assert_eq!(
        submit(
            &mut kernel,
            &store,
            &[open("core.translate", 1, "", foreign)]
        ),
        ERR_INVALID
    );
    // A begun text that is not committed.
    let begun = store.handover_make(stamp(), &[]).unwrap();
    store.text_write(stamp(), begun, 0, b"half", false).unwrap();
    assert_eq!(
        submit(&mut kernel, &store, &[open("core.translate", 1, "", begun)]),
        ERR_STATE
    );
    assert!(kernel.stack.is_empty());
    assert!(OPENED.with(|opened| opened.borrow().is_empty()));
}

#[test]
fn a_stage_dropped_before_its_drain_frees_what_its_submits_claimed() {
    let (kernel, store) = wired();
    let owner = Id::new_static("factory");
    let id = made(&store, &[], b"text");
    {
        let env = kernel.shared.env(Instant(0));
        let mut stage = CommandStage::new(&owner, env.settings(), None, 8, 4096)
            .with_ui(&env, None, 0, 8)
            .with_handovers(&store, stamp());
        let scope = core::pin::pin!(CommandScope::new());
        let mut bytes = [0; 256];
        let len = commands_encode(&[open("core.translate", 1, "", id)], &mut bytes).unwrap();
        let status = scope
            .as_ref()
            // SAFETY: the initialized input outlives the synchronous call.
            .scope(&mut stage, || unsafe { scope.submit(bytes.as_ptr(), len) })
            .unwrap();
        assert_eq!(status, 0);
        // Claimed: the callback's own end frees nothing of it.
        store.end_callback(stamp()).unwrap();
        assert_eq!(store.len(), 1);
    }
    // The callback returned an error: its stage is dropped, the claim rolled back.
    assert_eq!(store.len(), 0);
}

/// A plugin of the program offering `core.translate` first, with a screen of
/// its own that is not native.
struct Legacy;
impl paperpaw_sdk::Plugin for Legacy {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            Id::new_static("legacy"),
            "Legacy",
            paperpaw_sdk::Version::new(1, 0, 0),
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }
    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        struct Empty;
        impl paperpaw_sdk::Screen for Empty {
            fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}
            fn on_action(
                &mut self,
                _: &Id,
                _: &mut paperpaw_sdk::Cx<'_, '_>,
            ) -> paperpaw_sdk::Handled {
                paperpaw_sdk::Handled::No
            }
        }
        r.add_screen(paperpaw_sdk::ScreenSpec::new(
            Id::new_static("legacy.screen"),
            "legacy.title",
            Box::new(|_, _| Box::new(Empty)),
        ))?;
        r.bind_slot(
            Id::new_static("core.translate"),
            Id::new_static("legacy.screen"),
            paperpaw_sdk::Priority(100),
        )
    }
}

#[test]
fn a_screen_that_is_not_native_keeps_its_params_when_a_native_open_shows_its_slot() {
    let (_, store) = wired();
    reset(NORMAL);
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Legacy)
        .build()
        .unwrap();
    install_resources(&mut kernel, 8, None).unwrap();
    // The slot is the legacy screen's (priority 100): below it a base.
    kernel.apply(Command::OpenScreen(
        Id::new_static("factory.screen"),
        Params::new(),
    ));
    kernel.apply(Command::OpenSlot(
        Id::new_static("core.translate"),
        Params::new().with("page", "1"),
    ));
    assert_eq!(kernel.stack.last().unwrap().id.as_str(), "legacy.screen");
    let id = made(&store, &[], b"");
    assert_eq!(
        submit(&mut kernel, &store, &[open("core.translate", 1, "", id)]),
        0
    );
    // Its params stay; no hand-over was given to it and none is left claimed.
    assert_eq!(kernel.stack.last().unwrap().params.get("page"), Some("1"));
    assert_eq!(store.len(), 0);
    // A hand-over with params of its own: the screen is no native one and
    // owns none, so it stays as it is, not closed and opened again without
    // the params it has.
    let legacy = kernel.stack.last().unwrap().serial;
    let id = made(&store, &[("x", "1")], b"text");
    assert_eq!(
        submit(&mut kernel, &store, &[open("core.translate", 1, "", id)]),
        0
    );
    let top = kernel.stack.last().unwrap();
    assert_eq!((top.serial, top.params.get("page")), (legacy, Some("1")));
    assert_eq!(store.len(), 0);
}

#[test]
fn a_replace_applies_only_to_the_screen_that_was_on_top_when_it_was_submitted() {
    let (mut kernel, store) = wired();
    let base = made(&store, &[("page", "1")], b"");
    assert_eq!(
        submit(&mut kernel, &store, &[open("factory.screen", 0, "", base)]),
        0
    );
    let first = made(&store, &[("page", "2")], b"");
    let second = made(&store, &[("page", "3")], b"");
    // The second open replaces "the top", which is the first one's by then.
    assert_eq!(
        submit(
            &mut kernel,
            &store,
            &[
                open("factory.screen", 0, "", first),
                open("factory.screen", 2, "", second)
            ]
        ),
        0
    );
    assert_eq!(kernel.stack.len(), 2);
    let top = kernel.stack.last().unwrap().serial;
    assert_eq!(
        OPENED
            .with(|opened| opened.borrow().last().copied())
            .map(|row| row.1),
        Some(first)
    );
    assert_eq!(
        read_params_of(&store, top, first),
        [("page".to_owned(), "2".to_owned())]
    );
    // Its hand-over was freed: only the two screens' are left.
    assert_eq!(store.len(), 2);
}

fn read_params_of(store: &UiStore, instance: u32, handover: u32) -> Vec<(String, String)> {
    let mut params = [0; 128];
    let used = store.params(instance, handover, &mut params).unwrap();
    let mut seen = Vec::new();
    let mut rest = &params[..used];
    while !rest.is_empty() {
        let (record, len) = crate::native_host::registrations::record_decode_prefix(rest).unwrap();
        let crate::native_host::registrations::Record::ServiceUiParam(param) = record else {
            panic!("a param record");
        };
        seen.push((param.key.to_owned(), param.value.to_owned()));
        rest = &rest[len..];
    }
    seen
}

#[test]
fn modes_one_and_three_compare_the_params_in_any_order() {
    let (mut kernel, store) = wired();
    let base = made(&store, &[("page", "1")], b"");
    assert_eq!(
        submit(&mut kernel, &store, &[open("factory.screen", 0, "", base)]),
        0
    );
    let one = made(&store, &[("a", "1"), ("b", "2")], b"first");
    assert_eq!(
        submit(&mut kernel, &store, &[open("core.translate", 1, "", one)]),
        0
    );
    let instance = kernel.stack.last().unwrap().serial;
    // The same params in another order, another text: the same instance.
    let same = made(&store, &[("b", "2"), ("a", "1")], b"second text");
    assert_eq!(
        submit(&mut kernel, &store, &[open("core.translate", 1, "", same)]),
        0
    );
    assert_eq!(kernel.stack.last().unwrap().serial, instance);
    assert_eq!(kernel.stack.len(), 2);
    assert_eq!(read(&store, instance, same), "second text");
    // Another value: opened anew in place of the old screen.
    let other = made(&store, &[("a", "1"), ("b", "3")], b"");
    assert_eq!(
        submit(&mut kernel, &store, &[open("core.translate", 1, "", other)]),
        0
    );
    assert_eq!(kernel.stack.len(), 2);
    assert_ne!(kernel.stack.last().unwrap().serial, instance);
    // The same keys, in mode 3.
    let again = made(&store, &[("b", "3"), ("a", "1")], b"");
    let now = kernel.stack.last().unwrap().serial;
    assert_eq!(
        submit(
            &mut kernel,
            &store,
            &[open("factory.screen", 3, "core.translate", again)]
        ),
        0
    );
    assert_eq!(kernel.stack.last().unwrap().serial, now);
}

#[test]
fn claims_a_drain_leaves_are_rolled_back() {
    let (mut kernel, store) = wired();
    let id = made(&store, &[], b"text");
    let claims = store.claim(stamp(), &[id]).unwrap();
    let opens = crate::native_host::commands::Opens {
        at: Vec::new(),
        claims: alloc::vec![claims],
        store: None,
    };
    kernel.drain_native_opens(&Id::new_static("factory"), Vec::new(), None, opens);
    assert_eq!(store.len(), 0);
}

#[test]
fn every_allocation_of_a_submit_with_a_hand_over_leaves_nothing_claimed() {
    let mut reached_success = false;
    for nth in 1..200 {
        let (kernel, store) = wired();
        let id = made(&store, &[("page", "1")], b"text");
        let owner = Id::new_static("factory");
        let env = kernel.shared.env(Instant(0));
        let mut stage = CommandStage::new(&owner, env.settings(), None, 8, 4096)
            .with_ui(&env, None, 0, 8)
            .with_handovers(&store, stamp());
        let scope = core::pin::pin!(CommandScope::new());
        let mut bytes = [0; 256];
        let len = commands_encode(&[open("core.translate", 1, "", id)], &mut bytes).unwrap();
        arm_failure(nth);
        let status = scope
            .as_ref()
            // SAFETY: the initialized input outlives the synchronous call.
            .scope(&mut stage, || unsafe { scope.submit(bytes.as_ptr(), len) })
            .unwrap();
        arm_failure(0);
        if status != 0 {
            assert!(stage.staged().is_empty(), "{nth}");
        }
        drop(stage);
        // Refused, or staged and the stage dropped: nothing stays claimed, and
        // the callback's end frees what no open claimed.
        store.end_callback(stamp()).unwrap();
        assert_eq!(store.len(), 0, "{nth}: status {status}");
        if status == 0 {
            reached_success = true;
            break;
        }
    }
    assert!(reached_success);
}

/// Submits `commands` in a stage built as the production one is (the store
/// shared, the stamp set when the callback is entered): the status, and
/// whether the stage kept anything of it.
fn staged_by(
    kernel: &crate::Kernel,
    store: &Shared<UiStore>,
    callback: (u64, u64),
    commands: &[WireCommand<'_>],
) -> (i32, usize, crate::native_host::commands::Opens) {
    let owner = Id::new_static("factory");
    let env = kernel.shared.env(Instant(0));
    let mut stage = CommandStage::new(&owner, env.settings(), None, 8, 4096)
        .with_ui(&env, None, 0, 8)
        .with_handover_store(Some(store));
    stage.set_handover_stamp(callback.0, callback.1);
    let scope = core::pin::pin!(CommandScope::new());
    let mut bytes = [0; 512];
    let len = commands_encode(commands, &mut bytes).unwrap();
    let status = scope
        .as_ref()
        // SAFETY: the initialized input outlives the synchronous call.
        .scope(&mut stage, || unsafe { scope.submit(bytes.as_ptr(), len) })
        .unwrap();
    let (commands, _, _, opens) = stage.into_ui_output_with_opens();
    (status, commands.len(), opens)
}

#[test]
fn another_plugins_and_an_earlier_callbacks_hand_over_is_refused() {
    let (kernel, store) = wired();
    let theirs = store.handover_make(Stamp::new(8, 1).unwrap(), &[]).unwrap();
    let earlier = store.handover_make(Stamp::new(7, 1).unwrap(), &[]).unwrap();
    // The callback of plugin 7 is the second one now.
    // Another plugin's is no hand-over of this callback; an earlier callback's
    // is past (the host answers it as a state, not as a form).
    for (id, refused) in [(theirs, ERR_INVALID), (earlier, ERR_STATE)] {
        let (status, kept, opens) = staged_by(
            &kernel,
            &store,
            (7, 2),
            &[open("core.translate", 1, "", id)],
        );
        assert_eq!(status, refused, "{id}");
        assert_eq!(kept, 0);
        assert!(opens.claims.is_empty() && opens.at.is_empty());
    }
    // Nothing was claimed: each is still its maker's to free.
    store.end_callback(Stamp::new(8, 1).unwrap()).unwrap();
    store.end_callback(Stamp::new(7, 1).unwrap()).unwrap();
    assert_eq!(store.len(), 0);
}

#[test]
fn an_open_with_a_hand_over_and_a_refused_command_after_it_is_refused_whole() {
    let (kernel, store) = wired();
    let id = made(&store, &[], b"text");
    // The second open names a hand-over nobody made.
    let (status, kept, opens) = staged_by(
        &kernel,
        &store,
        (7, 1),
        &[
            open("core.translate", 1, "", id),
            open("core.translate", 1, "", 99),
        ],
    );
    assert_eq!(status, ERR_INVALID);
    assert_eq!(kept, 0);
    assert!(opens.claims.is_empty());
    // The first one's hand-over was not claimed either.
    drop(opens);
    store.end_callback(stamp()).unwrap();
    assert_eq!(store.len(), 0);
    // An open of a screen that is not there refuses the batch whole too.
    let id = made(&store, &[], b"text");
    let (status, kept, opens) = staged_by(
        &kernel,
        &store,
        (7, 1),
        &[
            open("core.translate", 1, "", id),
            open("no.such.screen", 0, "", 0),
        ],
    );
    assert_eq!(status, ERR_NOT_FOUND);
    assert_eq!(kept, 0);
    assert!(opens.claims.is_empty());
    drop(opens);
    store.end_callback(stamp()).unwrap();
    assert_eq!(store.len(), 0);
}

#[test]
fn opens_dropped_undrained_free_what_they_claimed() {
    let (kernel, store) = wired();
    let id = made(&store, &[], b"text");
    let (status, kept, opens) = staged_by(
        &kernel,
        &store,
        (7, 1),
        &[open("core.translate", 1, "", id)],
    );
    assert_eq!((status, kept), (0, 1));
    assert_eq!(opens.claims.len(), 1);
    // The plugin is gone, or a queue could not keep the batch: the opens are
    // dropped without a drain.
    drop(opens);
    // Not claimed any more: the callback's end frees it.
    store.end_callback(stamp()).unwrap();
    assert_eq!(store.len(), 0);
}

#[test]
fn a_native_screen_the_kernel_opens_with_params_gets_them_handed_over() {
    let (mut kernel, store) = wired();
    // The root is never closed: a base below, without params.
    kernel.apply(Command::OpenScreen(
        Id::new_static("factory.screen"),
        Params::new(),
    ));
    assert_eq!(store.len(), 0);
    // Opened by no native plugin (a shell's script, an interface-6 plugin,
    // the kernel): the host hands the params over itself.
    kernel.apply(Command::OpenScreen(
        Id::new_static("factory.screen"),
        Params::new()
            .with("id", "core.rotation")
            .with("text", "A note."),
    ));
    assert_eq!(kernel.stack.len(), 2);
    let instance = kernel.stack.last().unwrap().serial;
    let (seen, handover, params, text) = OPENED
        .with(|opened| opened.borrow().last().copied())
        .unwrap();
    assert_eq!((seen, params, text), (instance, 2, 7));
    assert_ne!(handover, 0);
    assert_eq!(
        read_params_of(&store, instance, handover),
        [
            ("id".to_owned(), "core.rotation".to_owned()),
            ("text".to_owned(), "A note.".to_owned())
        ]
    );
    assert_eq!(read(&store, instance, handover), "A note.");
    assert_eq!(store.len(), 1);
    // Closing the screen frees it.
    kernel.apply(Command::Close(paperpaw_sdk::ScreenKey::new(instance)));
    kernel.drain_deferred_ui_closes();
    assert_eq!(store.len(), 0);
    // A native open is as it was: one of none makes no hand-over of the host's.
    assert_eq!(
        submit(&mut kernel, &store, &[open("core.translate", 1, "", 0)]),
        0
    );
    let fresh = kernel.stack.last().unwrap().serial;
    assert_eq!(
        OPENED.with(|opened| opened.borrow().last().copied()),
        Some((fresh, 0, 0, 0))
    );
    assert_eq!(store.len(), 0);
}
