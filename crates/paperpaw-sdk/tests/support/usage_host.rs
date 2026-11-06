//! Deterministic interface-7 usage host for SDK and Battery port tests. No kernel.
extern crate std;
use self::std::{
    cell::{Cell, RefCell},
    string::String,
    vec::Vec,
};
use core::{alloc::Layout, ffi::c_void};
use paperpaw_native::host_glue::*;

pub const USAGE: &[u8] = include_bytes!("../fixtures/abi7-services/usage.bin");
pub const PAGE0: &[u8] = include_bytes!("../fixtures/abi7-services/page0.bin");
pub const PAGE1: &[u8] = include_bytes!("../fixtures/abi7-services/page1.bin");
pub const EMPTY: &[u8] = include_bytes!("../fixtures/abi7-services/empty.bin");
pub const NAME: &[u8] = include_bytes!("../fixtures/abi7-services/name.bin");
pub const NOTE: &[u8] = include_bytes!("../fixtures/abi7-services/note.bin");

#[derive(Debug)]
pub struct UsageHost {
    pub responses: Vec<(String, Vec<u8>)>,
    pub queries: RefCell<Vec<String>>,
    pub releases: Cell<u32>,
    pub max_read: Cell<usize>,
    pub query_error: Cell<i32>,
    pub read_error: Cell<i32>,
    pub release_error: Cell<i32>,
    pub invalid_count: Cell<bool>,
    pub active: Cell<u32>,
    serial: Cell<u32>,
    data: RefCell<Vec<u8>>,
}
impl Default for UsageHost {
    fn default() -> Self {
        Self {
            responses: [
                ("core.usage", USAGE),
                ("core.plugin-usage/from.0", PAGE0),
                ("core.plugin-usage/from.1", PAGE1),
                ("core.plugin/battery", NAME),
                ("core.work-note/battery", NOTE),
            ]
            .into_iter()
            .map(|(k, b)| (k.into(), b.into()))
            .collect(),
            queries: RefCell::new(Vec::new()),
            releases: Cell::new(0),
            max_read: Cell::new(384),
            query_error: Cell::new(OK),
            read_error: Cell::new(OK),
            release_error: Cell::new(OK),
            invalid_count: Cell::new(false),
            active: Cell::new(0),
            serial: Cell::new(0),
            data: RefCell::new(Vec::new()),
        }
    }
}

/// A fake table consuming a live UsageHost as ctx; retain both through callbacks.
pub fn table() -> PpHost {
    let mut t = host_table();
    t.services_query = query;
    t.blob_read = read;
    t.blob_release = release;
    t.heap_alloc = allocate;
    t.heap_dealloc = deallocate;
    t.clock_micros = clock;
    t
}
unsafe extern "C" fn query(
    ctx: *mut c_void,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    // SAFETY: fixture users retain UsageHost and supply disjoint borrowed key/output.
    unsafe {
        let host = &*ctx.cast::<UsageHost>();
        let Ok(key) = core::str::from_utf8(core::slice::from_raw_parts(key.ptr, key.len)) else {
            return ERR_UTF8;
        };
        host.queries.borrow_mut().push(key.into());
        if host.query_error.get() != OK {
            return host.query_error.get();
        }
        if major != 1 || minor > 0 {
            return ERR_UNSUPPORTED;
        }
        if host.active.get() != 0 {
            return ERR_BUSY;
        }
        let Some((_, data)) = host.responses.iter().find(|(k, _)| k == key) else {
            return ERR_NOT_FOUND;
        };
        host.data.borrow_mut().clone_from(data);
        host.serial.set(host.serial.get() + 1);
        host.active.set(host.serial.get());
        out.write(host.active.get());
        OK
    }
}
unsafe extern "C" fn read(ctx: *mut c_void, handle: u32, offset: u64, out: PpBuffer) -> i32 {
    // SAFETY: fixture users retain UsageHost and exclusively borrow output.
    unsafe {
        let host = &*ctx.cast::<UsageHost>();
        if handle == 0 || host.active.get() != handle {
            return ERR_STATE;
        }
        if host.read_error.get() != OK {
            return host.read_error.get();
        }
        if host.invalid_count.get() {
            return i32::try_from(out.cap + 1).unwrap();
        }
        let data = host.data.borrow();
        let Some(tail) = usize::try_from(offset).ok().and_then(|i| data.get(i..)) else {
            return 0;
        };
        let n = tail.len().min(out.cap).min(host.max_read.get());
        core::ptr::copy_nonoverlapping(tail.as_ptr(), out.ptr, n);
        i32::try_from(n).unwrap()
    }
}
unsafe extern "C" fn release(ctx: *mut c_void, handle: u32) -> i32 {
    // SAFETY: fixture users retain UsageHost through this call.
    unsafe {
        let host = &*ctx.cast::<UsageHost>();
        host.releases.set(host.releases.get() + 1);
        if host.active.get() != handle || handle == 0 {
            return ERR_STATE;
        }
        host.active.set(0);
        host.release_error.get()
    }
}
unsafe extern "C" fn allocate(
    _: *mut c_void,
    size: usize,
    align: usize,
    out: *mut *mut c_void,
) -> i32 {
    // SAFETY: entry glue supplies a valid layout and aligned writable output.
    unsafe {
        out.write(self::std::alloc::alloc(Layout::from_size_align(size, align).unwrap()).cast());
    }
    OK
}
unsafe extern "C" fn deallocate(
    _: *mut c_void,
    block: *mut c_void,
    size: usize,
    align: usize,
) -> i32 {
    // SAFETY: entry destruction supplies its live block and original layout once.
    unsafe {
        self::std::alloc::dealloc(block.cast(), Layout::from_size_align(size, align).unwrap());
    }
    OK
}
unsafe extern "C" fn clock(_: *mut c_void, out: *mut u64) -> i32 {
    // SAFETY: clock wrapper supplies aligned call-local output.
    unsafe {
        out.write(123_456);
    }
    OK
}
