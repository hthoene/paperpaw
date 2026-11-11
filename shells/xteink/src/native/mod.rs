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
pub mod partition;
#[cfg(feature = "ppn5")]
mod ppn5;
pub mod seed;
#[cfg(all(feature = "static-native", feature = "esp32c3"))]
pub(crate) use ppn5::register_linked_probe;
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

/// A format-5 plugin's image as the place put it.
#[cfg(all(feature = "ppn5", feature = "esp32s3"))]
pub type Placed5 = psram::Placed5;
#[cfg(all(feature = "ppn5", feature = "esp32c3"))]
pub type Placed5 = flash::Placed5;

/// Loads code plugins for the kernel (see the [module documentation](self)).
pub struct Loader {
    place: Place,
    /// The device-wide table of handles the plugins' owners share (made with
    /// the first plugin; each plugin holds a counted share of it).
    #[cfg(feature = "ppn5")]
    resources: DeviceTable,
    /// The network's side of `core.sockets`: this loader wires core with it,
    /// the network runs it.
    #[cfg(feature = "ppn5")]
    host_net: alloc::rc::Rc<crate::host_net::Bridge>,
    /// The device behind `core.device`, wired with the table of handles and
    /// told the code room when the loader is done.
    #[cfg(feature = "ppn5")]
    device: alloc::rc::Rc<paperpaw_x4::device_host::X4Device>,
    /// `core.secrets`: sealed with the device key (`crate::secrets`).
    #[cfg(feature = "ppn5")]
    secrets: crate::secrets::Wiring,
}

/// The device-wide table of handles, one for the whole firmware: the code
/// loader and the linked-plugin probe (`register_linked_probe`) both take their
/// share of this one, so what is wired into it (`core.ui`, `text.hyphenate`)
/// stays single. A clone is another view of the same cell (nothing is made
/// twice); the table itself is made at the first use, with `core.device`
/// wired to the device it holds.
#[cfg(feature = "ppn5")]
#[derive(Clone)]
pub struct DeviceTable(
    alloc::rc::Rc<RefCell<Option<paperpaw_core::native_host::HostResources>>>,
    alloc::rc::Rc<paperpaw_x4::device_host::X4Device>,
    alloc::rc::Rc<paperpaw_x4::device_depth::DeviceDepth>,
);

#[cfg(feature = "ppn5")]
impl DeviceTable {
    /// An empty table that wires `device` into `core.device` when it is made.
    fn new(device: alloc::rc::Rc<paperpaw_x4::device_host::X4Device>) -> Self {
        Self(alloc::rc::Rc::default(), device, alloc::rc::Rc::default())
    }

    /// A counted share of the device's one callback-depth counter, for the
    /// loader and the linked-plugin probe alike: the kernel refuses a plugin
    /// that brings another (`paperpaw_x4::device_depth`).
    pub(crate) fn depth(&self) -> Option<paperpaw_core::native_host::flat::FlatDepth> {
        self.2.share()
    }

    /// The device as the host services take it.
    fn device_host(
        &self,
    ) -> alloc::rc::Rc<dyn paperpaw_core::native_host::device_service::DeviceHost> {
        self.1.clone()
    }

    /// A counted share of the table, made (with `core.ui` wired) if it is not
    /// there yet. `None` where it has no room, or where the loader is busy
    /// with it (a plugin loading: no other caller exists on this one core).
    #[cfg(all(feature = "static-native", feature = "esp32c3"))]
    pub(crate) fn share(&self) -> Option<paperpaw_core::native_host::HostResources> {
        let mut table = self.0.try_borrow_mut().ok()?;
        if table.is_none() {
            *table = new_resources(Some(self.device_host()));
        }
        table.as_ref()?.try_clone()
    }
}

