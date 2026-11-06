//! Ratchet for the packaged entry records used by the native loader.
//! C/Rust layout parity is checked separately by `test_abi_layout.py`.

#[allow(
    dead_code,
    reason = "probe includes the complete packaged ABI declarations"
)]
mod raw {
    include!("../src/host_glue/paperpaw_abi.rs");
}

use core::ffi::c_void;
use core::mem::{align_of, offset_of, size_of};
use raw::{PpBuffer, PpBytes, PpEntry, PpHost, PpPlugin};

type Make = unsafe extern "C" fn(*const PpHost, *mut c_void, *mut *const PpPlugin) -> i32;
type Drop = unsafe extern "C" fn(*mut c_void);
type Event = unsafe extern "C" fn(*mut c_void, u32, PpBytes, PpBuffer) -> i32;
type Draw = unsafe extern "C" fn(*mut c_void, u32, PpBytes) -> i32;
type Input = unsafe extern "C" fn(*mut c_void, u32, PpBytes) -> i32;

#[test]
fn bounded_plugin_readers_are_available_to_loaders() {
    use paperpaw_native::host_glue as public;
    type Reader<T> = unsafe fn(*const u8, usize) -> Result<T, i32>;
    type Dispatch =
        unsafe extern "C" fn(*mut c_void, u32, public::PpBytes, public::PpBuffer) -> i32;
    type PublicDraw = unsafe extern "C" fn(*mut c_void, u32, public::PpBytes) -> i32;
    let _: Reader<usize> = public::read_plugin_size;
    let _: Reader<*mut c_void> = public::read_plugin_state;
    let _: Reader<Drop> = public::read_plugin_drop;
    let _: Reader<Dispatch> = public::read_plugin_dispatch_event;
    let _: Reader<PublicDraw> = public::read_plugin_draw;
    assert_eq!(public::PLUGIN_PREFIX_BYTES, raw::PLUGIN_PREFIX_BYTES);
}

#[test]
fn entry_and_plugin_fields_remain_exhaustive_and_typed() {
    let entry = PpEntry {
        size: size_of::<PpEntry>() as u32,
        abi_major: raw::MAJOR,
        needs_minor: raw::MINOR,
        metadata: PpBytes {
            ptr: core::ptr::null(),
            len: 0,
        },
        make: None,
    };
    let PpEntry {
        size,
        abi_major,
        needs_minor,
        metadata,
        make,
    } = entry;
    let _: (u32, u16, u16, PpBytes, Option<Make>) = (size, abi_major, needs_minor, metadata, make);
    let plugin = PpPlugin {
        size: size_of::<PpPlugin>() as u32,
        state: core::ptr::null_mut(),
        drop: None,
        dispatch_event: None,
        draw: None,
        input: None,
    };
    let PpPlugin {
        size,
        state,
        drop,
        dispatch_event,
        draw,
        input,
    } = plugin;
    let _: (u32, *mut c_void, Option<Drop>) = (size, state, drop);
    let _: (Option<Event>, Option<Draw>, Option<Input>) = (dispatch_event, draw, input);
}

#[test]
fn entry_and_lifecycle_layouts_keep_the_reviewed_offsets() {
    let pointer = size_of::<*const c_void>();
    assert!(matches!(pointer, 4 | 8));
    assert_eq!(align_of::<PpEntry>(), pointer);
    assert_eq!(size_of::<PpEntry>(), 8 + 3 * pointer);
    assert_eq!(offset_of!(PpEntry, size), 0);
    assert_eq!(offset_of!(PpEntry, abi_major), 4);
    assert_eq!(offset_of!(PpEntry, needs_minor), 6);
    assert_eq!(offset_of!(PpEntry, metadata), 8);
    assert_eq!(offset_of!(PpEntry, make), 8 + 2 * pointer);
    assert_eq!(align_of::<PpPlugin>(), pointer);
    assert_eq!(size_of::<PpPlugin>(), 5 * pointer);
    assert_eq!(raw::PLUGIN_PREFIX_BYTES, 3 * pointer);
    assert_eq!(offset_of!(PpPlugin, size), 0);
    assert_eq!(offset_of!(PpPlugin, state), pointer);
    assert_eq!(offset_of!(PpPlugin, drop), 2 * pointer);
    assert_eq!(offset_of!(PpPlugin, dispatch_event), 3 * pointer);
    assert_eq!(offset_of!(PpPlugin, draw), 4 * pointer);
    assert_eq!(size_of::<Option<Make>>(), pointer);
    assert_eq!(size_of::<Option<Drop>>(), pointer);
}
