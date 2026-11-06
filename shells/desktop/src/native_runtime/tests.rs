use super::*;
use std::process::Command;

pub(super) fn metadata_bytes() -> Vec<u8> {
    use wire::*;
    let mut bytes = vec![0; PREFIX_SIZE];
    bytes[..MAGIC_BYTES.len()].copy_from_slice(MAGIC_BYTES);
    for (at, value) in [
        (LAYOUT, FORMAT),
        (PREFIX_BYTES, PREFIX_SIZE as u16),
        (ABI_MAJOR, 7),
        (NEEDS_MINOR, 1),
        (ENTRY_LAYOUT, PROFILE_ENTRY_LAYOUT as u16),
        (ENTRY_BYTES, PROFILE_ENTRY_BYTES as u16),
        (TARGET_BYTES, PROFILE.len() as u16),
        (EXPORTS, 1),
    ] {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(PROFILE);
    let mut record = vec![0; EXPORT_SIZE];
    record[EXPORT_RECORD_BYTES..EXPORT_RECORD_BYTES + 2]
        .copy_from_slice(&((EXPORT_SIZE + ENTRY_SYMBOL.len()) as u16).to_le_bytes());
    record[EXPORT_KIND] = 1;
    record[EXPORT_EXPECTED_BYTES..EXPORT_EXPECTED_BYTES + 4]
        .copy_from_slice(&(PROFILE_ENTRY_BYTES as u32).to_le_bytes());
    record[EXPORT_NAME_BYTES..EXPORT_NAME_BYTES + 2]
        .copy_from_slice(&(ENTRY_SYMBOL.len() as u16).to_le_bytes());
    bytes.extend(record);
    bytes.extend_from_slice(ENTRY_SYMBOL.as_bytes());
    let total = bytes.len() as u32;
    bytes[TOTAL_BYTES..TOTAL_BYTES + 4].copy_from_slice(&total.to_le_bytes());
    bytes
}

#[test]
fn metadata_is_exact_and_requirements_fail_closed() {
    let host = crate::host_services::desktop_host();
    let bytes = metadata_bytes();
    assert_eq!(metadata(&bytes, host.table()), Ok(1));
    for at in [
        wire::ABI_MAJOR,
        wire::NEEDS_MINOR,
        wire::FLAGS,
        wire::TARGET_BYTES,
        wire::SERVICES,
        wire::FEATURES,
        wire::EXPORTS,
        wire::ENTRY_BYTES,
    ] {
        let mut bad = bytes.clone();
        bad[at] = 255;
        assert_eq!(
            metadata(&bad, host.table()),
            Err(RuntimeError::Metadata),
            "offset {at}"
        );
    }
    for len in 0..bytes.len() {
        assert_eq!(
            metadata(&bytes[..len], host.table()),
            Err(RuntimeError::Metadata)
        );
    }
    let mut tail = bytes;
    tail.push(0);
    assert_eq!(metadata(&tail, host.table()), Err(RuntimeError::Metadata));
}

/// Build actual no_std shared libraries and run make/drop through dlopen.
/// No compiler failure is accepted as a skipped positive control.
#[test]
fn real_library_lifecycle_and_interface6_rejection() {
    let root = crate::private::dir("paperpaw-abi7-runtime").unwrap();
    let abi = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../abi/generated/paperpaw_abi.rs")
        .canonicalize()
        .unwrap();
    let bytes = metadata_bytes();
    let source = format!(
        r#"
#![no_std]
#![allow(dead_code)]
include!({abi:?});
#[panic_handler] fn panic(_: &core::panic::PanicInfo) -> ! {{ loop {{}} }}
#[unsafe(no_mangle)] pub static PAPERPAW_METADATA: [u8; {len}] = {bytes:?};
#[repr(transparent)] pub struct Entry(PpEntry);
unsafe impl Sync for Entry {{}}
#[unsafe(no_mangle)] pub static PAPERPAW_ENTRY: Entry = Entry(PpEntry {{
    size: core::mem::size_of::<PpEntry>() as u32, abi_major: 7, needs_minor: 1,
    metadata: PpBytes {{ ptr: PAPERPAW_METADATA.as_ptr(), len: {len} }}, make: Some(make),
}});
#[repr(C)] struct State {{ host: *const PpHost, ctx: *mut core::ffi::c_void, plugin: PpPlugin }}
unsafe extern "C" fn make(host: *const PpHost, ctx: *mut core::ffi::c_void, out: *mut *const PpPlugin) -> i32 {{
    let mut block = core::ptr::null_mut();
    let h = unsafe {{ &*host }};
    let status = unsafe {{ (h.heap_alloc)(ctx, core::mem::size_of::<State>(), core::mem::align_of::<State>(), &mut block) }};
    if status != OK {{ return status; }}
    let state = block.cast::<State>();
    unsafe {{ state.write(State {{ host, ctx, plugin: PpPlugin {{ size: core::mem::size_of::<PpPlugin>() as u32, state: block, drop: Some(destroy), dispatch_event: None, draw: None }} }});
        out.write(&raw const (*state).plugin); }}
    OK
}}
unsafe extern "C" fn destroy(state: *mut core::ffi::c_void) {{
    let s = unsafe {{ &*state.cast::<State>() }};
    let h = unsafe {{ &*s.host }};
    unsafe {{ (h.heap_dealloc)(s.ctx, state, core::mem::size_of::<State>(), core::mem::align_of::<State>()); }}
}}
"#,
        len = bytes.len()
    );
    let compile = |name: &str, source: &str| {
        let input = root.join(format!("{name}.rs"));
        let output = root.join(format!("{name}.so"));
        std::fs::write(&input, source).unwrap();
        let result = Command::new("rustc")
            .args([
                "--edition=2024",
                "--crate-type=cdylib",
                "-Cpanic=abort",
                "-Copt-level=2",
                "-Clink-arg=-nostartfiles",
            ])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        output
    };
    let path = compile("lifecycle", &source);
    let digest = paperpaw_core::sdk::digest::sha256::digest(&std::fs::read(&path).unwrap());
    let host = crate::host_services::desktop_host();
    for _ in 0..4 {
        // SAFETY: the local fixture above implements the reviewed factory/drop contract.
        let plugin = unsafe { load(&path, &digest, host) }.unwrap();
        assert!(!plugin.state.is_null());
        drop(plugin);
    }
    // Interface-6's differently shaped entry must never be read as PpEntry.
    let old = compile(
        "old_interface",
        r#"#![no_std]
#[panic_handler] fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
#[unsafe(no_mangle)] pub static PAPERPAW_ENTRY: u64 = 0x0006c55d0a4fa341;
"#,
    );
    let digest = paperpaw_core::sdk::digest::sha256::digest(&std::fs::read(&old).unwrap());
    // SAFETY: the fixture has no executable entry path or initialisers.
    assert!(matches!(
        unsafe { load(&old, &digest, host) },
        Err(RuntimeError::Export)
    ));
    std::fs::remove_dir_all(root).unwrap();
}

