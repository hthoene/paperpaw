//! `Frame::call_native`: depth, owner, re-entry and the answer's checks.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::abi7::core_frame::CallbackStamp;
use crate::native_host::{
    declaration_store::Allocations,
    flat::FlatDepth,
    native_declarations::{self, NativeDeclarations},
    native_instances::{NativeInstance, NativeLease},
    raw_declarations::RawDeclarations,
};
use alloc::rc::{Rc, Weak};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::ERR_IO;
use paperpaw_sdk::{Env, Id};

/// The called owner (and the key of the callee's own frame).
const CALLEE: u64 = 900;
/// The calling owner.
const CALLER: u64 = 100;

/// What the callee did in its call.
#[derive(Default)]
struct Seen {
    /// `(token, depth, owner of the callee's own frame, status of a nested
    /// call it made through its own frame)`.
    calls: Vec<(u32, u8, u64, i32)>,
}

struct Callee {
    directory: Weak<NativeDeclarations>,
    depth: FlatDepth,
    seen: Rc<RefCell<Seen>>,
    answer: Rc<RefCell<Vec<u8>>>,
    status: Rc<Cell<i32>>,
    dropped: Rc<RefCell<Vec<(u32, i32)>>>,
    guard: crate::abi7::calls::Instance,
}
impl Dispatch for Callee {
    fn dropped(&self, token: u32, status: i32) {
        self.dropped.borrow_mut().push((token, status));
    }
    fn event(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        unreachable!()
    }
    fn input(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        unreachable!()
    }
    fn draw(
        &self,
        _: u32,
        _: &crate::native_host::registrations::DrawFrame<'_>,
        _: &Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        unreachable!()
    }
    fn stage(&self, token: u32, _: &[u8], out: &mut [u8], env: &Env<'_>) -> Result<i32, i32> {
        let _guard = self
            .guard
            .enter(&self.depth.as_depth().0)
            .map_err(|_| ERR_BUSY)?;
        let directory = self.directory.upgrade().unwrap();
        assert!(
            directory.store.try_borrow_mut().is_ok(),
            "no store borrow crosses the callee's code"
        );
        // The callee's own frame: its owner, and one more level down.
        let kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        let frame = CoreFrame {
            callback: Some(CallbackStamp::new(CALLEE, 1).unwrap()),
            instance: None,
            depth: Some(self.depth.as_depth()),
            directory: Some(&directory),
            env,
            meter: &kernel.shared.meter,
            stack_generation: kernel.shared.stack_rows.generation(),
            stack_rows: &kernel.shared.stack_rows,
            font_removals: &kernel.shared.font_removals,
            registry_generation: &kernel.shared.registry_generation,
            usage: &kernel.shared.usage,
            plugins: &kernel.shared.plugins,
            work: &kernel.shared.work_states,
            ui: None,
            log: crate::native_host::device_log::DeviceLog::new(None),
        };
        let nested = frame
            .call_native(CALLER, token, &[], &mut [0; 8], &mut |_| Ok(()))
            .err()
            .unwrap_or(0);
        self.seen.borrow_mut().calls.push((
            token,
            self.depth.as_depth().0.current(),
            frame.caller().unwrap(),
            nested,
        ));
        if self.status.get() != 0 {
            return Ok(self.status.get());
        }
        let answer = self.answer.borrow();
        // A callee that claims more than the buffer holds writes nothing.
        if let Some(room) = out.get_mut(..answer.len()) {
            room.copy_from_slice(&answer);
        }
        Ok(i32::try_from(answer.len()).unwrap())
    }
}
impl NativeInstance for Callee {
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

struct Fixture {
    directory: Rc<NativeDeclarations>,
    depth: FlatDepth,
    seen: Rc<RefCell<Seen>>,
    answer: Rc<RefCell<Vec<u8>>>,
    status: Rc<Cell<i32>>,
    dropped: Rc<RefCell<Vec<(u32, i32)>>>,
}
impl Fixture {
    fn new() -> Self {
        let directory = Rc::new(NativeDeclarations::default());
        let depth = FlatDepth::try_new().unwrap();
        let seen = Rc::new(RefCell::new(Seen::default()));
        let answer = Rc::new(RefCell::new(b"ok".to_vec()));
        let status = Rc::new(Cell::new(0));
        let dropped = Rc::new(RefCell::new(Vec::new()));
        let lease = NativeLease::try_new(Callee {
            directory: Rc::downgrade(&directory),
            depth: depth.try_clone().unwrap(),
            seen: Rc::clone(&seen),
            answer: Rc::clone(&answer),
            status: Rc::clone(&status),
            dropped: Rc::clone(&dropped),
            guard: crate::abi7::calls::Instance::new(),
        })
        .unwrap();
        let mut allocations = Allocations::default();
        let pending = native_declarations::pending(
            &Id::new_static("callee"),
            CALLEE,
            lease,
            RawDeclarations::default(),
            &mut allocations,
        )
        .unwrap();
        directory
            .stage(pending, CALLEE, &mut allocations)
            .unwrap()
            .commit();
        Self {
            directory,
            depth,
            seen,
            answer,
            status,
            dropped,
        }
    }

