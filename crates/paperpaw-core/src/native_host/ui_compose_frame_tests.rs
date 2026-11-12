//! Slot 35 from a plugin's own `CoreFrame`: the device's handle table, the
//! declaration directory for the owner, the frame's stack and depth. No table
//! borrow crosses the child's `draw`.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::abi7::context::ComposeKind;
use crate::native_host::{
    HostResources,
    declaration_store::Allocations,
    flat::FlatDepth,
    native_declarations,
    native_instances::NativeInstance,
    raw_declarations::RawDeclarations,
    registrations::{self as wire, Record},
    shared::Shared,
    ui_service::UiServices,
    ui_store::{Limits, UiStore},
};
use alloc::{rc::Rc, rc::Weak, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_graphics::Framebuffer;
use paperpaw_native::host_glue::ERR_DENIED;
use paperpaw_platform::{MemoryClass, PixelFormat, Rect};
use paperpaw_sdk::Id;

const OWNER: u64 = u64::MAX;
const CALLER: u64 = 7;
const TOKEN: u32 = 42;
/// The caller's screen instance.
const SCREEN: u32 = 5;

/// What the child's `draw` saw: depth, both tables free, the purpose and id.
type Seen = (u8, bool, bool, u32, u32);

struct Child {
    directory: Weak<NativeDeclarations>,
    ui: Rc<HostResources>,
    depth: FlatDepth,
    guard: crate::abi7::calls::Instance,
    seen: Rc<RefCell<Vec<Seen>>>,
    answer: Rc<Cell<i32>>,
    dropped: Rc<RefCell<Vec<i32>>>,
    stack: Rc<crate::native_host::stack_rows::StackRows>,
    /// The (owner, instance) the stack tracker named during each `draw`.
    running: Rc<RefCell<Vec<(u64, u32)>>>,
}
impl NativeInstance for Child {
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(Some(&[]));
    }
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn callback_depth(&self) -> u8 {
        self.depth.as_depth().0.current()
    }
}
impl Dispatch for Child {
    fn event(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        unreachable!()
    }
    fn input(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        unreachable!()
    }
    fn draw(
        &self,
        token: u32,
        frame: &DrawFrame<'_>,
        _: &Env<'_>,
        _: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        assert_eq!(token, TOKEN);
        self.running.borrow_mut().push(self.stack.running());
        // As the dispatch does: the child's draw is the second level.
        let _level = self.guard.enter(&self.depth.as_depth().0).unwrap();
        let store_free = self
            .directory
            .upgrade()
            .unwrap()
            .store
            .try_borrow_mut()
            .is_ok();
        let handles_free = self
            .ui
            .hosts()
            .ui()
            .unwrap()
            .widgets
            .handles
            .try_borrow_mut()
            .is_ok();
        self.seen.borrow_mut().push((
            self.depth.as_depth().0.current(),
            store_free,
            handles_free,
            frame.purpose,
            frame.id,
        ));
        Ok(self.answer.get())
    }
    fn dropped(&self, _: u32, status: i32) {
        self.dropped.borrow_mut().push(status);
    }
}

struct World {
    pool: Rc<HostResources>,
    directory: Rc<NativeDeclarations>,
    depth: FlatDepth,
    kernel: crate::Kernel,
    seen: Rc<RefCell<Vec<Seen>>>,
    answer: Rc<Cell<i32>>,
    dropped: Rc<RefCell<Vec<i32>>>,
    running: Rc<RefCell<Vec<(u64, u32)>>>,
    widget: u32,
}

fn world() -> World {
    world_on(crate::tests::DEVICE)
}

