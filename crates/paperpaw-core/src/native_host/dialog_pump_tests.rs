//! Questions end to end through the real kernel: the dialog of a request
//! opens in its slot once the asking callback returned, a failed callback or a
//! slot nobody serves cancels it as `not_started`, and an answer reaches the
//! asker as `event.answered`, one callback each, and frees the request.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::Staging;
use crate::native_host::{
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
    shared::Shared,
    ui_requests::Question,
    ui_service::UiServices,
    ui_store::{Limits, Stamp, UiStore},
};
use crate::{Place, Turn};
use alloc::{boxed::Box, rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Canvas, Handled, Manifest, Priority, RegisterError, Version};

const KEYBOARD_SCREEN: Id = Id::new_static("d.keyboard");

std::thread_local! {
    /// The params the dialog screen was opened with.
    static OPENED: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

std::thread_local! {
    /// Whether the asker opens the keyboard screen when it hears its answer.
    static OPENS: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

std::thread_local! {
    /// Whether the next dialog shown closes itself.
    static SPAWN: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

std::thread_local! {
    /// The store a confirm answers through, as the dialog plugin does (op 11).
    static ANSWERS: RefCell<Option<Shared<UiStore>>> = const { RefCell::new(None) };
}

/// The dialog on the keyboard and confirm slots: *Confirm* answers yes to a
/// yes or no question and closes, as the dialog plugin does.
struct Dialog {
    request: Option<u32>,
}

impl crate::testing::Fake for Dialog {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}
    fn on_action(&mut self, action: &Id, cx: &mut Turn<'_, '_>) -> Handled {
        let store = ANSWERS.with(|store| store.borrow().as_ref().and_then(|s| s.try_clone()));
        if *action != paperpaw_sdk::builtin::actions::CONFIRM {
            return Handled::No;
        }
        if let (Some(store), Some(request), Some(key)) = (store, self.request, cx.key()) {
            let _ = store.answer(key.serial(), request, 0, 1);
        }
        cx.command(crate::Ask::Back);
        Handled::Yes
    }
    fn on_show(&mut self, cx: &mut Turn<'_, '_>) {
        if SPAWN.with(core::cell::Cell::take) {
            cx.command(crate::Ask::Back);
        }
    }
}

/// A plugin that serves the keyboard and the confirm slot.
fn serves() -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    |r| {
        r.add_fake(crate::testing::FakeSpec::new(
            KEYBOARD_SCREEN,
            "keyboard",
            Box::new(|_, params| {
                let request = params.get("core.request").and_then(|r| r.parse().ok());
                OPENED.with(|opened| {
                    opened.borrow_mut().push((
                        params
                            .get("core.request")
                            .map(String::from)
                            .unwrap_or_default(),
                        params
                            .get("core.kind")
                            .map(String::from)
                            .unwrap_or_default(),
                    ));
                });
                Box::new(Dialog { request })
            }),
        ))?;
        r.bind_slot(
            paperpaw_sdk::builtin::slots::KEYBOARD,
            KEYBOARD_SCREEN,
            Priority::DEFAULT,
        )?;
        r.bind_slot(
            paperpaw_sdk::builtin::slots::CONFIRM,
            KEYBOARD_SCREEN,
            Priority::DEFAULT,
        )
    }
}

/// What [`serves`] registers under.
fn serves_manifest() -> Manifest {
    Manifest::new(Id::new_static("d"), "Dialogs", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

struct Lock;

impl crate::testing::Fake for Lock {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}
    fn flags(&self) -> u32 {
        crate::testing::GUARDS
    }
    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }
}

/// A plugin `id` with a screen `<id>.lock` that guards the device.
fn locks_items(kept: &'static str) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        r.add_fake(crate::testing::FakeSpec::new(
            Id::try_parse(&std::format!("{}.lock", kept)).unwrap(),
            "lock",
            Box::new(|_, _| Box::new(Lock)),
        ))
    }
}

