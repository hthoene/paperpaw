//! Questions end to end through the real kernel: the dialog of a request
//! opens in its slot once the asking callback returned, a failed callback or a
//! slot nobody serves cancels it as `not_started`, and an answer reaches the
//! asker as `event.answered`, one callback each, and frees the request.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::{
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
    shared::Shared,
    ui_requests::Question,
    ui_service::UiServices,
    ui_store::{Limits, Stamp, UiStore},
};
use alloc::{boxed::Box, rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_platform::Instant;
use paperpaw_sdk::{
    Canvas, Cx, Env, Handled, Manifest, Plugin, Priority, RegisterError, Registrar, Screen,
    ScreenSpec, Version,
};

const KEYBOARD_SCREEN: Id = Id::new_static("d.keyboard");

std::thread_local! {
    /// The params the dialog screen was opened with.
    static OPENED: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

std::thread_local! {
    /// Whether the next dialog shown closes itself.
    static SPAWN: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

struct Dialog;

impl Screen for Dialog {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}
    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if SPAWN.with(core::cell::Cell::take) {
            cx.command(paperpaw_sdk::Command::Back);
        }
    }
}

/// A plugin that serves the keyboard slot and nothing else.
struct Serves;

impl Plugin for Serves {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("d"), "Dialogs", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            KEYBOARD_SCREEN,
            "keyboard",
            Box::new(|_, params| {
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
                Box::new(Dialog)
            }),
        ))?;
        r.bind_slot(
            paperpaw_sdk::builtin::slots::KEYBOARD,
            KEYBOARD_SCREEN,
            Priority::DEFAULT,
        )
    }
}

/// A plugin `id` with a screen `<id>.lock` that guards the device.
struct Locks(&'static str);

struct Lock;

impl Screen for Lock {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}
    fn guards(&self) -> bool {
        true
    }
    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for Locks {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static(self.0), "Lock", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            Id::try_parse(&std::format!("{}.lock", self.0)).unwrap(),
            "lock",
            Box::new(|_, _| Box::new(Lock)),
        ))
    }
}

/// The asker: records the `event.answered` records it was called with.
struct Asker {
    seen: Rc<RefCell<Vec<wire::EventAnswered>>>,
    status: Result<i32, i32>,
}

impl NativeInstance for Asker {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn service_event(&self, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        self.status
    }
    fn service_event_staged(
        &self,
        bytes: &[u8],
        _: &Env<'_>,
        _: &mut crate::native_host::commands::CommandStage<'_>,
    ) -> Result<i32, i32> {
        if let Ok(wire::Record::EventAnswered(answered)) = wire::records_decode_single(bytes) {
            self.seen.borrow_mut().push(answered);
        }
        self.status
    }
}

