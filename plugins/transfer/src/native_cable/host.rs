//! The host's part of the cable: the plugin's data folder for what is staged and
//! the offer, `core.files` for the room, `core.registry` for the plugins,
//! `core.device` for what the device is, `core.library` to put a book in, and
//! `core.ui` for the question to the reader.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    device::Device,
    files::{self, Files},
    formatting::Arg,
    library::{Existing, Import, Library},
    registry::Registry,
    storage::{self, Root},
    ui::{Question, Ui},
};
use paperpaw_ui_kit::{
    Messages,
    files::Volume,
    mem::{SERVICE_BUFFER, buffer},
    text::{concat, copy, decimal_u64, format},
};

use super::{CableHost, Imported, STAGING, read_offer};
use crate::native_install::{Installs, Known, Plugins, Staged};
use crate::native_install_host::HostPlugins;
use crate::native_transfer::Offer;

/// Bytes of an inline answer the plugin list may take (an inline body holds
/// 2,048).
const LISTING_LIMIT: usize = 2000;
/// What the host's question holds at most (`core.ui` `ask`: 1,024 bytes).
const QUESTION_TEXT: usize = 1000;
/// Room kept at the end of the question's text for the count of the books
/// that are not named.
const MORE_ROOM: usize = 100;

/// The data folder as [`Volume`], for reading the offer a line at a time.
struct DataVolume<'a, 'h> {
    host: &'a Host<'h>,
}

impl Volume for DataVolume<'_, '_> {
    fn size(&self, name: &str) -> Result<u64, Error> {
        storage::size(self.host, Root::Data, name)
    }
    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        storage::read_at(self.host, Root::Data, name, at, out)
    }
    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        storage::write(self.host, Root::Data, name, data)
    }
    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        storage::append(self.host, Root::Data, name, data)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        storage::rename(self.host, Root::Data, from, to)
    }
    fn remove(&self, name: &str) -> Result<(), Error> {
        storage::remove(self.host, Root::Data, name)
    }
    fn sync(&self) -> Result<(), Error> {
        storage::sync(self.host)
    }
}

/// [`CableHost`] on the host.
pub struct HostCable<'a, 'h> {
    host: &'a Host<'h>,
    files: &'a Files,
    device: &'a Device,
    ui: &'a Ui,
    library: Option<&'a Library>,
    plugins: HostPlugins<'a, 'h>,
    installs: &'a Installs,
}

impl<'a, 'h> HostCable<'a, 'h> {
    /// The cable over the host's handles; the plugins are staged in the
    /// cable's own folder.
    #[must_use]
    pub fn new(
        host: &'a Host<'h>,
        handles: Handles<'a>,
        registry: &'a mut Option<Registry>,
        installs: &'a Installs,
    ) -> Self {
        Self {
            host,
            files: handles.files,
            device: handles.device,
            ui: handles.ui,
            library: handles.library,
            plugins: HostPlugins::new(host, handles.files, registry).staging_in(STAGING),
            installs,
        }
    }
}

/// The service handles the cable's host part uses.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct Handles<'a> {
    pub files: &'a Files,
    pub device: &'a Device,
    pub ui: &'a Ui,
    /// Where `core.library` is not there or not granted, books are refused
    /// (*501*).
    pub library: Option<&'a Library>,
}