/// What [`locks_items`] registers under.
fn locks_manifest(kept: &'static str) -> Manifest {
    Manifest::new(Id::new_static(kept), "Lock", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

/// The asker: records the `event.answered` records it was called with.
struct Asker {
    seen: Rc<RefCell<Vec<wire::EventAnswered>>>,
    status: Result<i32, i32>,
    /// Where given: it hears `event.stack_changed` too, and logs the order
    /// of what it hears.
    heard: Option<Rc<RefCell<Vec<&'static str>>>>,
}

impl Asker {
    fn hear(&self, bytes: &[u8]) {
        let Some(heard) = &self.heard else {
            return;
        };
        match wire::records_decode_single(bytes) {
            Ok(wire::Record::EventAnswered(_)) => heard.borrow_mut().push("answered"),
            Ok(wire::Record::EventStackChanged(_)) => heard.borrow_mut().push("stack"),
            _ => {}
        }
    }
}

impl NativeInstance for Asker {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        self.heard.is_some() && tag == wire::EVENT_STACK_CHANGED_TAG
    }
    fn service_event(&self, bytes: &[u8], _: &Place<'_>) -> Result<i32, i32> {
        self.hear(bytes);
        self.status
    }
    fn service_event_staged(
        &self,
        bytes: &[u8],
        _: &Place<'_>,
        stage: &mut crate::native_host::commands::CommandStage<'_>,
    ) -> Result<i32, i32> {
        use crate::native_host::commands::{WireCommand, commands_decode, commands_encode};
        self.hear(bytes);
        if let Ok(wire::Record::EventAnswered(answered)) = wire::records_decode_single(bytes) {
            self.seen.borrow_mut().push(answered);
            if OPENS.with(core::cell::Cell::take) {
                let open = WireCommand::Open {
                    target: "d.keyboard",
                    mode: 0,
                    slot: "",
                    handover: 0,
                };
                let mut bytes = [0; 64];
                let len = commands_encode(&[open], &mut bytes).unwrap();
                assert_eq!(stage.stage(&commands_decode(&bytes[..len]).unwrap()), OK);
            }
        }
        self.status
    }
}

impl Dispatch for Asker {
    fn event(&self, _: u32, _: &[u8], _: &Place<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &Place<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &Place<'_>,
        _: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn dropped(&self, _: u32, _: i32) {}
}

struct World {
    kernel: crate::Kernel,
    store: Shared<UiStore>,
    owner: u64,
    seen: Rc<RefCell<Vec<wire::EventAnswered>>>,
}

fn world(status: Result<i32, i32>) -> World {
    world_with(status, None)
}

/// A world where plugin `locks` has a lock screen to open.
fn world_with(status: Result<i32, i32>, locks: Option<&'static str>) -> World {
    OPENED.with(|opened| opened.borrow_mut().clear());
    let store = Shared::try_new(UiStore::new(Limits::ELSEWHERE)).unwrap();
    ANSWERS.with(|answers| *answers.borrow_mut() = store.try_clone());
    let mut builder =
        crate::Kernel::builder(crate::tests::DEVICE).items(serves_manifest(), serves());
    if let Some(name) = locks {
        builder = builder.items(locks_manifest(name), locks_items(name));
    }
    let mut kernel = builder.build().unwrap();
    kernel.start(Instant(0));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let owner = kernel
        .shared
        .native_instances
        .test_insert(
            Id::try_parse("a").unwrap(),
            Some(
                NativeLease::try_new(Asker {
                    seen: Rc::clone(&seen),
                    status,
                    heard: None,
                })
                .unwrap(),
            ),
            true,
        )
        .unwrap();
    kernel
        .shared
        .native_instances
        .test_resources()
        .unwrap()
        .hosts()
        .wire_ui(UiServices::new(store.try_clone().unwrap(), false))
        .unwrap();
    World {
        kernel,
        store,
        owner,
        seen,
    }
}

fn ask(w: &World, kind: u32, epoch: u64) -> u32 {
    ask_with(w, kind, 0, if kind == 1 { "one\ntwo,x" } else { "" }, epoch)
}

fn ask_with(w: &World, kind: u32, flags: u32, choices: &str, epoch: u64) -> u32 {
    let question = Question {
        kind,
        flags,
        remote: false,
        title: "Title",
        text: "Question",
        choices,
    };
    let who = Stamp::new(w.owner, epoch).unwrap();
    w.store
        .ask(who, None, &question, false, None, false)
        .unwrap()
        .0
}

#[test]
fn the_dialog_opens_in_its_slot_after_the_asking_callback_returned() {
    let mut w = world(Ok(OK));
    let id = ask(&w, 0, 1);
    assert!(!w.kernel.native_dialog_pump());
    assert_eq!(w.kernel.active_screen(), Some(&KEYBOARD_SCREEN));
    // The params are ASCII decimal: the request and the kind.
    OPENED.with(|opened| {
        assert_eq!(
            *opened.borrow(),
            [(std::format!("{id}"), String::from("0"))]
        );
    });
    let top = w.kernel.stack.last().unwrap().serial;
    assert_eq!(w.store.dialog_of(top).map(|d| d.0), Some(id));
    // A native dialog (this one, a test's): the host's hand-over waits for the
    // screen to take it.
    assert_eq!(w.store.len(), 1);
}

#[test]
fn a_kind_whose_slot_nobody_serves_is_cancelled_as_not_started() {
    let mut w = world(Ok(OK));
    // Nothing serves the folder chooser here.
    let before = w.kernel.active_screen().cloned();
    let id = ask(&w, 3, 1);
    w.kernel.native_dialog_pump();
    assert_eq!(w.kernel.active_screen().cloned(), before);
    let answered = w.seen.borrow().clone();
    assert_eq!(answered.len(), 1);
    assert_eq!((answered[0].request, answered[0].outcome), (id, 1));
    // `not_started` is bit 1.
    assert_eq!(answered[0].flags & 2, 2);
    // Freed once the callback returned.
    assert_eq!(w.store.request_count(), 0);
}

#[test]
fn a_failed_asking_callback_opens_no_dialog() {
    let mut w = world(Ok(OK));
    let before = w.kernel.active_screen().cloned();
    let id = ask(&w, 0, 5);
    w.store
        .callback_failed(Stamp::new(w.owner, 5).unwrap())
        .unwrap();
    w.kernel.native_dialog_pump();
    assert_eq!(w.kernel.active_screen().cloned(), before);
    OPENED.with(|opened| assert!(opened.borrow().is_empty()));
    let answered = w.seen.borrow().clone();
    assert_eq!(
        (
            answered[0].request,
            answered[0].outcome,
            answered[0].flags & 2
        ),
        (id, 1, 2)
    );
}

#[test]
fn an_answer_reaches_the_asker_and_frees_the_request_whatever_the_callback_says() {
    let mut w = world(Err(ERR_INVALID));
    let id = ask(&w, 0, 1);
    w.kernel.native_dialog_pump();
    let top = w.kernel.stack.last().unwrap().serial;
    // The dialog answers: outcome 0 needs a committed text.
    let dialog = Stamp::new(w.owner, 9).unwrap();
    w.store
        .request_text_write(dialog, Some(top), id, 0, b"hello", true)
        .unwrap();
    w.store.answer(top, id, 0, 0).unwrap();
    w.kernel.native_dialog_pump();
    let answered = w.seen.borrow().clone();
    assert_eq!(answered.len(), 1);
    assert_eq!(
        (
            answered[0].request,
            answered[0].kind,
            answered[0].outcome,
            answered[0].length
        ),
        (id, 0, 0, 5)
    );
    // The callback failed: the request is freed all the same.
    assert_eq!(w.store.request_count(), 0);
}

#[test]
fn a_dialog_closed_without_an_answer_is_a_cancel_the_asker_hears_of() {
    let mut w = world(Ok(OK));
    let id = ask(&w, 0, 1);
    w.kernel.native_dialog_pump();
    assert_eq!(w.kernel.active_screen(), Some(&KEYBOARD_SCREEN));
    // The user leaves the dialog.
    w.kernel.apply(crate::Ask::Back);
    w.kernel.finish();
    w.kernel.native_dialog_pump();
    let answered = w.seen.borrow().clone();
    assert_eq!(answered.len(), 1);
    assert_eq!((answered[0].request, answered[0].outcome), (id, 1));
    assert_eq!(w.store.request_count(), 0);
}

fn lock(w: &mut World, name: &str) {
    let id = Id::try_parse(&std::format!("{name}.lock")).unwrap();
    w.kernel
        .open_screen(Instant(2), id, paperpaw_sdk::Params::new());
    assert!(w.kernel.guard().is_some());
}

#[test]
fn the_guards_own_question_opens_above_the_guard_as_its_dialog() {
    let mut w = world_with(Ok(OK), Some("a"));
    lock(&mut w, "a");
    let id = ask(&w, 0, 1);
    w.kernel.native_dialog_pump();
    // The keyboard is on top of the lock, authorized for its question.
    assert_eq!(w.kernel.active_screen(), Some(&KEYBOARD_SCREEN));
    let top = w.kernel.stack.last().unwrap();
    assert!(top.guard_dialog);
    assert_eq!(w.store.dialog_of(top.serial).map(|d| d.0), Some(id));
    assert!(
        w.kernel
            .guard()
            .is_some_and(|guard| guard + 1 == w.kernel.stack.len() - 1)
    );
}

#[test]
fn another_plugins_question_waits_below_the_guard_tracked_by_its_instance() {
    let mut w = world_with(Ok(OK), Some("g"));
    lock(&mut w, "g");
    let id = ask(&w, 0, 1);
    w.kernel.native_dialog_pump();
    // The lock stays on top; the dialog waits directly below it.
    let guard = w.kernel.guard().unwrap();
    assert_eq!(guard, w.kernel.stack.len() - 1);
    let dialog = &w.kernel.stack[guard - 1];
    assert_eq!(dialog.id, KEYBOARD_SCREEN);
    assert!(!dialog.guard_dialog);
    // Tracked, not a ghost: the request knows its instance, and nothing was
    // cancelled.
    assert_eq!(w.store.dialog_of(dialog.serial).map(|d| d.0), Some(id));
    assert!(w.seen.borrow().is_empty());
}

#[test]
fn an_answer_whose_callback_did_not_run_is_kept_for_the_next_pass() {
    let mut w = world(Ok(OK));
    let id = ask(&w, 0, 1);
    w.kernel.native_dialog_pump();
    let top = w.kernel.stack.last().unwrap().serial;
    let dialog = Stamp::new(w.owner, 9).unwrap();
    w.store
        .request_text_write(dialog, Some(top), id, 0, b"x", true)
        .unwrap();
    w.store.answer(top, id, 0, 0).unwrap();
    let taken = w.store.take_delivery(w.owner).unwrap();
    assert!(w.store.take_delivery(w.owner).is_none());
    w.store.undo_delivery(&taken).unwrap();
    let again = w.store.take_delivery(w.owner).unwrap();
    assert_eq!(
        (again.request, again.outcome, again.length),
        (taken.request, 0, 1)
    );
}

#[test]
fn another_plugin_can_neither_answer_cancel_nor_fail_the_request_of_an_asker() {
    let mut w = world(Ok(OK));
    let id = ask(&w, 0, 5);
    let b = w.owner + 1_000;
    // While the asker's callback runs: another plugin's stamp changes nothing,
    // nor does the asker's other epoch.
    w.store.callback_failed(Stamp::new(b, 5).unwrap()).unwrap();
    w.store
        .callback_failed(Stamp::new(w.owner, 6).unwrap())
        .unwrap();
    w.store.callbacks_over().unwrap();
    assert_eq!(w.store.next_starting().map(|s| s.request), Some(id));
    w.kernel.native_dialog_pump();
    let top = w.kernel.stack.last().unwrap().serial;
    // Open: another plugin, or another instance, cannot write, answer or cancel.
    let other = Stamp::new(b, 1).unwrap();
    assert!(
        w.store
            .request_text_write(other, Some(top + 1), id, 0, b"x", true)
            .is_err()
    );
    assert!(w.store.answer(top + 1, id, 1, 0).is_err());
    assert!(
        w.store
            .request_text_write(other, None, id, 0, b"x", true)
            .is_err()
    );
    assert!(w.seen.borrow().is_empty());
    assert_eq!(w.store.dialog_of(top).map(|d| d.0), Some(id));
}

fn act(w: &mut World, at: u64, action: paperpaw_sdk::Id) {
    w.kernel.run_action(Instant(at), action);
}

fn answers(w: &World) -> Vec<(u32, u32, u32, u32)> {
    w.seen
        .borrow()
        .iter()
        .map(|a| (a.kind, a.outcome, a.choice, a.flags & 1))
        .collect()
}

/// The dialog on the slot answers the question; the asker hears it once the
/// dialog closed.
#[test]
fn a_confirm_question_is_answered_by_the_dialog_on_its_slot() {
    use paperpaw_sdk::builtin::actions;
    let mut w = world(Ok(OK));
    // `focus_yes` (bit 9): the dialog reads it.
    let id = ask_with(&w, 2, 1 << 9, "", 1);
    w.kernel.native_dialog_pump();
    assert_eq!(w.kernel.active_screen(), Some(&KEYBOARD_SCREEN));
    act(&mut w, 10, actions::CONFIRM);
    w.kernel.native_dialog_pump();
    // Yes is choice 1, outcome 0, the dialog closed by then (bit 0).
    assert_eq!(answers(&w), [(2, 0, 1, 1)]);
    assert_eq!(w.seen.borrow()[0].request, id);
    assert_eq!(w.store.request_count(), 0);
    assert_ne!(w.kernel.active_screen(), Some(&KEYBOARD_SCREEN));
}

/// An action run for a shell's remote control or a test closes the dialog
/// before the asker hears the answer, as a key press does (#266): the screen
/// the answer opens is not what the dialog's own close takes.
#[test]
fn a_screen_the_answer_opens_stays_on_top_after_an_action_that_was_run() {
    use paperpaw_sdk::builtin::actions;
    let mut w = world(Ok(OK));
    ask_with(&w, 2, 1 << 9, "", 1);
    w.kernel.native_dialog_pump();
    OPENS.with(|opens| opens.set(true));
    act(&mut w, 10, actions::CONFIRM);
    let _ = w.kernel.tick(Instant(20));
    // Bit 0: the dialog was closed when the answer was heard, as after a tap.
    assert_eq!(answers(&w), [(2, 0, 1, 1)]);
    assert_eq!(w.kernel.active_screen().map(Id::as_str), Some("d.keyboard"));
}

#[test]
fn back_on_a_dialog_cancels_the_question() {
    use paperpaw_sdk::builtin::actions;
    let mut w = world(Ok(OK));
    ask_with(&w, 2, 0, "", 1);
    w.kernel.native_dialog_pump();
    act(&mut w, 10, actions::BACK);
    w.kernel.native_dialog_pump();
    // Cancelled (outcome 1) by the close of the dialog, bit 0 `dialog closed`.
    assert_eq!(answers(&w), [(2, 1, 0, 1)]);
    assert_eq!(w.store.request_count(), 0);
}

/// With no dialog plugin on its slot a choice is cancelled as not started:
/// the host has no dialog of its own (#293).
#[test]
fn a_choice_nobody_serves_is_cancelled_as_not_started() {
    let mut w = world(Ok(OK));
    ask_with(&w, 1, 0, "one\ntwo,x", 1);
    w.kernel.native_dialog_pump();
    w.kernel.native_dialog_pump();
    // Outcome 1, bit 1 `not_started`.
    assert_eq!(
        w.seen
            .borrow()
            .iter()
            .map(|a| (a.kind, a.outcome, a.flags & 2))
            .collect::<Vec<_>>(),
        [(1, 1, 2)]
    );
}

#[test]
fn a_question_whose_dialog_the_stack_closes_is_cancelled() {
    let mut w = world(Ok(OK));
    ask_with(&w, 2, 0, "", 1);
    w.kernel.native_dialog_pump();
    // The stack closes it with no answer (a guard, a shell).
    w.kernel.apply(crate::Ask::Back);
    w.kernel.finish();
    w.kernel.native_dialog_pump();
    assert_eq!(answers(&w), [(2, 1, 0, 1)]);
}

/// The dialog is bound at once, before the commands its open leads to run: one
/// of them closing it is then a close of an open request (a cancel the asker
/// hears of), not an instance nobody knows, and what is bound is the instance
/// opened, however many are opened after it.
#[test]
fn the_dialog_is_bound_before_the_commands_it_leads_to_run() {
    let mut w = world_with(Ok(OK), Some("a"));
    lock(&mut w, "a");
    let id = ask(&w, 0, 1);
    SPAWN.with(|spawn| spawn.set(true));
    w.kernel.native_dialog_pump();
    // The dialog's `on_show` closed it: the request is cancelled and heard of.
    w.kernel.native_dialog_pump();
    let answered = w.seen.borrow().clone();
    assert_eq!(answered.len(), 1);
    assert_eq!((answered[0].request, answered[0].outcome), (id, 1));
    // Closed (bit 0), not `not_started` (bit 1): it was open when it went.
    assert_eq!(answered[0].flags & 3, 1);
    assert_eq!(w.store.request_count(), 0);
    assert!(w.kernel.stack.iter().all(|s| s.id != KEYBOARD_SCREEN));
}

/// A text op 18 committed goes out to the shell (`Io::Copy`); with no memory
/// for the copy it is skipped and the log says so.
#[test]
fn a_committed_clipboard_text_is_copied_out_or_the_log_says_why_not() {
    let mut w = world(Ok(OK));
    w.store.bind_clipboard(&w.kernel.shared.clipboard);
    let who = crate::native_host::ui_store::Stamp::new(w.owner, 1).unwrap();
    w.store.clipboard_write(who, 0, "copied", true).unwrap();
    w.kernel.clipboard_copy_out();
    assert_eq!(
        w.kernel.io.clipboard.take_copied().as_deref(),
        Some("copied")
    );
    // The next commit finds no memory for its copy.
    w.store.clipboard_write(who, 0, "lost", true).unwrap();
    crate::native_host::shell::tests::FAIL_NTH.with(|n| n.set(1));
    w.kernel.clipboard_copy_out();
    crate::native_host::shell::tests::FAIL_NTH.with(|n| n.set(0));
    assert!(w.kernel.io.clipboard.take_copied().is_none());
    assert!(
        w.kernel
            .take_warnings()
            .iter()
            .any(|warning| warning.message.contains("could not be copied out"))
    );
}

#[test]
fn a_tool_cannot_stage_a_text_question_where_no_native_keyboard_serves_the_slot() {
    let mut w = world(Ok(OK));
    // The slot's plugin (`d`) is no native instance here.
    let staged = w.kernel.stage_text_question(Instant(10), "Name", false);
    assert!(staged.is_err());
    assert_eq!(w.store.request_count(), 0);
    OPENED.with(|opened| assert!(opened.borrow().is_empty()));
}

#[test]
fn a_tool_stages_a_text_question_and_the_keyboard_slot_opens_for_it() {
    let mut w = world(Ok(OK));
    let seen = Rc::new(RefCell::new(Vec::new()));
    w.kernel
        .shared
        .native_instances
        .test_insert(
            Id::try_parse("d").unwrap(),
            Some(
                NativeLease::try_new(Asker {
                    seen: Rc::clone(&seen),
                    status: Ok(OK),
                    heard: None,
                })
                .unwrap(),
            ),
            true,
        )
        .unwrap();
    assert_eq!(w.kernel.keyboard_screen(), Some(&KEYBOARD_SCREEN));
    w.kernel
        .stage_text_question(Instant(10), "Name", false)
        .unwrap();
    assert_eq!(w.kernel.active_screen(), Some(&KEYBOARD_SCREEN));
    // The params a plugin's `ui.ask` makes: the request (kept in the store)
    // and the kind, 0 for text.
    OPENED.with(|opened| {
        let opened = opened.borrow();
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0].1, "0");
        assert!(w.store.is_request(opened[0].0.parse().unwrap()));
    });
}

