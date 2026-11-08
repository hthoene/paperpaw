//! `Kernel::register_linked`: refusals run no `make`; a good entry runs it once.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]

use super::*;
use crate::native_host::{HostResources, registrations as wire};
use alloc::boxed::Box;
use core::{cell::Cell, ffi::c_void, ptr};
use paperpaw_native::host_glue::{self, PpBytes, PpHost, PpPlugin};
use paperpaw_native::metadata::{Feature, Host, Profile};
use std::alloc::Layout;

extern crate std;

// Emitted by tools/abi-metadata.py with target linux-x86_64-sysv64-le,
// floor 1, and empty used/services/features (as in `linked_entry_tests`).
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

std::thread_local! {
    static MAKES: Cell<u32> = const { Cell::new(0) };
    static DROPS: Cell<u32> = const { Cell::new(0) };
}

/// A plugin that declares one screen: `make` counts itself, submits the
/// declaration and publishes a table (a plugin that declares nothing is
/// refused by the registration, which is not what these tests are about).
unsafe extern "C" fn make(host: *const PpHost, ctx: *mut c_void, out: *mut *const PpPlugin) -> i32 {
    MAKES.with(|count| count.set(count.get() + 1));
    let mut bytes = [0; 128];
    let len = wire::records_encode(
        &[wire::Record::ScreenSpec(wire::ScreenSpec {
            name: "screen",
            title: "linked.title",
            token: 91,
            flags: 0,
        })],
        &mut bytes,
    )
    .unwrap();
    // SAFETY: the host table and context are live for this call; the encoded
    // declaration stays readable until it returns.
    let status = unsafe {
        ((*host).registrations_submit)(
            ctx,
            PpBytes {
                ptr: bytes.as_ptr(),
                len,
            },
        )
    };
    if status != host_glue::OK {
        return status;
    }
    let table = Box::into_raw(Box::new(PpPlugin {
        size: u32::try_from(size_of::<PpPlugin>()).unwrap(),
        state: ptr::null_mut(),
        drop: Some(destroy),
        dispatch_event: None,
        input: None,
        draw: None,
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

fn entry(change: impl FnOnce(&mut PpEntry)) -> &'static PpEntry {
    let mut entry = PpEntry {
        size: u32::try_from(size_of::<PpEntry>()).unwrap(),
        abi_major: host_glue::MAJOR,
        needs_minor: 1,
        metadata: PpBytes {
            ptr: METADATA.as_ptr(),
            len: METADATA.len(),
        },
        make: Some(make),
    };
    change(&mut entry);
    Box::leak(Box::new(entry))
}

const REGISTRY: &str = "uses.registry";
const WIRED: &[Feature<'static>] = &[Feature {
    key: REGISTRY,
    revision: 1,
}];

/// The shell's card-plugin host, as a test shell would build it.
fn shell_host(features: &'static [Feature<'static>]) -> Host<'static> {
    Host {
        abi_major: host_glue::MAJOR,
        minor: host_glue::MINOR,
        profile: Profile {
            target: "linux-x86_64-sysv64-le",
            entry_layout: 1,
            entry_bytes: u16::try_from(size_of::<PpEntry>()).unwrap(),
        },
        services: &[],
        features,
    }
}