impl CableHost for HostCable<'_, '_> {
    fn hello(&mut self) -> Result<String, Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let (id, name) = {
            let identity = self.device.device(self.host, &mut input, &mut out)?;
            (copy(identity.id())?, copy(identity.model())?)
        };
        // The firmware's version, else the PaperPaw this plugin was built with.
        let version = match self.device.firmware(self.host, &mut input, &mut out) {
            Ok(firmware) => copy(firmware.version())?,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => format(format_args!("{}", paperpaw_sdk::SDK_VERSION))?,
        };
        let mut json = String::new();
        json.try_reserve(160).map_err(|_| Error::NoMemory)?;
        json.push_str("{\"protocol\":1,\"device\":");
        crate::native_web::push_json(&mut json, &id)?;
        json.push_str(",\"name\":");
        crate::native_web::push_json(&mut json, &name)?;
        json.push_str(",\"version\":");
        crate::native_web::push_json(&mut json, &version)?;
        // Interface 7 has no fingerprint to give: what a code plugin must be
        // built for is `core.device` `native` (`PORTING-7.md`).
        json.try_reserve(24).map_err(|_| Error::NoMemory)?;
        json.push_str(",\"fingerprint\":null}");
        Ok(json)
    }

    fn listing(&mut self, skip: usize) -> Result<String, Error> {
        self.plugins.inline_listing(skip, LISTING_LIMIT)
    }

    fn read_offer(&mut self, body: &str) -> Result<Option<Offer>, Error> {
        read_offer(&DataVolume { host: self.host }, body)
    }

    fn free_space(&mut self) -> Result<Option<u64>, Error> {
        let (mut input, mut out) = (buffer(files::INPUT_BYTES)?, buffer(files::OUTPUT_BYTES)?);
        let space = self
            .files
            .space(self.host, Root::Files, &mut input, &mut out)?;
        Ok(space.free())
    }

    fn wifi_is_sent(&mut self, id: &str) -> bool {
        self.installs.holds(id)
    }

    fn ask(&mut self, offer: &Offer) -> Result<u32, Error> {
        let mut messages = Messages::new()?;
        let (title, text) = question(self.host, &mut messages, offer)?;
        let yes = messages.copy(self.host, "transfer.cable.allow")?;
        // Only the yes word is ours: the dialog's own *No* stays.
        let labels = concat(&[&yes, "\n"])?;
        let asked = Question::confirm(&title, &text)?.confirm_labels(&labels)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        Ok(self.ui.ask(self.host, asked, &mut input, &mut out)?.get())
    }

    fn forget(&mut self, id: &str) {
        self.plugins.clear(id);
    }

    fn stage(
        &mut self,
        id: &str,
        path: &str,
        body: &str,
        sha256: &[u8; 32],
    ) -> Result<Staged, Error> {
        self.plugins.stage(id, path, body, sha256)
    }

    fn staged_files(&mut self, id: &str, limit: usize) -> Result<Vec<String>, Error> {
        let top = concat(&[STAGING, "/", id])?;
        let (mut input, mut out) = (buffer(files::INPUT_BYTES)?, buffer(files::OUTPUT_BYTES)?);
        let mut found: Vec<String> = Vec::new();
        // The folders still to read, as paths below the plugin's folder.
        let mut folders: Vec<String> = Vec::new();
        folders.try_reserve(1).map_err(|_| Error::NoMemory)?;
        folders.push(String::new());
        while let Some(below) = folders.pop() {
            let dir = if below.is_empty() {
                copy(&top)?
            } else {
                concat(&[&top, "/", &below])?
            };
            let mut cursor = 0;
            loop {
                let page =
                    match self
                        .files
                        .list(self.host, Root::Data, &dir, cursor, &mut input, &mut out)
                    {
                        Ok(page) => page,
                        Err(Error::NotFound) => break,
                        Err(error) => return Err(error),
                    };
                for entry in page.entries() {
                    let path = if below.is_empty() {
                        copy(entry.name())?
                    } else {
                        concat(&[&below, "/", entry.name()])?
                    };
                    let list = if entry.is_folder() {
                        &mut folders
                    } else {
                        &mut found
                    };
                    list.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    list.push(path);
                    if found.len() >= limit || folders.len() > limit {
                        return Ok(found);
                    }
                }
                match page.next() {
                    Some(next) => cursor = next,
                    None => break,
                }
            }
        }
        Ok(found)
    }

    fn staged_matches(
        &mut self,
        id: &str,
        path: &str,
        size: u64,
        sha256: &[u8; 32],
    ) -> Result<bool, Error> {
        let at = concat(&[STAGING, "/", id, "/", path])?;
        match storage::size(self.host, Root::Data, &at) {
            Ok(found) if found == size => {}
            Ok(_) | Err(Error::NotFound) => return Ok(false),
            Err(error) => return Err(error),
        }
        match self.plugins.sha256_of(&at) {
            Ok(found) => Ok(found == *sha256),
            Err(Error::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn known(&mut self, id: &str) -> Result<Known, Error> {
        self.plugins.known(id)
    }

    fn install(&mut self, request: u32, id: &str, update: bool) -> Result<(), Error> {
        self.plugins.install(request, id, update)
    }

    fn import(&mut self, body: &str, name: &str, sha256: &[u8; 32]) -> Result<Imported, Error> {
        match self.plugins.sha256_of(body) {
            Ok(found) if found == *sha256 => {}
            Ok(_) => return Ok(Imported::Damaged),
            Err(error) => return Err(error),
        }
        let (mut input, mut out) = (buffer(2048)?, buffer(SERVICE_BUFFER)?);
        // A free name, never over a book: `name (2).epub`.
        let file = Import::new(body, name).existing(Existing::FreeName);
        let library = self.library.ok_or(Error::Unsupported)?;
        let path = library.import(self.host, &file, &mut input, &mut out)?;
        Ok(Imported::Taken(copy(path)?))
    }
}

/// What the cable's folders hold at the start and the end of a screen: all of
/// it goes (except what the host is copying, which stays busy and goes at the
/// next start).
///
/// # Errors
/// `NoMemory`.
pub fn clean(host: &Host<'_>, files: &Files) -> Result<(), Error> {
    let (mut input, mut out) = (buffer(files::INPUT_BYTES)?, buffer(files::OUTPUT_BYTES)?);
    match files.remove_tree(host, Root::Data, STAGING, &mut input, &mut out) {
        Err(Error::NoMemory) => Err(Error::NoMemory),
        _ => Ok(()),
    }
}

/// The question about `offer` and the names and sizes below it: the title and
/// the text, the text at most [`QUESTION_TEXT`] bytes (the books that do not fit
/// are counted).
fn question(
    host: &Host<'_>,
    messages: &mut Messages,
    offer: &Offer,
) -> Result<(String, String), Error> {
    let key = match (offer.plugin.is_some(), offer.books.is_empty()) {
        (true, true) => "transfer.cable.ask-plugin",
        (true, false) => "transfer.cable.ask-both",
        (false, _) => "transfer.cable.ask-books",
    };
    let title = messages.copy(host, key)?;
    let mark = messages
        .copy(host, "core.decimal")
        .ok()
        .filter(|mark| !mark.is_empty())
        .map_or_else(|| copy("."), Ok)?;
    let mut text = String::new();
    text.try_reserve(QUESTION_TEXT)
        .map_err(|_| Error::NoMemory)?;
    if let Some((id, files)) = &offer.plugin {
        let total = files
            .iter()
            .fold(0u64, |sum, (_, s, _)| sum.saturating_add(*s));
        let size = messages.digits(host, &size_text(total, &mark)?)?;
        let line = messages.format(
            host,
            "transfer.cable.plugin",
            &[
                Arg::Text {
                    name: "name",
                    value: id,
                },
                Arg::Text {
                    name: "size",
                    value: &size,
                },
            ],
        )?;
        add_line(&mut text, &line, QUESTION_TEXT);
        if offer.code() {
            let code = messages.copy(host, "transfer.cable.code")?;
            add_line(&mut text, &code, QUESTION_TEXT);
        }
    }
    let mut shown = 0usize;
    for (name, size) in &offer.named {
        let size = messages.digits(host, &size_text(*size, &mark)?)?;
        let line = concat(&[name, " (", &size, ")"])?;
        if !add_line(&mut text, &line, QUESTION_TEXT - MORE_ROOM) {
            break;
        }
        shown += 1;
    }
    let more = offer.books.len().saturating_sub(shown);
    if more > 0 {
        let line = messages.format(
            host,
            "transfer.cable.more",
            &[Arg::Int {
                name: "count",
                value: more as i64,
            }],
        )?;
        add_line(&mut text, &line, QUESTION_TEXT);
    }
    Ok((title, text))
}

/// Adds `line` on a line of its own where the text stays within `limit` bytes
/// (the room was reserved): `false` where it does not fit.
fn add_line(text: &mut String, line: &str, limit: usize) -> bool {
    let extra = line.len() + usize::from(!text.is_empty());
    if text.len() + extra > limit || text.try_reserve(extra).is_err() {
        return false;
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(line);
    true
}

/// A size as people read it: `820 B`, `14 KB`, `2,3 MB`, with whole numbers
/// only (no floating point): the unit is the first where the size is below
/// 1,000, a tenth shows below 10.
///
/// # Errors
/// `NoMemory`.
pub fn size_text(bytes: u64, mark: &str) -> Result<String, Error> {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let wide = u128::from(bytes);
    let (mut unit, mut scale) = (0usize, 1u128);
    while wide >= 1000 * scale && unit + 1 < UNITS.len() {
        unit += 1;
        scale *= 1024;
    }
    let mut digits = [0; 20];
    let mut out = String::new();
    let mut push = |text: &str| -> Result<(), Error> {
        out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
        out.push_str(text);
        Ok(())
    };
    if unit == 0 || wide >= 10 * scale {
        let whole = (wide * 2 + scale) / (2 * scale);
        push(decimal_u64(
            u64::try_from(whole).unwrap_or(u64::MAX),
            &mut digits,
        ))?;
    } else {
        let tenths = u64::try_from((wide * 20 + scale) / (2 * scale)).unwrap_or(u64::MAX);
        push(decimal_u64(tenths / 10, &mut digits))?;
        push(mark)?;
        push(decimal_u64(tenths % 10, &mut digits))?;
    }
    push(" ")?;
    push(UNITS.get(unit).copied().unwrap_or("B"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::size_text;

    #[test]
    fn sizes_are_read_as_people_do() {
        assert_eq!(size_text(0, ",").unwrap(), "0 B");
        assert_eq!(size_text(820, ",").unwrap(), "820 B");
        assert_eq!(size_text(999, ",").unwrap(), "999 B");
        assert_eq!(size_text(1000, ",").unwrap(), "1,0 KB");
        assert_eq!(size_text(14 * 1024, ".").unwrap(), "14 KB");
        assert_eq!(size_text(2_400_000, ",").unwrap(), "2,3 MB");
        assert_eq!(size_text(u64::MAX, ".").unwrap(), "17179869184 GB");
    }
}
