//! Native code plugins (ADR 0024): a plugin built into a file that a
//! device loads at its start, without a new firmware. It runs as part of
//! PaperPaw, with no sandbox, and its code is the same as a plugin built
//! in: one more crate makes the file, with [`plugin!`](crate::plugin).
//!
//! ```ignore
//! #![no_std]
//! #![no_main]
//!
//! paperpaw_sdk::plugin!(my_plugin::MyPlugin::default());
//! ```
//!
//! A plugin file fits a PaperPaw of the same plugin interface
//! ([`INTERFACE`]: what of the SDK plugins see, the layouts of the types
//! that cross, the firmware's table, how a plugin is linked), built for
//! the same chip with the features that change what crosses. Until
//! PaperPaw 1.0 the SDK's types cross between them as they are, so the
//! compiler is the release's too (`shells/xteink/rust-toolchain.toml`,
//! `versions.env`), and a new one is a new interface (ADR 0026). A plugin
//! file names the compiler that built it ([`compiler`]), and a device
//! loads it only where that built the firmware too.
//! [`FINGERPRINT`] says which, and a device loads only files with its own
//! ([`Native::fingerprint`]); its top 16 bits are the interface's version,
//! so a file of another can say so ([`Fingerprint::interface`]). A plugin
//! built for an interface runs on every PaperPaw of it, whichever release
//! made the plugin. `docs/plugins.md` says how a plugin is built, packed
//! (`paperpaw-sign pack`), signed and published.
//!
//! Functions a plugin spends its time in may be marked [`hot!`](crate::hot):
//! a device with faster memory than its plugins' code runs them from
//! there.
//!
//! A code plugin is a folder like a data plugin's (`plugin.ini`,
//! translations, settings) with its code as [`Native::file`]; the kernel
//! loads the code of those switched on at the start, and switching one
//! takes a restart. Where PaperPaw loads code plugins, [`Env::native`]
//! says with what (the store takes the build that fits), and whether this
//! start went without them ([`SafeMode`]).
//!
//! [`Env::native`]: crate::Env

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::Id;
use crate::plugins::PluginProblem;
use paperpaw_platform::Storage;

pub use paperpaw_native::compiler;
pub use paperpaw_native::signature::{self, BadSignature};
pub use paperpaw_native::{Chip, HEADER_LEN, Header, Problem, Signature};

/// The version of the plugin interface: what code plugins are built
/// against (see the [module documentation](self)). Raised by hand, and
/// only when what plugins see changes; `crates/paperpaw-sdk/interface.txt`
/// keeps what it was, and CI fails while they differ (ADR 0026).
pub const INTERFACE: u16 = 5;

/// The crates whose types cross between a plugin and PaperPaw, by the
/// names their paths begin with: the SDK and every crate of PaperPaw it
/// depends on. The plugin interface keeps their types as they are
/// (`crates/paperpaw-sdk/interface/crates.txt`, which a test keeps the
/// same), so a type made of theirs alone is the same in every build of
/// one interface. The interface's test checks this list against that
/// file.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub const CROSSING_CRATES: &[&str] = &[
    "paperpaw_sdk",
    "paperpaw_platform",
    "paperpaw_graphics",
    "paperpaw_i18n",
    "paperpaw_config",
    "paperpaw_text",
    "paperpaw_document",
    "paperpaw_layout",
    "paperpaw_xml",
    "paperpaw_zip",
    "paperpaw_hyphen",
    "paperpaw_image",
    "paperpaw_update",
    "paperpaw_digest",
    "paperpaw_json",
    "paperpaw_qr",
    "paperpaw_native",
    "paperpaw_lang",
    "paperpaw_table",
];

/// The layouts of the types that cross between a plugin and PaperPaw, as
/// this build lays them out: a digest of their sizes and alignments, and
/// of where the firmware's table keeps each of its entries. Part of the
/// fingerprint, so made for each chip as it lays them out: a plugin whose
/// layouts differ from the firmware's is not loaded.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub const LAYOUT: u64 = layout::DIGEST;

/// What this build of the SDK makes plugins fit: the plugin interface, the
/// chip and the features that change what crosses (see the [module
/// documentation](self)). From constants alone: no build script, so the
/// computer it is built on changes nothing.
pub const FINGERPRINT: Fingerprint = Fingerprint::of(INTERFACE, {
    let mut hash = layout::Fnv::new();
    hash = hash.text(layout::TARGET);
    hash = hash.number(core::mem::size_of::<usize>() as u64);
    // Features of crates below the SDK that change what a plugin's own
    // copy of their code does with what crosses, whichever crate
    // switched them on. Colour does not: Android and the emulator draw
    // in colour, and plugins built without it run there.
    hash = hash.number(paperpaw_text::OUTLINES as u64);
    hash = hash.number(layout::DIGEST);
    hash.0
});

/// The compiler of PaperPaw's release a plugin for this chip is built
/// with (the X4's `shells/xteink/rust-toolchain.toml`, the X4 Pro's
/// `versions.env`; a test keeps them so), and the chip's target: what a
/// plugin must be built with to fit. A shell that knows better (the
/// emulator, built by the computer's compiler) says so in
/// [`Native::toolchain`].
pub const TOOLCHAIN: &str = if cfg!(target_arch = "riscv32") {
    "rustc 1.99.0, riscv32imc-unknown-none-elf"
} else if cfg!(target_arch = "xtensa") {
    "esp 1.97.0.0, xtensa-esp32s3-none-elf"
} else {
    "the compiler of PaperPaw's release, for this computer"
};

/// The names a plugin folder's code has: `code.ppn` on the X4 and the X4
/// Pro, `code.so` on Android, `code-host.dll`, `.so` or `.dylib` built for
/// a computer (the desktop emulator). A folder with one is a code plugin's,
/// also where PaperPaw loads none.
pub const CODE_FILES: &[&str] = &[
    "code.ppn",
    "code.so",
    "code-host.dll",
    "code-host.so",
    "code-host.dylib",
];

/// The folder in the plugins' folder where code plugins fetched for a
/// firmware update wait for it (`plugins/.stats.next`), which PaperPaw
/// does not load: the update plugin puts each plugin's new build there,
/// with its folder's files, before the update is installed (ADR 0024),
/// and the updated firmware installs those built for it at its first
/// start, in place of every version before (its settings and data stay,
/// as they belong to its id). A plugin built in before and a code plugin
/// now comes back so.
pub fn fetched_folder(plugin: &str) -> String {
    alloc::format!("{}/.{plugin}.next", crate::builtin::PLUGIN_DIR)
}

