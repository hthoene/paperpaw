//! The kernel's events (tick, started, resume, suspend, stopping,
//! battery_measured, page_turned) to the native plugins that subscribed: who
//! hears them, never inside a callback, one order, the bounds, and a stopped
//! plugin.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use crate::native_host::{
    codec,
    commands::CommandStage,
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_STATE, OK};
use paperpaw_sdk::{Id, Params, builtin::events as builtin};

const ALL: &[u32] = &[
    wire::EVENT_TICK_TAG,
    wire::EVENT_STARTED_TAG,
    wire::EVENT_RESUME_TAG,
    wire::EVENT_SUSPEND_TAG,
    wire::EVENT_STOPPING_TAG,
    wire::EVENT_BATTERY_MEASURED_TAG,
    wire::EVENT_PAGE_TURNED_TAG,
];

/// What every plugin was told: the plugin, the tag and the clock of the record.
type Told = Rc<RefCell<Vec<(char, u32, u64)>>>;

struct Probe {
    name: char,
    tags: &'static [u32],
    told: Told,
    /// The page titles it was told of.
    titles: Rc<RefCell<Vec<String>>>,
    /// Fails every call when set.
    fails: Rc<Cell<bool>>,
}

impl NativeInstance for Probe {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        self.tags.contains(&tag)
    }
    fn service_event(&self, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        if self.fails.get() {
            return Err(ERR_STATE);
        }
        if wire::RawRecord::single(bytes).unwrap().tag() == wire::EVENT_TICK_TAG {
            let now = wire::tick_decode(bytes).unwrap();
            self.told
                .borrow_mut()
                .push((self.name, wire::EVENT_TICK_TAG, now));
            return Ok(OK);
        }
        let record = codec::decode_any_single(bytes).unwrap();
        let (tag, now) = match record {
            wire::Record::EventStarted(event) => (wire::EVENT_STARTED_TAG, event.now_ms),
            wire::Record::EventResume(event) => (wire::EVENT_RESUME_TAG, event.now_ms),
            wire::Record::EventSuspend(event) => (wire::EVENT_SUSPEND_TAG, event.now_ms),
            wire::Record::EventStopping(event) => (wire::EVENT_STOPPING_TAG, event.now_ms),
            wire::Record::EventBatteryMeasured(event) => {
                (wire::EVENT_BATTERY_MEASURED_TAG, event.now_ms)
            }
            wire::Record::EventBookOpened(event) => (wire::EVENT_BOOK_OPENED_TAG, event.now_ms),
            wire::Record::EventBookClosed(event) => (wire::EVENT_BOOK_CLOSED_TAG, event.now_ms),
            wire::Record::EventBookFinished(event) => (wire::EVENT_BOOK_FINISHED_TAG, event.now_ms),
            wire::Record::EventBookMoved(event) => {
                self.titles
                    .borrow_mut()
                    .push(alloc::format!("{}>{}", event.from, event.path));
                (wire::EVENT_BOOK_MOVED_TAG, event.now_ms)
            }
            wire::Record::EventPageTurned(event) => {
                self.titles.borrow_mut().push(event.title.into());
                (wire::EVENT_PAGE_TURNED_TAG, event.now_ms)
            }
            _ => return Err(ERR_INVALID),
        };
        self.told.borrow_mut().push((self.name, tag, now));
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
    a: u64,
    b: u64,
    told: Told,
    titles: Rc<RefCell<Vec<String>>>,
    fails: Rc<Cell<bool>>,
}

/// Plugin `a` hears the tags `a_tags`, plugin `b` the tags `b_tags`.
fn world(a_tags: &'static [u32], b_tags: &'static [u32]) -> World {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let told = Told::default();
    let titles = Rc::new(RefCell::new(Vec::new()));
    let fails = Rc::new(Cell::new(false));
    let mut add = |name: char, tags: &'static [u32]| {
        let probe = Probe {
            name,
            tags,
            told: Rc::clone(&told),
            titles: Rc::clone(&titles),
            fails: Rc::clone(&fails),
        };
        kernel
            .shared
            .native_instances
            .test_insert(
                Id::try_parse(&String::from(name)).unwrap(),
                Some(NativeLease::try_new(probe).unwrap()),
                true,
            )
            .unwrap()
    };
    let a = add('a', a_tags);
    let b = add('b', b_tags);
    World {
        kernel,
        a,
        b,
        told,
        titles,
        fails,
    }
}

