//! Native code plugins (ADR 0024): a plugin built into a file that a
//! device loads at its start, without a new firmware. It runs as part of
//! PaperPaw, with no sandbox, and its code is the same as a plugin built
//! in: one more crate makes the file, with
//! [`native_plugin!`](crate::native_plugin).
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
//! takes a restart. Where PaperPaw loads code plugins, `Place::native`
//! says with what (the store takes the build that fits), and whether this
//! start went without them ([`SafeMode`]).
//!

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
pub const INTERFACE: u16 = 7;

/// The draft of [`INTERFACE`] this build is, while the owner has not frozen
/// it. A draft may change what plugins see without a new interface, but
/// each such change takes the next draft, and the draft is part of
/// [`FINGERPRINT`]: a plugin built for an earlier draft is refused, never
/// loaded. `crates/paperpaw-sdk/interface.txt` keeps it as `draft`.
/// Freezing deletes this constant and that line: the fingerprint is then
/// made as if there had never been drafts, and refuses every draft's
/// builds too.
pub const DRAFT: u16 = 37;

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
    "paperpaw_sort",
];

/// The layouts of the types that cross between a plugin and PaperPaw, as
/// this build lays them out: a digest of their sizes and alignments, and
/// of where the firmware's table keeps each of its entries. Part of the
/// fingerprint, so made for each chip as it lays them out: a plugin whose
/// layouts differ from the firmware's is not loaded. Not only the
/// kernel's: `shape.txt` pins its value, which changes with a layout.
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
    // Only while a draft: a frozen interface's fingerprint is made as if
    // there had never been drafts (freezing deletes `DRAFT`).
    if DRAFT != 0 {
        hash = hash.number(DRAFT as u64);
    }
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
/// Pro, `code-host.dll`, `.so` or `.dylib` built for a computer (the
/// desktop emulator), and `code.so`, which no shipped loader reads
/// (Android loads no code plugins, ADR 0024). A folder with one is a code
/// plugin's, also where PaperPaw loads none: it is then listed as
/// unsupported.
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

/// Checks the plugin file at `path` whole, reading it once: its header,
/// its length, its digest and, if it is signed, its author's signature.
/// Its header and its author's signature (`None` if it is not signed);
/// whether it fits this device and build is for the caller to see
/// ([`Header::chip`], [`Header::fingerprint`]), and whether the author is
/// one known (a store remembers the key of the first install).
/// `Problem::Other(1)` where there was no memory to check the author's
/// signature: the file is not judged.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn check_file(
    storage: &dyn Storage,
    path: &str,
) -> Result<(Header, Option<Signature>), Problem> {
    checked_file(storage, path)
}

/// The [`Problem::Other`] code of a check that had no memory for itself
/// (the author's check, about 700 bytes): the file is not judged, and a
/// later check may well hold. Not a signature that fails (#250).
#[cfg(any(test, feature = "kernel"))]
pub const NO_MEMORY: u16 = 1;
/// As the kernel's, for the SDK's own checks.
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) const NO_MEMORY: u16 = 1;

/// [`check_file`], for the marks an installer left ([`install::marks`](crate::install::marks)).
/// [`Problem::Other`]`(`[`NO_MEMORY`]`)` where the author's check found no
/// memory.
pub(crate) fn checked_file(
    storage: &dyn Storage,
    path: &str,
) -> Result<(Header, Option<Signature>), Problem> {
    let mut head = [0u8; HEADER_LEN];
    read_exact(storage, path, 0, &mut head)?;
    let header = Header::read(&head)?;
    let mut whole = Whole::before_interface_7(head, &header, storage, path)?;
    let signature = whole.check(storage, path)?;
    Ok((header, signature))
}

/// A plugin file of either format as its header describes it, to check
/// whole: the header's bytes (the first `head_len` of `head`), where the
/// file ends without its author's signature, whether one follows, the
/// SHA-256 the header carries and the digest begun with the header's part.
struct Whole {
    head: [u8; HEADER_LEN],
    head_len: usize,
    end: u64,
    signed: bool,
    sha256: [u8; 32],
    digest: paperpaw_digest::sha256::Sha256,
}

