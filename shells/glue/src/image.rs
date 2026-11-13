//! The program's read-only data, and the profile the shell hosts.

use paperpaw_native::metadata;

/// A value the linker puts in this program's read-only data (the same segment
/// as the plugins' declaration bytes), to find that segment by.
static RODATA_ANCHOR: [u8; 16] = *b"paperpaw-rodata\0";

/// The profile this shell hosts: the declared one of the target it is built for
/// (`paperpaw_native::profile`, `abi/abi.def`), as the kernel's admission of a
/// linked plugin names it. Empty where there is none, which admits nothing
/// (`build.rs` only hosts on targets that have one).
pub fn host_profile() -> metadata::Profile<'static> {
    metadata::BUILD_PROFILE.unwrap_or_else(|| {
        eprintln!("linked plugins: no declared profile for this target; none is admitted");
        metadata::Profile {
            target: "",
            entry_layout: 0,
            entry_bytes: 0,
        }
    })
}

/// The program's read-only data as a mapping (address and length) that stays
/// mapped for the whole run, or `(null, 0)` where it cannot be found: the
/// linked plugins keep their declarations as offsets into it, not as copies
/// (`Kernel::register_linked`).
///
/// Found as the mapping of this process that holds `RODATA_ANCHOR` and is
/// readable and not writable: on Linux by `/proc/self/maps` (readable, not
/// writable and not executable), on Windows by `VirtualQuery`, on macOS by
/// `mach_vm_region` (the linker puts constants in the executable `__TEXT`
/// segment there).
pub fn rodata_image() -> (*const u8, usize) {
    let anchor = std::ptr::addr_of!(RODATA_ANCHOR) as usize;
    match readonly_mapping(anchor) {
        Some((start, len)) => (start as *const u8, len),
        None => (std::ptr::null(), 0),
    }
}

/// The `(start, length)` of the mapping of this process that holds `anchor`,
/// if it cannot be written.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn readonly_mapping(anchor: usize) -> Option<(usize, usize)> {
    std::fs::read_to_string("/proc/self/maps")
        .ok()
        .and_then(|maps| mapping_of(&maps, anchor))
}

#[cfg(windows)]
fn readonly_mapping(anchor: usize) -> Option<(usize, usize)> {
    use std::ffi::c_void;
    /// `MEMORY_BASIC_INFORMATION` of a 64-bit process (`winnt.h`).
    #[repr(C)]
    struct MemoryBasicInformation {
        base_address: *mut c_void,
        allocation_base: *mut c_void,
        allocation_protect: u32,
        partition_id: u16,
        region_size: usize,
        state: u32,
        protect: u32,
        kind: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn VirtualQuery(
            address: *const c_void,
            buffer: *mut MemoryBasicInformation,
            length: usize,
        ) -> usize;
    }
    const MEM_COMMIT: u32 = 0x1000;
    const PAGE_READONLY: u32 = 0x02;
    const PAGE_EXECUTE_READ: u32 = 0x20;
    let query = |at: usize| {
        let mut info = std::mem::MaybeUninit::<MemoryBasicInformation>::zeroed();
        let size = std::mem::size_of::<MemoryBasicInformation>();
        // SAFETY: `VirtualQuery` writes at most `length` bytes of `info`, which
        // is as large as the structure it fills.
        unsafe {
            (VirtualQuery(at as *const c_void, info.as_mut_ptr(), size) == size)
                .then(|| info.assume_init())
        }
    };
    // `VirtualQuery` describes the run of pages with the same attributes that
    // starts at the page of the address asked, not at the run's own start: walk
    // the runs from the start of the image's allocation to the one that holds
    // the anchor, which is then whole.
    let first = query(anchor)?;
    let mut at = first.allocation_base as usize;
    loop {
        let run = query(at)?;
        let (start, len) = (run.base_address as usize, run.region_size);
        if start != at || len == 0 {
            return None;
        }
        if anchor < start.saturating_add(len) {
            let readable_only = run.protect == PAGE_READONLY || run.protect == PAGE_EXECUTE_READ;
            return (run.state == MEM_COMMIT && readable_only).then_some((start, len));
        }
        at = start.saturating_add(len);
    }
}

#[cfg(target_os = "macos")]
fn readonly_mapping(anchor: usize) -> Option<(usize, usize)> {
    unsafe extern "C" {
        static mach_task_self_: u32;
        fn mach_vm_region(
            task: u32,
            address: *mut u64,
            size: *mut u64,
            flavor: i32,
            info: *mut i32,
            count: *mut u32,
            object: *mut u32,
        ) -> i32;
    }
    /// `VM_REGION_BASIC_INFO_64`: `vm_region_basic_info_data_64_t`, nine
    /// 32-bit words, the first of them the protection.
    const VM_REGION_BASIC_INFO_64: i32 = 9;
    const INFO_WORDS: u32 = 9;
    const VM_PROT_READ: i32 = 1;
    const VM_PROT_WRITE: i32 = 2;
    let (mut start, mut size) = (anchor as u64, 0_u64);
    let (mut info, mut words, mut object) = ([0_i32; INFO_WORDS as usize], INFO_WORDS, 0_u32);
    // SAFETY: the arguments are the ones `mach_vm_region` documents, the
    // buffer holds `INFO_WORDS` words, and `mach_task_self_` is the task port
    // the system keeps for this process.
    let status = unsafe {
        mach_vm_region(
            mach_task_self_,
            &mut start,
            &mut size,
            VM_REGION_BASIC_INFO_64,
            info.as_mut_ptr(),
            &mut words,
            &mut object,
        )
    };
    // The region that holds the anchor, not the next one after it.
    let holds =
        status == 0 && start <= anchor as u64 && (anchor as u64) < start.saturating_add(size);
    let protection = info[0];
    (holds && protection & VM_PROT_READ != 0 && protection & VM_PROT_WRITE == 0)
        .then_some((usize::try_from(start).ok()?, usize::try_from(size).ok()?))
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    windows,
    target_os = "macos"
)))]
fn readonly_mapping(_anchor: usize) -> Option<(usize, usize)> {
    None
}

/// The `(start, length)` of the read-only, non-executable mapping in `maps`
/// (the text of `/proc/self/maps`) that holds `address`.
#[cfg(any(target_os = "linux", target_os = "android", test))]
pub fn mapping_of(maps: &str, address: usize) -> Option<(usize, usize)> {
    for line in maps.lines() {
        let mut parts = line.split_ascii_whitespace();
        let (range, perms) = (parts.next()?, parts.next()?);
        let (start, end) = range.split_once('-')?;
        let start = usize::from_str_radix(start, 16).ok()?;
        let end = usize::from_str_radix(end, 16).ok()?;
        if address < start || address >= end {
            continue;
        }
        return (perms.starts_with("r--")).then_some((start, end - start));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn this_programs_read_only_data_is_found_and_holds_the_anchor() {
        let (start, len) = rodata_image();
        assert!(len > 0, "no read-only mapping found");
        let anchor = std::ptr::addr_of!(RODATA_ANCHOR) as usize;
        assert!(start as usize <= anchor && anchor < start as usize + len);
    }
}
