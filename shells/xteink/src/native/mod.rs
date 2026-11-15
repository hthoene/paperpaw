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
#[cfg(feature = "native-on")]
pub(crate) use ppn5::register_linked;
#[cfg(feature = "esp32s3")]
mod psram;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use critical_section::Mutex;
use log::warn;
use paperpaw_core::{CodeFile, CodeLoader};
use paperpaw_native::Problem;
use paperpaw_platform::Storage;
use paperpaw_sdk::PluginProblem;
use paperpaw_sdk::native::FINGERPRINT;
#[cfg(feature = "remote")]
use paperpaw_sdk::native::TOOLCHAIN;

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

/// The chip family of this device, for what the kernel's plugin and firmware
/// jobs check a file against (`paperpaw_x4::host_jobs`).
#[cfg(feature = "esp32c3")]
pub const FAMILY: paperpaw_x4::host_jobs::Family = paperpaw_x4::host_jobs::Family::Riscv;
#[cfg(feature = "esp32s3")]
pub const FAMILY: paperpaw_x4::host_jobs::Family = paperpaw_x4::host_jobs::Family::Xtensa;

/// What this firmware's loader reads of interface-7 files (its chip,
/// interface and minor; `None` in a build without the loader). The one
/// source of it: [`code_to_load`] asks with it, and the kernel installs the
/// code fetched for an update by it (`CodeLoader::reads`).
pub fn reads() -> Option<paperpaw_native::header5::Host> {
    #[cfg(feature = "ppn5")]
    let reads = Some(ppn5::HOST);
    #[cfg(not(feature = "ppn5"))]
    let reads = None;
    reads
}