/// The plugin `name` of the plugins' folder fetched for a firmware
/// update, if `name` is such a folder's ([`fetched_folder`]).
pub fn fetched_plugin(name: &str) -> Option<&str> {
    name.strip_prefix('.')?
        .strip_suffix(".next")
        .filter(|id| crate::install::is_plugin_name(id))
}

/// The plugins fetched for a firmware update that wait for this one
/// (`native`): their folders ([`fetched_folder`]) hold code built for it
/// and fetched by the update plugin ([`remember_fetched`]), not put
/// there otherwise. Their ids and the SHA-256 of their headers, sorted;
/// the kernel installs them at the start, before plugins are found.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn fetched_for(storage: &dyn Storage, native: &Native) -> Vec<(String, [u8; 32])> {
    let mut ids: Vec<String> = Vec::new();
    let _ = storage.list(crate::builtin::PLUGIN_DIR, &mut |entry| {
        if entry.is_dir
            && let Some(id) = fetched_plugin(entry.name)
        {
            ids.push(String::from(id));
        }
    });
    ids.sort_unstable();
    ids.into_iter()
        .filter_map(|id| {
            let header = read_header(storage, &native.path(&fetched_folder(&id)))?;
            (native.fits(&header) && fetched_code(storage, &id) == Some(header.sha256))
                .then_some((id, header.sha256))
        })
        .collect()
}

/// Checks the plugin file at `path` whole, reading it once: its header,
/// its length, its digest and, if it is signed, its author's signature.
/// Its header and its author's signature (`None` if it is not signed);
/// whether it fits this device and build is for the caller to see
/// ([`Header::chip`], [`Header::fingerprint`]), and whether the author is
/// one known (a store remembers the key of the first install).
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn check_file(
    storage: &dyn Storage,
    path: &str,
) -> Result<(Header, Option<Signature>), Problem> {
    let mut head = [0u8; HEADER_LEN];
    read_exact(storage, path, 0, &mut head)?;
    let header = Header::read(&head)?;
    let size = storage.size(path).map_err(|_| Problem::Length)?;
    let end = header.file_len();
    let signature = if header.signed(size)? {
        let mut trailer = [0u8; signature::SIGNATURE_LEN];
        read_exact(storage, path, end, &mut trailer)?;
        let signature = Signature::read(&trailer).ok_or(Problem::Signature)?;
        if u64::from(signature.signed_len) != end {
            return Err(Problem::Signature);
        }
        Some(signature)
    } else {
        None
    };
    let mut author = match &signature {
        Some(signature) => Some(signature.check().map_err(|_| Problem::Signature)?),
        None => None,
    };
    if let Some(check) = author.as_mut() {
        check.absorb(&head);
    }
    let mut digest = header.digest();
    let mut buf = [0u8; 512];
    let mut at = HEADER_LEN as u64;
    while at < end {
        let len = (end - at).min(buf.len() as u64) as usize;
        read_exact(storage, path, at, &mut buf[..len])?;
        digest.update(&buf[..len]);
        if let Some(check) = author.as_mut() {
            check.absorb(&buf[..len]);
        }
        at += len as u64;
    }
    if !header.holds(digest) {
        return Err(Problem::Damaged);
    }
    if let Some(check) = author {
        check.finish().map_err(|_| Problem::Signature)?;
    }
    Ok((header, signature))
}

/// Where the keys of the authors of the code plugins installed are kept:
/// one file for every installer (a store, the firmware's first start after
/// the web flasher), so a key stays known whichever installs.
const AUTHORS: &str = crate::data_path!("plugin-authors.txt");

/// Whose a plugin's code is, by its author's key, as this device knows
/// the plugin's author (trust on first use, ADR 0024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Author {
    /// The key remembered for the plugin.
    Known,
    /// No key is remembered for the plugin yet: its first install.
    New,
    /// Another key than the one remembered: the store and the updates
    /// refuse it (trust on first install).
    Other,
}

/// Whether `key` (a [`Signature::key`]) is the author of `plugin` known on
/// this device, the first one, or another.
pub fn author(storage: &dyn Storage, plugin: &str, key: &[u8; 32]) -> Author {
    match known_author(storage, plugin) {
        Some(known) if known == *key => Author::Known,
        Some(_) => Author::Other,
        None => Author::New,
    }
}

/// The key of the author of `plugin`'s code, as remembered at its first
/// install (`remember_author`).
pub fn known_author(storage: &dyn Storage, plugin: &str) -> Option<[u8; 32]> {
    entry(storage, AUTHORS, plugin)
}

/// Remembers `key` as the author of `plugin`'s code, in place of one
/// known before: at its first install, and when the reader installed an
/// update by another; `false` if it could not be kept (the file left as
/// it is where it could not be read).
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn remember_author(storage: &dyn Storage, plugin: &str, key: &[u8; 32]) -> bool {
    keep_entry(storage, AUTHORS, plugin, key)
}

/// Where the code plugins' files are named as they were installed, by the
/// SHA-256 their headers carry (`Header::sha256`, which holds for all of
/// the file but its signature): beside the authors' keys, where neither
/// the Wi-Fi transfer nor plugins' own folders reach.
const INSTALLED: &str = crate::data_path!("plugin-code.txt");

#[cfg(feature = "kernel")]
mod installed;
#[cfg(feature = "kernel")]
pub use installed::InstalledCodes;

/// The code of `plugin` as installed (`remember_code`): the SHA-256 of
/// its file's header. The kernel loads a code plugin's file only when its
/// header says the same, so that a file put into a plugin's folder
/// otherwise (copied there by hand, or over the Wi-Fi transfer) does not
/// run in its place.
pub fn installed_code(storage: &dyn Storage, plugin: &str) -> Option<[u8; 32]> {
    entry(storage, INSTALLED, plugin)
}

/// Names the code of `plugin` as installed, by its header's SHA-256, in
/// place of the version before: every installer does, once it checked the
/// file ([`check_file`]) and its author; `false` if it could not be kept
/// (the file left as it is where it could not be read).
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn remember_code(storage: &dyn Storage, plugin: &str, sha256: &[u8; 32]) -> bool {
    keep_entry(storage, INSTALLED, plugin, sha256)
}

/// Where the code fetched for a firmware update is named
/// ([`remember_fetched`]), as [`remember_code`] names the code installed.
const FETCHED: &str = crate::data_path!("plugin-code-fetched.txt");

/// Names the code of `plugin` fetched for a firmware update (into
/// [`fetched_folder`]), by its header's SHA-256: the updated firmware
/// installs it at its start only when its file is that one, so a folder
/// put there otherwise does not run (see [`installed_code`]). The code
/// installed now stays named until then. `false` if it could not be kept.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn remember_fetched(storage: &dyn Storage, plugin: &str, sha256: &[u8; 32]) -> bool {
    keep_entry(storage, FETCHED, plugin, sha256)
}

