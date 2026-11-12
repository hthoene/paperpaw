//! The shared shell owner against a plugin written in Rust behind the same
//! C table: registration, dispatch, the exactly-once destructor and its
//! order against the module, and the refusals.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate std;
use super::frame_tests::in_frame;
use super::*;
use alloc::vec;
use core::alloc::Layout;
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use std::vec::Vec;

std::thread_local! {
    static LOG: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    static MODE: Cell<u8> = const { Cell::new(0) };
    static MODULE_ALIVE: Cell<bool> = const { Cell::new(false) };
    static EVENTS: Cell<u32> = const { Cell::new(0) };
    static EXPECT_CORE: Cell<bool> = const { Cell::new(false) };
    static MAKES: Cell<u32> = const { Cell::new(0) };
    /// Calls announced to the shell, and how many are open now.
    static CALLS: Cell<u32> = const { Cell::new(0) };
    static OPEN: Cell<i32> = const { Cell::new(0) };
    /// The shell refuses every call (`Hooks::call_begin` answers `false`).
    static REFUSE: Cell<bool> = const { Cell::new(false) };
    /// Host slots the shell announced on this thread, and how many are open.
    static SLOTS: Cell<u32> = const { Cell::new(0) };
    static SLOTS_OPEN: Cell<i32> = const { Cell::new(0) };
    /// Destructor runs that were outside any announced call.
    static UNCOVERED: Cell<u32> = const { Cell::new(0) };
    /// The nth allocation of this thread fails (1-based; 0: none).
    pub(in crate::native_host) static FAIL_NTH: Cell<u32> = const { Cell::new(0) };
    /// Bytes this thread holds from the heap now, and the most it held.
    pub(in crate::native_host) static HEAP_LIVE: Cell<usize> = const { Cell::new(0) };
    pub(in crate::native_host) static HEAP_PEAK: Cell<usize> = const { Cell::new(0) };
    /// Fixture work (a card in memory) is not counted while this is set.
    pub(in crate::native_host) static HEAP_SHIELD: Cell<bool> = const { Cell::new(false) };
    /// An address to watch (0: none) and whether a block holding it was freed.
    static WATCH: Cell<usize> = const { Cell::new(0) };
    static WATCH_FREED: Cell<bool> = const { Cell::new(false) };
    /// `make` registers the one event only (the flat adapter routes no draw).
    static EVENT_ONLY: Cell<bool> = const { Cell::new(false) };
    /// `make` also registers for `event.tick`.
    static TICKS_TOO: Cell<bool> = const { Cell::new(false) };
    /// The draw callback draws with `text.draw_inked` (`shell_inked_tests.rs`).
    static DRAW_INKED: Cell<bool> = const { Cell::new(false) };
}

/// The test binary's allocator: the system's, failing on request so the
/// refusals of a fallible construction can be tried.
struct Failing;

// SAFETY: forwards to the system allocator; failing returns null, which the
// trait allows.
unsafe impl core::alloc::GlobalAlloc for Failing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let fail = FAIL_NTH.try_with(|n| {
            let left = n.get();
            if left == 0 {
                return false;
            }
            n.set(left - 1);
            left == 1
        });
        if fail == Ok(true) {
            return ptr::null_mut();
        }
        // SAFETY: the caller's layout, forwarded.
        let block = unsafe { std::alloc::System.alloc(layout) };
        if !block.is_null() && HEAP_SHIELD.try_with(Cell::get) == Ok(false) {
            let _ = HEAP_LIVE.try_with(|live| {
                live.set(live.get() + layout.size());
                let _ = HEAP_PEAK.try_with(|peak| peak.set(peak.get().max(live.get())));
            });
        }
        block
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _ = WATCH.try_with(|watch| {
            let at = watch.get();
            let start = ptr as usize;
            if at != 0 && at >= start && at < start + layout.size() {
                let _ = WATCH_FREED.try_with(|freed| freed.set(true));
            }
        });
        if HEAP_SHIELD.try_with(Cell::get) == Ok(false) {
            let _ = HEAP_LIVE.try_with(|live| live.set(live.get().saturating_sub(layout.size())));
        }
        // SAFETY: allocated by `alloc` above.
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Failing = Failing;

pub(in crate::native_host) const NORMAL: u8 = 0;
const FAILS: u8 = 1;
/// `make` returns OK with an output table that does not validate.
const BAD_TABLE: u8 = 2;
/// `make` submits garbage, ignores the refusal and returns OK.
const IGNORES_REFUSAL: u8 = 3;
const TOKEN: u32 = 9;

fn report(_: &Heap, owner: &str, _: super::super::TeardownStep, _: super::super::Error) {
    assert!(!owner.is_empty());
    log("report");
}

fn log(what: &'static str) {
    LOG.with(|log| log.borrow_mut().push(what));
}

fn logged() -> Vec<&'static str> {
    LOG.with(|log| log.borrow().clone())
}

pub(in crate::native_host) fn reset(mode: u8) {
    super::frame_tests::clear();
    EXPECT_CORE.with(|expected| expected.set(false));
    LOG.with(|log| log.borrow_mut().clear());
    MODE.with(|m| m.set(mode));
    EVENTS.with(|e| e.set(0));
    MAKES.with(|m| m.set(0));
    CALLS.with(|c| c.set(0));
    OPEN.with(|o| o.set(0));
    REFUSE.with(|r| r.set(false));
    UNCOVERED.with(|u| u.set(0));
    EVENT_ONLY.with(|e| e.set(false));
    TICKS_TOO.with(|e| e.set(false));
    DRAW_INKED.with(|d| d.set(false));
}

/// The shell's heap and clock.
pub(in crate::native_host) struct Heap;

impl Services for Heap {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        // SAFETY: the layout is valid and nonzero (checked by the caller).
        Layout::from_size_align(size, align)
            .map_or(ptr::null_mut(), |l| unsafe { std::alloc::alloc(l) })
    }
    unsafe fn dealloc(&self, ptr: *mut u8, size: usize, align: usize) {
        // SAFETY: the trait's contract: this allocator's block, freed once.
        unsafe { std::alloc::dealloc(ptr, Layout::from_size_align(size, align).unwrap()) };
    }
    unsafe fn realloc(&self, ptr: *mut u8, old: usize, align: usize, new: usize) -> *mut u8 {
        // SAFETY: as `dealloc`.
        unsafe { std::alloc::realloc(ptr, Layout::from_size_align(old, align).unwrap(), new) }
    }
    fn micros(&self) -> u64 {
        0
    }
}

/// What keeps the plugin alive: its destructor checks the module still is.
pub(in crate::native_host) struct Module;

