//! Code plugins in the emulator (ADR 0024): a plugin's crate built for
//! this computer (an interface-7 library, `tools/plugin-host-libs`) is
//! `code-host.dll`, `code-host.so` or `code-host.dylib` in its folder of
//! the emulated card (`plugins/<id>/`), beside its `plugin.ini`. The
//! emulator hosts it as the devices host their `.ppn` files: its bytes are
//! checked before anything of it runs, and the kernel admits it with
//! everything it declares (`native_runtime`; Linux on x86-64, where that
//! loader is built: elsewhere a card's code is not supported, as on
//! Android). It must be installed: only a file whose SHA-256 the card has
//! (`native::remember_code`) runs, as on the devices, which [`trust`] gives
//! every one on a card (`--trust-code`).
//!
//! A library built for interface 6 (with `plugin!`, as for Beta 3) is told
//! from its bytes and listed as built for an older PaperPaw: nothing of it
//! is loaded.

use paperpaw_core::sdk::PluginProblem;
use paperpaw_core::sdk::native::Native;
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

/// Loads code plugins built for this computer.
///
/// An interface-7 library is hosted over
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
    /// It hosts the card's code plugins; not the loader of a start without
    /// them, which serves the linked plugins alone ([`Self::linked_only`]).
    card_code: bool,
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
            card_code: true,
        }
    }

    /// For a start that loads no code from the card: what the linked
    /// plugins need is served, nothing for card plugins.
    #[must_use]
    pub fn linked_only(mut self) -> Self {
        self.card_code = false;
        self
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
    #[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
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
    /// A library of the card, the file installed, is hosted for the kernel
    /// to admit with everything it declares; one built for interface 6, one
    /// that is no library of either kind, and any where this loader is not
    /// built or switched off are refused with why (`Self::native`).
    fn load_native(
        &mut self,
        storage: &dyn Storage,
        code: &CodeFile<'_>,
    ) -> Result<Option<Box<dyn paperpaw_core::NativeAdmission>>, PluginProblem> {
        // Not the device's heap: its code lies in flash (or the PSRAM it
        // runs from), never on the heap a plugin's budget counts.
        crate::budget::untracked(|| {
            let bytes = read_installed(storage, code)?;
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
        // The linked plugins (the app's own, `native-linked`) read the tables
        // as the card ones do, switch or not; as the firmware's sink is.
        let hosts = self.card_code && cfg!(all(target_os = "linux", target_arch = "x86_64"));
        if !hosts && !cfg!(feature = "native-linked") {
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

    /// The library `bytes` of `code`, told from its bytes: an interface-7 one is
    /// checked and hosted for the
    /// kernel to admit: nothing of it runs but what the kernel's admission
    /// calls (`make`), and what it declared is registered with it or the
    /// plugin is refused, never loaded without it.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn native(
        &mut self,
        bytes: &[u8],
        code: &CodeFile<'_>,
    ) -> Result<Box<dyn paperpaw_core::NativeAdmission>, PluginProblem> {
        use crate::native_runtime::{Kind, Refusal};
        let id = code.id.as_str();
        // First, whatever the file is: the id is the folder's.
        refuse_linked_id(id)?;
        match crate::native_runtime::classify(bytes) {
            Kind::Interface7 => {}
            Kind::Interface6 => {
                eprintln!(
                    "code plugin {id}: built for interface 6 (an older PaperPaw); not loaded"
                );
                return Err(PluginProblem::OlderInterface { built_for: 6 });
            }
            Kind::Unclear => {
                eprintln!("code plugin {id}: no library of a plugin; not loaded");
                return Err(PluginProblem::Damaged);
            }
        }
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

/// Code plugins built for this computer, from the card (`on`: `device::set_code_plugins`).
pub fn code_plugins(
    builder: paperpaw_core::KernelBuilder,
    on: bool,
    memory: MemoryClass,
    pool: &DevicePool,
) -> paperpaw_core::KernelBuilder {
    if !on {
        // No code is loaded (and so no code device): the linked plugins'
        // `text.hyphenate` still reads the hyphenation packs on the card (todo 613).
        let mut loader = HostLoader::for_device_in(memory, pool.clone()).linked_only();
        return builder
            .hyphenation_sink(move |card| CodeLoader::hyphenation_sink(&mut loader, card));
    }
    builder.code_plugins(
        native(),
        Box::new(HostLoader::for_device_in(memory, pool.clone())),
    )
}

/// The installed file, read: only the file installed may run.
fn read_installed(storage: &dyn Storage, code: &CodeFile<'_>) -> Result<Vec<u8>, PluginProblem> {
    let bytes = paperpaw_core::sdk::read_file(storage, code.path, MAX_FILE)
        .map_err(|_| PluginProblem::Damaged)?;
    if paperpaw_core::sdk::digest::sha256::digest(&bytes) != code.sha256 {
        return Err(PluginProblem::Changed);
    }
    Ok(bytes)
}

/// A loaded library and the open file it was loaded from. The system loader
/// finds a library it already has by the name it was given, and the name
/// `/proc/self/fd/N` is the same for every file that gets the number N: so
/// the file stays open as long as the library is loaded, and a number in use
/// by a loaded library is never given to another.
pub(super) struct HeldLibrary {
    /// First, so it is unloaded before its file is closed.
    pub(super) library: libloading::Library,
    /// Held, never read: open as long as the library is loaded.
    _file: std::fs::File,
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
        Ok(Self {
            library,
            _file: file,
        })
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

    /// What a file that is no library of a plugin comes to: told from its
    /// bytes and refused before anything of it is loaded; where this loader is
    /// not built (not Linux on x86-64) a card's code is not supported at all.
    fn not_a_library() -> PluginProblem {
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            PluginProblem::Damaged
        } else {
            PluginProblem::Unsupported
        }
    }

    const INI: &str = "[plugin]\nid = hello\nname = Hello\nversion = 0.1.0\n";

    /// What the plugin manager says of the code plugin `hello` of the card in
    /// `root` with code plugins on (`--trust-code`): the loader's answer, by
    /// the way a start goes (the card read, the file installed, [`HostLoader`]).
    fn said(root: &std::path::Path) -> Option<PluginProblem> {
        let _one = crate::one_device_at_a_time();
        crate::device::set_code_plugins(true);
        let storage = crate::device::library(root).unwrap();
        let profile = crate::profiles::find("boox-go-7").unwrap();
        let device = crate::device::Device::new(profile, None, storage, true).unwrap();
        let problem = device.kernel.plugins().get("hello").and_then(|p| p.problem);
        crate::device::set_code_plugins(false);
        problem
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
        // Installed but no library of a plugin: said, not fatal.
        std::fs::write(hello.join("plugin.ini"), INI).unwrap();
        assert_eq!(said(&root), Some(not_a_library()));
        // Changed since it was installed: not the file installed.
        std::fs::write(hello.join(file_name()), b"another file").unwrap();
        let changed = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            PluginProblem::Changed
        } else {
            PluginProblem::Unsupported
        };
        assert_eq!(said(&root), Some(changed));
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
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
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
        std::fs::write(hello.join("plugin.ini"), INI).unwrap();
        for bad in [&moved, &counted] {
            std::fs::write(&lib, bad).unwrap();
            assert_eq!(trust(&root).unwrap(), ["hello"]);
            assert_eq!(said(&root), Some(PluginProblem::Damaged));
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// ADR 0034 section 6: a library whose initialiser runs when it is loaded
    /// is refused before it is loaded. The control loads the same library
    /// by hand and sees the initialiser run, so the test can fail.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
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
        std::fs::write(hello.join("plugin.ini"), INI).unwrap();
        assert_eq!(trust(&root).unwrap(), ["hello"]);
        assert_eq!(said(&root), Some(PluginProblem::Damaged));
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
    /// `core.ui` for its device beside `text.hyphenate`, once; a loader for a
    /// start without card code wires nothing for card plugins.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn the_loader_wires_core_ui_only_where_it_hosts_card_code() {
        use paperpaw_core::native_host::HostWiring;
        let _one = crate::one_device_at_a_time();
        let storage: std::rc::Rc<dyn Storage> =
            std::rc::Rc::new(paperpaw_core::sdk::MemoryStorage::new());
        let mut off = HostLoader::for_device(MemoryClass::Small).linked_only();
        // With the linked plugins built in, the tables are wired card code or not.
        assert_eq!(
            off.hyphenation_sink(&storage).is_some(),
            cfg!(feature = "native-linked")
        );
        assert_eq!(off.resources.is_some(), cfg!(feature = "native-linked"));
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
