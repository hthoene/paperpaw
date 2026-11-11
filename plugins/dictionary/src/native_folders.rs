//! Listing the dictionary folder through `core.files` (root 1).
use crate::declarations::DEFAULT_FOLDER;
use crate::{
    native_disk::is_stale_cache,
    native_model::{Item, Listing},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    files::{self, Files},
    settings::{self, SettingValue},
    storage::Root,
};
use paperpaw_ui_kit::{
    mem::{buffer, push},
    text::copy,
};

/// The setting of the folder.
const FOLDER_KEY: &str = "dictionary.folder";

/// The folder the setting names, or the default where it names none.
///
/// # Errors
/// The host's refusal of the read.
pub fn folder_setting<'b>(host: &Host<'_>, scratch: &'b mut [u8; 300]) -> Result<&'b str, Error> {
    match settings::read_setting(host, FOLDER_KEY, scratch)? {
        SettingValue::Text(text) if !text.trim_matches('/').is_empty() => Ok(text),
        _ => Ok(DEFAULT_FOLDER),
    }
}

/// A folder listing on the host's files; the service is opened for the walk and
/// released with [`Folders::finish`].
#[derive(Debug)]
pub struct Folders<'h, 'cb> {
    host: &'h Host<'cb>,
    files: Files,
}

impl<'h, 'cb> Folders<'h, 'cb> {
    /// Opens the files service.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn open(host: &'h Host<'cb>) -> Result<Self, Error> {
        Ok(Self {
            host,
            files: Files::open(host)?,
        })
    }

    /// The caches of dictionaries no longer in the folder go: the folders of
    /// 16 hexadecimal digits in the data folder that `kept` (paths of
    /// [`crate::native_disk::Routed`]) does not name. All of them are made
    /// again from their dictionaries when wanted. Best effort: a folder that
    /// cannot be removed stays, and the next start tries again.
    ///
    /// # Errors
    /// [`Error::NoMemory`], the listing's refusal.
    pub fn forget_caches(&mut self, kept: &[String]) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(files::INPUT_BYTES)?, buffer(files::OUTPUT_BYTES)?);
        let mut gone: Vec<String> = Vec::new();
        let mut cursor = 0;
        loop {
            let page = self
                .files
                .list(self.host, Root::Data, "", cursor, &mut input, &mut out)?;
            for entry in page.entries() {
                let name = entry.name();
                if entry.is_folder() && is_stale_cache(name, kept) {
                    push(&mut gone, copy(name)?)?;
                }
            }
            match page.next() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        for name in gone {
            let removed =
                self.files
                    .remove_tree(self.host, Root::Data, &name, &mut input, &mut out);
            if removed == Err(Error::NoMemory) {
                return Err(Error::NoMemory);
            }
        }
        Ok(())
    }

    /// Releases the service.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn finish(self) -> Result<(), Error> {
        self.files.release(self.host)
    }
}

impl Listing for Folders<'_, '_> {
    fn list(&mut self, dir: &str, each: &mut dyn FnMut(Item<'_>)) -> Result<(), Error> {
        // On the heap: the buffers are too big for the stack of a callback.
        let mut input = buffer(files::INPUT_BYTES)?;
        let mut out = buffer(files::OUTPUT_BYTES)?;
        let mut cursor = 0;
        loop {
            let page =
                self.files
                    .list(self.host, Root::Files, dir, cursor, &mut input, &mut out)?;
            for entry in page.entries() {
                if entry.unaddressable() {
                    continue;
                }
                each(Item {
                    name: entry.name(),
                    is_dir: entry.is_folder(),
                });
            }
            match page.next() {
                Some(next) => cursor = next,
                None => return Ok(()),
            }
        }
    }
}