fn page(title: &str) -> Params {
    let mut params = Params::new();
    for (key, value) in [
        ("section", "2"),
        ("offset", "10"),
        ("progress", "0.5"),
        ("path", "/books/a.epub"),
        ("title", title),
        ("chapter", "One"),
    ] {
        params.set(key, value);
    }
    params
}

/// Raises the kernel event of wire tag `tag`.
fn raise(kernel: &mut crate::Kernel, tag: u32, params: &Params) {
    let event: Id = match tag {
        wire::EVENT_TICK_TAG => super::super::native_tick::TICK.id().clone(),
        wire::EVENT_STARTED_TAG => builtin::STARTED.id().clone(),
        wire::EVENT_RESUME_TAG => builtin::RESUME.id().clone(),
        wire::EVENT_SUSPEND_TAG => builtin::SUSPEND.id().clone(),
        wire::EVENT_STOPPING_TAG => builtin::STOPPING.id().clone(),
        wire::EVENT_BATTERY_MEASURED_TAG => builtin::BATTERY_MEASURED.id().clone(),
        wire::EVENT_BOOK_OPENED_TAG => builtin::BOOK_OPENED.id().clone(),
        wire::EVENT_BOOK_CLOSED_TAG => builtin::BOOK_CLOSED.id().clone(),
        wire::EVENT_BOOK_FINISHED_TAG => builtin::BOOK_FINISHED.id().clone(),
        wire::EVENT_BOOK_MOVED_TAG => builtin::BOOK_MOVED.id().clone(),
        _ => builtin::PAGE_TURNED.id().clone(),
    };
    kernel.emit(&event, params);
    // The pump tells what waits between callbacks (suspend and stopping were
    // told by the drain already).
    if kernel.shared.native_instances.depth() == 0 {
        while kernel.native_setting_pump() {}
    }
}

fn tags(w: &World, name: char) -> Vec<u32> {
    w.told
        .borrow()
        .iter()
        .filter(|(who, _, _)| *who == name)
        .map(|(_, tag, _)| *tag)
        .collect()
}

#[test]
fn each_kernel_event_reaches_a_subscribed_plugin_and_an_unsubscribed_one_hears_nothing() {
    let mut w = world(ALL, &[]);
    for &tag in ALL {
        raise(&mut w.kernel, tag, &page("T"));
        assert_eq!(tags(&w, 'a').last(), Some(&tag), "tag {tag}");
    }
    assert_eq!(tags(&w, 'a'), ALL);
    assert!(tags(&w, 'b').is_empty());
    assert_eq!(*w.titles.borrow(), ["T"]);
}

#[test]
fn a_plugin_hears_only_the_events_it_subscribed_to() {
    let mut w = world(&[wire::EVENT_SUSPEND_TAG], &[wire::EVENT_TICK_TAG]);
    for &tag in ALL {
        raise(&mut w.kernel, tag, &page("T"));
    }
    assert_eq!(tags(&w, 'a'), [wire::EVENT_SUSPEND_TAG]);
    assert_eq!(tags(&w, 'b'), [wire::EVENT_TICK_TAG]);
}

#[test]
fn an_event_raised_during_a_callback_arrives_after_it_returns() {
    let mut w = world(ALL, &[]);
    w.kernel.shared.native_instances.test_enter_callback();
    raise(&mut w.kernel, wire::EVENT_STARTED_TAG, &Params::new());
    raise(&mut w.kernel, wire::EVENT_SUSPEND_TAG, &Params::new());
    assert!(
        tags(&w, 'a').is_empty(),
        "nothing is called inside a callback"
    );
    // The pump asks for another pass while the callback is on the stack.
    assert!(w.kernel.native_setting_pump());
    assert!(tags(&w, 'a').is_empty());
    w.kernel.shared.native_instances.test_leave_callback();
    while w.kernel.native_setting_pump() {}
    assert_eq!(
        tags(&w, 'a'),
        [wire::EVENT_STARTED_TAG, wire::EVENT_SUSPEND_TAG],
        "in the order raised"
    );
}

