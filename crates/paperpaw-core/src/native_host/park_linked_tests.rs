//! Linked plugins parked at their admission (#248, #2638): which are parked,
//! what is published for them, and how a first use makes them again.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]

use super::*;
use crate::native_host::{FixedTables, HostResources, registrations as wire};
use core::{cell::Cell, ffi::c_void, ptr};
use paperpaw_native::host_glue::{self, ERR_CANCELLED, PpBuffer, PpBytes, PpHost, PpPlugin};
use paperpaw_native::metadata::{Host, Profile};
use std::{alloc::Layout, sync::OnceLock};

extern crate std;

// As in `linked_install_tests`: target linux-x86_64-sysv64-le, floor 1,
// nothing used or required.
static METADATA: &[u8] = b"PPAM\x02\x00 \x00N\x00\x00\x00\x07\x00\x01\x00\x01\x00 \x00\x00\x00\x00\x00\x16\x00\x00\x00\x00\x00\x01\x00linux-x86_64-sysv64-le\x18\x00\x01\x00 \x00\x00\x00\x0e\x00PAPERPAW_ENTRY";

struct Heap;
impl Services for Heap {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        // SAFETY: the caller passes a valid nonzero layout.
        Layout::from_size_align(size, align).map_or(ptr::null_mut(), |layout| unsafe {
            std::alloc::alloc(layout)
        })
    }
    unsafe fn dealloc(&self, block: *mut u8, size: usize, align: usize) {
        // SAFETY: this allocator's block, freed once.
        unsafe { std::alloc::dealloc(block, Layout::from_size_align(size, align).unwrap()) };
    }
    unsafe fn realloc(&self, block: *mut u8, old: usize, align: usize, new: usize) -> *mut u8 {
        // SAFETY: as `dealloc`.
        unsafe { std::alloc::realloc(block, Layout::from_size_align(old, align).unwrap(), new) }
    }
    fn micros(&self) -> u64 {
        0
    }
}

/// What the fixture's `make` does.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Submits one screen whose bytes lie in the image: parked.
    Parkable,
    /// The same screen, encoded on its stack: copied, not parked.
    Copied,
    /// Reads the clock before submitting: not only registered.
    ReadsClock,
    /// A widget besides the screen: a role another plugin reaches.
    Widget,
    /// Told `event.started`: would be made at once.
    Started,
    /// The screen under another title: another batch than the admission's.
    Retitled,
}

std::thread_local! {
    static MODE: Cell<Mode> = const { Cell::new(Mode::Parkable) };
    static MAKES: Cell<u32> = const { Cell::new(0) };
    static DROPS: Cell<u32> = const { Cell::new(0) };
}

/// The program's read-only data as far as the fixture goes: the encoded
/// declaration batches, at fixed places, never written after they are made.
struct Image {
    bytes: &'static [u8],
    screen: (usize, usize),
    retitled: (usize, usize),
    widget: (usize, usize),
}

fn image() -> &'static Image {
    static IMAGE: OnceLock<Image> = OnceLock::new();
    IMAGE.get_or_init(|| {
        let mut bytes = std::vec![0; 1024];
        let screen = |title| {
            wire::Record::ScreenSpec(wire::ScreenSpec {
                name: "screen",
                title,
                token: 91,
                flags: 0,
            })
        };
        let first = wire::records_encode(&[screen("linked.title")], &mut bytes).unwrap();
        let second = wire::records_encode(&[screen("linked.other")], &mut bytes[first..]).unwrap();
        let widget = wire::Record::WidgetSpec(wire::WidgetSpec {
            name: "widget",
            title: "linked.widget",
            category: "core.widgets.words",
            previews: "",
            cells: 0x0101_0102,
            token: 92,
            flags: 0,
        });
        let third = wire::records_encode(
            &[screen("linked.title"), widget],
            &mut bytes[first + second..],
        )
        .unwrap();
        Image {
            screen: (0, first),
            retitled: (first, second),
            widget: (first + second, third),
            bytes: std::vec::Vec::leak(bytes),
        }
    })
}

fn submit(host: *const PpHost, ctx: *mut c_void, bytes: &[u8]) -> i32 {
    // SAFETY: the host table and context are live for this `make`; the bytes
    // stay readable until the call returns.
    unsafe {
        ((*host).registrations_submit)(
            ctx,
            PpBytes {
                ptr: bytes.as_ptr(),
                len: bytes.len(),
            },
        )
    }
}