/// The code of `plugin` fetched for a firmware update
/// (`remember_fetched`).
pub fn fetched_code(storage: &dyn Storage, plugin: &str) -> Option<[u8; 32]> {
    entry(storage, FETCHED, plugin)
}

/// The 32 bytes kept for `plugin` in `file`, a line `<plugin>\t<hex>` each.
fn entry(storage: &dyn Storage, file: &str, plugin: &str) -> Option<[u8; 32]> {
    let text = crate::read_file(storage, file, 64 * 1024).ok()?;
    entry_in(core::str::from_utf8(&text).ok()?, plugin)
}

/// The first matching entry decides, including when its digest is invalid.
fn entry_in(text: &str, plugin: &str) -> Option<[u8; 32]> {
    let hex = text
        .lines()
        .find_map(|line| line.strip_prefix(plugin)?.strip_prefix('\t'))?;
    paperpaw_digest::hex::decode(hex.trim())?.try_into().ok()
}

/// Keeps `value` for `plugin` in `file`, in place of what was kept. A
/// file that is there but cannot be read (a failing card, too large) is
/// not written over: it holds every other plugin's.
fn keep_entry(storage: &dyn Storage, file: &str, plugin: &str, value: &[u8; 32]) -> bool {
    let Ok(kept) = crate::files::load_text_for_update(storage, file, 64 * 1024) else {
        return false;
    };
    let kept = kept.unwrap_or_default();
    let mut text: String = kept
        .lines()
        .filter(|line| line.split('\t').next() != Some(plugin))
        .map(|line| alloc::format!("{line}\n"))
        .collect();
    text.push_str(&alloc::format!(
        "{plugin}\t{}\n",
        paperpaw_digest::hex::encode(value)
    ));
    let _ = storage.create_dir(crate::data::DIR);
    storage.write(file, text.as_bytes()).is_ok()
}

/// Fills `buf` from `path` at `at`.
fn read_exact(
    storage: &dyn Storage,
    path: &str,
    mut at: u64,
    mut buf: &mut [u8],
) -> Result<(), Problem> {
    while !buf.is_empty() {
        match storage.read_at(path, at, buf) {
            Ok(n) if n > 0 => {
                at += n as u64;
                buf = &mut buf[n..];
            }
            _ => return Err(Problem::Length),
        }
    }
    Ok(())
}

/// The room code plugins run from, as the shell found it when it loaded
/// them at this start: the X4's flash region behind the firmware, the X4
/// Pro's PSRAM. The shell keeps it on the storage (`keep_code_room`);
/// the store and the plugin manager read it ([`code_room`]) to say how
/// much is left and to refuse a plugin that would not fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct CodeRoom {
    /// Bytes code plugins may take in all.
    pub total: u64,
    /// Bytes the plugins loaded at this start take.
    pub used: u64,
    /// Each plugin's image takes a whole number of these bytes (at least
    /// 1).
    pub granule: u32,
    /// Plugins at most, where the room limits their number (the X4's
    /// index).
    pub max_plugins: Option<u32>,
    /// Plugins loaded at this start.
    pub placed: u32,
    /// The room is memory shared with everything else (the X4 Pro's
    /// PSRAM): `total` is what was free when the plugins were loaded, an
    /// estimate to say as "about".
    pub shared: bool,
}

impl CodeRoom {
    /// `total` bytes for code plugins, `used` of them taken, each image
    /// taking whole `granule`s; no limit on their number, none placed, not
    /// shared (see [`CodeRoom::with_plugins`], [`CodeRoom::shared`]).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn new(total: u64, used: u64, granule: u32) -> Self {
        Self {
            total,
            used,
            granule: granule.max(1),
            max_plugins: None,
            placed: 0,
            shared: false,
        }
    }

    /// The same, with `placed` plugins placed at this start, `most` at
    /// most.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn with_plugins(mut self, placed: u32, most: Option<u32>) -> Self {
        self.placed = placed;
        self.max_plugins = most;
        self
    }

    /// The same, in memory shared with everything else: what is free is
    /// an estimate (the X4 Pro's PSRAM).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shared(mut self) -> Self {
        self.shared = true;
        self
    }

    /// The X4's room: the flash region from `start` to `end`, its first
    /// sector the index of what lies in it, the images placed from there
    /// up to `images_end` (`placed` of them), each in whole sectors, as
    /// many as the index holds at most.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn in_flash(start: u32, end: u32, images_end: u32, placed: u32) -> Self {
        use paperpaw_native::index::{MAX_PLACED, SECTOR};
        let first = start.saturating_add(SECTOR);
        let total = end.saturating_sub(first);
        let used = images_end.saturating_sub(first).min(total);
        Self::new(u64::from(total), u64::from(used), SECTOR)
            .with_plugins(placed, Some(MAX_PLACED as u32))
    }

    /// The X4 Pro's room: the PSRAM `used` by the `placed` images of this
    /// start, and what was `free` of it beside them (shared with all else,
    /// an estimate), each image aligned to a cache line of 32 bytes.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn in_psram(used: u64, free: u64, placed: u32) -> Self {
        Self::new(used.saturating_add(free), used, 32)
            .with_plugins(placed, None)
            .shared()
    }

    /// The bytes an image of `image_len` bytes, aligned to `align`, takes
    /// where this room places it.
    pub fn needs(&self, image_len: u64, align: u32) -> u64 {
        let step = u64::from(self.granule.max(align).max(1));
        image_len.div_ceil(step).saturating_mul(step)
    }

    /// Bytes left once `taken` bytes are taken.
    pub fn left(&self, taken: u64) -> u64 {
        self.total.saturating_sub(taken)
    }

    /// Whether one more plugin of `need` bytes ([`CodeRoom::needs`]) fits
    /// beside `plugins` plugins taking `taken` bytes (see
    /// `installed_code_need`).
    pub fn fits(&self, taken: u64, plugins: u32, need: u64) -> bool {
        let count = self.max_plugins.is_none_or(|most| plugins < most);
        count && taken.saturating_add(need) <= self.total
    }
}

/// Where the shell keeps the room of this start.
const ROOM: &str = crate::data_path!("code-room.txt");