impl Whole {
    /// A file of the format before interface 7, by its header (`head` its
    /// bytes).
    fn before_interface_7(
        head: [u8; HEADER_LEN],
        header: &Header,
        storage: &dyn Storage,
        path: &str,
    ) -> Result<Whole, Problem> {
        let size = storage.size(path).map_err(|_| Problem::Length)?;
        Ok(Whole {
            head,
            head_len: HEADER_LEN,
            end: header.file_len(),
            signed: header.signed(size)?,
            sha256: header.sha256,
            digest: header.digest(),
        })
    }

    /// Nothing read yet (see [`Whole::read`]).
    #[cfg(feature = "kernel")]
    fn unread() -> Whole {
        Whole {
            head: [0; HEADER_LEN],
            head_len: 0,
            end: 0,
            signed: false,
            sha256: [0; 32],
            digest: paperpaw_digest::sha256::Sha256::new(),
        }
    }

    /// Reads the header of the file at `path`, of interface 7 (format 5) or
    /// of the format before, into this. Not inlined, and into the caller's
    /// place: what reading either header takes (the headers themselves,
    /// some 600 bytes on the X4) is gone from the stack before the file is
    /// checked, an install's deepest calls (#250).
    #[cfg(feature = "kernel")]
    #[inline(never)]
    fn read(&mut self, storage: &dyn Storage, path: &str) -> Result<(), Problem> {
        read_exact(storage, path, 0, &mut self.head)?;
        match Header::read(&self.head) {
            Ok(header) => {
                *self = Whole::before_interface_7(self.head, &header, storage, path)?;
            }
            Err(Problem::NewerFormat) => {
                // The same bytes again, as far as this header goes.
                let (head5, header) = read_header5(storage, path)?;
                self.head_len = head5.len();
                self.end = header.file_len();
                self.signed = header.signed;
                self.sha256 = header.sha256;
                self.digest = header.digest();
            }
            Err(problem) => return Err(problem),
        }
        Ok(())
    }

    /// Reads the file once after its header: its digest and, if it is
    /// signed, its author's signature over all of it.
    /// [`Problem::Other`]`(`[`NO_MEMORY`]`)` where the author's check found
    /// no memory. Not inlined: one frame for both formats, under the
    /// caller's small one.
    #[inline(never)]
    fn check(&mut self, storage: &dyn Storage, path: &str) -> Result<Option<Signature>, Problem> {
        let head_len = self.head_len;
        let end = self.end;
        let signature = if self.signed {
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
            Some(signature) => Some(author_check(signature)?),
            None => None,
        };
        if let Some([check]) = author.as_deref_mut() {
            check.absorb(&self.head[..head_len]);
        }
        let mut buf = [0u8; 512];
        let mut at = head_len as u64;
        while at < end {
            let len = (end - at).min(buf.len() as u64) as usize;
            read_exact(storage, path, at, &mut buf[..len])?;
            self.digest.update(&buf[..len]);
            if let Some([check]) = author.as_deref_mut() {
                check.absorb(&buf[..len]);
            }
            at += len as u64;
        }
        let digest = core::mem::replace(&mut self.digest, paperpaw_digest::sha256::Sha256::new());
        if digest.finish() != self.sha256 {
            return Err(Problem::Damaged);
        }
        if let Some(check) = author {
            let [check] = *check;
            check.finish().map_err(|_| Problem::Signature)?;
        }
        Ok(signature)
    }
}

/// The header of an interface-7 plugin file (format 5) at `path`, read for
/// what it says of the file itself (its length, its digest), whatever chip
/// and minor it is made for: whether it runs here is the loader's to say.
/// The header's bytes with it.
#[cfg(feature = "kernel")]
fn read_header5(
    storage: &dyn Storage,
    path: &str,
) -> Result<
    (
        [u8; paperpaw_native::header5::HEADER_LEN],
        paperpaw_native::header5::Header5,
    ),
    Problem,
> {
    use paperpaw_native::header5::{Header5, Host, Refusal};
    let mut head = [0u8; paperpaw_native::header5::HEADER_LEN];
    read_exact(storage, path, 0, &mut head)?;
    let size = storage.size(path).map_err(|_| Problem::Length)?;
    let host = match Chip::from_id(u16::from_le_bytes([head[6], head[7]])) {
        Some(Chip::Esp32C3) => Host::x4(u16::MAX),
        Some(Chip::Esp32S3) => Host::x4_pro(u16::MAX),
        _ => return Err(Problem::UnknownChip),
    };
    let header = Header5::read(&head, size, &host).map_err(|why| match why {
        Refusal::NotAPlugin => Problem::NotAPlugin,
        Refusal::Truncated | Refusal::Length => Problem::Length,
        Refusal::OlderFormat | Refusal::OlderInterface => Problem::OlderFormat,
        Refusal::NewerFormat | Refusal::NewerInterface => Problem::NewerFormat,
        Refusal::UnknownChip | Refusal::NoProfile | Refusal::OtherChip => Problem::UnknownChip,
        _ => Problem::Malformed,
    })?;
    Ok((head, header))
}

