//! The format call's transport: framing, lifetime guards, statuses, bounds.
#![allow(clippy::unwrap_used, reason = "fixture refusal must fail the test")]
use super::*;
use crate::native_host::{
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
};
use alloc::{rc::Rc, vec, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED};
use paperpaw_sdk::Id;

type Seen = Rc<RefCell<Vec<(u32, Vec<u8>)>>>;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Behavior {
    /// Answers one stage-local record (tag 5, two bytes).
    Good,
    /// The format's own status as the callback's result.
    Status(i32),
    /// A callback that fails to dispatch.
    Fail(i32),
    /// Claims more bytes than the buffer holds.
    Oversize,
    /// Claims bytes that are not whole frames.
    Truncated,
    /// More than 64 records.
    TooMany,
    /// Exactly these bytes as the reply.
    Raw(&'static [u8]),
    /// Writes over all its reply room, then answers this status.
    Scribble(i32),
    /// Notes the heap held while it runs ([`HELD_AT_STAGE`]), then as `Good`.
    Measure,
}

extern crate std;
std::thread_local! {
    /// The test allocator's live bytes when a `Measure` format ran.
    static HELD_AT_STAGE: Cell<usize> = const { Cell::new(0) };
}

pub(crate) struct Fake {
    pub(crate) seen: Seen,
    pub(crate) dropped: Rc<RefCell<Vec<(u32, i32)>>>,
    behavior: Behavior,
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
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &Env<'_>,
        _: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn dropped(&self, token: u32, status: i32) {
        self.dropped.borrow_mut().push((token, status));
    }
    fn stage(&self, token: u32, bytes: &[u8], out: &mut [u8], _: &Env<'_>) -> Result<i32, i32> {
        self.seen.borrow_mut().push((token, bytes.to_vec()));
        match self.behavior {
            Behavior::Good if out.len() < 5 => Ok(5),
            Behavior::Good => {
                out[..5].copy_from_slice(&[1, 5, 2, 0xaa, 0xbb]);
                Ok(5)
            }
            Behavior::Status(status) => Ok(status),
            Behavior::Fail(status) => Err(status),
            Behavior::Oversize => Ok(i32::try_from(out.len() + 1).unwrap()),
            Behavior::Truncated => {
                out[..4].copy_from_slice(&[1, 5, 9, 0]);
                Ok(4)
            }
            Behavior::TooMany => {
                out[0] = 65;
                Ok(1)
            }
            Behavior::Raw(bytes) if out.len() < bytes.len() => {
                Ok(i32::try_from(bytes.len()).unwrap())
            }
            Behavior::Raw(bytes) => {
                out[..bytes.len()].copy_from_slice(bytes);
                Ok(i32::try_from(bytes.len()).unwrap())
            }
            Behavior::Scribble(status) => {
                out.fill(0xee);
                Ok(status)
            }
            Behavior::Measure => {
                let live = crate::native_host::shell::tests::HEAP_LIVE.with(Cell::get);
                HELD_AT_STAGE.with(|held| held.set(live));
                out[..5].copy_from_slice(&[1, 5, 2, 0xaa, 0xbb]);
                Ok(5)
            }
        }
    }
}

pub(crate) struct Harness {
    pub(crate) registry: NativeInstances,
    pub(crate) seen: Seen,
    pub(crate) dropped: Rc<RefCell<Vec<(u32, i32)>>>,
}
impl Harness {
    pub(crate) fn new() -> Self {
        Self {
            registry: NativeInstances::default(),
            seen: Rc::new(RefCell::new(Vec::new())),
            dropped: Rc::new(RefCell::new(Vec::new())),
        }
    }
    pub(crate) fn add(&mut self, id: &'static str, behavior: Behavior) -> u64 {
        let lease = NativeLease::try_new(Fake {
            seen: self.seen.clone(),
            dropped: self.dropped.clone(),
            behavior,
            live: Cell::new(true),
        })
        .unwrap();
        self.registry
            .test_insert(Id::new_static(id), Some(lease), true)
            .unwrap()
    }
}

fn run(h: &Harness, plugin: u64, payload: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    call(
        &h.registry,
        Target { plugin, token: 7 },
        payload,
        out,
        &kernel.shared.env(paperpaw_platform::Instant(0)),
    )
}

#[test]
fn a_call_is_one_stage_call_record_and_the_reply_comes_back_whole() {
    let mut h = Harness::new();
    let plugin = h.add("format-epub", Behavior::Good);
    let mut out = [0_u8; 64];
    assert_eq!(run(&h, plugin, &[1, 2, 3], &mut out), Ok(5));
    assert_eq!(out[..5], [1, 5, 2, 0xaa, 0xbb]);
    let seen = h.seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, 7, "the format's declared token");
    let Record::StageCall(framed) = wire::records_decode_single(&seen[0].1).unwrap() else {
        panic!("stage.call")
    };
    assert_eq!(
        (framed.stage, framed.schema, framed.payload),
        ("core.format", 1, &[1_u8, 2, 3][..])
    );
    assert_eq!(h.registry.depth(), 0, "the epoch ends with the call");
}

