//! Code plugins in the emulator (ADR 0024): a plugin's crate built for
//! this computer (`cargo build --release`, a `cdylib` with `plugin!`) is
//! `code-host.dll`, `code-host.so` or `code-host.dylib` in its folder of
//! the emulated card (`plugins/<id>/`), beside its `plugin.ini`; the emulator loads it
//! with the same entry and table as `.ppn` files on the X4 (Android loads no
//! code plugins). It must be built by the same compiler, for this computer,
//! against the same SDK as this emulator ([`FINGERPRINT`], which
//! `--stdio` says; `built_here` checks the compiler), and installed: only a file whose SHA-256 the card
//! has (`native::remember_code`) runs, as on the devices, which
//! [`trust`] gives every one on a card (`--trust-code`).
//!
//! Each file is copied before it is loaded, so a new build can replace
//! it while the emulator runs; it is loaded anew at the next start (a
//! restart, `reload` over `--stdio`).

use std::alloc::Layout;
use std::sync::OnceLock;
use std::time::Instant as Clock;

use paperpaw_core::sdk::native::support::{Api, ApiSlot, Entry};
use paperpaw_core::sdk::native::{FINGERPRINT, Native, take};
use paperpaw_core::sdk::{Plugin, PluginProblem};
use paperpaw_core::{CodeFile, CodeLoader};
use paperpaw_platform::{MemoryClass, Storage};

/// The name of a code plugin's file built for this computer.
pub fn file_name() -> String {
    format!("code-host{}", std::env::consts::DLL_SUFFIX)
}

/// How the emulator loads code plugins: files named [`file_name`], fit
/// for this build, built by the compiler that built it.
pub fn native() -> Native {
    let mut native = Native::new(file_name());
    native.toolchain = std::borrow::Cow::Borrowed(env!("PAPERPAW_HOST_TOOLCHAIN"));
    native.compiler = paperpaw_core::sdk::native::compiler::id(env!("PAPERPAW_RUSTC"));
    native
}

/// The largest code plugin file read.
const MAX_FILE: usize = 256 << 20;

/// The folder this process copies code plugins into before it loads
/// them: its own, made at the first ([`crate::private::dir`]).
fn copies() -> std::io::Result<&'static std::path::Path> {
    static COPIES: OnceLock<std::path::PathBuf> = OnceLock::new();
    if let Some(dir) = COPIES.get() {
        return Ok(dir);
    }
    let dir = crate::private::dir("paperpaw-code")?;
    Ok(COPIES.get_or_init(|| dir))
}

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
    panic!("a code plugin panicked: {}", String::from_utf8_lossy(bytes));
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
        .and_then(paperpaw_core::sdk::icons::char)
        .map_or(0, u32::from)
}

/// The emulator's table, as a firmware's: the computer's heap (one for
/// both sides), its clock.
static API: Api = Api {
    size: Api::SIZE,
    alloc,
    dealloc,
    realloc,
    panic,
    micros,
    icon,
    alloc_in,
    // a computer shares no functions: plugins keep their copies.
    family_revs: [0; 8],
    family_min: [0; 8],
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

/// Loads code plugins built for this computer.
///
/// An interface-7 library (only with the developer switch on) is hosted over
/// its pinned owner and admitted by the kernel
/// ([`CodeLoader::load_native`]); every such plugin of this loader shares one
/// [`DispatchDepth`](paperpaw_core::native_host::DispatchDepth), the device-wide nesting budget.
#[derive(Debug)]
pub struct HostLoader {
    /// The table of handles and the dispatch budget this device has, kept
    /// here for the linked plugins registered with the same kernel too.
    pool: DevicePool,
    resources: Option<paperpaw_core::native_host::HostResources>,
    /// The memory class the device this loader runs on declares (the
    /// profile's), which sizes the `core.ui` store.
    memory: MemoryClass,
    /// The file `load_native` read and found not to be interface 7, for the
    /// `load` that follows it for the same plugin (read once, not twice).
    read: Option<(String, [u8; 32], Vec<u8>)>,
}

impl HostLoader {
    /// A loader for a device of the memory class `memory` (a profile's).
    #[must_use]
    pub fn for_device(memory: MemoryClass) -> Self {
        Self::for_device_in(memory, DevicePool::default())
    }

    /// A loader whose plugins share `pool` with every other plugin of the
    /// same kernel (the linked ones): a kernel keeps one pool of resources
    /// and one dispatch budget, and refuses a plugin that brings another.
    #[must_use]
    pub fn for_device_in(memory: MemoryClass, pool: DevicePool) -> Self {
        Self {
            pool,
            resources: None,
            memory,
            read: None,
        }
    }
}

/// The table of handles and the dispatch budget of one kernel's interface-7
/// plugins, made at the first that asks (a card plugin's loader or the linked
/// plugins' registration) and shared by every later one.
#[derive(Clone, Debug, Default)]
pub struct DevicePool(std::rc::Rc<std::cell::RefCell<PoolParts>>);

#[derive(Debug, Default)]
struct PoolParts {
    resources: Option<paperpaw_core::native_host::HostResources>,
    /// The memory class `core.ui` was wired for: later asks name the same.
    memory: Option<MemoryClass>,
    /// What card plugins are offered (set by the linked registration, which
    /// knows what is wired); none until then.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    offer: Option<crate::native_runtime::Offer>,
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    depth: Option<paperpaw_core::native_host::flat::FlatDepth>,
}

impl DevicePool {
    /// A handle on the table of handles, `core.ui` wired for a device of
    /// `memory` when it was made.
    pub(crate) fn resources(
        &self,
        memory: MemoryClass,
    ) -> Option<paperpaw_core::native_host::HostResources> {
        let mut parts = self.0.borrow_mut();
        if parts.resources.is_none() {
            parts.resources = new_resources(memory);
            parts.memory = Some(memory);
        }
        debug_assert_eq!(
            parts.memory,
            Some(memory),
            "one kernel, one memory class: the table was made for another"
        );
        parts
            .resources
            .as_ref()
            .and_then(paperpaw_core::native_host::HostResources::try_clone)
    }

    /// What card plugins are offered, once the registration has said.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub(crate) fn offer(&self) -> Option<crate::native_runtime::Offer> {
        self.0.borrow().offer
    }

    /// The registration says what is wired: the services of the linked
    /// plugins and, of the features, those a card plugin may reach.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub(crate) fn set_offer(&self, offer: crate::native_runtime::Offer) {
        self.0.borrow_mut().offer = Some(offer);
    }

    /// A handle on the one device-wide dispatch budget.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub(crate) fn depth(&self) -> Option<paperpaw_core::native_host::flat::FlatDepth> {
        use paperpaw_core::native_host::flat::FlatDepth;
        let mut parts = self.0.borrow_mut();
        if parts.depth.is_none() {
            parts.depth = Some(FlatDepth::try_new().ok()?);
        }
        parts.depth.as_ref().and_then(FlatDepth::try_clone)
    }
}