impl Module {
    fn new() -> Self {
        MODULE_ALIVE.with(|a| a.set(true));
        Self
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        MODULE_ALIVE.with(|a| a.set(false));
        log("module");
    }
}

#[repr(C)]
struct Fake {
    plugin: PpPlugin,
    host: *const PpHost,
    ctx: *mut c_void,
}

unsafe extern "C" fn make(host: *const PpHost, ctx: *mut c_void, out: *mut *const PpPlugin) -> i32 {
    MAKES.with(|m| m.set(m.get() + 1));
    in_frame(1, ctx);
    let mode = MODE.with(Cell::get);
    if mode == FAILS {
        return ERR_INVALID;
    }
    if mode == BAD_TABLE {
        // A table that says it is 4 bytes long: a size word and nothing else.
        static SHORT: [u32; 1] = [4];
        // SAFETY: `out` is writable; the static is aligned for the table.
        unsafe { out.write(SHORT.as_ptr().cast()) };
        return OK;
    }
    // SAFETY: the host table is valid for this call.
    let table = unsafe { &*host };
    let mut block = ptr::null_mut();
    // SAFETY: the table's own heap, with this size and alignment.
    let status =
        unsafe { (table.heap_alloc)(ctx, size_of::<Fake>(), align_of::<Fake>(), &mut block) };
    if status != OK {
        return status;
    }
    if mode == IGNORES_REFUSAL {
        // SAFETY: readable bytes for this synchronous call.
        let _ = unsafe {
            (table.registrations_submit)(
                ctx,
                PpBytes {
                    ptr: [0xff_u8; 8].as_ptr(),
                    len: 8,
                },
            )
        };
    }
    // On the stack: the plugin's own allocations are not under test here.
    let mut bytes = [0u8; registrations::REGISTRATION_MAX_BYTES];
    let started = Registration::Event {
        event_tag: registrations::EVENT_STARTED_TAG,
        token: TOKEN,
    };
    let tick = Registration::Event {
        event_tag: registrations::EVENT_TICK_TAG,
        token: TOKEN,
    };
    let len = if TICKS_TOO.with(Cell::get) {
        registrations::registrations_encode(&[started, tick], &mut bytes)
    } else if EVENT_ONLY.with(Cell::get) {
        registrations::registrations_encode(&[started], &mut bytes)
    } else {
        registrations::registrations_encode(
            &[started, Registration::Draw { token: TOKEN }],
            &mut bytes,
        )
    }
    .unwrap();
    // SAFETY: readable bytes for this synchronous call.
    let status = unsafe {
        (table.registrations_submit)(
            ctx,
            PpBytes {
                ptr: bytes.as_ptr(),
                len,
            },
        )
    };
    // A plugin in the ignoring mode goes on with a valid table, whatever
    // the host said to its submits.
    if status != OK && mode != IGNORES_REFUSAL {
        // SAFETY: the block just allocated, same layout.
        unsafe { (table.heap_dealloc)(ctx, block, size_of::<Fake>(), align_of::<Fake>()) };
        return status;
    }
    let fake = block.cast::<Fake>();
    // SAFETY: a fresh block of the right size and alignment.
    unsafe {
        fake.write(Fake {
            plugin: PpPlugin {
                size: size_of::<PpPlugin>() as u32,
                state: block,
                drop: Some(destroy),
                dispatch_event: Some(event),
                draw: Some(draw),
                input: None,
            },
            host,
            ctx,
        });
        out.write(ptr::addr_of!((*fake).plugin));
    }
    OK
}

unsafe extern "C" fn destroy(state: *mut c_void) {
    assert!(
        MODULE_ALIVE.with(Cell::get),
        "the plugin is destroyed while its module is alive"
    );
    log("destroy");
    if OPEN.with(Cell::get) <= 0 {
        UNCOVERED.with(|u| u.set(u.get() + 1));
    }
    let fake = state.cast::<Fake>();
    // SAFETY: the block `make` wrote, destroyed once.
    let (host, ctx) = unsafe { ((*fake).host, (*fake).ctx) };
    in_frame(8, ctx);
    // SAFETY: the table and context outlive the destructor.
    unsafe {
        ((*host).heap_dealloc)(ctx, state, size_of::<Fake>(), align_of::<Fake>());
    }
}

unsafe extern "C" fn event(state: *mut c_void, token: u32, input: PpBytes, reply: PpBuffer) -> i32 {
    // Started and the other events 32-39 have no reply family (section 6).
    // SAFETY: the block `make` wrote, live while it is dispatched.
    in_frame(2, unsafe { (*state.cast::<Fake>()).ctx });
    if token != TOKEN || input.len != 3 || reply.ptr.is_null() || reply.cap != 0 {
        return ERR_INVALID;
    }
    if !has_expected_core_frame(state) {
        return host_glue::ERR_STATE;
    }
    EVENTS.with(|e| e.set(e.get() + 1));
    OK
}

unsafe extern "C" fn draw(state: *mut c_void, token: u32, frame: PpBytes) -> i32 {
    // SAFETY: as for `event`.
    in_frame(4, unsafe { (*state.cast::<Fake>()).ctx });
    if !has_expected_core_frame(state) {
        return host_glue::ERR_STATE;
    }
    if token != TOKEN || frame.len == 0 || frame.ptr.is_null() {
        return ERR_INVALID;
    }
    if DRAW_INKED.with(Cell::get) {
        return inked::draw(state);
    }
    // A positive result is a fault, never a length: the host maps it.
    if frame.len == 1 { 1 } else { OK }
}

pub(in crate::native_host) fn frame() -> Vec<u8> {
    let mut bytes = [0; 64];
    let frame = super::super::frames::screen(0, 0, 8, 8, (0, 8));
    let len = super::super::frames::encode(&frame, &mut bytes).unwrap();
    bytes[..len].to_vec()
}

pub(in crate::native_host) fn fake() -> Hosted<Heap, Module> {
    // SAFETY: `make` is this file's plugin, always callable; no image.
    unsafe {
        Hosted::new(
            Heap,
            Module::new(),
            make,
            "fake",
            (core::ptr::null(), 0),
            report,
            super::super::HostResources::try_new().unwrap(),
            DeclaredServices::none(),
        )
    }
    .unwrap()
}

pub(in crate::native_host) fn finish<S: Services, M>(hosted: &Hosted<S, M>) {
    hosted.owner().revoke();
    hosted.owner().finish().unwrap();
}

