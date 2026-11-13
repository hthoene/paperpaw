//! The handle table and the compose algorithm: caps at the limit and one
//! beyond, the refusal order of `docs/abi/ui.md` section 5.1, expiry of
//! status handles, and heap refusal.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use core::cell::{Cell, RefCell};
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{Color, PixelFormat, Rect};

const ME: u64 = 1;
const OWNER: u64 = 2;
const OTHER: u64 = 3;

fn caller(callback: u64) -> Caller {
    Caller {
        plugin: ME,
        callback,
        depth: 1,
    }
}

struct Guard {
    plugin: Option<u64>,
    below: Vec<u32>,
}
impl GuardState for Guard {
    fn guard_plugin(&self) -> Option<u64> {
        self.plugin
    }
    fn below_guard(&self, screen: u32) -> bool {
        self.below.contains(&screen)
    }
}

#[test]
fn instances_come_from_a_checked_counter_up_to_the_cap_and_one_beyond() {
    for (x4, cap) in [(false, 32), (true, 16)] {
        let mut table = UiHandles::new(x4);
        let mut last = 0;
        for n in 0..cap {
            let id = table.make_widget(OWNER, ME, 7, 100 + n, false).unwrap();
            assert!(id > last, "ids grow and are never 0");
            last = id;
        }
        assert_eq!(table.widgets(), cap as usize);
        assert_eq!(table.make_widget(OWNER, ME, 7, 1, false), Err(ERR_FULL));
        assert_eq!(table.widgets(), cap as usize, "a refusal adds nothing");
        // Dropping one makes room once its owner has been told; its id is not
        // used again.
        table.drop_widget(ME, last).unwrap();
        assert_eq!(table.make_widget(OWNER, ME, 7, 1, false), Err(ERR_FULL));
        let notice = table.next_dropped(OWNER).unwrap();
        table.dropped_delivered(&notice);
        let again = table.make_widget(OWNER, ME, 7, 1, false).unwrap();
        assert!(again > last);
    }
}

#[test]
fn a_spent_counter_and_bad_indices_are_refused_without_a_change() {
    let mut table = UiHandles::new(false);
    table.next_widget = u32::MAX;
    assert_eq!(table.make_widget(OWNER, ME, 7, 1, false), Err(ERR_FULL));
    assert_eq!(table.widgets(), 0);
    let mut table = UiHandles::new(false);
    assert_eq!(table.make_widget(OWNER, ME, 0, 1, false), Err(ERR_INVALID));
    // Owner ids are full width: the largest ones need no index and no packing.
    assert!(
        table
            .make_widget(u64::MAX, u64::MAX - 1, 7, 1, true)
            .is_ok()
    );
    assert_eq!(table.widgets(), 1);
}

#[test]
fn a_heap_refusal_is_no_memory_and_leaves_the_table_as_it_was() {
    use crate::native_host::shell::tests::FAIL_NTH;
    let mut table = UiHandles::new(false);
    FAIL_NTH.with(|n| n.set(1));
    let widget = table.make_widget(OWNER, ME, 7, 1, false);
    FAIL_NTH.with(|n| n.set(0));
    assert_eq!(widget, Err(ERR_NO_MEMORY));
    assert_eq!(table.widgets(), 0);
    assert_eq!(table.next_widget, 1, "the id was not spent");
    FAIL_NTH.with(|n| n.set(1));
    let status = table.grant_status(ME, 1, OWNER, 5, false);
    FAIL_NTH.with(|n| n.set(0));
    assert_eq!(status, Err(ERR_NO_MEMORY));
    assert_eq!(table.status.len(), 0);
    assert_eq!(table.next_status, 1);
    // And it works again afterwards.
    assert_eq!(table.make_widget(OWNER, ME, 7, 1, false), Ok(1));
}

#[test]
fn a_heap_refusal_of_any_room_op_4_takes_leaves_no_instance_notice_or_room() {
    use crate::native_host::shell::tests::FAIL_NTH;
    let new = NewWidget {
        owner: OWNER,
        token: 1,
        private: false,
        widget: "a.w",
        place: "p",
    };
    // The notice's text and list, then the instance's three tables.
    for nth in 1..=5 {
        let mut table = UiHandles::new(false);
        FAIL_NTH.with(|n| n.set(nth));
        let made = table.make_told(ME, 7, &new);
        FAIL_NTH.with(|n| n.set(0));
        assert_eq!(made, Err(ERR_NO_MEMORY), "allocation {nth}");
        assert_eq!(table.widgets(), 0, "allocation {nth}");
        assert_eq!(table.made_waiting(), 0, "allocation {nth}");
        assert_eq!(table.made.capacity(), 0, "allocation {nth}: no room kept");
        assert_eq!(table.make_told(ME, 7, &new), Ok(1), "allocation {nth}");
    }
}