/// The table of handles the interface-7 plugins' owners share, with `core.ui`
/// wired at once (before the first plugin loads, so that a host call made
/// while one loads or declares finds it); `None` and a message where it has no
/// room.
fn new_resources(memory: MemoryClass) -> Option<paperpaw_core::native_host::HostResources> {
    use paperpaw_core::native_host::{HostResources, HostWiring};
    let Ok(resources) = HostResources::try_new() else {
        eprintln!("native: no room for the host's table of handles");
        return None;
    };
    match resources.wire(HostWiring::new().ui(memory)) {
        Ok(wired) if wired.ui_skipped() => {
            eprintln!("native: core.ui not wired (no memory for its store)");
        }
        Ok(_) => {}
        Err(status) => eprintln!("native: core.ui not wired ({status})"),
    }
    Some(resources)
}

impl CodeLoader for HostLoader {
    fn load(
        &mut self,
        storage: &dyn Storage,
        code: &CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        let installed = Installed::of(code);
        // Not the device's heap: its code lies in flash (or the PSRAM it
        // runs from), never on the heap a plugin's budget counts.
        crate::budget::untracked(|| {
            let bytes = match self.read.take() {
                Some((path, sha256, bytes))
                    if path == installed.path && sha256 == installed.sha256 =>
                {
                    bytes
                }
                _ => read_installed(storage, &installed)?,
            };
            // An interface-7 library is the kernel's to admit
            // ([`Self::load_native`]), never an interface-6 plugin.
            if dev_switch() && is_interface7(&bytes) {
                return Err(PluginProblem::Unsupported);
            }
            load_bytes(&bytes, &installed)
        })
    }

    /// An interface-7 library (only with the developer switch on) is hosted
    /// for the kernel to admit with everything it declares.
    fn load_native(
        &mut self,
        storage: &dyn Storage,
        code: &CodeFile<'_>,
    ) -> Result<Option<Box<dyn paperpaw_core::NativeAdmission>>, PluginProblem> {
        let installed = Installed::of(code);
        crate::budget::untracked(|| {
            if !dev_switch() {
                return Ok(None);
            }
            let bytes = read_installed(storage, &installed)?;
            if !is_interface7(&bytes) {
                // `load` follows for this file: it is not read again.
                self.read = Some((installed.path.into(), installed.sha256, bytes));
                return Ok(None);
            }
            self.native(&bytes, code).map(Some)
        })
    }

    /// `text.hyphenate` (the data plugins' tables are files on the card) and
    /// `core.ui` (the screen stack's store, sized for the device) are served
    /// where interface-7 plugins run (the developer switch).
    fn hyphenation_sink(
        &mut self,
        storage: &std::rc::Rc<dyn Storage>,
    ) -> Option<paperpaw_core::native_host::HyphenTables> {
        use paperpaw_core::native_host::HostWiring;
        if !dev_switch() {
            return None;
        }
        if self.resources.is_none() {
            self.resources = self.pool.resources(self.memory);
        }
        // `text.hyphenate` is wired only here, because it needs the card's `Rc`.
        // A plugin's `make` that asks for it earlier gets a one-off
        // `ERR_UNSUPPORTED`; plugins use hyphenation only when laying out text,
        // and their allowed features come from the host's fixed list, not from
        // what is wired, so nothing is recorded wrongly.
        // `core.ui` was wired when the table of handles was made (before any
        // plugin loaded); `text.hyphenate` needs the card, which only comes here.
        let wired = match self
            .resources
            .as_ref()?
            .wire(HostWiring::new().hyphenation(storage.clone()))
        {
            Ok(wired) => wired,
            Err(status) => {
                eprintln!("native: text.hyphenate not wired ({status})");
                return None;
            }
        };
        wired.hyphen().cloned()
    }
}

impl HostLoader {
    /// The device-wide dispatch budget, made at the first plugin: every plugin
    /// admitted by this loader gets a clone of this one shared handle, never
    /// a budget of its own (nested dispatch is limited device-wide).
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn shared_depth(
        &mut self,
    ) -> Result<paperpaw_core::native_host::flat::FlatDepth, PluginProblem> {
        self.pool.depth().ok_or(PluginProblem::NoRoom)
    }

    /// The interface-7 library `bytes` of `code`, checked and hosted for the
    /// kernel to admit: nothing of it runs but what the kernel's admission
    /// calls (`make`), and what it declared is registered with it or the
    /// plugin is refused, never loaded without it.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn native(
        &mut self,
        bytes: &[u8],
        code: &CodeFile<'_>,
    ) -> Result<Box<dyn paperpaw_core::NativeAdmission>, PluginProblem> {
        use crate::native_runtime::Refusal;
        let id = code.id.as_str();
        refuse_linked_id(id)?;
        let depth = self.shared_depth()?;
        if self.resources.is_none() {
            self.resources = Some(
                self.pool
                    .resources(self.memory)
                    .ok_or(PluginProblem::NoRoom)?,
            );
        }
        let resources = self
            .resources
            .as_ref()
            .and_then(paperpaw_core::native_host::HostResources::try_clone)
            .ok_or(PluginProblem::NoRoom)?;
        crate::native_runtime::admission(id, bytes, resources, depth, self.pool.offer()).map_err(
            |why| {
                eprintln!("code plugin {id}: {why}");
                match why {
                    Refusal::Allocation => PluginProblem::NoRoom,
                    _ => PluginProblem::Damaged,
                }
            },
        )
    }

    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    fn native(
        &mut self,
        _: &[u8],
        _: &CodeFile<'_>,
    ) -> Result<Box<dyn paperpaw_core::NativeAdmission>, PluginProblem> {
        Err(PluginProblem::Unsupported)
    }
}

