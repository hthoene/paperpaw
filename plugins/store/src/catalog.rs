//! The plugin library's list and its plugins (ADR 0023, format 2), read
//! as they stream from the files they were saved to.
//!
//! The list (`plugins.json?device=x4&lang=de&order=fair`) holds only the
//! plugins for this device, in the order to show them (by group, then in
//! the library's fair order of the day, or by name in the language asked
//! for where the library knows no fair order), without their files:
//!
//! ```json
//! {"format": 2, "plugins": [{
//!   "id": "airy-theme", "name": "Luftig", "summary": "…", "group": "look",
//!   "author": "…", "licence": "Apache-2.0", "kind": "data",
//!   "latest": {"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 2345}}]}
//! ```
//!
//! A library with a fair order says so with `sections`: the plugins it
//! picks for a start (`recommended`, `new`, `trending`, `updated`,
//! `discover`: lists of ids). Without them the list is by name, and the
//! store shows its start as before.
//!
//! A code plugin is `"kind": "code"`, with `latest.builds` (each a
//! `device`, the `abi` it was built for, `url`, `size`, `sha256`) and the
//! `author_key` its author signs with; it is listed where a build fits
//! this device and PaperPaw.
//!
//! With the setting *Community plugins* on, the store asks for them too
//! (`community=1`): plugins published by their authors from GitHub, which
//! nobody reviewed, each marked `"unreviewed": true` with its
//! `repository`. The store says so wherever it shows one, and refuses an
//! update of one signed by another key (`trust`).
//!
//! `"official": true` marks PaperPaw's own plugins. It is taken only from
//! PaperPaw's own library ([`OFFICIAL_LIST`], over HTTPS), and for a code
//! plugin only where its `author_key` is PaperPaw's ([`FIRST_PARTY_KEY`]):
//! a list made elsewhere cannot give the mark, whatever it says.
//!
//! The plugins that fit go to an index on the storage: a line each, and
//! where each line begins (4 bytes each). In memory are only the runs of
//! plugins of one group, so the list may grow without limit. A plugin's
//! page asks for it with its files (`plugins/<id>.json`, the same fields
//! and `latest.files`, with the list's own query), read when it comes.
//!
//! A plugin of the library is passed over where the device has a plugin
//! of that id it cannot take the place of ([`may_install`]): one built
//! in, or one of the other kind.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::install::{self, Digest, MAX_FILES, Package, PackageFile};
use paperpaw_sdk::json::stream::{Event, read_file};
use paperpaw_sdk::native::Fingerprint;
use paperpaw_sdk::web::{encode, join_url};
use paperpaw_sdk::{Env, PluginKind, Plugins, Storage, Version};

use crate::PLUGIN;
use crate::needs::{Fit, Needs, Provides};
use crate::revoked::Revocations;
use crate::trust::{Named, Rotation};

mod runs;

pub(crate) use runs::RUN;
use runs::{Cursor, KEYED, Keyed, Output, before, order_of};

/// The format of the list and of a plugin's page this plugin reads.
const FORMAT: u32 = 2;
/// The longest summary kept, in bytes, and the longest name and other
/// short texts.
/// The largest files the store takes from the library: the list of
/// plugins, a plugin's page, a picture of it.
pub(crate) const MAX_LIST_FILE: u64 = 4 * 1024 * 1024;
pub(crate) const MAX_PAGE_FILE: u64 = 512 * 1024;
pub(crate) const MAX_SHOT_FILE: u64 = 2 * 1024 * 1024;
const MAX_SUMMARY: usize = 300;
const MAX_NAME: usize = 80;
/// The longest line of the index read.
const MAX_LINE: usize = 1024;
/// The index is written in pieces of about this many bytes.
const WRITE_AT: usize = 2048;
/// Runs of plugins of one group at most: a list in the website's order
/// has one a group; plugins of a list out of order beyond these go with
/// the last run.
const MAX_RUNS: usize = 32;

/// PaperPaw's own plugin library: the only list whose word that a plugin
/// is PaperPaw's is taken.
pub(crate) const OFFICIAL_LIST: &str = "https://paperpaw.org/api/v1/plugins.json";

/// The public key PaperPaw's own plugins are signed with (the first-party
/// plugin key of its releases).
pub(crate) const FIRST_PARTY_KEY: [u8; 32] = [
    0x14, 0x8c, 0xcf, 0x3e, 0xbc, 0x30, 0x50, 0x6b, 0x35, 0x5c, 0x6b, 0x5c, 0xb2, 0xba, 0xf8, 0x7d,
    0x92, 0x65, 0x60, 0x6c, 0xc6, 0xa8, 0x1f, 0x08, 0x5b, 0xb3, 0xb9, 0x74, 0x0a, 0x95, 0xae, 0xbd,
];

/// The plugins a library picks for a start (its `sections`), in the order
/// the start shows them.
pub(crate) const SECTIONS: [&str; 5] = ["recommended", "new", "trending", "updated", "discover"];

/// The plugins of a section kept at most.
const MAX_SECTION: usize = 20;

/// The groups, in the order the website shows them (its `catalogue.rs`,
/// and paperpaw-dev's `package/folder.rs`: keep them in step). `system`
/// stays last: it takes a plugin of a group this plugin does not know.
pub(crate) const GROUPS: [&str; 13] = [
    "reading",
    "formats",
    "library",
    "sync",
    "home",
    "look",
    "notes",
    "learning",
    "news",
    "tools",
    "games",
    "languages",
    "system",
];

/// The group of `name`, the last (`system`) for one this plugin does not
/// know.
pub(crate) fn group(name: &str) -> usize {
    GROUPS
        .iter()
        .position(|g| *g == name)
        .unwrap_or(GROUPS.len() - 1)
}

/// What the store installs for: the device as plugin libraries call it,
/// where PaperPaw loads code plugins the build they must be made for and
/// the name their code gets, and the plugins it has.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Device<'a> {
    pub name: &'a str,
    pub fingerprint: Option<Fingerprint>,
    pub code_file: &'a str,
    pub plugins: Option<&'a Plugins>,
    /// The SDK of the kernel that runs, which data plugins must not be
    /// later than.
    pub sdk: Version,
    /// The list is PaperPaw's own library ([`OFFICIAL_LIST`]).
    pub official: bool,
    /// What its firmware provides code plugins (`None` where it does not
    /// say: every build for it fits, as before).
    pub provides: Option<Provides>,
}

