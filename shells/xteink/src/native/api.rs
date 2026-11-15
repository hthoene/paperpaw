//! What is left of the firmware's table for the plugin files before
//! interface 7 (`paperpaw_native::Api`): none of them is loaded any more.
//! The release still reads the table's numbers from the image
//! ([`EXPORTED_ABI`]), and the SDK's allocations with a placement come here
//! ([`alloc_in`]).

use core::alloc::Layout;

use paperpaw_native::Api;

/// The revisions of the families of shared functions this firmware has,
/// and the oldest whose plugins it still runs: the RISC-V chips (X4, X3)
/// share, the X4 Pro's plugins carry their own copies.
#[cfg(feature = "esp32c3")]
pub(crate) const SHARED_REVISIONS: paperpaw_table::Revisions = paperpaw_table::REVISIONS;
#[cfg(feature = "esp32c3")]
pub(crate) const SHARED_OLDEST: paperpaw_table::Revisions = paperpaw_table::OLDEST;
#[cfg(feature = "esp32s3")]
pub(crate) const SHARED_REVISIONS: paperpaw_table::Revisions = [0; paperpaw_table::FAMILIES];
#[cfg(feature = "esp32s3")]
pub(crate) const SHARED_OLDEST: paperpaw_table::Revisions = [0; paperpaw_table::FAMILIES];

/// What the release writes into its `-abi.txt` beside the fingerprint
/// (`paperpaw-sign abi`), under a name of its own in the ELF file: the
/// record's version (1), the plugin interface, the size of that table, then
/// the revision of each family and the oldest it still runs (eight words
/// each). The store and the web flasher read it to refuse a plugin that
/// needs more of the table than the firmware has.
#[used]
#[unsafe(export_name = "PAPERPAW_NATIVE_ABI")]
static EXPORTED_ABI: [u32; 3 + 2 * paperpaw_table::FAMILIES] = {
    let mut words = [0; 3 + 2 * paperpaw_table::FAMILIES];
    words[0] = 1;
    words[1] = paperpaw_sdk::native::INTERFACE as u32;
    words[2] = Api::SIZE;
    let mut i = 0;
    while i < paperpaw_table::FAMILIES {
        words[3 + i] = SHARED_REVISIONS[i] as u32;
        words[3 + paperpaw_table::FAMILIES + i] = SHARED_OLDEST[i] as u32;
        i += 1;
    }
    words
};

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