#[test]
fn tick_and_resume_raised_before_they_were_told_are_one_each() {
    let mut w = world(ALL, &[]);
    w.kernel.shared.native_instances.test_enter_callback();
    for _ in 0..5 {
        raise(&mut w.kernel, wire::EVENT_TICK_TAG, &Params::new());
        raise(&mut w.kernel, wire::EVENT_RESUME_TAG, &Params::new());
    }
    w.kernel.shared.native_instances.test_leave_callback();
    while w.kernel.native_setting_pump() {}
    assert_eq!(
        tags(&w, 'a'),
        [wire::EVENT_TICK_TAG, wire::EVENT_RESUME_TAG],
        "once each, in the order of the last time"
    );
}

#[test]
fn the_order_holds_with_the_store_full_and_a_drop_never_overtakes() {
    let mut w = world(ALL, &[]);
    w.kernel.shared.native_instances.test_enter_callback();
    // Sixteen edges fill the store; the next two are dropped.
    for _ in 0..18 {
        raise(
            &mut w.kernel,
            wire::EVENT_BATTERY_MEASURED_TAG,
            &Params::new(),
        );
    }
    // Suspend and stopping never wait for room in the store.
    raise(&mut w.kernel, wire::EVENT_SUSPEND_TAG, &Params::new());
    raise(&mut w.kernel, wire::EVENT_STOPPING_TAG, &Params::new());
    w.kernel.shared.native_instances.test_leave_callback();
    while w.kernel.native_setting_pump() {}
    let told = tags(&w, 'a');
    assert_eq!(told.len(), 18, "sixteen edges, then suspend and stopping");
    assert!(
        told[..16]
            .iter()
            .all(|&tag| tag == wire::EVENT_BATTERY_MEASURED_TAG)
    );
    assert_eq!(
        told[16..],
        [wire::EVENT_SUSPEND_TAG, wire::EVENT_STOPPING_TAG]
    );
}

#[test]
fn an_edge_never_overtakes_one_held_for_the_plugin() {
    let mut w = world(ALL, &[]);
    w.kernel.shared.native_instances.test_enter_callback();
    raise(&mut w.kernel, wire::EVENT_STARTED_TAG, &Params::new());
    w.kernel.shared.native_instances.test_leave_callback();
    // Raised outside a callback with an edge still held: it goes behind it.
    raise(
        &mut w.kernel,
        wire::EVENT_BATTERY_MEASURED_TAG,
        &Params::new(),
    );
    assert_eq!(
        tags(&w, 'a'),
        [wire::EVENT_STARTED_TAG, wire::EVENT_BATTERY_MEASURED_TAG]
    );
}

#[test]
fn a_page_turn_that_says_what_cannot_be_is_dropped_and_logged() {
    let mut w = world(ALL, &[]);
    // A page turn without its location cannot be encoded.
    raise(&mut w.kernel, wire::EVENT_PAGE_TURNED_TAG, &Params::new());
    assert!(tags(&w, 'a').is_empty());
    // The next is told.
    raise(&mut w.kernel, wire::EVENT_PAGE_TURNED_TAG, &page("ok"));
    assert_eq!(tags(&w, 'a'), [wire::EVENT_PAGE_TURNED_TAG]);
}

#[test]
fn a_stopped_plugin_gets_nothing() {
    let mut w = world(ALL, ALL);
    let owner = w.a;
    w.kernel.shared.native_instances.stop(owner).unwrap();
    for &tag in ALL {
        raise(&mut w.kernel, tag, &page("T"));
    }
    assert!(tags(&w, 'a').is_empty());
    assert_eq!(tags(&w, 'b'), ALL);
    let _ = w.b;
}

#[test]
fn a_plugin_that_stops_while_an_event_waits_loses_it() {
    let mut w = world(ALL, &[]);
    w.kernel.shared.native_instances.test_enter_callback();
    raise(&mut w.kernel, wire::EVENT_STOPPING_TAG, &Params::new());
    let owner = w.a;
    w.kernel.shared.native_instances.stop(owner).unwrap();
    w.kernel.shared.native_instances.test_leave_callback();
    while w.kernel.native_setting_pump() {}
    assert!(tags(&w, 'a').is_empty());
}

#[test]
fn a_failing_callback_is_not_told_the_event_again() {
    let mut w = world(ALL, &[]);
    w.fails.set(true);
    raise(&mut w.kernel, wire::EVENT_STARTED_TAG, &Params::new());
    w.fails.set(false);
    while w.kernel.native_setting_pump() {}
    assert!(tags(&w, 'a').is_empty());
}

