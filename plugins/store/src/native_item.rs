//! The reading of a plugin of the library's JSON (slices 2 and 3, host-free):
//! the keys of a plugin and of its `latest` version that make the index line,
//! say whether this device lists it and what its page adds (the files, the
//! build for this device and PaperPaw, the author's keys and key rotations,
//! the screenshots, the source and the description). The interface-6 rules,
//! made fallible: a refused allocation is `NoMemory`, and every text of the
//! library (nobody reviewed it, community plugins too) is **cut** at the
//! limit of its field, at a character's end, never dropped with its plugin;
//! only a line of the index is refused at a cap (see [`Entry::parse`]).
//! Nothing indexes text in a way that can panic.
//!
//! What the install does with the files (the package's validity, the paths, the
//! digests checked, the key pinned) stays with the install (`PORTING-7.md`).
use crate::native_model::{
    Abi, Entry, FAMILIES, Fit, MAX_NAME, MAX_SUMMARY, Needs, Provides, group,
};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Version;
use paperpaw_sdk::digest::hex;
use paperpaw_sdk::install::{fits_sdk, is_plugin_name};
use paperpaw_sdk::native::{Error, Fingerprint};
use paperpaw_ui_kit::{mem::push, text::copy};

/// Files a package may have at most (one more is kept, so that it shows).
pub const MAX_FILES: usize = paperpaw_sdk::install::MAX_FILES;
/// Screenshots of a plugin at most, and their longest side.
pub const MAX_SHOTS: usize = 4;
pub const MAX_SHOT_SIDE: u32 = 2560;
/// The longest *How it works* kept, in bytes.
pub const MAX_DESCRIPTION: usize = 4000;
/// The most steps of key rotations taken from the library at a time: an
/// install's batch holds the install, the author's keys and six steps
/// (`docs/abi/device.md` 5.2); a longer chain is cut, so the host ends the
/// install with reason 6 (key changed) rather than follow a part of it.
pub const MAX_STEPS: usize = 6;
/// The longest address kept of a source or a repository, and of a picture.
const MAX_ADDRESS: usize = 200;
const MAX_SHOT_URL: usize = 300;
/// The public key PaperPaw's own plugins are signed with.
pub const FIRST_PARTY_KEY: [u8; 32] = [
    0x14, 0x8c, 0xcf, 0x3e, 0xbc, 0x30, 0x50, 0x6b, 0x35, 0x5c, 0x6b, 0x5c, 0xb2, 0xba, 0xf8, 0x7d,
    0x92, 0x65, 0x60, 0x6c, 0xc6, 0xa8, 0x1f, 0x08, 0x5b, 0xb3, 0xb9, 0x74, 0x0a, 0x95, 0xae, 0xbd,
];

/// `text` cut to at most `max` bytes, at a character's end.
///
/// # Errors
/// `NoMemory` when the cut text cannot be kept.
pub fn cut(text: &str, max: usize) -> Result<String, Error> {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    copy(text.get(..end).unwrap_or_default())
}

/// A date of the library (`2026-03-14`, or a year) as `20260314` (a year as
/// its January 1st); 0 for any other.
#[must_use]
pub fn date(text: &str) -> u32 {
    let (mut n, mut value) = (0_u32, 0_u32);
    for digit in text.chars().filter_map(|c| c.to_digit(10)).take(8) {
        n += 1;
        value = value * 10 + digit;
    }
    match n {
        8 => value,
        4 => value * 10_000 + 101,
        _ => 0,
    }
}

/// `text` in lower case, kept without aborting.
fn lower(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    out.extend(text.chars().map(|c| c.to_ascii_lowercase()));
    Ok(out)
}

/// What a file is checked by, in lower-case hex digits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Digest {
    Sha1(String),
    Sha256(String),
}

impl Default for Digest {
    fn default() -> Self {
        Digest::Sha1(String::new())
    }
}

