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
    /// Calls announced to the shell, and how many are open now.
    static CALLS: Cell<u32> = const { Cell::new(0) };
    static OPEN: Cell<i32> = const { Cell::new(0) };
    /// Destructor runs that were outside any announced call.
    static UNCOVERED: Cell<u32> = const { Cell::new(0) };
    /// The nth allocation of this thread fails (1-based; 0: none).
    static FAIL_NTH: Cell<u32> = const { Cell::new(0) };
    /// `make` registers the one event only (the flat adapter routes no draw).
    static EVENT_ONLY: Cell<bool> = const { Cell::new(false) };
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
    CALLS.with(|c| c.set(0));
    OPEN.with(|o| o.set(0));
    UNCOVERED.with(|u| u.set(0));
    EVENT_ONLY.with(|e| e.set(false));
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
    // On the stack: the plugin's own allocations are not under test here.
    let mut bytes = [0u8; registrations::REGISTRATION_MAX_BYTES];
    let started = Registration::Event {
        event_tag: registrations::EVENT_STARTED_TAG,
        token: TOKEN,
    };
    let len = if EVENT_ONLY.with(Cell::get) {
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
    unsafe { Hosted::new(Heap, Module::new(), make, "fake", (0, 0), report) }.unwrap()
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
        arm_failure(nth);
        // SAFETY: `make` is this file's plugin; no image.
        let refused = unsafe { Hosted::new(Heap, module, make, "fake", (0, 0), report) };
        arm_failure(0);
        assert_eq!(refused.err(), Some(Error::Allocation), "allocation {nth}");
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
    fn begin(_: &str) {
        CALLS.with(|c| c.set(c.get() + 1));
        OPEN.with(|o| o.set(o.get() + 1));
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
    fn load() -> Result<Instance<Heap, Module>, Error> {
        // SAFETY: `make` is this file's plugin, always callable; no image.
        unsafe { Instance::load(Heap, Module::new(), make, "fake", (0, 0), report, hooks()) }
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
            meter: crate::meter::Meter::new(None),
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
            arm_failure(nth);
            let result = load();
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
