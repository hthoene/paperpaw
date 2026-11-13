//! Inactive interface-7 desktop lifecycle (ADR 0034).
//!
//! This module qualifies the factory and destruction contract separately from
//! CodeLoader activation. Core registration, events and rendering are not wired.
//! Only the reviewed Linux x86-64 profile is supported.

use std::ffi::c_void;
use std::path::Path;
use std::ptr;

use paperpaw_native::host_glue::{ENTRY_SYMBOL, METADATA_SYMBOL, OK, PpEntry, PpHost, PpPlugin};
use paperpaw_native::preflight::{
    FreestandingRefusal, PLUGIN_EXPORTS, check_freestanding, data_export,
};

use crate::host_services::DesktopHost;
use crate::native::HeldLibrary;
use paperpaw_core::native_host;

#[allow(
    dead_code,
    reason = "all descriptor prefix offsets are generated together"
)]
mod wire {
    include!(concat!(env!("OUT_DIR"), "/metadata.rs"));
}

/// The profile this shell hosts (the reviewed Linux x86-64 one), as the
/// kernel's admission of a linked plugin names it (`crate::linked`).
#[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
pub(crate) fn host_profile() -> paperpaw_native::metadata::Profile<'static> {
    paperpaw_native::metadata::Profile {
        target: core::str::from_utf8(wire::PROFILE).unwrap_or(""),
        entry_layout: wire::PROFILE_ENTRY_LAYOUT as u16,
        entry_bytes: wire::PROFILE_ENTRY_BYTES as u16,
    }
}

/// Refusal before factory invocation, or a failed factory result.
#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeError {
    /// The installed bytes no longer match their digest, or cannot be read.
    File,
    /// The ELF structures or exported objects are unsuitable.
    Export,
    /// The library needs another library or imports a symbol (strong or weak)
    /// instead of reaching the host through its table: refused from its
    /// bytes, before any load.
    Import,
    /// The interface, profile, requirements or metadata structure is unsupported.
    Metadata,
    /// Memory for checked admission requirements could not be allocated.
    NoMemory,
    /// A required semantic feature is absent or needs a newer revision.
    RequiredFeature { key: String, revision: u16 },
    /// The system loader could not load the checked file.
    Load,
    /// Loaded entry and locator disagree with the bytes checked before loading.
    Entry,
    /// The factory returned a failure status.
    Factory(i32),
    /// A successful factory did not return a current lifecycle table.
    Plugin,
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, RuntimeError> {
    let raw = bytes.get(at..at + 2).ok_or(RuntimeError::Metadata)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, RuntimeError> {
    let raw = bytes.get(at..at + 4).ok_or(RuntimeError::Metadata)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

/// This shell advertises only capabilities it implements, never slot presence.
#[cfg(test)]
fn metadata(bytes: &[u8], host: &PpHost) -> Result<u16, RuntimeError> {
    checked_metadata(bytes, host, &[]).map(|checked| checked.needs_minor)
}

// Only implemented semantic features belong here. Slot presence is insufficient.
pub(crate) const WIRED_FEATURES: &[paperpaw_native::metadata::Feature<'static>] = &[];

#[cfg(test)]
fn checked_metadata<'a>(
    bytes: &'a [u8],
    host: &PpHost,
    services: &[paperpaw_native::metadata::Service<'_>],
) -> Result<paperpaw_native::metadata::Checked<'a>, RuntimeError> {
    checked_metadata_with(bytes, host, services, WIRED_FEATURES)
}

/// What a card plugin is offered besides the table: the services and the
/// consent features of the emulated device (the same as the firmware's card
/// path, `paperpaw_x4::native_offer::card_host`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Offer {
    pub(crate) services: &'static [paperpaw_native::metadata::Service<'static>],
    pub(crate) features: &'static [paperpaw_native::metadata::Feature<'static>],
}