/// Whether a start with code plugins would load any from `storage`
/// (`paperpaw_core::code_to_load`), with what this firmware's loader reads
/// of interface-7 files: asked once an update on trial is kept.
pub fn code_to_load(storage: &dyn Storage) -> bool {
    paperpaw_core::code_to_load(storage, &this_firmware(), reads().as_ref())
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
        api::SHARED_REVISIONS,
        api::SHARED_OLDEST,
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

/// The length of the header before the image in a plugin's file: an
/// interface-7 file's (96 bytes), the only files placed.
#[cfg(feature = "ppn5")]
const FILE_HEADER: u32 = paperpaw_native::header5::HEADER_LEN as u32;
/// Without the loader no image is placed.
#[cfg(not(feature = "ppn5"))]
const FILE_HEADER: u32 = 0;

/// The plugin whose code the instruction address `pc` lies in, and the
/// offset into its file (the image begins after the header): with the
/// plugin's ELF, its function is at `0x3C000000 + 0x06000000 + offset -
/// FILE_HEADER`. Code in the arena of hot code is its plugin's, at its
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
                    found(&image.id, offset + FILE_HEADER);
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
/// loader and the linked plugins (`register_linked`) both take their
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
    /// loader and the linked plugins alike: the kernel refuses a plugin
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
    #[cfg(feature = "native-on")]
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
    /// linked plugins: one table, not a second one.
    #[cfg(feature = "native-on")]
    pub fn device_table(&self) -> DeviceTable {
        self.resources.clone()
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

/// What the card cannot tell the catalog reads, on the X4: the walk is the
/// kernel's own (nothing to show here) and a book is new by the date the clock
/// has, when it has one.
#[cfg(feature = "ppn5")]
struct CardCatalog;

#[cfg(feature = "ppn5")]
impl paperpaw_core::native_host::shell_wiring::CatalogEnv for CardCatalog {
    fn walking(&self) -> Option<u32> {
        None
    }

    fn today(&self) -> Option<u32> {
        let millis = crate::clock::unix_ms(crate::now())?;
        u32::try_from(millis / 86_400_000).ok()
    }
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
                SockTransport, WorkInputs,
            },
        };
        // The loader's table of handles (one for the device, shared with the
        // linked plugins): not borrowed by anything else at this point.
        let Ok(table) = self.resources.0.try_borrow() else {
            warn!("native: core.net, core.sockets and core.work not wired (table busy)");
            return;
        };
        let Some(resources) = table.as_ref() else {
            warn!("native: core.net, core.sockets and core.work not wired (no table)");
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
            // The computer's lines over the cable (`PPT/1`, `usb_lines`) reach the
            // plugin that serves `net_serve` port 0 (the transfer, #900).
            .cable(true)
            .max_body(16 * 1024);
        let net = NetInputs::new(storage.clone(), http, network, net_limits);
        // `core.work` with them: its scheduler reads the same clock and the
        // same network the bridge does, and is wired before the per-plugin
        // network policy is filled.
        let work = WorkInputs::new(bridge.clone(), crate::DEVICE.memory);
        match resources.wire(HostWiring::new().net(net).sockets(sockets).work(work)) {
            Ok(wired) => {
                bridge.wired(storage.clone(), wired.net(), wired.sockets());
                crate::device_host::wired_work();
            }
            Err(status) => {
                warn!("native: core.net, core.sockets and core.work not wired ({status})")
            }
        }
    }

    /// Wires `core.document`, `core.library` and `core.files` over the card:
    /// the books a plugin reads, the catalog, and the card's files. One wiring,
    /// because `core.files` needs the sources of what the host holds in use
    /// (the library's moves, the reader's book). Offered to plugins only when
    /// it succeeded (`device_host::wired_card`); a table that is
    /// borrowed or missing counts as not wired. Not reached by a plugin until
    /// the activation (the stack gate).
    fn wire_card(&self, storage: &alloc::rc::Rc<dyn Storage>) {
        use alloc::rc::Rc;
        use paperpaw_core::native_host::HostWiring;
        let Ok(table) = self.resources.0.try_borrow() else {
            warn!("native: core.document, core.library and core.files not wired (table busy)");
            return;
        };
        let Some(resources) = table.as_ref() else {
            warn!("native: core.document, core.library and core.files not wired (no table)");
            return;
        };
        let display = crate::DEVICE.display;
        let wiring = HostWiring::new()
            .documents(
                crate::DEVICE.memory,
                u32::from(display.width.max(display.height)),
            )
            .library(storage.clone(), Rc::new(CardCatalog))
            .files(storage.clone());
        match resources.wire(wiring) {
            Ok(_) => crate::device_host::wired_card(),
            Err(status) => {
                warn!("native: core.document, core.library and core.files not wired ({status})")
            }
        }
    }

    /// Wires `core.lookup`: the dictionaries' query service. It reads what the
    /// plugins declared and keeps nothing of its own; offered to the linked
    /// plugins only where it succeeded (`device_host::wired_lookup`), and
    /// never to a card plugin (`native_offer::card_services`).
    #[cfg(feature = "native-on")]
    fn wire_lookup(&self) {
        use paperpaw_core::native_host::HostWiring;
        let Ok(table) = self.resources.0.try_borrow() else {
            warn!("native: core.lookup not wired (table busy)");
            return;
        };
        let Some(resources) = table.as_ref() else {
            warn!("native: core.lookup not wired (no table)");
            return;
        };
        match resources.wire(HostWiring::new().lookup()) {
            Ok(_) => crate::device_host::wired_lookup(),
            Err(status) => warn!("native: core.lookup not wired ({status})"),
        }
    }

    /// Wires `core.secrets`: the card's file, sealed with the device key.
    /// Before the first plugin is admitted, as the families above, so a card
    /// plugin that declares it is admitted at this start; offered to plugins
    /// only where it succeeded (`device_host::wired_secrets`). A plugin reaches
    /// its own secrets only: the file is keyed by the plugin's id, which the host
    /// gives (the card folder's name, which `load_native` refuses where it is
    /// the id of a linked plugin), never the plugin.
    #[cfg(feature = "ppn5")]
    fn wire_secrets(&self, storage: &alloc::rc::Rc<dyn Storage>) {
        use paperpaw_core::native_host::HostWiring;
        let Ok(table) = self.resources.0.try_borrow() else {
            warn!("native: core.secrets not wired (table busy)");
            return;
        };
        let Some(resources) = table.as_ref() else {
            warn!("native: core.secrets not wired (no table)");
            return;
        };
        match resources.wire(HostWiring::new().secrets(self.secrets.inputs(storage))) {
            Ok(mut wired) => {
                if wired.secrets().is_some() {
                    crate::device_host::wired_secrets();
                }
                self.secrets.keep(&mut wired);
            }
            Err(status) => warn!("native: core.secrets not wired ({status})"),
        }
    }
}

