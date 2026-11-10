//! `event.wifi_done` to the sender of the request: told once, to the sender
//! only, released for nobody, one plugin not holding up another.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use crate::native_host::{
    codec,
    commands::CommandStage,
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    radio_requests::{Outcome, reason},
    registrations as wire,
};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_sdk::Id;

/// What a plugin was told: request, ssid, status.
type Told = Rc<RefCell<Vec<(u32, String, i32)>>>;

struct Probe {
    subscribed: bool,
    told: Told,
}

impl NativeInstance for Probe {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        self.subscribed && tag == wire::EVENT_WIFI_DONE_TAG
    }
    fn service_event(&self, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        let wire::Record::EventWifiDone(done) = codec::decode_any_single(bytes).unwrap() else {
            return Err(ERR_INVALID);
        };
        self.told
            .borrow_mut()
            .push((done.request, done.ssid.into(), done.status));
        Ok(OK)
    }
    fn service_event_staged(
        &self,
        bytes: &[u8],
        env: &paperpaw_sdk::Env<'_>,
        _: &mut CommandStage<'_>,
    ) -> Result<i32, i32> {
        self.service_event(bytes, env)
    }
}

impl Dispatch for Probe {
    fn event(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &paperpaw_sdk::Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn dropped(&self, _: u32, _: i32) {}
}

struct World {
    kernel: crate::Kernel,
    a: (u64, Told),
    b: (u64, Told),
}

/// Plugins `a` and `b` (running, subscribed as given) and `c` (never started).
fn world(a_subscribes: bool, b_subscribes: bool) -> World {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let mut add = |name: &str, subscribed: bool, running: bool| {
        let told = Told::default();
        let lease = running.then(|| {
            NativeLease::try_new(Probe {
                subscribed,
                told: Rc::clone(&told),
            })
            .unwrap()
        });
        let owner = kernel
            .shared
            .native_instances
            .test_insert(Id::try_parse(name).unwrap(), lease, true)
            .unwrap();
        (owner, told)
    };
    let a = add("a", a_subscribes, true);
    let b = add("b", b_subscribes, true);
    add("c", true, false);
    World { kernel, a, b }
}

/// `owner`'s request `request` has ended with `reason`.
fn ended(w: &World, owner: &str, request: u32, ssid: &str, why: u32) {
    let mut table = w.kernel.radio_requests.borrow_mut();
    table
        .reserve(&Id::try_parse(owner).unwrap(), request, false, ssid)
        .unwrap();
    assert!(table.end(owner, request, Outcome::failed(why)));
}

fn run(w: &mut World) {
    while w.kernel.native_wifi_pump() {}
}

fn held(w: &World) -> usize {
    w.kernel.radio_requests.borrow().len()
}

#[test]
fn the_sender_is_told_once_and_its_entry_is_released() {
    let mut w = world(true, true);
    ended(&w, "a", 7, "home", reason::PASSWORD);
    run(&mut w);
    assert_eq!(
        *w.a.1.borrow(),
        [(7, "home".into(), paperpaw_native::host_glue::ERR_DENIED)]
    );
    assert!(w.b.1.borrow().is_empty(), "only the sender is told");
    assert_eq!(held(&w), 0);
    run(&mut w);
    assert_eq!(w.a.1.borrow().len(), 1, "never twice");
}

#[test]
fn a_request_that_has_not_ended_is_not_told() {
    let mut w = world(true, true);
    w.kernel
        .radio_requests
        .borrow_mut()
        .reserve(&Id::try_parse("a").unwrap(), 1, false, "home")
        .unwrap();
    assert!(!w.kernel.native_wifi_pump());
    assert!(w.a.1.borrow().is_empty());
    assert_eq!(held(&w), 1);
}

#[test]
fn a_plugin_that_did_not_subscribe_loses_nothing_it_holds_room_for() {
    let mut w = world(false, true);
    ended(&w, "a", 1, "home", reason::NOT_FOUND);
    ended(&w, "b", 1, "away", reason::NOT_FOUND);
    run(&mut w);
    assert!(w.a.1.borrow().is_empty());
    assert_eq!(w.b.1.borrow().len(), 1);
    assert_eq!(
        held(&w),
        0,
        "the entry of the silent plugin is released too"
    );
}

#[test]
fn a_plugin_that_stopped_is_not_called_and_its_entry_is_released() {
    let mut w = world(true, true);
    ended(&w, "a", 1, "home", reason::TIMEOUT);
    let owner = w.a.0;
    w.kernel.shared.native_instances.stop(owner).unwrap();
    run(&mut w);
    assert!(w.a.1.borrow().is_empty());
    assert_eq!(held(&w), 0);
}

#[test]
fn nothing_is_told_inside_a_callback() {
    let mut w = world(true, true);
    ended(&w, "a", 1, "home", reason::TIMEOUT);
    w.kernel.shared.native_instances.test_enter_callback();
    assert!(w.kernel.native_wifi_pump(), "asks for another pass");
    assert!(w.a.1.borrow().is_empty());
    assert_eq!(held(&w), 1);
}

#[test]
fn a_plugin_not_started_yet_is_started_and_told_later_without_holding_up_another() {
    let mut w = world(true, true);
    ended(&w, "c", 1, "home", reason::TIMEOUT);
    ended(&w, "a", 2, "away", reason::TIMEOUT);
    w.kernel.now = crate::Instant(10);
    assert!(!w.kernel.native_wifi_pump(), "held: nothing is due now");
    assert_eq!(w.a.1.borrow().len(), 1, "a is told although c waits");
    assert_eq!(held(&w), 1, "c keeps its entry");
    assert!(w.kernel.shared.native_instances.take_start().is_some());
    assert_eq!(w.kernel.native_wifi_due(), Some(1_010));
    // Before the deadline nothing is tried; at it, c is tried again.
    w.kernel.now = crate::Instant(500);
    assert!(!w.kernel.native_wifi_pump());
    assert!(w.kernel.shared.native_instances.take_start().is_none());
    w.kernel.now = crate::Instant(1_010);
    w.kernel.native_wifi_pump();
    assert!(w.kernel.shared.native_instances.take_start().is_some());
}

#[test]
fn the_due_time_is_none_when_nothing_waits() {
    let w = world(true, true);
    assert_eq!(w.kernel.native_wifi_due(), None);
}