/// A card plugin never takes the id of a linked plugin: the id keys secrets
/// and files, a card folder is named by whoever made it, and card plugins are
/// admitted before the linked ones register.
#[cfg_attr(
    not(all(target_os = "linux", target_arch = "x86_64")),
    allow(dead_code, reason = "called from the Linux x86-64 loader only")
)]
fn refuse_linked_id(id: &str) -> Result<(), PluginProblem> {
    if paperpaw_app::is_linked_id(id) {
        eprintln!("code plugin {id}: a plugin built in has this id; not loaded");
        return Err(PluginProblem::Mismatch);
    }
    Ok(())
}

/// Whether `bytes` is an interface-7 library (never where that loader is not
/// built).
fn is_interface7(bytes: &[u8]) -> bool {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return crate::native_runtime::classify(bytes) == crate::native_runtime::Kind::Interface7;
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        let _ = bytes;
        false
    }
}

/// [`load`] with its memory not counted against the budget (the file read
/// here, then loaded as an interface-6 library).
#[cfg(test)]
fn load_installed(
    storage: &dyn Storage,
    code: &Installed<'_>,
) -> Result<Box<dyn Plugin>, PluginProblem> {
    crate::budget::untracked(|| load_bytes(&read_installed(storage, code)?, code))
}

/// What [`load`] needs of a [`CodeFile`]: the plugin and the file installed.
/// The kernel's `host` handle is not part of it (it has no public
/// constructor), so that tests can load a file without a kernel.
#[derive(Debug, Clone, Copy)]
struct Installed<'a> {
    id: &'a paperpaw_core::sdk::Id,
    path: &'a str,
    sha256: [u8; 32],
}

impl<'a> Installed<'a> {
    fn of(code: &CodeFile<'a>) -> Self {
        Self {
            id: code.id,
            path: code.path,
            sha256: code.sha256,
        }
    }
}

/// Whether the interface-7 developer switch is on (never where that loader is
/// not built).
fn dev_switch() -> bool {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return crate::native_runtime::enabled();
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    false
}

/// Whether this (interface-6) loader may take the library: always with the
/// developer switch off, else only one told from its bytes to be interface 6.
pub(crate) fn interface6_only(bytes: &[u8], switch_on: bool) -> bool {
    if !switch_on {
        return true;
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return crate::native_runtime::classify(bytes) == crate::native_runtime::Kind::Interface6;
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        let _ = bytes;
        true
    }
}

/// The installed file, read: only the file installed may run.
fn read_installed(storage: &dyn Storage, code: &Installed<'_>) -> Result<Vec<u8>, PluginProblem> {
    let bytes = paperpaw_core::sdk::read_file(storage, code.path, MAX_FILE)
        .map_err(|_| PluginProblem::Damaged)?;
    if paperpaw_core::sdk::digest::sha256::digest(&bytes) != code.sha256 {
        return Err(PluginProblem::Changed);
    }
    Ok(bytes)
}

/// Loads the interface-6 library `bytes` of the installed file `code`.
fn load_bytes(bytes: &[u8], code: &Installed<'_>) -> Result<Box<dyn Plugin>, PluginProblem> {
    // A copy of its own (the build may change the file meanwhile), in a
    // folder this process made for itself, made anew, then read back and
    // checked again: only what was checked is loaded.
    let copy = copies().map_err(|_| PluginProblem::Damaged)?.join(format!(
        "{}-{}{}",
        code.id.as_str(),
        paperpaw_core::sdk::digest::hex::encode(&code.sha256[..8]),
        std::env::consts::DLL_SUFFIX
    ));
    if !copy.exists() {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&copy)
            .map_err(|_| PluginProblem::Damaged)?;
        std::io::Write::write_all(&mut file, bytes).map_err(|_| PluginProblem::Damaged)?;
        // Written once: read-only from here on.
        let mut permissions = file
            .metadata()
            .map_err(|_| PluginProblem::Damaged)?
            .permissions();
        permissions.set_readonly(true);
        file.set_permissions(permissions)
            .map_err(|_| PluginProblem::Damaged)?;
    }
    // Opened once: what is read, checked and loaded is this one file, not
    // whatever the name leads to at the moment of loading (ADR 0034 section 6).
    let mut copy_file = std::fs::File::open(&copy).map_err(|_| PluginProblem::Damaged)?;
    let mut copied = Vec::new();
    std::io::Read::read_to_end(&mut copy_file, &mut copied).map_err(|_| PluginProblem::Damaged)?;
    if paperpaw_core::sdk::digest::sha256::digest(&copied) != code.sha256 {
        return Err(PluginProblem::Changed);
    }
    // Before anything of it can run: a library's initialisers run while
    // the system loads it, so what it holds is read from its bytes first.
    if let Err(why) = preflight(&copied) {
        eprintln!(
            "code plugin {}: refused before loading ({why:?})",
            code.id.as_str()
        );
        return Err(PluginProblem::Damaged);
    }
    // With the interface-7 developer switch on, this loader takes interface-6
    // libraries only: one that also exports interface 7's metadata (or neither
    // kind) is refused, and an interface-7 library is left to the other loader.
    if !interface6_only(&copied, dev_switch()) {
        eprintln!(
            "code plugin {}: not an interface-6 library (the interface-7 developer switch is on)",
            code.id.as_str()
        );
        return Err(PluginProblem::Damaged);
    }
    if let Err(why) = built_here(&copied) {
        eprintln!("code plugin {}: {why}", code.id.as_str());
        return Err(PluginProblem::OtherBuild);
    }
    // SAFETY: a file the reader installed on the card (`trust`), its
    // SHA-256 the one installed; a code plugin runs as part of
    // PaperPaw, without a sandbox (ADR 0024).
    let held =
        unsafe { HeldLibrary::open(&copy, copy_file) }.map_err(|_| PluginProblem::Damaged)?;
    let library = &held.library;
    // SAFETY: the statics `plugin!` exports, of these types.
    let (slot, entry) = unsafe {
        let slot = library
            .get::<*const ApiSlot>(b"PAPERPAW_API\0")
            .map_err(|_| PluginProblem::Damaged)?;
        let entry = library
            .get::<*const Entry>(b"PAPERPAW_ENTRY\0")
            .map_err(|_| PluginProblem::Damaged)?;
        (&**slot, *entry)
    };
    // The fingerprint first, through a pointer: an entry of another
    // interface may be shorter than this one's `Entry`.
    // SAFETY: the static `plugin!` exports, which every interface's entry
    // begins with its fingerprint, aligned to 8.
    if unsafe { Entry::fingerprint_at(entry) } != FINGERPRINT.0 {
        return Err(PluginProblem::OtherBuild);
    }
    // SAFETY: an entry of this build's interface, as its fingerprint says.
    let entry = unsafe { &*entry };
    // A library built in table mode calls shared functions this shell does
    // not have (it shares none): refused before anything of it runs.
    if let Some(misfit) = paperpaw_table::misfit(&entry.families, &API.family_min, &API.family_revs)
    {
        eprintln!(
            "code plugin {}: calls shared functions this build has not ({misfit:?})",
            code.id.as_str()
        );
        return Err(PluginProblem::NeedsNewerPaperPaw);
    }
    slot.set(&API);
    // SAFETY: a plugin of this build's fingerprint, made once.
    let plugin = unsafe { take((entry.make)()) }.ok_or(PluginProblem::Damaged)?;
    if plugin.manifest().id != *code.id {
        return Err(PluginProblem::Mismatch);
    }
    // Its code stays loaded as long as the emulator runs: what it made
    // is in use. Its file stays open as long: see [`HeldLibrary`].
    held.leak();
    Ok(plugin)
}