#[test]
fn a_made_plugin_registers_dispatches_and_is_destroyed_once_before_its_module() {
    reset(NORMAL);
    let hosted = fake();
    let batch = hosted
        .make_registered(hosted.begin().unwrap())
        .unwrap()
        .commit()
        .unwrap();
    assert_eq!(batch.len(), 2);
    // The records are read in place, without dispatching; none past the end.
    assert!(matches!(batch.record(0), Some(Registration::Event { .. })));
    assert!(matches!(batch.record(1), Some(Registration::Draw { .. })));
    assert!(batch.record(2).is_none() && batch.record(7).is_none());
    assert_eq!(EVENTS.with(Cell::get), 0, "reading runs no plugin code");
    let depth = DispatchDepth::new();
    assert_eq!(
        dispatch_registration(&batch, 0, &depth, Invocation::Event(&[1, 2, 3])),
        Ok(OK)
    );
    assert_eq!(EVENTS.with(Cell::get), 1);
    // The callback's own refusal is its status, not a host error.
    assert_eq!(
        dispatch_registration(&batch, 0, &depth, Invocation::Event(&[1])),
        Ok(ERR_INVALID)
    );
    assert_eq!(
        dispatch_registration(&batch, 1, &depth, Invocation::Draw(&frame())),
        Ok(OK)
    );
    // A positive draw result is a plugin fault, never a length.
    assert_eq!(
        dispatch_registration(&batch, 1, &depth, Invocation::Draw(&[0])),
        Ok(ERR_INVALID)
    );
    // A record that was never registered is the owner's error.
    assert_eq!(
        dispatch_registration(&batch, 7, &depth, Invocation::Draw(&frame())),
        Err(Error::Lifecycle(super::super::Error::State))
    );
    assert!(logged().is_empty(), "nothing destroyed while in use");
    drop(batch);
    finish(&hosted);
    assert_eq!(logged(), ["destroy"], "once, at finish");
    finish(&hosted);
    assert_eq!(logged(), ["destroy"], "finish twice is the same");
    drop(hosted);
    assert_eq!(logged(), ["destroy", "module"], "the module goes last");
}

#[test]
fn a_dispatch_after_revoke_is_the_owners_error_and_reaches_no_code() {
    reset(NORMAL);
    let hosted = fake();
    let batch = hosted
        .make_registered(hosted.begin().unwrap())
        .unwrap()
        .commit()
        .unwrap();
    hosted.owner().revoke();
    assert_eq!(
        dispatch_registration(
            &batch,
            0,
            &DispatchDepth::new(),
            Invocation::Event(&[1, 2, 3])
        ),
        Err(Error::Lifecycle(super::super::Error::State))
    );
    assert_eq!(EVENTS.with(Cell::get), 0);
    drop(batch);
    hosted.owner().finish().unwrap();
    drop(hosted);
    assert_eq!(logged(), ["destroy", "module"]);
}

#[test]
fn a_failing_make_leaves_nothing_to_destroy_and_make_is_tried_once() {
    reset(FAILS);
    let hosted = fake();
    let refused = hosted.make_registered(hosted.begin().unwrap()).err();
    assert_eq!(refused, Some(Error::Factory(ERR_INVALID)));
    let again = hosted.make_registered(hosted.begin().unwrap()).err();
    assert_eq!(again, Some(Error::Attempt));
    drop(hosted);
    assert_eq!(logged(), ["module"], "no destructor for output never made");
}

#[test]
fn a_made_plugin_dropped_before_commit_is_destroyed_once_while_its_module_lives() {
    reset(NORMAL);
    let hosted = fake();
    let pending = hosted.make_registered(hosted.begin().unwrap()).unwrap();
    drop(pending);
    assert_eq!(logged(), ["destroy"]);
    drop(hosted);
    assert_eq!(logged(), ["destroy", "module"]);
}

#[test]
fn a_refused_commit_destroys_the_output_once() {
    reset(NORMAL);
    let hosted = fake();
    let pending = hosted.make_registered(hosted.begin().unwrap()).unwrap();
    hosted.owner().revoke();
    assert!(pending.commit().is_err());
    assert_eq!(logged(), ["destroy"], "exactly once, by the drop");
    drop(hosted);
    assert_eq!(logged(), ["destroy", "module"]);
}

#[test]
fn the_image_is_given_before_make_once_and_read_back_as_given() {
    reset(NORMAL);
    static IMAGE: [u8; 4] = [1, 2, 3, 4];
    // SAFETY: `make` is this file's plugin; the image is a static.
    let hosted = unsafe {
        Hosted::new(
            Heap,
            Module::new(),
            make,
            "fake",
            (IMAGE.as_ptr(), IMAGE.len()),
            report,
            super::super::HostResources::try_new().unwrap(),
            DeclaredServices::none(),
        )
    }
    .unwrap();
    assert_eq!(hosted.image(), Some(&IMAGE[..]));
    let mut transaction = hosted.begin::<Registration>().unwrap();
    transaction.set_image(&IMAGE).unwrap();
    assert_eq!(
        transaction.set_image(&IMAGE),
        Err(super::super::Error::State),
        "a second call"
    );
    let batch = hosted
        .make_registered(transaction)
        .unwrap()
        .commit()
        .unwrap();
    drop(batch);
    finish(&hosted);
    drop(hosted);
    assert_eq!(logged(), ["destroy", "module"]);
    // No image: empty, not a pointer at zero.
    reset(NORMAL);
    let none = fake();
    assert_eq!(none.image(), Some(&[][..]));
    // Stopped: none at all, and the owner's error from then on.
    none.stop();
    assert_eq!(none.image(), None);
    none.stop();
    assert_eq!(none.image(), None, "stopping twice is the same");
}

#[test]
fn an_output_table_that_does_not_validate_is_refused_without_a_destructor() {
    reset(BAD_TABLE);
    let hosted = fake();
    let refused = hosted.make_registered(hosted.begin().unwrap()).err();
    assert_eq!(refused, Some(Error::Plugin));
    assert_eq!(MAKES.with(Cell::get), 1);
    let again = hosted.make_registered(hosted.begin().unwrap()).err();
    assert_eq!(again, Some(Error::Attempt), "make is tried once");
    assert_eq!(MAKES.with(Cell::get), 1, "and not called again");
    drop(hosted);
    assert_eq!(
        logged(),
        ["module"],
        "nothing to destroy, nothing dispatched"
    );
}