impl Digest {
    /// Takes the digest the library gives as `key` (`sha256` or `sha1`), in
    /// whichever order: SHA-256 where it gives both.
    ///
    /// # Errors
    /// `NoMemory` when the digits cannot be kept.
    pub fn take(&mut self, key: &str, hex: &str) -> Result<(), Error> {
        match key {
            "sha256" => *self = Digest::Sha256(lower(hex)?),
            "sha1" if !matches!(self, Digest::Sha256(kept) if !kept.is_empty()) => {
                *self = Digest::Sha1(lower(hex)?);
            }
            _ => {}
        }
        Ok(())
    }
}

/// One file of a plugin, as its page lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct File {
    pub path: String,
    pub url: String,
    pub size: u64,
    pub digest: Digest,
}

/// A build of a code plugin, as `latest.builds` lists them.
#[derive(Debug, Clone, Default)]
struct Build {
    device: String,
    abi: String,
    url: String,
    size: u64,
    sha256: String,
    image_len: Option<u64>,
    needs: Needs,
    /// The `families` read so far, whole only with eight.
    families: Vec<u16>,
}

/// A screenshot of a plugin, as its page lists them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shot {
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub size: u64,
}

/// What a plugin's page tells beyond the list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct More {
    pub source: String,
    /// A community plugin: the repository it comes from.
    pub repository: String,
    /// A code plugin: its author's key as the library names it.
    pub author_key: String,
    /// `yyyymmdd`, 0 where not said.
    pub author_since: u32,
    pub author_plugins: u32,
    /// `verified`, `unverified`, `failed`, or empty.
    pub verified: String,
    /// Bytes of memory it takes while running at most (`latest.heap`).
    pub heap: Option<u64>,
    /// The bytes the code of the build for this device takes where placed.
    pub image_len: Option<u64>,
    pub shots: Vec<Shot>,
    pub description: String,
}

/// The recovery key's statement that the plugin moves to key `to`. Only the
/// device decides whether it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rotation {
    pub to: [u8; 32],
    pub signature: [u8; 64],
}

/// What the device has of a plugin of the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Have {
    /// Comes with the firmware; switched on or off.
    BuiltIn {
        enabled: bool,
    },
    Data,
    Code,
}

/// What the device has of an id.
pub type HaveFn<'a> = dyn Fn(&str) -> Option<Have> + 'a;

/// The device that asks for the library's plugins.
#[derive(Clone, Copy)]
pub struct Device<'a> {
    pub name: &'a str,
    pub fingerprint: Option<Fingerprint>,
    /// The name a build's code gets, empty where none loads.
    pub code_file: &'a str,
    /// What is installed of an id; `None` where the device does not say (then
    /// everything may be installed).
    pub have: Option<&'a HaveFn<'a>>,
    /// The SDK of the kernel that runs, which data plugins must not be later
    /// than.
    pub sdk: Version,
    /// The list is PaperPaw's own library.
    pub official: bool,
    pub provides: Option<Provides>,
}

impl core::fmt::Debug for Device<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Device")
            .field("name", &self.name)
            .field("code_file", &self.code_file)
            .field("official", &self.official)
            .finish_non_exhaustive()
    }
}

impl Device<'_> {
    /// Whether the device has a plugin `id` at all (true where it does not
    /// say what it has).
    #[must_use]
    pub fn knows(&self, id: &str) -> bool {
        self.have.is_none_or(|have| have(id).is_some())
    }

    fn switched_off(&self, id: &str) -> bool {
        self.have
            .is_some_and(|have| have(id) == Some(Have::BuiltIn { enabled: false }))
    }

    /// Whether a plugin `id`, with code or without, may take the place of what
    /// the device has of that id: nothing, or a plugin of the same kind.
    fn may_install(&self, id: &str, code: bool) -> bool {
        self.have.is_none_or(|have| {
            have(id).is_none_or(|have| match have {
                Have::BuiltIn { .. } => false,
                Have::Data => !code,
                Have::Code => code,
            })
        })
    }
}