#[test]
fn a_staged_secret_question_is_secret_and_a_second_one_is_refused() {
    let mut w = world(Ok(OK));
    let seen = Rc::new(RefCell::new(Vec::new()));
    w.kernel
        .shared
        .native_instances
        .test_insert(
            Id::try_parse("d").unwrap(),
            Some(
                NativeLease::try_new(Asker {
                    seen: Rc::clone(&seen),
                    status: Ok(OK),
                    heard: None,
                })
                .unwrap(),
            ),
            true,
        )
        .unwrap();
    w.kernel
        .stage_text_question(Instant(10), "PIN", true)
        .unwrap();
    assert_eq!(w.kernel.active_screen(), Some(&KEYBOARD_SCREEN));
    let request = OPENED.with(|opened| opened.borrow()[0].0.parse::<u32>().unwrap());
    let top = w.kernel.stack.last().unwrap().serial;
    assert!(
        w.store
            .question(top, request, |shown| shown.flags & 4 != 0)
            .unwrap()
    );
    // One open: another is refused, nothing is replaced.
    assert!(
        w.kernel
            .stage_text_question(Instant(20), "Again", false)
            .is_err()
    );
    assert_eq!(w.store.request_count(), 1);
}

/// A question cancelled after a pass reaches its asker before anything else of the
/// next pass (here the change of the stack the close made): what is served
/// after it (a computer's next request on the cable) never finds the
/// question still open.
#[test]
fn a_cancelled_question_is_told_before_the_next_passes_events() {
    let mut w = world(Ok(OK));
    let heard = Rc::new(RefCell::new(Vec::new()));
    let owner = w
        .kernel
        .shared
        .native_instances
        .test_insert(
            Id::try_parse("e").unwrap(),
            Some(
                NativeLease::try_new(Asker {
                    seen: Rc::new(RefCell::new(Vec::new())),
                    status: Ok(OK),
                    heard: Some(Rc::clone(&heard)),
                })
                .unwrap(),
            ),
            true,
        )
        .unwrap();
    let question = Question {
        kind: 2,
        flags: 0,
        remote: false,
        title: "Title",
        text: "Question",
        choices: "",
    };
    let who = Stamp::new(owner, 1).unwrap();
    w.store
        .ask(who, None, &question, false, None, false)
        .unwrap();
    let _ = w.kernel.tick(Instant(5));
    // The close is queued (as one a plugin stages from an event of the
    // pass): the pass of this call runs before the dialog closes.
    w.kernel.output.commands.push(crate::Ask::Back);
    let _ = w.kernel.tick(Instant(10));
    assert!(w.kernel.native_dialog_answers_waiting());
    heard.borrow_mut().clear();
    let _ = w.kernel.tick(Instant(20));
    assert_eq!(
        heard.borrow().first(),
        Some(&"answered"),
        "{:?}",
        heard.borrow()
    );
}

#[test]
fn a_question_asked_after_the_pump_ran_wakes_the_kernel_at_once() {
    let mut w = world(Ok(OK));
    // Asked by a callback that ran after this call's pump (an action run,
    // `Kernel::run_action`): the pump is due now, not at the next wake (#267).
    ask(&w, 2, 1);
    let now = w.kernel.shared.now;
    assert_eq!(w.kernel.service_due(), Some(now));
    w.kernel.tick(Instant(10));
    assert_eq!(w.kernel.active_screen(), Some(&KEYBOARD_SCREEN));
    // Open: no more wake for it.
    assert!(!w.kernel.native_question_waiting());
}
