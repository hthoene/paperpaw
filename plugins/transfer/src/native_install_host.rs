//! The host's part of installing plugins sent from the computer: the files
//! staged in the data folder (`core.files` root 0), the plugins `core.registry`
//! lists, and the install the host is asked for (`command.install_plugin`).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::digest::sha256::Sha256;
use paperpaw_sdk::native::{
    Error, Host,
    commands::{CommandSender, InstallRoot},
    files::{self, Files},
    registry::{PageCursor, Registry, RegistryPlugin, RegistryPluginKind},
    storage::{self, Root},
};
use paperpaw_ui_kit::{
    files::replace,
    mem::{SERVICE_BUFFER, buffer},
    text::{concat, copy},
};

use crate::native_install::{Installs, Known, LISTING_FILE, Plugins, STAGING, Staged};

/// Pages of the registry read at most (32 plugins each).
const MOST_PAGES: usize = 16;
/// The list of the plugins is cut here (some 100 bytes a plugin).
const MOST_LISTING: usize = 24 * 1024;
/// Bytes of a file read a time while its SHA-256 is made.
const HASH_PART: usize = 2048;

/// [`Plugins`] on the host.
pub struct HostPlugins<'a, 'h> {
    host: &'a Host<'h>,
    files: &'a Files,
    registry: &'a mut Option<Registry>,
}

impl<'a, 'h> HostPlugins<'a, 'h> {
    pub fn new(host: &'a Host<'h>, files: &'a Files, registry: &'a mut Option<Registry>) -> Self {
        Self {
            host,
            files,
            registry,
        }
    }

    /// Hands every plugin to `take` until it says `false`; a list that changes
    /// while it is read is read again once.
    fn each(
        &mut self,
        take: &mut dyn FnMut(&RegistryPlugin<'_>) -> Result<bool, Error>,
    ) -> Result<(), Error> {
        let host = self.host;
        if self.registry.is_none() {
            *self.registry = Some(Registry::open(host)?);
        }
        let registry = self.registry.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        for attempt in 0..2 {
            let mut cursor = PageCursor::start();
            let mut stale = false;
            for _ in 0..MOST_PAGES {
                let page = match registry.plugins(host, cursor, &mut input, &mut out) {
                    Ok(page) => page,
                    Err(Error::State) if attempt == 0 => {
                        stale = true;
                        break;
                    }
                    Err(error) => return Err(error),
                };
                for plugin in page.rows() {
                    if !take(&plugin)? {
                        return Ok(());
                    }
                }
                match page.end().next_cursor() {
                    Some(next) => cursor = next,
                    None => return Ok(()),
                }
            }
            if !stale {
                return Ok(());
            }
        }
        Ok(())
    }

    /// The SHA-256 of the data file `path`, read a part at a time.
    fn sha256_of(&self, path: &str) -> Result<[u8; 32], Error> {
        let size = storage::size(self.host, Root::Data, path)?;
        let mut digest = Sha256::new();
        let mut part = buffer(HASH_PART)?;
        let mut at = 0u64;
        while at < size {
            let n = storage::read_at(self.host, Root::Data, path, at, &mut part)?;
            if n == 0 {
                return Err(Error::Invalid);
            }
            digest.update(part.get(..n).ok_or(Error::Invalid)?);
            at += n as u64;
        }
        Ok(digest.finish())
    }

    fn folder(id: &str) -> Result<String, Error> {
        concat(&[STAGING, "/", id])
    }
}

impl Plugins for HostPlugins<'_, '_> {
    fn known(&mut self, id: &str) -> Result<Known, Error> {
        let mut found = Known::Absent;
        self.each(&mut |plugin| {
            if plugin.id() != id {
                return Ok(true);
            }
            found = if plugin.kind() == RegistryPluginKind::BuiltIn {
                Known::BuiltIn
            } else {
                Known::Installed
            };
            Ok(false)
        })?;
        Ok(found)
    }

