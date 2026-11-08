//! `command.open` (1547): a staged open applies in its mode, names the
//! hand-over it claimed, and the instance it shows gets it: the selected text
//! of the reader's translate arrives as the hand-over's host text, never as a
//! param (`docs/abi/ui.md` 1.1).
use super::*;
use crate::native_host::{
    commands::{CommandScope, CommandStage, WireCommand, commands_encode},
    ui_guard::UiStackView,
    ui_service::{UiHost, UiServices},
    ui_store::{Limits, Param, Stamp, UiStore},
};
use alloc::{borrow::ToOwned, rc::Rc};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NOT_FOUND, ERR_STATE};
use paperpaw_platform::Instant;

struct Host;
impl UiHost for Host {
    fn callback_epoch(&self) -> u64 {
        1
    }
    fn calling_instance(&self, _: u64) -> Option<u32> {
        None
    }
    fn with_stack(&self, _: &mut dyn FnMut(&dyn UiStackView)) {}
    fn x4(&self) -> bool {
        false
    }
    fn close_dialog(&self, _: u32) {}
}

/// A kernel with the factory plugin installed (its screen is offered for
/// `core.translate`), the pool wired to a hand-over store.
fn wired() -> (crate::Kernel, Rc<UiStore>) {
    let store = Rc::new(UiStore::new(Limits::ELSEWHERE));
    let pool = crate::native_host::HostResources::try_new().unwrap();
    pool.hosts()
        .wire_ui(UiServices {
            store: Rc::clone(&store),
            host: Rc::new(Host),
        })
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
        let opens = stage.take_opens();
        let (staged, redraw, _) = stage.into_ui_output();
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
    // An open of none clears its params and text, and says so.
    assert_eq!(
        submit(&mut kernel, &store, &[open("core.translate", 1, "", 0)]),
        0
    );
    assert_eq!(
        OPENED.with(|opened| opened.borrow().last().copied()),
        Some((instance, 0, 0, 0))
    );
    assert_eq!(store.len(), 0);
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
