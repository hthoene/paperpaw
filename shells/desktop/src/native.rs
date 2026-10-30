//! Code plugins in the emulator (ADR 0024): a plugin's crate built for
//! this computer (`cargo build --release`, a `cdylib` with `plugin!`) is
//! `code-host.dll`, `code-host.so` or `code-host.dylib` in its folder of
//! the emulated card (`plugins/<id>/`), beside its `plugin.ini`; the emulator loads it as
//! Android loads `code.so`, with the same entry and table as `.ppn` files
//! on the X4. It must be built by the same compiler, for this computer,
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
use paperpaw_platform::Storage;

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
    // The firmware's shared functions, as the X4's table has them: plugins
    // built for a chip keep none of their own, and a library built in
    // table mode (`--cfg paperpaw_table_host`, CI) calls them.
    shared: paperpaw_core::sdk::shared::table(),
    shared_len: paperpaw_core::sdk::shared::COUNT,
};

/// One heap here: wherever asked, as `alloc`.
unsafe extern "C" fn alloc_in(size: usize, align: usize, _placement: u32) -> *mut u8 {
    // SAFETY: as `alloc`.
    unsafe { alloc(size, align) }
}

/// Loads code plugins built for this computer.
#[derive(Debug, Default)]
pub struct HostLoader;

impl CodeLoader for HostLoader {
    fn load(
        &mut self,
        storage: &dyn Storage,
        code: &CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        // Not the device's heap: its code lies in flash (or the PSRAM it
        // runs from), never on the heap a plugin's budget counts.
        crate::budget::untracked(|| load(storage, code))
    }
}

/// [`HostLoader::load`], its memory not counted against the budget.
fn load(storage: &dyn Storage, code: &CodeFile<'_>) -> Result<Box<dyn Plugin>, PluginProblem> {
    let bytes = paperpaw_core::sdk::read_file(storage, code.path, MAX_FILE)
        .map_err(|_| PluginProblem::Damaged)?;
    // Only the file installed.
    if paperpaw_core::sdk::digest::sha256::digest(&bytes) != code.sha256 {
        return Err(PluginProblem::Changed);
    }
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
        std::io::Write::write_all(&mut file, &bytes).map_err(|_| PluginProblem::Damaged)?;
    }
    let copied = std::fs::read(&copy).map_err(|_| PluginProblem::Damaged)?;
    if paperpaw_core::sdk::digest::sha256::digest(&copied) != code.sha256 {
        return Err(PluginProblem::Changed);
    }
    if let Err(why) = built_here(&copied) {
        eprintln!("code plugin {}: {why}", code.id.as_str());
        return Err(PluginProblem::OtherBuild);
    }
    // SAFETY: a file the reader installed on the card (`trust`), its
    // SHA-256 the one installed; a code plugin runs as part of
    // PaperPaw, without a sandbox (ADR 0024).
    let library = unsafe { libloading::Library::new(&copy) }.map_err(|_| PluginProblem::Damaged)?;
    // SAFETY: the statics `plugin!` exports, of these types.
    let (slot, entry) = unsafe {
        let slot = library
            .get::<*const ApiSlot>(b"PAPERPAW_API\0")
            .map_err(|_| PluginProblem::Damaged)?;
        let entry = library
            .get::<*const Entry>(b"PAPERPAW_ENTRY\0")
            .map_err(|_| PluginProblem::Damaged)?;
        (&**slot, &**entry)
    };
    if entry.fingerprint != FINGERPRINT.0 {
        return Err(PluginProblem::OtherBuild);
    }
    // Built for more shared functions than the table has: refused before
    // anything of it runs, as the firmware does.
    if entry.needs > API.shared_len {
        eprintln!(
            "code plugin {}: needs {} entries of the shared function table, this build has {}",
            code.id.as_str(),
            entry.needs,
            API.shared_len
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
    // is in use.
    std::mem::forget(library);
    Ok(plugin)
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

#[cfg(test)]
mod tests {
    use super::*;

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
            Some(PluginProblem::OtherBuild),
            "loaded, and found no library of this compiler"
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
        // Installed but no library of this compiler: said, not fatal.
        let id = paperpaw_core::sdk::Id::new_static("hello");
        let path = format!("plugins/hello/{}", file_name());
        let code = CodeFile {
            id: &id,
            path: &path,
            sha256: paperpaw_core::sdk::digest::sha256::digest(b"not a library"),
        };
        assert_eq!(
            HostLoader.load(&storage, &code).err(),
            Some(PluginProblem::OtherBuild)
        );
        let changed = CodeFile {
            sha256: [0; 32],
            ..code
        };
        assert_eq!(
            HostLoader.load(&storage, &changed).err(),
            Some(PluginProblem::Changed)
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
