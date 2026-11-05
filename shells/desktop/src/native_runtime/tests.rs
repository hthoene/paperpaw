use super::*;
use std::process::Command;

fn metadata_bytes() -> Vec<u8> {
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