#[test]
fn only_the_maker_drops_an_instance_and_only_once() {
    let mut table = UiHandles::new(false);
    let id = table.make_widget(OWNER, ME, 7, 1, false).unwrap();
    assert_eq!(table.drop_widget(OTHER, id), Err(ERR_DENIED));
    assert_eq!(table.drop_widget(ME, id + 1), Err(ERR_NOT_FOUND));
    assert_eq!(table.drop_widget(ME, id), Ok(()));
    assert_eq!(table.drop_widget(ME, id), Err(ERR_NOT_FOUND));
}

#[test]
fn a_closed_screen_and_a_stopped_plugin_take_their_instances_along() {
    let mut table = UiHandles::new(false);
    let a = table.make_widget(OWNER, ME, 7, 1, false).unwrap();
    let b = table.make_widget(OWNER, ME, 8, 1, true).unwrap();
    let c = table.make_widget(OWNER, OTHER, 9, 1, false).unwrap();
    assert_eq!(table.drop_screen(7), 1);
    assert_eq!(table.drop_screen(7), 0);
    assert_eq!(table.drop_widget(ME, a), Err(ERR_NOT_FOUND));
    // The maker stopped: its instance goes; the owner is told of it.
    table.drop_plugin(ME);
    assert_eq!(table.widgets(), 1);
    assert_eq!(table.next_dropped(OWNER).map(|d| d.instance), Some(a));
    table.drop_plugin(OWNER);
    assert_eq!(table.widgets(), 0);
    // The owner stopped: nobody is left to tell.
    assert_eq!(table.next_dropped(OWNER), None);
    assert_eq!(table.drop_widget(OTHER, c), Err(ERR_NOT_FOUND));
    let _ = b;
}

#[test]
fn an_owner_hears_of_every_drop_once_in_order_and_one_attempt_ends_a_notice() {
    let mut table = UiHandles::new(false);
    let a = table.make_widget(OWNER, ME, 7, 4, false).unwrap();
    let b = table.make_widget(OWNER, ME, 7, 4, false).unwrap();
    let c = table.make_widget(OTHER, ME, 7, 5, false).unwrap();
    // One by its maker (op 5), the rest with their screen.
    table.drop_widget(ME, a).unwrap();
    assert_eq!(table.drop_screen(7), 2);
    let first = table.next_dropped(OWNER).unwrap();
    assert_eq!(
        first,
        Dropped {
            owner: OWNER,
            token: 4,
            instance: a
        }
    );
    // Not told yet: still the first.
    assert_eq!(table.next_dropped(OWNER), Some(first));
    table.dropped_delivered(&first);
    table.dropped_delivered(&first);
    let second = table.next_dropped(OWNER).unwrap();
    assert_eq!(second.instance, b);
    table.dropped_delivered(&second);
    assert_eq!(table.next_dropped(OWNER), None);
    // Another owner's is its own, with its own token.
    assert_eq!(
        table.next_dropped(OTHER),
        Some(Dropped {
            owner: OTHER,
            token: 5,
            instance: c
        })
    );
}

#[test]
fn undelivered_drops_stop_new_instances_instead_of_being_lost() {
    let mut table = UiHandles::new(true);
    // Fill and drop the cap's worth without Core delivering any.
    for _ in 0..MAX_WIDGETS_X4 {
        table.make_widget(OWNER, ME, 7, 1, false).unwrap();
    }
    assert_eq!(table.drop_screen(7), MAX_WIDGETS_X4);
    assert_eq!(table.make_widget(OWNER, ME, 8, 1, false), Err(ERR_FULL));
    let first = table.next_dropped(OWNER).unwrap();
    table.dropped_delivered(&first);
    assert!(table.make_widget(OWNER, ME, 8, 1, false).is_ok());
}

