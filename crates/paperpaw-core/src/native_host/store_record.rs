//! The host's record of the versions a store installed (`registry.md`
//! `reg_plugin.flags` bit 5; `device.md` 5.2 `install_plugin` bit 2): the
//! file of interface 6 (`<id>\t<release>` lines, the newest last;
//! `paperpaw_sdk::install::Origins`), out of reach of plugins and of the card
//! transfers. An install from a catalog adds its version when it is in place
//! (`install::marks::settle`), any other forgets the id's; a device that kept
//! none takes the plugins installed for the store's once ([`adopt`]), as
//! interface 6's store did.
use alloc::vec::Vec;
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sdk::{PluginKind, Plugins, Version};

/// Where the record is (interface 6's `data_path!("plugin-origin.txt")`).
pub(crate) const STORE_INSTALLS: &str = ".paperpaw/plugin-origin.txt";
/// The longest record read (as the SDK's reader).
const RECORD_MAX: usize = 64 * 1024;
/// The longest line kept for a plugin: an id (64), a tab, a version (32).
const LINE_MAX: usize = 100;
/// Versions the record keeps of one plugin (interface 6 `VERSIONS_KEPT`).
const VERSIONS_KEPT: usize = 4;

/// The record, read once for a call.
pub(crate) struct StoreRecord(Vec<u8>);

impl StoreRecord {
    /// The record of a device with `plugins` plugins: at most what it
    /// holds for them ([`VERSIONS_KEPT`] lines each), its newest lines (the
    /// end of the file) where it is longer. None kept, a card that cannot
    /// read it or no memory for it is an empty record (no plugin counts as a
    /// store's, as the SDK's reader takes it): the plugin list never fails
    /// for it.
    pub(crate) fn read(storage: &dyn Storage, plugins: usize) -> Self {
        let Ok(size) = storage.size(STORE_INSTALLS) else {
            return Self(Vec::new());
        };
        let cap = plugins
            .max(1)
            .saturating_mul(VERSIONS_KEPT * LINE_MAX)
            .min(RECORD_MAX);
        let size = usize::try_from(size).unwrap_or(usize::MAX);
        let (from, len) = (size.saturating_sub(cap), size.min(cap));
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(len).is_err() {
            return Self(Vec::new());
        }
        bytes.resize(len, 0);
        let mut filled = 0;
        while filled < len {
            match storage.read_at(STORE_INSTALLS, (from + filled) as u64, &mut bytes[filled..]) {
                Ok(0) | Err(_) => break,
                Ok(read) => filled += read,
            }
        }
        bytes.truncate(filled);
        // A line cut at the start of the piece read is not one.
        if from > 0 {
            let start = bytes
                .iter()
                .position(|&b| b == b'\n')
                .map_or(bytes.len(), |at| at + 1);
            bytes.drain(..start);
        }
        Self(bytes)
    }

    /// The release of `plugin` a store installed with the numbers of
    /// `version`, as the store named it (`1.2.0-beta.1`): the last kept.
    pub(crate) fn release(&self, plugin: &str, version: Version) -> Option<&str> {
        self.0.rsplit(|&byte| byte == b'\n').find_map(|line| {
            let line = core::str::from_utf8(line).ok()?;
            let (id, written) = line.split_once('\t')?;
            let written = written.trim();
            (id == plugin && Version::parse(written) == Some(version)).then_some(written)
        })
    }
}

/// Where no record was ever kept on this device, every plugin installed
/// (data or code, not one built in) is taken for the store's at its version,
/// once: before PaperPaw kept them the store's installs went unnamed, and
/// they keep its updates (interface 6 `install::adopt`). Not at start: the
/// first that needs the record calls it, before a plugin install or removal
/// commits and before `reg_plugin` rows say bit 5 ("installers and those who
/// look at origins call it first"). Without room or card the next one does.
/// `true` where the record is kept (now or before); `false` where it could
/// not be written, or the card could not say whether there is one (a record
/// is never written over).
#[inline(never)]
pub(crate) fn adopt(storage: &dyn Storage, plugins: &Plugins) -> bool {
    match storage.size(STORE_INSTALLS) {
        Ok(_) => return true,
        Err(StorageError::NotFound) => {}
        Err(_) => return false,
    }
    let mut text = Vec::new();
    for info in plugins.iter() {
        if !matches!(info.kind, PluginKind::Data | PluginKind::Code) {
            continue;
        }
        let mut line = Line::default();
        let version = info.update.unwrap_or(info.manifest.version);
        if core::fmt::write(&mut line, format_args!("{}\t{version}\n", info.manifest.id)).is_err()
            || text.try_reserve(line.len).is_err()
        {
            return false;
        }
        text.extend_from_slice(&line.bytes[..line.len]);
    }
    let _ = storage.create_dir(paperpaw_sdk::data::DIR);
    storage.write(STORE_INSTALLS, &text).is_ok()
}

/// One line of the record, on the stack.
struct Line {
    bytes: [u8; LINE_MAX],
    len: usize,
}

impl Default for Line {
    fn default() -> Self {
        Self {
            bytes: [0; LINE_MAX],
            len: 0,
        }
    }
}

impl core::fmt::Write for Line {
    fn write_str(&mut self, part: &str) -> core::fmt::Result {
        let end = self.len + part.len();
        self.bytes
            .get_mut(self.len..end)
            .ok_or(core::fmt::Error)?
            .copy_from_slice(part.as_bytes());
        self.len = end;
        Ok(())
    }
}

#[cfg(test)]
#[path = "store_record_tests.rs"]
pub(in crate::native_host) mod tests;