/// The room code plugins have on this device, as the shell found it at
/// this start; `None` where it loads none or did not say.
pub fn code_room(storage: &dyn Storage) -> Option<CodeRoom> {
    let text = crate::read_file(storage, ROOM, 256).ok()?;
    let text = core::str::from_utf8(&text).ok()?;
    let mut fields = text.split_whitespace();
    let mut number = || fields.next()?.parse::<u64>().ok();
    let (total, used, granule) = (number()?, number()?, number()?);
    let (max, placed, shared) = (number()?, number()?, number()?);
    Some(CodeRoom {
        total,
        used,
        granule: u32::try_from(granule).ok()?.max(1),
        max_plugins: u32::try_from(max).ok().filter(|&m| m > 0),
        placed: u32::try_from(placed).ok()?,
        shared: shared != 0,
    })
}

/// Keeps `room` for [`code_room`] (`None`: there is none this start):
/// the shell does, once it loaded the code plugins of this start. `false`
/// if it could not be written.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn keep_code_room(storage: &dyn Storage, room: Option<&CodeRoom>) -> bool {
    let Some(room) = room else {
        return storage.remove(ROOM).is_ok() || !storage.exists(ROOM);
    };
    let text = alloc::format!(
        "{} {} {} {} {} {}\n",
        room.total,
        room.used,
        room.granule,
        room.max_plugins.unwrap_or(0),
        room.placed,
        u8::from(room.shared)
    );
    let _ = storage.create_dir(crate::data::DIR);
    storage.write(ROOM, text.as_bytes()).is_ok()
}

/// The bytes the code plugins installed take where `room` places them,
/// and how many they are: each folder in `plugins` with a code file named
/// `file` ([`Native::file`]), by its header. Those installed since the
/// start (loaded only after a restart) count, those switched off too: what
/// the next start would place at most. Folders whose names begin with a
/// dot (a download being put together, e.g. `.clock.part`) are no plugins
/// and do not count.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn installed_code_need(storage: &dyn Storage, room: &CodeRoom, file: &str) -> (u64, u32) {
    let mut folders: Vec<String> = Vec::new();
    let _ = storage.list(crate::builtin::PLUGIN_DIR, &mut |entry| {
        if entry.is_dir && !entry.name.starts_with('.') {
            folders.push(String::from(entry.name));
        }
    });
    let (mut taken, mut count) = (0u64, 0u32);
    for folder in folders {
        let path = alloc::format!("{}/{folder}/{file}", crate::builtin::PLUGIN_DIR);
        if let Some(header) = read_header(storage, &path) {
            taken = taken.saturating_add(room.needs(u64::from(header.image_len), header.align));
            count += 1;
        }
    }
    (taken, count)
}

/// The header of the plugin file at `path`, if it has one this SDK reads
/// (its first [`HEADER_LEN`] bytes only: the rest is not checked).
pub fn read_header(storage: &dyn Storage, path: &str) -> Option<Header> {
    let mut head = [0u8; HEADER_LEN];
    read_exact(storage, path, 0, &mut head).ok()?;
    Header::read(&head).ok()
}

/// Which build a code plugin fits: a hash of what it was built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub u64);

impl Fingerprint {
    /// The fingerprint of plugin interface `interface` and of `build` (a
    /// hash of the chip and the features): the interface in the top 16
    /// bits, so that a file of another says which it was made for.
    pub const fn of(interface: u16, build: u64) -> Fingerprint {
        Fingerprint(((interface as u64) << 48) | (build & 0x0000_ffff_ffff_ffff))
    }

    /// The plugin interface it was made for (see [`INTERFACE`]).
    pub const fn interface(self) -> u16 {
        (self.0 >> 48) as u16
    }

    /// Why a plugin file of this fingerprint does not run on this build,
    /// in a sentence for the log and for authors; `None` where it fits.
    pub fn misfit(self) -> Option<String> {
        if self == FINGERPRINT {
            None
        } else if self.interface() != INTERFACE {
            Some(alloc::format!(
                "built for plugin interface {}, and this PaperPaw has {INTERFACE}: it needs a build for this one",
                self.interface()
            ))
        } else {
            Some(String::from(
                "built for another chip, or with other features, of the same plugin interface",
            ))
        }
    }

    /// From 16 hexadecimal digits, as the plugin library lists builds.
    pub fn parse(text: &str) -> Option<Fingerprint> {
        let text = text.trim();
        if text.len() != 16 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u64::from_str_radix(text, 16).ok().map(Fingerprint)
    }
}

/// 16 lower-case hexadecimal digits, as the plugin library lists builds
/// (`abi`).
impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// How this PaperPaw loads code plugins
/// ([`Env::native`](crate::Env)): the shell tells the kernel.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Native {
    /// The fingerprint plugin files must have to be loaded here.
    pub fingerprint: Fingerprint,
    /// What they must be built with ([`TOOLCHAIN`] of the shell's build).
    pub toolchain: Cow<'static, str>,
    /// The compiler plugin files must name ([`compiler::id`] of the
    /// release of the one that built the shell, [`Header::compiler`]); 0
    /// where the shell does not say.
    pub compiler: u32,
    /// The code's file in a plugin's folder, e.g. `code.ppn`.
    pub file: Cow<'static, str>,
    /// This start went without code plugins, and why.
    pub safe_mode: Option<SafeMode>,
    /// Plugins the shell had to install at this start and did not.
    pub not_installed: Vec<NotInstalled>,
    /// The bytes of the shell's [`Api`](paperpaw_native::Api): a plugin
    /// that needs more ([`Header::needs_api`]) needs a newer PaperPaw.
    pub api_size: u32,
    /// The revision of each family of shared functions the shell has
    /// (0: none); on RISC-V chips only (`paperpaw_table`).
    pub family_revs: paperpaw_table::Revisions,
    /// The oldest revision of each family whose plugins the shell runs.
    pub family_min: paperpaw_table::Revisions,
}

impl Native {
    /// Code plugins in files named `file`, fit for this build of the SDK.
    pub fn new(file: impl Into<Cow<'static, str>>) -> Self {
        Self {
            fingerprint: FINGERPRINT,
            toolchain: Cow::Borrowed(TOOLCHAIN),
            compiler: 0,
            file: file.into(),
            safe_mode: None,
            not_installed: Vec::new(),
            api_size: paperpaw_native::Api::SIZE,
            family_revs: paperpaw_table::REVISIONS,
            family_min: paperpaw_table::OLDEST,
        }
    }

    /// Whether a plugin file with `header` runs here: of this fingerprint,
    /// and of this compiler where the shell names one. Installers check
    /// it before a file is installed or placed, as the shell does before
    /// it loads one.
    pub fn fits(&self, header: &Header) -> bool {
        self.problem(header).is_none()
    }