/// The SHA-256 the header of the code file at `path` carries, of a file
/// of interface 7 (format 5) or of the format before: what names the code
/// installed ([`remember_code`]). Its header only: the rest is not checked.
/// Not inlined: the headers it reads stay out of its caller's frame, which
/// an install's deepest calls stand on (#250).
#[cfg(feature = "kernel")]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
#[inline(never)]
pub fn code_sha256(storage: &dyn Storage, path: &str) -> Option<[u8; 32]> {
    match read_header(storage, path) {
        Some(header) => Some(header.sha256),
        None => read_header5(storage, path)
            .ok()
            .map(|(_, header)| header.sha256),
    }
}

/// [`check_file`] for a code file of interface 7 (format 5) or of the
/// format before: the file read once and checked whole (its length, its
/// digest and, if it is signed, its author's signature). The SHA-256 its
/// header carries and the signature (`None` if it is not signed); whether
/// it runs on this device is for the loader to see.
#[cfg(feature = "kernel")]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn check_code(
    storage: &dyn Storage,
    path: &str,
) -> Result<([u8; 32], Option<Signature>), Problem> {
    let mut whole = Whole::unread();
    whole.read(storage, path)?;
    let sha256 = whole.sha256;
    let signature = whole.check(storage, path)?;
    Ok((sha256, signature))
}

/// The author's check of a signed file, on the heap. Not inlined: the check
/// (about 700 bytes on the X4) is made in this call's frame alone, so the
/// caller's frame does not hold it twice while the signature's curve is
/// decoded, the X4 start's and installs' deepest calls (#250). Without
/// memory for it: [`NO_MEMORY`], not a failed signature.
#[inline(never)]
fn author_check(
    signature: &Signature,
) -> Result<alloc::boxed::Box<[signature::SignatureCheck; 1]>, Problem> {
    let check = signature.check().map_err(|_| Problem::Signature)?;
    let mut place = Vec::new();
    place
        .try_reserve_exact(1)
        .map_err(|_| Problem::Other(NO_MEMORY))?;
    place.push(check);
    // Exactly one place: no reallocation, and the length always fits.
    alloc::boxed::Box::try_from(place.into_boxed_slice()).map_err(|_| Problem::Other(NO_MEMORY))
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
/// known before; `false` if it could not be kept (the file left as it is
/// where it could not be read). The installs call it after the file is
/// in place: a key other than the known one was refused before
/// ([`Author::Other`]), so nothing replaces a key that is known.
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

mod batch;
mod binding;
#[doc(hidden)]
pub use binding::entry as __entry;
pub use binding::entry::Linked;
pub use binding::heap as __heap;
pub use binding::{Error, Host, Plugin};
pub mod callbacks;
pub mod canvas;
pub mod clock;
pub mod commands;
pub mod convert;
pub mod declare;
pub mod device;
pub mod document;
pub mod events;
pub mod files;
pub mod fonts;
pub mod format;
pub mod formatting;
pub mod i18n;
pub mod image;
pub mod keys;
pub mod library;
pub mod lookup;
pub mod net;
pub mod page_layers;
pub mod registrations;
/// Borrowed registry rows and lossless generation-bound paging.
pub mod registry;
pub mod reply;
pub mod secrets;
pub mod services;
pub mod settings;
pub mod snapshot;
pub mod sockets;
pub mod stages;
pub mod storage;
pub mod text;
mod text_part;
pub mod ui;
mod wire;
pub mod work;

#[cfg(test)]
#[path = "native/slot_limits_tests.rs"]
mod slot_limits_tests;

#[cfg(feature = "kernel")]
mod installed;
#[cfg(feature = "kernel")]
pub use installed::InstalledCodes;

#[cfg(any(test, feature = "kernel"))]
mod need;
#[cfg(any(test, feature = "kernel"))]
pub use need::{InstalledNeed, installed_code_need};

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
    paperpaw_digest::hex::decode_array(hex.trim())
}