#[test]
fn a_plugin_that_ignores_a_refused_submit_cannot_activate() {
    reset(IGNORES_REFUSAL);
    let hosted = fake();
    // `make` succeeds with a valid table; its first submit was refused and
    // latched, so the construction is poisoned whatever `make` returned.
    let pending = hosted.make_registered(hosted.begin().unwrap()).unwrap();
    assert_eq!(MAKES.with(Cell::get), 1);
    let refused = pending.commit().err();
    assert!(refused.is_some(), "commit of a poisoned construction");
    // Destroyed exactly once, while the module lives; nothing was dispatched.
    assert_eq!(logged(), ["destroy"]);
    assert_eq!(EVENTS.with(Cell::get), 0);
    // And the one-step path refuses it the same way.
    drop(hosted);
    assert_eq!(logged(), ["destroy", "module"]);
    reset(IGNORES_REFUSAL);
    let hosted = fake();
    assert!(
        hosted
            .construct_registered(hosted.begin().unwrap())
            .is_err()
    );
    drop(hosted);
    assert_eq!(logged(), ["destroy", "module"]);
}

#[test]
fn a_hosted_plugin_dropped_after_commit_without_finish_is_destroyed_once_before_its_module() {
    reset(NORMAL);
    let hosted = fake();
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    drop(batch);
    assert!(logged().is_empty());
    drop(hosted);
    assert_eq!(logged(), ["destroy", "module"]);
}

#[test]
fn host_entries_outside_their_scope_are_refused() {
    reset(NORMAL);
    let hosted = fake();
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    let context = hosted.owner().context();
    let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast::<c_void>();
    // SAFETY: the pinned context of a live owner, valid byte buffers.
    unsafe {
        // A canvas call outside a draw dispatch.
        assert_ne!((context.table.canvas_fill_rect)(ctx, 0, 0, 1, 1, 0), OK);
        // An environment query outside a dispatch.
        let mut out = [0u8; 8];
        assert_ne!(
            (context.table.env_snapshot)(
                ctx,
                PpBuffer {
                    ptr: out.as_mut_ptr(),
                    cap: out.len(),
                }
            ),
            OK
        );
        // A registrations submit after `make` returned (as from a destructor).
        let mut bytes = vec![0; registrations::REGISTRATION_MAX_BYTES];
        let len =
            registrations::registrations_encode(&[Registration::Draw { token: TOKEN }], &mut bytes)
                .unwrap();
        assert_ne!(
            (context.table.registrations_submit)(
                ctx,
                PpBytes {
                    ptr: bytes.as_ptr(),
                    len,
                }
            ),
            OK
        );
    }
    drop(batch);
    drop(hosted);
}

pub(in crate::native_host) fn arm_failure(nth: u32) {
    FAIL_NTH.with(|n| n.set(nth));
}

#[test]
fn an_owner_that_cannot_be_placed_is_a_refusal_that_drops_the_module_and_never_calls_make() {
    // The 1st allocation is the room reserved for the owner's retirement and
    // the 2nd the owner's place in the heap accounting (the owner is not
    // admitted: a status), the 3rd the id, the 4th the owner.
    for nth in [1, 2, 3, 4] {
        reset(NORMAL);
        let module = Module::new();
        let resources = super::super::HostResources::try_new().unwrap();
        arm_failure(nth);
        // SAFETY: `make` is this file's plugin; no image.
        let refused = unsafe {
            Hosted::new(
                Heap,
                module,
                make,
                "fake",
                (core::ptr::null(), 0),
                report,
                resources,
                DeclaredServices::none(),
            )
        };
        arm_failure(0);
        let expected = if nth <= 2 {
            Error::Status(host_glue::ERR_NO_MEMORY)
        } else {
            Error::Allocation
        };
        assert_eq!(refused.err(), Some(expected), "allocation {nth}");
        assert_eq!(logged(), ["module"], "the module is dropped, once");
        assert_eq!(MAKES.with(Cell::get), 0, "make was never called");
    }
}

mod instance_tests {
    use super::*;
    use crate::native_host::flat::{DropReason, FlatInstance, FlatRegistration};
    use crate::native_host::shell::instance::{Hooks, Instance};

    fn dropped(_: &str, _: u32, _: DropReason) {
        log("dropped");
    }
    fn leaked(_: &str) {
        log("leaked");
    }
    fn teardown_failed(_: &str, _: super::super::super::Error) {
        log("teardown_failed");
    }
    fn begin(_: &str) -> bool {
        if REFUSE.with(Cell::get) {
            return false;
        }
        CALLS.with(|c| c.set(c.get() + 1));
        OPEN.with(|o| o.set(o.get() + 1));
        true
    }
    fn end(_: &str) {
        OPEN.with(|o| o.set(o.get() - 1));
    }
    fn hooks() -> Hooks {
        Hooks {
            event_dropped: dropped,
            leaked,
            teardown_failed,
            call_begin: begin,
            call_end: end,
        }
    }
    pub(super) fn load() -> Result<Instance<Heap, Module>, Error> {
        load_with(crate::native_host::HostResources::try_new().unwrap())
    }
    /// With the device's table made beforehand (a test that fails the nth
    /// allocation of the load makes it before arming the failure).
    fn load_with(
        resources: crate::native_host::HostResources,
    ) -> Result<Instance<Heap, Module>, Error> {
        // SAFETY: `make` is this file's plugin, always callable; no image.
        unsafe {
            Instance::load(
                Heap,
                Module::new(),
                make,
                "fake",
                (core::ptr::null(), 0),
                report,
                hooks(),
                resources,
                DeclaredServices::none(),
            )
        }
    }

    fn slot_begin() {
        SLOTS.with(|n| n.set(n.get() + 1));
        SLOTS_OPEN.with(|n| n.set(n.get() + 1));
    }

    fn slot_end() {
        SLOTS_OPEN.with(|n| n.set(n.get() - 1));
    }

    #[test]
    fn a_plugin_call_into_a_host_slot_is_announced_and_ended() {
        // The hooks are global but count per thread, so tests running beside
        // this one only count their own.
        crate::native_host::host_slot::set(slot_begin, slot_end);
        reset(NORMAL);
        SLOTS.with(|n| n.set(0));
        SLOTS_OPEN.with(|n| n.set(0));
        let instance = load().unwrap();
        // `make` submitted its registrations through the host's slot.
        assert!(SLOTS.with(Cell::get) >= 1, "the slot was announced");
        assert_eq!(SLOTS_OPEN.with(Cell::get), 0, "and ended");
        drop(instance);
        assert_eq!(SLOTS_OPEN.with(Cell::get), 0);
    }

    #[test]
    fn a_tick_goes_to_the_tick_registrations_only() {
        // Registered for `event.started` (and a draw) only: no tick record is
        // dispatched to it, whatever its other event registrations.
        reset(NORMAL);
        let instance = load().unwrap();
        assert!(instance.tick(60_000).is_empty());
        assert_eq!(CALLS.with(Cell::get), 1, "only `make` ran plugin code");
        drop(instance);
        // Registered for `event.tick` too: that one registration, once a tick.
        reset(NORMAL);
        TICKS_TOO.with(|value| value.set(true));
        let instance = load().unwrap();
        let before = CALLS.with(Cell::get);
        assert_eq!(instance.tick(60_000).len(), 1);
        assert_eq!(CALLS.with(Cell::get), before + 1);
    }