impl<'a> Device<'a> {
    /// This device, if plugin libraries know it.
    pub(crate) fn of(env: &Env<'a>) -> Option<Device<'a>> {
        Some(Device {
            name: env.catalog_device()?,
            fingerprint: env.native().map(|native| native.fingerprint),
            code_file: env.native().map_or("", |native| &native.file),
            plugins: Some(env.plugins()),
            sdk: env
                .plugins()
                .kernel()
                .map_or(paperpaw_sdk::SDK_VERSION, |kernel| kernel.manifest.version),
            official: env.settings().read(&crate::CATALOG).trim() == OFFICIAL_LIST,
            provides: env.native().map(Provides::of),
        })
    }
}

/// Whether a plugin `id` of the library, with code or without, may take
/// the place of what `plugins` has of that id: nothing, or a plugin of
/// the same kind installed before. A built-in plugin comes only with the
/// firmware, and one kind does not replace the other.
pub(crate) fn may_install(plugins: &Plugins, id: &str, code: bool) -> bool {
    plugins.get(id).is_none_or(|info| match info.kind {
        PluginKind::BuiltIn => false,
        PluginKind::Data => !code,
        PluginKind::Code => code,
        _ => false,
    })
}

/// Whether `id` is a built-in plugin that is switched off here (Android
/// builds the store's file plugins in, off until the reader wants them):
/// it is switched on, never downloaded.
pub(crate) fn switched_off(plugins: &Plugins, id: &str) -> bool {
    plugins
        .get(id)
        .is_some_and(|info| info.kind == PluginKind::BuiltIn && !info.enabled)
}

/// A plugin the library offers for this device: a data plugin, or a code
/// plugin with a build for this PaperPaw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Entry {
    pub id: String,
    /// In the UI's language where the library has it.
    pub name: String,
    pub summary: String,
    /// Index into [`GROUPS`].
    pub group: usize,
    pub author: String,
    pub licence: String,
    pub version: String,
    /// The PaperPaw (its SDK) it is made for, e.g. `0.4`.
    pub sdk: String,
    /// It has code, which runs with full access to the device.
    pub code: bool,
    /// One of PaperPaw's own: the library says so, and its author's key
    /// is PaperPaw's.
    pub official: bool,
    /// A community plugin, which nobody reviewed: the library says so.
    pub unreviewed: bool,
    /// Its newest build is made for a newer PaperPaw than this device
    /// runs (it has a table or a family of shared functions the firmware
    /// lacks): not offered, and what is installed is kept as it is.
    pub needs_newer: bool,
    /// The library has builds of it for this device, but none for the
    /// PaperPaw that runs (`waits`: its author has to build it): listed,
    /// nothing to install, and what is installed is kept as it is.
    pub waits_author: bool,
    /// Bytes to download, its code with them.
    pub size: u64,
    /// When it was first published and when its newest version, as
    /// `yyyymmdd` (0 where the library does not say).
    pub added: u32,
    pub updated: u32,
}

impl Entry {
    /// The version installed, if it is (a code plugin's update waiting for
    /// the restart counts).
    pub(crate) fn installed(&self, env: &Env<'_>) -> Option<Version> {
        env.plugins()
            .get(&self.id)
            .map(|info| info.update.unwrap_or(info.manifest.version))
    }

    /// Whether it is a built-in plugin switched off here: *Switch on* is
    /// what the store offers for it.
    pub(crate) fn switched_off(&self, env: &Env<'_>) -> bool {
        switched_off(env.plugins(), &self.id)
    }

    /// Its line in the index, tabs and line ends of the list left out.
    fn line(&self) -> String {
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            self.group,
            self.size,
            u8::from(self.code)
                | (u8::from(self.official) << 1)
                | (u8::from(self.unreviewed) << 2)
                | (u8::from(self.needs_newer) << 3)
                | (u8::from(self.waits_author) << 4),
            self.added,
            self.updated,
            clean(&self.id),
            clean(&self.version),
            clean(&self.sdk),
            clean(&self.author),
            clean(&self.licence),
            clean(&self.name),
            clean(&self.summary)
        )
    }

    fn parse(line: &str) -> Option<Entry> {
        let mut f = line.splitn(12, '\t');
        let group = f.next()?.parse().ok()?;
        let size = f.next()?.parse().ok()?;
        let flags: u8 = f.next()?.parse().ok()?;
        Some(Entry {
            group,
            size,
            code: flags & 1 != 0,
            official: flags & 2 != 0,
            unreviewed: flags & 4 != 0,
            needs_newer: flags & 8 != 0,
            waits_author: flags & 16 != 0,
            added: f.next()?.parse().ok()?,
            updated: f.next()?.parse().ok()?,
            id: f.next()?.to_string(),
            version: f.next()?.to_string(),
            sdk: f.next()?.to_string(),
            author: f.next()?.to_string(),
            licence: f.next()?.to_string(),
            name: f.next()?.to_string(),
            summary: f.next()?.to_string(),
        })
    }
}

/// A build of a code plugin, as `latest.builds` lists them.
#[derive(Debug, Clone, Default)]
struct Build {
    device: String,
    abi: String,
    url: String,
    size: u64,
    sha256: String,
    /// The bytes its image takes where the device places it (its
    /// header's), where the library says.
    image_len: Option<u64>,
    /// What it needs of the firmware, where the library says.
    needs: Needs,
    /// The `families` read so far, whole only with eight.
    families: Vec<u16>,
}

/// What is known of the plugin being read.
#[derive(Default)]
struct Item {
    entry: Entry,
    kind: String,
    built_in: bool,
    /// The library says it is PaperPaw's own (see [`Entry::official`]).
    official: bool,
    /// `devices`, where the library names them: any, and this one.
    devices: bool,
    device: bool,
    formats: u32,
    /// The key its author signs its code with, as the library says.
    author_key: String,
    /// The recovery key its author named, as the library says.
    recovery_key: String,
    /// The step of `rotations` being read: the key it moves to (`to`) and
    /// the recovery key's signature of that, both hexadecimal.
    rotation_to: String,
    rotation_signature: String,
    /// The steps read, in the order the library gives them.
    rotations: Vec<Rotation>,
    /// The files read so far (a plugin's page only), and the one being
    /// read.
    files: Vec<PackageFile>,
    file: PackageFile,
    /// The build being read, and the one for this device and PaperPaw.
    build: Build,
    code: Option<PackageFile>,
    /// A build for this device is there, but needs a newer PaperPaw.
    needs_newer: bool,
    /// What the library says it `waits` for: `author` or `update`.
    waits: String,
    /// What only a plugin's page says.
    more: More,
    shot: Shot,
}