fn world_on(device: paperpaw_platform::DeviceInfo) -> World {
    let pool = Rc::new(HostResources::try_new().unwrap());
    pool.hosts()
        .wire_ui(UiServices::new(
            Shared::try_new(UiStore::new(Limits::ELSEWHERE)).unwrap(),
            false,
        ))
        .unwrap();
    let directory = Rc::new(NativeDeclarations::default());
    let depth = FlatDepth::try_new().unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let answer = Rc::new(Cell::new(0));
    let dropped = Rc::new(RefCell::new(Vec::new()));
    let running = Rc::new(RefCell::new(Vec::new()));
    let kernel = crate::Kernel::builder(device).build().unwrap();
    let lease = NativeLease::try_new(Child {
        directory: Rc::downgrade(&directory),
        ui: Rc::clone(&pool),
        depth: depth.try_clone().unwrap(),
        guard: crate::abi7::calls::Instance::new(),
        seen: Rc::clone(&seen),
        answer: Rc::clone(&answer),
        dropped: Rc::clone(&dropped),
        stack: Rc::clone(&kernel.shared.stack_rows),
        running: Rc::clone(&running),
    })
    .unwrap();
    let row = [Record::StatusItem(wire::StatusItem {
        name: "one",
        title: "One",
        tap: "",
        priority: 0,
        min_width: 0,
        token: 11,
        flags: 0,
    })];
    let mut bytes = [0; 128];
    let length = wire::records_encode(&row, &mut bytes).unwrap();
    let bytes = &bytes[..length];
    let decoded = wire::records_decode(bytes).unwrap();
    let mut raw = RawDeclarations::default();
    let prepared = raw.prepare(None, Some(bytes), &decoded).unwrap();
    raw.commit(prepared);
    let mut allocations = Allocations::default();
    let pending = native_declarations::pending(
        &Id::new_static("remote"),
        OWNER,
        lease,
        raw,
        &mut allocations,
    )
    .unwrap();
    directory
        .stage(pending, OWNER, &mut allocations)
        .unwrap()
        .commit();
    let widget = pool
        .hosts()
        .ui()
        .unwrap()
        .widgets
        .handles
        .borrow_mut()
        .make_widget(OWNER, CALLER, 3, TOKEN, false)
        .unwrap();
    World {
        pool,
        directory,
        depth,
        kernel,
        running,
        seen,
        answer,
        dropped,
        widget,
    }
}

impl World {
    /// Compose `request` from `CALLER`'s frame at the depth already entered.
    fn run(
        &self,
        callback: Option<CallbackStamp>,
        stack_generation: u64,
        with_ui: bool,
        request: &ComposeRequest,
    ) -> Result<(), i32> {
        let env = self.kernel.shared.env(paperpaw_platform::Instant(0));
        let frame = CoreFrame {
            callback,
            instance: Some(SCREEN),
            depth: Some(self.depth.as_depth()),
            directory: Some(&self.directory),
            env: &env,
            meter: &self.kernel.shared.meter,
            stack_generation,
            stack_rows: &self.kernel.shared.stack_rows,
            font_removals: &self.kernel.shared.font_removals,
            registry_generation: &self.kernel.shared.registry_generation,
            usage: &self.kernel.shared.usage,
            plugins: &self.kernel.shared.plugins,
            work: &self.kernel.shared.work_states,
            log: Default::default(),
            ui: with_ui.then(|| self.pool.hosts().ui().unwrap()),
        };
        let mut fb = Framebuffer::new(16, 16, PixelFormat::Gray8).unwrap();
        let mut canvas = fb.canvas();
        let mut view = canvas.clipped(Rect::new(2, 3, 6, 4));
        compose_in(&frame, request, &mut view)
    }

    fn generation(&self) -> u64 {
        self.kernel.shared.stack_rows.generation()
    }

    fn stamp() -> Option<CallbackStamp> {
        CallbackStamp::new(CALLER, 1).ok()
    }

    fn request(&self) -> ComposeRequest {
        ComposeRequest {
            kind: ComposeKind::Widget,
            handle: self.widget,
            x: 2,
            y: 2,
            width: 6,
            height: 6,
            visible: Some(Rect::new(2, 3, 6, 4)),
        }
    }
}

