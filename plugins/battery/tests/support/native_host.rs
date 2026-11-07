//! Test-only native host. Raw ABI hooks are confined to this fixture.
//!
//! `Harness` owns boxed table/context allocations until it drops the native
//! instance. Calls are synchronous, never reentrant, and lend disjoint buffers.
//! Snapshot/settings bytes come from the public host environment encoder;
//! service bytes come from the public generated host record encoder.

use std::{
    alloc::Layout,
    cell::{Cell, RefCell},
    collections::BTreeMap,
    ffi::c_void,
    ptr,
    rc::Rc,
};

use paperpaw_core::{
    Kernel,
    native_host::{
        environment::{EnvironmentScope, SNAPSHOT_BYTES},
        registrations::{Record, ServiceEnd, ServiceUsage, record_encode_one},
    },
};
use paperpaw_native::host_glue::*;
use paperpaw_platform::{
    Battery, DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
    SystemStatus, WallClock,
};
use paperpaw_sdk::{ActionSpec, Id, Manifest, Plugin, RegisterError, Registrar, SettingSpec};

pub const LOG: &str = "log.csv";
pub const OLD: &str = "log.old.csv";
pub const ENABLED: &str = "battery.enabled";
const CAPTURE: Id = Id::new_static("battery.fixture-capture");
const DEVICE: DeviceInfo = DeviceInfo::new(
    "battery-native-fixture",
    "Battery native fixture",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

#[derive(Clone, Debug)]
pub struct EnvironmentBytes {
    snapshot: Vec<u8>,
    enabled: Vec<u8>,
}

/// Creates wire data through public host operations, without a Battery plugin.
/// The legacy fixture plugin supplies an Env only; every recorder runs natively.
pub fn environment(
    at: u64,
    battery: Option<Battery>,
    unix: Option<i64>,
    enabled: bool,
) -> EnvironmentBytes {
    let result = Rc::new(RefCell::new(None));
    let mut kernel = Kernel::builder(DEVICE)
        .user_config(format!(
            "[battery]\nenabled = {}\n",
            if enabled { "on" } else { "off" }
        ))
        .plugin(Capture(Rc::clone(&result)))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let clock = unix.map(|seconds| WallClock::new(Instant(at), seconds * 1000, Some(0)));
    kernel.set_status(Instant(at), SystemStatus::new(clock, battery));
    kernel.run_action(Instant(at), CAPTURE);
    result.borrow_mut().take().expect("capture action ran")
}

struct Capture(Rc<RefCell<Option<EnvironmentBytes>>>);
impl Plugin for Capture {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("battery"),
            "Wire fixture",
            paperpaw_sdk::Version::new(1, 0, 0),
        )
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_setting(SettingSpec::switch(
            ENABLED,
            "battery.fixture-enabled",
            true,
            Id::new_static("power.group"),
        ))?;
        let result = Rc::clone(&self.0);
        r.add_action(ActionSpec::new(CAPTURE, "battery.fixture-capture").handler(
            move |env, _, _| {
                let scope = Box::pin(EnvironmentScope::new());
                scope
                    .as_ref()
                    .scope(env, || {
                        let mut snapshot = vec![0; SNAPSHOT_BYTES];
                        let mut enabled = vec![0; 32];
                        // SAFETY: the pinned scope borrows this live Env. Outputs are
                        // exclusive local vectors, disjoint from the static key.
                        unsafe {
                            assert_eq!(scope.copy(snapshot.as_mut_ptr(), snapshot.len()), OK);
                            let length = scope.settings_read(
                                ENABLED.as_ptr(),
                                ENABLED.len(),
                                enabled.as_mut_ptr(),
                                enabled.len(),
                            );
                            assert!(
                                length > 0 && usize::try_from(length).unwrap() <= enabled.len()
                            );
                            enabled.truncate(usize::try_from(length).unwrap());
                        }
                        *result.borrow_mut() = Some(EnvironmentBytes { snapshot, enabled });
                    })
                    .unwrap();
            },
        ))
    }
}

pub fn record(record: Record<'_>) -> Vec<u8> {
    let mut bytes = vec![0; 512];
    let length = record_encode_one(&record, &mut bytes).unwrap();
    bytes.truncate(length);
    bytes
}

pub fn usage(full: u32, pixels: u64) -> ServiceUsage {
    ServiceUsage {
        fast: 0,
        partial: 0,
        full,
        pixels,
        inputs: 0,
        sleeps: 0,
        panel_ms: 0,
        bluetooth_ms: 0,
    }
}

