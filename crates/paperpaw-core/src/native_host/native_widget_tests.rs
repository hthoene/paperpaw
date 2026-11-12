//! `Frame::widget_call`: the order of the gates, the framed input the owner's
//! stage dispatch sees, the checked reply, and a reply the stage's codec refuses.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::native_host::{
    declaration_store::Allocations,
    flat::FlatDepth,
    native_declarations::{self, NativeDeclarations},
    native_instances::{NativeInstance, NativeLease},
    raw_declarations::RawDeclarations,
    registrations::Record,
};
extern crate std;
use alloc::{rc::Rc, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_NOT_FOUND, ERR_STATE};
use paperpaw_platform::MemoryClass;
use paperpaw_sdk::{Env, Id};

const OWNER: u64 = u64::MAX;
const CALLER: u64 = 7;
const TOKEN: u32 = 33;

struct Target {
    depth: FlatDepth,
    guard: crate::abi7::calls::Instance,
    /// The stage-local records each call carried, and what it answers.
    seen: Rc<RefCell<Vec<widget::Record>>>,
    answer: Rc<RefCell<Vec<u8>>>,
    dropped: Rc<RefCell<Vec<i32>>>,
}
impl NativeInstance for Target {
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
impl Dispatch for Target {
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
        let _level = self.guard.enter(&self.depth.as_depth().0).unwrap();
        assert_eq!(token, TOKEN);
        assert_eq!(self.depth.as_depth().0.current(), 2);
        let Record::StageCall(call) = codec::decode_any_single(bytes).unwrap() else {
            panic!("not a stage.call")
        };
        assert_eq!(
            (call.stage, call.schema),
            (widget::KEY, u32::from(widget::MAJOR))
        );
        let records = widget::input_decode(call.payload).unwrap();
        self.seen
            .borrow_mut()
            .extend(records.map(|record| record.unwrap()));
        let answer = self.answer.borrow();
        // A reply that does not fit the caller's buffer is the stage's own
        // refusal (the host hands it the caller's buffer as it is).
        if out.len() < answer.len() {
            return Err(paperpaw_native::host_glue::ERR_NO_SPACE);
        }
        out[..answer.len()].copy_from_slice(&answer);
        Ok(i32::try_from(answer.len()).unwrap())
    }
    fn dropped(&self, _: u32, status: i32) {
        self.dropped.borrow_mut().push(status);
    }
}

struct World {
    directory: Rc<NativeDeclarations>,
    depth: FlatDepth,
    kernel: crate::Kernel,
    seen: Rc<RefCell<Vec<widget::Record>>>,
    answer: Rc<RefCell<Vec<u8>>>,
    dropped: Rc<RefCell<Vec<i32>>>,
    owner: Cell<u64>,
}

fn world(device: paperpaw_platform::DeviceInfo) -> World {
    let directory = Rc::new(NativeDeclarations::default());
    let depth = FlatDepth::try_new().unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let answer = Rc::new(RefCell::new(Vec::new()));
    let dropped = Rc::new(RefCell::new(Vec::new()));
    let lease = NativeLease::try_new(Target {
        depth: depth.try_clone().unwrap(),
        guard: crate::abi7::calls::Instance::new(),
        seen: Rc::clone(&seen),
        answer: Rc::clone(&answer),
        dropped: Rc::clone(&dropped),
    })
    .unwrap();
    // The directory holds the owner by a declaration; a status item stands in.
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
    World {
        directory,
        depth,
        kernel: crate::Kernel::builder(device).build().unwrap(),
        seen,
        answer,
        dropped,
        owner: Cell::new(OWNER),
    }
}

impl World {
    fn size_reply(&self) {
        let mut bytes = [0; 64];
        let used = widget::reply_encode(
            &[widget::Record::WidgetSize(widget::WidgetSize::new(40, 20))],
            &mut bytes,
        )
        .unwrap();
        *self.answer.borrow_mut() = bytes[..used].to_vec();
    }