#[test]
fn only_the_plugin_an_event_is_for_hears_it() {
    let mut w = world(ALL, ALL);
    w.kernel
        .native_core_event(&builtin::RESUME.id().clone(), &Params::new(), &|plugin| {
            plugin == "b"
        });
    while w.kernel.native_setting_pump() {}
    assert!(tags(&w, 'a').is_empty());
    assert_eq!(tags(&w, 'b'), [wire::EVENT_RESUME_TAG]);
}

#[test]
fn an_event_that_is_not_the_kernels_is_left_alone() {
    let mut w = world(ALL, ALL);
    w.kernel
        .native_core_event(&Id::try_parse("x.other").unwrap(), &Params::new(), &|_| {
            true
        });
    assert!(w.told.borrow().is_empty());
    assert!(!w.kernel.native_setting_pump());
}

#[test]
fn the_real_lifecycle_reaches_a_subscribed_plugin_in_order() {
    let mut w = world(ALL, &[]);
    let now = paperpaw_platform::Instant(1_000);
    w.kernel.suspend(now);
    w.kernel.stop(now);
    assert_eq!(
        tags(&w, 'a'),
        [wire::EVENT_SUSPEND_TAG, wire::EVENT_STOPPING_TAG]
    );
}

#[test]
fn only_suspend_and_stopping_are_told_before_the_pump_runs() {
    let mut w = world(ALL, &[]);
    let env_event = |tag: u32| match tag {
        wire::EVENT_STARTED_TAG => builtin::STARTED.id().clone(),
        wire::EVENT_BATTERY_MEASURED_TAG => builtin::BATTERY_MEASURED.id().clone(),
        wire::EVENT_SUSPEND_TAG => builtin::SUSPEND.id().clone(),
        _ => builtin::STOPPING.id().clone(),
    };
    w.kernel
        .emit(&env_event(wire::EVENT_STARTED_TAG), &Params::new());
    w.kernel
        .emit(&env_event(wire::EVENT_BATTERY_MEASURED_TAG), &Params::new());
    assert!(tags(&w, 'a').is_empty(), "queued, not called from emit_to");
    // Suspend drains the plugin's queue first: earlier edges, then itself.
    w.kernel
        .emit(&env_event(wire::EVENT_SUSPEND_TAG), &Params::new());
    assert_eq!(
        tags(&w, 'a'),
        [
            wire::EVENT_STARTED_TAG,
            wire::EVENT_BATTERY_MEASURED_TAG,
            wire::EVENT_SUSPEND_TAG
        ]
    );
}

#[test]
fn the_drain_at_stop_does_not_run_inside_a_callback() {
    let mut w = world(ALL, &[]);
    w.kernel.shared.native_instances.test_enter_callback();
    w.kernel
        .emit(&builtin::STOPPING.id().clone(), &Params::new());
    assert!(tags(&w, 'a').is_empty());
    w.kernel.shared.native_instances.test_leave_callback();
    while w.kernel.native_setting_pump() {}
    assert_eq!(tags(&w, 'a'), [wire::EVENT_STOPPING_TAG]);
}

#[test]
fn a_full_store_logs_once_until_the_queue_is_empty_again() {
    let mut w = world(ALL, &[]);
    w.kernel.take_warnings();
    w.kernel.shared.native_instances.test_enter_callback();
    // Sixteen fill the store; the next ten are dropped: one line, static.
    for _ in 0..26 {
        raise(
            &mut w.kernel,
            wire::EVENT_BATTERY_MEASURED_TAG,
            &Params::new(),
        );
    }
    let warnings = w.kernel.take_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    w.kernel.shared.native_instances.test_leave_callback();
    while w.kernel.native_setting_pump() {}
    assert_eq!(tags(&w, 'a').len(), 16);
    // Drained: a new drop is logged again.
    w.kernel.shared.native_instances.test_enter_callback();
    for _ in 0..17 {
        raise(
            &mut w.kernel,
            wire::EVENT_BATTERY_MEASURED_TAG,
            &Params::new(),
        );
    }
    assert_eq!(w.kernel.take_warnings().len(), 1);
}