    #[test]
    fn an_instance_visits_dispatches_stops_and_releases_in_order() {
        reset(NORMAL);
        let instance = load().unwrap();
        assert_eq!(instance.id(), "fake");
        let mut seen = Vec::new();
        instance.registrations(&mut |r| seen.push(r));
        assert_eq!(
            seen,
            [
                FlatRegistration::Event {
                    event_tag: registrations::EVENT_STARTED_TAG,
                    index: 0
                },
                FlatRegistration::Other { record_tag: 2 },
            ]
        );
        let depth = DispatchDepth::new();
        assert_eq!(
            instance.dispatch_event(0, &[1, 2, 3], &mut [], &depth),
            Ok(OK)
        );
        assert_eq!(
            instance.dispatch_event(0, &[1], &mut [], &depth),
            Ok(ERR_INVALID),
            "the callback's own refusal is its status"
        );
        assert_eq!(
            instance.dispatch_event(9, &[1, 2, 3], &mut [], &depth),
            Err(super::super::super::Error::State)
        );
        assert_eq!(
            instance.dispatch_event(0, &[1, 2, 3], &mut [0; 4], &depth),
            Err(super::super::super::Error::Invalid),
            "an event without a reply family takes no reply buffer"
        );
        assert_eq!(instance.declared_records(), 0);
        assert!(instance.image().is_some());
        // The same scope every call.
        let first = ptr::from_ref(instance.environment().get_ref());
        assert_eq!(first, ptr::from_ref(instance.environment().get_ref()));
        instance.event_dropped(registrations::EVENT_STARTED_TAG, DropReason::Busy);
        assert_eq!(logged(), ["dropped"]);
        // Stopped: the owner's error before anything of the plugin runs, no image.
        let before = EVENTS.with(Cell::get);
        instance.stop();
        assert_eq!(
            instance.dispatch_event(0, &[1, 2, 3], &mut [], &depth),
            Err(super::super::super::Error::State)
        );
        assert_eq!(EVENTS.with(Cell::get), before);
        assert_eq!(instance.image(), None);
        assert_eq!(logged(), ["dropped"], "nothing destroyed while it is held");
        drop(instance);
        assert_eq!(logged(), ["dropped", "destroy", "module"]);
        // make (1), the dispatches that reached the plugin (3: two before the
        // stop, none after, the unknown index fails before any plugin code
        // but is announced), the drop (1): every announced call was ended.
        assert_eq!(OPEN.with(Cell::get), 0, "every call ended");
        assert!(CALLS.with(Cell::get) >= 5);
    }

    #[test]
    fn dropping_the_flat_plugin_unloads_in_order_with_the_destructor_inside_a_call() {
        use crate::native_host::FlatHost;
        use crate::native_host::flat::FlatPlugin;
        reset(NORMAL);
        EVENT_ONLY.with(|e| e.set(true));
        let instance = load().unwrap();
        let host = FlatHost {
            log: Default::default(),
            meter: crate::meter::Meter::new(None),
            directory: None,
            stack_rows: alloc::rc::Rc::new(Default::default()),
            font_removals: alloc::rc::Rc::new(
                crate::native_host::font_removals::FontRemovals::new(),
            ),
            registry_generation: alloc::rc::Rc::new(
                crate::native_host::registry_generation::RegistryGeneration::new(),
            ),
        };
        let manifest = paperpaw_sdk::Manifest::new(
            paperpaw_sdk::Id::parse("fake.plugin").unwrap(),
            "fake",
            paperpaw_sdk::Version::new(0, 0, 0),
        );
        let plugin = FlatPlugin::new(
            manifest,
            alloc::boxed::Box::new(instance),
            crate::native_host::flat::FlatDepth::try_new().unwrap(),
            &host,
        )
        .unwrap();
        assert!(
            logged().is_empty(),
            "nothing is destroyed while the plugin is held"
        );
        // The kernel removes the plugin's subscriptions and queued work (they
        // held the instance); the last reference going is the unload: stop,
        // the batch's lease, cancel and release, finish (the destructor), then
        // the module, each exactly once and no step failing.
        drop(plugin);
        assert_eq!(logged(), ["destroy", "module"]);
        assert_eq!(
            UNCOVERED.with(Cell::get),
            0,
            "the destructor ran inside a call"
        );
        assert_eq!(OPEN.with(Cell::get), 0, "every call ended");
    }

    #[test]
    fn a_failing_cancel_or_release_is_told_once_and_the_unload_still_finishes() {
        use crate::native_host::flat::FlatPlugin;
        use crate::native_host::{Error as HostError, FlatHost, TeardownStep};
        // A Busy step cannot be deferred from a drop: it is told, `finish`
        // retries the same step (the fault is one-shot) and the destructor
        // then runs; a step that failed for good is told through the owner's
        // recorded error. Either way: told exactly once, destroy once.
        for (step, error) in [
            (TeardownStep::Cancel, HostError::Invalid),
            (TeardownStep::Release, HostError::Invalid),
            (TeardownStep::Release, HostError::Busy),
        ] {
            reset(NORMAL);
            EVENT_ONLY.with(|e| e.set(true));
            let host = FlatHost {
                log: Default::default(),
                meter: crate::meter::Meter::new(None),
                directory: None,
                stack_rows: alloc::rc::Rc::new(Default::default()),
                font_removals: alloc::rc::Rc::new(
                    crate::native_host::font_removals::FontRemovals::new(),
                ),
                registry_generation: alloc::rc::Rc::new(
                    crate::native_host::registry_generation::RegistryGeneration::new(),
                ),
            };
            let manifest = paperpaw_sdk::Manifest::new(
                paperpaw_sdk::Id::parse("fake.plugin").unwrap(),
                "fake",
                paperpaw_sdk::Version::new(0, 0, 0),
            );
            let instance = load().unwrap();
            // Control for the kept-owner test: here the owner is freed.
            WATCH_FREED.with(|f| f.set(false));
            WATCH.with(|w| w.set(core::ptr::from_ref(&*instance.environment()) as usize));
            let plugin = FlatPlugin::new(
                manifest,
                alloc::boxed::Box::new(instance),
                crate::native_host::flat::FlatDepth::try_new().unwrap(),
                &host,
            )
            .unwrap();
            teardown_fault::arm(step, error);
            drop(plugin);
            assert!(
                WATCH_FREED.with(Cell::get),
                "{step:?} {error:?}: owner freed"
            );
            WATCH.with(|w| w.set(0));
            assert_eq!(
                logged(),
                ["teardown_failed", "destroy", "module"],
                "{step:?} {error:?}"
            );
            assert_eq!(OPEN.with(Cell::get), 0, "every call ended");
        }
    }