#[derive(Debug)]
pub struct Context {
    pub environment: RefCell<EnvironmentBytes>,
    pub files: RefCell<BTreeMap<String, Vec<u8>>>,
    pub services: RefCell<BTreeMap<String, Vec<u8>>>,
    pub queries: RefCell<Vec<String>>,
    pub operations: RefCell<Vec<String>>,
    pub syncs: Cell<u32>,
    pub allocated: Cell<u32>,
    pub freed: Cell<u32>,
    pub active_blob: Cell<u32>,
    pub blobs_created: Cell<u32>,
    pub blobs_released: Cell<u32>,
    pub max_read: Cell<usize>,
    pub snapshot_error: Cell<i32>,
    pub settings_error: Cell<i32>,
    pub query_error: Cell<i32>,
    pub append_error: Cell<i32>,
    pub sync_error: Cell<i32>,
    blob: RefCell<Vec<u8>>,
}

impl Context {
    pub fn new() -> Self {
        let empty = record(Record::ServiceEnd(ServiceEnd {
            flags: 0,
            total: 0,
            generation: 1,
            next: 0,
        }));
        Self {
            environment: RefCell::new(environment(0, None, None, true)),
            files: RefCell::new(BTreeMap::new()),
            services: RefCell::new(BTreeMap::from([
                (
                    "core.usage".into(),
                    record(Record::ServiceUsage(usage(0, 0))),
                ),
                ("core.plugin-usage/from.0".into(), empty),
            ])),
            queries: RefCell::new(Vec::new()),
            operations: RefCell::new(Vec::new()),
            syncs: Cell::new(0),
            allocated: Cell::new(0),
            freed: Cell::new(0),
            active_blob: Cell::new(0),
            blobs_created: Cell::new(0),
            blobs_released: Cell::new(0),
            max_read: Cell::new(384),
            snapshot_error: Cell::new(OK),
            settings_error: Cell::new(OK),
            query_error: Cell::new(OK),
            append_error: Cell::new(OK),
            sync_error: Cell::new(OK),
            blob: RefCell::new(Vec::new()),
        }
    }

    pub fn seed(&self, path: &str, bytes: impl AsRef<[u8]>) {
        self.files
            .borrow_mut()
            .insert(path.into(), bytes.as_ref().to_vec());
    }

    pub fn text(&self, path: &str) -> String {
        String::from_utf8(self.files.borrow().get(path).expect("file exists").clone()).unwrap()
    }

    pub fn assert_blobs_released(&self) {
        assert_eq!(self.active_blob.get(), 0);
        assert_eq!(self.blobs_created.get(), self.blobs_released.get());
    }
}

/// Owns the two stable allocations that entry glue retains until destruction.
pub struct Harness {
    pub context: Box<Context>,
    _table: Box<PpHost>,
    plugin: *const PpPlugin,
}

impl Harness {
    pub fn new(entry: &PpEntry) -> Self {
        let context = Box::new(Context::new());
        let table = Box::new(table());
        let mut plugin = ptr::null();
        // SAFETY: both boxes remain at stable addresses until Drop releases the
        // instance. Output is a disjoint local pointer; make is called once.
        assert_eq!(
            unsafe {
                entry.make.unwrap()(
                    &*table,
                    ptr::from_ref(&*context).cast_mut().cast(),
                    &mut plugin,
                )
            },
            OK
        );
        assert!(!plugin.is_null());
        assert_eq!(context.allocated.get(), 1);
        Self {
            context,
            _table: table,
            plugin,
        }
    }

    pub fn dispatch(&mut self, input: &[u8]) -> i32 {
        // SAFETY: this owner exclusively admits the live instance. Input is
        // borrowed only for this call; the empty output aliases nothing.
        let status = unsafe {
            let plugin = &*self.plugin;
            plugin.dispatch_event.unwrap()(
                plugin.state,
                42,
                PpBytes {
                    ptr: input.as_ptr(),
                    len: input.len(),
                },
                PpBuffer {
                    ptr: ptr::null_mut(),
                    cap: 0,
                },
            )
        };
        self.context.assert_blobs_released();
        status
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // SAFETY: no callback is active. The matching table/context and allocator
        // still live, and the sole owner destroys this initialized state once.
        unsafe {
            let plugin = &*self.plugin;
            plugin.drop.unwrap()(plugin.state);
        }
        assert_eq!(self.context.allocated.get(), 1);
        assert_eq!(self.context.freed.get(), 1);
        self.context.assert_blobs_released();
    }
}

