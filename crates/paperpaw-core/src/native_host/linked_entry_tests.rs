//! Linked admission uses generated metadata and never executes a factory.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]

use super::*;
use alloc::{boxed::Box, vec::Vec};
use core::{cell::Cell, ffi::c_void, ptr};
use paperpaw_native::host_glue::{self, PpBytes, PpHost, PpPlugin};

extern crate std;

// Emitted by tools/abi-metadata.py with target linux-x86_64-sysv64-le,
// floor 1, and empty used/services/features. This is the packed metadata form.
const METADATA: &[u8] = b"PPAM\x02\x00 \x00N\x00\x00\x00\x07\x00\x01\x00\x01\x00 \x00\x00\x00\x00\x00\x16\x00\x00\x00\x00\x00\x01\x00linux-x86_64-sysv64-le\x18\x00\x01\x00 \x00\x00\x00\x0e\x00PAPERPAW_ENTRY";

struct NoHeap;
impl Services for NoHeap {
    fn alloc(&self, _: usize, _: usize) -> *mut u8 {
        ptr::null_mut()
    }
    unsafe fn dealloc(&self, _: *mut u8, _: usize, _: usize) {}
    unsafe fn realloc(&self, _: *mut u8, _: usize, _: usize, _: usize) -> *mut u8 {
        ptr::null_mut()
    }
    fn micros(&self) -> u64 {
        0
    }
}

std::thread_local! {
    static MAKES: Cell<u32> = const { Cell::new(0) };
}

unsafe extern "C" fn make(_: *const PpHost, _: *mut c_void, _: *mut *const PpPlugin) -> i32 {
    MAKES.with(|count| count.set(count.get() + 1));
    host_glue::ERR_UNSUPPORTED
}

fn host() -> metadata::Host<'static> {
    metadata::Host {
        abi_major: host_glue::MAJOR,
        minor: host_glue::MINOR,
        profile: metadata::Profile {
            target: "linux-x86_64-sysv64-le",
            entry_layout: 1,
            entry_bytes: u16::try_from(core::mem::size_of::<PpEntry>()).unwrap(),
        },
        services: &[],
        features: &[],
    }
}

fn entry(bytes: &'static [u8], change: impl FnOnce(&mut PpEntry)) -> &'static PpEntry {
    let mut entry = PpEntry {
        size: u32::try_from(core::mem::size_of::<PpEntry>()).unwrap(),
        abi_major: host_glue::MAJOR,
        needs_minor: 1,
        metadata: PpBytes {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        },
        make: Some(make),
    };
    change(&mut entry);
    Box::leak(Box::new(entry))
}

fn refused(
    entry: &'static PpEntry,
    bytes: &'static [u8],
    host: &metadata::Host<'static>,
) -> Refusal {
    MAKES.with(|count| count.set(0));
    let refusal = check(entry, bytes, host).unwrap_err();
    assert_eq!(MAKES.with(Cell::get), 0, "admission never calls make");
    refusal
}

#[test]
fn linked_entry_admits_the_generated_metadata_without_running_make() {
    MAKES.with(|count| count.set(0));
    let admitted = check(entry(METADATA, |_| {}), METADATA, &host()).unwrap();
    assert_eq!(admitted.metadata.abi_major, host_glue::MAJOR);
    assert_eq!(admitted.metadata.needs_minor, 1);
    assert_eq!(admitted.metadata.available_features().count(), 0);
    assert_eq!(MAKES.with(Cell::get), 0);
    // SAFETY: this test factory touches no pointers and returns an error.
    let status = unsafe { (admitted.make)(ptr::null(), ptr::null_mut(), ptr::null_mut()) };
    assert_eq!(status, host_glue::ERR_UNSUPPORTED);
    assert_eq!(MAKES.with(Cell::get), 1);
}

#[test]
fn linked_entry_refuses_bad_prefix_versions_and_missing_factory_before_make() {
    for entry in [
        entry(METADATA, |entry| entry.size -= 1),
        entry(METADATA, |entry| entry.size += 1),
        entry(METADATA, |entry| entry.abi_major += 1),
        entry(METADATA, |entry| entry.needs_minor = 0),
        entry(METADATA, |entry| entry.needs_minor += 1),
        entry(METADATA, |entry| entry.make = None),
    ] {
        assert_eq!(refused(entry, METADATA, &host()), Refusal::Entry);
    }
}

#[test]
fn linked_entry_refuses_unverified_metadata_pointers_without_reading_them() {
    for entry in [
        entry(METADATA, |entry| entry.metadata.ptr = ptr::null()),
        entry(METADATA, |entry| entry.metadata.ptr = ptr::dangling()),
        entry(METADATA, |entry| entry.metadata.len = usize::MAX),
        entry(METADATA, |entry| entry.metadata.len -= 1),
    ] {
        assert_eq!(refused(entry, METADATA, &host()), Refusal::Entry);
    }
    // Equal contents in another allocation are not the paired static object.
    let copy = Box::leak(METADATA.to_vec().into_boxed_slice());
    assert_eq!(
        refused(entry(METADATA, |_| {}), copy, &host()),
        Refusal::Entry
    );
}

#[test]
fn linked_entry_uses_the_card_metadata_reader_and_its_target_admission() {
    for bytes in [b"".as_slice(), b"PPAM", &[0; metadata::MAX_BYTES + 1]] {
        assert_eq!(
            refused(entry(bytes, |_| {}), bytes, &host()),
            Refusal::Metadata(metadata::Refusal::Malformed)
        );
    }
    let mut other = host();
    other.profile.target = "esp32c3-riscv32imc-ilp32-soft-le";
    assert_eq!(
        refused(entry(METADATA, |_| {}), METADATA, &other),
        Refusal::Metadata(metadata::Refusal::Incompatible)
    );
    other = host();
    other.minor = 0;
    assert_eq!(
        refused(entry(METADATA, |_| {}), METADATA, &other),
        Refusal::Metadata(metadata::Refusal::Incompatible)
    );
}

fn fixture(hex: &str) -> &'static [u8] {
    let hex = hex.trim();
    let bytes: Vec<_> = (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
        .collect();
    Box::leak(bytes.into_boxed_slice())
}

#[test]
fn linked_entry_preserves_required_feature_refusals() {
    let bytes = fixture(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../abi/fixtures/metadata/feature-required-unknown.hex"
    )));
    let host = metadata::Host {
        profile: metadata::Profile {
            target: "test-only-le64",
            entry_layout: u16::MAX,
            entry_bytes: 64,
        },
        ..host()
    };
    assert_eq!(
        refused(entry(bytes, |_| {}), bytes, &host),
        Refusal::Metadata(metadata::Refusal::RequiredFeature {
            key: "test.unknown",
            revision: 1,
        })
    );
}