    /// Points at the hosted plugin that `a_release_that_stays_busy...` keeps for good.
    pub(super) static KEPT_OWNER: core::sync::atomic::AtomicPtr<u8> =
        core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

    #[test]
    fn a_release_that_stays_busy_keeps_the_owner_and_reports_leaked_without_destroying() {
        use crate::native_host::{Error as HostError, TeardownStep};
        reset(NORMAL);
        EVENT_ONLY.with(|e| e.set(true));
        let host = flat_host();
        let mut depth = None;
        let instance = load().unwrap();
        // An address inside the owner's allocation (its environment scope).
        let watched_ptr = core::ptr::from_ref(&*instance.environment()).cast::<u8>();
        let watched = watched_ptr.addr();
        WATCH_FREED.with(|f| f.set(false));
        WATCH.with(|w| w.set(watched));
        // The kept hosted plugin (with its owner) is never freed, by design: a
        // static that points at it keeps it reachable, so Miri's leak check
        // (a leak is a bug elsewhere) does not report this one.
        KEPT_OWNER.store(
            instance.hosted_address(),
            core::sync::atomic::Ordering::Relaxed,
        );
        let plugin = instance.into_plugin(manifest(), &mut depth, &host).unwrap();
        teardown_fault::arm_persistent(TeardownStep::Release, HostError::Busy);
        drop(plugin);
        teardown_fault::disarm();
        assert!(
            !WATCH_FREED.with(Cell::get),
            "the kept owner's memory was not freed: it stays readable"
        );
        // SAFETY: the owner was kept (never freed), so the scope inside it is
        // still allocated; reading one byte of it only checks that.
        let _ = unsafe { core::ptr::read_volatile(watched_ptr) };
        WATCH.with(|w| w.set(0));
        // Told once (the first attempt), refused again by `finish`: the owner
        // is kept, so the destructor never ran and the module was not freed.
        assert_eq!(logged(), ["teardown_failed", "leaked"]);
        assert!(
            MODULE_ALIVE.with(Cell::get),
            "the module outlives the kept owner"
        );
        assert_eq!(OPEN.with(Cell::get), 0, "every call ended");
    }

    fn flat_host() -> crate::native_host::FlatHost {
        crate::native_host::FlatHost {
            log: Default::default(),
            meter: crate::meter::Meter::new(None),
            directory: None,
            stack_rows: alloc::rc::Rc::new(Default::default()),
            font_removals: alloc::rc::Rc::new(
                crate::native_host::font_removals::FontRemovals::new(),
            ),
            registry_generation: alloc::rc::Rc::new(
                crate::native_host::registry_generation::RegistryGeneration::new(),
            ),
        }
    }

    fn manifest() -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            paperpaw_sdk::Id::parse("fake.plugin").unwrap(),
            "fake",
            paperpaw_sdk::Version::new(0, 0, 0),
        )
    }

    /// An allocation that fails at any step after `make` (the instance box,
    /// the device's depth, the adapter's node and box) is a refusal that
    /// unloads the plugin exactly once, inside an announced call; the first
    /// plugin and a later one (sharing the depth) alike.
    #[test]
    fn an_allocation_failing_after_make_unloads_the_plugin_once_for_the_first_and_a_later_plugin() {
        use crate::native_host::flat::FlatError;
        let host = flat_host();
        for later in [false, true] {
            // The first plugin allocates the instance box, the device depth,
            // the adapter's shared node and the adapter's box (4); a later
            // one shares the depth (3). Every one of them can fail; past them
            // the plugin is made.
            let allocations = if later { 3 } else { 4 };
            for nth in 1..=allocations + 2 {
                reset(NORMAL);
                EVENT_ONLY.with(|e| e.set(true));
                let mut depth = None;
                let first = later.then(|| {
                    let plugin = load()
                        .unwrap()
                        .into_plugin(manifest(), &mut depth, &host)
                        .unwrap();
                    LOG.with(|l| l.borrow_mut().clear());
                    plugin
                });
                let held = first.as_ref().map(|p| p.reference_counts());
                // The next plugin's module is alive from here on.
                let instance = load().unwrap();
                let manifest = manifest();
                arm_failure(nth);
                let result = instance.into_plugin(manifest, &mut depth, &host);
                arm_failure(0);
                let context = alloc::format!("later {later}, allocation {nth}");
                assert_eq!(UNCOVERED.with(Cell::get), 0, "{context}");
                assert_eq!(OPEN.with(Cell::get), 0, "{context}");
                match result {
                    Err(why) => {
                        assert!(nth <= allocations, "{context}: refused past the last");
                        assert_eq!(why, FlatError::Allocation, "{context}");
                        assert_eq!(logged(), ["destroy", "module"], "{context}: once");
                        if let (Some(first), Some(held)) = (first.as_ref(), held) {
                            assert_eq!(first.reference_counts(), held, "{context}: shares");
                        }
                    }
                    Ok(plugin) => {
                        assert!(nth > allocations, "{context}: made despite a failure");
                        drop(plugin);
                        assert_eq!(logged(), ["destroy", "module"], "{context}");
                    }
                }
                // The other module died first; the first plugin's destructor
                // checks its own is alive.
                MODULE_ALIVE.with(|a| a.set(true));
                drop(first);
                assert_eq!(UNCOVERED.with(Cell::get), 0, "{context}: first");
            }
        }
    }

    #[test]
    fn every_announced_call_is_ended_also_when_make_fails_or_a_load_is_refused() {
        for mode in [FAILS, IGNORES_REFUSAL, BAD_TABLE] {
            reset(mode);
            assert!(load().is_err());
            assert_eq!(OPEN.with(Cell::get), 0, "mode {mode}");
            assert!(CALLS.with(Cell::get) >= 1, "make was announced");
        }
    }

    #[test]
    fn a_failing_make_is_a_refusal_with_the_module_dropped_once() {
        reset(FAILS);
        assert_eq!(load().err(), Some(Error::Factory(ERR_INVALID)));
        assert_eq!(logged(), ["module"]);
    }

    #[test]
    fn a_plugin_that_ignores_a_refused_submit_is_not_loaded() {
        reset(IGNORES_REFUSAL);
        assert!(load().is_err());
        assert_eq!(logged(), ["destroy", "module"]);
        assert_eq!(EVENTS.with(Cell::get), 0);
    }

    #[test]
    fn an_allocation_that_fails_anywhere_in_a_load_leaves_nothing_behind() {
        let mut loaded = 0;
        let mut destroyed_in_a_refusal = 0;
        for nth in 1..=12 {
            reset(NORMAL);
            let resources = crate::native_host::HostResources::try_new().unwrap();
            arm_failure(nth);
            let result = load_with(resources);
            arm_failure(0);
            // Every destructor run of the load, a refused one included, was
            // inside an announced call (attributed, stack checked after it).
            assert_eq!(UNCOVERED.with(Cell::get), 0, "allocation {nth}");
            assert_eq!(OPEN.with(Cell::get), 0, "allocation {nth}");
            match result {
                Ok(instance) => {
                    loaded += 1;
                    drop(instance);
                    assert_eq!(UNCOVERED.with(Cell::get), 0, "allocation {nth}: drop");
                    assert_eq!(logged(), ["destroy", "module"], "allocation {nth}");
                }
                Err(_) => {
                    let log = logged();
                    assert_eq!(log.last(), Some(&"module"), "allocation {nth}: {log:?}");
                    assert!(
                        log.iter().filter(|e| **e == "destroy").count() <= 1,
                        "allocation {nth}: {log:?}"
                    );
                    destroyed_in_a_refusal += log.contains(&"destroy") as u32;
                }
            }
        }
        assert!(
            destroyed_in_a_refusal > 0,
            "a refusal after make destroyed the plugin inside the announced call"
        );
        assert!(loaded > 0, "past the last allocation it loads");
    }

    // The file is `native_host/instance_tests/host_slot_table.rs`.
    mod host_slot_table;
}