/// A loaded library and the open file it was loaded from. The system loader
/// finds a library it already has by the name it was given, and the name
/// `/proc/self/fd/N` is the same for every file that gets the number N: so
/// the file stays open as long as the library is loaded, and a number in use
/// by a loaded library is never given to another.
pub(super) struct HeldLibrary {
    /// First, so it is unloaded before its file is closed.
    pub(super) library: libloading::Library,
    file: std::fs::File,
}

impl HeldLibrary {
    /// Loads the library from `file`, the one that was read and checked.
    ///
    /// # Safety
    ///
    /// Loading a library runs its initialisers: the file passed the
    /// pre-load check and is one the reader installed (ADR 0024).
    pub(super) unsafe fn open(
        path: &std::path::Path,
        file: std::fs::File,
    ) -> Result<Self, libloading::Error> {
        // SAFETY: as documented. Every symbol is bound while loading (`RTLD_NOW`),
        // so nothing is resolved lazily later, and none is shared with the process.
        #[cfg(unix)]
        let library = unsafe {
            libloading::os::unix::Library::open(
                Some(load_path(path, &file)),
                libloading::os::unix::RTLD_NOW | libloading::os::unix::RTLD_LOCAL,
            )
        }?
        .into();
        // SAFETY: as documented.
        #[cfg(not(unix))]
        let library = unsafe { libloading::Library::new(load_path(path, &file)) }?;
        Ok(Self { library, file })
    }

    /// Keeps both for the life of the process.
    fn leak(self) {
        let Self { library, file } = self;
        std::mem::forget(library);
        std::mem::forget(file);
    }
}

/// What the system loader is given to load the checked file: on Linux the
/// open file itself (`/proc/self/fd/N`), so a name that now leads elsewhere
/// changes nothing; elsewhere its path.
#[cfg(target_os = "linux")]
fn load_path(_path: &std::path::Path, file: &std::fs::File) -> std::path::PathBuf {
    use std::os::fd::AsRawFd;
    std::path::PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

/// See the Linux version: the path, until the other platforms hold the file.
#[cfg(not(target_os = "linux"))]
fn load_path(path: &std::path::Path, _file: &std::fs::File) -> std::path::PathBuf {
    path.to_path_buf()
}

/// The preflight of ADR 0034 section 6 for this computer's libraries:
/// refuses every way to run code while the library loads.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn preflight(bytes: &[u8]) -> Result<(), paperpaw_native::preflight::Refusal> {
    let machine = if cfg!(target_arch = "x86_64") {
        62
    } else {
        183
    };
    paperpaw_native::preflight::check_elf64_le(bytes, machine)
}

/// Mach-O and PE libraries are not yet checked before loading (their
/// preflight comes with the generated metadata, ADR 0034 M1), so they are
/// refused. Whoever starts the emulator may say, in the open, that unchecked
/// libraries are loaded anyway: `PAPERPAW_UNCHECKED_CODE_PLUGINS=1`, which
/// is said on standard error for every library it lets through. It never
/// applies where a check exists.
#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
fn preflight(_bytes: &[u8]) -> Result<(), paperpaw_native::preflight::Refusal> {
    if std::env::var_os("PAPERPAW_UNCHECKED_CODE_PLUGINS").is_some_and(|v| v == "1") {
        eprintln!(
            "code plugin loaded without the pre-load check (PAPERPAW_UNCHECKED_CODE_PLUGINS=1)"
        );
        return Ok(());
    }
    Err(paperpaw_native::preflight::Refusal::UnsupportedPlatform)
}

/// Whether the library `bytes` was built by the compiler that built the
/// emulator: Rust types cross as they are until PaperPaw 1.0 (ADR 0026).
/// A library for Linux or Android names the compiler's release (in
/// `.comment`); every library names the commit of its `std` in the paths
/// of its sources (`/rustc/<commit>/library/…`). One that tells neither is
/// refused too.
fn built_here(bytes: &[u8]) -> Result<(), String> {
    let ours = env!("PAPERPAW_RUSTC");
    let releases = paperpaw_core::sdk::native::compiler::named_in(bytes);
    if let Some(other) = releases.iter().find(|r| **r != ours) {
        return Err(format!(
            "built by rustc {other}, the emulator by rustc {ours}"
        ));
    }
    let commits = commits_in(bytes);
    let commit = env!("PAPERPAW_RUSTC_COMMIT");
    if let Some(other) = commits.iter().find(|c| **c != commit) {
        return Err(format!(
            "its std is of the compiler {other}, the emulator's of {commit}"
        ));
    }
    if releases.is_empty() && commits.is_empty() {
        return Err(String::from("it does not tell which compiler built it"));
    }
    Ok(())
}

