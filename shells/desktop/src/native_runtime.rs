//! Inactive interface-7 desktop lifecycle (ADR 0034).
//!
//! This module qualifies the factory and destruction contract separately from
//! CodeLoader activation. Core registration, events and rendering are not wired.
//! Only the reviewed Linux x86-64 profile is supported.

use std::ffi::c_void;
use std::path::Path;
use std::ptr;

use paperpaw_native::host_glue::{ENTRY_SYMBOL, METADATA_SYMBOL, OK, PpEntry, PpHost, PpPlugin};
use paperpaw_native::preflight::data_export;

use crate::host_services::DesktopHost;
use crate::native::HeldLibrary;

#[allow(
    dead_code,
    reason = "all descriptor prefix offsets are generated together"
)]
mod wire {
    include!(concat!(env!("OUT_DIR"), "/metadata.rs"));
}

/// Refusal before factory invocation, or a failed factory result.
#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeError {
    /// The installed bytes no longer match their digest, or cannot be read.
    File,
    /// The ELF structures or exported objects are unsuitable.
    Export,
    /// The interface, profile, requirements or metadata structure is unsupported.
    Metadata,
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

/// M1 has no qualified service or feature catalogs. Reject every requirement
/// rather than treating a present but unsupported table slot as implemented.
fn metadata(bytes: &[u8], host: &PpHost) -> Result<u16, RuntimeError> {
    use wire::*;
    if bytes.len() > MAX_BYTES
        || bytes.get(..MAGIC_BYTES.len()) != Some(MAGIC_BYTES)
        || u16_at(bytes, LAYOUT)? != FORMAT
        || usize::from(u16_at(bytes, PREFIX_BYTES)?) != PREFIX_SIZE
        || u32_at(bytes, TOTAL_BYTES)? as usize != bytes.len()
        || u16_at(bytes, ABI_MAJOR)? != host.major
        || u16_at(bytes, NEEDS_MINOR)? == 0
        || u16_at(bytes, NEEDS_MINOR)? > host.minor
        || usize::from(u16_at(bytes, ENTRY_LAYOUT)?) != PROFILE_ENTRY_LAYOUT
        || usize::from(u16_at(bytes, ENTRY_BYTES)?) != PROFILE_ENTRY_BYTES
        || u32_at(bytes, FLAGS)? != 0
        || u16_at(bytes, SERVICES)? != 0
        || u16_at(bytes, FEATURES)? != 0
        || u16_at(bytes, EXPORTS)? != 1
    {
        return Err(RuntimeError::Metadata);
    }
    let target_end = PREFIX_SIZE + usize::from(u16_at(bytes, TARGET_BYTES)?);
    if bytes.get(PREFIX_SIZE..target_end) != Some(PROFILE) {
        return Err(RuntimeError::Metadata);
    }
    let record = bytes.get(target_end..).ok_or(RuntimeError::Metadata)?;
    if usize::from(u16_at(record, EXPORT_RECORD_BYTES)?) != record.len()
        || record.get(EXPORT_KIND) != Some(&1)
        || record.get(EXPORT_RESERVED) != Some(&0)
        || u32_at(record, EXPORT_EXPECTED_BYTES)? as usize != PROFILE_ENTRY_BYTES
        || usize::from(u16_at(record, EXPORT_NAME_BYTES)?) != ENTRY_SYMBOL.len()
        || record.get(EXPORT_SIZE..) != Some(ENTRY_SYMBOL.as_bytes())
    {
        return Err(RuntimeError::Metadata);
    }
    u16_at(bytes, NEEDS_MINOR)
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
    let locator =
        data_export(&bytes, 62, METADATA_SYMBOL, true).map_err(|_| RuntimeError::Export)?;
    let minor = metadata(locator.bytes, host.table())?;
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
        || entry.abi_major != host.table().major
        || entry.needs_minor != minor
        || entry.metadata.ptr != loaded_meta
        || entry.metadata.len != view.len()
    {
        return Err(RuntimeError::Entry);
    }
    let make = entry.make.ok_or(RuntimeError::Entry)?;
    let mut out = ptr::null();
    // SAFETY: table and context outlive the plugin; output storage is writable.
    let status = unsafe { make(host.table(), host.ctx(), &mut out) };
    if status != OK {
        return Err(RuntimeError::Factory(status));
    }
    if out.is_null() || !(out as usize).is_multiple_of(align_of::<PpPlugin>()) {
        return Err(RuntimeError::Plugin);
    }
    // SAFETY: successful factory supplies a live, readable size prefix.
    if unsafe { out.cast::<u32>().read() } < size_of::<PpPlugin>() as u32 {
        return Err(RuntimeError::Plugin);
    }
    // SAFETY: a current table is accessible as its size declares.
    let plugin = unsafe { &*out };
    Ok(LoadedPlugin {
        state: plugin.state,
        destroy: plugin.drop.ok_or(RuntimeError::Plugin)?,
        _library: library,
        _host: host,
    })
}

#[cfg(test)]
mod tests;
