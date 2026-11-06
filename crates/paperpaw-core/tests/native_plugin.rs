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
//!
//! A library built apart (`PAPERPAW_TEST_PLUGIN`, as `tools/table-compat.sh`
//! does) says in `PAPERPAW_TEST_TABLES` whether it was built in table mode
//! (1) or with its own copies (0); the test checks it is so. One built
//! for an older plugin interface or an older draft of this one says so in
//! `PAPERPAW_TEST_EXPECT=refused`, with the interface it was built for in
//! `PAPERPAW_TEST_BUILT_FOR`: then it must be refused, before anything of
//! it runs, as the shells refuse it.

use std::alloc::Layout;
use std::cell::{Cell, RefCell};
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
use paperpaw_sdk::native::{Chip, FINGERPRINT, Fingerprint, Header, INTERFACE, Native, take};
use paperpaw_sdk::{
    Env, Id, Manifest, Params, Plugin, PluginProblem, RegisterError, Registrar, Version,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, true, false, &[]),
    MemoryClass::Large,
);

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
    // The functions this shell shares, as the X4's firmware does: a
    // library built in table mode (`--cfg paperpaw_table_host`) calls them.
    family_revs: paperpaw_table::REVISIONS,
    family_min: paperpaw_table::OLDEST,
    build: BUILD.as_ptr(),
    build_len: BUILD.len() as u32,
};

/// What the logs call this build.
const BUILD: &str = concat!("PaperPaw ", env!("CARGO_PKG_VERSION"));

/// One heap here: wherever asked, as `alloc`.
unsafe extern "C" fn alloc_in(size: usize, align: usize, _placement: u32) -> *mut u8 {
    // SAFETY: as `alloc`.
    unsafe { alloc(size, align) }
}

/// The same calls as the plugin makes: with this build's own copies.
#[path = "native-plugin/src/probe.rs"]
mod probe;
#[path = "native-plugin/src/ui_probe.rs"]
mod ui_probe;

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
/// name; it brings the interface's font, which the ui calls draw with.
struct Looks(
    Rc<Cell<Option<Version>>>,
    Rc<Cell<Option<bool>>>,
    Rc<Cell<Option<bool>>>,
    Rc<RefCell<Option<Result<(), String>>>>,
);

impl Plugin for Looks {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("looks"), "Looks", Version::new(1, 0, 0))
            .description("")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let found = self.0.clone();
        let text = self.1.clone();
        let families = self.2.clone();
        let ui = self.3.clone();
        let font = paperpaw_text::Font::from_data(probe::PACK, &[]).map_err(|_| {
            RegisterError::Invalid {
                message: "the test pack".into(),
            }
        })?;
        r.add_font(paperpaw_sdk::FontSpec::new(
            Id::new_static("looks.font"),
            font,
        ))?;
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
                // And its calls of the later families (ui, data, graphics).
                families.set(
                    services
                        .get::<probe::FamilyProbe>("hello.family-probe", Version::new(1, 0, 0))
                        .map(|got| *got == probe::family_probe()),
                );
                // And its ui calls, run with this kernel's `Env`: every
                // entry of the ui family, and text drawn right to left.
                let here = ui_probe::ui_calls(env);
                *ui.borrow_mut() = services
                    .get::<ui_probe::UiProbe>("hello.ui-probe", Version::new(1, 0, 0))
                    .map(|probe| differences(&(probe.run)(env), &here));
            },
        )
    }
}

/// Where the plugin's ui calls came out otherwise than this tree's.
fn differences(got: &ui_probe::UiCalls, here: &ui_probe::UiCalls) -> Result<(), String> {
    // Drawings that left no ink would agree without showing anything.
    let white = here.gray.iter().filter(|&&gray| gray == 0).count();
    assert!(
        here.numbers.iter().all(|n| n != "no font") && white == 0,
        "the ui calls draw with a font: {here:?}"
    );
    if got == here {
        return Ok(());
    }
    let numbers = got.numbers.iter().zip(&here.numbers).enumerate();
    let mut said: Vec<String> = numbers
        .filter(|(_, (a, b))| a != b)
        .map(|(n, (a, b))| format!("number {n}: the plugin's {a:?}, here {b:?}"))
        .collect();
    let gray = got.gray.iter().zip(&here.gray).enumerate();
    said.extend(
        gray.filter(|(_, (a, b))| a != b)
            .map(|(n, (a, b))| format!("drawing {n}: the plugin's ink {a}, here {b}")),
    );
    if got.numbers.len() != here.numbers.len() || got.gray.len() != here.gray.len() {
        said.push(String::from("not as many calls"));
    }
    Err(said.join("\n"))
}