/// The commits of the compilers whose `std` built into `bytes`, each once.
fn commits_in(bytes: &[u8]) -> Vec<&str> {
    const MARK: &[u8] = b"/rustc/";
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(start) = bytes[at..].windows(MARK.len()).position(|w| w == MARK) {
        let from = at + start + MARK.len();
        let hash = bytes.get(from..from + 41);
        if let Some(hash) = hash
            && hash[40] == b'/'
            && hash[..40].iter().all(u8::is_ascii_hexdigit)
            && let Ok(hash) = std::str::from_utf8(&hash[..40])
            && !found.contains(&hash)
        {
            found.push(hash);
        }
        at = from;
    }
    found
}

/// Installs every code plugin built for this computer on the card in
/// `root` (`plugins/<id>/code-host.*`): the reader's own
/// builds, as an author names a file over USB on the devices. Returns
/// those installed.
pub fn trust(root: &std::path::Path) -> std::io::Result<Vec<String>> {
    let storage = crate::device::library(root)?;
    let plugins = root.join(paperpaw_core::sdk::builtin::PLUGIN_DIR);
    let mut installed = Vec::new();
    let Ok(folders) = std::fs::read_dir(&plugins) else {
        return Ok(installed);
    };
    for folder in folders.flatten() {
        let file = folder.path().join(file_name());
        let Ok(bytes) = std::fs::read(&file) else {
            continue;
        };
        let id = folder.file_name().to_string_lossy().into_owned();
        let sha256 = paperpaw_core::sdk::digest::sha256::digest(&bytes);
        if paperpaw_core::sdk::native::remember_code(&storage, &id, &sha256) {
            installed.push(id);
        }
    }
    installed.sort();
    Ok(installed)
}

