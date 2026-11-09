//! `event.widget_dropped` (85) reaches the widget's maker at the widget's
//! token after its screen closed; the entry ends at the first attempt whatever
//! came of it; an owner that cannot be called loses its notices without a
//! delivery; the record fits its buffer, and a buffer too small leaves the
//! notice listed.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::{
    HostResources,
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
    shared::Shared,
    ui_service::UiServices,
    ui_store::{Limits, UiStore},
};
use alloc::{rc::Rc, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_STATE, OK};
use paperpaw_sdk::Id;

const TOKEN: u32 = 41;

/// A native owner that logs the events it is called with.
struct Probe {
    log: Rc<RefCell<Vec<(u32, u32)>>>,
    answer: Rc<Cell<Result<i32, i32>>>,
    running: Cell<bool>,
}

impl Dispatch for Probe {
    fn event(&self, token: u32, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        let wire::Record::EventWidgetDropped(event) = wire::records_decode_single(bytes)? else {
            return Err(ERR_INVALID);
        };
        self.log.borrow_mut().push((token, event.instance));
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
    pool: HostResources,
    kernel: crate::Kernel,
    key: u64,
    log: Rc<RefCell<Vec<(u32, u32)>>>,
    answer: Rc<Cell<Result<i32, i32>>>,
}

fn world() -> World {
    let pool = HostResources::try_new().unwrap();
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
    let answer = Rc::new(Cell::new(Ok(OK)));
    let lease = NativeLease::try_new(Probe {
        log: Rc::clone(&log),
        answer: Rc::clone(&answer),
        running: Cell::new(true),
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
        kernel,
        key,
        log,
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
