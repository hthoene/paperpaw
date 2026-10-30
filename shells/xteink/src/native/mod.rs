//! Native code plugins from the card (ADR 0024): the kernel asks for the
//! code of each code plugin switched on (`plugins/<id>/code.ppn`) at the
//! start; [`Loader`] checks the file (`paperpaw_native`), puts its image
//! where it runs, relocated, and makes its plugin.
//!
//! - **X4 Pro and X4 Classic** (ESP32-S3): the image is read into PSRAM
//!   and runs from there, through
//!   the instruction bus's view of the same memory (`psram`).
//! - **X4**: the image is written once into the free end of the app
//!   partition the firmware runs from and mapped through free MMU entries,
//!   so its code runs from flash and only its data takes RAM (`flash`).
//!   Its hot code runs from an arena of internal RAM, as far as it holds
//!   it (`hot`).
//!
//! Plugins are never unloaded: switching one takes a restart. Where each
//! lies is kept ([`attribute`]), so a panic's calls inside a plugin are
//! written with its name and the offset into its file.

pub(crate) mod api;
pub mod bundle;
#[cfg(feature = "esp32c3")]
mod flash;
#[cfg(feature = "esp32c3")]
mod hot;
mod partition;
#[cfg(feature = "esp32s3")]
mod psram;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use critical_section::Mutex;
use log::{info, warn};
use paperpaw_core::{CodeFile, CodeLoader};
use paperpaw_native::{CODE_OFFSET, Chip, Entry, HEADER_LEN, Header, Problem};
use paperpaw_platform::Storage;
use paperpaw_sdk::native::{FINGERPRINT, TOOLCHAIN};
use paperpaw_sdk::{Plugin, PluginProblem};

/// The fingerprint plugin files must have, under a name of its own in the
/// firmware's ELF file: the release takes it from there (`paperpaw-sign
/// fingerprint`), so it is the one compared here, however it is made.
#[used]
#[unsafe(export_name = "PAPERPAW_NATIVE_FINGERPRINT")]
static EXPORTED_FINGERPRINT: [u8; 8] = FINGERPRINT.0.to_le_bytes();

/// The compiler plugin files must be built with: this firmware's own, by
/// its release (`build.rs`).
const COMPILER: &str = env!("PAPERPAW_RUSTC");

/// How this firmware loads code plugins: files named `code.ppn` of its
/// fingerprint and of its compiler.
pub fn this_firmware() -> paperpaw_sdk::native::Native {
    let mut native = paperpaw_sdk::native::Native::new("code.ppn");
    native.compiler = paperpaw_native::compiler::id(COMPILER);
    native
}

/// The internal RAM kept for plugins' hot code (the X4's arena), which
/// the heap gets once plugins are loaded, as far as they did not take it.
#[cfg(feature = "esp32c3")]
pub const HOT_ARENA_LEN: usize = hot::LEN;
#[cfg(feature = "esp32s3")]
pub const HOT_ARENA_LEN: usize = 0;

/// Gives the arena for hot code to the heap: no code plugins are loaded
/// at this start.
pub fn no_hot_code() {
    #[cfg(feature = "esp32c3")]
    hot::release(0);
}

/// The chip plugin files must be made for.
#[cfg(feature = "esp32c3")]
const CHIP: Chip = Chip::Esp32C3;
#[cfg(feature = "esp32s3")]
const CHIP: Chip = Chip::Esp32S3;

/// A plugin's image, where it runs.
struct Image {
    /// The plugin's id, as its `plugin.ini` says.
    id: String,
    /// The instruction address of the image's first byte.
    code_base: u32,
    len: u32,
    /// Where its hot code runs, if not in place (0), where it lies in the
    /// image and its length.
    hot: u32,
    hot_at: u32,
    hot_len: u32,
    /// What loading it took: microseconds to place it and to make its
    /// plugin, bytes of the heap and of its internal RAM.
    took: Option<Took>,
}

#[derive(Debug, Clone, Copy)]
struct Took {
    place_us: u64,
    make_us: u64,
    heap: isize,
    internal: isize,
    /// Bytes of hot code run from internal RAM, which the heap does not
    /// get.
    hot: u32,
}

