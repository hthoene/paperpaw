//! The firmware's table for plugins (`paperpaw_native::Api`): its heap,
//! its panics, its clock, the icons' names.

use core::alloc::Layout;

use paperpaw_native::Api;

/// The table, at an address the loader writes into every plugin.
pub static API: Api = Api {
    size: Api::SIZE,
    alloc,
    dealloc,
    realloc,
    panic,
    micros,
    icon,
    alloc_in,
    // no family shared yet (the text family comes with the firmware step of interface 5).
    family_revs: [0; 8],
    family_min: [0; 8],
    build: BUILD.as_ptr(),
    build_len: BUILD.len() as u32,
};

/// What the logs call this build.
const BUILD: &str = concat!("PaperPaw ", env!("CARGO_PKG_VERSION"));

unsafe extern "C" fn alloc(size: usize, align: usize) -> *mut u8 {
    match Layout::from_size_align(size, align) {
        // SAFETY: not zero-sized; as `GlobalAlloc::alloc`.
        Ok(layout) if size > 0 => unsafe { alloc::alloc::alloc(layout) },
        _ => core::ptr::null_mut(),
    }
}

/// Allocates where `placement` says (`paperpaw_sdk::memory::Placement`):
/// 1 internal RAM, 2 PSRAM, where the device has it; where that has no
/// room, anywhere, so the plugin runs, more slowly.
pub unsafe extern "C" fn alloc_in(size: usize, align: usize, placement: u32) -> *mut u8 {
    use esp_alloc::MemoryCapability;
    let Ok(layout) = Layout::from_size_align(size, align) else {
        return core::ptr::null_mut();
    };
    if size == 0 {
        return core::ptr::null_mut();
    }
    let caps = match placement {
        1 => Some(MemoryCapability::Internal),
        #[cfg(feature = "esp32s3")]
        2 => Some(MemoryCapability::External),
        _ => None,
    };
    if let Some(caps) = caps {
        // SAFETY: not zero-sized; freed by the global allocator, which
        // frees blocks of any region.
        let ptr = unsafe { esp_alloc::HEAP.alloc_caps(caps.into(), layout) };
        if !ptr.is_null() {
            return ptr;
        }
    }
    // SAFETY: not zero-sized; as `GlobalAlloc::alloc`.
    unsafe { alloc::alloc::alloc(layout) }
}

unsafe extern "C" fn dealloc(ptr: *mut u8, size: usize, align: usize) {
    if let Ok(layout) = Layout::from_size_align(size, align) {
        // SAFETY: given by `alloc` or `realloc` with this layout.
        unsafe { alloc::alloc::dealloc(ptr, layout) }
    }
}

unsafe extern "C" fn realloc(ptr: *mut u8, size: usize, align: usize, new_size: usize) -> *mut u8 {
    match Layout::from_size_align(size, align) {
        // SAFETY: given by `alloc` or `realloc` with this layout.
        Ok(layout) if new_size > 0 => unsafe { alloc::alloc::realloc(ptr, layout, new_size) },
        _ => core::ptr::null_mut(),
    }
}

unsafe extern "C" fn panic(message: *const u8, len: usize) -> ! {
    // SAFETY: the plugin's message, `len` bytes on its stack.
    let bytes = unsafe { core::slice::from_raw_parts(message, len) };
    // The firmware's panic: kept, the plugin named by its calls.
    panic!(
        "{}",
        core::str::from_utf8(bytes).unwrap_or("a plugin panicked")
    )
}

extern "C" fn micros() -> u64 {
    crate::micros()
}

unsafe extern "C" fn icon(name: *const u8, len: usize) -> u32 {
    // SAFETY: the plugin's text, `len` bytes, read while this answers.
    let bytes = unsafe { core::slice::from_raw_parts(name, len) };
    core::str::from_utf8(bytes)
        .ok()
        .and_then(paperpaw_sdk::icons::char)
        .map_or(0, u32::from)
}