#[test]
fn a_screens_draw_composes_a_widget_of_another_plugin_at_the_second_level() {
    let w = world();
    let caller = crate::abi7::calls::Instance::new();
    let _outer = caller.enter(&w.depth.as_depth().0).unwrap();
    let request = w.request();
    // Stack generation 0 is "no stack read": the guard fails closed, so a
    // real generation is needed even for an unguarded stack.
    assert_eq!(
        w.run(World::stamp(), w.generation(), true, &request),
        Ok(())
    );
    // Depth 2 inside the child, the directory and the handle table both free,
    // purpose 4 (widget), the handle as the frame's id.
    assert_eq!(*w.seen.borrow(), [(2, true, true, 4, w.widget)]);
    // The caller's screen callback stayed on the tracker for the child: the
    // child's owner on the caller's screen; the entry before comes back.
    assert_eq!(*w.running.borrow(), [(OWNER, SCREEN)]);
    assert_eq!(w.kernel.shared.stack_rows.running(), (0, 0));
    assert!(w.dropped.borrow().is_empty());
    // A failing child is logged and told; the caller's frame stays.
    w.answer.set(-5);
    assert_eq!(
        w.run(World::stamp(), w.generation(), true, &request),
        Ok(())
    );
    assert_eq!(*w.dropped.borrow(), [-5]);
}

#[test]
fn a_child_cannot_compose_and_nothing_is_called_outside_a_callback() {
    let w = world();
    let request = w.request();
    let first = crate::abi7::calls::Instance::new();
    let second = crate::abi7::calls::Instance::new();
    let _outer = first.enter(&w.depth.as_depth().0).unwrap();
    let _inner = second.enter(&w.depth.as_depth().0).unwrap();
    assert_eq!(
        w.run(World::stamp(), w.generation(), true, &request),
        Err(ERR_BUSY)
    );
    assert_eq!(w.run(None, w.generation(), true, &request), Err(ERR_STATE));
    assert!(w.seen.borrow().is_empty());
}

#[test]
fn without_the_ui_service_it_is_unsupported() {
    let w = world();
    let caller = crate::abi7::calls::Instance::new();
    let _outer = caller.enter(&w.depth.as_depth().0).unwrap();
    assert_eq!(
        w.run(World::stamp(), w.generation(), false, &w.request()),
        Err(ERR_UNSUPPORTED)
    );
    assert!(w.seen.borrow().is_empty());
}

#[test]
fn a_stack_the_frame_cannot_read_hides_everything_below_it() {
    let w = world();
    let caller = crate::abi7::calls::Instance::new();
    let _outer = caller.enter(&w.depth.as_depth().0).unwrap();
    // A stale generation: the stack is not readable, the guard fails closed.
    assert_eq!(
        w.run(World::stamp(), w.generation() + 1, true, &w.request()),
        Err(ERR_DENIED)
    );
    assert!(w.seen.borrow().is_empty());
}

#[test]
fn a_stopped_owner_is_the_childs_failure_not_the_callers() {
    let w = world();
    let caller = crate::abi7::calls::Instance::new();
    let _outer = caller.enter(&w.depth.as_depth().0).unwrap();
    w.directory
        .store
        .try_borrow_mut()
        .unwrap()
        .remove_host_owner(OWNER);
    assert_eq!(
        w.run(World::stamp(), w.generation(), true, &w.request()),
        Ok(())
    );
    assert!(w.seen.borrow().is_empty());
}

#[test]
#[cfg_attr(feature = "x4-nested", ignore = "x4-nested opens the X4 gate (#250)")]
fn a_small_device_blocks_compose_until_its_stack_gate_passes() {
    // Every memory class but `Large` is the small one (`ui_store::is_x4`: a
    // class added later fails closed), and the small one answers slot 35
    // `ERR_UNSUPPORTED` whatever the handle is: the X4, the X3 and any later
    // ESP32-C3 profile (`shells/desktop` pins their classes).
    assert!(crate::native_host::ui_store::is_x4(MemoryClass::Small));
    assert!(!crate::native_host::ui_store::is_x4(MemoryClass::Large));
    let w = world_on(crate::tests::DEVICE.with_memory(MemoryClass::Small));
    let caller = crate::abi7::calls::Instance::new();
    let _outer = caller.enter(&w.depth.as_depth().0).unwrap();
    assert_eq!(
        w.run(World::stamp(), w.generation(), true, &w.request()),
        Err(ERR_UNSUPPORTED)
    );
    assert!(w.seen.borrow().is_empty());
    // The same call on a large device draws.
    let large = world();
    let caller = crate::abi7::calls::Instance::new();
    let _outer = caller.enter(&large.depth.as_depth().0).unwrap();
    assert_eq!(
        large.run(World::stamp(), large.generation(), true, &large.request()),
        Ok(())
    );
    assert_eq!(large.seen.borrow().len(), 1);
}