/// Every image loaded, for naming the plugin a panic's call was in.
static IMAGES: Mutex<RefCell<Vec<Image>>> = Mutex::new(RefCell::new(Vec::new()));

/// Where images go, as found at the start (for [`describe`]).
#[cfg(feature = "remote")]
#[derive(Debug, Clone, Copy)]
pub enum Room {
    /// Not looked for yet: no code plugin was loaded.
    Unknown,
    /// PSRAM (the X4 Pro).
    #[cfg(feature = "esp32s3")]
    Psram,
    /// Flash from `start` to `end`, mapped at `base` (the X4).
    #[cfg(feature = "esp32c3")]
    Flash { start: u32, end: u32, base: u32 },
    /// None, and why.
    #[cfg(feature = "esp32c3")]
    None(&'static str),
}

#[cfg(feature = "remote")]
static ROOM: Mutex<core::cell::Cell<Room>> = Mutex::new(core::cell::Cell::new(Room::Unknown));

/// Notes where images go.
#[cfg(feature = "remote")]
fn set_room(room: Room) {
    critical_section::with(|cs| ROOM.borrow(cs).set(room));
}

/// What there is to know of native plugins, a line each: the
/// fingerprint, where images go, and each image loaded (for the remote
/// control over USB).
#[cfg(feature = "remote")]
pub fn describe(line: &mut dyn FnMut(&str)) {
    line(&alloc::format!(
        "fingerprint {FINGERPRINT} {TOOLCHAIN}, rustc {COMPILER}"
    ));
    line(&alloc::format!(
        "shared functions: revisions {:?}, oldest run {:?}, tables {}",
        api::API.family_revs,
        api::API.family_min,
        paperpaw_table::HASH
    ));
    let room = critical_section::with(|cs| ROOM.borrow(cs).get());
    line(&match room {
        Room::Unknown => String::from("room unknown"),
        #[cfg(feature = "esp32s3")]
        Room::Psram => String::from("room psram"),
        #[cfg(feature = "esp32c3")]
        Room::Flash { start, end, base } => {
            alloc::format!("room flash {start:#x}..{end:#x} at {base:#x}")
        }
        #[cfg(feature = "esp32c3")]
        Room::None(why) => alloc::format!("room none: {why}"),
    });
    critical_section::with(|cs| {
        for image in IMAGES.borrow_ref(cs).iter() {
            let took = image.took.map_or_else(String::new, |t| {
                alloc::format!(
                    ", placed in {} us, made in {} us, heap {:+} B, internal RAM {:+} B, hot code {} B",
                    t.place_us,
                    t.make_us,
                    t.heap,
                    t.internal,
                    t.hot
                )
            });
            line(&alloc::format!(
                "image {} {} bytes, code at {:#x}{took}",
                image.id,
                image.len,
                image.code_base
            ));
        }
    });
}

/// The plugin whose code the instruction address `pc` lies in, and the
/// offset into its file (the image begins after the header): with the
/// plugin's ELF, its function is at `0x3C000000 + 0x06000000 + offset -
/// HEADER_LEN`. Code in the arena of hot code is its plugin's, at its
/// place in the image. Called from the panic handler: allocates nothing.
pub fn attribute(pc: u32, found: &mut dyn FnMut(&str, u32)) {
    critical_section::with(|cs| {
        if let Ok(images) = IMAGES.borrow(cs).try_borrow() {
            for image in images.iter() {
                let hot = pc.wrapping_sub(image.hot);
                let offset = if image.hot != 0 && hot < image.hot_len {
                    image.hot_at + hot
                } else {
                    pc.wrapping_sub(image.code_base)
                };
                if offset < image.len {
                    found(&image.id, offset + HEADER_LEN as u32);
                    return;
                }
            }
        }
    });
}

/// Where images go: PSRAM on the ESP32-S3 devices, flash on the X4.
#[cfg(feature = "esp32s3")]
pub type Place = psram::Psram;
#[cfg(feature = "esp32c3")]
pub type Place = flash::Flash;

/// Loads code plugins for the kernel (see the [module documentation](self)).
pub struct Loader {
    place: Place,
}

impl Loader {
    pub fn new(place: Place) -> Self {
        Self { place }
    }

