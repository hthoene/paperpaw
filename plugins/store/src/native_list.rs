//! The streaming reading of the library's JSON (slice 3, host-free): the list
//! (`{"format": 2, "plugins": […], "sections": …, "revoked_keys": […]}`) and a
//! plugin's page (`{"format": 2, "plugin": {…}}`), fed in pieces of any size
//! with the fallible parser (`Parser::try_feed`): memory refused is `NoMemory`,
//! never an abort. The bytes come from the files facade or the network host
//! (pending); nothing here touches the host. The rules are
//! [`native_item`](crate::native_item)'s: long values are cut, not dropped.
use crate::native_item::{Device, File, Item, More, Rotation};
use crate::native_model::{Entry, FORMAT, Revocations, Revoked, SECTIONS, keep_in_section};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::digest::hex;
use paperpaw_sdk::json::stream::{Event, OutOfMemory, Parser};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{mem::push, text::copy};

/// What a list says beside its plugins: whether it is in a fair order (it has
/// `sections`), and the plugins of each section.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Picks {
    pub fair: bool,
    pub sections: [Vec<String>; SECTIONS.len()],
}

/// What the kernel and the card say of the plugins here, asked only when a
/// revocation could hold.
#[derive(Clone, Copy)]
pub struct Hooks<'a> {
    /// The key the device pins for the author of a plugin.
    pub author_of: &'a dyn Fn(&str) -> Option<[u8; 32]>,
    /// The plugins whose keys are already stopped.
    pub stopped: &'a dyn Fn() -> Vec<String>,
    /// The device does not say whose key a plugin is pinned to (interface 7: the
    /// host keeps the pins): a revocation of a plugin that is here is handed
    /// over whatever `author_of` says, and the host keeps only what holds.
    pub host_checks: bool,
}

impl core::fmt::Debug for Hooks<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Hooks").finish_non_exhaustive()
    }
}

/// A list read whole: its sections and the revocations to hand over (only of a
/// list this plugin reads).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct List {
    pub picks: Picks,
    pub revoked: Vec<Revoked>,
}

fn memory(_: OutOfMemory) -> Error {
    Error::NoMemory
}

impl core::fmt::Debug for ListReader<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ListReader")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl core::fmt::Debug for PageReader<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PageReader")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

/// Reads a list in pieces, handing each plugin that fits to the caller.
pub struct ListReader<'a> {
    device: Device<'a>,
    hooks: Hooks<'a>,
    parser: Parser,
    format: Option<u32>,
    item: Option<Item>,
    picks: Picks,
    revoked: Revocations,
    failed: Option<Error>,
}

impl<'a> ListReader<'a> {
    #[must_use]
    pub fn new(device: Device<'a>, hooks: Hooks<'a>) -> Self {
        Self {
            device,
            hooks,
            parser: Parser::default(),
            format: None,
            item: None,
            picks: Picks::default(),
            revoked: Revocations::default(),
            failed: None,
        }
    }

    /// The next piece of the text. `found` gets each plugin of this device.
    ///
    /// # Errors
    /// `NoMemory` when memory is refused; the reader is then done.
    pub fn feed(
        &mut self,
        data: &[u8],
        found: &mut dyn FnMut(Entry) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let Self {
            device,
            hooks,
            parser,
            format,
            item,
            picks,
            revoked,
            failed,
        } = self;
        parser
            .try_feed(data, &mut |event| {
                if failed.is_none() {
                    *failed = step(event, device, hooks, format, item, picks, revoked, found).err();
                }
            })
            .map_err(memory)?;
        failed.map_or(Ok(()), Err)
    }

