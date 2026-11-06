//! The shared shell owner against a plugin written in Rust behind the same
//! C table: registration, dispatch, the exactly-once destructor and its
//! order against the module, and the refusals.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate std;
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
    static MAKES: Cell<u32> = const { Cell::new(0) };
    /// The nth allocation of this thread fails (1-based; 0: none).
    static FAIL_NTH: Cell<u32> = const { Cell::new(0) };
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
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: allocated by `alloc` above.
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Failing = Failing;

const NORMAL: u8 = 0;
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

fn reset(mode: u8) {
    LOG.with(|log| log.borrow_mut().clear());
    MODE.with(|m| m.set(mode));
    EVENTS.with(|e| e.set(0));
    MAKES.with(|m| m.set(0));
}

/// The shell's heap and clock.
struct Heap;

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
struct Module;

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
    let mut bytes = vec![0; registrations::REGISTRATION_MAX_BYTES];
    let len = registrations::registrations_encode(
        &[
            Registration::Event {
                event_tag: registrations::EVENT_STARTED_TAG,
                token: TOKEN,
            },
            Registration::Draw { token: TOKEN },
        ],
        &mut bytes,
    )
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
    let fake = state.cast::<Fake>();
    // SAFETY: the block `make` wrote, destroyed once.
    let (host, ctx) = unsafe { ((*fake).host, (*fake).ctx) };
    // SAFETY: the table and context outlive the destructor.
    unsafe {
        ((*host).heap_dealloc)(ctx, state, size_of::<Fake>(), align_of::<Fake>());
    }
}

unsafe extern "C" fn event(_: *mut c_void, token: u32, input: PpBytes, reply: PpBuffer) -> i32 {
    // Started and the other events 32-39 have no reply family (section 6).
    if token != TOKEN || input.len != 3 || reply.ptr.is_null() || reply.cap != 0 {
        return ERR_INVALID;
    }
    EVENTS.with(|e| e.set(e.get() + 1));
    OK
}

unsafe extern "C" fn draw(_: *mut c_void, token: u32, frame: PpBytes) -> i32 {
    if token != TOKEN || frame.len == 0 || frame.ptr.is_null() {
        return ERR_INVALID;
    }
    // A positive result is a fault, never a length: the host maps it.
    if frame.len == 1 { 1 } else { OK }
}

fn frame() -> Vec<u8> {
    let mut bytes = [0; 64];
    let frame = super::super::frames::screen(0, 0, 8, 8, (0, 8));
    let len = super::super::frames::encode(&frame, &mut bytes).unwrap();
    bytes[..len].to_vec()
}

fn fake() -> Hosted<Heap, Module> {
    // SAFETY: `make` is this file's plugin, always callable; no image.
    unsafe {
        Hosted::new(
            Heap,
            Module::new(),
            make,
            "fake",
            (0, 0),
            report,
            super::super::HostResources::try_new().unwrap(),
            DeclaredServices::none(),
        )
    }
    .unwrap()
}

fn finish<S: Services, M>(hosted: &Hosted<S, M>) {
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
            (IMAGE.as_ptr() as usize, IMAGE.len()),
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

fn arm_failure(nth: u32) {
    FAIL_NTH.with(|n| n.set(nth));
}

#[test]
fn an_owner_that_cannot_be_placed_is_a_refusal_that_drops_the_module_and_never_calls_make() {
    // The 1st allocation is the id, the 2nd the owner.
    for nth in [1, 2] {
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
                (0, 0),
                report,
                resources,
                DeclaredServices::none(),
            )
        };
        arm_failure(0);
        assert_eq!(refused.err(), Some(Error::Allocation), "allocation {nth}");
        assert_eq!(logged(), ["module"], "the module is dropped, once");
        assert_eq!(MAKES.with(Cell::get), 0, "make was never called");
    }
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