/// A screenshot of a plugin, as its page lists them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Shot {
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub size: u64,
}

/// Screenshots of a plugin at most, and their longest side.
pub(crate) const MAX_SHOTS: usize = 4;
const MAX_SHOT_SIDE: u32 = 2560;

/// What a plugin's page tells beyond the list: where its source is, since
/// when its author publishes, whether its build was checked against its
/// source, screenshots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct More {
    pub source: String,
    /// A community plugin: the repository it comes from.
    pub repository: String,
    /// A code plugin: its author's key, as the library names it, in
    /// hexadecimal.
    pub author_key: String,
    /// `yyyymmdd`, 0 where not said.
    pub author_since: u32,
    pub author_plugins: u32,
    /// `verified`, `unverified`, `failed`, or empty.
    pub verified: String,
    /// Bytes of memory it takes while running at most, where its author
    /// measured it (`latest.heap`).
    pub heap: Option<u64>,
    /// The bytes the code of the build for this device takes where it is
    /// placed (`latest.builds[].image_len`), where the library says.
    pub image_len: Option<u64>,
    pub shots: Vec<Shot>,
    /// How it works, at length (`description`), where the library says.
    pub description: String,
}

/// The longest *How it works* kept, in characters.
const MAX_DESCRIPTION: usize = 4000;

/// `yyyymmdd` of a date written `YYYY-MM-DD` (or a year alone), 0 for
/// anything else.
pub(crate) fn date(text: &str) -> u32 {
    let digits: String = text.chars().filter(char::is_ascii_digit).take(8).collect();
    match digits.len() {
        8 => digits.parse().unwrap_or(0),
        4 => digits.parse::<u32>().map_or(0, |y| y * 10000 + 101),
        _ => 0,
    }
}

impl Item {
    fn new() -> Self {
        Self {
            formats: 1,
            ..Self::default()
        }
    }

    /// Whether no build for this device runs here but one needs a newer
    /// PaperPaw: listed, with that said, nothing to install.
    fn waits_for_newer(&self) -> bool {
        self.code.is_none()
            && (self.needs_newer || self.waits == "update" || self.waits_for_author())
    }

    /// Whether the library has no build of it for the PaperPaw that runs
    /// yet, and says its author has to make one (`waits`, asked with
    /// `abi`).
    fn waits_for_author(&self) -> bool {
        self.waits == "author"
    }

    /// Whether this device can install it: a data plugin, or one with code
    /// built for it, that takes the place of none of its own.
    fn fits(&self, device: &Device<'_>) -> bool {
        let entry = &self.entry;
        // A built-in plugin switched off is offered to be switched on,
        // with or without a file for this device.
        let off = device
            .plugins
            .is_some_and(|plugins| switched_off(plugins, &entry.id));
        let kind = match self.kind.as_str() {
            "data" => true,
            "code" => self.code.is_some() || self.waits_for_newer() || off,
            _ => false,
        };
        let code = self.code.is_some() || self.waits_for_newer();
        kind && !self.built_in
            && (off
                || device
                    .plugins
                    .is_none_or(|plugins| may_install(plugins, &entry.id, code)))
            && (!self.devices || self.device)
            && install::is_plugin_name(&entry.id)
            && !entry.name.is_empty()
            && Version::parse(&entry.version).is_some()
            && install::fits_sdk(device.sdk, &entry.sdk, self.formats)
    }

    /// Takes what `key` (the path below the plugin) says.
    fn read(&mut self, key: &[String], value: &str, device: &Device<'_>) {
        let entry = &mut self.entry;
        match key {
            [key] => match key.as_str() {
                "id" => entry.id = cut(value, MAX_NAME),
                "name" => entry.name = cut(value, MAX_NAME),
                "summary" => entry.summary = cut(value, MAX_SUMMARY),
                "author" => entry.author = cut(value, MAX_NAME),
                "licence" => entry.licence = cut(value, MAX_NAME),
                "group" => entry.group = group(value),
                "kind" => self.kind = cut(value, MAX_NAME),
                "waits" => self.waits = cut(value, MAX_NAME),
                "built_in" => self.built_in = value == "true",
                "official" => self.official = value == "true",
                "unreviewed" => entry.unreviewed = value == "true",
                "repository" if value.starts_with("https://") => {
                    self.more.repository = cut(value, 200);
                }
                "author_key" => self.author_key = cut(value, MAX_NAME),
                "recovery_key" => self.recovery_key = cut(value, MAX_NAME),
                "added" => entry.added = date(value),
                // Its home; `latest.source.url` (that version's source)
                // goes before it.
                "source" if value.starts_with("https://") && self.more.source.is_empty() => {
                    self.more.source = cut(value, 200);
                }
                "author_since" => self.more.author_since = date(value),
                "description" => self.more.description = cut(value, MAX_DESCRIPTION),
                "author_plugins" => self.more.author_plugins = value.parse().unwrap_or(0),
                _ => {}
            },
            [rotations, at, key] if rotations == "rotations" && at == "[]" => match key.as_str() {
                "to" => self.rotation_to = cut(value, MAX_NAME),
                "signature" => self.rotation_signature = cut(value, 2 * MAX_NAME),
                _ => {}
            },
            [latest, source, url] if latest == "latest" && source == "source" && url == "url" => {
                if value.starts_with("https://") {
                    self.more.source = cut(value, 200);
                }
            }
            [shots, at, key] if shots == "screenshots" && at == "[]" => match key.as_str() {
                "url" => self.shot.url = cut(value, 300),
                "width" => self.shot.width = value.parse().unwrap_or(0),
                "height" => self.shot.height = value.parse().unwrap_or(0),
                "size" => self.shot.size = value.parse().unwrap_or(0),
                _ => {}
            },
            [devices, at] if devices == "devices" && at == "[]" => {
                self.devices = true;
                self.device |= value == device.name;
            }
            [latest, key] if latest == "latest" => match key.as_str() {
                "version" => entry.version = cut(value, MAX_NAME),
                "sdk" => entry.sdk = cut(value, MAX_NAME),
                "formats" => self.formats = value.parse().unwrap_or(u32::MAX),
                "size" => entry.size = value.parse().unwrap_or(0),
                "date" => entry.updated = date(value),
                "heap" => self.more.heap = value.parse().ok().filter(|&h| h > 0),
                // Code plugins only: the device's build was rebuilt from
                // its source and matches.
                "verified" if self.more.verified.is_empty() => {
                    self.more.verified = if value == "true" {
                        "verified"
                    } else {
                        "unverified"
                    }
                    .into();
                }
                _ => {}
            },
            [latest, files, at, key] if latest == "latest" && files == "files" && at == "[]" => {
                let file = &mut self.file;
                match key.as_str() {
                    "path" => file.path = value.to_string(),
                    "url" => file.url = value.to_string(),
                    "size" => file.size = value.parse().unwrap_or(0),
                    key => {
                        file.digest.take(key, value);
                    }
                }
            }
            [latest, builds, at, rest @ ..]
                if latest == "latest" && builds == "builds" && at == "[]" =>
            {
                self.read_build(rest, value);
            }
            _ => {}
        }
    }

