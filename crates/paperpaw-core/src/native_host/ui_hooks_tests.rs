//! The kernel calls the `core.ui` cleanup hooks at their lifecycle points:
//! screen close (with the sweep after a refusal), callback end, status
//! change (event 73 and the minute tick). Plugin stop and guard start are
//! covered in `host_services_tests` and `tests/guard.rs`.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::{
    HostResources,
    ui_service::UiServices,
    ui_store::{Limits, UiStore},
};
use alloc::rc::Rc;
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_sdk::Id;

const MAKER: u64 = 7;
const OWNER: u64 = 9;

struct World {
    pool: HostResources,
    kernel: crate::Kernel,
}

fn world() -> World {
    let pool = HostResources::try_new().unwrap();
    pool.hosts()
        .wire_ui(UiServices::new(
            Rc::new(UiStore::new(Limits::ELSEWHERE)),
            false,
        ))
        .unwrap();
    let owner = pool.try_owner().unwrap();
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel
        .shared
        .native_instances
        .adopt_pool(owner.try_view().unwrap())
        .unwrap();
    World { pool, kernel }
}

impl World {
    fn ui(&self) -> &UiServices {
        self.pool.hosts().ui().unwrap()
    }
    fn widget(&self, screen: u32) {
        self.ui()
            .widgets
            .handles
            .borrow_mut()
            .make_widget(OWNER, MAKER, screen, 1, false)
            .unwrap();
    }
    fn widgets(&self) -> usize {
        self.ui().widgets.handles.borrow().widgets()
    }
    fn cached(&self) -> bool {
        self.ui()
            .widgets
            .cache
            .borrow()
            .get(OWNER, "a.item", b"")
            .is_some()
    }
    fn cache(&self) {
        self.ui()
            .widgets
            .cache
            .borrow_mut()
            .put(OWNER, "a.item", b"", false, "v");
    }
}

#[test]
fn a_closing_screen_drops_its_widget_instances_and_not_another_screens() {
    let w = world();
    w.widget(8);
    w.widget(9);
    w.kernel.native_instance_closed(8).unwrap();
    assert_eq!(w.widgets(), 1);
    // Idempotent: closing it again changes nothing.
    w.kernel.native_instance_closed(8).unwrap();
    assert_eq!(w.widgets(), 1);
}

#[test]
fn a_refused_screen_close_is_swept_later_and_a_live_screen_is_kept() {
    let mut w = world();
    w.widget(8);
    w.widget(9);
    let borrowed = w.pool.hosts().ui().unwrap().widgets.handles.borrow_mut();
    assert_eq!(w.kernel.native_instance_closed(8), Err(ERR_BUSY));
    w.kernel.shared.stack_rows.retry_cleanup(8).unwrap();
    drop(borrowed);
    assert_eq!(w.widgets(), 2);
    // The sweep (no screens on the stack here) frees the widgets of closed
    // screens even though the store holds nothing for them.
    w.kernel.drain_deferred_ui_closes();
    assert_eq!(w.widgets(), 0);
    assert!(!w.kernel.shared.stack_rows.cleanup_pending());
}

#[test]
fn a_busy_table_during_the_sweep_asks_for_another_pass() {
    let mut w = world();
    w.widget(8);
    w.kernel.shared.stack_rows.retry_cleanup(8).unwrap();
    let borrowed = w.pool.hosts().ui().unwrap().widgets.handles.borrow_mut();
    w.kernel.drain_deferred_ui_closes();
    assert!(w.kernel.shared.stack_rows.cleanup_pending());
    drop(borrowed);
    w.kernel.drain_deferred_ui_closes();
    assert_eq!(w.widgets(), 0);
    assert!(!w.kernel.shared.stack_rows.cleanup_pending());
}

#[test]
fn status_handles_end_with_their_callback_and_only_theirs() {
    let mut w = world();
    let plugin = Id::new_static("probe");
    let owner = w.pool.try_owner().unwrap();
    let key = w
        .kernel
        .shared
        .native_instances
        .insert(owner.try_view().unwrap(), plugin, None, true)
        .unwrap();
    let identity = w.kernel.shared.native_instances.identity(key).unwrap();
    let grant = |epoch: u64| {
        w.ui()
            .widgets
            .handles
            .borrow_mut()
            .grant_status(identity.owner(), epoch, OWNER, 1, false)
            .unwrap()
    };
    let outer = identity.callback().unwrap();
    let _first = grant(outer.epoch());
    let inner = identity.callback().unwrap();
    let _second = grant(inner.epoch());
    let held = || w.ui().widgets.handles.borrow().statuses();
    // The grant of a later callback already dropped the earlier one's.
    assert_eq!(held(), 1);
    drop(inner);
    // The inner callback returned: its handle is gone.
    assert_eq!(held(), 0);
    drop(outer);
    assert_eq!(held(), 0);
}