    /// Why a plugin file with `header` does not run here, if it does not:
    /// built for an older or a newer plugin interface, needing more of
    /// the firmware than it has, or of another build.
    pub fn problem(&self, header: &Header) -> Option<PluginProblem> {
        if header.interface < INTERFACE {
            return Some(PluginProblem::OlderInterface {
                built_for: header.interface,
            });
        }
        let families = header.chip == paperpaw_native::Chip::Esp32C3;
        let misfit = if families {
            paperpaw_table::misfit(&header.families, &self.family_min, &self.family_revs)
        } else {
            None
        };
        if header.interface > INTERFACE
            || header.needs_api > self.api_size
            || misfit == Some(paperpaw_table::Misfit::Newer)
        {
            return Some(PluginProblem::NeedsNewerPaperPaw);
        }
        if misfit == Some(paperpaw_table::Misfit::Older) {
            return Some(PluginProblem::OlderInterface {
                built_for: header.interface,
            });
        }
        let compiler = self.compiler == 0 || header.compiler == self.compiler;
        (header.fingerprint != self.fingerprint.0 || !compiler).then_some(PluginProblem::OtherBuild)
    }

    /// Why a plugin file whose header [`Header::read`] refused with
    /// `problem` does not run: a file of an older format was built for an
    /// older PaperPaw ([`Header::read_older`] tells it from a damaged
    /// one), one of a newer format needs a newer PaperPaw.
    pub fn problem_of_file(head: &[u8], problem: paperpaw_native::Problem) -> PluginProblem {
        match problem {
            paperpaw_native::Problem::OlderFormat => match Header::read_older(head) {
                // Every fingerprint holds the interface in its top bits.
                Some(older) => PluginProblem::OlderInterface {
                    built_for: Fingerprint(older.fingerprint).interface(),
                },
                None => PluginProblem::Damaged,
            },
            paperpaw_native::Problem::NewerFormat => PluginProblem::NeedsNewerPaperPaw,
            paperpaw_native::Problem::UnknownChip => PluginProblem::OtherBuild,
            _ => PluginProblem::Damaged,
        }
    }

    /// The path of the code of the plugin in `folder`.
    pub fn path(&self, folder: &str) -> String {
        alloc::format!("{}/{}", folder.trim_end_matches('/'), self.file)
    }
}

/// A start without code plugins: when PaperPaw did not start twice with
/// them, or the reader asked for it with a key held at power-on. The code
/// plugins it switched off stay off until switched on again (in the
/// plugin manager), one after the other: one that crashes again is then
/// the one switched off.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SafeMode {
    pub reason: SafeModeReason,
    /// The code plugin loaded last before, if any.
    pub last_loaded: Option<String>,
    /// The code plugin the last crash was in, if it was in one.
    pub crashed: Option<String>,
    /// The code plugins it switched off.
    pub switched_off: Vec<Id>,
}

impl SafeMode {
    pub fn new(reason: SafeModeReason) -> Self {
        Self {
            reason,
            last_loaded: None,
            crashed: None,
            switched_off: Vec::new(),
        }
    }
}

/// Why a start went without code plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SafeModeReason {
    /// The last starts with code plugins did not get to their first
    /// frame.
    FailedStarts,
    /// The reader held the key for it at power-on.
    KeyHeld,
}

/// A plugin the shell had to install at this start (one the web flasher
/// wrote behind the firmware, which the reader chose there) and did not:
/// the plugin manager says which, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NotInstalled {
    pub plugin: Id,
    pub reason: NotInstalledReason,
}

impl NotInstalled {
    pub fn new(plugin: Id, reason: NotInstalledReason) -> Self {
        Self { plugin, reason }
    }
}

/// Why a plugin was not installed at the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotInstalledReason {
    /// Its code is signed by another author than the version installed
    /// before ([`Author::Other`]): not installed.
    OtherAuthor,
    /// Its code is damaged, or no plugin file this PaperPaw reads.
    Damaged,
}

/// The plugin a plugin file's entry made, for the shell that loaded it.
///
/// # Safety
///
/// `made` must be what [`Entry::make`](paperpaw_native::Entry) of a plugin
/// file with this build's [`FINGERPRINT`] returned, taken once.
pub unsafe fn take(made: *mut core::ffi::c_void) -> Option<alloc::boxed::Box<dyn crate::Plugin>> {
    if made.is_null() {
        return None;
    }
    // SAFETY: made by `plugin!` (`support::make`) from this box, with the
    // same layout: the fingerprint says the same plugin interface and
    // layouts, and the shell checked the compiler.
    Some(*unsafe {
        alloc::boxed::Box::from_raw(made.cast::<alloc::boxed::Box<dyn crate::Plugin>>())
    })
}

/// Makes a crate a native plugin file: defines its entry, its heap (the
/// kernel's) and its panic handler, around the plugin `$plugin` makes.
/// The crate is `#![no_std]`; for the X4 (`riscv32imc-unknown-none-elf`)
/// and the X4 Pro (`xtensa-esp32s3-none-elf`) a `#![no_main]` binary,
/// linked with `paperpaw-plugin.x` and packed by `paperpaw-sign pack`;
/// where a system loads libraries (a computer, Android) a `cdylib` with
/// the same entry. See the [module documentation](crate::native).
#[macro_export]
macro_rules! plugin {
    ($plugin:expr) => {
        /// The firmware's table, whose address the loader wrote into the
        /// word `paperpaw-plugin.x` makes before any code of the plugin
        /// ran.
        #[cfg(target_os = "none")]
        fn paperpaw_api() -> &'static $crate::native::support::Api {
            unsafe extern "C" {
                static PAPERPAW_API: *const $crate::native::support::Api;
            }
            // SAFETY: a table that lives as long as the firmware runs.
            unsafe { &*PAPERPAW_API }
        }

        /// Where the shell puts its table before it makes the plugin: a
        /// library the system loads may have data of its own.
        #[cfg(not(target_os = "none"))]
        #[unsafe(no_mangle)]
        pub static PAPERPAW_API: $crate::native::support::ApiSlot =
            $crate::native::support::ApiSlot::new();

        #[cfg(not(target_os = "none"))]
        fn paperpaw_api() -> &'static $crate::native::support::Api {
            PAPERPAW_API.get()
        }

        #[global_allocator]
        static PAPERPAW_HEAP: $crate::native::support::Heap =
            $crate::native::support::Heap(paperpaw_api);

        #[panic_handler]
        fn paperpaw_panic(info: &::core::panic::PanicInfo<'_>) -> ! {
            $crate::native::support::panic(paperpaw_api(), info)
        }

        /// A library a system loads takes `core` and `alloc` as the
        /// toolchain built them, for unwinding: their code names the
        /// unwinder's personality, which a plugin that never unwinds
        /// (`panic = "abort"`) does not have, and Linux loads no library
        /// with a name left open. Never called; should an unwind from a
        /// shell built to unwind ever reach the plugin, it panics, and the
        /// plugin's panics abort.
        #[cfg(all(not(target_os = "none"), panic = "abort"))]
        #[unsafe(no_mangle)]
        extern "C" fn rust_eh_personality() -> ! {
            ::core::panic!("unwinding into a plugin")
        }

        unsafe extern "C" fn paperpaw_make() -> *mut ::core::ffi::c_void {
            $crate::native::support::make($plugin)
        }

        #[unsafe(no_mangle)]
        #[cfg_attr(target_os = "none", unsafe(link_section = ".rodata.paperpaw_entry"))]
        pub static PAPERPAW_ENTRY: $crate::native::support::Entry =
            $crate::native::support::Entry {
                fingerprint: $crate::native::FINGERPRINT.0,
                make: paperpaw_make,
                needs_api: $crate::native::support::Api::SIZE as u16,
                interface: $crate::native::INTERFACE,
                families: $crate::native::support::REVISIONS,
            };
    };
}