    /// Takes what `key` (the path below a build) says.
    fn read_build(&mut self, key: &[String], value: &str) {
        let build = &mut self.build;
        match key {
            [key] => match key.as_str() {
                "device" => build.device = cut(value, MAX_NAME),
                "abi" => build.abi = cut(value, MAX_NAME),
                "url" => build.url = value.to_string(),
                "size" => build.size = value.parse().unwrap_or(0),
                "sha256" => build.sha256 = value.to_ascii_lowercase(),
                "image_len" => build.image_len = value.parse().ok(),
                "needs_api" => build.needs.api = value.parse().ok(),
                "rebuilt" if value == "differs" => self.more.verified = "failed".into(),
                _ => {}
            },
            // More than eight is odd: one more is kept, so that it shows.
            [key, item]
                if key == "families"
                    && item == "[]"
                    && build.families.len() <= crate::needs::FAMILIES =>
            {
                build.families.push(value.parse().unwrap_or(u16::MAX));
            }
            _ => {}
        }
    }

    /// A file of `latest.files` was read whole (one more than a package
    /// may have is kept, so that it is refused).
    fn file_done(&mut self) {
        let file = core::mem::take(&mut self.file);
        if self.files.len() <= MAX_FILES {
            self.files.push(file);
        }
    }

    /// A build of `latest.builds` was read whole: kept as the plugin's
    /// code if it is for this device and PaperPaw; one that needs a newer
    /// PaperPaw is noted (see [`Entry::needs_newer`]).
    fn build_done(&mut self, device: &Device<'_>) {
        let mut build = core::mem::take(&mut self.build);
        let right = build.device == device.name
            && !device.code_file.is_empty()
            && device
                .fingerprint
                .is_some_and(|f| Fingerprint::parse(&build.abi) == Some(f));
        if !right || self.code.is_some() {
            return;
        }
        // The interface is the fingerprint's own (the library's `interface`
        // field says the same, and is not taken twice).
        build.needs.interface = Fingerprint::parse(&build.abi).map(Fingerprint::interface);
        // Eight revisions, each a number: else the list is not read.
        build.needs.families = <[u16; crate::needs::FAMILIES]>::try_from(build.families.as_slice())
            .ok()
            .filter(|f| !f.contains(&u16::MAX));
        match device.provides.map_or(Fit::Yes, |p| p.fit(&build.needs)) {
            Fit::Yes => {
                self.more.image_len = build.image_len;
                self.code = Some(PackageFile::new(
                    device.code_file.to_string(),
                    build.url,
                    build.size,
                    Digest::Sha256(build.sha256),
                ));
            }
            Fit::Newer => self.needs_newer = true,
            Fit::Older => {}
        }
    }

    /// A screenshot of `screenshots` was read whole.
    /// One step of `rotations` is read: kept if it is whole, and not more
    /// than [`trust::MAX_STEPS`].
    fn rotation_done(&mut self) {
        let step = key(&self.rotation_to).zip(signature(&self.rotation_signature));
        self.rotation_to.clear();
        self.rotation_signature.clear();
        if let Some((to, signature)) = step
            && self.rotations.len() < crate::trust::MAX_STEPS
        {
            self.rotations.push(Rotation { to, signature });
        }
    }

    fn shot_done(&mut self) {
        let shot = core::mem::take(&mut self.shot);
        // The library keeps them as their authors made them: only those
        // PaperPaw decodes, and not too large to decode here.
        let path = shot
            .url
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let readable = [".png", ".jpg", ".jpeg"]
            .iter()
            .any(|end| path.ends_with(end));
        let small = shot.width <= MAX_SHOT_SIDE && shot.height <= MAX_SHOT_SIDE;
        if self.more.shots.len() < MAX_SHOTS && readable && small {
            self.more.shots.push(shot);
        }
    }

    /// The plugin as listed: its code's size counted in.
    fn finish(mut self, device: &Device<'_>) -> Entry {
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
        // PaperPaw's, where its own library says so (code signed with its
        // key, as the library names it, which the install checks).
        let signed = !self.entry.code || key(&self.author_key) == Some(FIRST_PARTY_KEY);
        self.entry.official = self.official && device.official && signed;
        // PaperPaw's own are reviewed, whatever else is said.
        self.entry.unreviewed &= !self.entry.official;
        self.entry
    }
}

/// An Ed25519 public key written as 64 hex digits.
/// A signature in hexadecimal (128 digits).
fn signature(hex: &str) -> Option<[u8; 64]> {
    paperpaw_sdk::digest::hex::decode(hex.trim())?
        .try_into()
        .ok()
}

fn key(hex: &str) -> Option<[u8; 32]> {
    let hex = hex.trim().as_bytes();
    if hex.len() != 64 {
        return None;
    }
    let digit = |b: u8| char::from(b).to_digit(16);
    let mut key = [0u8; 32];
    for (byte, pair) in key.iter_mut().zip(hex.chunks(2)) {
        *byte = u8::try_from(digit(pair[0])? * 16 + digit(pair[1])?).ok()?;
    }
    Some(key)
}