fn table() -> PpHost {
    let mut table = host_table();
    table.heap_alloc = allocate;
    table.heap_dealloc = deallocate;
    table.env_snapshot = snapshot;
    table.settings_read = setting;
    table.services_query = query;
    table.blob_read = blob_read;
    table.blob_release = blob_release;
    table.storage_size = size;
    table.storage_read_at = read_at;
    table.storage_append = append;
    table.storage_rename = rename;
    table.storage_remove = remove;
    table.storage_sync = sync;
    table
}

/// # Safety
/// `ctx` is this Harness's live boxed Context throughout synchronous callbacks.
unsafe fn context<'a>(ctx: *mut c_void) -> &'a Context {
    // SAFETY: every installed test hook receives the owner's unchanged pointer.
    unsafe { &*ctx.cast::<Context>() }
}

/// # Safety
/// Input describes initialized, borrowed SDK bytes, unchanged for this call.
unsafe fn input<'a>(bytes: PpBytes) -> &'a [u8] {
    // SAFETY: the native typed SDK lends valid input storage for each hook.
    unsafe { std::slice::from_raw_parts(bytes.ptr, bytes.len) }
}

/// # Safety
/// Output is writable/exclusive and disjoint from source and host state.
unsafe fn copy(bytes: &[u8], out: PpBuffer) -> usize {
    let count = bytes.len().min(out.cap);
    if count != 0 {
        // SAFETY: caller supplies disjoint buffers with at least count bytes.
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), out.ptr, count) };
    }
    count
}

unsafe extern "C" fn allocate(
    ctx: *mut c_void,
    size: usize,
    align: usize,
    out: *mut *mut c_void,
) -> i32 {
    // SAFETY: entry supplies a valid layout and exclusive output; ctx is retained.
    unsafe {
        let context = context(ctx);
        let block = std::alloc::alloc(Layout::from_size_align(size, align).unwrap());
        if block.is_null() {
            return ERR_NO_MEMORY;
        }
        out.write(block.cast());
        context.allocated.set(context.allocated.get() + 1);
    }
    OK
}

unsafe extern "C" fn deallocate(
    ctx: *mut c_void,
    block: *mut c_void,
    size: usize,
    align: usize,
) -> i32 {
    // SAFETY: entry returns the one live allocation with its original layout.
    unsafe {
        std::alloc::dealloc(block.cast(), Layout::from_size_align(size, align).unwrap());
        let context = context(ctx);
        context.freed.set(context.freed.get() + 1);
    }
    OK
}

unsafe extern "C" fn snapshot(ctx: *mut c_void, out: PpBuffer) -> i32 {
    // SAFETY: ctx is retained and out is disjoint exclusive callback storage.
    unsafe {
        let context = context(ctx);
        if context.snapshot_error.get() != OK {
            return context.snapshot_error.get();
        }
        let environment = context.environment.borrow();
        if out.cap < environment.snapshot.len() {
            return ERR_NO_SPACE;
        }
        assert_eq!(copy(&environment.snapshot, out), SNAPSHOT_BYTES);
    }
    OK
}

unsafe extern "C" fn setting(ctx: *mut c_void, key: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: ctx/key live for this call; output is exclusive and disjoint.
    unsafe {
        let context = context(ctx);
        if context.settings_error.get() != OK {
            return context.settings_error.get();
        }
        if input(key) != ENABLED.as_bytes() {
            return ERR_NOT_FOUND;
        }
        let environment = context.environment.borrow();
        copy(&environment.enabled, out);
        i32::try_from(environment.enabled.len()).unwrap()
    }
}

unsafe extern "C" fn query(
    ctx: *mut c_void,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    // SAFETY: retained ctx, borrowed SDK key and disjoint aligned output.
    unsafe {
        let context = context(ctx);
        let key = std::str::from_utf8(input(key)).unwrap();
        context.queries.borrow_mut().push(key.into());
        if context.query_error.get() != OK {
            return context.query_error.get();
        }
        if major != 1 || minor != 0 {
            return ERR_UNSUPPORTED;
        }
        if context.active_blob.get() != 0 {
            return ERR_BUSY;
        }
        let services = context.services.borrow();
        let Some(bytes) = services.get(key) else {
            return ERR_NOT_FOUND;
        };
        context.blob.borrow_mut().clone_from(bytes);
        let handle = context.blobs_created.get() + 1;
        context.blobs_created.set(handle);
        context.active_blob.set(handle);
        out.write(handle);
    }
    OK
}

