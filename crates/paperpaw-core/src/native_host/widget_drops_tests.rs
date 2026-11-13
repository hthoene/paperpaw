//! `event.widget_dropped` (85) reaches the widget's maker at the widget's
//! token after its screen closed; the entry ends at the first attempt whatever
//! came of it; an owner that cannot be called loses its notices without a
//! delivery; the record fits its buffer, and a buffer too small leaves the
//! notice listed. `event.widget_made` (84) reaches the owner between
//! callbacks before the instance's drop, once, within the widget cap.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::ui_handles::NewWidget;
use super::{
    HostResources,
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
    shared::Shared,
    ui_service::UiServices,
    ui_store::{Limits, UiStore},
};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_FULL, ERR_INVALID, ERR_NO_SPACE, ERR_STATE, OK};
use paperpaw_sdk::Id;

const TOKEN: u32 = 41;

/// A made notice as the owner got it: token, instance, widget, place.
type MadeSeen = (u32, u32, String, String);

/// A native owner that logs the events it is called with.
struct Probe {
    log: Rc<RefCell<Vec<(u32, u32)>>>,
    made: Rc<RefCell<Vec<MadeSeen>>>,
    /// The tags of the events, in order.
    order: Rc<RefCell<Vec<u32>>>,
    answer: Rc<Cell<Result<i32, i32>>>,
    running: Cell<bool>,
    /// What the owner does inside its callback (given its running flag).
    hook: Hook,
}

type Hook = Rc<RefCell<Option<Rc<dyn Fn(&Cell<bool>)>>>>;