/// Cuts `text` to at most `max` bytes, at a character's end.
fn cut(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Whether `path` (below a plugin) is that of a file of `latest.files`.
fn is_file(path: &[String]) -> bool {
    matches!(path, [latest, files, at] if latest == "latest" && files == "files" && at == "[]")
}

/// Whether `path` (below a plugin) is that of a build of `latest.builds`.
fn is_build(path: &[String]) -> bool {
    matches!(path, [latest, builds, at] if latest == "latest" && builds == "builds" && at == "[]")
}

/// What a list says beside its plugins: whether it is in a fair order
/// (it has `sections`), and the plugins of each section.
#[derive(Debug, Default)]
struct Picks {
    fair: bool,
    sections: [Vec<String>; SECTIONS.len()],
}

/// Reads the list at `path` for `device`, handing each plugin that fits to
/// `found`; what it says beside them. `None` if it is no list this plugin
/// reads.
fn read_list(
    storage: &dyn Storage,
    path: &str,
    device: &Device<'_>,
    found: &mut dyn FnMut(Entry),
) -> Option<Picks> {
    let mut format = None;
    let mut item: Option<Item> = None;
    let mut picks = Picks::default();
    let mut revoked = Revocations::default();
    read_file(storage, path, &mut |event| match event {
        Event::Value([key], value) if key == "format" => format = value.parse::<u32>().ok(),
        Event::Value([list, at, field], value) if list == "revoked_keys" && at == "[]" => {
            revoked.field(field, value);
        }
        // After `format`, which a list names first: that it is a list this
        // plugin reads comes before anything it says is taken.
        Event::Close([list, at])
            if list == "revoked_keys" && at == "[]" && format == Some(FORMAT) =>
        {
            revoked.entry_done(storage, &|id| {
                device
                    .plugins
                    .is_none_or(|plugins| plugins.get(id).is_some())
            });
        }
        Event::Open([key, ..]) if key == "sections" => picks.fair = true,
        Event::Value([key, section, at], value) if key == "sections" && at == "[]" => {
            picks.fair = true;
            let ids = SECTIONS
                .iter()
                .position(|s| s == section)
                .map(|s| &mut picks.sections[s]);
            if let Some(ids) = ids.filter(|ids| ids.len() < MAX_SECTION)
                && install::is_plugin_name(value)
            {
                ids.push(value.to_string());
            }
        }
        Event::Open([list, at]) if list == "plugins" && at == "[]" => item = Some(Item::new()),
        Event::Close([list, at]) if list == "plugins" && at == "[]" => {
            if let Some(done) = item.take().filter(|item| item.fits(device)) {
                found(done.finish(device));
            }
        }
        Event::Close([list, at, rest @ ..])
            if list == "plugins" && at == "[]" && is_build(rest) =>
        {
            if let Some(item) = item.as_mut() {
                item.build_done(device);
            }
        }
        Event::Value([list, at, rest @ ..], value) if list == "plugins" && at == "[]" => {
            if let Some(item) = item.as_mut() {
                item.read(rest, value, device);
            }
        }
        _ => {}
    });
    // Only a list this plugin reads is believed at all.
    if format == Some(FORMAT) {
        revoked.hand_over(storage);
    }
    (format == Some(FORMAT)).then_some(picks)
}

/// A plugin as its page at the library gives it: what it is, its files
/// to install (a code plugin's code among them, as [`Device::code_file`]),
/// and the key its author signs its code with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plugin {
    pub entry: Entry,
    pub package: Package,
    pub author_key: Option<[u8; 32]>,
    /// The key its author named to replace a lost author key (#20): only
    /// taken at the plugin's first install ([`trust`](crate::trust)).
    pub recovery_key: Option<[u8; 32]>,
    /// The moves to other keys its author's recovery key signed, in order
    /// (a key moved from twice is two), if the library says so (#20).
    pub rotations: Vec<Rotation>,
    pub more: More,
}

impl Plugin {
    /// What it names for its author's key.
    pub(crate) fn named(&self) -> Option<Named> {
        Some(Named {
            author: self.author_key?,
            recovery: self.recovery_key,
            rotations: self.rotations.clone(),
        })
    }
}

/// Reads a plugin's page at `path` (`{"format": 2, "plugin": {…}}`): the
/// plugin if it fits `device`, with its files, their addresses relative to
/// `base` (the page's).
fn read_plugin(
    storage: &dyn Storage,
    path: &str,
    device: &Device<'_>,
    base: &str,
) -> Option<Plugin> {
    let mut format = None;
    let mut item = Item::new();
    read_file(storage, path, &mut |event| match event {
        Event::Value([key], value) if key == "format" => format = value.parse::<u32>().ok(),
        Event::Close([plugin, rest @ ..]) if plugin == "plugin" && is_file(rest) => {
            item.file_done();
        }
        Event::Close([plugin, rest @ ..]) if plugin == "plugin" && is_build(rest) => {
            item.build_done(device);
        }
        Event::Close([plugin, shots, at])
            if plugin == "plugin" && shots == "screenshots" && at == "[]" =>
        {
            item.shot_done();
        }
        Event::Close([plugin, rotations, at])
            if plugin == "plugin" && rotations == "rotations" && at == "[]" =>
        {
            item.rotation_done();
        }
        Event::Value([plugin, rest @ ..], value) if plugin == "plugin" => {
            item.read(rest, value, device);
        }
        _ => {}
    });
    if format != Some(FORMAT) || !item.fits(device) {
        return None;
    }
    let mut files = core::mem::take(&mut item.files);
    files.extend(item.code.clone());
    for file in &mut files {
        file.url = join_url(base, &file.url);
    }
    let author_key = key(&item.author_key);
    let recovery_key = key(&item.recovery_key);
    let rotations = core::mem::take(&mut item.rotations);
    let code = item.code.is_some();
    let mut more = core::mem::take(&mut item.more);
    if author_key.is_some() {
        more.author_key = item.author_key.trim().to_lowercase();
    }
    for shot in &mut more.shots {
        shot.url = join_url(base, &shot.url);
    }
    let entry = item.finish(device);
    let package = Package::new(entry.id.clone(), entry.name.clone()).files(files);
    // A code plugin names the key its author signs with.
    let whole = package.is_valid() && (!code || author_key.is_some());
    whole.then_some(Plugin {
        entry,
        package,
        author_key,
        recovery_key,
        rotations,
        more,
    })
}