fn submit_registrations(
    host: *const PpHost,
    ctx: *mut c_void,
    batch: &[wire::Registration],
) -> i32 {
    let mut bytes = [0; 64];
    let len = wire::registrations_encode(batch, &mut bytes).unwrap();
    submit(host, ctx, &bytes[..len])
}

unsafe extern "C" fn make(host: *const PpHost, ctx: *mut c_void, out: *mut *const PpPlugin) -> i32 {
    MAKES.with(|count| count.set(count.get() + 1));
    let image = image();
    let slice = |(at, len): (usize, usize)| &image.bytes[at..at + len];
    let mode = MODE.with(Cell::get);
    let status = match mode {
        Mode::Parkable | Mode::Started => submit(host, ctx, slice(image.screen)),
        Mode::Retitled => submit(host, ctx, slice(image.retitled)),
        Mode::Copied => {
            let copy = slice(image.screen).to_vec();
            submit(host, ctx, &copy)
        }
        Mode::ReadsClock => {
            let mut micros = 0;
            // SAFETY: as `submit`; `micros` is this frame's.
            unsafe { ((*host).clock_micros)(ctx, &mut micros) };
            submit(host, ctx, slice(image.screen))
        }
        Mode::Widget => match submit(host, ctx, slice(image.widget)) {
            host_glue::OK => {
                submit_registrations(host, ctx, &[wire::Registration::Draw { token: 92 }])
            }
            status => status,
        },
    };
    if status != host_glue::OK {
        return status;
    }
    if mode == Mode::Started {
        let started = wire::Registration::Event {
            event_tag: wire::EVENT_STARTED_TAG,
            token: 93,
        };
        let status = submit_registrations(host, ctx, &[started]);
        if status != host_glue::OK {
            return status;
        }
    }
    let table = Box::into_raw(Box::new(PpPlugin {
        size: u32::try_from(size_of::<PpPlugin>()).unwrap(),
        state: ptr::null_mut(),
        drop: Some(destroy),
        dispatch_event: Some(event),
        input: None,
        draw: Some(draw),
    }));
    // SAFETY: `out` is the host's slot; the table is released by `destroy`.
    unsafe {
        (*table).state = table.cast();
        out.write(table);
    }
    host_glue::OK
}

unsafe extern "C" fn destroy(state: *mut c_void) {
    DROPS.with(|count| count.set(count.get() + 1));
    // SAFETY: the table `make` leaked, released once.
    drop(unsafe { Box::from_raw(state.cast::<PpPlugin>()) });
}

unsafe extern "C" fn event(_: *mut c_void, _: u32, _: PpBytes, _: PpBuffer) -> i32 {
    host_glue::OK
}

unsafe extern "C" fn draw(_: *mut c_void, _: u32, _: PpBytes) -> i32 {
    host_glue::OK
}

fn entry() -> &'static PpEntry {
    Box::leak(Box::new(PpEntry {
        size: u32::try_from(size_of::<PpEntry>()).unwrap(),
        abi_major: host_glue::MAJOR,
        needs_minor: 1,
        metadata: PpBytes {
            ptr: METADATA.as_ptr(),
            len: METADATA.len(),
        },
        make: Some(make),
    }))
}

fn shell_host() -> Host<'static> {
    Host {
        abi_major: host_glue::MAJOR,
        minor: host_glue::MINOR,
        profile: Profile {
            target: "linux-x86_64-sysv64-le",
            entry_layout: 1,
            entry_bytes: u16::try_from(size_of::<PpEntry>()).unwrap(),
        },
        services: &[],
        features: &[],
    }
}

fn kernel(mode: Mode) -> crate::Kernel {
    MODE.with(|cell| cell.set(mode));
    MAKES.with(|count| count.set(0));
    DROPS.with(|count| count.set(0));
    crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap()
}