fn checked_metadata_with<'a>(
    bytes: &'a [u8],
    host: &PpHost,
    services: &[paperpaw_native::metadata::Service<'_>],
    features: &[paperpaw_native::metadata::Feature<'_>],
) -> Result<paperpaw_native::metadata::Checked<'a>, RuntimeError> {
    use paperpaw_native::metadata::{self, Host, Profile, Refusal};
    let capabilities = Host {
        abi_major: host.major,
        minor: host.minor,
        profile: Profile {
            target: core::str::from_utf8(wire::PROFILE).map_err(|_| RuntimeError::Metadata)?,
            entry_layout: wire::PROFILE_ENTRY_LAYOUT as u16,
            entry_bytes: wire::PROFILE_ENTRY_BYTES as u16,
        },
        services,
        features,
    };
    match metadata::check(bytes, &capabilities) {
        Ok(checked) => Ok(checked),
        Err(Refusal::RequiredFeature { key, revision }) => {
            let mut owned = String::new();
            owned
                .try_reserve_exact(key.len())
                .map_err(|_| RuntimeError::Metadata)?;
            owned.push_str(key);
            Err(RuntimeError::RequiredFeature {
                key: owned,
                revision,
            })
        }
        Err(_) => Err(RuntimeError::Metadata),
    }
}

/// A live plugin. Its factory output and code remain owned until destruction.
#[allow(
    missing_debug_implementations,
    reason = "holds code and an opaque plugin state"
)]
pub struct LoadedPlugin {
    state: *mut c_void,
    destroy: unsafe extern "C" fn(*mut c_void),
    // Drop::drop invokes plugin destruction before this library is unloaded.
    _library: HeldLibrary,
    // The host context remains alive through destruction; currently process-owned.
    _host: &'static DesktopHost,
    _callbacks: plugin_output::PluginOutput,
}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        // SAFETY: a successful factory transferred exactly one drop obligation.
        unsafe { (self.destroy)(self.state) };
    }
}

/// Loads an installed interface-7 library and calls `make(host, host_ctx, out)`.
/// The open descriptor checked here stays alive until after plugin destruction.
///
/// # Safety
///
/// The file must be reader-authorized native code. Preflight is a structural
/// check, not a sandbox. The plugin must obey the generated pointer, factory
/// and destruction contracts, including valid live output on success, no live
/// output on failure, and no calls after destruction. The checked inode must
/// remain immutable until unloading. It must not spawn work outliving its state.
pub unsafe fn load(
    path: &Path,
    digest: &[u8; 32],
    host: &'static DesktopHost,
) -> Result<LoadedPlugin, RuntimeError> {
    // SAFETY: the same authorization and immutable-file contract applies.
    let CheckedLibrary { library, make, .. } =
        unsafe { checked_library(path, digest, host.table(), &[], WIRED_FEATURES) }?;
    let mut out = ptr::null();
    // SAFETY: table and context outlive the plugin; output storage is writable.
    let status = unsafe { make(host.table(), host.ctx(), &mut out) };
    if status != OK {
        return Err(RuntimeError::Factory(status));
    }
    // SAFETY: successful authorized make supplies its stable declared span;
    // the library and host stay alive while generated prefix/tail readers run.
    let plugin = unsafe { plugin_output::read(out) }?;
    Ok(LoadedPlugin {
        state: plugin.state,
        destroy: plugin.destroy,
        _library: library,
        _host: host,
        _callbacks: plugin,
    })
}

type Make = unsafe extern "C" fn(*const PpHost, *mut c_void, *mut *const PpPlugin) -> i32;

struct CheckedLibrary {
    declared_services: native_host::shell::DeclaredServices,
    library: HeldLibrary,
    make: Make,
    /// The library's read-only data as mapped: its address and length
    /// (`read_only_segment`), valid as long as `library` is loaded.
    image: (usize, usize),
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    let raw = bytes.get(at..at.checked_add(8)?)?;
    Some(u64::from_le_bytes(raw.try_into().ok()?))
}