impl Loader {
    pub fn new(
        place: Place,
        #[cfg(feature = "ppn5")] host_net: alloc::rc::Rc<crate::host_net::Bridge>,
        #[cfg(feature = "ppn5")] device: alloc::rc::Rc<paperpaw_x4::device_host::X4Device>,
        #[cfg(feature = "ppn5")] secrets: crate::secrets::Wiring,
    ) -> Self {
        Self {
            place,
            #[cfg(feature = "ppn5")]
            resources: DeviceTable::new(device.clone()),
            #[cfg(feature = "ppn5")]
            host_net,
            #[cfg(feature = "ppn5")]
            device,
            #[cfg(feature = "ppn5")]
            secrets,
        }
    }

    /// The device's table of handles as the loader holds it, for the
    /// linked-plugin probe: one table, not a second one.
    #[cfg(all(feature = "static-native", feature = "esp32c3"))]
    pub fn device_table(&self) -> DeviceTable {
        self.resources.clone()
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
        // Interface 7's files (format 5) are read by their own loader.
        #[cfg(feature = "ppn5")]
        if ppn5::is_format5(&head) {
            // Without the net at the stack's end (`stack::arm_band`) a
            // frame that overflows is not caught: refused, for this start.
            if !crate::stack::band_armed() {
                warn!("native: {path} not run: the stack's band is not armed");
                return Err(PluginProblem::Unsupported);
            }
            // The nesting budget every interface-7 plugin of this device
            // shares: the device's one counter (the linked probe takes the same),
            // made at the first use (a fallible allocation: `NoRoom`).
            let Some(depth) = self.resources.depth() else {
                return Err(PluginProblem::NoRoom);
            };
            let Ok(mut resources) = self.resources.0.try_borrow_mut() else {
                return Err(PluginProblem::NoRoom);
            };
            let loaded = ppn5::load(
                &mut self.place,
                &depth,
                &mut resources,
                &self.device,
                storage,
                code,
            );
            drop(resources);
            if loaded.is_ok() {
                crate::stack::native_loaded();
            }
            return loaded;
        }
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
                // Built for this interface, against tables of a revision
                // since given up: `built_for` can only name the interface
                // (the detail is in the log line above), so the reason
                // reads "older" and the same number as this firmware's.
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

/// The device-wide table of handles the plugins' owners share, with `core.ui`
/// wired at once (before the first plugin loads, so that a host call made while
/// one loads or declares finds it). `None` (and a warning) where the table or
/// the store has no room: then no interface-7 plugin loads, or `core.ui` is
/// absent and the plugin is told so.
#[cfg(feature = "ppn5")]
fn new_resources(
    device: Option<alloc::rc::Rc<dyn paperpaw_core::native_host::device_service::DeviceHost>>,
) -> Option<paperpaw_core::native_host::HostResources> {
    use paperpaw_core::native_host::{HostResources, HostWiring};
    let resources = match HostResources::try_new() {
        Ok(resources) => resources,
        Err(_) => {
            warn!("native: no room for the host's table of handles");
            return None;
        }
    };
    let wiring = HostWiring::new().ui(crate::DEVICE.memory);
    let wiring = match device {
        Some(device) => wiring.device(device),
        None => wiring,
    };
    match resources.wire(wiring) {
        Ok(wired) if wired.ui_skipped() => {
            warn!("native: core.ui not wired (no memory for its store)");
        }
        Ok(_) => {}
        Err(status) => warn!("native: core.ui not wired ({status})"),
    }
    Some(resources)
}

#[cfg(feature = "ppn5")]
impl Loader {
    /// Wires `core.net` and `core.sockets` over the network's bridge, once
    /// the card is known. The TLS is the bridge's over the device's TLS
    /// session (`tls_slots`); a session that cannot be had at its handshake
    /// ends the socket or request `no_memory`.
    fn wire_network(&mut self, storage: &alloc::rc::Rc<dyn Storage>) {
        use alloc::rc::Rc;
        use paperpaw_core::native_host::{
            HostWiring,
            shell_wiring::{
                NetInputs, NetLimits, ShellHttp, ShellNetwork, SockInputs, SockLimits, SockTls,
                SockTransport,
            },
        };
        // The loader's table of handles (one for the device, shared with the
        // linked-plugin probe): not borrowed by anything else at this point.
        let Ok(table) = self.resources.0.try_borrow() else {
            return;
        };
        let Some(resources) = table.as_ref() else {
            return;
        };
        let bridge = &self.host_net;
        let mut limits = SockLimits::small(crate::host_net::POOL_BYTES);
        limits.tls_sessions = crate::tls_slots::SESSIONS;
        let transport: Rc<dyn SockTransport> = bridge.clone();
        let network: Rc<dyn ShellNetwork> = bridge.clone();
        let tls: Rc<dyn SockTls> = bridge.clone();
        let sockets = SockInputs::new(storage.clone(), transport, network.clone(), limits).tls(tls);
        // Two requests at a time, as in interface 6; bodies in memory are
        // small (the pool is 48 KiB, and the sockets' two buffers take 16).
        let http: Rc<dyn ShellHttp> = bridge.clone();
        let net_limits = NetLimits::new(crate::host_net::POOL_BYTES, 2)
            .tls_sessions(crate::tls_slots::SESSIONS)
            .max_body(16 * 1024);
        let net = NetInputs::new(storage.clone(), http, network, net_limits);
        match resources.wire(HostWiring::new().net(net).sockets(sockets)) {
            Ok(wired) => bridge.wired(storage.clone(), wired.net(), wired.sockets()),
            Err(status) => warn!("native: core.net and core.sockets not wired ({status})"),
        }
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

    /// What interface-7 plugins are served: `text.hyphenate` (the data
    /// plugins' tables are files on the card) and `core.ui` (the screen stack's
    /// store, sized for this device; nothing is allocated until a plugin uses it).
    #[cfg(feature = "ppn5")]
    fn hyphenation_sink(
        &mut self,
        storage: &alloc::rc::Rc<dyn Storage>,
    ) -> Option<paperpaw_core::native_host::HyphenTables> {
        use paperpaw_core::native_host::HostWiring;
        let mut resources = self.resources.0.try_borrow_mut().ok()?;
        if resources.is_none() {
            *resources = new_resources(Some(self.resources.device_host()));
        }
        // `text.hyphenate` is wired only here, because it needs the card's `Rc`.
        // A plugin's `make` that asks for it earlier gets a one-off
        // `ERR_UNSUPPORTED`; plugins use hyphenation only when laying out text,
        // and their allowed features come from the host's fixed list, not from
        // what is wired, so nothing is recorded wrongly.
        // `core.ui` was wired when the table of handles was made (before any
        // plugin loaded); `text.hyphenate` needs the card, which only comes here.
        // `core.secrets` with them: the card's file, sealed with the device key.
        let mut wired = match resources.as_ref()?.wire(
            HostWiring::new()
                .hyphenation(storage.clone())
                .secrets(self.secrets.inputs(storage)),
        ) {
            Ok(wired) => wired,
            Err(status) => {
                warn!("native: text.hyphenate and core.secrets not wired ({status})");
                return None;
            }
        };
        self.secrets.keep(&mut wired);
        // The table is borrowed no longer: `wire_sockets` takes it again.
        drop(resources);
        // This hook runs at the end of the kernel's build, before `Kernel::start`:
        // `core.net` and `core.sockets` are wired before the per-plugin network policy is
        // filled (a later wiring would show plugins a false `network_off`).
        // It needs the card too (files read to, the staged file).
        self.wire_network(storage);
        wired.hyphen().cloned()
    }

    fn finish(&mut self, storage: &dyn Storage) {
        self.place.finish();
        // For the store and the plugin manager: how much room is left.
        let room = self.place.room();
        #[cfg(feature = "ppn5")]
        if let Some(room) = room.as_ref() {
            let (free, largest) = paperpaw_x4::device_facts::code_room(room);
            self.device.set_code_room(free, largest);
        }
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