impl Dispatch for Probe {
    fn event(&self, token: u32, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        match wire::records_decode_single(bytes)? {
            wire::Record::EventWidgetDropped(event) => {
                self.order.borrow_mut().push(wire::EVENT_WIDGET_DROPPED_TAG);
                self.log.borrow_mut().push((token, event.instance));
            }
            wire::Record::EventWidgetMade(event) => {
                self.order.borrow_mut().push(wire::EVENT_WIDGET_MADE_TAG);
                self.made.borrow_mut().push((
                    token,
                    event.instance,
                    event.widget.into(),
                    event.place.into(),
                ));
            }
            _ => return Err(ERR_INVALID),
        }
        let hook = self.hook.borrow().clone();
        if let Some(hook) = hook {
            hook(&self.running);
        }
        self.answer.get()
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

impl NativeInstance for Probe {
    fn running(&self) -> bool {
        self.running.get()
    }
    fn stop(&self) {
        self.running.set(false);
    }
}

struct World {
    pool: Rc<HostResources>,
    hook: Hook,
    kernel: crate::Kernel,
    key: u64,
    log: Rc<RefCell<Vec<(u32, u32)>>>,
    made: Rc<RefCell<Vec<MadeSeen>>>,
    order: Rc<RefCell<Vec<u32>>>,
    answer: Rc<Cell<Result<i32, i32>>>,
}

fn world() -> World {
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
    let log = Rc::new(RefCell::new(Vec::new()));
    let made = Rc::new(RefCell::new(Vec::new()));
    let order = Rc::new(RefCell::new(Vec::new()));
    let answer = Rc::new(Cell::new(Ok(OK)));
    let hook: Hook = Rc::new(RefCell::new(None));
    let lease = NativeLease::try_new(Probe {
        log: Rc::clone(&log),
        made: Rc::clone(&made),
        order: Rc::clone(&order),
        answer: Rc::clone(&answer),
        running: Cell::new(true),
        hook: Rc::clone(&hook),
    })
    .unwrap();
    let key = kernel
        .shared
        .native_instances
        .insert(
            owner.try_view().unwrap(),
            Id::new_static("probe"),
            Some(lease),
            true,
        )
        .unwrap();
    World {
        pool,
        hook,
        kernel,
        key,
        log,
        made,
        order,
        answer,
    }
}

impl World {
    fn ui(&self) -> &UiServices {
        self.pool.hosts().ui().unwrap()
    }
    /// A widget the owner made for `screen`, whose screen then closed.
    fn dropped(&self, screen: u32) -> u32 {
        let id = self
            .ui()
            .widgets
            .handles
            .borrow_mut()
            .make_widget(self.key, self.key, screen, TOKEN, false)
            .unwrap();
        assert_eq!(self.ui().screen_closed(screen), Ok(1));
        id
    }
    /// An instance another plugin (99) made of the probe's widget for `screen`.
    fn made_for(&self, screen: u32, widget: &str, place: &str) -> Result<u32, i32> {
        let new = NewWidget {
            owner: self.key,
            token: TOKEN,
            private: false,
            widget,
            place,
        };
        self.ui()
            .widgets
            .handles
            .borrow_mut()
            .make_told(99, screen, &new)
    }
    fn made_waiting(&self) -> usize {
        self.ui().widgets.handles.borrow().made_waiting()
    }
    fn waiting(&self) -> Option<u64> {
        self.ui().next_dropped_owner().unwrap()
    }
}

#[test]
fn the_maker_hears_of_a_drop_after_its_screen_closed_at_the_widgets_token() {
    let mut w = world();
    let id = w.dropped(5);
    assert_eq!(w.waiting(), Some(w.key));
    // The commands round delivers it, once.
    w.kernel.run_commands();
    assert_eq!(&*w.log.borrow(), &[(TOKEN, id)]);
    assert_eq!(w.waiting(), None);
    w.kernel.run_commands();
    assert_eq!(w.log.borrow().len(), 1);
    // The slot is free again for the owner.
    let again = w
        .ui()
        .widgets
        .handles
        .borrow_mut()
        .make_widget(w.key, w.key, 6, TOKEN, false);
    assert!(again.is_ok());
}

#[test]
fn drops_are_told_oldest_first_each_once() {
    let mut w = world();
    let first = w.dropped(5);
    let second = w.dropped(6);
    assert!(w.kernel.deliver_widget_drops());
    assert_eq!(&*w.log.borrow(), &[(TOKEN, first), (TOKEN, second)]);
    assert!(!w.kernel.deliver_widget_drops());
}

#[test]
fn a_failed_or_trapped_delivery_ends_the_entry_with_no_retry() {
    for answer in [Ok(ERR_INVALID), Err(ERR_STATE), Err(ERR_NO_SPACE)] {
        let mut w = world();
        w.answer.set(answer);
        let id = w.dropped(5);
        w.kernel.run_commands();
        assert_eq!(&*w.log.borrow(), &[(TOKEN, id)], "{answer:?}");
        assert_eq!(w.waiting(), None, "{answer:?}: the entry ended");
        w.kernel.run_commands();
        assert_eq!(w.log.borrow().len(), 1, "{answer:?}: no retry");
    }
}

#[test]
fn an_owner_that_cannot_be_called_loses_its_notices_without_a_delivery() {
    // Switched off or unloaded: the instance is stopped before the pass.
    let mut w = world();
    w.dropped(5);
    w.kernel.shared.native_instances.stop(w.key).unwrap();
    w.kernel.run_commands();
    assert!(w.log.borrow().is_empty(), "a stopped owner is not called");
    assert_eq!(w.waiting(), None, "and does not pin the widget cap");
    // Its retirement discards what is left (`owner_retired`): no 'no memory'
    // can come of it, so the retiring caller has nothing to loop on.
    let w = world();
    w.dropped(5);
    assert_eq!(w.ui().plugin_stopped(w.key), Ok(()));
    assert_eq!(w.waiting(), None);
    // A notice of an owner no instance is registered for ends the same way.
    let mut w = world();
    w.ui()
        .widgets
        .handles
        .borrow_mut()
        .make_widget(999, 999, 5, TOKEN, false)
        .unwrap();
    assert_eq!(w.ui().screen_closed(5), Ok(1));
    assert_eq!(w.waiting(), Some(999));
    w.kernel.run_commands();
    assert_eq!(w.waiting(), None);
}

#[test]
fn the_record_fits_its_buffer_and_a_buffer_too_small_leaves_the_notice() {
    let w = world();
    // The largest values the record carries.
    {
        let mut handles = w.ui().widgets.handles.borrow_mut();
        handles
            .make_widget(w.key, w.key, u32::MAX, u32::MAX, false)
            .unwrap();
    }
    assert_eq!(w.ui().screen_closed(u32::MAX), Ok(1));
    let mut small = [0u8; 4];
    assert!(
        w.ui()
            .next_widget_dropped(w.key, u64::MAX, &mut small)
            .is_err()
    );
    assert_eq!(w.waiting(), Some(w.key), "the notice stays");
    let mut record = [0u8; 64];
    let (notice, len) = w
        .ui()
        .next_widget_dropped(w.key, u64::MAX, &mut record)
        .unwrap()
        .unwrap();
    assert!(len < 64 / 2, "the record is {len} bytes");
    w.ui().widget_dropped_delivered(&notice).unwrap();
    assert_eq!(w.waiting(), None);
}

#[test]
fn a_callback_that_drops_a_widget_or_stops_its_owner_ends_its_own_entry_once() {
    // The callback makes and drops a widget of its own: that notice is told
    // by a later round, and the entry being told is over exactly once.
    let mut w = world();
    let first = w.dropped(5);
    let pool = Rc::clone(&w.pool);
    let key = w.key;
    let more = Rc::new(Cell::new(true));
    *w.hook.borrow_mut() = Some(Rc::new(move |_| {
        if more.replace(false) {
            let ui = pool.hosts().ui().unwrap();
            ui.widgets
                .handles
                .borrow_mut()
                .make_widget(key, key, 6, TOKEN, false)
                .unwrap();
            assert_eq!(ui.screen_closed(6), Ok(1));
        }
    }));
    w.kernel.run_commands();
    let told = w.log.borrow().len();
    assert!((1..=2).contains(&told), "{told} told");
    assert_eq!(w.log.borrow()[0].1, first);
    w.kernel.run_commands();
    assert_eq!(w.log.borrow().len(), 2, "the second drop is told, once");
    assert_eq!(w.waiting(), None);
    w.kernel.run_commands();
    assert_eq!(w.log.borrow().len(), 2);

    // The callback stops its own owner: the entry ends with the call, and the
    // notice that is left is dropped with no second call.
    let mut w = world();
    w.dropped(5);
    w.dropped(6);
    *w.hook.borrow_mut() = Some(Rc::new(|running: &Cell<bool>| running.set(false)));
    w.kernel.run_commands();
    assert_eq!(
        w.log.borrow().len(),
        1,
        "a stopped owner is not called again"
    );
    assert_eq!(w.waiting(), None, "and does not pin the widget cap");
}

#[test]
fn the_owner_hears_of_a_made_instance_between_callbacks_before_its_drop() {
    let mut w = world();
    let id = w.made_for(5, "probe.w@2", "home").unwrap();
    assert_eq!(w.ui().screen_closed(5), Ok(1));
    w.kernel.run_commands();
    assert_eq!(
        &*w.made.borrow(),
        &[(TOKEN, id, String::from("probe.w@2"), String::from("home"))]
    );
    assert_eq!(&*w.log.borrow(), &[(TOKEN, id)]);
    assert_eq!(
        &*w.order.borrow(),
        &[wire::EVENT_WIDGET_MADE_TAG, wire::EVENT_WIDGET_DROPPED_TAG],
        "made before dropped"
    );
    assert_eq!(w.made_waiting(), 0);
    w.kernel.run_commands();
    assert_eq!(w.order.borrow().len(), 2, "each told once");
}

#[test]
fn a_made_notice_ends_at_its_first_attempt_or_with_its_owner() {
    let mut w = world();
    w.answer.set(Err(ERR_STATE));
    w.made_for(5, "probe.w", "").unwrap();
    assert!(w.kernel.deliver_widget_made());
    assert_eq!(w.made.borrow().len(), 1);
    assert!(!w.kernel.deliver_widget_made(), "no retry");
    // An owner that cannot be called is not, and its notice ends.
    let mut w = world();
    w.made_for(5, "probe.w", "").unwrap();
    w.kernel.shared.native_instances.stop(w.key).unwrap();
    assert!(!w.kernel.deliver_widget_made());
    assert!(w.made.borrow().is_empty());
    assert_eq!(w.made_waiting(), 0);
    // A stopped owner's notices go with it.
    let w = world();
    w.made_for(5, "probe.w", "").unwrap();
    assert_eq!(w.ui().plugin_stopped(w.key), Ok(()));
    assert_eq!(w.made_waiting(), 0);
}

#[test]
fn notices_waiting_are_held_to_the_widget_cap() {
    let mut w = world();
    for screen in 1..=32 {
        w.made_for(screen, "probe.w", "").unwrap();
        assert_eq!(w.ui().screen_closed(screen), Ok(1));
    }
    // The drops alone are told: no instance and no drop holds the cap now.
    assert!(w.kernel.deliver_widget_drops());
    assert_eq!(w.waiting(), None);
    assert_eq!(w.made_for(40, "probe.w", ""), Err(ERR_FULL));
    assert!(w.kernel.deliver_widget_made());
    assert_eq!(w.made.borrow().len(), 32);
    assert!(w.made_for(40, "probe.w", "").is_ok());
}

#[test]
fn the_largest_made_record_fits_its_buffer() {
    let w = world();
    let widget = "w".repeat(128);
    let place = "p".repeat(128);
    w.made_for(u32::MAX, &widget, &place).unwrap();
    let mut record = [0u8; super::widget_drops::MADE_BYTES];
    let (notice, len) =
        super::widget_drops::made_record(&w.ui().widgets.handles, None, u64::MAX, &mut record)
            .unwrap()
            .unwrap();
    assert_eq!(notice.owner, w.key);
    let wire::Record::EventWidgetMade(event) = wire::records_decode_single(&record[..len]).unwrap()
    else {
        panic!("not event 84")
    };
    assert_eq!((event.widget, event.place), (&*widget, &*place));
}