    fn load_file(
        &mut self,
        storage: &dyn Storage,
        code: &CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        let path = code.path;
        let heap = esp_alloc::HEAP.used();
        let internal = internal_free();
        let started = crate::micros();
        let mut head = [0; HEADER_LEN];
        read_exact(storage, path, 0, &mut head).map_err(|_| PluginProblem::Damaged)?;
        let header = Header::read(&head).map_err(|p| problem_of(&head, p))?;
        if header.chip != CHIP {
            warn!(
                "native: {path} is for chip {}, this firmware's is {CHIP}",
                header.chip
            );
            return Err(PluginProblem::OtherBuild);
        }
        // Built for another plugin interface, or needing more of this
        // firmware than it has: said as such, not as another build.
        if header.interface != paperpaw_sdk::native::INTERFACE
            || header.needs_api > paperpaw_native::Api::SIZE
        {
            warn!(
                "native: {path} is built for plugin interface {} and needs {} bytes of the table; \
                 this firmware has {} and {}",
                header.interface,
                header.needs_api,
                paperpaw_sdk::native::INTERFACE,
                paperpaw_native::Api::SIZE
            );
            return Err(if header.interface < paperpaw_sdk::native::INTERFACE {
                PluginProblem::OlderInterface {
                    built_for: header.interface,
                }
            } else {
                PluginProblem::NeedsNewerPaperPaw
            });
        }
        // The families of shared functions it calls in this firmware: not
        // a newer revision than this has, nor one before a change old
        // plugins would see. Refused before anything is placed.
        if let Some(misfit) = paperpaw_table::misfit(
            &header.families,
            &api::API.family_min,
            &api::API.family_revs,
        ) {
            warn!(
                "native: {path} calls revisions {:?} of the shared functions; this firmware \
                 has {:?} and runs plugins from {:?} on",
                header.families,
                api::API.family_revs,
                api::API.family_min
            );
            return Err(match misfit {
                paperpaw_table::Misfit::Older => PluginProblem::OlderInterface {
                    built_for: header.interface,
                },
                // Newer, and any reason a later SDK adds: the firmware is
                // to be updated.
                _ => PluginProblem::NeedsNewerPaperPaw,
            });
        }
        if let Some(why) = paperpaw_sdk::native::Fingerprint(header.fingerprint).misfit() {
            warn!("native: {path} is {why} ({FINGERPRINT}, {TOOLCHAIN})");
            return Err(PluginProblem::OtherBuild);
        }
        if header.compiler != paperpaw_native::compiler::id(COMPILER) {
            warn!(
                "native: {path} is built by another compiler than rustc {COMPILER}, this firmware's"
            );
            return Err(PluginProblem::OtherBuild);
        }
        // The file installed, its content checked against its header
        // where it is placed: not one put in its folder since.
        if header.sha256 != code.sha256 {
            warn!("native: {path} is not the file installed");
            return Err(PluginProblem::Changed);
        }
        // Signed by its author or not: the store and the bundle checked
        // the signature when they installed it.
        let size = storage.size(path).map_err(|_| PluginProblem::Damaged)?;
        header.signed(size).map_err(problem)?;
        let words = read_words(storage, path, &header)?;
        let (base, hot) = self.place.put(storage, path, &header, &words)?;
        drop(words);
        let placed = crate::micros();
        let entry = (base + header.entry) as *const Entry;
        // SAFETY: the entry lies in the image just placed and relocated,
        // its address checked to be in the image's read-only data, aligned
        // to 8; its fingerprint first, as an entry of another interface
        // may be shorter than this one's.
        if unsafe { Entry::fingerprint_at(entry) } != FINGERPRINT.0 {
            return Err(PluginProblem::Damaged);
        }
        // SAFETY: as above, an entry of this interface.
        let entry = unsafe { &*entry };
        critical_section::with(|cs| {
            IMAGES.borrow_ref_mut(cs).push(Image {
                id: String::from(code.id.as_str()),
                code_base: base + CODE_OFFSET,
                len: header.image_len,
                hot,
                hot_at: header.hot_at,
                hot_len: header.hot_len,
                took: None,
            })
        });
        // SAFETY: a plugin of this firmware's plugin interface, chip and
        // layouts (its fingerprint) and of its compiler (its header),
        // placed as its file says.
        let plugin =
            unsafe { paperpaw_sdk::native::take((entry.make)()) }.ok_or(PluginProblem::Damaged)?;
        let took = Took {
            place_us: placed - started,
            make_us: crate::micros() - placed,
            heap: esp_alloc::HEAP.used() as isize - heap as isize,
            internal: internal as isize - internal_free() as isize,
            hot: if hot == 0 { 0 } else { header.hot_len },
        };
        info!(
            "native: {path}: {} B at {base:#x}, {} words moved, {} patched; placed in {} us, made in {} us; heap {:+} B, internal RAM {:+} B, hot code {} B at {hot:#x}",
            header.image_len,
            header.relocs,
            header.patches,
            took.place_us,
            took.make_us,
            took.heap,
            took.internal,
            took.hot
        );
        critical_section::with(|cs| {
            if let Some(image) = IMAGES.borrow_ref_mut(cs).last_mut() {
                image.took = Some(took);
            }
        });
        Ok(plugin)
    }
}

impl CodeLoader for Loader {
    fn load(
        &mut self,
        storage: &dyn Storage,
        code: &CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        // By its id, as safe mode switches it off.
        crate::safe_mode::loading(code.id.as_str());
        self.load_file(storage, code)
    }