#[test]
fn payload_limit_and_limit_plus_one() {
    let mut h = Harness::new();
    let plugin = h.add("format-epub", Behavior::Good);
    let mut out = [0_u8; 64];
    let exact = vec![0_u8; PAYLOAD_MAX];
    assert_eq!(run(&h, plugin, &exact, &mut out), Ok(5));
    let seen = h.seen.borrow().len();
    let over = vec![0_u8; PAYLOAD_MAX + 1];
    assert_eq!(run(&h, plugin, &over, &mut out), Err(ERR_NO_SPACE));
    assert_eq!(h.seen.borrow().len(), seen, "the format is not entered");
}

#[test]
fn the_formats_own_status_passes_through_and_is_no_misbehaviour() {
    let mut h = Harness::new();
    let not_found = h.add("a", Behavior::Status(ERR_NOT_FOUND));
    let state = h.add("b", Behavior::Status(ERR_STATE));
    let busy = h.add("c", Behavior::Fail(ERR_BUSY));
    let mut out = [0_u8; 64];
    assert_eq!(run(&h, not_found, &[], &mut out), Err(ERR_NOT_FOUND));
    assert_eq!(run(&h, state, &[], &mut out), Err(ERR_STATE));
    assert_eq!(run(&h, busy, &[], &mut out), Err(ERR_BUSY));
    assert!(h.dropped.borrow().is_empty());
    assert_eq!(out, [0; 64], "nothing was written");
}

#[test]
fn a_reply_the_host_cannot_take_is_invalid_dropped_and_never_written() {
    let mut h = Harness::new();
    let big = h.add("a", Behavior::Oversize);
    let cut = h.add("b", Behavior::Truncated);
    let many = h.add("c", Behavior::TooMany);
    for plugin in [big, cut, many] {
        let mut out = [0_u8; 64];
        assert_eq!(run(&h, plugin, &[], &mut out), Err(ERR_INVALID));
        assert_eq!(out, [0; 64]);
    }
    assert_eq!(h.dropped.borrow().len(), 3, "each is told once");
    assert!(
        h.dropped
            .borrow()
            .iter()
            .all(|(token, status)| *token == 7 && *status == ERR_INVALID)
    );
}

#[test]
fn a_reply_longer_than_the_callers_buffer_is_refused() {
    let mut h = Harness::new();
    let plugin = h.add("format-epub", Behavior::Good);
    let mut out = [0_u8; 4];
    assert_eq!(run(&h, plugin, &[], &mut out), Err(ERR_INVALID));
}

#[test]
fn a_lazy_format_asks_for_its_start_and_a_stopped_one_is_cancelled() {
    let mut h = Harness::new();
    let lazy = h
        .registry
        .test_insert(Id::new_static("lazy"), None, true)
        .unwrap();
    let stopped = h.add("gone", Behavior::Good);
    h.registry.stop(stopped).unwrap();
    let mut out = [0_u8; 64];
    assert_eq!(run(&h, lazy, &[], &mut out), Err(ERR_BUSY));
    assert_eq!(h.registry.take_start(), Some(lazy));
    assert_eq!(run(&h, stopped, &[], &mut out), Err(ERR_CANCELLED));
    assert!(h.seen.borrow().is_empty());
    assert_eq!(h.registry.depth(), 0);
}

#[test]
fn what_a_format_wrote_before_an_error_is_zeros_and_past_its_room_untouched() {
    let mut h = Harness::new();
    let failing = h.add("a", Behavior::Scribble(ERR_STATE));
    let cut = h.add("b", Behavior::Truncated);
    for plugin in [failing, cut] {
        let mut out = vec![0xa5_u8; PAYLOAD_MAX + 16];
        assert!(run(&h, plugin, &[], &mut out).is_err());
        assert!(out[..PAYLOAD_MAX].iter().all(|byte| *byte == 0));
        assert!(out[PAYLOAD_MAX..].iter().all(|byte| *byte == 0xa5));
    }
    // Not asked at all (a stopped format): untouched.
    let stopped = h.add("c", Behavior::Good);
    h.registry.stop(stopped).unwrap();
    let mut out = [0xa5_u8; 8];
    assert_eq!(run(&h, stopped, &[], &mut out), Err(ERR_CANCELLED));
    assert_eq!(out, [0xa5; 8]);
}

#[test]
fn the_format_answers_into_the_callers_buffer_and_no_copy_of_it_is_held() {
    let mut h = Harness::new();
    let plugin = h.add("format-epub", Behavior::Measure);
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let mut out = vec![0_u8; 4096];
    let before = crate::native_host::shell::tests::HEAP_LIVE.with(Cell::get);
    let target = Target { plugin, token: 7 };
    assert_eq!(call(&h.registry, target, &[1, 2, 3], &mut out, &env), Ok(5));
    assert_eq!(out[..5], [1, 5, 2, 0xaa, 0xbb]);
    let held = HELD_AT_STAGE.with(Cell::get).saturating_sub(before);
    // The framed call and the fixture's note of it; never 3,840 bytes of reply.
    assert!(held < 512, "{held} bytes held while the format ran");
}