#[test]
fn an_empty_visible_region_sends_no_frame_and_a_full_one_its_rows() {
    let mut out = [0; 64];
    for empty in [Rect::new(0, 0, 8, 0), Rect::new(0, 4, 0, 8)] {
        assert_eq!(screen_frame(empty, &mut out), Ok(None), "{empty:?}");
    }
    let len = screen_frame(Rect::new(2, 4, 8, 6), &mut out)
        .unwrap()
        .unwrap();
    let batch = registrations::records_decode(&out[..len]).unwrap();
    let Some(registrations::Record::DrawFrame(frame)) = batch.iter().next() else {
        panic!("not a frame");
    };
    assert_eq!((frame.y, frame.clip_top, frame.clip_bottom), (4, 4, 10));
}

#[test]
fn shared_shell_reports_fallback_failure_before_destroying_and_unmapping() {
    reset(NORMAL);
    let hosted = fake();
    {
        let mut pending = hosted
            .make_registered(hosted.begin::<Registration>().unwrap())
            .unwrap();
        let transaction = pending.transaction.take().unwrap();
        let batch = transaction
            .staging
            .commit(crate::native_host::Teardown {
                report: report_context::<Heap>,
                cancel: |_| Err(super::super::Error::Busy),
                release: |_| Ok(()),
                destroy: destroy_context::<Heap>,
            })
            .unwrap();
        pending.armed = false;
        drop(pending);
        drop(batch);
    }
    drop(hosted);
    assert_eq!(logged(), ["report", "destroy", "module"]);
}

#[test]
fn pinned_owner_allocation_failure_drops_unactivated_context_then_module() {
    struct Probe(&'static str);
    impl Drop for Probe {
        fn drop(&mut self) {
            log(self.0);
        }
    }
    reset(NORMAL);
    let value = super::super::HostOwner::new(Probe("context"), Probe("module"));
    arm_failure(1);
    let refused = super::super::PinnedOwner::try_new(value);
    arm_failure(0);
    assert!(refused.is_none());
    assert_eq!(logged(), ["context", "module"]);
}

#[path = "shell_oom_tests.rs"]
mod oom;

#[test]
fn teardown_faults_are_one_shot_and_propagate_through_production_commit() {
    use crate::native_host::{Error as HostError, TeardownStep};
    for (step, error) in [
        (TeardownStep::Cancel, HostError::Invalid),
        (TeardownStep::Release, HostError::Invalid),
        (TeardownStep::Release, HostError::Busy),
    ] {
        reset(NORMAL);
        let hosted = fake();
        let batch = hosted
            .make_registered(hosted.begin::<Registration>().unwrap())
            .unwrap()
            .commit()
            .unwrap();
        teardown_fault::arm(step, error);
        let prepared = hosted.owner().prepare_teardown();
        if error == HostError::Busy {
            assert_eq!(prepared, Err(HostError::Busy));
            assert_eq!(hosted.owner().take_teardown_error(), None);
            assert!(logged().is_empty());
            hosted.owner().prepare_teardown().unwrap();
        } else {
            assert_eq!(prepared, Ok(()));
            assert_eq!(hosted.owner().take_teardown_error(), Some(error));
            assert_eq!(hosted.owner().take_teardown_error(), None);
        }
        drop(batch);
        hosted.owner().finish().unwrap();
        drop(hosted);
        assert_eq!(logged(), ["destroy", "module"]);
    }
}

#[test]
fn persistent_teardown_busy_keeps_owner_until_disarmed() {
    use crate::native_host::{Error as HostError, TeardownStep};
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            teardown_fault::disarm();
        }
    }
    reset(NORMAL);
    let hosted = fake();
    let batch = hosted
        .make_registered(hosted.begin::<Registration>().unwrap())
        .unwrap()
        .commit()
        .unwrap();
    let _reset = Reset;
    teardown_fault::arm_persistent(TeardownStep::Release, HostError::Busy);
    assert_eq!(hosted.owner().prepare_teardown(), Err(HostError::Busy));
    drop(batch);
    assert_eq!(hosted.owner().finish(), Err(HostError::Busy));
    assert!(logged().is_empty());
    assert!(MODULE_ALIVE.with(Cell::get));
    teardown_fault::disarm();
    hosted.owner().finish().unwrap();
    drop(hosted);
    assert_eq!(logged(), ["destroy", "module"]);
}