impl Dispatch for Asker {
    fn event(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &Env<'_>,
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
    let mut builder = crate::Kernel::builder(crate::tests::DEVICE).plugin(Serves);
    if let Some(name) = locks {
        builder = builder.plugin(Locks(name));
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
    // A dialog that is not native (this one) keeps the params: the host's
    // hand-over no screen took is freed.
    assert_eq!(w.store.len(), 0);
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
    w.kernel.apply(paperpaw_sdk::Command::Back);
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

/// With no dialog plugin on the slot, the host's own confirm answers the question.
#[test]
fn a_confirm_question_is_answered_by_the_host_dialog() {
    use paperpaw_sdk::builtin::actions;
    let mut w = world(Ok(OK));
    // `focus_yes` (bit 9): the focus starts on yes.
    let id = ask_with(&w, 2, 1 << 9, "", 1);
    w.kernel.native_dialog_pump();
    assert_eq!(
        w.kernel.active_screen().map(Id::as_str),
        Some("core.confirm-dialog")
    );
    act(&mut w, 10, actions::CONFIRM);
    w.kernel.native_dialog_pump();
    // Yes is choice 1, outcome 0; the reply event reached nobody else.
    assert_eq!(answers(&w), [(2, 0, 1, 0)]);
    assert_eq!(w.seen.borrow()[0].request, id);
    assert_eq!(w.store.request_count(), 0);
    assert_ne!(
        w.kernel.active_screen().map(Id::as_str),
        Some("core.confirm-dialog")
    );
}

#[test]
fn back_on_a_host_dialog_cancels_the_question() {
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

#[test]
fn a_choice_question_answers_the_line_chosen_commas_kept() {
    use paperpaw_sdk::builtin::actions;
    let mut w = world(Ok(OK));
    // Two lines, the second holds a comma of its own.
    ask_with(&w, 1, 0, "one\ntwo,x", 1);
    w.kernel.native_dialog_pump();
    assert_eq!(
        w.kernel.active_screen().map(Id::as_str),
        Some("core.choice-dialog")
    );
    // The first step shows the focus, the second moves it to the second line.
    act(&mut w, 10, actions::DOWN);
    act(&mut w, 20, actions::DOWN);
    act(&mut w, 30, actions::CONFIRM);
    w.kernel.native_dialog_pump();
    assert_eq!(answers(&w), [(1, 0, 1, 0)]);
}

#[test]
fn a_question_whose_dialog_the_stack_closes_is_cancelled() {
    let mut w = world(Ok(OK));
    ask_with(&w, 2, 0, "", 1);
    w.kernel.native_dialog_pump();
    // The stack closes it with no answer (a guard, a shell).
    w.kernel.apply(paperpaw_sdk::Command::Back);
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

/// Two equal lines answer each its own index, and a comma stays in its line.
#[test]
fn equal_lines_answer_their_own_index_and_commas_stay() {
    use paperpaw_sdk::builtin::actions;
    for (steps, expected) in [(1, 0), (2, 1)] {
        let mut w = world(Ok(OK));
        ask_with(&w, 1, 0, "same, line\nsame, line", 1);
        w.kernel.native_dialog_pump();
        // The first step shows the focus; each next moves one line.
        for at in 0..steps {
            act(&mut w, 10 + at, actions::DOWN);
        }
        act(&mut w, 50, actions::CONFIRM);
        w.kernel.native_dialog_pump();
        assert_eq!(answers(&w), [(1, 0, expected, 0)], "after {steps} steps");
    }
}

/// An index the list does not hold is a cancel, and the log says so.
#[test]
fn a_choice_outside_the_list_is_cancelled_and_logged() {
    let mut w = world(Ok(OK));
    let id = ask_with(&w, 1, 0, "one\ntwo", 1);
    w.kernel.native_dialog_pump();
    let top = w.kernel.stack.last().unwrap().serial;
    // What the dialog would do with an index past its two lines.
    let to = crate::native_host::dialogs::Answerer::new(w.store.try_clone().unwrap(), top, id);
    to.choose(5, 2);
    w.kernel.native_dialog_pump();
    assert_eq!(answers(&w), [(1, 1, 0, 0)]);
    assert!(
        w.kernel
            .take_warnings()
            .iter()
            .any(|warning| warning.message.contains("was not taken"))
    );
}

/// A question with a title of its own.
fn ask_titled(w: &World, kind: u32, title: &str, choices: &str) -> u32 {
    let question = Question {
        kind,
        flags: 0,
        remote: false,
        title,
        text: "Question",
        choices,
    };
    let who = Stamp::new(w.owner, 1).unwrap();
    w.store
        .ask(who, None, &question, false, None, false)
        .unwrap()
        .0
}

/// The region of the dialog on top, its row height, gap and pager height.
fn geometry(w: &World, at: u64) -> (paperpaw_sdk::Rect, i32, i32, i32) {
    let env = w.kernel.shared.env(Instant(at));
    let style = paperpaw_sdk::ui::UiStyle::new(&env);
    let region = w
        .kernel
        .stack
        .last()
        .unwrap()
        .screen
        .region(&env)
        .expect("a dialog has a region");
    (
        region,
        i32::from(style.row()),
        i32::from(style.gap()),
        i32::from(paperpaw_sdk::ui::Pager::height(&style)),
    )
}

/// A tap: down, then up, then what changed drawn.
fn tap_at(w: &mut World, at: u64, x: i32, y: i32) {
    use paperpaw_platform::{InputEvent, Point, PointerEvent, PointerPhase, PointerTool};
    let event = |phase| {
        InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            Point::new(x, y),
            PointerTool::Finger,
            1000,
        ))
    };
    w.kernel.handle(Instant(at), event(PointerPhase::Down));
    w.kernel.handle(Instant(at + 60), event(PointerPhase::Up));
    w.kernel.render(&mut |_, _| {});
}

/// A tap on a line answers it. (On the small test screen two lines do not fit
/// one page; the paged lines are tapped in the pager test.)
#[test]
fn a_tap_on_a_line_of_a_host_choice_answers_it() {
    let mut w = world(Ok(OK));
    ask_with(&w, 1, 0, "only", 1);
    w.kernel.native_dialog_pump();
    let (region, row, gap, _) = geometry(&w, 10);
    // The last line sits above the gap at the foot of the sheet.
    let x = i32::from(region.x) + i32::from(region.width) / 2;
    let y = i32::from(region.y) + i32::from(region.height) - gap - row / 2;
    tap_at(&mut w, 100, x, y);
    w.kernel.native_dialog_pump();
    // The dialog is gone by then: bit 0 may be set, the answer is the same.
    let seen: Vec<(u32, u32, u32)> = answers(&w).iter().map(|a| (a.0, a.1, a.2)).collect();
    assert_eq!(seen, [(1, 0, 0)]);
}

/// More lines than the screen holds go on pages: the pager turns the page and
/// a tap on the first line of the second page answers its own index.
#[test]
fn a_pager_tap_turns_the_page_of_a_host_choice() {
    let mut w = world(Ok(OK));
    let lines: Vec<String> = (0..32).map(|n| std::format!("line {n}")).collect();
    ask_with(&w, 1, 0, &lines.join("\n"), 1);
    w.kernel.native_dialog_pump();
    let (region, row, gap, pager) = geometry(&w, 10);
    assert!(pager > 0, "the lines do not fit the screen: a pager shows");
    let (x, rx) = (
        i32::from(region.x) + i32::from(region.width) / 2,
        i32::from(region.x) + i32::from(region.width) * 3 / 4,
    );
    let foot = i32::from(region.y) + i32::from(region.height) - gap;
    // Next page: the right half of the pager below the lines.
    tap_at(&mut w, 100, rx, foot - pager / 2);
    // The first line of the second page: after the title row.
    let first = i32::from(region.y) + gap + row + row / 2;
    tap_at(&mut w, 300, x, first);
    w.kernel.native_dialog_pump();
    let seen = answers(&w);
    assert_eq!(seen.len(), 1, "one answer: {seen:?}");
    let (kind, outcome, choice, _) = seen[0];
    assert_eq!((kind, outcome), (1, 0));
    // The page holds what fits; the first line of the second one follows them.
    let shown = ((i32::from(w.kernel.shared.env(Instant(0)).device().display.height)
        - (row + gap * 2)
        - pager)
        / row)
        .max(1);
    assert_eq!(i64::from(choice), i64::from(shown));
}

/// A title of many lines is drawn, shortened, and the lines still answer.
#[test]
fn a_long_title_does_not_stop_a_host_choice_or_confirm() {
    use paperpaw_sdk::builtin::actions;
    let long = "a very long title that goes on and on ".repeat(30);
    let mut w = world(Ok(OK));
    ask_titled(&w, 1, &long, "one\ntwo");
    w.kernel.native_dialog_pump();
    w.kernel.render(&mut |_, _| {});
    act(&mut w, 10, actions::DOWN);
    act(&mut w, 20, actions::DOWN);
    act(&mut w, 30, actions::CONFIRM);
    w.kernel.native_dialog_pump();
    assert_eq!(answers(&w), [(1, 0, 1, 0)]);
    ask_titled(&w, 2, &long, "");
    w.kernel.native_dialog_pump();
    w.kernel.render(&mut |_, _| {});
    act(&mut w, 40, actions::BACK);
    w.kernel.native_dialog_pump();
    assert_eq!(answers(&w).last().copied(), Some((2, 1, 0, 1)));
}

/// A refused answer that is no fault (the request was answered already) is not
/// logged; one the store holds no line for is.
#[test]
fn a_second_answer_is_not_logged_as_refused() {
    let mut w = world(Ok(OK));
    let id = ask_with(&w, 1, 0, "one\ntwo", 1);
    w.kernel.native_dialog_pump();
    let top = w.kernel.stack.last().unwrap().serial;
    let to = crate::native_host::dialogs::Answerer::new(w.store.try_clone().unwrap(), top, id);
    to.choose(0, 2);
    // Answered; the second answer finds it done.
    to.choose(1, 2);
    w.kernel.native_dialog_pump();
    assert!(
        w.kernel
            .take_warnings()
            .iter()
            .all(|warning| !warning.message.contains("was not taken"))
    );
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
