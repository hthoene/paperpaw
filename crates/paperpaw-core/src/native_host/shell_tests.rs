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
}

const NORMAL: u8 = 0;
const FAILS: u8 = 1;
const TOKEN: u32 = 9;

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
    if MODE.with(Cell::get) == FAILS {
        return ERR_INVALID;
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
    if status != OK {
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

unsafe extern "C" fn event(_: *mut c_void, token: u32, input: PpBytes) -> i32 {
    if token != TOKEN || input.len != 3 {
        return ERR_INVALID;
    }
    EVENTS.with(|e| e.set(e.get() + 1));
    OK
}

unsafe extern "C" fn draw(_: *mut c_void, token: u32) -> i32 {
    if token == TOKEN { OK } else { ERR_INVALID }
}

fn fake() -> Hosted<Heap, Module> {
    Hosted::new(Heap, Module::new(), make, "fake", (0, 0)).unwrap()
}

fn finish<S: Services, M>(hosted: &Hosted<S, M>) {
    hosted.owner.as_ref().revoke();
    hosted.owner.as_ref().finish().unwrap();
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
        dispatch_registration(&batch, 1, &depth, Invocation::Draw),
        Ok(OK)
    );
    // A record that was never registered is the owner's error.
    assert_eq!(
        dispatch_registration(&batch, 7, &depth, Invocation::Draw),
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
    hosted.owner.as_ref().revoke();
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
    hosted.owner.as_ref().finish().unwrap();
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
    hosted.owner.as_ref().revoke();
    assert!(pending.commit().is_err());
    assert_eq!(logged(), ["destroy"], "exactly once, by the drop");
    drop(hosted);
    assert_eq!(logged(), ["destroy", "module"]);
}

#[test]
fn the_image_is_given_before_make_once_and_read_back_as_given() {
    reset(NORMAL);
    static IMAGE: [u8; 4] = [1, 2, 3, 4];
    let hosted = Hosted::new(
        Heap,
        Module::new(),
        make,
        "fake",
        (IMAGE.as_ptr() as usize, IMAGE.len()),
    )
    .unwrap();
    // SAFETY: a static, which outlives everything.
    assert_eq!(unsafe { hosted.image() }, &IMAGE);
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
    // SAFETY: an empty range reads nothing.
    assert!(unsafe { none.image() }.is_empty());
}