    fn call(&self, with_callback: bool, out: &mut [u8]) -> Result<usize, i32> {
        let env = self.kernel.shared.env(paperpaw_platform::Instant(0));
        let frame = CoreFrame {
            callback: with_callback
                .then(|| crate::abi7::core_frame::CallbackStamp::new(CALLER, 1).unwrap()),
            instance: None,
            depth: Some(self.depth.as_depth()),
            directory: Some(&self.directory),
            env: &env,
            meter: &self.kernel.shared.meter,
            stack_generation: self.kernel.shared.stack_rows.generation(),
            stack_rows: &self.kernel.shared.stack_rows,
            font_removals: &self.kernel.shared.font_removals,
            registry_generation: &self.kernel.shared.registry_generation,
            usage: &self.kernel.shared.usage,
            plugins: &self.kernel.shared.plugins,
            work: &self.kernel.shared.work_states,
            log: Default::default(),
            ui: None,
        };
        let measure = widget::Record::WidgetMeasure(widget::WidgetMeasure::new(9, 200));
        let mut payload = [0; 64];
        let length = widget::input_encode(&[measure], &mut payload).unwrap();
        super::call(&frame, self.owner.get(), TOKEN, &payload[..length], out)
    }
}

#[test]
fn the_owner_gets_one_framed_record_and_its_checked_reply_comes_back() {
    let w = world(crate::tests::DEVICE);
    w.size_reply();
    let caller = crate::abi7::calls::Instance::new();
    let _outer = caller.enter(&w.depth.as_depth().0).unwrap();
    let mut out = [0; 3840];
    let length = w.call(true, &mut out).unwrap();
    assert_eq!(&out[..length], &*w.answer.borrow());
    assert_eq!(
        *w.seen.borrow(),
        [widget::Record::WidgetMeasure(widget::WidgetMeasure::new(
            9, 200
        ))]
    );
    assert!(w.dropped.borrow().is_empty());
}

#[test]
fn the_gates_refuse_before_the_owner_is_called() {
    let w = world(crate::tests::DEVICE);
    w.size_reply();
    let mut out = [0; 3840];
    // Outside a callback.
    assert_eq!(w.call(false, &mut out), Err(ERR_STATE));
    let first = crate::abi7::calls::Instance::new();
    let second = crate::abi7::calls::Instance::new();
    let _outer = first.enter(&w.depth.as_depth().0).unwrap();
    // The caller's own plugin is never called back.
    w.owner.set(CALLER);
    assert_eq!(w.call(true, &mut out), Err(ERR_BUSY));
    // An owner the directory does not know.
    w.owner.set(OWNER - 1);
    assert_eq!(w.call(true, &mut out), Err(ERR_NOT_FOUND));
    w.owner.set(OWNER);
    // A caller that is itself nested.
    let _inner = second.enter(&w.depth.as_depth().0).unwrap();
    assert_eq!(w.call(true, &mut out), Err(ERR_BUSY));
    assert!(w.seen.borrow().is_empty());
}

#[test]
#[cfg_attr(feature = "x4-nested", ignore = "x4-nested opens the X4 gate (#250)")]
fn a_small_device_is_unsupported_until_its_stack_gate_passes() {
    let w = world(crate::tests::DEVICE.with_memory(MemoryClass::Small));
    w.size_reply();
    let caller = crate::abi7::calls::Instance::new();
    let _outer = caller.enter(&w.depth.as_depth().0).unwrap();
    assert_eq!(w.call(true, &mut [0; 3840]), Err(ERR_UNSUPPORTED));
    assert!(w.seen.borrow().is_empty());
}

#[test]
fn a_reply_the_codec_refuses_or_the_caller_cannot_hold_is_the_calls_failure() {
    let w = world(crate::tests::DEVICE);
    let caller = crate::abi7::calls::Instance::new();
    let _outer = caller.enter(&w.depth.as_depth().0).unwrap();
    // A status row is not a widget reply row.
    *w.answer.borrow_mut() = std::vec![1, 9, 9];
    assert!(w.call(true, &mut [0; 3840]).is_err());
    assert_eq!(w.dropped.borrow().len(), 1, "the owner is told");
    // An output too short for a good reply.
    w.size_reply();
    assert_eq!(w.call(true, &mut [0; 2]), Err(ERR_NO_SPACE));
    assert_eq!(w.dropped.borrow().len(), 2);
}