/// PR #133: a library that is otherwise valid (metadata, entry, factory) but
/// imports a symbol is refused from its bytes, before it is mapped. The
/// positive control is the same source without the import.
#[test]
fn real_libraries_that_import_symbols_are_refused_before_any_load() {
    let root = crate::private::dir("paperpaw-abi7-imports").unwrap();
    let abi = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../abi/generated/paperpaw_abi.rs")
        .canonicalize()
        .unwrap();
    let bytes = metadata_bytes();
    let source = |imports: &str, call: &str| {
        format!(
            r#"
#![no_std]
#![allow(dead_code)]
include!({abi:?});
#[panic_handler] fn panic(_: &core::panic::PanicInfo) -> ! {{ loop {{}} }}
{imports}
#[unsafe(no_mangle)] pub static PAPERPAW_METADATA: [u8; {len}] = {bytes:?};
#[repr(transparent)] pub struct Entry(PpEntry);
unsafe impl Sync for Entry {{}}
#[unsafe(no_mangle)] pub static PAPERPAW_ENTRY: Entry = Entry(PpEntry {{
    size: core::mem::size_of::<PpEntry>() as u32, abi_major: 7, needs_minor: 1,
    metadata: PpBytes {{ ptr: PAPERPAW_METADATA.as_ptr(), len: {len} }}, make: Some(make),
}});
unsafe extern "C" fn make(_: *const PpHost, _: *mut core::ffi::c_void, out: *mut *const PpPlugin) -> i32 {{
    {call}
    unsafe {{ out.write(core::ptr::null()); }}
    ERR_INVALID
}}
"#,
            len = bytes.len()
        )
    };
    let compile = |name: &str, source: &str| {
        let input = root.join(format!("{name}.rs"));
        let output = root.join(format!("{name}.so"));
        std::fs::write(&input, source).unwrap();
        let result = Command::new("rustc")
            .args([
                "--edition=2024",
                "--crate-type=cdylib",
                "-Cpanic=abort",
                "-Copt-level=2",
                "-Clink-arg=-nostartfiles",
            ])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        output
    };
    let host = crate::host_services::desktop_host();
    let refused = |path: &std::path::Path| {
        let digest = paperpaw_core::sdk::digest::sha256::digest(&std::fs::read(path).unwrap());
        // SAFETY: refusal is decided on bytes; an accepted library is never run here.
        let result = unsafe { load(path, &digest, host) };
        let mapped = std::fs::read_to_string("/proc/self/maps")
            .unwrap()
            .contains(path.to_str().unwrap());
        (result.err(), mapped)
    };
    for (name, imports, call) in [
        (
            "memcpy",
            r#"unsafe extern "C" { fn memcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8; }"#,
            "let mut a = [0u8; 64]; let n = unsafe { core::ptr::read_volatile(&PAPERPAW_METADATA[0]) } as usize % 64; unsafe { memcpy(a.as_mut_ptr(), PAPERPAW_METADATA.as_ptr(), n); core::ptr::read_volatile(&a[0]); }",
        ),
        (
            "personality",
            r#"unsafe extern "C" { fn rust_eh_personality(); }
#[used] static KEEP: unsafe extern "C" fn() = rust_eh_personality;"#,
            "",
        ),
    ] {
        let path = compile(name, &source(imports, call));
        let (error, mapped) = refused(&path);
        assert_eq!(error, Some(RuntimeError::Import), "{name} must be refused");
        assert!(!mapped, "{name} was never mapped");
    }
    // Positive control: no import. It passes the import check and is only then
    // turned away by its (deliberately failing) factory, after a real load.
    let path = compile("clean", &source("", ""));
    let (error, _) = refused(&path);
    assert_eq!(
        error,
        Some(RuntimeError::Factory(
            paperpaw_native::host_glue::ERR_INVALID
        ))
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// A 64-bit little-endian ELF header with `loads` as `PT_LOAD` program headers
/// `(flags, offset, vaddr, filesz, memsz)`, padded to `file_len` bytes.
fn elf_with_loads(loads: &[(u32, u64, u64, u64, u64)], file_len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; file_len.max(64 + 56 * loads.len())];
    bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
    bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
    bytes[56..58].copy_from_slice(&(loads.len() as u16).to_le_bytes());
    for (i, (flags, offset, vaddr, filesz, memsz)) in loads.iter().enumerate() {
        let at = 64 + 56 * i;
        bytes[at..at + 4].copy_from_slice(&1u32.to_le_bytes());
        bytes[at + 4..at + 8].copy_from_slice(&flags.to_le_bytes());
        bytes[at + 8..at + 16].copy_from_slice(&offset.to_le_bytes());
        bytes[at + 16..at + 24].copy_from_slice(&vaddr.to_le_bytes());
        bytes[at + 32..at + 40].copy_from_slice(&filesz.to_le_bytes());
        bytes[at + 40..at + 48].copy_from_slice(&memsz.to_le_bytes());
    }
    bytes
}