/// `METADATA` plus one required `uses.registry` feature record.
fn registry_metadata() -> &'static [u8] {
    let mut bytes = METADATA.to_vec();
    bytes.extend_from_slice(&u16::try_from(8 + REGISTRY.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&u16::try_from(REGISTRY.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(REGISTRY.as_bytes());
    let length = u32::try_from(bytes.len()).unwrap();
    bytes[8..12].copy_from_slice(&length.to_le_bytes());
    bytes[28..30].copy_from_slice(&1_u16.to_le_bytes());
    Box::leak(bytes.into_boxed_slice())
}

fn install(
    kernel: &mut crate::Kernel,
    entry: &'static PpEntry,
    metadata: &'static [u8],
    id: &str,
) -> Result<(), shell::Error> {
    install_on(kernel, entry, metadata, &shell_host(&[]), id)
}

fn install_on(
    kernel: &mut crate::Kernel,
    entry: &'static PpEntry,
    metadata: &'static [u8],
    host: &Host<'static>,
    id: &str,
) -> Result<(), shell::Error> {
    // SAFETY: the test factory obeys the contract and its code is static.
    unsafe {
        kernel.register_linked(
            entry,
            metadata,
            host,
            id,
            Heap,
            HostResources::try_new().unwrap(),
            &FlatDepth::try_new().unwrap(),
            |_, _, _, _| {},
            |_, _, _| {},
        )
    }
}

fn kernel() -> crate::Kernel {
    MAKES.with(|count| count.set(0));
    DROPS.with(|count| count.set(0));
    crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap()
}

fn status(result: Result<(), shell::Error>) -> i32 {
    match result {
        Err(shell::Error::Status(status)) => status,
        other => panic!("not a status refusal: {other:?}"),
    }
}

#[test]
fn a_good_entry_runs_make_once_and_its_id_is_taken() {
    let mut kernel = kernel();
    install(&mut kernel, entry(|_| {}), METADATA, "linked").unwrap();
    assert_eq!(MAKES.with(Cell::get), 1);
    assert!(kernel.shared.native_instances.find("linked").is_some());
    // The same id again is refused and `make` does not run a second time.
    let again = install(&mut kernel, entry(|_| {}), METADATA, "linked");
    assert_eq!(status(again), ERR_INVALID);
    assert_eq!(MAKES.with(Cell::get), 1);
    drop(kernel);
    assert_eq!(DROPS.with(Cell::get), 1);
}

#[test]
fn a_wrong_metadata_pointer_or_length_is_an_entry_refusal_before_make() {
    let mut kernel = kernel();
    for bad in [
        entry(|entry| entry.metadata.ptr = ptr::null()),
        entry(|entry| entry.metadata.ptr = ptr::dangling()),
        entry(|entry| entry.metadata.len -= 1),
        entry(|entry| entry.metadata.len = usize::MAX),
    ] {
        assert_eq!(
            status(install(&mut kernel, bad, METADATA, "linked")),
            ERR_INVALID
        );
    }
    // The same bytes in another allocation are not the entry's own view.
    let copy: &'static [u8] = Box::leak(METADATA.to_vec().into_boxed_slice());
    assert_eq!(
        status(install(&mut kernel, entry(|_| {}), copy, "linked")),
        ERR_INVALID
    );
    // Versions and the factory are part of the entry.
    for bad in [
        entry(|entry| entry.abi_major += 1),
        entry(|entry| entry.needs_minor = 0),
        entry(|entry| entry.make = None),
    ] {
        assert_eq!(
            status(install(&mut kernel, bad, METADATA, "linked")),
            ERR_INVALID
        );
    }
    assert_eq!(MAKES.with(Cell::get), 0);
    assert!(kernel.shared.native_instances.find("linked").is_none());
}

#[test]
fn metadata_the_kernel_refuses_is_unsupported_before_make() {
    let mut kernel = kernel();
    for bytes in [
        b"".as_slice(),
        b"PPAM",
        &[0; paperpaw_native::metadata::MAX_BYTES + 1],
    ] {
        let bad = entry(|entry| {
            entry.metadata = PpBytes {
                ptr: bytes.as_ptr(),
                len: bytes.len(),
            };
        });
        assert_eq!(
            status(install(&mut kernel, bad, bytes, "linked")),
            ERR_UNSUPPORTED
        );
    }
    // A shell host of another target profile is incompatible.
    let mut elsewhere = shell_host(&[]);
    elsewhere.profile.target = "esp32c3-riscv32imc-ilp32-soft-le";
    assert_eq!(
        status(install_on(
            &mut kernel,
            entry(|_| {}),
            METADATA,
            &elsewhere,
            "linked"
        )),
        ERR_UNSUPPORTED
    );
    // Metadata of another target is incompatible with the shell's.
    let other: &'static [u8] = {
        let mut bytes = METADATA.to_vec();
        let at = bytes
            .windows(22)
            .position(|window| window == b"linux-x86_64-sysv64-le")
            .unwrap();
        bytes[at..at + 5].copy_from_slice(b"linuy");
        Box::leak(bytes.into_boxed_slice())
    };
    let bad = entry(|entry| {
        entry.metadata = PpBytes {
            ptr: other.as_ptr(),
            len: other.len(),
        };
    });
    assert_eq!(
        status(install(&mut kernel, bad, other, "linked")),
        ERR_UNSUPPORTED
    );
    assert_eq!(MAKES.with(Cell::get), 0);
}

#[test]
fn an_unparseable_id_is_refused_before_make() {
    let mut kernel = kernel();
    let long = "a".repeat(41);
    for id in [
        "",
        "Not An Id",
        "a b",
        "a..b",
        "a@x",
        "x@2",
        "a.b",
        long.as_str(),
    ] {
        assert_eq!(
            status(install(&mut kernel, entry(|_| {}), METADATA, id)),
            ERR_INVALID
        );
    }
    assert_eq!(MAKES.with(Cell::get), 0);
}

#[test]
fn a_required_feature_the_shell_host_lacks_refuses_the_plugin_before_make() {
    let bytes = registry_metadata();
    let with_view = entry(|entry| {
        entry.metadata = PpBytes {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        };
    });
    let mut kernel = kernel();
    let refused = install_on(&mut kernel, with_view, bytes, &shell_host(&[]), "linked");
    assert_eq!(status(refused), ERR_UNSUPPORTED);
    assert_eq!(MAKES.with(Cell::get), 0);
    assert!(kernel.shared.native_instances.find("linked").is_none());
}

#[test]
fn a_required_feature_the_shell_host_has_admits_the_plugin() {
    let bytes = registry_metadata();
    let with_view = entry(|entry| {
        entry.metadata = PpBytes {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        };
    });
    let mut kernel = kernel();
    install_on(&mut kernel, with_view, bytes, &shell_host(WIRED), "linked").unwrap();
    assert_eq!(MAKES.with(Cell::get), 1);
    assert!(kernel.shared.native_instances.find("linked").is_some());
}
