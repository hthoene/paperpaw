//! What the release reads from the image about the plugins it runs
//! ([`EXPORTED_ABI`]), the revisions of the shared functions the remote
//! control describes, and where the SDK's allocations with a placement come
//! ([`alloc_in`]).

use core::alloc::Layout;

/// The revisions of the families of shared functions this firmware has,
/// and the oldest whose plugins it still runs, for the remote control's
/// description: the RISC-V chips (X4, X3) share, the X4 Pro's plugins carry
/// their own copies.
#[cfg(all(feature = "remote", feature = "esp32c3"))]
pub(crate) const SHARED_REVISIONS: paperpaw_table::Revisions = paperpaw_table::REVISIONS;
#[cfg(all(feature = "remote", feature = "esp32c3"))]
pub(crate) const SHARED_OLDEST: paperpaw_table::Revisions = paperpaw_table::OLDEST;
#[cfg(all(feature = "remote", feature = "esp32s3"))]
pub(crate) const SHARED_REVISIONS: paperpaw_table::Revisions = [0; paperpaw_table::FAMILIES];
#[cfg(all(feature = "remote", feature = "esp32s3"))]
pub(crate) const SHARED_OLDEST: paperpaw_table::Revisions = [0; paperpaw_table::FAMILIES];

/// What the release writes into its `-abi.txt` (`paperpaw-sign abi`), under a
/// name of its own in the ELF file: the record's version (2), the plugin
/// interface and its minor, the newest a plugin file may need to run here
/// (`abi/ppn5.md`). The store and the web flasher read the file to choose
/// the plugins this firmware runs.
#[used]
#[unsafe(export_name = "PAPERPAW_NATIVE_ABI")]
static EXPORTED_ABI: [u32; 3] = [
    2,
    paperpaw_sdk::native::INTERFACE as u32,
    paperpaw_native::host_glue::MINOR as u32,
];

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