/// A file of the store: the list fetched, the index and where its lines
/// begin, a plugin's page, the hits of a search.
fn file(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

pub(crate) fn list_part() -> String {
    file("list.part")
}

/// Where plugin `id`'s page is fetched to: a file of its own, so that
/// pages fetched at once do not write into each other.
pub(crate) fn plugin_part(id: &str) -> String {
    file(&format!("plugin-{id}.part"))
}

fn index_path() -> String {
    file("index.txt")
}

fn head_path() -> String {
    file("index.head")
}

fn offsets_path() -> String {
    file("index.bin")
}

fn hits_path() -> String {
    file("search.bin")
}

fn sections_path() -> String {
    file("sections.txt")
}

/// The first line of an index's head: its format, then the runs of
/// plugins of one group (`group:count`), in order, then `fair` where the
/// list is in a fair order, then how many plugins each section has.
const HEADER: &str = "store-index 5";

/// The address of the list at `list` for `device` in `language`, with
/// the community plugins where `community`, and the builds for the
/// fingerprint `abi` of the PaperPaw that runs (without it the library
/// answers as it did to clients before plugin interface 5: with builds
/// this PaperPaw drops).
pub(crate) fn list_url(
    list: &str,
    device: &str,
    language: &str,
    community: bool,
    abi: Option<Fingerprint>,
) -> String {
    let join = if list.contains('?') { '&' } else { '?' };
    let community = if community { "&community=1" } else { "" };
    let abi = abi.map_or_else(String::new, |abi| format!("&abi={abi}"));
    format!(
        "{list}{join}device={}&lang={}{community}{abi}",
        encode(device),
        encode(language)
    )
}

/// The address of plugin `id`'s page, beside the list at `list` (the
/// setting's, without the device and the language) and with its query
/// (a library may want a key there), for `device` in `language`, a
/// community plugin too where `community`.
pub(crate) fn plugin_url(
    list: &str,
    device: &str,
    language: &str,
    id: &str,
    community: bool,
    abi: Option<Fingerprint>,
) -> String {
    let list = list.split('#').next().unwrap_or(list);
    let (address, query) = list.split_once('?').unwrap_or((list, ""));
    let mut page = join_url(address, &format!("plugins/{}.json", encode(id)));
    if !query.is_empty() {
        page.push('?');
        page.push_str(query);
    }
    list_url(&page, device, language, community, abi)
}

/// Writes the index of the list at `json` (`index`, `offsets`, `head` on
/// the storage). `false` (and no index) if it is no list this plugin
/// reads.
fn build(
    storage: &dyn Storage,
    json: &str,
    [index, offsets, head, sections]: [&str; 4],
    device: &Device<'_>,
) -> bool {
    let mut runs: Vec<(usize, u32)> = Vec::new();
    let mut lines = String::new();
    let mut starts: Vec<u8> = Vec::new();
    let mut offset = 0u32;
    let mut written = storage.write(index, b"").is_ok() && storage.write(offsets, b"").is_ok();
    let read = read_list(storage, json, device, &mut |entry| {
        let full = runs.len() >= MAX_RUNS;
        match runs.last_mut() {
            Some((group, count)) if *group == entry.group || full => *count += 1,
            _ => runs.push((entry.group, 1)),
        }
        let line = entry.line();
        starts.extend_from_slice(&offset.to_le_bytes());
        offset = offset.saturating_add(line.len() as u32);
        lines.push_str(&line);
        if lines.len() > WRITE_AT {
            written &= storage.append(index, lines.as_bytes()).is_ok();
            written &= storage.append(offsets, &starts).is_ok();
            lines.clear();
            starts.clear();
        }
    });
    written &= storage.append(index, lines.as_bytes()).is_ok();
    written &= storage.append(offsets, &starts).is_ok();
    let read_whole = read.is_some();
    let picks = read.unwrap_or_default();
    let mut text = String::new();
    for (key, ids) in SECTIONS.iter().zip(&picks.sections) {
        text.push_str(&format!("{key}\t{}\n", ids.join(",")));
    }
    written &= storage.write(sections, text.as_bytes()).is_ok();
    let runs: Vec<String> = runs
        .iter()
        .map(|(group, count)| format!("{group}:{count}"))
        .collect();
    let counts: Vec<String> = picks.sections.iter().map(|s| s.len().to_string()).collect();
    let fair = if picks.fair { "fair" } else { "name" };
    let text = format!(
        "{HEADER}\t{}\t{fair}\t{}\n",
        runs.join(","),
        counts.join(",")
    );
    written &= storage.write(head, text.as_bytes()).is_ok();
    if !read_whole || !written {
        for path in [index, offsets, head, sections] {
            let _ = storage.remove(path);
        }
        return false;
    }
    true
}

/// The line at `offset` of the file at `path`.
fn line_at(storage: &dyn Storage, path: &str, offset: u32) -> Option<String> {
    let mut buf = [0u8; 256];
    let mut at = u64::from(offset);
    let mut line = Vec::new();
    loop {
        let n = storage.read_at(path, at, &mut buf).ok()?;
        if n == 0 {
            break;
        }
        if let Some(end) = buf[..n].iter().position(|&b| b == b'\n') {
            line.extend_from_slice(&buf[..end]);
            break;
        }
        line.extend_from_slice(&buf[..n]);
        if line.len() > MAX_LINE {
            return None;
        }
        at += n as u64;
    }
    String::from_utf8(line).ok()
}

/// The `n`th number of 4 bytes in the file at `path`.
fn number_at(storage: &dyn Storage, path: &str, n: usize) -> Option<u32> {
    let mut bytes = [0u8; 4];
    let read = storage.read_at(path, (n as u64) * 4, &mut bytes).ok()?;
    (read == 4).then(|| u32::from_le_bytes(bytes))
}

/// The plugins of the index on the storage: in memory only the runs of
/// plugins of one group, in order, and how many each section has.
#[derive(Debug, Default)]
pub(crate) struct Index {
    runs: Vec<(usize, usize)>,
    /// The list is in the library's fair order (else by name).
    fair: bool,
    sections: [usize; SECTIONS.len()],
}

impl Index {
    /// The index kept on the storage, if there is one.
    pub(crate) fn kept(storage: &dyn Storage) -> Option<Index> {
        Self::open(storage, &head_path())
    }

    fn open(storage: &dyn Storage, head: &str) -> Option<Index> {
        let text = line_at(storage, head, 0)?;
        let mut fields = text.split('\t');
        if fields.next() != Some(HEADER) {
            return None;
        }
        let runs = fields.next().unwrap_or("");
        let fair = fields.next() == Some("fair");
        let mut sections = [0; SECTIONS.len()];
        for (count, text) in sections
            .iter_mut()
            .zip(fields.next().unwrap_or("").split(','))
        {
            *count = text.parse().unwrap_or(0);
        }
        let runs = runs
            .split(',')
            .filter(|run| !run.is_empty())
            .filter_map(|run| {
                let (group, count) = run.split_once(':')?;
                Some((group.parse().ok()?, count.parse().ok()?))
            })
            .filter(|&(group, _)| group < GROUPS.len())
            .collect();
        Some(Index {
            runs,
            fair,
            sections,
        })
    }

    /// The list is in the library's fair order: its order is
    /// [`Order::Fair`], and A to Z is sorted here.
    pub(crate) fn fair(&self) -> bool {
        self.fair
    }

    /// How many plugins section `section` (of [`SECTIONS`]) has.
    pub(crate) fn section_len(&self, section: usize) -> usize {
        self.sections.get(section).copied().unwrap_or(0)
    }

    /// The ids of the plugins of section `section`, in its order.
    pub(crate) fn section(&self, storage: &dyn Storage, section: usize) -> Vec<String> {
        let Some(key) = SECTIONS.get(section) else {
            return Vec::new();
        };
        let mut ids = Vec::new();
        let mut at = 0u32;
        while let Some(line) = line_at(storage, &sections_path(), at) {
            at += line.len() as u32 + 1;
            if let Some(list) = line.strip_prefix(key).and_then(|l| l.strip_prefix('\t')) {
                ids.extend(
                    list.split(',')
                        .filter(|id| !id.is_empty())
                        .map(String::from),
                );
                break;
            }
            if line.is_empty() {
                break;
            }
        }
        ids
    }

    /// The orders a list of the index may show: the list's own first.
    pub(crate) fn orders(&self) -> &'static [Order] {
        if self.fair {
            &[Order::Fair, Order::Name, Order::Added, Order::Updated]
        } else {
            &[Order::Name, Order::Added, Order::Updated]
        }
    }

    /// The order a list shows `key` in: one of [`Index::orders`], the first
    /// where `key` is none of them.
    pub(crate) fn order(&self, key: &str) -> Order {
        let orders = self.orders();
        orders
            .iter()
            .copied()
            .find(|o| o.key() == key)
            .unwrap_or(orders[0])
    }

    /// The runs of plugins of one group, in order: (group, how many).
    pub(crate) fn runs(&self) -> &[(usize, usize)] {
        &self.runs
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.runs.iter().map(|(_, count)| count).sum()
    }

    /// Plugin `n`, read from the storage.
    pub(crate) fn entry(&self, storage: &dyn Storage, n: usize) -> Option<Entry> {
        let offset = number_at(storage, &offsets_path(), n)?;
        Entry::parse(&line_at(storage, &index_path(), offset)?)
    }

    /// Searches the names and summaries for `query` (the case of letters
    /// left aside), keeping the hits on the storage; how many there are.
    pub(crate) fn search(&self, storage: &dyn Storage, query: &str) -> usize {
        let query = query.trim().to_lowercase();
        self.pick(
            storage,
            |_, entry| {
                (entry.name.to_lowercase().contains(&query)
                    || entry.summary.to_lowercase().contains(&query))
                .then_some(0)
            },
            Order::Fair,
            usize::MAX,
        )
    }

    /// Each plugin with its place, read through the index once, in order.
    pub(crate) fn scan(&self, storage: &dyn Storage, found: &mut dyn FnMut(usize, Entry)) {
        let path = index_path();
        let mut buf = [0u8; 512];
        let mut line: Vec<u8> = Vec::new();
        let (mut at, mut n) = (0u64, 0usize);
        while let Ok(read) = storage.read_at(&path, at, &mut buf) {
            if read == 0 {
                break;
            }
            at += read as u64;
            for &byte in &buf[..read] {
                if byte != b'\n' {
                    if line.len() <= MAX_LINE {
                        line.push(byte);
                    }
                    continue;
                }
                if let Some(entry) = core::str::from_utf8(&line).ok().and_then(Entry::parse) {
                    found(n, entry);
                }
                line.clear();
                n += 1;
            }
        }
    }

    /// Keeps the plugins `keep` takes (with a rank, for
    /// [`Order::Rank`]), in `order`, `most` of them at most, on the storage
    /// (read back with [`hit`]); how many there are.
    ///
    /// However many plugins there are, little is held at once: in the
    /// list's order they are written as they are read; by a key, the best
    /// `most` are kept while reading where that is few, and otherwise runs
    /// of [`RUN`] are sorted, kept on the storage and merged. A to Z is
    /// the list's order where it is not fair, and sorted by [`name_key`]
    /// where it is.
    pub(crate) fn pick(
        &self,
        storage: &dyn Storage,
        mut keep: impl FnMut(usize, &Entry) -> Option<u32>,
        order: Order,
        most: usize,
    ) -> usize {
        let hits = hits_path();
        let mut out = Output::new(storage, &hits);
        if order == Order::Fair || (order == Order::Name && !self.fair) {
            self.scan(storage, &mut |n, entry| {
                if out.count < most && keep(n, &entry).is_some() {
                    out.push(n as u32);
                }
            });
            return out.finish();
        }
        let key = |entry: &Entry, rank: u32| match order {
            Order::Added => u64::from(entry.added),
            Order::Updated => u64::from(entry.updated),
            // The first of A to Z the highest.
            Order::Name => u64::MAX - name_key(&entry.name),
            Order::Fair | Order::Rank => u64::from(rank),
        };
        if most <= RUN {
            // The best few, kept in order while reading.
            let mut best: Vec<Keyed> = Vec::new();
            self.scan(storage, &mut |n, entry| {
                if let Some(rank) = keep(n, &entry) {
                    let item = (key(&entry, rank), n as u32);
                    let at = best.partition_point(|b| before(*b, item));
                    if at < most {
                        if best.len() == most {
                            best.pop();
                        }
                        best.insert(at, item);
                    }
                }
            });
            for (_, n) in best {
                out.push(n);
            }
            return out.finish();
        }
        // Runs sorted on the storage, then merged.
        let runs_path = file("sort.bin");
        let mut runs: Vec<u32> = Vec::new();
        let mut run: Vec<Keyed> = Vec::new();
        let mut kept = storage.write(&runs_path, b"").is_ok();
        let mut flush = |run: &mut Vec<Keyed>, runs: &mut Vec<u32>| {
            run.sort_by(|a, b| order_of(*a, *b));
            let mut bytes = Vec::with_capacity(run.len() * KEYED);
            for (key, n) in run.iter() {
                bytes.extend_from_slice(&key.to_le_bytes());
                bytes.extend_from_slice(&n.to_le_bytes());
            }
            kept &= storage.append(&runs_path, &bytes).is_ok();
            runs.push(run.len() as u32);
            run.clear();
        };
        self.scan(storage, &mut |n, entry| {
            if let Some(rank) = keep(n, &entry) {
                run.push((key(&entry, rank), n as u32));
                if run.len() == RUN {
                    flush(&mut run, &mut runs);
                }
            }
        });
        if !run.is_empty() {
            flush(&mut run, &mut runs);
        }
        drop(run);
        if !kept {
            let _ = storage.remove(&runs_path);
            return 0;
        }
        let mut cursors: Vec<Cursor> = Vec::with_capacity(runs.len());
        let mut at = 0u64;
        for len in runs {
            cursors.push(Cursor::new(at, len));
            at += u64::from(len) * KEYED as u64;
        }
        while out.count < most {
            let mut first: Option<(usize, Keyed)> = None;
            for (i, cursor) in cursors.iter_mut().enumerate() {
                if let Some(item) = cursor.head(storage, &runs_path)
                    && first.is_none_or(|(_, best)| before(item, best))
                {
                    first = Some((i, item));
                }
            }
            let Some((i, (_, n))) = first else {
                break;
            };
            cursors[i].next();
            out.push(n);
        }
        let _ = storage.remove(&runs_path);
        out.finish()
    }
}