/// The library's read-only data, as the registration step's image: the
/// loadable segment of the 64-bit little-endian ELF `bytes` that holds
/// `anchor` (the address of `PAPERPAW_METADATA`, a read-only record), as
/// `(address, length)` once the library is mapped at `base`. Offsets of
/// `DeclRef::Image` are relative to its start.
///
/// The segment must be readable and neither writable nor executable, and its
/// file bytes must lie wholly inside the file (`bytes`, the file as checked),
/// so no mapped page of it reaches past the end of the file (a read there
/// would be a bus error). `(0, 0)` where there is none: a batch is then kept
/// in the plugin's own budget, not as offsets. The segment holding the
/// headers and the dynamic symbols is not taken for it: with GNU ld the
/// `.rodata` lies in a later one. Only the program headers are read and every
/// offset is checked, so a hostile table gives `(0, 0)`, never a panic.
fn read_only_segment(bytes: &[u8], base: usize, anchor: u64) -> (usize, usize) {
    const PT_LOAD: u32 = 1;
    const PF_X: u32 = 1;
    const PF_W: u32 = 2;
    const PF_R: u32 = 4;
    let find = || {
        let phoff = usize::try_from(u64_at(bytes, 32)?).ok()?;
        let entry = usize::from(u16_at(bytes, 54).ok()?);
        let count = usize::from(u16_at(bytes, 56).ok()?);
        if entry < 56 {
            return None;
        }
        for i in 0..count.min(64) {
            let at = phoff.checked_add(i.checked_mul(entry)?)?;
            let flags = u32_at(bytes, at.checked_add(4)?).ok()?;
            if u32_at(bytes, at).ok()? != PT_LOAD {
                continue;
            }
            let offset = u64_at(bytes, at.checked_add(8)?)?;
            let vaddr = u64_at(bytes, at.checked_add(16)?)?;
            let filesz = u64_at(bytes, at.checked_add(32)?)?;
            let memsz = u64_at(bytes, at.checked_add(40)?)?;
            // The segment that holds the anchor, whatever its flags.
            if anchor < vaddr || anchor - vaddr >= filesz {
                continue;
            }
            if flags & (PF_X | PF_W) != 0 || flags & PF_R == 0 {
                return None;
            }
            // `p_filesz` bytes come from the file, the rest up to `p_memsz` is
            // zeros: more file than memory is no segment, and the file bytes
            // must lie wholly inside the file.
            if filesz > memsz || offset.checked_add(filesz)? > u64::try_from(bytes.len()).ok()? {
                return None;
            }
            let len = filesz;
            let vaddr = usize::try_from(vaddr).ok()?;
            let len = usize::try_from(len).ok()?;
            base.checked_add(vaddr)?.checked_add(len)?;
            return Some((base + vaddr, len));
        }
        None
    };
    find().unwrap_or((0, 0))
}

