//! The storage a `core.format` callback gives the parsers of this crate: the
//! book (and, for a `siblings` format, the files beside it) is root 3, and
//! everything below the plugin's own cache folder (`cache_prefix`, where the
//! interface-6 code keeps its conversions) is the document cache, root 6. The
//! host owns that cache per conversion, so the parsers' own stamps and
//! variants there are redundant but harmless.
use alloc::string::String;
use core::cmp::min;
use paperpaw_sdk::native::{
    Error, Host,
    storage::{self, Root},
};
use paperpaw_sdk::{DirEntry, Storage, StorageError};

/// The code a refused allocation travels as through `StorageError`.
pub(crate) const NO_MEMORY: u16 = 0x6e6d;

/// Roots 3 and 6 of the document being called, through the host.
pub struct NativeIo<'h, 'cb> {
    host: &'h Host<'cb>,
    prefix: String,
}

impl<'h, 'cb> NativeIo<'h, 'cb> {
    /// The roots of the call `host` belongs to; `cache_prefix` is the folder
    /// the parsers keep their conversions in.
    #[must_use]
    pub fn new(host: &'h Host<'cb>, cache_prefix: String) -> Self {
        Self {
            host,
            prefix: cache_prefix,
        }
    }
}

/// The suffix of the file a long write is staged in, beside the file. Not
/// the SDK's `STAGED_SUFFIX`: the host's calls refuse names ending in it.
const STAGED: &str = ".tmp";

impl NativeIo<'_, '_> {
    /// Writes `data` to `staged` in `root`, then puts it in the place of
    /// `name`.
    fn stage(&self, root: Root, staged: &str, name: &str, data: &[u8]) -> Result<(), StorageError> {
        // What a cut or a failure left would get the pieces after its own
        // bytes, and the rename would publish them.
        match storage::remove(self.host, root, staged) {
            Ok(()) | Err(Error::NotFound) => {}
            Err(error) => return Err(storage_error(error)),
        }
        for piece in data.chunks(storage::MAX_DATA) {
            storage::append(self.host, root, staged, piece).map_err(storage_error)?;
        }
        // The pieces reach the card before the rename that publishes them.
        self.sync()?;
        storage::rename(self.host, root, staged, name).map_err(storage_error)
    }
}

impl core::fmt::Debug for NativeIo<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NativeIo").finish_non_exhaustive()
    }
}

/// Which root `path` is in, and its name there.
pub(crate) fn route<'a>(prefix: &str, path: &'a str) -> (Root, &'a str) {
    match path.strip_prefix(prefix) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            (Root::DocumentCache, rest.trim_start_matches('/'))
        }
        _ => (Root::Book, path),
    }
}

pub(crate) fn storage_error(error: Error) -> StorageError {
    match error {
        Error::NotFound => StorageError::NotFound,
        Error::Full | Error::NoSpace => StorageError::Full,
        Error::Unsupported | Error::Denied => StorageError::Unsupported,
        Error::Invalid => StorageError::InvalidPath,
        // A refused allocation is not a damaged book: it keeps its name.
        Error::NoMemory => StorageError::Other(NO_MEMORY),
        _ => StorageError::Io,
    }
}

impl Storage for NativeIo<'_, '_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let (root, name) = route(&self.prefix, path);
        let mut done = 0;
        while done < buf.len() {
            let end = min(buf.len(), done + storage::MAX_DATA);
            let Some(part) = buf.get_mut(done..end) else {
                break;
            };
            let at = offset.saturating_add(done as u64);
            let n = storage::read_at(self.host, root, name, at, part).map_err(storage_error)?;
            done += n;
            if n < part.len() {
                break;
            }
        }
        Ok(done)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let (root, name) = route(&self.prefix, path);
        storage::size(self.host, root, name).map_err(storage_error)
    }

    /// A write of at most [`storage::MAX_DATA`] is the host's, which is
    /// atomic. A longer one is staged as the SDK's `DataStorage` stages it:
    /// appended to a file beside, flushed, and renamed over the file, so a
    /// power cut leaves the old file or the new one whole, never the first
    /// pieces of the new one.
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let (root, name) = route(&self.prefix, path);
        if root == Root::Book {
            return Err(StorageError::Unsupported);
        }
        if data.len() <= storage::MAX_DATA {
            return storage::write(self.host, root, name, data).map_err(storage_error);
        }
        let mut staged = String::new();
        staged
            .try_reserve_exact(name.len() + STAGED.len())
            .map_err(|_| StorageError::Other(NO_MEMORY))?;
        staged.push_str(name);
        staged.push_str(STAGED);
        let written = self.stage(root, &staged, name, data);
        if written.is_err() {
            // Best effort: the next long write of the file removes it first.
            let _ = storage::remove(self.host, root, &staged);
        }
        written
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let (root, name) = route(&self.prefix, path);
        if root == Root::Book {
            return Err(StorageError::Unsupported);
        }
        if data.is_empty() {
            return storage::append(self.host, root, name, data).map_err(storage_error);
        }
        for piece in data.chunks(storage::MAX_DATA) {
            storage::append(self.host, root, name, piece).map_err(storage_error)?;
        }
        Ok(())
    }

    /// Listing the book's folder needs `native::files` on root 3 (#384).
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    /// A host without a flush keeps nothing back (as the default says).
    fn sync(&self) -> Result<(), StorageError> {
        match storage::sync(self.host) {
            Ok(()) | Err(Error::Unsupported) => Ok(()),
            Err(error) => Err(storage_error(error)),
        }
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        let (root, name) = route(&self.prefix, path);
        if root == Root::Book {
            return Err(StorageError::Unsupported);
        }
        storage::remove(self.host, root, name).map_err(storage_error)
    }

    /// The cache the host gives a conversion starts empty and is the host's to
    /// clear: what the interface-6 code removes to start a book over is not
    /// there, and the book's own root is read only.
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        match route(&self.prefix, path).0 {
            Root::Book => Err(StorageError::Unsupported),
            _ => Ok(()),
        }
    }

    /// Within the document cache (root 6, which a format may write); the
    /// book's own root is read only, and a move between roots is none.
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let (root, from) = route(&self.prefix, from);
        let (to_root, to) = route(&self.prefix, to);
        if root == Root::Book || to_root != root {
            return Err(StorageError::Unsupported);
        }
        storage::rename(self.host, root, from, to).map_err(storage_error)
    }
}