#[test]
fn callback_admission_busy_preserves_read_output_before_raw_slot_entry() {
    reset(NORMAL);
    let hosted = fake();
    let batch = hosted
        .make_registered(hosted.begin().unwrap())
        .unwrap()
        .commit()
        .unwrap();
    let depth = DispatchDepth::new();
    let mut out = [0xa5; 7];
    let mut entered = false;
    batch
        .dispatch(0, &depth, |_, _| {
            // Reentry is refused by RegistrationBatch admission. The raw read
            // helper itself does not produce Busy and must not be reached here.
            let result = batch.dispatch(0, &depth, |context, _| {
                entered = true;
                // SAFETY: live path and exclusive output; no borrowed data escapes.
                unsafe {
                    super::super::storage_slots::read_at(
                        &context.environment,
                        "fake",
                        None,
                        0,
                        PpBytes {
                            ptr: b"state".as_ptr(),
                            len: 5,
                        },
                        0,
                        PpBuffer {
                            ptr: out.as_mut_ptr(),
                            cap: out.len(),
                        },
                    )
                }
            });
            assert_eq!(result, Err(super::super::Error::Busy));
        })
        .unwrap();
    assert!(!entered);
    assert_eq!(out, [0xa5; 7]);
    drop(batch);
    finish(&hosted);
}

fn has_expected_core_frame(state: *mut c_void) -> bool {
    if !EXPECT_CORE.with(Cell::get) {
        return true;
    }
    // SAFETY: the admitted callback retains this fixture and its host context.
    let context = unsafe { &*(*state.cast::<Fake>()).ctx.cast::<Context<Heap>>() };
    context
        .environment
        .with_frame(|frame| frame.device_usage().is_ok())
        .unwrap_or(false)
}

#[test]
fn kernel_dispatch_supplies_real_core_views_and_clears_them_after_callbacks() {
    reset(NORMAL);
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let hosted = fake();
    let batch = hosted
        .make_registered(hosted.begin().unwrap())
        .unwrap()
        .commit()
        .unwrap();
    let depth = DispatchDepth::new();
    let now = paperpaw_platform::Instant(12);
    EXPECT_CORE.with(|expected| expected.set(true));
    let env = kernel.shared.env(now);
    assert_eq!(
        dispatch_with_env(&batch, 0, &depth, Invocation::Event(&[1, 2, 3]), &env),
        Ok(host_glue::ERR_STATE)
    );
    assert_eq!(
        kernel.dispatch_native_for_test(&batch, 0, &depth, Invocation::Event(&[1, 2, 3]), now),
        Ok(OK)
    );
    let mut framebuffer =
        paperpaw_graphics::Framebuffer::new(20, 80, paperpaw_platform::PixelFormat::Gray8).unwrap();
    assert_eq!(
        kernel.draw_native_for_test(&batch, 1, &depth, now, &mut framebuffer.canvas()),
        Ok(OK)
    );
    assert_eq!(
        hosted.owner().context().environment.with_frame(|_| ()),
        Err(host_glue::ERR_STATE)
    );
    drop(batch);
    finish(&hosted);
}

#[path = "shell_inked_tests.rs"]
mod inked;

#[path = "shell_service_oom_tests.rs"]
mod service_oom;

#[test]
fn stage_slot_missing_frame_and_zero_capacity_do_not_allocate() {
    reset(NORMAL);
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let hosted = fake();
    let context = hosted.owner().context();
    let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast();
    let stage = PpBytes {
        ptr: b"fake.insert".as_ptr(),
        len: b"fake.insert".len(),
    };
    let input = PpBytes {
        ptr: ptr::null(),
        len: 0,
    };
    let mut out = [0xa5; 8];
    // SAFETY: pinned fixture context and disjoint byte buffers are live.
    assert_eq!(
        unsafe {
            super::stage_slot::run::<Heap>(
                ctx,
                stage,
                0,
                input,
                PpBuffer {
                    ptr: out.as_mut_ptr(),
                    cap: out.len(),
                },
            )
        },
        host_glue::ERR_STATE
    );
    assert_eq!(out, [0xa5; 8]);
    // SAFETY: EnvironmentScope is structurally pinned inside this retained context.
    let scope = unsafe { context.map_unchecked(|value| &value.environment) };
    scope
        .scope(&env, || {
            arm_failure(1);
            // SAFETY: the same pinned fixture and buffer pairs remain live.
            let refused = unsafe {
                super::stage_slot::run::<Heap>(
                    ctx,
                    stage,
                    0,
                    input,
                    PpBuffer {
                        ptr: out.as_mut_ptr(),
                        cap: out.len(),
                    },
                )
            };
            let mut probe = Vec::<u8>::new();
            let still_armed = probe.try_reserve_exact(1).is_err();
            arm_failure(0);
            assert_eq!(refused, host_glue::ERR_UNSUPPORTED);
            assert!(
                still_armed,
                "slot must not allocate even with nonempty output"
            );
            assert_eq!(out, [0xa5; 8]);
            // A query frame has no broker and returns UNSUPPORTED.
            // SAFETY: the same pinned fixture and buffer pairs remain live.
            assert_eq!(
                unsafe {
                    super::stage_slot::run::<Heap>(
                        ctx,
                        stage,
                        0,
                        input,
                        PpBuffer {
                            ptr: out.as_mut_ptr(),
                            cap: out.len(),
                        },
                    )
                },
                host_glue::ERR_UNSUPPORTED
            );
            arm_failure(1);
            // SAFETY: a null zero-capacity output is permitted and never dereferenced.
            let empty = unsafe {
                super::stage_slot::run::<Heap>(
                    ctx,
                    stage,
                    0,
                    input,
                    PpBuffer {
                        ptr: ptr::null_mut(),
                        cap: 0,
                    },
                )
            };
            let mut probe = Vec::<u8>::new();
            let still_armed = probe.try_reserve_exact(1).is_err();
            arm_failure(0);
            assert_eq!(empty, host_glue::ERR_UNSUPPORTED);
            assert!(still_armed, "zero-capacity slot must not allocate");
        })
        .unwrap();
}

#[path = "fmt_call_oom_tests.rs"]
mod fmt_call_oom;
#[path = "format_call_oom_tests.rs"]
mod format_call_oom;
#[path = "format_scope_oom_tests.rs"]
mod format_scope_oom;
#[path = "lib_reads_oom_tests.rs"]
mod lib_reads_oom;
#[path = "stage_broker_oom_tests.rs"]
mod stage_broker_oom;
#[path = "stage_run_tests.rs"]
mod stage_run;

#[path = "shell_evt_oom_tests.rs"]
mod evt_oom;

#[path = "shell_command_dispatch_tests.rs"]
mod command_dispatch;
#[path = "stage_broker_native_tests.rs"]
mod stage_broker_native;

#[path = "memory_notice_tests.rs"]
mod memory_notice;
#[path = "native_device_defaults_tests.rs"]
mod native_device_defaults;
#[path = "native_late_config_tests.rs"]
mod native_late_config;
#[path = "native_screen_install_tests.rs"]
mod native_screen_install;

#[path = "shell_describe_tests.rs"]
mod describe;
#[path = "shell_refuse_tests.rs"]
mod refuse;