#[test]
fn a_busy_table_at_callback_end_loses_nothing_and_the_next_end_frees() {
    let mut w = world();
    let plugin = Id::new_static("probe");
    let owner = w.pool.try_owner().unwrap();
    let key = w
        .kernel
        .shared
        .native_instances
        .insert(owner.try_view().unwrap(), plugin, None, true)
        .unwrap();
    let identity = w.kernel.shared.native_instances.identity(key).unwrap();
    let epoch = identity.callback().unwrap();
    w.ui()
        .widgets
        .handles
        .borrow_mut()
        .grant_status(identity.owner(), epoch.epoch(), OWNER, 1, false)
        .unwrap();
    let borrowed = w.ui().widgets.handles.borrow_mut();
    drop(epoch);
    drop(borrowed);
    assert_eq!(w.ui().widgets.handles.borrow().statuses(), 1);
    // The plugin's next grant or its stop frees the stale handle, and it is
    // useless meanwhile: a lookup checks the callback's epoch.
    w.ui().plugin_stopped(identity.owner()).unwrap();
    assert_eq!(w.ui().widgets.handles.borrow().statuses(), 0);
}

#[test]
fn event_73_and_a_new_minute_end_the_cached_values_other_hints_do_not() {
    let mut w = world();
    w.cache();
    w.kernel
        .native_hint(crate::native_host::registrations::EVENT_PLUGINS_CHANGED_TAG);
    assert!(w.cached());
    w.kernel
        .native_hint(crate::native_host::registrations::EVENT_STATUS_CHANGED_TAG);
    assert!(!w.cached());
    // The first tick sets the minute; inside it nothing goes.
    w.kernel.tick(paperpaw_platform::Instant(60_000));
    w.cache();
    w.kernel.tick(paperpaw_platform::Instant(119_999));
    assert!(w.cached());
    w.kernel.tick(paperpaw_platform::Instant(120_000));
    assert!(!w.cached());
}

#[test]
fn a_busy_cache_at_the_minute_tick_tries_again_on_the_next_tick() {
    let mut w = world();
    w.kernel.tick(paperpaw_platform::Instant(0));
    w.cache();
    let borrowed = w.pool.hosts().ui().unwrap().widgets.cache.borrow_mut();
    w.kernel.tick(paperpaw_platform::Instant(60_000));
    drop(borrowed);
    assert!(w.cached());
    w.kernel.tick(paperpaw_platform::Instant(60_001));
    assert!(!w.cached());
}

#[test]
fn without_core_ui_every_hook_is_a_no_op() {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel.native_instance_closed(8).unwrap();
    kernel.tick(paperpaw_platform::Instant(60_000));
    kernel.native_hint(crate::native_host::registrations::EVENT_STATUS_CHANGED_TAG);
}

#[test]
fn a_closed_screens_widgets_are_told_to_their_owner_as_event_85_until_delivered() {
    use super::registrations::{self as wire, Record};
    let w = world();
    w.widget(5);
    w.widget(5);
    w.widget(6);
    w.ui().screen_closed(5).unwrap();
    let mut bytes = [0; 64];
    let mut told = alloc::vec::Vec::new();
    // A listed drop stays until it is given back, then the next comes.
    while let Some((notice, len)) = w.ui().next_widget_dropped(OWNER, 777, &mut bytes).unwrap() {
        let again = w.ui().next_widget_dropped(OWNER, 777, &mut bytes).unwrap();
        assert_eq!(again.map(|(n, _)| n), Some(notice));
        let Record::EventWidgetDropped(event) = wire::records_decode_single(&bytes[..len]).unwrap()
        else {
            panic!("not event 85")
        };
        assert_eq!(event.now_ms, 777);
        assert_eq!(event.instance, notice.instance);
        assert_eq!(notice.token, 1);
        told.push(event.instance);
        w.ui().widget_dropped_delivered(&notice).unwrap();
    }
    assert_eq!(told.len(), 2);
    assert_ne!(told[0], told[1]);
    // The maker's other screen is still open; nobody else is told anything.
    assert_eq!(w.widgets(), 1);
    assert!(
        w.ui()
            .next_widget_dropped(MAKER, 0, &mut bytes)
            .unwrap()
            .is_none()
    );
    // A buffer too small for the record is a status (nothing was attempted), and the drop stays listed.
    w.ui().screen_closed(6).unwrap();
    assert!(w.ui().next_widget_dropped(OWNER, 0, &mut [0; 2]).is_err());
    assert!(
        w.ui()
            .next_widget_dropped(OWNER, 0, &mut bytes)
            .unwrap()
            .is_some()
    );
}
