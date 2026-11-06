//! Implementation details for `native_plugin!`. Not an application API.
use super::{Error, Host, Plugin, status};
use core::{ffi::c_void, mem, ptr};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED, MAX_ALIGN, OK};
use paperpaw_native::host_glue::{MAJOR, MINOR, PpBytes, PpEntry, PpHost, PpPlugin};

/// Immutable entry export; metadata and function addresses live with the module.
#[repr(transparent)]
pub struct Entry(PpEntry);
impl Entry {
    /// Create an immutable export with the plugin's static metadata bytes.
    #[must_use]
    pub const fn new<T: Plugin>(metadata: &'static [u8]) -> Self {
        Self(PpEntry {
            size: mem::size_of::<PpEntry>() as u32,
            abi_major: MAJOR,
            needs_minor: MINOR,
            metadata: PpBytes {
                ptr: metadata.as_ptr(),
                len: metadata.len(),
            },
            make: Some(make::<T>),
        })
    }
}
// SAFETY: macro exports point only to immutable static metadata and code.
unsafe impl Sync for Entry {}
impl core::fmt::Debug for Entry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Entry").finish_non_exhaustive()
    }
}

struct State<T> {
    host: *const PpHost,
    ctx: *mut c_void,
    plugin: PpPlugin,
    value: T,
}

/// Construct the plugin behind an entry export.
///
/// # Safety
/// A non-null host exposes its size header and the claimed bytes, aligned for
/// PpHost. Context and table outlive destruction. Output is writable, aligned
/// and disjoint from them. Host allocation obeys the allocator contract.
pub(super) unsafe extern "C" fn make<T: Plugin>(
    host: *const PpHost,
    ctx: *mut c_void,
    out: *mut *const PpPlugin,
) -> i32 {
    if out.is_null() || host.is_null() || ctx.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: the host supplies at least a readable size field.
    if (unsafe { ptr::read_unaligned(host.cast::<u32>()) } as usize) < mem::size_of::<PpHost>() {
        return ERR_UNSUPPORTED;
    }
    // SAFETY: size was checked before forming a reference to the full table.
    let table = unsafe { &*host };
    if table.major != MAJOR || table.minor < MINOR {
        return ERR_UNSUPPORTED;
    }
    if mem::align_of::<State<T>>() > MAX_ALIGN {
        return ERR_INVALID;
    }
    let mut block = ptr::null_mut();
    // SAFETY: call-local output; layout is nonzero and bounded by MAX_ALIGN.
    if let Err(e) = status(unsafe {
        (table.heap_alloc)(
            ctx,
            mem::size_of::<State<T>>(),
            mem::align_of::<State<T>>(),
            &mut block,
        )
    }) {
        return e.status();
    }
    if block.is_null() {
        return ERR_NO_MEMORY;
    }
    let binding = Host { table, ctx };
    let value = match T::make(&binding) {
        Ok(value) => value,
        Err(e) => {
            // SAFETY: the block is uninitialized and was allocated above.
            unsafe { free::<T>(host, ctx, block) };
            return e.status();
        }
    };
    let state = block.cast::<State<T>>();
    // SAFETY: allocator provides sufficient aligned exclusive storage; output is
    // published only after all fields and the plugin value have been initialized.
    unsafe {
        state.write(State {
            host,
            ctx,
            plugin: PpPlugin {
                size: mem::size_of::<PpPlugin>() as u32,
                state: block,
                drop: Some(destroy::<T>),
                dispatch_event: Some(event::<T>),
                draw: Some(draw::<T>),
            },
            value,
        });
        out.write(&raw const (*state).plugin);
    }
    OK
}

unsafe extern "C" fn event<T: Plugin>(state: *mut c_void, token: u32, input: PpBytes) -> i32 {
    if state.is_null() || input.len > 4096 || (input.len != 0 && input.ptr.is_null()) {
        return ERR_INVALID;
    }
    let input = if input.len == 0 {
        &[]
    } else {
        // SAFETY: admitted host input is readable and disjoint from plugin state.
        unsafe { core::slice::from_raw_parts(input.ptr, input.len) }
    };
    // SAFETY: the host retains and exclusively admits this live instance.
    let state = unsafe { &mut *state.cast::<State<T>>() };
    // SAFETY: make retained the table lease until destroy.
    let host = Host {
        table: unsafe { &*state.host },
        ctx: state.ctx,
    };
    state
        .value
        .event(&host, token, input)
        .map_or_else(Error::status, |()| OK)
}

unsafe extern "C" fn draw<T: Plugin>(state: *mut c_void, token: u32) -> i32 {
    if state.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: the host retains and exclusively admits this live instance.
    let state = unsafe { &mut *state.cast::<State<T>>() };
    // SAFETY: make retained the table lease until destroy.
    let host = Host {
        table: unsafe { &*state.host },
        ctx: state.ctx,
    };
    state
        .value
        .draw(&host, token)
        .map_or_else(Error::status, |()| OK)
}

unsafe extern "C" fn destroy<T: Plugin>(state: *mut c_void) {
    if state.is_null() {
        return;
    }
    let state = state.cast::<State<T>>();
    // SAFETY: the host calls destroy exactly once after all callbacks drain.
    let (host, ctx) = unsafe { ((*state).host, (*state).ctx) };
    // SAFETY: state is initialized; drop the plugin before returning its storage.
    unsafe {
        ptr::drop_in_place(state);
        free::<T>(host, ctx, state.cast());
    }
}

unsafe fn free<T>(host: *const PpHost, ctx: *mut c_void, block: *mut c_void) {
    // Deallocation has no recovery path; a conforming allocator accepts its own
    // live block with the original layout and returns OK.
    // SAFETY: same host/context/block/layout as allocation, used exactly once.
    unsafe {
        ((*host).heap_dealloc)(
            ctx,
            block,
            mem::size_of::<State<T>>(),
            mem::align_of::<State<T>>(),
        );
    }
}

/// Export a flat interface-7 plugin with immutable generated metadata.
#[macro_export]
macro_rules! native_plugin {
    ($ty:ty, metadata = $metadata:expr) => {
        #[unsafe(no_mangle)]
        pub static PAPERPAW_ENTRY: $crate::native::__entry::Entry =
            $crate::native::__entry::Entry::new::<$ty>($metadata);
    };
}