/// Answers one `core.format` call of `provider` over the roots of the call
/// `host` belongs to: what each native plugin of this family does in its
/// `event`. The cache folder the parsers use is copied fallibly; an open of a
/// provider that applies the plugins' stylesheets is given them (root 7).
///
/// # Errors
/// What [`FormatProvider::call`](crate::provider::FormatProvider::call) says;
/// `NoMemory` where the folder's name cannot be copied.
pub fn serve(
    provider: &mut crate::provider::FormatProvider,
    host: &Host<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    if provider.reads_plugin_css() && crate::provider::FormatProvider::is_open(input) {
        provider.set_plugin_css(plugin_css(host)?);
    }
    let mut prefix = String::new();
    prefix
        .try_reserve_exact(provider.cache().len())
        .map_err(|_| Error::NoMemory)?;
    prefix.push_str(provider.cache());
    provider.call(&NativeIo::new(host, prefix), input, out)
}

/// The most of the plugins' stylesheets an open reads, as of one's own
/// (`document.rs` `MAX_USER_CSS`): what is beyond is left out.
const MAX_PLUGIN_CSS: u64 = 64 * 1024;

/// The stylesheets plugins bring for books, root 7 of the call `host`
/// belongs to, as one text: empty where no plugin has one or the host has no
/// root 7 (one older than draft 39). Bytes that are no UTF-8 are replaced.
///
/// # Errors
/// `NoMemory`; what reading root 7 says, but `NotFound` and `Unsupported`.
fn plugin_css(host: &Host<'_>) -> Result<String, Error> {
    let size = match storage::size(host, Root::PluginCss, "") {
        Ok(size) => size.min(MAX_PLUGIN_CSS),
        Err(Error::NotFound | Error::Unsupported) => return Ok(String::new()),
        Err(error) => return Err(error),
    };
    let mut bytes = alloc::vec::Vec::new();
    let len = usize::try_from(size).map_err(|_| Error::NoSpace)?;
    bytes.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    bytes.resize(len, 0);
    let mut done = 0;
    while let Some(part) = bytes.get_mut(done..).filter(|rest| !rest.is_empty()) {
        let end = min(part.len(), storage::MAX_DATA);
        let n = storage::read_at(host, Root::PluginCss, "", done as u64, &mut part[..end])?;
        if n == 0 {
            break;
        }
        done += n;
    }
    bytes.truncate(done);
    match String::from_utf8(bytes) {
        Ok(text) => Ok(text),
        Err(error) => match crate::fallible::lossy(error.as_bytes()) {
            Ok(alloc::borrow::Cow::Owned(text)) => Ok(text),
            Ok(alloc::borrow::Cow::Borrowed(text)) => {
                crate::fallible::string(text).map_err(|_| Error::NoMemory)
            }
            Err(_) => Err(Error::NoMemory),
        },
    }
}

/// As [`serve`], where a call that opens a document is first given the
/// values of the settings `keys` (full ids) of the format's plugin, read
/// from the host: the book's own where the host has a scope for it. A key
/// the host has no value for is left out.
///
/// # Errors
/// As [`serve`].
pub fn serve_with_settings(
    provider: &mut crate::provider::FormatProvider,
    host: &Host<'_>,
    input: &[u8],
    out: &mut [u8],
    keys: &[&str],
) -> Result<usize, Error> {
    use paperpaw_sdk::native::settings::{self, SettingValue};
    if crate::provider::FormatProvider::is_open(input) {
        let mut values = alloc::vec::Vec::new();
        values
            .try_reserve_exact(keys.len())
            .map_err(|_| Error::NoMemory)?;
        for key in keys {
            let mut scratch = [0_u8; 64];
            let text = match settings::read_setting(host, key, &mut scratch) {
                Ok(SettingValue::Bool(on)) => {
                    if on {
                        "true"
                    } else {
                        "false"
                    }
                }
                Ok(SettingValue::Text(text)) => text,
                Ok(SettingValue::Int(_) | SettingValue::Absent) | Err(_) => continue,
                Ok(_) => continue,
            };
            let (mut id, mut value) = (String::new(), String::new());
            id.try_reserve_exact(key.len())
                .map_err(|_| Error::NoMemory)?;
            value
                .try_reserve_exact(text.len())
                .map_err(|_| Error::NoMemory)?;
            id.push_str(key);
            value.push_str(text);
            values.push((id, value));
        }
        provider.set_settings(values);
    }
    serve(provider, host, input, out)
}
