//! Implementation details for `native_plugin!`. Not an application API.
use super::{Error, Host, Plugin, status};
use core::{ffi::c_void, mem, ptr};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED, MAX_ALIGN, OK};
use paperpaw_native::host_glue::{MAJOR, MINOR, PpBuffer, PpBytes, PpEntry, PpHost, PpPlugin};

/// Immutable entry export; metadata and function addresses live with the module.
#[repr(transparent)]
pub struct Entry(PpEntry);
impl Entry {
    /// Construct the macro's export, checking that its derived minor matches
    /// the validated metadata. Invalid records return None without panicking.
    #[must_use]
    pub const fn new<T: Plugin, const NEEDS_MINOR: u16>(metadata: &'static [u8]) -> Option<Self> {
        match metadata_minor(metadata) {
            Some(minor) if minor == NEEDS_MINOR => Some(Self(PpEntry {
                size: mem::size_of::<PpEntry>() as u32,
                abi_major: MAJOR,
                needs_minor: minor,
                metadata: PpBytes {
                    ptr: metadata.as_ptr(),
                    len: metadata.len(),
                },
                make: Some(make::<T, NEEDS_MINOR>),
            })),
            _ => None,
        }
    }
}

/// Validate the generated layout-2 prefix for the export macro. The metadata's
/// declared minor, not the SDK's current minor, controls host admission.
#[must_use]
pub const fn metadata_minor(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 32 {
        return None;
    }
    if bytes[0] != b'P'
        || bytes[1] != b'P'
        || bytes[2] != b'A'
        || bytes[3] != b'M'
        || u16::from_le_bytes([bytes[4], bytes[5]]) != 2
        || u16::from_le_bytes([bytes[6], bytes[7]]) != 32
        || u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize != bytes.len()
        || u16::from_le_bytes([bytes[12], bytes[13]]) != MAJOR
    {
        return None;
    }
    let minor = u16::from_le_bytes([bytes[14], bytes[15]]);
    if minor > MINOR { None } else { Some(minor) }
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
pub(super) unsafe extern "C" fn make<T: Plugin, const NEEDS_MINOR: u16>(
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
    if table.major != MAJOR || table.minor < NEEDS_MINOR {
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
                input: Some(input::<T>),
            },
            value,
        });
        out.write(&raw const (*state).plugin);
    }
    OK
}

/// # Safety
/// The host enforces reentry deny: no callback re-enters this same instance.
unsafe extern "C" fn event<T: Plugin>(
    state: *mut c_void,
    token: u32,
    input: PpBytes,
    out: PpBuffer,
) -> i32 {
    if state.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: host lends disjoint input/output for this callback only.
    let input = match unsafe {
        paperpaw_native::host_glue::buffers::bytes_in(input, isize::MAX as usize)
    } {
        Ok(input) => input,
        Err(e) => return e,
    };
    // SAFETY: host output is writable and does not overlap state or input.
    let out = match unsafe {
        paperpaw_native::host_glue::buffers::buffer_out(out, isize::MAX as usize)
    } {
        Ok(out) => out,
        Err(e) => return e,
    };
    let event = match super::super::events::Event::decode(input) {
        Ok(event) => event,
        Err(e) => return e.status(),
    };
    let mut reply = super::super::reply::Reply::new(out);
    // SAFETY: host retains and exclusively admits this live instance.
    let state = unsafe { &mut *state.cast::<State<T>>() };
    let host = Host {
        // SAFETY: make retained the table lease until destroy.
        table: unsafe { &*state.host },
        ctx: state.ctx,
    };
    match state.value.event(&host, token, event, &mut reply) {
        Ok(()) => i32::try_from(reply.len()).unwrap_or(ERR_INVALID),
        Err(e) => e.status(),
    }
}