// SAFETY requirements match load: authorized code and immutable checked inode.
unsafe fn checked_library(
    path: &Path,
    digest: &[u8; 32],
    host: &PpHost,
    services: &[paperpaw_native::metadata::Service<'_>],
    features: &'static [paperpaw_native::metadata::Feature<'static>],
) -> Result<CheckedLibrary, RuntimeError> {
    let mut file = std::fs::File::open(path).map_err(|_| RuntimeError::File)?;
    let mut bytes = Vec::new();
    // Bound allocations even for a malformed or replaced input file.
    std::io::Read::read_to_end(
        &mut std::io::Read::take(&mut file, (256 << 20) + 1),
        &mut bytes,
    )
    .map_err(|_| RuntimeError::File)?;
    if bytes.len() > 256 << 20 || paperpaw_core::sdk::digest::sha256::digest(&bytes) != *digest {
        return Err(RuntimeError::File);
    }
    // PR #133: a plugin needs no library, binds no symbol of the process and
    // exports nothing but its two entry symbols. One rule, shared with the packager.
    check_freestanding(&bytes, 62, PLUGIN_EXPORTS).map_err(|refusal| match refusal {
        FreestandingRefusal::UndefinedImport | FreestandingRefusal::NeededLibrary => {
            RuntimeError::Import
        }
        _ => RuntimeError::Export,
    })?;
    let locator =
        data_export(&bytes, 62, METADATA_SYMBOL, true).map_err(|_| RuntimeError::Export)?;
    let checked = checked_metadata_with(locator.bytes, host, services, features)?;
    let minor = checked.needs_minor;
    let declared_services = native_host::shell::DeclaredServices::try_from_metadata(
        &checked, features,
    )
    .map_err(|error| match error {
        native_host::shell::DeclaredServicesError::Invalid => RuntimeError::Metadata,
        native_host::shell::DeclaredServicesError::NoMemory => RuntimeError::NoMemory,
    })?;
    let entry = data_export(&bytes, 62, ENTRY_SYMBOL, false).map_err(|_| RuntimeError::Export)?;
    if entry.bytes.len() != wire::PROFILE_ENTRY_BYTES
        || entry.address % wire::PROFILE_ENTRY_ALIGN as u64 != 0
        || size_of::<PpEntry>() != wire::PROFILE_ENTRY_BYTES
    {
        return Err(RuntimeError::Entry);
    }
    // SAFETY: authorization and immutability are required above; both exports
    // were checked before the system loader can execute anything.
    let library = unsafe { HeldLibrary::open(path, file) }.map_err(|_| RuntimeError::Load)?;
    // SAFETY: global, defined object exports, bounded by their file symbol sizes.
    let (loaded_meta, loaded_entry) = unsafe {
        let meta = library
            .library
            .get::<*const u8>(format!("{METADATA_SYMBOL}\0").as_bytes())
            .map_err(|_| RuntimeError::Entry)?;
        let entry = library
            .library
            .get::<*const PpEntry>(format!("{ENTRY_SYMBOL}\0").as_bytes())
            .map_err(|_| RuntimeError::Entry)?;
        (*meta, *entry)
    };
    // SAFETY: locator's checked size and the entry's checked exact current size.
    let view = unsafe { std::slice::from_raw_parts(loaded_meta, locator.bytes.len()) };
    if view != locator.bytes
        || !(loaded_entry as usize).is_multiple_of(align_of::<PpEntry>())
        || (loaded_entry as usize).checked_sub(entry.address as usize)
            != (loaded_meta as usize).checked_sub(locator.address as usize)
    {
        return Err(RuntimeError::Entry);
    }
    // SAFETY: the export holds a current PpEntry, checked above.
    let entry = unsafe { &*loaded_entry };
    if entry.size as usize != size_of::<PpEntry>()
        || entry.abi_major != host.major
        || entry.needs_minor != minor
        || entry.metadata.ptr != loaded_meta
        || entry.metadata.len != view.len()
    {
        return Err(RuntimeError::Entry);
    }
    let make = entry.make.ok_or(RuntimeError::Entry)?;
    let image = read_only_segment(
        &bytes,
        (loaded_meta as usize).wrapping_sub(locator.address as usize),
        locator.address,
    );
    Ok(CheckedLibrary {
        declared_services,
        library,
        make,
        image,
    })
}

mod admit;
mod plugin_output;

// Separate from both CodeLoader and the process-owned inactive load path.
#[allow(
    dead_code,
    reason = "owner integration is inactive pending runtime review"
)]
mod owned;

pub(crate) use owned::dev::admission;
/// The developer switch's loader for interface-7 plugins (assignment #772),
/// off unless `--abi7-dev-loader` or `PAPERPAW_ABI7_DEV_LOADER=1` says so.
pub use owned::dev::{DevPlugins, Kind, Refusal, classify, enabled, set_enabled};

#[cfg(test)]
mod tests;