    /// Runs `check` with a frame of the caller, which is inside its own
    /// callback (depth 1).
    fn with_frame<R>(&self, check: impl FnOnce(&CoreFrame<'_>) -> R) -> R {
        self.with_directory(true, check)
    }

    fn with_directory<R>(&self, directory: bool, check: impl FnOnce(&CoreFrame<'_>) -> R) -> R {
        let kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        let env = kernel.shared.env(paperpaw_platform::Instant(0));
        let frame = CoreFrame {
            callback: Some(CallbackStamp::new(CALLER, 1).unwrap()),
            instance: None,
            depth: Some(self.depth.as_depth()),
            directory: directory.then_some(&*self.directory),
            env: &env,
            meter: &kernel.shared.meter,
            stack_generation: kernel.shared.stack_rows.generation(),
            stack_rows: &kernel.shared.stack_rows,
            font_removals: &kernel.shared.font_removals,
            registry_generation: &kernel.shared.registry_generation,
            usage: &kernel.shared.usage,
            plugins: &kernel.shared.plugins,
            work: &kernel.shared.work_states,
            ui: None,
            log: crate::native_host::device_log::DeviceLog::new(None),
        };
        let outer = crate::abi7::calls::Instance::new();
        let _outer = outer.enter(&self.depth.as_depth().0).unwrap();
        check(&frame)
    }
}

#[test]
fn the_callee_runs_at_depth_two_under_its_own_owner_and_a_third_level_is_busy() {
    let fixture = Fixture::new();
    let mut reply = [0; 16];
    let length = fixture
        .with_frame(|frame| frame.call_native(CALLEE, 7, b"in", &mut reply, &mut |_| Ok(())))
        .unwrap();
    assert_eq!(&reply[..length], b"ok");
    // Token 7, depth 2, the callee's frame names the callee, and its call
    // one level further down is refused.
    assert_eq!(
        &fixture.seen.borrow().calls[..],
        &[(7, 2, CALLEE, ERR_BUSY)]
    );
}

#[test]
fn a_caller_below_depth_one_is_busy_and_nothing_is_called() {
    let fixture = Fixture::new();
    let status = fixture.with_frame(|frame| {
        let inner = crate::abi7::calls::Instance::new();
        let _inner = inner.enter(&fixture.depth.as_depth().0).unwrap();
        frame.call_native(CALLEE, 7, &[], &mut [0; 8], &mut |_| Ok(()))
    });
    assert_eq!(status, Err(ERR_BUSY));
    assert!(fixture.seen.borrow().calls.is_empty());
}

#[test]
fn a_plugin_does_not_call_itself() {
    let fixture = Fixture::new();
    let status =
        fixture.with_frame(|frame| frame.call_native(CALLER, 7, &[], &mut [0; 8], &mut |_| Ok(())));
    assert_eq!(status, Err(ERR_BUSY));
    assert!(fixture.seen.borrow().calls.is_empty());
}

#[test]
fn an_unknown_owner_and_a_frame_without_a_directory_are_refused() {
    let fixture = Fixture::new();
    let status = fixture
        .with_frame(|frame| frame.call_native(CALLEE + 1, 7, &[], &mut [0; 8], &mut |_| Ok(())));
    assert_eq!(status, Err(paperpaw_native::host_glue::ERR_NOT_FOUND));
    let status = fixture.with_directory(false, |frame| {
        frame.call_native(CALLEE, 7, &[], &mut [0; 8], &mut |_| Ok(()))
    });
    assert_eq!(status, Err(paperpaw_native::host_glue::ERR_UNSUPPORTED));
    assert!(fixture.seen.borrow().calls.is_empty());
}

#[test]
fn the_callees_own_status_passes_through_and_is_not_a_misbehaviour() {
    let fixture = Fixture::new();
    fixture.status.set(ERR_IO);
    let status =
        fixture.with_frame(|frame| frame.call_native(CALLEE, 7, &[], &mut [0; 8], &mut |_| Ok(())));
    assert_eq!(status, Err(ERR_IO));
    assert!(fixture.dropped.borrow().is_empty());
}

#[test]
fn a_refused_or_oversized_answer_is_dropped_and_reported() {
    let fixture = Fixture::new();
    let status = fixture.with_frame(|frame| {
        frame.call_native(CALLEE, 7, &[], &mut [0; 8], &mut |_| Err(ERR_INVALID))
    });
    assert_eq!(status, Err(ERR_INVALID));
    // An answer longer than the caller's buffer is not read at all.
    *fixture.answer.borrow_mut() = alloc::vec![1; 8];
    let status =
        fixture.with_frame(|frame| frame.call_native(CALLEE, 8, &[], &mut [0; 4], &mut |_| Ok(())));
    assert!(status.is_err());
    assert_eq!(fixture.dropped.borrow().len(), 2);
    assert_eq!(fixture.dropped.borrow()[0], (7, ERR_INVALID));
}

#[test]
fn the_callee_is_metered_to_the_callee_and_not_the_caller() {
    let fixture = Fixture::new();
    let (callee, caller) = fixture.with_frame(|frame| {
        let _caller = frame.meter.enter_scoped("caller");
        frame
            .call_native(CALLEE, 7, &[], &mut [0; 8], &mut |_| Ok(()))
            .unwrap();
        (
            frame.meter.calls_of("callee"),
            frame.meter.calls_of("caller"),
        )
    });
    assert_eq!((callee, caller), (1, 1));
}

#[test]
fn a_refused_call_charges_nobody() {
    let fixture = Fixture::new();
    let calls = fixture.with_frame(|frame| {
        let _ = frame.call_native(CALLEE + 1, 7, &[], &mut [0; 8], &mut |_| Ok(()));
        let _ = frame.call_native(CALLER, 7, &[], &mut [0; 8], &mut |_| Ok(()));
        frame.meter.calls_of("callee")
    });
    assert_eq!(calls, 0);
}