    fn write_listing(&mut self) -> Result<(), Error> {
        let mut json = String::new();
        json.try_reserve(SERVICE_BUFFER)
            .map_err(|_| Error::NoMemory)?;
        json.push_str("{\"plugins\":[");
        let mut first = true;
        self.each(&mut |plugin| {
            let version = if plugin.update().is_empty() {
                plugin.version()
            } else {
                plugin.update()
            };
            let origin = if plugin.kind() == RegistryPluginKind::BuiltIn {
                "built-in"
            } else {
                "installed"
            };
            let enabled = if plugin.flags() & 1 != 0 {
                "true"
            } else {
                "false"
            };
            let mut row = String::new();
            for part in [if first { "" } else { "," }, "{\"id\":"] {
                push(&mut row, part)?;
            }
            crate::native_web::push_json(&mut row, plugin.id())?;
            push(&mut row, ",\"version\":")?;
            crate::native_web::push_json(&mut row, version)?;
            for part in [",\"on\":", enabled, ",\"origin\":\"", origin, "\"}"] {
                push(&mut row, part)?;
            }
            // The list is cut where it would grow past what a device has.
            if json.len() + row.len() + 2 > MOST_LISTING {
                return Ok(false);
            }
            push(&mut json, &row)?;
            first = false;
            Ok(true)
        })?;
        push(&mut json, "]}")?;
        replace(self.host, Root::Data, LISTING_FILE, json.as_bytes())
    }

    fn stage(
        &mut self,
        id: &str,
        path: &str,
        body: &str,
        sha256: &[u8; 32],
    ) -> Result<Staged, Error> {
        match self.sha256_of(body) {
            Ok(found) if found == *sha256 => {}
            Ok(_) => return Ok(Staged::Damaged),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(Staged::Storage),
        }
        let target = concat(&[&Self::folder(id)?, "/", path])?;
        let (mut input, mut out) = (buffer(files::INPUT_BYTES)?, buffer(files::OUTPUT_BYTES)?);
        if let Some((dir, _)) = target.rsplit_once('/') {
            match self
                .files
                .make_dir(self.host, Root::Data, dir, &mut input, &mut out)
            {
                Ok(()) => {}
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => return Ok(Staged::Storage),
            }
        }
        let moved = self.files.move_entry(
            self.host,
            Root::Data,
            body,
            Root::Data,
            &target,
            true,
            &mut input,
            &mut out,
        );
        match moved {
            Ok(()) => Ok(Staged::Taken),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(Staged::Storage),
        }
    }

    fn discard(&mut self, path: &str) {
        // Nothing more to do where it cannot be removed: the host removes
        // what a server received when the server stops.
        let _ = storage::remove(self.host, Root::Data, path);
    }

    fn clear(&mut self, id: &str) {
        let Ok(folder) = Self::folder(id) else {
            return;
        };
        let (Ok(mut input), Ok(mut out)) =
            (buffer(files::INPUT_BYTES), buffer(files::OUTPUT_BYTES))
        else {
            return;
        };
        // A folder the host is copying is busy: the screen's next start removes it.
        let _ = self
            .files
            .remove_tree(self.host, Root::Data, &folder, &mut input, &mut out);
    }

    fn install(&mut self, request: u32, id: &str, update: bool) -> Result<(), Error> {
        let folder = Self::folder(id)?;
        let mut scratch = [0; 640];
        // An older build is allowed: the reader is told it is older (decision
        // 132's question), as a computer's own build may be.
        CommandSender::new(self.host, &mut scratch).install_plugin(
            request,
            InstallRoot::Data,
            &folder,
            update,
            true,
        )
    }
}

/// Removes what is staged in the data folder for no plugin that is being asked
/// about: the leftovers of a time of serving that did not end well.
///
/// # Errors
/// `NoMemory`; the host's refusals are not the caller's (a folder that is in
/// use stays for the next time).
pub fn remove_unknown_staging(
    host: &Host<'_>,
    files: &Files,
    installs: &Installs,
) -> Result<(), Error> {
    let (mut input, mut out) = (buffer(files::INPUT_BYTES)?, buffer(files::OUTPUT_BYTES)?);
    let mut names: Vec<String> = Vec::new();
    match files.list(host, Root::Data, STAGING, 0, &mut input, &mut out) {
        Ok(page) => {
            for entry in page.entries() {
                if !installs.is_asking(entry.name()) {
                    names.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    names.push(copy(entry.name())?);
                }
            }
        }
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        // Not there: nothing is staged.
        Err(_) => return Ok(()),
    }
    for name in names {
        let path = concat(&[STAGING, "/", &name])?;
        let removed = files.remove_tree(host, Root::Data, &path, &mut input, &mut out);
        if removed == Err(Error::NoMemory) {
            return Err(Error::NoMemory);
        }
    }
    Ok(())
}

fn push(out: &mut String, more: &str) -> Result<(), Error> {
    out.try_reserve(more.len()).map_err(|_| Error::NoMemory)?;
    out.push_str(more);
    Ok(())
}