/// Keeps `value` for `plugin` in `file`, in place of what was kept. A
/// file that is there but cannot be read (a failing card, too large) is
/// not written over: it holds every other plugin's.
fn keep_entry(storage: &dyn Storage, file: &str, plugin: &str, value: &[u8; 32]) -> bool {
    write_entry(storage, file, plugin, Some(value))
}

/// `plugin`'s line in `file` as [`keep_entry`] writes it, or none.
fn write_entry(storage: &dyn Storage, file: &str, plugin: &str, value: Option<&[u8; 32]>) -> bool {
    let Ok(kept) = crate::files::load_text_for_update(storage, file, 64 * 1024) else {
        return false;
    };
    let kept = kept.unwrap_or_default();
    let mut text: String = kept
        .lines()
        .filter(|line| line.split('\t').next() != Some(plugin))
        .map(|line| alloc::format!("{line}\n"))
        .collect();
    if let Some(value) = value {
        text.push_str(&alloc::format!(
            "{plugin}\t{}\n",
            paperpaw_digest::hex::encode(value)
        ));
    }
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
    /// The room a full flash would give, in bytes, where this device runs
    /// a smaller partition layout than PaperPaw's own (the X4 after an SD
    /// install from CrossPoint): the store and the plugin manager then say
    /// that the web flasher gives more. `None` where it is PaperPaw's own
    /// or none is known.
    pub larger: Option<u64>,
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
            larger: None,
        }
    }

    /// The same, in an app slot of `slot` bytes where PaperPaw's own layout
    /// has slots of `own` bytes: a smaller slot's room would grow by the
    /// difference with a full flash ([`larger`](Self::larger)).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn in_slot(mut self, slot: u32, own: u32) -> Self {
        self.larger = (slot < own).then(|| self.total.saturating_add(u64::from(own - slot)));
        self
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
    // Written by later shells only (0: none).
    let larger = number().filter(|&l| l > 0);
    Some(CodeRoom {
        total,
        used,
        granule: u32::try_from(granule).ok()?.max(1),
        max_plugins: u32::try_from(max).ok().filter(|&m| m > 0),
        placed: u32::try_from(placed).ok()?,
        shared: shared != 0,
        larger,
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
        "{} {} {} {} {} {} {}\n",
        room.total,
        room.used,
        room.granule,
        room.max_plugins.unwrap_or(0),
        room.placed,
        u8::from(room.shared),
        room.larger.unwrap_or(0)
    );
    // Not written again as it is (#305): the room is the same at most
    // starts, and a write is a file made anew on the card. The write makes
    // the folder where it is not there.
    let mut kept = [0u8; 96];
    let same = text.len() < kept.len()
        && storage.read_at(ROOM, 0, &mut kept).ok() == Some(text.len())
        && kept[..text.len()] == *text.as_bytes();
    same || storage.write(ROOM, text.as_bytes()).is_ok()
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
                "built for another chip, with other features or for another draft, of the same plugin interface",
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
/// (`Place::native`): the shell tells the kernel.
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
    /// Not used: the revisions of the shared function tables, which are
    /// gone. All 0; kept so that the fingerprint stays as it is.
    pub family_revs: [u16; 8],
    /// Not used, as [`family_revs`](Self::family_revs).
    pub family_min: [u16; 8],
    /// The interface-7 minor this PaperPaw loads: a format-5 plugin file
    /// runs here when its `needs_minor` is at most this (a store compares
    /// the catalogue's `abi` "7.<needs_minor>" with it).
    pub minor: u16,
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
            family_revs: [0; 8],
            family_min: [0; 8],
            minor: paperpaw_native::host_glue::MINOR,
        }
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
    /// The reader held Power for the 8 s escape while a call into a code
    /// plugin ran past the grace, and the device was reset:
    /// [`SafeMode::crashed`] names that plugin, which is switched off.
    PowerEscape,
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

mod hot;

/// The layouts [`LAYOUT`] digests, and the hash the fingerprint is made
/// with.
mod layout;

#[cfg(test)]
pub(crate) mod tests;

/// Generated host-stage payload records and their local tag tables.
#[path = "native/wire/stages_wire.rs"]
#[allow(clippy::all, clippy::pedantic, reason = "generated stage wire codecs")]
pub mod stage_schemas;