#[test]
fn the_drain_at_suspend_tells_every_waiting_plugin_in_order() {
    let mut w = world(ALL, ALL);
    w.kernel.shared.native_instances.test_enter_callback();
    raise(&mut w.kernel, wire::EVENT_STARTED_TAG, &Params::new());
    w.kernel.shared.native_instances.test_leave_callback();
    w.kernel
        .emit(&builtin::SUSPEND.id().clone(), &Params::new());
    let told = w.told.borrow().clone();
    assert_eq!(
        told.iter()
            .map(|(who, tag, _)| (*who, *tag))
            .collect::<Vec<_>>(),
        [
            ('a', wire::EVENT_STARTED_TAG),
            ('a', wire::EVENT_SUSPEND_TAG),
            ('b', wire::EVENT_STARTED_TAG),
            ('b', wire::EVENT_SUSPEND_TAG)
        ]
    );
}

const BOOKS: &[u32] = &[
    wire::EVENT_BOOK_OPENED_TAG,
    wire::EVENT_BOOK_CLOSED_TAG,
    wire::EVENT_BOOK_FINISHED_TAG,
    wire::EVENT_SUSPEND_TAG,
];

#[test]
fn a_book_event_without_a_location_is_not_told_and_is_logged_once() {
    let mut w = world(BOOKS, &[]);
    w.kernel.take_warnings();
    // No location says nothing true to tell (a zero would be a real place):
    // it is dropped, with one line in the log until the queue is empty again.
    for _ in 0..3 {
        raise(&mut w.kernel, wire::EVENT_BOOK_CLOSED_TAG, &Params::new());
    }
    assert!(tags(&w, 'a').is_empty());
    assert_eq!(w.kernel.take_warnings().len(), 1);
}

/// The params of a move of a book from `from` to `path`.
fn moved(from: &str, path: &str) -> Params {
    use paperpaw_sdk::event::EventData;
    builtin::BookMoved::new(from.into(), path.into()).to_params()
}

#[test]
fn a_moved_book_is_told_to_its_subscribers_with_both_paths_and_to_no_one_else() {
    let mut w = world(&[wire::EVENT_BOOK_MOVED_TAG], &[wire::EVENT_STARTED_TAG]);
    raise(
        &mut w.kernel,
        wire::EVENT_BOOK_MOVED_TAG,
        &moved("books/a.txt", "books/new/a.txt"),
    );
    assert_eq!(tags(&w, 'a'), [wire::EVENT_BOOK_MOVED_TAG]);
    assert!(tags(&w, 'b').is_empty());
    assert_eq!(*w.titles.borrow(), ["books/a.txt>books/new/a.txt"]);
    // Told: nothing waits.
    assert!(
        w.kernel
            .shared
            .native_instances
            .setting_events
            .waiting()
            .is_empty()
    );
}

#[test]
fn a_moved_book_with_a_path_over_the_cap_is_dropped_whole_and_logged_once_not_cut() {
    let mut w = world(&[wire::EVENT_BOOK_MOVED_TAG], &[]);
    w.kernel.take_warnings();
    let long = "a".repeat(513);
    for _ in 0..2 {
        raise(
            &mut w.kernel,
            wire::EVENT_BOOK_MOVED_TAG,
            &moved(&long, "b.txt"),
        );
        raise(
            &mut w.kernel,
            wire::EVENT_BOOK_MOVED_TAG,
            &moved("a.txt", &long),
        );
    }
    assert!(tags(&w, 'a').is_empty(), "never told with a cut path");
    assert_eq!(w.kernel.take_warnings().len(), 1, "logged once");
    // The caps themselves still pass.
    let edge = "a".repeat(512);
    raise(
        &mut w.kernel,
        wire::EVENT_BOOK_MOVED_TAG,
        &moved(&edge, &edge),
    );
    assert_eq!(tags(&w, 'a'), [wire::EVENT_BOOK_MOVED_TAG]);
}

#[test]
fn a_move_without_both_paths_is_dropped_without_a_panic() {
    let mut w = world(&[wire::EVENT_BOOK_MOVED_TAG], &[]);
    let mut params = Params::new();
    params.set("from", "books/a.txt");
    raise(&mut w.kernel, wire::EVENT_BOOK_MOVED_TAG, &params);
    assert!(tags(&w, 'a').is_empty());
}