/// The order a view of the plugins shows them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Order {
    /// The library's fair order of the day: the list's where it is fair.
    Fair,
    /// A to Z: the list's where it is not fair.
    Name,
    /// Newest first.
    Added,
    /// Most recently updated first.
    Updated,
    /// Highest rank first (as `pick` was given), then in the list's order.
    Rank,
}

impl Order {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Order::Fair => "fair",
            Order::Name => "name",
            Order::Added => "added",
            Order::Updated => "updated",
            Order::Rank => "rank",
        }
    }
}

/// Letters and digits a name is sorted by: so many fit a [`name_key`].
const KEY_CHARS: u32 = 12;

/// Where `name` goes from A to Z, as a number: its first [`KEY_CHARS`]
/// letters and digits, accents and case left aside, digits first and
/// letters of other scripts after z (names alike in those keep the
/// list's order). Twelve of 38 values fit 64 bits.
pub(crate) fn name_key(name: &str) -> u64 {
    let mut key = 0u64;
    let mut taken = 0;
    for value in name
        .chars()
        .flat_map(char::to_lowercase)
        .filter_map(sort_value)
    {
        if taken == KEY_CHARS {
            break;
        }
        key = key * 38 + value;
        taken += 1;
    }
    key * 38u64.pow(KEY_CHARS - taken)
}

