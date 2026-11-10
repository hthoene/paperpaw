//! Owner calls use the shared guard and release declaration borrows first.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::native_host::{
    declaration_store::Allocations,
    flat::FlatDepth,
    native_declarations,
    native_instances::{NativeInstance, NativeLease},
    raw_declarations::RawDeclarations,
};
use alloc::rc::{Rc, Weak};
use core::cell::{Cell, RefCell};
use paperpaw_sdk::{Env, Id};

struct Target {
    directory: Weak<NativeDeclarations>,
    depth: FlatDepth,
    calls: Rc<RefCell<Vec<u32>>>,
    failure: Rc<Cell<i32>>,
    live: Cell<bool>,
    guard: crate::abi7::calls::Instance,
}
impl Dispatch for Target {
    fn dropped(&self, _: u32, status: i32) {
        assert_eq!(status, self.failure.get());
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
        _: &wire::DrawFrame<'_>,
        _: &Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        unreachable!()
    }
    fn stage(&self, token: u32, bytes: &[u8], out: &mut [u8], _: &Env<'_>) -> Result<i32, i32> {
        let _guard = self
            .guard
            .enter(&self.depth.as_depth().0)
            .map_err(|_| ERR_BUSY)?;
        assert_eq!(self.depth.as_depth().0.current(), 2);
        let directory = self.directory.upgrade().unwrap();
        assert!(
            directory.store.try_borrow_mut().is_ok(),
            "no store borrow crosses target code"
        );
        self.calls.borrow_mut().push(token);
        if self.failure.get() != 0 {
            return Ok(self.failure.get());
        }
        let Record::StageCall(call) = wire::records_decode_single(bytes)? else {
            return Err(ERR_INVALID);
        };
        let ask = status::StatusAsk::decode(call.payload)?;
        let length = status::reply_encode(
            &[status::Record::StatusValue(status::StatusValue::new(
                ask.items, 0, "value",
            ))],
            out,
        )?;
        Ok(i32::try_from(length).unwrap())
    }
}
impl NativeInstance for Target {
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(Some(&[]));
    }

    fn running(&self) -> bool {
        self.live.get()
    }
    fn stop(&self) {
        self.live.set(false);
    }
    fn callback_depth(&self) -> u8 {
        self.depth.as_depth().0.current()
    }
}

struct Fixture {
    directory: Rc<NativeDeclarations>,
    depth: FlatDepth,
    calls: Rc<RefCell<Vec<u32>>>,
    failure: Rc<Cell<i32>>,
}
impl Fixture {
    fn new() -> Self {
        let directory = Rc::new(NativeDeclarations::default());
        let depth = FlatDepth::try_new().unwrap();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let failure = Rc::new(Cell::new(0));
        let lease = NativeLease::try_new(Target {
            directory: Rc::downgrade(&directory),
            depth: depth.try_clone().unwrap(),
            calls: Rc::clone(&calls),
            failure: Rc::clone(&failure),
            live: Cell::new(true),
            guard: crate::abi7::calls::Instance::new(),
        })
        .unwrap();
        let rows = [
            Record::StatusItem(wire::StatusItem {
                name: "one",
                title: "One",
                tap: "",
                priority: 0,
                min_width: 0,
                token: 11,
                flags: 0,
            }),
            Record::StatusItem(wire::StatusItem {
                name: "two",
                title: "Two",
                tap: "",
                priority: 0,
                min_width: 0,
                token: 22,
                flags: 0,
            }),
        ];
        let mut bytes = [0; 256];
        let length = wire::records_encode(&rows, &mut bytes).unwrap();
        let bytes = &bytes[..length];
        let decoded = wire::records_decode(bytes).unwrap();
        let mut raw = RawDeclarations::default();
        let prepared = raw.prepare(None, Some(bytes), &decoded).unwrap();
        raw.commit(prepared);
        let owner = u64::MAX;
        let mut allocations = Allocations::default();
        let pending = native_declarations::pending(
            &Id::new_static("remote"),
            owner,
            lease,
            raw,
            &mut allocations,
        )
        .unwrap();
        directory
            .stage(pending, owner, &mut allocations)
            .unwrap()
            .commit();
        Self {
            directory,
            depth,
            calls,
            failure,
        }
    }
}

#[test]
fn owner_tokens_are_separate_calls_and_failures_leave_output_untouched() {
    let Fixture {
        directory,
        depth,
        calls,
        failure,
    } = Fixture::new();
    let owner = u64::MAX;
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let frame = CoreFrame {
        callback: Some(crate::abi7::core_frame::CallbackStamp::new(owner - 1, 1).unwrap()),
        instance: None,
        depth: Some(depth.as_depth()),
        directory: Some(&directory),
        env: &env,
        meter: &kernel.shared.meter,
        stack_generation: kernel.shared.stack_rows.generation(),
        stack_rows: &kernel.shared.stack_rows,
        font_removals: &kernel.shared.font_removals,
        registry_generation: &kernel.shared.registry_generation,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
        log: Default::default(),
        ui: None,
    };
    let host_calls = StatusCalls {
        frame: &frame,
        directory: &directory,
    };
    let caller = crate::abi7::calls::Instance::new();
    let _outer = caller.enter(&depth.as_depth().0).unwrap();
    let mut out = [77; 3840];
    for (token, item) in [(11, "remote.one"), (22, "remote.two")] {
        assert!(
            host_calls
                .ask(owner, token, &[item], &[], &mut out)
                .unwrap()
                > 0
        );
    }
    assert_eq!(&*calls.borrow(), &[11, 22]);
    assert_eq!(
        host_calls.ask(owner, 22, &["remote.one"], &[], &mut out),
        Err(ERR_DENIED)
    );
    failure.set(paperpaw_native::host_glue::ERR_IO);
    out.fill(77);
    assert_eq!(
        host_calls.ask(owner, 11, &["remote.one"], &[], &mut out),
        Err(paperpaw_native::host_glue::ERR_IO)
    );
    assert!(out.iter().all(|byte| *byte == 77));
    let nested = crate::abi7::calls::Instance::new();
    let _inner = nested.enter(&depth.as_depth().0).unwrap();
    assert_eq!(
        host_calls.ask(owner, 11, &["remote.one"], &[], &mut out),
        Err(ERR_BUSY)
    );
    assert_eq!(&*calls.borrow(), &[11, 22, 11]);
}
