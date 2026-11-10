//! Taps queued by op 6 reach the widget's owner between callbacks: once, with
//! the callback's stage, and never after the instance dropped, the owner
//! stopped or a guard came up over the forwarding screen.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use crate::native_host::{
    HostResources,
    commands::{CommandStage, WireCommand, commands_decode, commands_encode},
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
    shared::Shared,
    stack_rows::{GUARDS, StackRow},
    ui_service::UiServices,
    ui_store::{Limits, UiStore},
    ui_widget_layout::PendingInput,
};
use alloc::{rc::Rc, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_platform::Instant;
use paperpaw_sdk::Id;

const TOKEN: u32 = 41;
/// The plugin whose screen forwards the taps.
const WHO: u64 = 700;

/// What the owner's `input` callback saw: token, instance, target, x, y, kind.
type Seen = (u32, u32, u32, i32, i32, u32);

struct Owner {
    pool: Rc<HostResources>,
    drops_own: Rc<Cell<bool>>,
    seen: Rc<RefCell<Vec<Seen>>>,
    fail: Rc<Cell<bool>>,
    staged: Rc<Cell<Option<i32>>>,
}

impl NativeInstance for Owner {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn input_staged(
        &self,
        token: u32,
        bytes: &[u8],
        _: &paperpaw_sdk::Env<'_>,
        stage: &mut CommandStage<'_>,
    ) -> Result<i32, i32> {
        let wire::Record::InputWidget(tap) = crate::native_host::codec::decode_any_single(bytes)?
        else {
            return Err(ERR_INVALID);
        };
        self.seen
            .borrow_mut()
            .push((token, tap.instance, tap.target, tap.x, tap.y, tap.kind));
        if self.drops_own.get() {
            // It drops the very instance it was told of, inside the callback.
            let ui = self.pool.hosts().ui().unwrap();
            ui.widgets
                .handles
                .borrow_mut()
                .drop_widget(WHO, tap.instance)
                .unwrap();
        }
        // The callback has its command stage: a command is accepted in it.
        let mut batch = [0; 64];
        let len = commands_encode(
            &[WireCommand::WidgetChanged {
                instance: tap.instance,
            }],
            &mut batch,
        )
        .unwrap();
        self.staged
            .set(Some(stage.stage(&commands_decode(&batch[..len]).unwrap())));
        if self.fail.get() {
            Err(ERR_INVALID)
        } else {
            Ok(OK)
        }
    }
}

impl Dispatch for Owner {
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
    pool: Rc<HostResources>,
    kernel: crate::Kernel,
    key: u64,
    seen: Rc<RefCell<Vec<Seen>>>,
    fail: Rc<Cell<bool>>,
    drops_own: Rc<Cell<bool>>,
    staged: Rc<Cell<Option<i32>>>,
}

/// A kernel with the widget table and an owner plugin; `lease` false leaves
/// the owner not started (it cannot be called).
fn world(lease: bool) -> World {
    let pool = Rc::new(HostResources::try_new().unwrap());
    pool.hosts()
        .wire_ui(UiServices::new(
            Shared::try_new(UiStore::new(Limits::ELSEWHERE)).unwrap(),
            false,
        ))
        .unwrap();
    let owner = pool.try_owner().unwrap();
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    // WHO's screens 1, 2 and 5 are on the stack, nobody guards.
    kernel
        .shared
        .stack_rows
        .update([1, 2, 5].map(|instance| StackRow {
            instance,
            owner: WHO,
            flags: 0,
        }));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (fail, staged) = (Rc::new(Cell::new(false)), Rc::new(Cell::new(None)));
    let drops_own = Rc::new(Cell::new(false));
    let lease = lease.then(|| {
        NativeLease::try_new(Owner {
            pool: Rc::clone(&pool),
            drops_own: Rc::clone(&drops_own),
            seen: Rc::clone(&seen),
            fail: Rc::clone(&fail),
            staged: Rc::clone(&staged),
        })
        .unwrap()
    });
    let key = kernel
        .shared
        .native_instances
        .insert(
            owner.try_view().unwrap(),
            Id::new_static("clock"),
            lease,
            true,
        )
        .unwrap();
    World {
        pool,
        kernel,
        key,
        seen,
        fail,
        drops_own,
        staged,
    }
}

impl World {
    fn ui(&self) -> &UiServices {
        self.pool.hosts().ui().unwrap()
    }

    /// An instance of the owner's widget, made by WHO's screen `screen`.
    fn made(&self, screen: u32, private: bool) -> u32 {
        self.ui()
            .widgets
            .handles
            .borrow_mut()
            .make_widget(self.key, WHO, screen, TOKEN, private)
            .unwrap()
    }

    fn tap(&self, instance: u32, x: i32) {
        self.ui()
            .widgets
            .inputs
            .borrow_mut()
            .push(PendingInput {
                who: WHO,
                owner: self.key,
                token: TOKEN,
                instance,
                target: 3,
                x,
                y: 9,
                kind: 0,
            })
            .unwrap();
    }

    fn pump(&mut self) -> bool {
        self.kernel.now = Instant(1_000);
        self.kernel.native_widget_pump()
    }

    fn waiting(&self) -> bool {
        matches!(self.ui().next_widget_input_owner(), Ok(Some(_)))
    }
}

#[test]
fn a_tap_reaches_its_owner_once_with_the_callbacks_stage() {
    let mut w = world(true);
    let a = w.made(5, false);
    w.tap(a, 12);
    assert!(!w.pump());
    assert_eq!(&*w.seen.borrow(), &[(TOKEN, a, 3, 12, 9, 0)]);
    assert_eq!(w.staged.get(), Some(0), "the callback had a stage");
    // Told once: nothing more until another is queued.
    assert!(!w.pump());
    assert_eq!(w.seen.borrow().len(), 1);
    assert!(!w.waiting());
}

#[test]
fn taps_go_in_order_a_few_a_pass_and_the_pump_asks_for_more() {
    let mut w = world(true);
    let a = w.made(5, false);
    for x in 0..6 {
        w.tap(a, x);
    }
    assert!(w.pump(), "four a pass, two wait");
    assert_eq!(w.seen.borrow().len(), 4);
    assert!(!w.pump());
    let xs: Vec<i32> = w.seen.borrow().iter().map(|seen| seen.3).collect();
    assert_eq!(xs, [0, 1, 2, 3, 4, 5]);
}

#[test]
fn a_guard_that_came_up_since_the_tap_was_queued_stops_it() {
    let mut w = world(true);
    // WHO's screen 1 shows, nobody guards: the tap is queued.
    w.kernel.shared.stack_rows.update([StackRow {
        instance: 1,
        owner: WHO,
        flags: 0,
    }]);
    let a = w.made(1, false);
    w.tap(a, 1);
    // Another plugin's guard comes up above it before the pump runs.
    w.kernel.shared.stack_rows.update([
        StackRow {
            instance: 1,
            owner: WHO,
            flags: 0,
        },
        StackRow {
            instance: 2,
            owner: WHO + 1,
            flags: GUARDS,
        },
    ]);
    assert!(!w.pump());
    assert!(w.seen.borrow().is_empty(), "nothing reaches the owner");
    assert!(!w.waiting(), "and the tap is gone, not retried");
    // A private instance is stopped too, under a guard.
    let p = w.made(1, true);
    w.tap(p, 2);
    w.pump();
    assert!(w.seen.borrow().is_empty());
}

#[test]
fn the_guards_own_plugin_still_reaches_its_widgets() {
    let mut w = world(true);
    w.kernel.shared.stack_rows.update([
        StackRow {
            instance: 1,
            owner: WHO,
            flags: 0,
        },
        StackRow {
            instance: 2,
            owner: WHO,
            flags: GUARDS,
        },
    ]);
    let a = w.made(2, true);
    w.tap(a, 4);
    w.pump();
    assert_eq!(w.seen.borrow().len(), 1);
}

#[test]
fn a_dropped_instance_or_an_owner_that_cannot_be_called_gets_nothing() {
    let mut w = world(true);
    let a = w.made(5, false);
    w.tap(a, 1);
    w.ui()
        .widgets
        .handles
        .borrow_mut()
        .drop_widget(WHO, a)
        .unwrap();
    w.pump();
    assert!(w.seen.borrow().is_empty());
    assert!(!w.waiting());
    // An owner that is not started: the tap is discarded, nobody is called.
    let mut w = world(false);
    let a = w.made(5, false);
    w.tap(a, 1);
    assert!(!w.pump());
    assert!(!w.waiting(), "a dead owner never pins the queue");
}

#[test]
fn a_failing_callback_is_not_told_again() {
    let mut w = world(true);
    w.fail.set(true);
    let a = w.made(5, false);
    w.tap(a, 1);
    assert!(!w.pump());
    assert_eq!(w.seen.borrow().len(), 1);
    assert!(!w.pump());
    assert_eq!(w.seen.borrow().len(), 1, "tried once");
}

#[test]
fn a_callback_that_drops_its_own_instance_is_not_told_again() {
    let mut w = world(true);
    w.drops_own.set(true);
    let a = w.made(5, false);
    w.tap(a, 1);
    assert!(!w.pump());
    assert_eq!(w.seen.borrow().len(), 1);
    assert!(!w.waiting());
    assert!(!w.pump());
    assert_eq!(w.seen.borrow().len(), 1, "tried once");
}

#[test]
fn a_forwarding_screen_that_closed_meanwhile_gets_no_delivery_even_without_a_guard() {
    let mut w = world(true);
    let a = w.made(5, false);
    w.tap(a, 1);
    // Screen 5 is gone from the stack before the pump runs.
    w.kernel.shared.stack_rows.update([StackRow {
        instance: 1,
        owner: WHO,
        flags: 0,
    }]);
    assert!(!w.pump());
    assert!(w.seen.borrow().is_empty());
    assert!(!w.waiting());
}