#[test]
fn a_widget_resolves_for_its_maker_in_the_contracts_order() {
    let mut table = UiHandles::new(false);
    let id = table.make_widget(OWNER, ME, 7, 42, false).unwrap();
    let none = Guard {
        plugin: None,
        below: Vec::new(),
    };
    assert_eq!(
        table.resolve(caller(1), ComposeKind::Widget, id, &none),
        Ok(Target {
            owner: OWNER,
            token: 42
        })
    );
    // Unknown handle, handle 0, the wrong kind of handle.
    for handle in [0, id + 1, u32::MAX] {
        assert_eq!(
            table.resolve(caller(1), ComposeKind::Widget, handle, &none),
            Err(ERR_NOT_FOUND)
        );
    }
    assert_eq!(
        table.resolve(caller(1), ComposeKind::StatusItem, id, &none),
        Err(ERR_NOT_FOUND)
    );
    // Someone else's instance is not revealed.
    let stranger = Caller {
        plugin: OTHER,
        ..caller(1)
    };
    assert_eq!(
        table.resolve(stranger, ComposeKind::Widget, id, &none),
        Err(ERR_NOT_FOUND)
    );
    // A child cannot compose: depth 2 is busy, before any lookup.
    let child = Caller {
        depth: 2,
        ..caller(1)
    };
    for handle in [id, 999] {
        assert_eq!(
            table.resolve(child, ComposeKind::Widget, handle, &none),
            Err(ERR_BUSY)
        );
    }
    // The caller's own plugin is never re-entered.
    let own = table.make_widget(ME, ME, 7, 1, false).unwrap();
    assert_eq!(
        table.resolve(caller(1), ComposeKind::Widget, own, &none),
        Err(ERR_BUSY)
    );
}

#[test]
fn a_guard_hides_private_widgets_and_those_made_below_it_from_everyone_else() {
    let mut table = UiHandles::new(false);
    let plain = table.make_widget(OWNER, ME, 7, 1, false).unwrap();
    let private = table.make_widget(OWNER, ME, 7, 2, true).unwrap();
    let below = table.make_widget(OWNER, ME, 5, 3, false).unwrap();
    let guard = Guard {
        plugin: Some(OTHER),
        below: alloc::vec![5],
    };
    // A widget of a screen above the guard that is not private is usable
    // (a clock on the sleep screen of a locked device).
    assert!(
        table
            .resolve(caller(1), ComposeKind::Widget, plain, &guard)
            .is_ok()
    );
    assert_eq!(
        table.resolve(caller(1), ComposeKind::Widget, private, &guard),
        Err(ERR_DENIED)
    );
    assert_eq!(
        table.resolve(caller(1), ComposeKind::Widget, below, &guard),
        Err(ERR_DENIED)
    );
    // The guard's plugin decides what its own screens show.
    let lock = Caller {
        plugin: OTHER,
        ..caller(1)
    };
    let own = table.make_widget(OWNER, OTHER, 9, 4, true).unwrap();
    assert!(
        table
            .resolve(lock, ComposeKind::Widget, own, &guard)
            .is_ok()
    );
    // Without a guard nothing is hidden.
    assert!(
        table
            .resolve(
                caller(1),
                ComposeKind::Widget,
                private,
                &Guard {
                    plugin: None,
                    below: alloc::vec![5]
                }
            )
            .is_ok()
    );
}

#[test]
fn status_handles_live_only_in_the_callback_that_got_them() {
    let mut table = UiHandles::new(false);
    let item = table.grant_status(ME, 10, OWNER, 77, false).unwrap();
    let line = table.grant_status(ME, 10, OWNER, 78, true).unwrap();
    assert_ne!(item, 0);
    assert_ne!(item, line);
    let g = NoGuard;
    assert_eq!(
        table.resolve(caller(10), ComposeKind::StatusItem, item, &g),
        Ok(Target {
            owner: OWNER,
            token: 77
        })
    );
    assert_eq!(
        table.resolve(caller(10), ComposeKind::StatusLine, line, &g),
        Ok(Target {
            owner: OWNER,
            token: 78
        })
    );
    // An item handle is no line handle and the other way round.
    assert_eq!(
        table.resolve(caller(10), ComposeKind::StatusLine, item, &g),
        Err(ERR_NOT_FOUND)
    );
    assert_eq!(
        table.resolve(caller(10), ComposeKind::StatusItem, line, &g),
        Err(ERR_NOT_FOUND)
    );
    // Another callback of the caller: expired.
    assert_eq!(
        table.resolve(caller(11), ComposeKind::StatusItem, item, &g),
        Err(ERR_NOT_FOUND)
    );
    // Another plugin never uses the caller's handle, even in the same callback number.
    let stranger = Caller {
        plugin: OTHER,
        ..caller(10)
    };
    assert_eq!(
        table.resolve(stranger, ComposeKind::StatusItem, item, &g),
        Err(ERR_NOT_FOUND)
    );
    // The callback ends: its handles go.
    table.end_callback(ME, 10);
    assert_eq!(table.status.len(), 0);
    assert_eq!(
        table.resolve(caller(10), ComposeKind::StatusItem, item, &g),
        Err(ERR_NOT_FOUND)
    );
}