/// Admits the fixture as `linked` through the parking entry.
fn admit(kernel: &mut crate::Kernel) -> u64 {
    let image = image().bytes;
    // SAFETY: the fixture obeys the factory contract, its code is static and
    // `image` is leaked and never written after it was made.
    unsafe {
        kernel.register_linked_parked(
            entry(),
            METADATA,
            &shell_host(),
            "linked",
            || Heap,
            HostResources::try_new().unwrap(),
            &FlatDepth::try_new().unwrap(),
            |_, _, _, _| {},
            |_, _, _| {},
            (image.as_ptr(), image.len()),
            FixedTables::NONE,
        )
    }
    .unwrap();
    kernel.shared.native_instances.find("linked").unwrap()
}

fn lease(kernel: &crate::Kernel, key: u64) -> super::super::native_instances::NativeLease {
    kernel.shared.native_instances.lease(key).unwrap()
}

fn makes() -> u32 {
    MAKES.with(Cell::get)
}

fn drops() -> u32 {
    DROPS.with(Cell::get)
}

#[test]
fn a_plugin_that_only_registered_is_parked_with_its_screen_published() {
    let mut kernel = kernel(Mode::Parkable);
    let key = admit(&mut kernel);
    // Made once, its instance dropped, its screen there to be opened.
    assert_eq!((makes(), drops()), (1, 1));
    assert!(lease(&kernel, key).is_parked());
    assert!(lease(&kernel, key).running());
    assert!(kernel.shared.registry.screens.contains_key("linked.screen"));
}

#[test]
fn the_first_use_makes_a_parked_plugin_once() {
    let mut kernel = kernel(Mode::Parkable);
    let key = admit(&mut kernel);
    let lease = lease(&kernel, key);
    assert!(lease.live().is_ok());
    assert_eq!(makes(), 2);
    assert!(!lease.is_parked());
    // Made: a second use takes the same instance.
    assert!(lease.live().is_ok());
    assert_eq!((makes(), drops()), (2, 1));
    drop(lease);
    drop(kernel);
    assert_eq!(drops(), 2);
}

#[test]
fn a_plugin_whose_declarations_are_copied_runs() {
    let mut kernel = kernel(Mode::Copied);
    let key = admit(&mut kernel);
    assert_eq!((makes(), drops()), (1, 0));
    assert!(!lease(&kernel, key).is_parked());
}

#[test]
fn a_plugin_whose_make_read_the_clock_runs() {
    let mut kernel = kernel(Mode::ReadsClock);
    let key = admit(&mut kernel);
    assert_eq!((makes(), drops()), (1, 0));
    assert!(!lease(&kernel, key).is_parked());
}

#[test]
fn a_plugin_with_a_widget_runs() {
    let mut kernel = kernel(Mode::Widget);
    let key = admit(&mut kernel);
    assert_eq!((makes(), drops()), (1, 0));
    assert!(!lease(&kernel, key).is_parked());
}

#[test]
fn a_plugin_told_at_the_start_runs() {
    let mut kernel = kernel(Mode::Started);
    let key = admit(&mut kernel);
    assert_eq!((makes(), drops()), (1, 0));
    assert!(!lease(&kernel, key).is_parked());
}

#[test]
fn another_batch_on_the_first_use_refuses_the_plugin() {
    let mut kernel = kernel(Mode::Parkable);
    let key = admit(&mut kernel);
    MODE.with(|cell| cell.set(Mode::Retitled));
    let lease = lease(&kernel, key);
    assert_eq!(lease.live().err(), Some(ERR_CANCELLED));
    // Made and dropped again; withdrawn at the end of a pass, not made again.
    assert_eq!((makes(), drops()), (2, 2));
    assert!(lease.refused());
    assert_eq!(lease.live().err(), Some(ERR_CANCELLED));
    assert_eq!(makes(), 2);
}

#[test]
fn the_plain_entry_never_parks() {
    let mut kernel = kernel(Mode::Parkable);
    let image = image().bytes;
    // SAFETY: as `admit`.
    unsafe {
        kernel.register_linked(
            entry(),
            METADATA,
            &shell_host(),
            "linked",
            Heap,
            HostResources::try_new().unwrap(),
            &FlatDepth::try_new().unwrap(),
            |_, _, _, _| {},
            |_, _, _| {},
            (image.as_ptr(), image.len()),
            FixedTables::NONE,
        )
    }
    .unwrap();
    let key = kernel.shared.native_instances.find("linked").unwrap();
    assert_eq!((makes(), drops()), (1, 0));
    assert!(!lease(&kernel, key).is_parked());
}