const R: u32 = 4;
const RW: u32 = 6;
const RX: u32 = 5;

#[test]
fn the_image_is_the_read_only_segment_that_holds_the_metadata_not_the_first_one() {
    // GNU ld with separate code: headers and dynsym first, `.rodata` later.
    let bytes = elf_with_loads(
        &[
            (R, 0, 0, 0x300, 0x300),
            (RX, 0x1000, 0x1000, 0x400, 0x400),
            (R, 0x2000, 0x2000, 0x500, 0x500),
            (RW, 0x2f00, 0x3f00, 0x100, 0x200),
        ],
        0x3000,
    );
    assert_eq!(
        read_only_segment(&bytes, 0x7000_0000, 0x2010),
        (0x7000_2000, 0x500)
    );
    // The first segment holds the metadata: that one.
    assert_eq!(
        read_only_segment(&bytes, 0x7000_0000, 0x10),
        (0x7000_0000, 0x300)
    );
}

#[test]
fn no_image_where_the_metadata_is_in_writable_or_executable_or_no_segment() {
    let bytes = elf_with_loads(
        &[(RX, 0, 0, 0x300, 0x300), (RW, 0x300, 0x300, 0x100, 0x100)],
        0x400,
    );
    assert_eq!(
        read_only_segment(&bytes, 0x1000, 0x10),
        (0, 0),
        "executable"
    );
    assert_eq!(read_only_segment(&bytes, 0x1000, 0x310), (0, 0), "writable");
    assert_eq!(
        read_only_segment(&bytes, 0x1000, 0x9000),
        (0, 0),
        "in no segment"
    );
    // Past the file bytes of its segment (memsz only): not the segment's data.
    let bss = elf_with_loads(&[(R, 0, 0, 0x100, 0x400)], 0x400);
    assert_eq!(read_only_segment(&bss, 0x1000, 0x200), (0, 0));
    assert_eq!(read_only_segment(&bss, 0x1000, 0x80), (0x1000, 0x100));
}