/// A plugin built for an older plugin interface, or an older draft of
/// this one (`PAPERPAW_TEST_BUILT_FOR`: the interface), is refused, and
/// nothing of it runs: not its entry's other fields, nor its `make`.
fn refused(fingerprint: Fingerprint) {
    let built_for: u16 = match std::env::var("PAPERPAW_TEST_BUILT_FOR").map(|n| n.parse()) {
        Ok(Ok(n)) => n,
        other => {
            panic!("PAPERPAW_TEST_BUILT_FOR: the interface the plugin was built for, not {other:?}")
        }
    };
    assert!(
        built_for <= INTERFACE,
        "a plugin of a later interface ({built_for}) is no old plugin"
    );
    assert_eq!(
        fingerprint.interface(),
        built_for,
        "its fingerprint names its interface"
    );
    // The emulator's and Android's loader compare the fingerprint alone,
    // the X4's asks why it differs: both refuse it.
    assert_ne!(fingerprint, FINGERPRINT, "an older build, refused");
    assert!(fingerprint.misfit().is_some());
    // And installing or placing its file: of an older interface it waits
    // for a build for this one, of an older draft it is of another build.
    let header = Header {
        chip: Chip::Esp32S3,
        fingerprint: fingerprint.0,
        base: 0,
        image_len: 0,
        code_at: 0,
        entry: 0,
        api: 0,
        relocs: 0,
        align: 0,
        compiler: 0,
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        needs_api: 0,
        families: [0; 8],
        interface: built_for,
        sha256: [0; 32],
    };
    let expected = if built_for < INTERFACE {
        PluginProblem::OlderInterface { built_for }
    } else {
        PluginProblem::OtherBuild
    };
    assert_eq!(Native::new("code.ppn").problem(&header), Some(expected));
}

/// The library's entry, where it is to run (`PAPERPAW_TEST_EXPECT` runs,
/// or not set); `None` where it is to be refused, and was.
fn expected_entry(_library: &libloading::Library, entry: *const Entry) -> Option<&Entry> {
    // The fingerprint first, through a pointer, as the shells read it: an
    // entry of another interface may be shorter than this one's `Entry`.
    // SAFETY: the static `plugin!` exports, which every interface's entry
    // begins with its fingerprint, aligned to 8.
    let fingerprint = unsafe { Entry::fingerprint_at(entry) };
    match std::env::var("PAPERPAW_TEST_EXPECT").as_deref() {
        Ok("refused") => {
            refused(Fingerprint(fingerprint));
            return None;
        }
        Ok("runs") | Err(std::env::VarError::NotPresent) => {}
        other => panic!("PAPERPAW_TEST_EXPECT is runs or refused, not {other:?}"),
    }
    assert_eq!(
        fingerprint, FINGERPRINT.0,
        "built by another compiler or SDK"
    );
    // SAFETY: an entry of this build's interface, as its fingerprint says,
    // in `_library`, loaded as long as the borrow.
    Some(unsafe { &*entry })
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
        (&**slot, *entry)
    };
    let Some(entry) = expected_entry(&library, entry) else {
        return;
    };
    // What the plugin calls of the shared functions fits this shell's.
    assert_eq!(
        paperpaw_table::misfit(&entry.families, &API.family_min, &API.family_revs),
        None
    );
    // A library built in table mode keeps its tables here, which the
    // loader of a chip writes into the plugin's words. Which mode it was
    // built in, the run says (`PAPERPAW_TEST_TABLES`: 1 table mode, 0 its
    // own copies): a table-mode library without them would be answered by
    // its own copies, and the probe would pass without using a table.
    // SAFETY: the static `paperpaw_table` exports in table mode, of this
    // type; not there in a library with its own copies.
    let tables = unsafe { library.get::<*const paperpaw_table::HostTables>(b"PAPERPAW_TABLES\0") };
    // (The library built by the steps above has its own copies.)
    let mode = std::env::var("PAPERPAW_TEST_TABLES").ok().or_else(|| {
        std::env::var_os("PAPERPAW_TEST_PLUGIN")
            .is_none()
            .then(|| "0".into())
    });
    match mode.as_deref() {
        Some("1") => assert!(
            tables.is_ok(),
            "built in table mode, but no PAPERPAW_TABLES"
        ),
        Some("0") => assert!(
            tables.is_err(),
            "built with its own copies, but has PAPERPAW_TABLES"
        ),
        other => panic!("PAPERPAW_TEST_TABLES is 1 or 0 (table mode or not), not {other:?}"),
    }
    if let Ok(tables) = tables {
        for n in 0..paperpaw_table::FAMILIES {
            if let Some(family) = paperpaw_table::Family::of(n as u8) {
                // SAFETY: as above.
                unsafe { &**tables }.set(family, paperpaw_sdk::shared::table(family));
            }
        }
    }
    slot.set(&API);
    // SAFETY: a plugin with this build's fingerprint, made once.
    let plugin = unsafe { take((entry.make)()) }.unwrap();
    assert_eq!(plugin.manifest().id.as_str(), "hello");

    let found = Rc::new(Cell::new(None));
    let text = Rc::new(Cell::new(None));
    let families = Rc::new(Cell::new(None));
    let ui = Rc::new(RefCell::new(None));
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Loaded(plugin))
        .plugin(Looks(
            found.clone(),
            text.clone(),
            families.clone(),
            ui.clone(),
        ))
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
    // A plugin built before the probe of the later families (an older
    // baseline of tools/table-compat.sh) has none: where it has one, it
    // agrees; this tree's own plugin has one.
    if std::env::var_os("PAPERPAW_TEST_PLUGIN").is_none() {
        assert_eq!(
            families.get(),
            Some(true),
            "its ui, data and graphics calls come out as the kernel's own"
        );
    } else {
        assert_ne!(
            families.get(),
            Some(false),
            "its ui, data and graphics calls come out as the kernel's own"
        );
    }

    // Its ui calls: tools/table-compat.sh builds this tree's probe against
    // the baseline's crates, so every plugin has them.
    match ui.borrow().as_ref() {
        Some(Ok(())) => {}
        Some(Err(said)) => panic!("its ui calls come out otherwise than the kernel's own:\n{said}"),
        None => panic!("no ui probe in the plugin"),
    }

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