impl CodeLoader for Loader {
    fn reads(&self) -> Option<paperpaw_native::header5::Host> {
        reads()
    }

    /// Wires what the shell offers (`core.net`, `core.sockets`, `core.work`
    /// with the network's bridge, and `core.document`, `core.library`,
    /// `core.files` over the card) before the first plugin is admitted, so the
    /// lists (`paperpaw_x4::native_offer`) are what is wired at this start. They are
    /// wired before the per-plugin network policy is filled (a later wiring
    /// would show plugins a false `network_off`); they need the card too
    /// (files read to, the staged file).
    #[cfg(feature = "ppn5")]
    fn prepare(&mut self, storage: &alloc::rc::Rc<dyn Storage>) {
        crate::device_host::reset_wired();
        {
            let Ok(mut table) = self.resources.0.try_borrow_mut() else {
                warn!("native: no host services wired (table busy)");
                return;
            };
            if table.is_none() {
                *table = new_resources(Some(self.resources.device_host()));
            }
        }
        self.wire_network(storage);
        self.wire_card(storage);
        #[cfg(feature = "native-on")]
        self.wire_lookup();
        self.wire_secrets(storage);
    }

    /// A format-5 file (interface 7) is hosted for the kernel to admit with
    /// everything it declares: `make` runs in the kernel's registration
    /// transaction, and its screens, settings and callbacks are registered
    /// all together or the plugin is refused (`Unsupported` where the stack's
    /// band is not armed, `NoRoom`, `Damaged`, ...; never loaded without what
    /// it declared). Any other file is none of this loader's: the kernel says
    /// why it does not run (built for an older PaperPaw, as Beta 3's plugins
    /// are, or damaged).
    #[cfg(feature = "ppn5")]
    fn load_native(
        &mut self,
        storage: &dyn Storage,
        code: &CodeFile<'_>,
    ) -> Result<Option<Box<dyn paperpaw_core::NativeAdmission>>, PluginProblem> {
        let path = code.path;
        // The id of a linked plugin is that plugin's: a card folder is named by
        // whoever made it, and the id keys secrets and files. The kernel refuses
        // such a folder before it asks here (`built_in_ids`, `main.rs`); whatever
        // the file is, this loader does not look at it either.
        if paperpaw_app::is_linked_id(code.id.as_str()) {
            warn!(
                "native: {path}: `{}` is a plugin built into this firmware; a card plugin cannot take its id",
                code.id.as_str()
            );
            return Err(PluginProblem::Mismatch);
        }
        let mut head = [0; paperpaw_native::header5::HEADER_LEN];
        read_exact(storage, path, 0, &mut head).map_err(|_| PluginProblem::Damaged)?;
        if !ppn5::is_format5(&head) {
            return Ok(None);
        }
        // By its id, as safe mode switches it off.
        crate::safe_mode::loading(code.id.as_str());
        // Without the net at the stack's end a frame that overflows is not
        // caught.
        if !crate::stack::band_armed() {
            warn!("native: {path} not run: the stack's band is not armed");
            return Err(PluginProblem::Unsupported);
        }
        let Some(depth) = self.resources.depth() else {
            return Err(PluginProblem::NoRoom);
        };
        let Ok(mut resources) = self.resources.0.try_borrow_mut() else {
            return Err(PluginProblem::NoRoom);
        };
        let hosted = ppn5::host(
            &mut self.place,
            &depth,
            &mut resources,
            &self.device,
            storage,
            code,
        );
        drop(resources);
        if hosted.is_ok() {
            crate::stack::native_loaded();
        }
        hosted.map(Some)
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
        let wired = match resources
            .as_ref()?
            .wire(HostWiring::new().hyphenation(storage.clone()))
        {
            Ok(wired) => wired,
            Err(status) => {
                warn!("native: text.hyphenate not wired ({status})");
                return None;
            }
        };
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
