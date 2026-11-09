//! Slot 35 through the retained registry: a real lease draws the child, the
//! refusals come first and a failing or missing owner never fails the caller.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::{
    abi7::calls,
    native_host::native_instances::{NativeInstance, NativeLease},
};
use alloc::{rc::Rc, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_graphics::Framebuffer;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_DENIED, ERR_NOT_FOUND};
use paperpaw_platform::{PixelFormat, Rect};
use paperpaw_sdk::Id;

use crate::abi7::context::ComposeKind;

/// Token, purpose, id, clip top and bottom of one `draw`.
type Drawn = (u32, u32, u32, i32, i32);

struct Seen {
    draws: RefCell<Vec<Drawn>>,
    dropped: RefCell<Vec<(u32, i32)>>,
    /// What the owners' `draw` answers.
    answer: Cell<Result<i32, i32>>,
}

struct Fake {
    seen: Rc<Seen>,
    live: Cell<bool>,
}
impl NativeInstance for Fake {
    fn running(&self) -> bool {
        self.live.get()
    }
    fn stop(&self) {
        self.live.set(false);
    }
}
impl Dispatch for Fake {
    fn event(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn input(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn draw(
        &self,
        token: u32,
        frame: &DrawFrame<'_>,
        _: &Env<'_>,
        _: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        self.seen.draws.borrow_mut().push((
            token,
            frame.purpose,
            frame.id,
            frame.clip_top,
            frame.clip_bottom,
        ));
        self.seen.answer.get()
    }
    fn dropped(&self, token: u32, status: i32) {
        self.seen.dropped.borrow_mut().push((token, status));
    }
}

struct Guard {
    plugin: Option<u64>,
    below: bool,
}
impl GuardState for Guard {
    fn guard_plugin(&self) -> Option<u64> {
        self.plugin
    }
    fn below_guard(&self, _: u32) -> bool {
        self.below
    }
}

struct World {
    registry: NativeInstances,
    seen: Rc<Seen>,
    me: u64,
    owner: u64,
    depth: DispatchDepth,
    handles: RefCell<UiHandles>,
    widget: u32,
}

fn world() -> World {
    let seen = Rc::new(Seen {
        draws: RefCell::default(),
        dropped: RefCell::default(),
        answer: Cell::new(Ok(0)),
    });
    let mut registry = NativeInstances::default();
    let mut add = |name: &'static str| {
        let lease = NativeLease::try_new(Fake {
            seen: seen.clone(),
            live: Cell::new(true),
        })
        .unwrap();
        registry
            .test_insert(Id::new_static(name), Some(lease), true)
            .unwrap()
    };
    let me = add("me");
    let owner = add("owner");
    let mut handles = UiHandles::new(false);
    let widget = handles.make_widget(owner, me, 7, 42, false).unwrap();
    World {
        registry,
        seen,
        me,
        owner,
        depth: DispatchDepth::new(),
        handles: RefCell::new(handles),
        widget,
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

fn visible() -> Option<Rect> {
    Some(Rect::new(2, 3, 6, 4))
}

fn run(
    w: &World,
    guard: &Guard,
    gate_open: bool,
    caller: u64,
    request: &ComposeRequest,
) -> Result<(), i32> {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let host = ComposeHost {
        handles: &w.handles,
        guard,
        leases: &w.registry,
        env: &env,
        depth: &w.depth,
        screen: None,
        gate_open,
    };
    let mut fb = Framebuffer::new(16, 16, PixelFormat::Gray8).unwrap();
    let mut canvas = fb.canvas();
    let mut view = canvas.clipped(Rect::new(2, 3, 6, 4));
    host.compose(CallbackStamp::new(caller, 1).unwrap(), request, &mut view)
}

const NONE: Guard = Guard {
    plugin: None,
    below: false,
};

#[test]
fn the_owners_draw_runs_with_the_handles_token_purpose_id_and_visible_rows() {
    let w = world();
    let r = request(ComposeKind::Widget, w.widget, visible());
    assert_eq!(run(&w, &NONE, true, w.me, &r), Ok(()));
    // Purpose 4 (widget), the handle as id, the visible rows 3..7.
    assert_eq!(*w.seen.draws.borrow(), [(42, 4, w.widget, 3, 7)]);
    assert!(w.seen.dropped.borrow().is_empty());
    // Nothing visible: resolved, but no call.
    w.seen.draws.borrow_mut().clear();
    let r = request(ComposeKind::Widget, w.widget, None);
    assert_eq!(run(&w, &NONE, true, w.me, &r), Ok(()));
    assert!(w.seen.draws.borrow().is_empty());
}

#[test]
fn refusals_come_before_any_call() {
    let w = world();
    let draws = || w.seen.draws.borrow().len();
    // Unknown handle, and another plugin's handle is not revealed.
    let unknown = request(ComposeKind::Widget, w.widget + 9, visible());
    assert_eq!(run(&w, &NONE, true, w.me, &unknown), Err(ERR_NOT_FOUND));
    let r = request(ComposeKind::Widget, w.widget, visible());
    assert_eq!(run(&w, &NONE, true, w.owner, &r), Err(ERR_NOT_FOUND));
    // A target of the caller's own plugin is busy (it draws its own).
    let mut own = world();
    own.widget = own
        .handles
        .borrow_mut()
        .make_widget(own.me, own.me, 7, 5, false)
        .unwrap();
    let r = request(ComposeKind::Widget, own.widget, visible());
    assert_eq!(run(&own, &NONE, true, own.me, &r), Err(ERR_BUSY));
    // A widget made below the guard of another plugin is denied.
    let guard = Guard {
        plugin: Some(w.owner),
        below: true,
    };
    assert_eq!(run(&w, &guard, true, w.me, &r_for(&w)), Err(ERR_DENIED));
    // The X4's gate: until it passes, unsupported.
    assert_eq!(
        run(&w, &NONE, false, w.me, &r_for(&w)),
        Err(ERR_UNSUPPORTED)
    );
    assert_eq!(draws(), 0);
}

fn r_for(w: &World) -> ComposeRequest {
    request(ComposeKind::Widget, w.widget, visible())
}

#[test]
fn a_compose_from_inside_a_child_is_busy() {
    let w = world();
    // The caller's draw is at depth 1; the child's at depth 2: composing from
    // there is refused whatever the handle is.
    let first = calls::Instance::new();
    let second = calls::Instance::new();
    let _outer = first.enter(&w.depth.0).unwrap();
    assert_eq!(run(&w, &NONE, true, w.me, &r_for(&w)), Ok(()));
    let _inner = second.enter(&w.depth.0).unwrap();
    assert_eq!(run(&w, &NONE, true, w.me, &r_for(&w)), Err(ERR_BUSY));
    assert_eq!(w.seen.draws.borrow().len(), 1);
}

#[test]
fn a_failing_child_is_told_and_the_callers_frame_stays() {
    let w = world();
    for answer in [Ok(3), Err(-77)] {
        w.seen.dropped.borrow_mut().clear();
        w.seen.answer.set(answer);
        assert_eq!(run(&w, &NONE, true, w.me, &r_for(&w)), Ok(()));
        let status = answer.unwrap_or_else(|e| e);
        assert_eq!(*w.seen.dropped.borrow(), [(42, status)]);
    }
}

#[test]
fn a_stopped_owner_fails_quietly() {
    let mut w = world();
    w.registry.stop(w.owner).unwrap();
    assert_eq!(run(&w, &NONE, true, w.me, &r_for(&w)), Ok(()));
    assert!(w.seen.draws.borrow().is_empty());
}

#[test]
fn the_host_is_a_few_borrows_and_holds_no_buffer() {
    // Ten words and a flag (two of them trait objects): no frame of this
    // path comes near the 2,047 bytes the ppn5 build holds every host function to.
    assert!(size_of::<ComposeHost<'_, '_>>() <= 88);
    assert!(size_of::<RegistryDraw<'_, '_>>() <= 40);
}
