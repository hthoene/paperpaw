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

struct Dialog;

impl Screen for Dialog {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}
    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
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
                        params.get("core.request").map(String::from).unwrap_or_default(),
                        params.get("core.kind").map(String::from).unwrap_or_default(),
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
    OPENED.with(|opened| opened.borrow_mut().clear());
    let store = Shared::try_new(UiStore::new(Limits::ELSEWHERE)).unwrap();
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Serves)
        .build()
        .unwrap();
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
    let question = Question {
        kind,
        flags: 0,
        remote: false,
        title: "Title",
        text: "Question",
        choices: if kind == 1 { "one\ntwo" } else { "" },
    };
    let who = Stamp::new(w.owner, epoch).unwrap();
    w.store.ask(who, None, &question, false, None, false).unwrap().0
}

#[test]
fn the_dialog_opens_in_its_slot_after_the_asking_callback_returned() {
    let mut w = world(Ok(OK));
    let id = ask(&w, 0, 1);
    assert!(!w.kernel.native_dialog_pump());
    assert_eq!(w.kernel.active_screen(), Some(&KEYBOARD_SCREEN));
    // The params are ASCII decimal: the request and the kind.
    OPENED.with(|opened| {
        assert_eq!(*opened.borrow(), [(std::format!("{id}"), String::from("0"))]);
    });
    let top = w.kernel.stack.last().unwrap().serial;
    assert_eq!(w.store.dialog_of(top).map(|d| d.0), Some(id));
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
    w.store.callback_failed(Stamp::new(w.owner, 5).unwrap()).unwrap();
    w.kernel.native_dialog_pump();
    assert_eq!(w.kernel.active_screen().cloned(), before);
    OPENED.with(|opened| assert!(opened.borrow().is_empty()));
    let answered = w.seen.borrow().clone();
    assert_eq!((answered[0].request, answered[0].outcome, answered[0].flags & 2), (id, 1, 2));
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
        (answered[0].request, answered[0].kind, answered[0].outcome, answered[0].length),
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