    /// The text ends: the list, if it is one this plugin reads.
    ///
    /// # Errors
    /// `NoMemory` when memory is refused.
    #[inline(never)]
    pub fn finish(
        mut self,
        found: &mut dyn FnMut(Entry) -> Result<(), Error>,
    ) -> Result<Option<List>, Error> {
        let Self {
            device,
            hooks,
            parser,
            format,
            item,
            picks,
            revoked,
            failed,
        } = &mut self;
        parser
            .try_finish(&mut |event| {
                if failed.is_none() {
                    *failed = step(event, device, hooks, format, item, picks, revoked, found).err();
                }
            })
            .map_err(memory)?;
        if let Some(error) = failed.take() {
            return Err(error);
        }
        // Only a list this plugin reads is believed at all.
        if *format != Some(FORMAT) {
            return Ok(None);
        }
        let mut handed = Vec::new();
        for r in revoked.pending() {
            push(
                &mut handed,
                Revoked {
                    plugin: copy(&r.plugin)?,
                    key: r.key,
                    signature: r.signature,
                },
            )?;
        }
        Ok(Some(List {
            picks: core::mem::take(picks),
            revoked: handed,
        }))
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the reader's state, borrowed apart"
)]
#[inline(never)]
fn step(
    event: Event<'_>,
    device: &Device<'_>,
    hooks: &Hooks<'_>,
    format: &mut Option<u32>,
    item: &mut Option<Item>,
    picks: &mut Picks,
    revoked: &mut Revocations,
    found: &mut dyn FnMut(Entry) -> Result<(), Error>,
) -> Result<(), Error> {
    match event {
        Event::Value([key], value) if key == "format" => *format = value.parse::<u32>().ok(),
        Event::Value([list, at, field], value) if list == "revoked_keys" && at == "[]" => {
            revoked.field(field, value)?;
        }
        // After `format`, which a list names first: that it is a list this
        // plugin reads comes before anything it says is taken.
        Event::Close([list, at])
            if list == "revoked_keys" && at == "[]" && *format == Some(FORMAT) =>
        {
            revoked.entry_done(
                &|id| device.knows(id),
                (hooks.author_of, hooks.stopped, hooks.host_checks),
            )?;
        }
        Event::Open([key, ..]) if key == "sections" => picks.fair = true,
        Event::Value([key, section, at], value) if key == "sections" && at == "[]" => {
            picks.fair = true;
            if let Some(ids) = SECTIONS
                .iter()
                .position(|s| s == section)
                .and_then(|s| picks.sections.get_mut(s))
            {
                keep_in_section(ids, value)?;
            }
        }
        Event::Open([list, at]) if list == "plugins" && at == "[]" => *item = Some(Item::new()),
        Event::Close([list, at]) if list == "plugins" && at == "[]" => {
            if let Some(done) = item.take().filter(|item| item.fits(device)) {
                found(done.finish(device))?;
            }
        }
        Event::Close([list, at, rest @ ..])
            if list == "plugins" && at == "[]" && is_build(rest) =>
        {
            if let Some(item) = item.as_mut() {
                item.build_done(device)?;
            }
        }
        Event::Value([list, at, rest @ ..], value) if list == "plugins" && at == "[]" => {
            if let Some(item) = item.as_mut() {
                item.read(rest, value, device.name)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Whether `path` (below a plugin) is that of a build of `latest.builds`.
fn is_build(path: &[String]) -> bool {
    matches!(path, [latest, builds, at] if latest == "latest" && builds == "builds" && at == "[]")
}

/// Whether `path` (below a plugin) is that of a file of `latest.files`.
fn is_file(path: &[String]) -> bool {
    matches!(path, [latest, files, at] if latest == "latest" && files == "files" && at == "[]")
}

/// A plugin as its page at the library gives it: what it is, its files to
/// install (a code plugin's code among them), and the keys its author named.
/// Whether the files make a package the install accepts, and the addresses
/// relative to the page's, are the install's and the fetch's (`PORTING-7.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub entry: Entry,
    pub files: Vec<File>,
    /// It has code (a build for this device).
    pub code: bool,
    pub author_key: Option<[u8; 32]>,
    pub recovery_key: Option<[u8; 32]>,
    pub rotations: Vec<Rotation>,
    pub more: More,
}

/// Reads a plugin's page in pieces.
pub struct PageReader<'a> {
    device: Device<'a>,
    parser: Parser,
    format: Option<u32>,
    item: Item,
    failed: Option<Error>,
}

impl<'a> PageReader<'a> {
    #[must_use]
    pub fn new(device: Device<'a>) -> Self {
        Self {
            device,
            parser: Parser::default(),
            format: None,
            item: Item::new(),
            failed: None,
        }
    }

    /// The next piece of the text.
    ///
    /// # Errors
    /// `NoMemory` when memory is refused; the reader is then done.
    pub fn feed(&mut self, data: &[u8]) -> Result<(), Error> {
        let Self {
            device,
            parser,
            format,
            item,
            failed,
            ..
        } = self;
        parser
            .try_feed(data, &mut |event| {
                if failed.is_none() {
                    *failed = page_step(event, device, format, item).err();
                }
            })
            .map_err(memory)?;
        failed.map_or(Ok(()), Err)
    }

    /// The text ends: the plugin, if the page is one this plugin reads and it
    /// fits the device.
    ///
    /// # Errors
    /// `NoMemory` when memory is refused.
    #[inline(never)]
    pub fn finish(mut self) -> Result<Option<Page>, Error> {
        let Self {
            device,
            parser,
            format,
            item,
            failed,
        } = &mut self;
        parser
            .try_finish(&mut |event| {
                if failed.is_none() {
                    *failed = page_step(event, device, format, item).err();
                }
            })
            .map_err(memory)?;
        if let Some(error) = failed.take() {
            return Err(error);
        }
        if *format != Some(FORMAT) || !item.fits(device) {
            return Ok(None);
        }
        let mut item = core::mem::take(item);
        let mut files = core::mem::take(&mut item.files);
        let author_key = hex::decode_array::<32>(item.author_key.trim());
        let recovery_key = hex::decode_array::<32>(item.recovery_key.trim());
        let rotations = core::mem::take(&mut item.rotations);
        let mut more = core::mem::take(&mut item.more);
        if author_key.is_some() {
            more.author_key = lower_trimmed(&item.author_key)?;
        }
        let (entry, code_file) = item.finish_parts(device);
        let code = code_file.is_some();
        if let Some(code_file) = code_file {
            push(&mut files, code_file)?;
        }
        Ok(Some(Page {
            entry,
            files,
            code,
            author_key,
            recovery_key,
            rotations,
            more,
        }))
    }
}

fn lower_trimmed(text: &str) -> Result<String, Error> {
    let text = text.trim();
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    out.extend(text.chars().flat_map(char::to_lowercase));
    Ok(out)
}

#[inline(never)]
fn page_step(
    event: Event<'_>,
    device: &Device<'_>,
    format: &mut Option<u32>,
    item: &mut Item,
) -> Result<(), Error> {
    match event {
        Event::Value([key], value) if key == "format" => *format = value.parse::<u32>().ok(),
        Event::Close([plugin, rest @ ..]) if plugin == "plugin" && is_file(rest) => {
            item.file_done()?;
        }
        Event::Close([plugin, rest @ ..]) if plugin == "plugin" && is_build(rest) => {
            item.build_done(device)?;
        }
        Event::Close([plugin, shots, at])
            if plugin == "plugin" && shots == "screenshots" && at == "[]" =>
        {
            item.shot_done()?;
        }
        Event::Close([plugin, rotations, at])
            if plugin == "plugin" && rotations == "rotations" && at == "[]" =>
        {
            item.rotation_done()?;
        }
        Event::Value([plugin, rest @ ..], value) if plugin == "plugin" => {
            item.read(rest, value, device.name)?;
        }
        _ => {}
    }
    Ok(())
}