#[test]
fn a_new_callback_replaces_the_callers_old_handles_and_the_cap_holds() {
    for (x4, cap) in [(false, MAX_STATUS), (true, MAX_STATUS_X4)] {
        let mut table = UiHandles::new(x4);
        for _ in 0..cap {
            table.grant_status(ME, 1, OWNER, 1, false).unwrap();
        }
        assert_eq!(
            table.grant_status(ME, 1, OWNER, 1, false),
            Err(ERR_FULL),
            "limit + 1 in one callback"
        );
        // Another plugin's handles count against the same table.
        assert_eq!(table.grant_status(OTHER, 1, OWNER, 1, false), Err(ERR_FULL));
        // The caller's next callback starts afresh.
        assert!(table.grant_status(ME, 2, OWNER, 1, false).is_ok());
        assert_eq!(table.status.len(), 1);
    }
    let mut table = UiHandles::new(false);
    table.next_status = u32::MAX;
    assert_eq!(table.grant_status(ME, 1, OWNER, 1, false), Err(ERR_FULL));
    let mut table = UiHandles::new(false);
    assert!(
        table
            .grant_status(u64::MAX, 1, u64::MAX - 1, 1, false)
            .is_ok()
    );
}

struct Child {
    answer: Cell<Result<i32, i32>>,
    draws: RefCell<Vec<(Target, u32, u32)>>,
    failed: RefCell<Vec<(Target, i32)>>,
}
impl Child {
    fn new(answer: Result<i32, i32>) -> Self {
        Self {
            answer: Cell::new(answer),
            draws: RefCell::default(),
            failed: RefCell::default(),
        }
    }
}
impl ChildDraw for Child {
    fn draw(
        &self,
        target: &Target,
        frame: &DrawFrame<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        self.draws
            .borrow_mut()
            .push((*target, frame.purpose, frame.id));
        canvas.fill_rect(0, 0, 4096, 4096, Color::BLACK);
        self.answer.get()
    }
    fn failed(&self, target: &Target, status: i32) {
        self.failed.borrow_mut().push((*target, status));
    }
}

fn request(kind: ComposeKind, handle: u32, visible: Option<Rect>) -> ComposeRequest {
    ComposeRequest {
        kind,
        handle,
        x: 2,
        y: 2,
        width: 6,
        height: 6,
        visible,
    }
}

#[test]
fn compose_resolves_first_then_draws_only_what_is_visible_and_logs_a_failing_child() {
    let mut table = UiHandles::new(false);
    let id = table.make_widget(OWNER, ME, 7, 42, false).unwrap();
    let mut fb = Framebuffer::new(16, 16, PixelFormat::Gray8).unwrap();
    let g = NoGuard;
    let vis = Some(Rect::new(2, 2, 6, 6));
    // A good child draws, with the owner's purpose and the handle as id.
    let child = Child::new(Ok(0));
    let mut canvas = fb.canvas();
    let mut view = canvas.clipped(Rect::new(2, 2, 6, 6));
    assert_eq!(
        compose(
            &table,
            &g,
            &child,
            caller(1),
            &request(ComposeKind::Widget, id, vis),
            &mut view
        ),
        Ok(())
    );
    assert_eq!(
        *child.draws.borrow(),
        [(
            Target {
                owner: OWNER,
                token: 42
            },
            4,
            id
        )]
    );
    assert!(child.failed.borrow().is_empty());
    // Nothing visible: refusals still come first, but no call is made.
    let child = Child::new(Ok(0));
    assert_eq!(
        compose(
            &table,
            &g,
            &child,
            caller(1),
            &request(ComposeKind::Widget, id, None),
            &mut view
        ),
        Ok(())
    );
    assert!(child.draws.borrow().is_empty());
    assert_eq!(
        compose(
            &table,
            &g,
            &child,
            caller(1),
            &request(ComposeKind::Widget, id + 1, None),
            &mut view
        ),
        Err(ERR_NOT_FOUND)
    );
    // A failing child (a fault status or an error) is logged and the call is Ok.
    for answer in [Ok(1), Err(ERR_NO_MEMORY), Err(-77)] {
        let child = Child::new(answer);
        assert_eq!(
            compose(
                &table,
                &g,
                &child,
                caller(1),
                &request(ComposeKind::Widget, id, vis),
                &mut view
            ),
            Ok(())
        );
        let status = answer.unwrap_or_else(|e| e);
        assert_eq!(child.failed.borrow().len(), 1);
        assert_eq!(child.failed.borrow()[0].1, status);
    }
    // A child cannot compose.
    let child = Child::new(Ok(0));
    let nested = Caller {
        depth: 2,
        ..caller(1)
    };
    assert_eq!(
        compose(
            &table,
            &g,
            &child,
            nested,
            &request(ComposeKind::Widget, id, vis),
            &mut view
        ),
        Err(ERR_BUSY)
    );
    assert!(child.draws.borrow().is_empty());
}