    fn finish(&mut self, storage: &dyn Storage) {
        self.place.finish();
        // For the store and the plugin manager: how much room is left.
        let room = self.place.room();
        if !paperpaw_sdk::native::keep_code_room(storage, room.as_ref()) {
            warn!("native: the room for code plugins could not be kept");
        }
    }
}

/// The internal RAM free on the heap (the X4 Pro's PSRAM apart).
fn internal_free() -> usize {
    esp_alloc::HEAP.free_caps(esp_alloc::MemoryCapability::Internal.into())
}

/// What the plugin manager says for a file that is not right.
fn problem(problem: Problem) -> PluginProblem {
    problem_of(&[], problem)
}

/// Why a plugin file whose header, `head`, does not read does not run:
/// built for an older PaperPaw, needing a newer one, or damaged.
fn problem_of(head: &[u8], problem: Problem) -> PluginProblem {
    let why = paperpaw_sdk::native::Native::problem_of_file(head, problem);
    if why != PluginProblem::Damaged {
        warn!("native: {problem}");
    }
    why
}

/// The words after the image (those to relocate, then the patches),
/// read from the end of the file.
fn read_words(
    storage: &dyn Storage,
    path: &str,
    header: &Header,
) -> Result<Vec<u32>, PluginProblem> {
    let count = header.tail_words();
    let mut words = Vec::new();
    words
        .try_reserve_exact(count)
        .map_err(|_| PluginProblem::NoRoom)?;
    let mut buf = [0u8; 256];
    let mut at = header.relocs_at();
    while words.len() < count {
        let want = ((count - words.len()) * 4).min(buf.len());
        read_exact(storage, path, at, &mut buf[..want]).map_err(|_| PluginProblem::Damaged)?;
        words.extend(
            buf[..want]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|w| u32::from_le_bytes(*w)),
        );
        at += want as u64;
    }
    paperpaw_native::check_relocations(header, &words).map_err(problem)?;
    Ok(words)
}

/// Fills `buf` from `path` at `at`.
fn read_exact(
    storage: &dyn Storage,
    path: &str,
    mut at: u64,
    mut buf: &mut [u8],
) -> Result<(), ()> {
    while !buf.is_empty() {
        match storage.read_at(path, at, buf) {
            Ok(0) | Err(_) => return Err(()),
            Ok(n) => {
                at += n as u64;
                buf = &mut buf[n..];
            }
        }
    }
    Ok(())
}

/// The bytes after the header the digest covers once the image is read:
/// the words after the image as the file has them.
#[cfg(feature = "esp32c3")]
fn digest_words(sha: &mut paperpaw_digest::sha256::Sha256, words: &[u32]) {
    for word in words {
        sha.update(&word.to_le_bytes());
    }
}