#[test]
fn a_segment_reaching_past_the_end_of_the_file_gives_no_image() {
    // The size lies: 0x500 bytes claimed at 0x100 in a file of 0x300.
    let lying = elf_with_loads(&[(R, 0x100, 0x100, 0x500, 0x500)], 0x300);
    assert_eq!(read_only_segment(&lying, 0x1000, 0x110), (0, 0));
    // The same segment ending exactly at the end of the file is taken.
    let exact = elf_with_loads(&[(R, 0x100, 0x100, 0x200, 0x200)], 0x300);
    assert_eq!(read_only_segment(&exact, 0x1000, 0x110), (0x1100, 0x200));
    // Offsets that overflow are refused, not wrapped.
    let wrap = elf_with_loads(&[(R, u64::MAX - 8, 0x100, 0x200, 0x200)], 0x300);
    assert_eq!(read_only_segment(&wrap, 0x1000, 0x110), (0, 0));
    let huge = elf_with_loads(&[(R, 0, u64::MAX - 4, 0x200, 0x200)], 0x300);
    assert_eq!(
        read_only_segment(&huge, usize::MAX - 8, u64::MAX - 2),
        (0, 0)
    );
    assert_eq!(read_only_segment(b"junk", 0, 0), (0, 0));
}

#[test]
fn a_segment_with_more_file_than_memory_or_past_the_file_gives_no_image() {
    // Reviewer's repro: filesz 0x500 > memsz 0x100 at offset/vaddr 0x100 in a
    // file of 0x300 bytes, anchor inside it.
    let lying = elf_with_loads(&[(R, 0x100, 0x100, 0x500, 0x100)], 0x300);
    assert_eq!(read_only_segment(&lying, 0x1000, 0x110), (0, 0));
    // More file than memory, though the file is long enough: no segment.
    let more = elf_with_loads(&[(R, 0x100, 0x100, 0x200, 0x100)], 0x400);
    assert_eq!(read_only_segment(&more, 0x1000, 0x110), (0, 0));
    // The file bytes (not the smaller of the two sizes) must lie in the file.
    let past = elf_with_loads(&[(R, 0x100, 0x100, 0x300, 0x400)], 0x300);
    assert_eq!(read_only_segment(&past, 0x1000, 0x110), (0, 0));
    let inside = elf_with_loads(&[(R, 0x100, 0x100, 0x200, 0x400)], 0x300);
    assert_eq!(read_only_segment(&inside, 0x1000, 0x110), (0x1100, 0x200));
}