#[test]
fn each_table_is_one_allocation_of_its_cap_and_never_grows() {
    use crate::native_host::shell::tests::FAIL_NTH;
    // The sizes the memory line is built on.
    assert_eq!(size_of::<Widget>(), 32);
    assert_eq!(size_of::<Status>(), 40);
    for (x4, widgets, status) in [(false, 32, 16), (true, 16, 8)] {
        let mut table = UiHandles::new(x4);
        assert_eq!((table.widgets.capacity(), table.status.capacity()), (0, 0));
        table.make_widget(OWNER, ME, 7, 0, false).unwrap();
        table.grant_status(ME, 1, OWNER, 0, false).unwrap();
        let (wc, sc, dc) = (
            table.widgets.capacity(),
            table.status.capacity(),
            table.dropped.capacity(),
        );
        assert!(wc >= widgets && sc >= status && dc >= widgets);
        // From here no insert allocates: a refusing heap changes nothing.
        FAIL_NTH.with(|n| n.set(1));
        let mut made = 1;
        while table.make_widget(OWNER, ME, 7, 0, false).is_ok() {
            made += 1;
        }
        let mut granted = 1;
        while table.grant_status(ME, 1, OWNER, 0, false).is_ok() {
            granted += 1;
        }
        let spared = FAIL_NTH.with(|n| n.replace(0));
        assert_eq!((made, granted), (widgets, status));
        assert_eq!(spared, 1, "no insert after the first asked the heap");
        assert_eq!(
            (
                table.widgets.capacity(),
                table.status.capacity(),
                table.dropped.capacity()
            ),
            (wc, sc, dc),
            "no growth to the cap"
        );
        // The bytes held at most: 32 x 32 + 40 x 16 + 32 x 16 (the drops to tell),
        // 16 x 32 + 40 x 8 + 16 x 16 on the X4.
        assert!(
            wc * size_of::<Widget>() + sc * size_of::<Status>() + dc * size_of::<Dropped>()
                <= if x4 { 1_088 } else { 2_176 }
        );
    }
}

#[test]
fn the_room_for_every_notice_is_taken_when_an_instance_is_made() {
    let mut table = UiHandles::new(false);
    for screen in 1..=5 {
        table.make_widget(OWNER, ME, screen, 1, false).unwrap();
        // Every live instance can be dropped without the heap.
        assert!(table.dropped_room() >= table.widgets(), "screen {screen}");
    }
    table.drop_screen(2);
    table.drop_screen(3);
    assert!(table.dropped_room() >= table.widgets());
    assert!(table.dropped_room() + 2 >= 32, "one allocation of the cap");
    table.drop_plugin(ME);
    assert_eq!(table.widgets(), 0);
    assert!(table.next_dropped(OWNER).is_some());
}

#[test]
fn a_dead_owner_or_a_failed_delivery_never_pins_the_cap() {
    for x4 in [false, true] {
        let cap = if x4 { MAX_WIDGETS_X4 } else { MAX_WIDGETS };
        // The owner stopped: its waiting drops go with it, at once.
        let mut table = UiHandles::new(x4);
        for _ in 0..cap {
            table.make_widget(OWNER, ME, 7, 1, false).unwrap();
        }
        assert_eq!(table.drop_screen(7), cap);
        assert_eq!(table.make_widget(OWNER, ME, 8, 1, false), Err(ERR_FULL));
        table.drop_plugin(OWNER);
        assert_eq!(table.next_dropped(OWNER), None);
        assert!(table.make_widget(OTHER, ME, 8, 1, false).is_ok());
        // A failed delivery is an attempt: giving the notice back ends it,
        // however the callback went, and the place is free again.
        let mut table = UiHandles::new(x4);
        for _ in 0..cap {
            table.make_widget(OWNER, ME, 7, 1, false).unwrap();
        }
        assert_eq!(table.drop_screen(7), cap);
        while let Some(notice) = table.next_dropped(OWNER) {
            // The callback failed or trapped: the attempt is over all the same.
            table.dropped_delivered(&notice);
        }
        assert_eq!(table.next_dropped(OWNER), None);
        for _ in 0..cap {
            table.make_widget(OWNER, ME, 9, 1, false).unwrap();
        }
    }
}