mod hot;

/// The layouts [`LAYOUT`] digests, and the hash the fingerprint is made
/// with.
mod layout;

/// What [`plugin!`] expands to uses, and what a shell that loads plugins
/// as libraries hands them; not for plugins themselves.
#[doc(hidden)]
pub mod support {
    use alloc::boxed::Box;
    use core::alloc::{GlobalAlloc, Layout};
    use core::fmt::Write;
    use core::sync::atomic::{AtomicPtr, Ordering};

    pub use paperpaw_native::{Api, Entry};
    pub use paperpaw_table::REVISIONS;

    /// The shell's table, where a library the system loads keeps it
    /// (`PAPERPAW_API`): set before the plugin is made.
    #[derive(Debug)]
    pub struct ApiSlot(AtomicPtr<Api>);

    impl ApiSlot {
        pub const fn new() -> Self {
            Self(AtomicPtr::new(core::ptr::null_mut()))
        }

        /// Gives the plugin the shell's table (the shell, once, before it
        /// makes the plugin).
        pub fn set(&self, api: &'static Api) {
            self.0
                .store(core::ptr::from_ref(api).cast_mut(), Ordering::Release);
        }

        pub fn get(&self) -> &'static Api {
            // SAFETY: set by the shell before it made the plugin, to a
            // table that lives as long as the shell runs.
            unsafe { &*self.0.load(Ordering::Acquire) }
        }
    }

    impl Default for ApiSlot {
        fn default() -> Self {
            Self::new()
        }
    }

    /// The plugin's heap: the kernel's (through the table the function
    /// gives), so boxes cross both ways.
    #[derive(Debug)]
    pub struct Heap(pub fn() -> &'static Api);

    // SAFETY: the firmware's allocator, which keeps `GlobalAlloc`'s rules.
    unsafe impl GlobalAlloc for Heap {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            // SAFETY: as `GlobalAlloc::alloc`.
            unsafe { (self.0().alloc)(layout.size(), layout.align()) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: as `GlobalAlloc::dealloc`.
            unsafe { (self.0().dealloc)(ptr, layout.size(), layout.align()) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            // SAFETY: as `GlobalAlloc::realloc`.
            unsafe { (self.0().realloc)(ptr, layout.size(), layout.align(), new_size) }
        }
    }

    /// The plugin made, as [`take`](super::take) reads it back.
    pub fn make(plugin: impl crate::Plugin + 'static) -> *mut core::ffi::c_void {
        let plugin: Box<dyn crate::Plugin> = Box::new(plugin);
        Box::into_raw(Box::new(plugin)).cast()
    }

    /// Writes into a fixed buffer, cutting what does not fit.
    struct Cut {
        buf: [u8; 192],
        len: usize,
    }

    impl Write for Cut {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for c in s.chars() {
                let mut bytes = [0; 4];
                let bytes = c.encode_utf8(&mut bytes).as_bytes();
                if self.len + bytes.len() > self.buf.len() {
                    break;
                }
                self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
                self.len += bytes.len();
            }
            Ok(())
        }
    }

    /// The plugin panicked: the firmware's panic, with what it says.
    pub fn panic(api: &Api, info: &core::panic::PanicInfo<'_>) -> ! {
        let mut cut = Cut {
            buf: [0; 192],
            len: 0,
        };
        let _ = write!(cut, "{}", info.message());
        if let Some(at) = info.location() {
            let file = at.file().rsplit(['/', '\\']).next().unwrap_or(at.file());
            let _ = write!(cut, " ({file}:{})", at.line());
        }
        // SAFETY: the firmware's table; the message lives on the stack
        // until the firmware has taken it.
        unsafe { (api.panic)(cut.buf.as_ptr(), cut.len) }
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn code_fetched_for_an_update_has_a_folder_not_loaded() {
        assert_eq!(fetched_folder("stats"), "plugins/.stats.next");
        assert_eq!(fetched_plugin(".stats.next"), Some("stats"));
        for other in [
            "stats",
            ".stats.part",
            ".stats.old",
            "..next",
            ".Stats.next",
        ] {
            assert_eq!(fetched_plugin(other), None, "{other}");
        }
        let storage = crate::MemoryStorage::new();
        assert_eq!(fetched_code(&storage, "stats"), None);
        assert!(remember_fetched(&storage, "stats", &[1; 32]));
        assert_eq!(fetched_code(&storage, "stats"), Some([1; 32]));
        // Apart from the code installed.
        assert_eq!(installed_code(&storage, "stats"), None);
    }

    #[test]
    fn what_cannot_be_read_is_not_written_over() {
        let kept = crate::MemoryStorage::new();
        assert!(remember_author(&kept, "clock", &[1; 32]));
        assert!(remember_code(&kept, "clock", &[2; 32]));
        let authors = crate::read_file(&kept, AUTHORS, 1024).unwrap();
        let codes = crate::read_file(&kept, INSTALLED, 1024).unwrap();
        // The card fails: nothing kept, the other plugins' left as they were.
        let failing = crate::memory_storage::Unreadable(kept);
        assert!(!remember_author(&failing, "weather", &[3; 32]));
        assert!(!remember_code(&failing, "weather", &[4; 32]));
        assert_eq!(
            crate::read_file(&failing.0, AUTHORS, 1024).unwrap(),
            authors
        );
        assert_eq!(
            crate::read_file(&failing.0, INSTALLED, 1024).unwrap(),
            codes
        );
        // A file too large to read is not written over either.
        let large = crate::MemoryStorage::new().with(INSTALLED, "x".repeat(70 * 1024));
        assert!(!remember_code(&large, "clock", &[2; 32]));
        assert_eq!(large.size(INSTALLED), Ok(70 * 1024));
    }

    /// A plugin file's header of an image of `len` bytes aligned to
    /// `align`.
    fn header_of(len: u32, align: u32) -> alloc::vec::Vec<u8> {
        let header = Header {
            chip: Chip::Esp32C3,
            fingerprint: 1,
            base: paperpaw_native::LINK_BASE,
            image_len: len,
            code_at: 96,
            entry: 0,
            api: 32,
            relocs: 0,
            align,
            compiler: 1,
            hot_at: 0,
            hot_len: 0,
            patches: 0,
            needs_api: 0,
            families: [0; 8],
            interface: INTERFACE,
            sha256: [0; 32],
        };
        header.to_bytes().to_vec()
    }

    #[test]
    fn the_room_for_code_is_counted_as_the_device_places_it() {
        let room = CodeRoom {
            total: 40_960,
            used: 8192,
            granule: 4096,
            max_plugins: Some(3),
            placed: 1,
            shared: false,
        };
        // Whole granules, or the image's own alignment where larger.
        assert_eq!(room.needs(1, 16), 4096);
        assert_eq!(room.needs(4096, 16), 4096);
        assert_eq!(room.needs(4097, 16), 8192);
        assert_eq!(room.needs(100, 16_384), 16_384);
        assert_eq!(room.left(8192), 32_768);
        assert!(room.fits(8192, 1, 32_768));
        assert!(!room.fits(8192, 1, 32_769));
        // No more plugins than the device places.
        assert!(!room.fits(0, 3, 4096));
        // Kept by the shell, read by the store; none at all is said too.
        let storage = crate::MemoryStorage::new();
        assert_eq!(code_room(&storage), None);
        assert!(keep_code_room(&storage, Some(&room)));
        assert_eq!(code_room(&storage), Some(room));
        let shared = CodeRoom {
            max_plugins: None,
            shared: true,
            granule: 32,
            ..room
        };
        assert!(keep_code_room(&storage, Some(&shared)));
        assert_eq!(code_room(&storage), Some(shared));
        assert!(keep_code_room(&storage, None));
        assert_eq!(code_room(&storage), None);
        assert!(keep_code_room(&storage, None));
    }

    #[test]
    fn the_room_is_made_as_each_device_places_code() {
        // The X4: a region of 1 MB, its first sector the index, two
        // images up to 40 KB after it.
        let x4 = CodeRoom::in_flash(0x66_0000, 0x76_0000, 0x66_1000 + 40_960, 2);
        assert_eq!((x4.total, x4.used, x4.granule), (1_044_480, 40_960, 4096));
        assert_eq!((x4.placed, x4.max_plugins, x4.shared), (2, Some(77), false));
        // Nothing placed yet: none of it used; a region too small: none.
        assert_eq!(CodeRoom::in_flash(0, 0x10_0000, 0, 0).used, 0);
        assert_eq!(CodeRoom::in_flash(0x1000, 0x1000, 0, 0).total, 0);
        // The X4 Pro: what the images took and what was free beside them.
        let pro = CodeRoom::in_psram(53_000, 6_000_000, 1);
        assert_eq!((pro.total, pro.used, pro.granule), (6_053_000, 53_000, 32));
        assert_eq!((pro.max_plugins, pro.shared), (None, true));
    }

    #[test]
    fn installed_code_is_measured_by_its_headers() {
        let room = CodeRoom {
            total: 1 << 20,
            granule: 4096,
            ..CodeRoom::default()
        };
        let storage = crate::MemoryStorage::new()
            .with("plugins/clock/code.ppn", header_of(5000, 16))
            .with("plugins/clock/plugin.ini", "[plugin]\n")
            .with("plugins/theme/plugin.ini", "[plugin]\n")
            .with("plugins/broken/code.ppn", "PPNP but short")
            // A download being put together: not installed.
            .with("plugins/.clock.part/code.ppn", header_of(5000, 16))
            .with("plugins/weather/code.ppn", header_of(100, 16));
        // The theme has no code, the broken file no header.
        assert_eq!(
            installed_code_need(&storage, &room, "code.ppn"),
            (8192 + 4096, 2)
        );
        assert_eq!(
            read_header(&storage, "plugins/clock/code.ppn").map(|h| h.image_len),
            Some(5000)
        );
        assert!(read_header(&storage, "plugins/broken/code.ppn").is_none());
        assert_eq!(installed_code_need(&storage, &room, "code.so"), (0, 0));
    }

    #[test]
    fn code_is_named_as_installed() {
        let storage = crate::MemoryStorage::new();
        assert_eq!(installed_code(&storage, "clock"), None);
        assert!(remember_code(&storage, "clock", &[1; 32]));
        assert!(remember_author(&storage, "clock", &[2; 32]));
        assert!(remember_code(&storage, "clock", &[3; 32]));
        assert_eq!(installed_code(&storage, "clock"), Some([3; 32]));
        // Kept apart from the authors.
        assert_eq!(known_author(&storage, "clock"), Some([2; 32]));
        assert_eq!(installed_code(&storage, "clock-face"), None);
    }

    #[test]
    fn authors_are_known_by_plugin() {
        let storage = crate::MemoryStorage::new();
        assert_eq!(known_author(&storage, "clock"), None);
        assert_eq!(author(&storage, "clock", &[1; 32]), Author::New);
        assert!(remember_author(&storage, "clock", &[1; 32]));
        assert!(remember_author(&storage, "clock-face", &[2; 32]));
        assert_eq!(author(&storage, "clock", &[1; 32]), Author::Known);
        assert_eq!(author(&storage, "clock", &[2; 32]), Author::Other);
        // Another author installed anyway: the one from now on.
        assert!(remember_author(&storage, "clock", &[3; 32]));
        assert_eq!(known_author(&storage, "clock"), Some([3; 32]));
        assert_eq!(known_author(&storage, "clock-face"), Some([2; 32]));
    }

    #[test]
    fn fingerprints_as_text() {
        let fingerprint = Fingerprint(0x0085_073a_6132_3717);
        assert_eq!(fingerprint.to_string(), "0085073a61323717");
        assert_eq!(Fingerprint::parse("0085073a61323717"), Some(fingerprint));
        assert_eq!(Fingerprint::parse(" 0085073A61323717\n"), Some(fingerprint));
        assert_eq!(Fingerprint::parse("85073a61323717"), None);
        assert_eq!(Fingerprint::parse("0085073a6132371g"), None);
        assert_eq!(Fingerprint::parse("+085073a61323717"), None);
    }

    /// A plugin of another interface, or needing more than this
    /// PaperPaw has, is said to be older or to need a newer PaperPaw, not
    /// "another build".
    #[test]
    fn older_and_newer_plugins_are_said_as_such() {
        let native = Native::new("code.ppn");
        let header = Header::read(&file()).unwrap();
        let fits = Header {
            fingerprint: FINGERPRINT.0,
            ..header
        };
        assert_eq!(native.problem(&fits), None);
        assert_eq!(native.problem(&header), Some(PluginProblem::OtherBuild));
        let older = Header {
            interface: INTERFACE - 1,
            ..fits
        };
        assert_eq!(
            native.problem(&older),
            Some(PluginProblem::OlderInterface {
                built_for: INTERFACE - 1
            })
        );
        for newer in [
            Header {
                interface: INTERFACE + 1,
                ..fits
            },
            Header {
                needs_api: native.api_size + 4,
                ..fits
            },
            Header {
                families: [native.family_revs[0] + 1, 0, 0, 0, 0, 0, 0, 0],
                ..fits
            },
        ] {
            assert_eq!(
                native.problem(&newer),
                Some(PluginProblem::NeedsNewerPaperPaw)
            );
        }
        // A file of an older format: its fingerprint names its interface.
        let mut old = file();
        old[4] = 3;
        old[8..16].copy_from_slice(&Fingerprint::of(4, 7).0.to_le_bytes());
        let problem = paperpaw_native::Problem::OlderFormat;
        assert_eq!(
            Native::problem_of_file(&old, problem),
            PluginProblem::OlderInterface { built_for: 4 }
        );
        assert_eq!(
            Native::problem_of_file(b"PP", problem),
            PluginProblem::Damaged
        );
        assert_eq!(
            Native::problem_of_file(&[], paperpaw_native::Problem::NewerFormat),
            PluginProblem::NeedsNewerPaperPaw
        );
    }

    /// A small plugin file of `Header`'s own tests: no code to run, only
    /// to check.
    fn file() -> Vec<u8> {
        let mut header = Header {
            chip: Chip::Esp32C3,
            fingerprint: 1,
            base: paperpaw_native::LINK_BASE,
            image_len: 128,
            code_at: 96,
            entry: 0,
            api: 32,
            relocs: 0,
            align: 16,
            compiler: 1,
            hot_at: 0,
            hot_len: 0,
            patches: 0,
            needs_api: 0,
            families: [0; 8],
            interface: INTERFACE,
            sha256: [0; 32],
        };
        let image = [9u8; 128];
        let mut sha = header.digest();
        sha.update(&image);
        header.sha256 = sha.finish();
        let mut file = header.to_bytes().to_vec();
        file.extend_from_slice(&image);
        file
    }

    #[test]
    fn plugin_files_on_the_storage_are_checked_whole() {
        let storage = crate::MemoryStorage::new();
        let unsigned = file();
        let mut signed = unsigned.clone();
        let author = signature::sign(&unsigned, &[2; 32]);
        signed.extend_from_slice(&author.to_bytes());
        storage.write("a.ppn", &unsigned).unwrap();
        storage.write("b.ppn", &signed).unwrap();
        assert_eq!(check_file(&storage, "a.ppn").map(|(_, s)| s), Ok(None));
        assert_eq!(
            check_file(&storage, "b.ppn").map(|(_, s)| s),
            Ok(Some(author))
        );
        let mut changed = signed.clone();
        changed[100] ^= 1;
        storage.write("c.ppn", &changed).unwrap();
        assert_eq!(check_file(&storage, "c.ppn"), Err(Problem::Damaged));
        let mut resigned = signed.clone();
        resigned[signed.len() - 30] ^= 1;
        storage.write("d.ppn", &resigned).unwrap();
        assert_eq!(check_file(&storage, "d.ppn"), Err(Problem::Signature));
        storage.write("e.ppn", &signed[..signed.len() - 1]).unwrap();
        assert_eq!(check_file(&storage, "e.ppn"), Err(Problem::Length));
        assert_eq!(check_file(&storage, "none.ppn"), Err(Problem::Length));
    }

    /// The web flasher's bundle installs only what `install` would: the
    /// same rules, kept in two places as the format crate is below the SDK.
    #[test]
    fn bundles_take_the_names_install_takes() {
        use paperpaw_native::bundle::{is_plugin_id, is_safe_path};
        let ids = [
            "a",
            "reading-nook",
            "a1",
            "",
            "-a",
            "a-",
            "A",
            "a.b",
            "a_b",
            "\u{e9}",
        ];
        let long_id = "a".repeat(41);
        for id in ids.iter().copied().chain([long_id.as_str(), &long_id[1..]]) {
            assert_eq!(
                is_plugin_id(id),
                crate::install::is_plugin_name(id),
                "{id:?}"
            );
        }
        let paths = [
            "plugin.ini",
            "i18n/de.ini",
            "fonts/A_b-c.ttf",
            "",
            "/a",
            "a/",
            "a//b",
            "../a",
            ".a",
            "a/.b",
            "a b",
            "a\\b",
            "a:b",
        ];
        let long_path = "a".repeat(101);
        for path in paths
            .iter()
            .copied()
            .chain([long_path.as_str(), &long_path[1..]])
        {
            assert_eq!(
                is_safe_path(path),
                crate::install::is_safe_path(path),
                "{path:?}"
            );
        }
    }

    #[test]
    fn this_build_names_itself() {
        let native = Native::new("code.ppn");
        assert_eq!(native.fingerprint, FINGERPRINT);
        assert_eq!(native.toolchain, TOOLCHAIN);
        assert_eq!(native.path("plugins/hello/"), "plugins/hello/code.ppn");
    }

    /// A file of this interface fits, of another it says which; the
    /// interface is in the fingerprint's top bits.
    #[test]
    fn a_file_of_another_interface_says_so() {
        assert_eq!(FINGERPRINT.interface(), INTERFACE);
        assert_eq!(FINGERPRINT.misfit(), None);
        let build = FINGERPRINT.0 & 0x0000_ffff_ffff_ffff;
        let next = Fingerprint::of(INTERFACE + 1, build);
        let message = next.misfit().unwrap();
        assert!(
            message.contains(&alloc::format!("plugin interface {}", INTERFACE + 1)),
            "{message}"
        );
        assert!(
            message.contains(&alloc::format!("has {INTERFACE}")),
            "{message}"
        );
        let other = Fingerprint::of(INTERFACE, build ^ 1);
        assert!(other.misfit().unwrap().contains("same plugin interface"));
    }
}