unsafe extern "C" fn blob_read(ctx: *mut c_void, handle: u32, offset: u64, out: PpBuffer) -> i32 {
    // SAFETY: retained ctx and exclusively borrowed SDK output.
    unsafe {
        let context = context(ctx);
        if handle == 0 || context.active_blob.get() != handle {
            return ERR_STATE;
        }
        let bytes = context.blob.borrow();
        let offset = usize::try_from(offset).unwrap();
        let bytes = bytes.get(offset..).unwrap_or_default();
        let count = bytes.len().min(out.cap).min(context.max_read.get());
        i32::try_from(copy(&bytes[..count], out)).unwrap()
    }
}

unsafe extern "C" fn blob_release(ctx: *mut c_void, handle: u32) -> i32 {
    // SAFETY: ctx lives until all callbacks and releases have completed.
    let context = unsafe { context(ctx) };
    if handle == 0 || context.active_blob.get() != handle {
        return ERR_STATE;
    }
    context.active_blob.set(0);
    context.blobs_released.set(context.blobs_released.get() + 1);
    OK
}

unsafe extern "C" fn size(ctx: *mut c_void, root: u32, path: PpBytes, out: *mut u64) -> i32 {
    assert_eq!(root, 0, "recorder storage belongs to the plugin data root");
    // SAFETY: retained ctx/path and disjoint aligned writable output.
    unsafe {
        let context = context(ctx);
        let path = std::str::from_utf8(input(path)).unwrap();
        let files = context.files.borrow();
        let Some(bytes) = files.get(path) else {
            return ERR_NOT_FOUND;
        };
        out.write(u64::try_from(bytes.len()).unwrap());
    }
    OK
}

unsafe extern "C" fn read_at(
    ctx: *mut c_void,
    root: u32,
    path: PpBytes,
    offset: u64,
    out: PpBuffer,
) -> i32 {
    assert_eq!(root, 0);
    // SAFETY: retained ctx/path and disjoint exclusively borrowed output.
    unsafe {
        let context = context(ctx);
        let path = std::str::from_utf8(input(path)).unwrap();
        let files = context.files.borrow();
        let Some(bytes) = files.get(path) else {
            return ERR_NOT_FOUND;
        };
        let tail = bytes
            .get(usize::try_from(offset).unwrap()..)
            .unwrap_or_default();
        i32::try_from(copy(tail, out)).unwrap()
    }
}

unsafe extern "C" fn append(ctx: *mut c_void, root: u32, path: PpBytes, data: PpBytes) -> i32 {
    assert_eq!(root, 0);
    assert!(data.len <= paperpaw_sdk::native::storage::MAX_DATA);
    // SAFETY: retained ctx; both borrowed SDK inputs are initialized and live.
    unsafe {
        let context = context(ctx);
        let path = std::str::from_utf8(input(path)).unwrap();
        context
            .operations
            .borrow_mut()
            .push(format!("append:{path}"));
        if context.append_error.get() != OK {
            return context.append_error.get();
        }
        context
            .files
            .borrow_mut()
            .entry(path.into())
            .or_default()
            .extend_from_slice(input(data));
    }
    OK
}

unsafe extern "C" fn rename(ctx: *mut c_void, root: u32, from: PpBytes, to: PpBytes) -> i32 {
    assert_eq!(root, 0);
    // SAFETY: retained ctx; both path buffers are borrowed for this call.
    unsafe {
        let context = context(ctx);
        let from = std::str::from_utf8(input(from)).unwrap();
        let to = std::str::from_utf8(input(to)).unwrap();
        context
            .operations
            .borrow_mut()
            .push(format!("rename:{from}:{to}"));
        let mut files = context.files.borrow_mut();
        if files.contains_key(to) {
            return ERR_IO;
        }
        let Some(bytes) = files.remove(from) else {
            return ERR_NOT_FOUND;
        };
        files.insert(to.into(), bytes);
    }
    OK
}

unsafe extern "C" fn remove(ctx: *mut c_void, root: u32, path: PpBytes) -> i32 {
    assert_eq!(root, 0);
    // SAFETY: retained ctx and path borrowed for this call only.
    unsafe {
        let context = context(ctx);
        let path = std::str::from_utf8(input(path)).unwrap();
        context
            .operations
            .borrow_mut()
            .push(format!("remove:{path}"));
        if context.files.borrow_mut().remove(path).is_none() {
            return ERR_NOT_FOUND;
        }
    }
    OK
}

unsafe extern "C" fn sync(ctx: *mut c_void) -> i32 {
    // SAFETY: the harness retains its boxed context through this callback.
    let context = unsafe { context(ctx) };
    context.operations.borrow_mut().push("sync".into());
    context.syncs.set(context.syncs.get() + 1);
    context.sync_error.get()
}