/// # Safety
/// Host exclusively admits the live state and lends readable input bytes.
unsafe extern "C" fn draw<T: Plugin>(state: *mut c_void, token: u32, input: PpBytes) -> i32 {
    if state.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: admitted input is readable and disjoint from plugin state.
    let bytes = match unsafe {
        paperpaw_native::host_glue::buffers::bytes_in(input, isize::MAX as usize)
    } {
        Ok(bytes) => bytes,
        Err(e) => return e,
    };
    let frame = match super::super::callbacks::DrawFrame::decode(bytes) {
        Ok(frame) => frame,
        Err(e) => return e.status(),
    };
    if matches!(
        frame.purpose,
        super::super::callbacks::DrawPurpose::Other(_)
    ) {
        return OK;
    }
    // SAFETY: host retains and exclusively admits this live instance.
    let state = unsafe { &mut *state.cast::<State<T>>() };
    let host = Host {
        // SAFETY: make retained the table lease until destroy.
        table: unsafe { &*state.host },
        ctx: state.ctx,
    };
    state
        .value
        .draw(&host, token, frame.bind(&host))
        .map_or_else(Error::status, |()| OK)
}

/// # Safety
/// Host exclusively admits the live state and lends readable input bytes.
unsafe extern "C" fn input<T: Plugin>(state: *mut c_void, token: u32, input: PpBytes) -> i32 {
    if state.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: admitted input is readable and disjoint from plugin state.
    let bytes = match unsafe {
        paperpaw_native::host_glue::buffers::bytes_in(input, isize::MAX as usize)
    } {
        Ok(bytes) => bytes,
        Err(e) => return e,
    };
    let input = match super::super::callbacks::Input::decode(bytes) {
        Ok(input) => input,
        Err(e) => return e.status(),
    };
    // SAFETY: host retains and exclusively admits this live instance.
    let state = unsafe { &mut *state.cast::<State<T>>() };
    let host = Host {
        // SAFETY: make retained the table lease until destroy.
        table: unsafe { &*state.host },
        ctx: state.ctx,
    };
    state
        .value
        .input(&host, token, input)
        .map_or_else(Error::status, |handled| match handled {
            crate::Handled::Yes => 1,
            crate::Handled::No => 0,
        })
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

/// Export a flat interface-7 plugin with generated metadata bytes. The macro
/// emits both required symbols in the platform linker sections. A malformed
/// prefix or metadata requiring a newer SDK minor fails at compile time.
///
/// It also gives the plugin its heap (`abi/heap-word.md`): the read-only heap
/// word `PAPERPAW_HEAP` the loader fills before `make`, a global allocator over
/// the host's context-free heap table (each allocation charged to the plugin),
/// and a panic handler that reports to the host and allocates nothing. Outside
/// tests only, so a plugin's own unit tests keep the test harness's. A plugin
/// that brings its own allocator and panic handler, or allocates nothing,
/// writes `heap = none`: then neither the word nor those two are emitted.
///
/// ```compile_fail
/// use paperpaw_sdk::native::{Plugin, Host, Error};
/// struct P;
/// impl Plugin for P {
///     fn make(_: &Host<'_>) -> Result<Self, Error> { Err(Error::Unsupported) }
/// }
/// // A different ABI major cannot be exported.
/// paperpaw_sdk::native_plugin!(P, metadata = b"PPAM\x02\0\x20\0\x20\0\0\0\x08\0\x01\0\x01\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0");
/// ```
/// ```compile_fail
/// use paperpaw_sdk::native::{Plugin, Host, Error};
/// struct P;
/// impl Plugin for P {
///     fn make(_: &Host<'_>) -> Result<Self, Error> { Err(Error::Unsupported) }
/// }
/// // A minor beyond this SDK cannot be exported.
/// paperpaw_sdk::native_plugin!(P, metadata = b"PPAM\x02\0\x20\0\x20\0\0\0\x07\0\xff\xff\x01\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0");
/// ```
#[macro_export]
macro_rules! native_plugin {
    ($ty:ty, metadata = $metadata:expr, heap = none) => {
        $crate::native_plugin!(@entry $ty, $metadata);
    };
    ($ty:ty, metadata = $metadata:expr) => {
        $crate::native_plugin!(@entry $ty, $metadata);
        $crate::native_plugin!(@heap);
    };
    (@entry $ty:ty, $metadata:expr) => {
        #[used]
        #[unsafe(no_mangle)]
        #[unsafe(link_section = ".rodata.paperpaw_metadata")]
        pub static PAPERPAW_METADATA: [u8; ($metadata).len()] = {
            let source: &[u8] = $metadata;
            let mut bytes = [0; ($metadata).len()];
            let mut i = 0;
            while i < source.len() {
                bytes[i] = source[i];
                i += 1;
            }
            bytes
        };
        #[used]
        #[unsafe(no_mangle)]
        #[unsafe(link_section = ".rodata.paperpaw_entry")]
        pub static PAPERPAW_ENTRY: $crate::native::__entry::Entry = {
            const MINOR: u16 = match $crate::native::__entry::metadata_minor($metadata) {
                Some(minor) => minor,
                None => panic!("invalid interface-7 metadata prefix"),
            };
            match $crate::native::__entry::Entry::new::<$ty, MINOR>(&PAPERPAW_METADATA) {
                Some(entry) => entry,
                None => panic!("invalid interface-7 entry metadata"),
            }
        };
    };
    (@heap) => {
        // The heap word: zero in the file, written by the loader before
        // `make`, never by the plugin. Defined in assembly so that Rust never
        // sees a value to fold; code reads only the hidden alias.
        #[cfg(not(any(target_os = "none", target_os = "linux", target_os = "android")))]
        ::core::compile_error!("the heap word needs an ELF target; use `heap = none`");
        ::core::arch::global_asm!(
            ".pushsection .rodata.paperpaw_heap,\"a\",@progbits",
            ".globl PAPERPAW_HEAP",
            ".type PAPERPAW_HEAP,@object",
            ".size PAPERPAW_HEAP,{size}",
            ".globl paperpaw_heap_local",
            ".hidden paperpaw_heap_local",
            ".balign {size}",
            "PAPERPAW_HEAP:",
            "paperpaw_heap_local:",
            ".zero {size}",
            ".popsection",
            size = const ::core::mem::size_of::<usize>(),
        );

        /// The heap word's value: the host's heap table, or null.
        #[cfg(not(test))]
        fn paperpaw_heap() -> *const $crate::native::__heap::PpHeap {
            unsafe extern "C" {
                static paperpaw_heap_local: *const $crate::native::__heap::PpHeap;
            }
            // SAFETY: the word defined above; the loader writes it before
            // `make` and it never changes while the plugin runs.
            unsafe { paperpaw_heap_local }
        }

        /// The global allocator over the host's heap table.
        #[cfg(not(test))]
        struct PaperpawHeap;

        // SAFETY: every call forwards to the host's allocator, which keeps
        // `GlobalAlloc`'s contract (`abi/heap-word.md` section 3).
        #[cfg(not(test))]
        unsafe impl ::core::alloc::GlobalAlloc for PaperpawHeap {
            unsafe fn alloc(&self, layout: ::core::alloc::Layout) -> *mut u8 {
                // SAFETY: the caller's layout and the heap word's table.
                unsafe { $crate::native::__heap::alloc(paperpaw_heap(), layout) }
            }

            unsafe fn dealloc(&self, block: *mut u8, layout: ::core::alloc::Layout) {
                // SAFETY: a block of this allocator with its layout.
                unsafe { $crate::native::__heap::dealloc(paperpaw_heap(), block, layout) }
            }

            unsafe fn realloc(
                &self,
                block: *mut u8,
                layout: ::core::alloc::Layout,
                new_size: usize,
            ) -> *mut u8 {
                // SAFETY: a block of this allocator with its layout.
                unsafe {
                    $crate::native::__heap::realloc(paperpaw_heap(), block, layout, new_size)
                }
            }
        }

        #[cfg(not(test))]
        #[global_allocator]
        static PAPERPAW_GLOBAL_HEAP: PaperpawHeap = PaperpawHeap;

        #[cfg(not(test))]
        #[panic_handler]
        fn paperpaw_panic(info: &::core::panic::PanicInfo<'_>) -> ! {
            // SAFETY: the heap word's value.
            unsafe { $crate::native::__heap::panic(paperpaw_heap(), info) }
        }
    };
}
