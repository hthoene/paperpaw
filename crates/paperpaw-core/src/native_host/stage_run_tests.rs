//! `stage.run` from a native plugin: the owner asks its subscribers one by
//! one, in order, by cursor, and a failing subscriber only costs its reply.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::{NORMAL, arm_failure, reset};
use crate::abi7::core_frame::{CallbackStamp, CoreFrame};
use crate::native_host::{
    codec,
    declaration_store::Allocations,
    flat::FlatDepth,
    native_declarations::{self, NativeDeclarations},
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    raw_declarations::RawDeclarations,
    registrations::{self as wire, Record},
    stage_run::run,
};
use alloc::{rc::Rc, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    mem::MaybeUninit,
};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_CANCELLED, ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND,
    ERR_STATE, ERR_UNSUPPORTED,
};
use paperpaw_platform::MemoryClass;
use paperpaw_sdk::{Env, Id};

const READER: u64 = 100;
const STAGE: &str = "reader.filter";

#[derive(Clone, Copy)]
enum Mode {
    /// An empty record batch.
    Good,
    /// The subscriber's own refusal.
    Refuse,
    /// A batch with more records than the stage allows.
    Garbage,
    /// A length beyond the buffer it was lent.
    Oversize,
}

struct Target {
    id: &'static str,
    depth: FlatDepth,
    mode: Rc<Cell<Mode>>,
    live: Rc<Cell<bool>>,
    calls: Rc<RefCell<Vec<(&'static str, u32)>>>,
    dropped: Rc<RefCell<Vec<i32>>>,
    guard: crate::abi7::calls::Instance,
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
    fn dropped(&self, _: u32, status: i32) {
        self.dropped.borrow_mut().push(status);
    }
    fn stage(&self, token: u32, bytes: &[u8], out: &mut [u8], _: &Env<'_>) -> Result<i32, i32> {
        let _guard = self
            .guard
            .enter(&self.depth.as_depth().0)
            .map_err(|_| ERR_BUSY)?;
        // The subscriber runs one level below the stage's owner.
        assert_eq!(self.depth.as_depth().0.current(), 2);
        let Record::StageCall(call) = wire::records_decode_single(bytes)? else {
            return Err(ERR_INVALID);
        };
        assert_eq!((call.stage, call.schema), (STAGE, 3));
        self.calls.borrow_mut().push((self.id, token));
        match self.mode.get() {
            Mode::Good => {
                // The input comes back as the reply, behind an empty batch.
                out[0] = 0;
                Ok(1)
            }
            Mode::Refuse => Ok(ERR_STATE),
            Mode::Garbage => {
                // More records than the stage's `max_records` (4).
                out[..2].copy_from_slice(&[9, 0]);
                Ok(2)
            }
            Mode::Oversize => Ok(i32::try_from(out.len() + 1).unwrap()),
        }
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
    mode: Rc<Cell<Mode>>,
    live: Rc<Cell<bool>>,
    /// `first` and `alpha` have their own: they stop alone.
    first_live: Rc<Cell<bool>>,
    alpha_live: Rc<Cell<bool>>,
    calls: Rc<RefCell<Vec<(&'static str, u32)>>>,
    dropped: Rc<RefCell<Vec<i32>>>,
}

impl Fixture {
    /// The reader, with the stage `reader.filter` (schema 3, input up to 100
    /// bytes, replies up to 200), and three subscribers: `zeta` (order 5,
    /// token 5), `alpha` (order 5, token 6) and `first` (order -1, token 7).
    /// One mode and one liveness for all three.
    fn new() -> Self {
        let directory = Rc::new(NativeDeclarations::default());
        let depth = FlatDepth::try_new().unwrap();
        let fixture = Self {
            directory,
            depth,
            mode: Rc::new(Cell::new(Mode::Good)),
            live: Rc::new(Cell::new(true)),
            first_live: Rc::new(Cell::new(true)),
            alpha_live: Rc::new(Cell::new(true)),
            // Room for every call up front: the sweep must refuse only the host's
            // allocations.
            calls: Rc::new(RefCell::new(Vec::with_capacity(8))),
            dropped: Rc::new(RefCell::new(Vec::new())),
        };
        fixture.install(
            "reader",
            READER,
            &[Record::StageSpec(wire::StageSpec {
                name: "filter",
                schema: 3,
                flags: 0,
                max_input: 100,
                max_reply: 200,
                max_records: 4,
            })],
        );
        for (id, owner, token, order) in
            [("zeta", 1, 5, 5), ("alpha", 2, 6, 5), ("first", 3, 7, -1)]
        {
            fixture.install(
                id,
                owner,
                &[Record::RegistrationStage(wire::RegistrationStage {
                    stage: STAGE,
                    token,
                    order,
                    signature: u64::from(token) * 10,
                })],
            );
        }
        fixture
    }

    fn install(&self, id: &'static str, owner: u64, rows: &[Record<'_>]) {
        let lease = NativeLease::try_new(Target {
            id,
            depth: self.depth.try_clone().unwrap(),
            mode: Rc::clone(&self.mode),
            live: Rc::clone(match id {
                "first" => &self.first_live,
                "alpha" => &self.alpha_live,
                _ => &self.live,
            }),
            calls: Rc::clone(&self.calls),
            dropped: Rc::clone(&self.dropped),
            guard: crate::abi7::calls::Instance::new(),
        })
        .unwrap();
        self.install_lease(lease, id, owner, rows);
    }

    /// `id` declared as `owner` with `rows`, its callbacks reached by `lease`.
    fn install_lease(&self, lease: NativeLease, id: &'static str, owner: u64, rows: &[Record<'_>]) {
        let mut bytes = [0; 512];
        let length = wire::records_encode(rows, &mut bytes).unwrap();
        let bytes = &bytes[..length];
        let decoded = wire::records_decode(bytes).unwrap();
        let mut raw = RawDeclarations::default();
        let prepared = raw.prepare(None, Some(bytes), &decoded).unwrap();
        raw.commit(prepared);
        let mut allocations = Allocations::default();
        let pending = native_declarations::pending(
            &Id::try_parse(id).unwrap(),
            owner,
            lease,
            raw,
            &mut allocations,
        )
        .unwrap();
        self.directory
            .stage(pending, owner, &mut allocations)
            .unwrap()
            .commit();
    }
}

/// Runs `body` with a frame of the plugin `caller`, `levels` callbacks deep.
fn with_frame<R>(
    fixture: &Fixture,
    caller: Option<u64>,
    levels: usize,
    body: impl FnOnce(&CoreFrame<'_>) -> R,
) -> R {
    with_frame_at(fixture, caller, levels, (1, &[], MemoryClass::Large), body)
}

/// [`with_frame`] in the callback `epoch`, with the style grants of the
/// settings in `switches` defined and set.
fn with_frame_at<R>(
    fixture: &Fixture,
    caller: Option<u64>,
    levels: usize,
    (epoch, switches, memory): (u64, &[(&str, bool)], MemoryClass),
    body: impl FnOnce(&CoreFrame<'_>) -> R,
) -> R {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE.with_memory(memory))
        .build()
        .unwrap();
    for &(key, value) in switches {
        let key: &'static str =
            alloc::boxed::Box::leak(alloc::string::String::from(key).into_boxed_str());
        kernel
            .shared
            .settings
            .define(paperpaw_sdk::SettingSpec::switch(
                key,
                "switch",
                true,
                paperpaw_sdk::builtin::groups::OTHER,
            ))
            .unwrap();
        kernel
            .shared
            .settings
            .set(key, paperpaw_sdk::SettingValue::Bool(value))
            .unwrap();
    }
    with_kernel_frame(&kernel, fixture, caller, (levels, epoch), body)
}

/// [`with_frame_at`] on a kernel of the caller's.
fn with_kernel_frame<R>(
    kernel: &crate::Kernel,
    fixture: &Fixture,
    caller: Option<u64>,
    (levels, epoch): (usize, u64),
    body: impl FnOnce(&CoreFrame<'_>) -> R,
) -> R {
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let frame = CoreFrame {
        callback: caller.map(|owner| CallbackStamp::new(owner, epoch).unwrap()),
        instance: None,
        depth: Some(fixture.depth.as_depth()),
        directory: Some(&fixture.directory),
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
    let guards: Vec<_> = (0..levels)
        .map(|_| crate::abi7::calls::Instance::new())
        .collect();
    let _entered: Vec<_> = guards
        .iter()
        .map(|guard| guard.enter(&fixture.depth.as_depth().0).unwrap())
        .collect();
    body(&frame)
}

fn ask(
    frame: &CoreFrame<'_>,
    stage: &str,
    cursor: u32,
    input: &[u8],
    out: &mut [MaybeUninit<u8>],
) -> Result<usize, i32> {
    run(frame, stage, cursor, input, out)
}

/// The record at the front of `out` after `count` bytes.
fn reply(out: &[MaybeUninit<u8>], count: usize) -> wire::StageReply<'static> {
    // SAFETY: the first `count` bytes were written by the call.
    let bytes: &'static [u8] =
        unsafe { core::slice::from_raw_parts(out.as_ptr().cast::<u8>(), count) };
    let bytes: &'static [u8] = Vec::leak(bytes.to_vec());
    // One bare framed record, as `record_encode_one` makes it.
    match codec::decode_any_prefix(bytes).unwrap() {
        (Record::StageReply(reply), used) => {
            assert_eq!(used, count);
            reply
        }
        (other, _) => panic!("{other:?}"),
    }
}

fn buffer() -> Vec<MaybeUninit<u8>> {
    let mut out = Vec::new();
    out.resize(4096, MaybeUninit::new(77));
    out
}

fn untouched(out: &[MaybeUninit<u8>]) -> bool {
    // SAFETY: the buffer was filled with 77 before the call.
    out.iter().all(|byte| unsafe { byte.assume_init() } == 77)
}

#[test]
fn the_owner_loops_the_cursor_over_the_subscribers_in_stage_order() {
    let fixture = Fixture::new();
    with_frame(&fixture, Some(READER), 1, |frame| {
        let mut seen = Vec::new();
        for cursor in 0.. {
            let mut out = buffer();
            match ask(frame, STAGE, cursor, b"in", &mut out) {
                Ok(count) => {
                    let reply = reply(&out, count);
                    assert_eq!((reply.index, reply.status, reply.flags), (cursor, 0, 0));
                    assert_eq!(reply.payload, &[0]);
                    seen.push((reply.subscriber, reply.signature));
                }
                Err(status) => {
                    assert_eq!(status, ERR_NOT_FOUND);
                    assert!(untouched(&out), "nothing written past the last");
                    break;
                }
            }
        }
        // By order, then plugin id: `first` (-1), `alpha`, `zeta` (both 5).
        assert_eq!(seen, [("first", 70), ("alpha", 60), ("zeta", 50)]);
    });
    assert_eq!(
        &*fixture.calls.borrow(),
        &[("first", 7), ("alpha", 6), ("zeta", 5)]
    );
    assert_eq!(fixture.depth.as_depth().0.current(), 0);
}

#[test]
fn every_check_refuses_before_a_subscriber_runs_and_leaves_out_alone() {
    let fixture = Fixture::new();
    let big = [0u8; 101];
    with_frame(&fixture, Some(READER), 1, |frame| {
        let mut out = buffer();
        // Not a declared stage; the host's own stages are not run by plugins.
        for stage in ["reader.nothing", "core.status", "other.filter"] {
            assert_eq!(ask(frame, stage, 0, b"", &mut out), Err(ERR_NOT_FOUND));
        }
        // An input over the stage's bound.
        assert_eq!(ask(frame, STAGE, 0, &big, &mut out), Err(ERR_NO_SPACE));
        // An out too small for the largest reply (200 bytes and a 64-byte id).
        let mut small = Vec::new();
        small.resize(250, MaybeUninit::new(77));
        assert_eq!(ask(frame, STAGE, 0, b"", &mut small), Err(ERR_NO_SPACE));
        assert!(untouched(&small));
        assert!(untouched(&out));
    });
    // Another plugin does not run the reader's stage.
    with_frame(&fixture, Some(1), 1, |frame| {
        let mut out = buffer();
        assert_eq!(ask(frame, STAGE, 0, b"", &mut out), Err(ERR_DENIED));
        assert!(untouched(&out));
    });
    // Outside a callback, and from a subscriber's (depth 2): state.
    with_frame(&fixture, None, 1, |frame| {
        let mut out = buffer();
        assert_eq!(ask(frame, STAGE, 0, b"", &mut out), Err(ERR_STATE));
    });
    with_frame(&fixture, Some(READER), 2, |frame| {
        let mut out = buffer();
        assert_eq!(ask(frame, STAGE, 0, b"", &mut out), Err(ERR_STATE));
        assert!(untouched(&out));
    });
    assert!(
        fixture.calls.borrow().is_empty(),
        "no subscriber was called"
    );
}

#[test]
fn a_subscriber_that_fails_or_is_too_big_costs_only_its_reply() {
    let fixture = Fixture::new();
    with_frame(&fixture, Some(READER), 1, |frame| {
        for (mode, status) in [
            (Mode::Refuse, ERR_STATE),
            (Mode::Garbage, ERR_INVALID),
            (Mode::Oversize, ERR_INVALID),
        ] {
            fixture.mode.set(mode);
            let mut out = buffer();
            let count = ask(frame, STAGE, 0, b"", &mut out).unwrap();
            let reply = reply(&out, count);
            assert_eq!((reply.status, reply.payload.len()), (status, 0));
            assert_eq!(reply.subscriber, "first");
        }
        // The next subscriber still answers.
        fixture.mode.set(Mode::Good);
        let mut out = buffer();
        let count = ask(frame, STAGE, 1, b"", &mut out).unwrap();
        assert_eq!(reply(&out, count).status, 0);
    });
    // What the host could not take, the subscriber was told of.
    assert_eq!(&*fixture.dropped.borrow(), &[ERR_INVALID, ERR_INVALID]);
}

#[test]
fn a_plugin_stopped_before_the_callback_is_not_a_member() {
    // Its declarations died with it before the callback froze the list.
    let fixture = Fixture::new();
    fixture.first_live.set(false);
    with_frame(&fixture, Some(READER), 1, |frame| {
        let mut out = buffer();
        let count = ask(frame, STAGE, 0, b"", &mut out).unwrap();
        assert_eq!(reply(&out, count).subscriber, "alpha");
        let count = ask(frame, STAGE, 1, b"", &mut out).unwrap();
        assert_eq!(reply(&out, count).subscriber, "zeta");
        assert_eq!(ask(frame, STAGE, 2, b"", &mut out), Err(ERR_NOT_FOUND));
    });
    assert_eq!(&*fixture.calls.borrow(), &[("alpha", 6), ("zeta", 5)]);
}

#[test]
fn the_list_is_frozen_for_the_callback_and_a_member_that_stops_answers_cancelled() {
    let fixture = Fixture::new();
    with_frame_at(
        &fixture,
        Some(READER),
        1,
        (2, &[], MemoryClass::Large),
        |frame| {
            let mut out = buffer();
            let count = ask(frame, STAGE, 0, b"", &mut out).unwrap();
            assert_eq!(reply(&out, count).subscriber, "first");
            // `alpha` stops between two cursors: it keeps its place.
            fixture.alpha_live.set(false);
            let count = ask(frame, STAGE, 1, b"", &mut out).unwrap();
            let alpha = reply(&out, count);
            assert_eq!(
                (
                    alpha.subscriber,
                    alpha.index,
                    alpha.status,
                    alpha.payload.len()
                ),
                ("alpha", 1, ERR_CANCELLED, 0)
            );
            // Nobody is skipped: `zeta` is at 2, as the caller's loop expects.
            let count = ask(frame, STAGE, 2, b"", &mut out).unwrap();
            assert_eq!(reply(&out, count).subscriber, "zeta");
            assert_eq!(ask(frame, STAGE, 3, b"", &mut out), Err(ERR_NOT_FOUND));
        },
    );
    assert_eq!(&*fixture.calls.borrow(), &[("first", 7), ("zeta", 5)]);
    // The next callback takes a fresh list, without the stopped plugin.
    with_frame_at(
        &fixture,
        Some(READER),
        1,
        (3, &[], MemoryClass::Large),
        |frame| {
            let mut out = buffer();
            let count = ask(frame, STAGE, 1, b"", &mut out).unwrap();
            assert_eq!(reply(&out, count).subscriber, "zeta");
        },
    );
}

#[test]
fn the_style_grant_of_the_subscriber_is_stamped_into_the_reply() {
    let fixture = Fixture::new();
    // Only `alpha` was granted; `first` and the unrelated `zeta` were not.
    with_frame_at(
        &fixture,
        Some(READER),
        1,
        (2, &[("core.grant.style.alpha", true)], MemoryClass::Large),
        |frame| {
            let mut out = buffer();
            let mut flags = Vec::new();
            for cursor in 0..3 {
                let count = ask(frame, STAGE, cursor, b"", &mut out).unwrap();
                let reply = reply(&out, count);
                flags.push((reply.subscriber, reply.flags));
            }
            assert_eq!(flags, [("first", 0), ("alpha", 1), ("zeta", 0)]);
        },
    );
}

#[test]
fn a_refused_allocation_is_no_memory_and_calls_and_writes_nothing() {
    reset(NORMAL);
    let fixture = Fixture::new();
    let mut ran = false;
    for nth in 1..=48_u32 {
        // A callback of its own each time, so that every freeze is asked.
        with_frame_at(
            &fixture,
            Some(READER),
            1,
            (u64::from(nth), &[], MemoryClass::Large),
            |frame| {
                let mut out = buffer();
                fixture.calls.borrow_mut().clear();
                arm_failure(nth);
                let result = ask(frame, STAGE, 0, b"in", &mut out);
                arm_failure(0);
                match result {
                    Ok(count) => {
                        ran = true;
                        assert_eq!(reply(&out, count).status, 0);
                    }
                    Err(status) => {
                        assert_eq!(status, ERR_NO_MEMORY, "allocation {nth}");
                        assert!(untouched(&out), "allocation {nth}");
                        assert!(fixture.calls.borrow().is_empty(), "allocation {nth}");
                    }
                }
            },
        );
    }
    // Past the last allocation the run succeeds.
    assert!(ran);
    assert_eq!(fixture.depth.as_depth().0.current(), 0);
}

#[test]
#[cfg_attr(feature = "x4-nested", ignore = "x4-nested opens the X4 gate (#250)")]
fn a_small_device_blocks_stage_runs_until_its_stack_gate_passes() {
    // Every class but `Large` is the small one (`ui_store::is_x4`).
    let fixture = Fixture::new();
    with_frame_at(
        &fixture,
        Some(READER),
        1,
        (2, &[], MemoryClass::Small),
        |frame| {
            let mut out = buffer();
            assert_eq!(ask(frame, STAGE, 0, b"in", &mut out), Err(ERR_UNSUPPORTED));
            assert!(untouched(&out));
        },
    );
    assert!(
        fixture.calls.borrow().is_empty(),
        "no subscriber was called"
    );
}

#[test]
fn the_frozen_lists_are_freed_when_the_callback_ends() {
    use crate::native_host::stage_members::Release;
    let fixture = Fixture::new();
    with_frame_at(
        &fixture,
        Some(READER),
        1,
        (7, &[], MemoryClass::Large),
        |frame| {
            let mut out = buffer();
            ask(frame, STAGE, 0, b"", &mut out).unwrap();
            assert!(!fixture.directory.members.borrow().is_empty());
            // A nested callback's end (another epoch) leaves the list alone.
            drop(Release::new(Some(&fixture.directory), 8, false));
            assert!(!fixture.directory.members.borrow().is_empty());
        },
    );
    drop(Release::new(Some(&fixture.directory), 7, false));
    assert!(fixture.directory.members.borrow().is_empty());
    // The next callback freezes a fresh list from the store.
    with_frame_at(
        &fixture,
        Some(READER),
        1,
        (9, &[], MemoryClass::Large),
        |frame| {
            let mut out = buffer();
            let count = ask(frame, STAGE, 0, b"", &mut out).unwrap();
            assert_eq!(reply(&out, count).subscriber, "first");
        },
    );
}

#[test]
fn an_outermost_callback_frees_what_an_earlier_path_left_frozen() {
    use crate::native_host::stage_members::Release;
    let fixture = Fixture::new();
    // A list frozen in a callback whose path had no release (epoch 4).
    with_frame_at(
        &fixture,
        Some(READER),
        1,
        (4, &[], MemoryClass::Large),
        |frame| {
            let mut out = buffer();
            ask(frame, STAGE, 0, b"", &mut out).unwrap();
        },
    );
    assert!(!fixture.directory.members.borrow().is_empty());
    // A nested callback starting (another one is running) leaves it alone.
    drop(Release::new(Some(&fixture.directory), 5, false));
    assert!(!fixture.directory.members.borrow().is_empty());
    // An outermost callback starting frees it, whatever its epoch.
    let outer = Release::new(Some(&fixture.directory), 6, true);
    assert!(fixture.directory.members.borrow().is_empty());
    drop(outer);
}

#[test]
fn a_subscriber_the_user_turned_off_is_inert_and_does_not_count() {
    let fixture = Fixture::new();
    // `alpha` is off for this stage, `zeta` is off for another one: only the
    // first changes the list.
    let off = [
        ("core.stage.reader.filter.alpha", false),
        ("core.stage.reader.other.zeta", false),
        ("core.stage.reader.filter.first", true),
    ];
    with_frame_at(
        &fixture,
        Some(READER),
        1,
        (2, &off, MemoryClass::Large),
        |frame| {
            let mut out = buffer();
            let mut seen = Vec::new();
            for cursor in 0.. {
                match ask(frame, STAGE, cursor, b"", &mut out) {
                    Ok(count) => seen.push(reply(&out, count).subscriber),
                    Err(status) => {
                        assert_eq!(status, ERR_NOT_FOUND);
                        break;
                    }
                }
            }
            assert_eq!(seen, ["first", "zeta"]);
        },
    );
    assert_eq!(&*fixture.calls.borrow(), &[("first", 7), ("zeta", 5)]);
    // Turned back on, it is in the next callback's list.
    with_frame_at(
        &fixture,
        Some(READER),
        1,
        (3, &[], MemoryClass::Large),
        |frame| {
            let mut out = buffer();
            let count = ask(frame, STAGE, 1, b"", &mut out).unwrap();
            assert_eq!(reply(&out, count).subscriber, "alpha");
        },
    );
}

#[test]
fn subscribers_turned_off_do_not_count_toward_the_thirty_two() {
    let fixture = Fixture::new();
    // 33 more subscribers at order 0, ahead of `alpha` and `zeta` (5) and
    // behind `first` (-1): 36 in all.
    let mut names = Vec::new();
    for number in 0..33_u32 {
        let id: &'static str =
            alloc::boxed::Box::leak(alloc::format!("p{number:02}").into_boxed_str());
        fixture.install(
            id,
            1000 + u64::from(number),
            &[Record::RegistrationStage(wire::RegistrationStage {
                stage: STAGE,
                token: 100 + number,
                order: 0,
                signature: 0,
            })],
        );
        names.push(id);
    }
    let run = |epoch: u64, off: &[(&str, bool)]| {
        let mut seen = Vec::new();
        with_frame_at(
            &fixture,
            Some(READER),
            1,
            (epoch, off, MemoryClass::Large),
            |frame| {
                let mut out = buffer();
                for cursor in 0..40 {
                    match ask(frame, STAGE, cursor, b"", &mut out) {
                        Ok(count) => {
                            seen.push(alloc::string::String::from(reply(&out, count).subscriber))
                        }
                        Err(status) => {
                            assert_eq!(status, ERR_NOT_FOUND);
                            break;
                        }
                    }
                }
            },
        );
        seen
    };
    // All on: only the first 32 are active (`first`, then p00 to p30).
    let all_on = run(2, &[]);
    assert_eq!(all_on.len(), 32);
    assert_eq!((all_on[0].as_str(), all_on[31].as_str()), ("first", "p30"));
    // `first`, `alpha`, `zeta` and p32 off: 32 are left, p00 to p31, and
    // none of the switched-off ones took a place.
    let off = [
        ("core.stage.reader.filter.first", false),
        ("core.stage.reader.filter.alpha", false),
        ("core.stage.reader.filter.zeta", false),
        ("core.stage.reader.filter.p32", false),
    ];
    let seen = run(3, &off);
    assert_eq!(seen.len(), 32);
    assert!(seen.iter().zip(&names).all(|(seen, name)| seen == name));
}

/// A real linked plugin as the subscriber: what its callback sees is its own.
mod native {
    extern crate std;
    use super::super::{Heap, NORMAL, reset};
    use super::*;
    use crate::native_host::{
        self, FlatHost, native_dispatch::NativeDispatch, retained::Retained, shell,
    };
    use core::{ffi::c_void, ptr};
    use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost, PpPlugin};
    use paperpaw_sdk::secrets::Running;

    std::thread_local! {
        static METER: RefCell<Option<Rc<crate::meter::Meter>>> = const { RefCell::new(None) };
        /// The caller the plugin's callback saw, and whether the meter named
        /// the plugin (`probe`) and the stage's owner (`reader`) as running.
        static SEEN: Cell<Option<(u64, bool, bool)>> = const { Cell::new(None) };
    }

    struct State {
        table: PpPlugin,
        context: *mut c_void,
    }

    unsafe extern "C" fn make(
        _: *const PpHost,
        context: *mut c_void,
        out: *mut *const PpPlugin,
    ) -> i32 {
        let state = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(State {
            table: PpPlugin {
                size: u32::try_from(size_of::<PpPlugin>()).unwrap(),
                state: ptr::null_mut(),
                drop: Some(destroy),
                dispatch_event: Some(event),
                draw: None,
                input: None,
            },
            context,
        }));
        // SAFETY: the host supplies exclusive output; this allocation lives
        // until `destroy`.
        unsafe {
            (*state).table.state = state.cast();
            out.write(&raw const (*state).table);
        }
        abi::OK
    }

    unsafe extern "C" fn destroy(state: *mut c_void) {
        // SAFETY: the owner destroys this allocation of `make` exactly once.
        unsafe { drop(alloc::boxed::Box::from_raw(state.cast::<State>())) };
    }

    unsafe extern "C" fn event(state: *mut c_void, _: u32, _: PpBytes, out: PpBuffer) -> i32 {
        // SAFETY: retained admission keeps the state and pinned host context
        // alive.
        let context = unsafe {
            &*(*state.cast::<State>())
                .context
                .cast::<shell::Context<Heap>>()
        };
        let caller = context.environment.with_frame(|frame| frame.caller());
        METER.with(|meter| {
            if let (Some(meter), Ok(Ok(caller))) = (&*meter.borrow(), caller) {
                SEEN.with(|seen| {
                    seen.set(Some((caller, meter.runs("probe"), meter.runs("reader"))));
                });
            }
        });
        // SAFETY: the callback receives exclusive, bounded output.
        let output = unsafe { core::slice::from_raw_parts_mut(out.ptr, out.cap) };
        // An empty record batch.
        output[0] = 0;
        1
    }

    fn instance(kernel: &crate::Kernel, depth: &FlatDepth) -> NativeLease {
        // SAFETY: the fixture's linked entry and code live throughout the test.
        let host = unsafe {
            shell::Hosted::new(
                Heap,
                (),
                make,
                "probe",
                (ptr::null(), 0),
                |_, _, _, _| {},
                native_host::HostResources::try_new().unwrap(),
                shell::DeclaredServices::none(),
            )
        }
        .unwrap();
        NativeLease::try_new(NativeDispatch::new(
            Retained::make(host).unwrap(),
            depth.try_clone().unwrap(),
            FlatHost {
                log: Default::default(),
                meter: Rc::clone(&kernel.shared.meter),
                directory: None,
                stack_rows: Rc::clone(&kernel.shared.stack_rows),
                font_removals: Rc::clone(&kernel.shared.font_removals),
                registry_generation: Rc::clone(&kernel.shared.registry_generation),
            },
            |_, _, _| {},
            native_host::native_instances::test_identity("probe"),
        ))
        .unwrap()
    }

    #[test]
    fn a_subscriber_sees_itself_not_the_stages_owner_and_the_owner_runs_again_after() {
        reset(NORMAL);
        let kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        let fixture = Fixture::new();
        // First in the stage's order.
        fixture.install_lease(
            instance(&kernel, &fixture.depth),
            "probe",
            50,
            &[Record::RegistrationStage(wire::RegistrationStage {
                stage: STAGE,
                token: 9,
                order: -5,
                signature: 0,
            })],
        );
        METER.with(|meter| *meter.borrow_mut() = Some(Rc::clone(&kernel.shared.meter)));
        SEEN.with(|seen| seen.set(None));
        // The stage's owner is the running plugin when it asks.
        let owner_running = kernel.shared.meter.enter("reader");
        with_kernel_frame(&kernel, &fixture, Some(READER), (1, 2), |frame| {
            let mut out = buffer();
            let count = ask(frame, STAGE, 0, b"", &mut out).unwrap();
            assert_eq!(reply(&out, count).subscriber, "probe");
        });
        let (caller, probe_runs, reader_runs) = SEEN.with(Cell::get).expect("the plugin ran");
        let own = native_host::native_instances::test_identity("probe").owner();
        assert_eq!(caller, own, "its own identity");
        assert_ne!(caller, READER, "not the stage owner's");
        assert!(probe_runs && !reader_runs, "the meter names the callee");
        assert!(kernel.shared.meter.runs("reader"), "the owner runs again");
        drop(owner_running);
        METER.with(|meter| *meter.borrow_mut() = None);
    }
}
