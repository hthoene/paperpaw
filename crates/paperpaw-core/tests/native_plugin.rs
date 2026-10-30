//! A native plugin built in a workspace of its own, loaded into a kernel
//! of this one (ADR 0024): its types, vtables and closures cross as they
//! are, its heap is the kernel's, and a service it publishes is found
//! though its build gives the SDK's types other `TypeId`s.
//!
//! The plugin (`native-plugin/`) is a library the system loads, as on
//! Android; on the X4 and the X4 Pro the same code is a `.ppn` file the
//! firmware places. Build it first, then run this test:
//!
//! ```sh
//! cargo build --release --manifest-path crates/paperpaw-core/tests/native-plugin/Cargo.toml
//! cargo test -p paperpaw-core --test native_plugin -- --ignored
//! ```

use std::alloc::Layout;
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Instant as Clock;

use paperpaw_core::Kernel;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::native::support::{Api, ApiSlot, Entry};
use paperpaw_sdk::native::{FINGERPRINT, take};
use paperpaw_sdk::{Env, Id, Manifest, Params, Plugin, RegisterError, Registrar, Version};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test",
    name: "Test device",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Gray4,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: true,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Large,
};

unsafe extern "C" fn alloc(size: usize, align: usize) -> *mut u8 {
    match Layout::from_size_align(size, align) {
        // SAFETY: not zero-sized, as `GlobalAlloc::alloc`.
        Ok(layout) if size > 0 => unsafe { std::alloc::alloc(layout) },
        _ => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn dealloc(ptr: *mut u8, size: usize, align: usize) {
    if let Ok(layout) = Layout::from_size_align(size, align) {
        // SAFETY: given by `alloc` or `realloc` with this layout.
        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

unsafe extern "C" fn realloc(ptr: *mut u8, size: usize, align: usize, new_size: usize) -> *mut u8 {
    match Layout::from_size_align(size, align) {
        // SAFETY: given by `alloc` or `realloc` with this layout.
        Ok(layout) if new_size > 0 => unsafe { std::alloc::realloc(ptr, layout, new_size) },
        _ => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn panic(message: *const u8, len: usize) -> ! {
    // SAFETY: the plugin's message, `len` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(message, len) };
    panic!("the plugin panicked: {}", String::from_utf8_lossy(bytes));
}

extern "C" fn micros() -> u64 {
    static START: OnceLock<Clock> = OnceLock::new();
    START.get_or_init(Clock::now).elapsed().as_micros() as u64
}

unsafe extern "C" fn icon(name: *const u8, len: usize) -> u32 {
    // SAFETY: the plugin's text, `len` bytes, read while this answers.
    let bytes = unsafe { std::slice::from_raw_parts(name, len) };
    std::str::from_utf8(bytes)
        .ok()
        .and_then(paperpaw_sdk::icons::char)
        .map_or(0, u32::from)
}

/// The shell's table, as a firmware's.
static API: Api = Api {
    size: Api::SIZE,
    alloc,
    dealloc,
    realloc,
    panic,
    micros,
    icon,
    alloc_in,
    shared: paperpaw_sdk::shared::table(),
    shared_len: paperpaw_sdk::shared::COUNT,
};

/// One heap here: wherever asked, as `alloc`.
unsafe extern "C" fn alloc_in(size: usize, align: usize, _placement: u32) -> *mut u8 {
    // SAFETY: as `alloc`.
    unsafe { alloc(size, align) }
}

/// The same text calls as the plugin makes: with this build's own copies.
#[path = "native-plugin/src/probe.rs"]
mod probe;

/// The plugin library, built as the module says (`PAPERPAW_TEST_PLUGIN`
/// names another build of it: the CI job `table-compat` builds it in
/// table mode, with `--cfg paperpaw_table_host`).
fn library() -> PathBuf {
    if let Some(path) = std::env::var_os("PAPERPAW_TEST_PLUGIN") {
        return PathBuf::from(path);
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/native-plugin/target/release");
    let name = format!(
        "{}paperpaw_test_native_plugin{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    dir.join(name)
}

/// A plugin the shell loaded, given to the kernel as any other.
struct Loaded(Box<dyn Plugin>);

impl Plugin for Loaded {
    fn manifest(&self) -> Manifest {
        self.0.manifest()
    }

    fn register(&self, registrar: &mut Registrar) -> Result<(), RegisterError> {
        self.0.register(registrar)
    }
}

/// A plugin of this build that looks for the loaded one's service by
/// name.
struct Looks(Rc<Cell<Option<Version>>>, Rc<Cell<Option<bool>>>);

impl Plugin for Looks {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("looks"), "Looks", Version::new(1, 0, 0))
            .description("")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let found = self.0.clone();
        let text = self.1.clone();
        r.on(
            events::STARTED,
            move |env: &Env<'_>, _, _: &mut paperpaw_sdk::Commands| {
                let services = env.services();
                found.set(
                    services
                        .get::<HelloVersion>("hello.version", Version::new(1, 0, 0))
                        .map(|v| v.0),
                );
                // What the plugin's text calls came to, against the same
                // calls here (through the table or on its own copies).
                text.set(
                    services
                        .get::<probe::TextProbe>("hello.text-probe", Version::new(1, 0, 0))
                        .map(|got| *got == probe::probe()),
                );
            },
        )
    }
}

#[test]
#[ignore = "needs the plugin built apart first: see the module's documentation"]
fn a_plugin_built_apart_runs_in_the_kernel() {
    let path = library();
    // SAFETY: the test's own plugin, built from this repository.
    let library = unsafe { libloading::Library::new(&path) }
        .unwrap_or_else(|e| panic!("{}: {e} (build it first, see the module)", path.display()));
    // SAFETY: the statics `plugin!` exports, of these types.
    let (slot, entry) = unsafe {
        let slot = library.get::<*const ApiSlot>(b"PAPERPAW_API\0").unwrap();
        let entry = library.get::<*const Entry>(b"PAPERPAW_ENTRY\0").unwrap();
        (&**slot, &**entry)
    };
    assert_eq!(
        entry.fingerprint, FINGERPRINT.0,
        "built by another compiler or SDK"
    );
    slot.set(&API);
    // SAFETY: a plugin with this build's fingerprint, made once.
    let plugin = unsafe { take((entry.make)()) }.unwrap();
    assert_eq!(plugin.manifest().id.as_str(), "hello");

    let found = Rc::new(Cell::new(None));
    let text = Rc::new(Cell::new(None));
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Loaded(plugin))
        .plugin(Looks(found.clone(), text.clone()))
        .build()
        .unwrap();
    assert!(kernel.take_warnings().is_empty());
    kernel.start(Instant(0));
    assert_eq!(
        found.get(),
        Some(Version::new(1, 2, 3)),
        "its service, by name"
    );

    assert_eq!(
        text.get(),
        Some(true),
        "its text calls come out as the kernel's own"
    );

    // Its action opens its screen, drawn by its code; its translations
    // are the kernel's.
    kernel.run_action(Instant(10), Id::new_static("hello.open"));
    assert_eq!(
        kernel.active_screen(),
        Some(&Id::new_static("hello.screen"))
    );
    kernel.send_event(
        Instant(20),
        Id::new_static("core.book-opened"),
        Params::new(),
    );
    assert_eq!(kernel.translator().get("hello.title"), Some("Hello"));
    // Its plugin and everything it registered are dropped with the
    // kernel, by the plugin's code; the library stays loaded until then.
    drop(kernel);
    drop(library);
}

/// The loaded plugin's version service, as it names it: the same key,
/// another build.
#[repr(C)]
struct HelloVersion(Version);
paperpaw_sdk::service_type!(HelloVersion, "hello.version/1");