/// Where `c` (in lower case) sorts: 1 to 10 for digits, 11 to 36 for
/// the letters (accented ones as their base letter), 37 for letters of
/// other scripts; `None` for what is passed over (spaces, signs).
fn sort_value(c: char) -> Option<u64> {
    let c = match c {
        'à'..='å' | 'æ' | 'ā' | 'ą' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è'..='ë' | 'ē' | 'ę' | 'ě' => 'e',
        'ì'..='ï' => 'i',
        'ł' => 'l',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò'..='ö' | 'ø' | 'œ' => 'o',
        'ř' => 'r',
        'ß' | 'ś' | 'š' => 's',
        'ù'..='ü' | 'ů' => 'u',
        'ý' | 'ÿ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        c => c,
    };
    match c {
        '0'..='9' => Some(1 + u64::from(c) - u64::from('0')),
        'a'..='z' => Some(11 + u64::from(c) - u64::from('a')),
        c if c.is_alphanumeric() => Some(37),
        _ => None,
    }
}

/// The plugin of hit `n` of the last search.
pub(crate) fn hit(storage: &dyn Storage, n: usize) -> Option<usize> {
    number_at(storage, &hits_path(), n).map(|n| n as usize)
}

/// Takes the list fetched to `fetched`: its index, kept for the next
/// visits. `None` (and the file gone) if it is no list this plugin reads.
pub(crate) fn take(storage: &dyn Storage, fetched: &str, device: &Device<'_>) -> Option<Index> {
    let fresh = [index_path(), offsets_path(), sections_path(), head_path()]
        .map(|path| format!("{path}.part"));
    let built = build(
        storage,
        fetched,
        [&fresh[0], &fresh[1], &fresh[3], &fresh[2]],
        device,
    );
    let _ = storage.remove(fetched);
    if !built {
        return None;
    }
    // The head last: an index is whole where its head is.
    let _ = storage.remove(&head_path());
    for (from, to) in fresh
        .iter()
        .zip([index_path(), offsets_path(), sections_path(), head_path()])
    {
        if paperpaw_sdk::files::move_to(storage, from, &to).is_err() {
            return None;
        }
    }
    // The hits of a search before were of the list before.
    let _ = storage.remove(&hits_path());
    Index::kept(storage)
}

/// The plugin of the page fetched to `fetched` from `address`, with its
/// files to install.
pub(crate) fn plugin(
    storage: &dyn Storage,
    fetched: &str,
    address: &str,
    device: &Device<'_>,
) -> Option<Plugin> {
    let read = read_plugin(storage, fetched, device, address);
    let _ = storage.remove(fetched);
    read
}

#[cfg(test)]
mod tests;