#[cfg(all(
    test,
    feature = "native-linked",
    target_os = "linux",
    target_arch = "x86_64"
))]
#[path = "native_card_id_tests.rs"]
mod card_id_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// What a file that is not a library comes to: refused before it is
    /// loaded (where no check exists yet, a library is refused too, unless the
    /// opt-in `PAPERPAW_UNCHECKED_CODE_PLUGINS` is set, which the tests do not).
    fn not_a_library() -> PluginProblem {
        PluginProblem::Damaged
    }

    /// A library is loaded only where it names the emulator's compiler,
    /// by its release or by the commit of its `std`, and none other.
    #[test]
    fn only_libraries_of_this_compiler_are_loaded() {
        let release = format!("rustc version {}\0", env!("PAPERPAW_RUSTC"));
        let std = format!(
            "/rustc/{}/library/core/src/str.rs",
            env!("PAPERPAW_RUSTC_COMMIT")
        );
        assert_eq!(built_here(release.as_bytes()), Ok(()));
        assert_eq!(built_here(std.as_bytes()), Ok(()));
        let both = format!("{release}{std}{std}");
        assert_eq!(built_here(both.as_bytes()), Ok(()));
        let other_release = b"rustc version 1.2.3 (aaaaaaaaa 2020-01-01)\0";
        assert!(built_here(other_release).is_err());
        let other_std = format!("{std}/rustc/{}/library/std/src/io.rs", "a".repeat(40));
        assert!(built_here(other_std.as_bytes()).is_err());
        assert!(built_here(b"no compiler at all").is_err());
    }

    /// A library that has a code plugin installed runs none of it unless
    /// the one who starts the emulator says so (`--trust-code`): a
    /// library copied from elsewhere would otherwise run its code.
    #[test]
    fn code_runs_only_where_the_caller_trusts_it() {
        let _one = crate::one_device_at_a_time();
        let root = crate::private::dir("paperpaw-trust-test").unwrap();
        let hello = root.join("plugins/hello");
        std::fs::create_dir_all(&hello).unwrap();
        std::fs::write(
            hello.join("plugin.ini"),
            "[plugin]
id = hello
name = Hello
version = 0.1.0
",
        )
        .unwrap();
        std::fs::write(hello.join(file_name()), b"not a library").unwrap();
        // Installed, as by `--trust-code` before.
        assert_eq!(trust(&root).unwrap(), ["hello"]);
        let problem = |trusted: bool| {
            crate::device::set_code_plugins(trusted);
            let storage = crate::device::library(&root).unwrap();
            let profile = crate::profiles::find("boox-go-7").unwrap();
            let device = crate::device::Device::new(profile, None, storage, true).unwrap();
            device.kernel.plugins().get("hello").and_then(|p| p.problem)
        };
        assert_eq!(
            problem(false),
            Some(PluginProblem::Unsupported),
            "not loaded"
        );
        assert_eq!(
            problem(true),
            Some(not_a_library()),
            "not a library: refused before it was loaded"
        );
        crate::device::set_code_plugins(false);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A code plugin's file installed from the card: named as installed
    /// with its SHA-256; a folder without one is passed over.
    #[test]
    fn the_readers_own_builds_are_installed() {
        let root = std::env::temp_dir().join(format!("paperpaw-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let hello = root.join("plugins/hello");
        std::fs::create_dir_all(&hello).unwrap();
        std::fs::create_dir_all(root.join("plugins/data-only")).unwrap();
        std::fs::write(hello.join(file_name()), b"not a library").unwrap();
        assert_eq!(trust(&root).unwrap(), ["hello"]);
        let storage = crate::device::library(&root).unwrap();
        assert_eq!(
            paperpaw_core::sdk::native::installed_code(&storage, "hello"),
            Some(paperpaw_core::sdk::digest::sha256::digest(b"not a library"))
        );
        // Installed but not a library the preflight reads: said, not fatal.
        let id = paperpaw_core::sdk::Id::new_static("hello");
        let path = format!("plugins/hello/{}", file_name());
        let code = Installed {
            id: &id,
            path: &path,
            sha256: paperpaw_core::sdk::digest::sha256::digest(b"not a library"),
        };
        assert_eq!(load_installed(&storage, &code).err(), Some(not_a_library()));
        let changed = Installed {
            sha256: [0; 32],
            ..code
        };
        assert_eq!(
            load_installed(&storage, &changed).err(),
            Some(PluginProblem::Changed)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    fn move_dynamic_offset_to_zeros(lib: &[u8]) -> Vec<u8> {
        // The recipe: append as many zero bytes as the dynamic segment has and
        // change only PT_DYNAMIC's file offset to them. The zeros read as an empty
        // dynamic section; the system loader still reads the real one at p_vaddr.
        let phoff = u64::from_le_bytes(lib[32..40].try_into().unwrap()) as usize;
        let phnum = usize::from(u16::from_le_bytes(lib[56..58].try_into().unwrap()));
        let mut out = lib.to_vec();
        for i in 0..phnum {
            let at = phoff + i * 56;
            if u32::from_le_bytes(lib[at..at + 4].try_into().unwrap()) == 2 {
                let filesz = u64::from_le_bytes(lib[at + 32..at + 40].try_into().unwrap()) as usize;
                let zeros_at = lib.len() as u64;
                out.resize(lib.len() + filesz, 0);
                out[at + 8..at + 16].copy_from_slice(&zeros_at.to_le_bytes());
                return out;
            }
        }
        panic!("no PT_DYNAMIC");
    }

    /// The file offset of virtual address `vaddr` in a library, by its `PT_LOAD`s.
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    fn offset_of_vaddr(lib: &[u8], vaddr: u64) -> usize {
        let u64_at = |at: usize| u64::from_le_bytes(lib[at..at + 8].try_into().unwrap());
        let phoff = u64_at(32) as usize;
        let phnum = usize::from(u16::from_le_bytes(lib[56..58].try_into().unwrap()));
        for i in 0..phnum {
            let at = phoff + i * 56;
            let (off, va, sz) = (u64_at(at + 8), u64_at(at + 16), u64_at(at + 32));
            if u32::from_le_bytes(lib[at..at + 4].try_into().unwrap()) == 1
                && vaddr >= va
                && vaddr < va + sz
            {
                return (off + (vaddr - va)) as usize;
            }
        }
        panic!("address {vaddr:#x} is in no PT_LOAD");
    }

    /// The file offset of the value of dynamic entry `tag` in a library.
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    fn dynamic_value_at(lib: &[u8], tag: u64) -> usize {
        let u64_at = |at: usize| u64::from_le_bytes(lib[at..at + 8].try_into().unwrap());
        let phoff = u64_at(32) as usize;
        let phnum = usize::from(u16::from_le_bytes(lib[56..58].try_into().unwrap()));
        for i in 0..phnum {
            let at = phoff + i * 56;
            if u32::from_le_bytes(lib[at..at + 4].try_into().unwrap()) == 2 {
                let start = u64_at(at + 8) as usize;
                let end = start + u64_at(at + 32) as usize;
                let mut d = start;
                while d < end && u64_at(d) != 0 {
                    if u64_at(d) == tag {
                        return d + 8;
                    }
                    d += 16;
                }
            }
        }
        panic!("no dynamic entry {tag:#x}");
    }

    /// The same library with the destination of its first `Rela` entry moved to
    /// `place`; the system loader writes there while it loads.
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    fn move_first_relocation(lib: &[u8], place: u64) -> Vec<u8> {
        let rela = u64::from_le_bytes(lib[dynamic_value_at(lib, 7)..][..8].try_into().unwrap());
        let entry = offset_of_vaddr(lib, rela);
        let mut out = lib.to_vec();
        out[entry..entry + 8].copy_from_slice(&place.to_le_bytes());
        out
    }

    /// The same library claiming `count` base-relative relocations (`DT_RELACOUNT`).
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    fn claim_relative_relocations(lib: &[u8], count: u64) -> Vec<u8> {
        let at = dynamic_value_at(lib, 0x6fff_fff9);
        let mut out = lib.to_vec();
        out[at..at + 8].copy_from_slice(&count.to_le_bytes());
        out
    }

    /// A library whose relocation points outside the library makes the system
    /// loader fault (SIGSEGV) while it loads. It is refused before it is loaded.
    /// The control loads it by hand in a child process and sees the crash.
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[test]
    fn a_library_whose_relocation_points_outside_is_refused_before_it_is_loaded() {
        use std::os::unix::process::ExitStatusExt;
        // The child of the control: load the file named in the environment.
        if let Some(path) = std::env::var_os("PAPERPAW_E6_CONTROL") {
            // SAFETY: the test loads a damaged library on purpose; it faults.
            drop(unsafe { libloading::Library::new(path) });
            return;
        }
        let root = std::env::temp_dir().join(format!("paperpaw-e6-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let hello = root.join("plugins/hello");
        std::fs::create_dir_all(&hello).unwrap();
        let source = r#"#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
extern "C" fn f() -> u32 { 7 }
#[unsafe(no_mangle)]
pub static TABLE: [extern "C" fn() -> u32; 2] = [f, f];
"#;
        let src = root.join("reloc.rs");
        let lib = hello.join(file_name());
        std::fs::write(&src, source).unwrap();
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
        let built = std::process::Command::new(rustc)
            .args([
                "--edition",
                "2024",
                "--crate-type",
                "cdylib",
                "-C",
                "panic=abort",
            ])
            .args(["-C", "link-arg=-nostartfiles", "-O", "-o"])
            .arg(&lib)
            .arg(&src)
            .status();
        let Ok(built) = built else {
            assert!(
                std::env::var("PAPERPAW_NO_FIXTURE_COMPILER").as_deref() == Ok("1"),
                "no rustc to build the fixture library (set PAPERPAW_NO_FIXTURE_COMPILER=1 to skip)"
            );
            eprintln!("no rustc to build the library; test skipped by request");
            let _ = std::fs::remove_dir_all(&root);
            return;
        };
        assert!(built.success(), "the fixture library did not compile");
        let good = std::fs::read(&lib).unwrap();
        let moved = move_first_relocation(&good, 0x7fff_f000_0000);
        let counted = claim_relative_relocations(&good, 1000);
        assert_ne!(good, moved, "the fixture has no relocation to move");
        assert_ne!(good, counted, "the fixture has no relocation count");
        // The control: the unchanged library loads by hand; each damaged one
        // takes down the process that loads it.
        let load_by_hand = |bytes: &[u8]| {
            let p = root.join("control.so");
            std::fs::write(&p, bytes).unwrap();
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "native::tests::a_library_whose_relocation_points_outside_is_refused_before_it_is_loaded",
                ])
                .env("PAPERPAW_E6_CONTROL", &p)
                .status()
                .unwrap()
        };
        assert!(
            load_by_hand(&good).success(),
            "the control library does not load"
        );
        assert_eq!(
            load_by_hand(&moved).signal(),
            Some(11),
            "the damaged library does not fault the system loader"
        );
        assert!(
            !load_by_hand(&counted).success(),
            "the library with a false relocation count loads"
        );
        // The loader refuses each and the process lives.
        let storage = crate::device::library(&root).unwrap();
        let id = paperpaw_core::sdk::Id::new_static("hello");
        let path = format!("plugins/hello/{}", file_name());
        for bad in [&moved, &counted] {
            std::fs::write(&lib, bad).unwrap();
            let code = Installed {
                id: &id,
                path: &path,
                sha256: paperpaw_core::sdk::digest::sha256::digest(bad),
            };
            assert_eq!(
                load_installed(&storage, &code).err(),
                Some(PluginProblem::Damaged)
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// ADR 0034 section 6: a library whose initialiser runs when it is loaded
    /// is refused before it is loaded. The control loads the same library
    /// by hand and sees the initialiser run, so the test can fail.
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[test]
    fn a_library_with_an_initialiser_is_refused_before_it_is_loaded() {
        let root = std::env::temp_dir().join(format!("paperpaw-init-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let hello = root.join("plugins/hello");
        std::fs::create_dir_all(&hello).unwrap();
        let marker = root.join("initialiser-ran");
        let source = format!(
            r#"#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {{ loop {{}} }}
// No C library call: the marker is made by a system call, so the library
// has no imports and so no symbol-version tables.
extern "C" fn make_marker() {{
    let path = b"{}\0".as_ptr();
    #[cfg(target_arch = "x86_64")]
    unsafe {{ core::arch::asm!("syscall", inlateout("rax") 257isize => _, in("rdi") -100isize, in("rsi") path, in("rdx") 0o101isize, in("r10") 0o600isize, lateout("rcx") _, lateout("r11") _, options(nostack)); }}
    #[cfg(target_arch = "aarch64")]
    unsafe {{ core::arch::asm!("svc 0", in("x8") 56isize, inlateout("x0") -100isize => _, in("x1") path, in("x2") 0o101isize, in("x3") 0o600isize, options(nostack)); }}
}}
#[used]
#[unsafe(link_section = ".init_array")]
static INIT: extern "C" fn() = make_marker;
"#,
            marker.display()
        );
        let src = root.join("init.rs");
        let lib = hello.join(file_name());
        std::fs::write(&src, source).unwrap();
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
        let built = std::process::Command::new(rustc)
            .args([
                "--edition",
                "2024",
                "--crate-type",
                "cdylib",
                "-C",
                "panic=abort",
            ])
            .args(["-C", "link-arg=-nostartfiles", "-O", "-o"])
            .arg(&lib)
            .arg(&src)
            .status();
        let Ok(built) = built else {
            // A compiler that cannot be started fails the test too, unless the
            // run says in the open that there is none.
            assert!(
                std::env::var("PAPERPAW_NO_FIXTURE_COMPILER").as_deref() == Ok("1"),
                "no rustc to build the fixture library (set PAPERPAW_NO_FIXTURE_COMPILER=1 to skip)"
            );
            eprintln!("no rustc to build the library; test skipped by request");
            let _ = std::fs::remove_dir_all(&root);
            return;
        };
        // A compiler that runs and fails is a failure, not a skipped test.
        assert!(built.success(), "the fixture library did not compile");
        // The control: loading it by hand runs the initialiser.
        // SAFETY: a library built here, whose only code is the marker.
        drop(unsafe { libloading::Library::new(&lib) }.unwrap());
        assert!(marker.exists(), "the fixture does not run an initialiser");
        std::fs::remove_file(&marker).unwrap();
        // The same through the open file (`/proc/self/fd/N`), as the loader
        // loads what it checked: the system loader accepts that path.
        let file = std::fs::File::open(&lib).unwrap();
        // SAFETY: as above.
        drop(unsafe { libloading::Library::new(load_path(&lib, &file)) }.unwrap());
        assert!(marker.exists(), "the open file was not loaded");
        std::fs::remove_file(&marker).unwrap();
        // The Senior Reviewer's recipe: only PT_DYNAMIC's file offset moved onto
        // appended zeros. Loaded by hand, the system loader still runs the
        // initialiser (the file is dangerous); the loader refuses it.
        let moved = move_dynamic_offset_to_zeros(&std::fs::read(&lib).unwrap());
        let moved_lib = root.join("moved.so");
        std::fs::write(&moved_lib, &moved).unwrap();
        // SAFETY: as above.
        drop(unsafe { libloading::Library::new(&moved_lib) }.unwrap());
        assert!(
            marker.exists(),
            "the moved-offset file does not run the initialiser"
        );
        std::fs::remove_file(&marker).unwrap();
        std::fs::write(&lib, &moved).unwrap();
        // The loader refuses it and never loads it.
        let bytes = std::fs::read(&lib).unwrap();
        let storage = crate::device::library(&root).unwrap();
        let id = paperpaw_core::sdk::Id::new_static("hello");
        let path = format!("plugins/hello/{}", file_name());
        let code = Installed {
            id: &id,
            path: &path,
            sha256: paperpaw_core::sdk::digest::sha256::digest(&bytes),
        };
        assert_eq!(
            load_installed(&storage, &code).err(),
            Some(PluginProblem::Damaged)
        );
        assert!(
            !marker.exists(),
            "the initialiser ran while the loader refused it"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Two different libraries loaded one after the other, both alive: each is
    /// itself. The system loader returns a library it already holds when it is
    /// given the same name again, and `/proc/self/fd/N` is the same name for any
    /// file that gets the number N, so the loader must keep the first file
    /// open for as long as its library lives (the Senior Reviewer's finding).
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[test]
    fn two_libraries_loaded_one_after_the_other_are_two_libraries() {
        let root = std::env::temp_dir().join(format!("paperpaw-two-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
        let mut held = Vec::new();
        for value in [11u32, 22] {
            let src = root.join(format!("v{value}.rs"));
            let lib = root.join(format!("libv{value}.so"));
            std::fs::write(
                &src,
                format!(
                    "#![no_std]\n#[panic_handler]\nfn panic(_: &core::panic::PanicInfo) -> ! {{ loop {{}} }}\n#[unsafe(no_mangle)]\npub extern \"C\" fn value() -> u32 {{ {value} }}\n"
                ),
            )
            .unwrap();
            let built = std::process::Command::new(&rustc)
                .args([
                    "--edition",
                    "2024",
                    "--crate-type",
                    "cdylib",
                    "-C",
                    "panic=abort",
                ])
                .args(["-C", "link-arg=-nostartfiles", "-O", "-o"])
                .arg(&lib)
                .arg(&src)
                .status();
            let Ok(built) = built else {
                assert!(
                    std::env::var("PAPERPAW_NO_FIXTURE_COMPILER").as_deref() == Ok("1"),
                    "no rustc to build the fixture libraries (set PAPERPAW_NO_FIXTURE_COMPILER=1 to skip)"
                );
                eprintln!("no rustc to build the libraries; test skipped by request");
                let _ = std::fs::remove_dir_all(&root);
                return;
            };
            assert!(built.success(), "the fixture library did not compile");
            let file = std::fs::File::open(&lib).unwrap();
            // SAFETY: a library built here with no initialiser.
            held.push(unsafe { HeldLibrary::open(&lib, file) }.unwrap());
        }
        let values: Vec<u32> = held
            .iter()
            .map(|h| {
                // SAFETY: `value` is a plain function of the library built above.
                unsafe {
                    (h.library
                        .get::<unsafe extern "C" fn() -> u32>(b"value\0")
                        .unwrap())()
                }
            })
            .collect();
        assert_eq!(values, [11, 22], "the second library is the first again");
        drop(held);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Where interface-7 plugins run (the developer switch), the loader wires
    /// `core.ui` for its device beside `text.hyphenate`, once; with the switch
    /// off it wires nothing.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn the_loader_wires_core_ui_only_with_the_developer_switch() {
        use paperpaw_core::native_host::HostWiring;
        let _one = crate::one_device_at_a_time();
        let storage: std::rc::Rc<dyn Storage> =
            std::rc::Rc::new(paperpaw_core::sdk::MemoryStorage::new());
        let was = crate::native_runtime::enabled();
        crate::native_runtime::set_enabled(false);
        let mut off = HostLoader::for_device(MemoryClass::Small);
        assert!(off.hyphenation_sink(&storage).is_none());
        assert!(off.resources.is_none(), "nothing is made, nothing wired");
        crate::native_runtime::set_enabled(true);
        for device in [MemoryClass::Small, MemoryClass::Large] {
            let mut on = HostLoader::for_device(device);
            assert!(on.hyphenation_sink(&storage).is_some(), "{device:?}");
            let resources = on.resources.as_ref().unwrap();
            // Wired already: a second `core.ui`, or a second `text.hyphenate`, is refused.
            assert!(resources.wire(HostWiring::new().ui(device)).is_err());
            assert!(
                resources
                    .wire(HostWiring::new().hyphenation(storage.clone()))
                    .is_err()
            );
        }
        crate::native_runtime::set_enabled(was);
    }

    /// Two plugins admitted by one loader share its one device-wide dispatch
    /// budget (Kernel's condition for H1b).
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn plugins_admitted_by_one_loader_share_one_dispatch_budget() {
        let mut loader = HostLoader::for_device(MemoryClass::Small);
        let first = loader.shared_depth().unwrap();
        let second = loader.shared_depth().unwrap();
        assert!(std::ptr::eq(first.as_depth(), second.as_depth()));
        let other = HostLoader::for_device(MemoryClass::Small)
            .shared_depth()
            .unwrap();
        assert!(!std::ptr::eq(first.as_depth(), other.as_depth()));
    }

    /// The id of a linked plugin is that plugin's, not a card folder's.
    #[test]
    fn a_card_folder_named_like_a_linked_plugin_is_refused() {
        for id in ["settings", "wifi", "opds", "home"] {
            assert!(paperpaw_app::is_linked_id(id), "{id} is linked here");
            assert_eq!(refuse_linked_id(id), Err(PluginProblem::Mismatch), "{id}");
        }
        assert!(!paperpaw_app::is_linked_id("a-plugin-of-its-own"));
        assert_eq!(refuse_linked_id("a-plugin-of-its-own"), Ok(()));
    }

    /// A kernel takes one table of handles and one dispatch budget: the card
    /// plugins' loader and the linked plugins' registration get the same ones
    /// from the pool they share (a second pool was refused with `ERR_STATE`,
    /// so no linked plugin registered beside an installed card plugin).
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn a_loader_and_the_linked_plugins_share_one_pool_and_one_budget() {
        use paperpaw_core::native_host::HostWiring;
        let pool = DevicePool::default();
        let mut loader = HostLoader::for_device_in(MemoryClass::Small, pool.clone());
        let card = loader.shared_depth().unwrap();
        let linked = pool.depth().unwrap();
        assert!(std::ptr::eq(card.as_depth(), linked.as_depth()));
        // The table is made once, `core.ui` wired: wiring it again is refused.
        let table = pool.resources(MemoryClass::Small).unwrap();
        assert!(
            table
                .wire(HostWiring::new().ui(MemoryClass::Small))
                .is_err()
        );
        // Another pool is another kernel's.
        let other = DevicePool::default().depth().unwrap();
        assert!(!std::ptr::eq(card.as_depth(), other.as_depth()));
    }

    /// The table of handles has `core.ui` from the moment it is made, before
    /// any plugin loads; only `text.hyphenate` waits for the card.
    #[test]
    fn core_ui_is_wired_when_the_table_of_handles_is_made() {
        use paperpaw_core::native_host::HostWiring;
        let resources = new_resources(MemoryClass::Small).unwrap();
        assert!(
            resources
                .wire(HostWiring::new().ui(MemoryClass::Small))
                .is_err()
        );
        let storage: std::rc::Rc<dyn Storage> =
            std::rc::Rc::new(paperpaw_core::sdk::MemoryStorage::new());
        assert!(
            resources
                .wire(HostWiring::new().hyphenation(storage))
                .is_ok()
        );
    }
}