#[test]
fn linked_host_copies_service_admission_before_any_factory_call() {
    MAKES.with(|count| count.set(0));
    let admitted = check(entry(METADATA, |_| {}), METADATA, &host()).unwrap();
    // SAFETY: this test's factory and data are static and never unloaded.
    let hosted = unsafe {
        admitted.hosted(
            NoHeap,
            "fixture",
            |_, _, _, _| {},
            super::super::HostResources::try_new().unwrap(),
            (core::ptr::null(), 0),
        )
    }
    .unwrap();
    assert_eq!(MAKES.with(Cell::get), 0);
    assert_eq!(hosted.image(), Some(&[][..]));
    let transaction = hosted
        .begin::<super::super::registrations::Registration>()
        .unwrap();
    assert_eq!(
        hosted.make_registered(transaction).err(),
        Some(shell::Error::Factory(host_glue::ERR_UNSUPPORTED))
    );
    assert_eq!(MAKES.with(Cell::get), 1);
}

#[test]
fn linked_host_retains_available_features_after_admission() {
    const FEATURES: &[metadata::Feature<'static>] = &[metadata::Feature {
        key: "uses.registry",
        revision: 1,
    }];
    let mut bytes = METADATA.to_vec();
    let key = FEATURES[0].key;
    bytes.extend_from_slice(&u16::try_from(8 + key.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&u16::try_from(key.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(key.as_bytes());
    let length = u32::try_from(bytes.len()).unwrap();
    bytes[8..12].copy_from_slice(&length.to_le_bytes());
    bytes[28..30].copy_from_slice(&1_u16.to_le_bytes());
    let bytes = Box::leak(bytes.into_boxed_slice());
    let capabilities = metadata::Host {
        features: FEATURES,
        ..host()
    };
    let admitted = check(entry(bytes, |_| {}), bytes, &capabilities).unwrap();
    // SAFETY: the fixture's factory and metadata are static for the whole test.
    let hosted = unsafe {
        admitted.hosted(
            NoHeap,
            "fixture",
            |_, _, _, _| {},
            super::super::HostResources::try_new().unwrap(),
            (core::ptr::null(), 0),
        )
    }
    .unwrap();
    assert_eq!(hosted.features().feature(key), Ok(()));
    assert_eq!(
        hosted.features().feature("uses.unknown"),
        Err(host_glue::ERR_UNSUPPORTED)
    );
}