/// What is known of the plugin being read.
#[derive(Debug, Clone, Default)]
pub struct Item {
    pub entry: Entry,
    /// `data` or `code` (anything else is not offered).
    pub kind: String,
    pub built_in: bool,
    /// The library says it is PaperPaw's own.
    pub official: bool,
    /// `devices`, where the library names them: any, and this one.
    pub devices: bool,
    pub device: bool,
    /// The format of the plugin's data the library says it is in.
    pub formats: u32,
    /// What the library says it waits for: `author` or `update`.
    pub waits: String,
    /// The key its author signs its code with, and the recovery key it named,
    /// as the library says.
    pub author_key: String,
    pub recovery_key: String,
    rotation_to: String,
    rotation_signature: String,
    /// The steps read, in the order the library gives them.
    pub rotations: Vec<Rotation>,
    /// The files read so far (a plugin's page only), and the one being read.
    pub files: Vec<File>,
    file: File,
    build: Build,
    /// The build for this device and PaperPaw.
    pub code: Option<File>,
    /// A build for this device is there, but needs a newer PaperPaw.
    pub needs_newer: bool,
    pub more: More,
    shot: Shot,
}

fn https(value: &str) -> bool {
    value.starts_with("https://")
}

impl Item {
    /// A plugin of which nothing is read yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            formats: 1,
            ..Self::default()
        }
    }

    /// Takes what `key` (the path below the plugin) says; `device` is the
    /// name of the device that asks.
    ///
    /// # Errors
    /// `NoMemory` when a text cannot be kept; what was read stays.
    pub fn read<S: AsRef<str>>(
        &mut self,
        key: &[S],
        value: &str,
        device: &str,
    ) -> Result<(), Error> {
        let entry = &mut self.entry;
        match key {
            [key] => match key.as_ref() {
                "id" => entry.id = cut(value, MAX_NAME)?,
                "name" => entry.name = cut(value, MAX_NAME)?,
                "summary" => entry.summary = cut(value, MAX_SUMMARY)?,
                "author" => entry.author = cut(value, MAX_NAME)?,
                "licence" => entry.licence = cut(value, MAX_NAME)?,
                "group" => entry.group = group(value),
                "kind" => self.kind = cut(value, MAX_NAME)?,
                "waits" => self.waits = cut(value, MAX_NAME)?,
                "built_in" => self.built_in = value == "true",
                "official" => self.official = value == "true",
                "unreviewed" => entry.unreviewed = value == "true",
                "repository" if https(value) => {
                    self.more.repository = cut(value, MAX_ADDRESS)?;
                }
                "author_key" => self.author_key = cut(value, MAX_NAME)?,
                "recovery_key" => self.recovery_key = cut(value, MAX_NAME)?,
                "added" => entry.added = date(value),
                // Its home; `latest.source.url` (that version's source) goes
                // before it.
                "source" if https(value) && self.more.source.is_empty() => {
                    self.more.source = cut(value, MAX_ADDRESS)?;
                }
                "author_since" => self.more.author_since = date(value),
                "description" => self.more.description = cut(value, MAX_DESCRIPTION)?,
                "author_plugins" => self.more.author_plugins = value.parse().unwrap_or(0),
                _ => {}
            },
            [rotations, at, key] if rotations.as_ref() == "rotations" && at.as_ref() == "[]" => {
                match key.as_ref() {
                    "to" => self.rotation_to = cut(value, MAX_NAME)?,
                    "signature" => self.rotation_signature = cut(value, 2 * MAX_NAME)?,
                    _ => {}
                }
            }
            [latest, source, url]
                if latest.as_ref() == "latest"
                    && source.as_ref() == "source"
                    && url.as_ref() == "url" =>
            {
                if https(value) {
                    self.more.source = cut(value, MAX_ADDRESS)?;
                }
            }
            [shots, at, key] if shots.as_ref() == "screenshots" && at.as_ref() == "[]" => {
                match key.as_ref() {
                    "url" => self.shot.url = cut(value, MAX_SHOT_URL)?,
                    "width" => self.shot.width = value.parse().unwrap_or(0),
                    "height" => self.shot.height = value.parse().unwrap_or(0),
                    "size" => self.shot.size = value.parse().unwrap_or(0),
                    _ => {}
                }
            }
            [devices, at] if devices.as_ref() == "devices" && at.as_ref() == "[]" => {
                self.devices = true;
                self.device |= value == device;
            }
            [latest, key] if latest.as_ref() == "latest" => match key.as_ref() {
                "version" => entry.version = cut(value, MAX_NAME)?,
                "sdk" => entry.sdk = cut(value, MAX_NAME)?,
                "formats" => self.formats = value.parse().unwrap_or(u32::MAX),
                "size" => entry.size = value.parse().unwrap_or(0),
                "date" => entry.updated = date(value),
                "heap" => self.more.heap = value.parse().ok().filter(|&h| h > 0),
                // Code plugins only: the device's build was rebuilt from its
                // source and matches.
                "verified" if self.more.verified.is_empty() => {
                    self.more.verified = copy(if value == "true" {
                        "verified"
                    } else {
                        "unverified"
                    })?;
                }
                _ => {}
            },
            [latest, files, at, key]
                if latest.as_ref() == "latest"
                    && files.as_ref() == "files"
                    && at.as_ref() == "[]" =>
            {
                let file = &mut self.file;
                match key.as_ref() {
                    "path" => file.path = copy(value)?,
                    "url" => file.url = copy(value)?,
                    "size" => file.size = value.parse().unwrap_or(0),
                    key => file.digest.take(key, value)?,
                }
            }
            [latest, builds, at, rest @ ..]
                if latest.as_ref() == "latest"
                    && builds.as_ref() == "builds"
                    && at.as_ref() == "[]" =>
            {
                self.read_build(rest, value)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Takes what `key` (the path below a build) says.
    fn read_build<S: AsRef<str>>(&mut self, key: &[S], value: &str) -> Result<(), Error> {
        let build = &mut self.build;
        match key {
            [key] => match key.as_ref() {
                "device" => build.device = cut(value, MAX_NAME)?,
                "abi" => build.abi = cut(value, MAX_NAME)?,
                "url" => build.url = copy(value)?,
                "size" => build.size = value.parse().unwrap_or(0),
                "sha256" => build.sha256 = lower(value)?,
                "image_len" => build.image_len = value.parse().ok(),
                "needs_api" => build.needs.api = value.parse().ok(),
                "rebuilt" if value == "differs" => self.more.verified = copy("failed")?,
                _ => {}
            },
            // More than eight is odd: one more is kept, so that it shows.
            [key, item]
                if key.as_ref() == "families"
                    && item.as_ref() == "[]"
                    && build.families.len() <= FAMILIES =>
            {
                push(&mut build.families, value.parse().unwrap_or(u16::MAX))?;
            }
            _ => {}
        }
        Ok(())
    }

    /// A file of `latest.files` was read whole (one more than a package may
    /// have is kept, so that it is refused).
    ///
    /// # Errors
    /// `NoMemory` when it cannot be kept.
    pub fn file_done(&mut self) -> Result<(), Error> {
        let file = core::mem::take(&mut self.file);
        if self.files.len() <= MAX_FILES {
            push(&mut self.files, file)?;
        }
        Ok(())
    }

    /// A build of `latest.builds` was read whole: kept as the plugin's code if
    /// it is for this device and PaperPaw; one that needs a newer PaperPaw is
    /// noted.
    ///
    /// # Errors
    /// `NoMemory` when it cannot be kept.
    pub fn build_done(&mut self, device: &Device<'_>) -> Result<(), Error> {
        let mut build = core::mem::take(&mut self.build);
        let abi = Abi::parse(&build.abi);
        let right = build.device == device.name
            && !device.code_file.is_empty()
            && abi.is_some_and(|abi| abi.is_for(device.fingerprint));
        if !right || self.code.is_some() {
            return Ok(());
        }
        // The interface and the minor are the `abi`'s own.
        build.needs.interface = abi.map(Abi::interface);
        build.needs.minor = abi.and_then(Abi::minor);
        // Eight revisions, each a number: else the list is not read.
        build.needs.families = <[u16; FAMILIES]>::try_from(build.families.as_slice())
            .ok()
            .filter(|f| !f.contains(&u16::MAX));
        match device.provides.map_or(Fit::Yes, |p| p.fit(&build.needs)) {
            Fit::Yes => {
                self.more.image_len = build.image_len;
                self.code = Some(File {
                    path: copy(device.code_file)?,
                    url: build.url,
                    size: build.size,
                    digest: Digest::Sha256(build.sha256),
                });
            }
            Fit::Newer => self.needs_newer = true,
            Fit::Older => {}
        }
        Ok(())
    }

    /// One step of `rotations` is read: kept if it is whole, and not more than
    /// [`MAX_STEPS`].
    ///
    /// # Errors
    /// `NoMemory` when it cannot be kept.
    pub fn rotation_done(&mut self) -> Result<(), Error> {
        let to = hex::decode_array::<32>(self.rotation_to.trim());
        let signature = hex::decode_array::<64>(self.rotation_signature.trim());
        self.rotation_to.clear();
        self.rotation_signature.clear();
        if let Some((to, signature)) = to.zip(signature)
            && self.rotations.len() < MAX_STEPS
        {
            push(&mut self.rotations, Rotation { to, signature })?;
        }
        Ok(())
    }

    /// A screenshot of `screenshots` was read whole: kept if PaperPaw decodes
    /// it and it is not too large to decode here.
    ///
    /// # Errors
    /// `NoMemory` when it cannot be kept.
    pub fn shot_done(&mut self) -> Result<(), Error> {
        let shot = core::mem::take(&mut self.shot);
        let path = shot.url.split(['?', '#']).next().unwrap_or("");
        let readable = [".png", ".jpg", ".jpeg"].iter().any(|end| {
            path.len()
                .checked_sub(end.len())
                .and_then(|at| path.as_bytes().get(at..))
                .is_some_and(|tail| tail.eq_ignore_ascii_case(end.as_bytes()))
        });
        let small = shot.width <= MAX_SHOT_SIDE && shot.height <= MAX_SHOT_SIDE;
        if self.more.shots.len() < MAX_SHOTS && readable && small {
            push(&mut self.more.shots, shot)?;
        }
        Ok(())
    }

    /// Whether no build for this device runs here but one needs a newer
    /// PaperPaw: listed, with that said, nothing to install.
    fn waits_for_newer(&self) -> bool {
        self.code.is_none()
            && (self.needs_newer || self.waits == "update" || self.waits_for_author())
    }

    fn waits_for_author(&self) -> bool {
        self.waits == "author"
    }

    /// Whether this device can install it: a data plugin, or one with code
    /// built for it, that takes the place of none of its own.
    #[must_use]
    pub fn fits(&self, device: &Device<'_>) -> bool {
        let entry = &self.entry;
        // A built-in plugin switched off is offered to be switched on, with or
        // without a file for this device.
        let off = device.switched_off(&entry.id);
        let kind = match self.kind.as_str() {
            "data" => true,
            "code" => self.code.is_some() || self.waits_for_newer() || off,
            _ => false,
        };
        let code = self.code.is_some() || self.waits_for_newer();
        kind && !self.built_in
            && (off || device.may_install(&entry.id, code))
            && (!self.devices || self.device)
            && is_plugin_name(&entry.id)
            && !entry.name.is_empty()
            && Version::parse(&entry.version).is_some()
            && fits_sdk(device.sdk, &entry.sdk, self.formats)
    }

    /// The plugin as listed: its code's size counted in.
    #[must_use]
    pub fn finish(self, device: &Device<'_>) -> Entry {
        self.finish_parts(device).0
    }

    /// As [`Item::finish`], handing over the build's code file too (moved, not
    /// copied: a copy would allocate where memory may be refused).
    #[must_use]
    pub fn finish_parts(mut self, device: &Device<'_>) -> (Entry, Option<File>) {
        if let Some(code) = &self.code {
            self.entry.code = true;
            self.entry.size = self.entry.size.saturating_add(code.size);
        } else if self.needs_newer || self.waits == "update" {
            self.entry.code = true;
            self.entry.needs_newer = true;
        } else if self.waits_for_author() {
            self.entry.code = true;
            self.entry.waits_author = true;
        }
        // PaperPaw's, where its own library says so (code signed with its key,
        // as the library names it, which the install checks).
        let signed = !self.entry.code
            || hex::decode_array::<32>(self.author_key.trim()) == Some(FIRST_PARTY_KEY);
        self.entry.official = self.official && device.official && signed;
        // PaperPaw's own are reviewed, whatever else is said.
        self.entry.unreviewed &= !self.entry.official;
        (self.entry, self.code)
    }
}
