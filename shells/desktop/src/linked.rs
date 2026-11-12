//! The first-party interface-7 plugins that are linked into the emulator
//! (`paperpaw_app::NATIVE_PLUGINS`, ADR 0034 "One path"), registered through the
//! kernel (`Kernel::register_linked`) before the device starts.
//!
//! The emulator is the shell the application's flows and snapshots run on, so
//! the plugins that left their interface-6 form (`paperpaw_app::install_with`
//! drops it under the feature `native-linked`) must be here, or the app lacks
//! the screens they gave. What a plugin needs of the host is checked at its
//! admission: a plugin that asks for a service this shell does not wire is
//! refused there (the list of what is offered is `offered`) and the refusal
//! is said on stderr, so nothing is half there.
//!
//! One table of handles (with `core.ui` and `core.device` wired) and one
//! dispatch budget are shared by every linked plugin, as the loader's are for
//! the plugins of the card (`native::HostLoader`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_core::native_host::device_service::DEVICE_SERVICES;
use paperpaw_core::native_host::flat::FlatDepth;
use paperpaw_core::native_host::shell::DATA_SERVICES;
use paperpaw_core::native_host::shell_wiring::{
    CatalogEnv, NetInputs, NetLimits, NetReports, SecretsInputs, SockInputs, SockLimits,
    SockReports, WiredHosts, WorkDevice, WorkInputs,
};
use paperpaw_core::native_host::{HostResources, HostWiring, TeardownStep};
use paperpaw_native::metadata::{self, Feature, Service};
use paperpaw_platform::Storage;

use crate::device_host::DesktopDevice;
use crate::host_services::DesktopServices;
use crate::profiles::Profile;

/// The services this shell wires for linked plugins: the data services of the
/// shared host table, `core.device` over the profile's device, and the ones
/// the table serves from the kernel's own views (`core.ui`, `core.registry`).
/// A service not named here is not offered: a plugin that needs it is refused
/// at admission.
fn offered(wired: Wired) -> &'static [Service<'static>] {
    use std::sync::OnceLock;
    // One list per combination, made once: a restart registers again.
    static OFFERED: [OnceLock<&'static [Service<'static>]>; Wired::COMBINATIONS] =
        [const { OnceLock::new() }; Wired::COMBINATIONS];
    OFFERED[wired.index()].get_or_init(|| offered_list(wired))
}

/// The list for one combination of wired services (leaked once, by
/// [`offered`]: the kernel's admission holds it as long as the plugins run).
fn offered_list(wired: Wired) -> &'static [Service<'static>] {
    const OWN: &[&str] = &["core.ui", "core.registry"];
    let wired_keys = [
        (wired.secrets, "core.secrets"),
        (wired.work, "core.work"),
        (wired.net, "core.net"),
        (wired.sockets, "core.sockets"),
        (wired.documents, "core.document"),
        (wired.library, "core.library"),
        (wired.files, "core.files"),
        (wired.lookup, "core.lookup"),
    ]
    .into_iter()
    .filter_map(|(on, key)| on.then_some(key));
    let services: Vec<Service<'static>> = DATA_SERVICES
        .iter()
        .chain(DEVICE_SERVICES)
        .copied()
        .chain(OWN.iter().copied().chain(wired_keys).map(|key| Service {
            key,
            major: 1,
            semantic_revision: 0,
            min_table_bytes: 0,
        }))
        .collect();
    Box::leak(services.into_boxed_slice())
}

/// Which of the services that are wired only when the shell can are wired.
#[derive(Clone, Copy, Debug, Default)]
struct Wired {
    secrets: bool,
    work: bool,
    net: bool,
    sockets: bool,
    documents: bool,
    library: bool,
    files: bool,
    lookup: bool,
    /// The profile's device has the radio the pretend Wi-Fi answers for
    /// (`wifi.rs`): `uses.wifi` and the radio commands are backed.
    wifi: bool,
}

impl Wired {
    /// How many combinations there are (one bit a service).
    const COMBINATIONS: usize = 1 << 9;

    fn index(self) -> usize {
        [
            self.secrets,
            self.work,
            self.net,
            self.sockets,
            self.documents,
            self.library,
            self.files,
            self.lookup,
            self.wifi,
        ]
        .into_iter()
        .enumerate()
        .fold(0, |index, (bit, on)| index | usize::from(on) << bit)
    }
}

/// The consent features this shell backs: the registry (the kernel's own),
/// the device's power view (`core.device`) and, where they are wired,
/// `uses.secrets` (it has a sealing key), `uses.network` (HTTP requests,
/// `core.net`), `uses.storage` (the card's files, `core.files`) and
/// `uses.library` (`core.library`) and, on a profile whose device has Wi-Fi,
/// `uses.wifi` (the pretend radio answers the radio commands). Not offered,
/// because the desktop does not back them yet: `uses.bluetooth`,
/// `uses.install`, `uses.firmware`. A plugin that needs one is refused at admission.
fn features(wired: Wired) -> &'static [Feature<'static>] {
    use std::sync::OnceLock;
    static FEATURES: [OnceLock<&'static [Feature<'static>]>; Wired::COMBINATIONS] =
        [const { OnceLock::new() }; Wired::COMBINATIONS];
    FEATURES[wired.index()].get_or_init(|| features_list(wired))
}

/// The list for one combination of wired services (leaked once).
fn features_list(wired: Wired) -> &'static [Feature<'static>] {
    const fn feature(key: &'static str) -> Feature<'static> {
        Feature { key, revision: 1 }
    }
    let mut features = vec![feature("uses.registry"), feature("uses.power")];
    if wired.secrets {
        features.push(feature("uses.secrets"));
    }
    // `uses.network` is the one feature of HTTP and sockets (`network.md` 1).
    if wired.net || wired.sockets {
        features.push(feature("uses.network"));
    }
    // Root 1 (the card's files) and the library are signed features of their
    // own services (`files.md`, `library.md`).
    if wired.files {
        features.push(feature("uses.storage"));
    }
    if wired.library {
        features.push(feature("uses.library"));
    }
    // The radio commands (`device.md`) are answered by the pretend radio.
    if wired.wifi {
        features.push(feature("uses.wifi"));
    }
    Box::leak(features.into_boxed_slice())
}

/// A linked plugin's diagnostics from the kernel (a refused declaration, a
/// failed callback) go to stderr.
fn linked_report(_: &DesktopServices, plugin: &str, tag: u32, status: i32) {
    eprintln!("native plugin {plugin}: call {tag} ended with {status}");
}

/// What the device tells `core.work` (`WorkDevice`): the kernel's clock as the
/// device last read it and whether the network is up. The device writes both
/// where it reads its clock ([`DeviceView::set`]), so the scheduler and the
/// kernel's tick agree.
#[derive(Debug, Default)]
pub struct DeviceView {
    now_ms: Cell<u64>,
    online: Cell<bool>,
}

impl DeviceView {
    /// The clock and the network as they are now.
    pub fn set(&self, now_ms: u64, online: bool) {
        self.now_ms.set(now_ms);
        self.online.set(online);
    }
}

impl WorkDevice for DeviceView {
    fn now_ms(&self) -> u64 {
        self.now_ms.get()
    }
    fn network_up(&self) -> bool {
        self.online.get()
    }
    /// The pretend radio is on when the network is up.
    fn radio_on(&self) -> bool {
        self.online.get()
    }
}

/// What the device hands the registration besides its profile.
#[derive(Clone)]
pub struct Inputs {
    /// Where the secrets file is kept (sealed): the card of a real run, a
    /// store in memory for a run on simulated time. `None` where the card
    /// cannot be opened: `core.secrets` is then not wired, rather than a
    /// store that loses what is saved.
    pub storage: Option<Rc<dyn Storage>>,
    /// The clock and network `core.work` asks for.
    pub view: Rc<DeviceView>,
    /// A run on simulated time (tests, scripts): its secrets key and secrets
    /// are kept in memory. The card is still the run's card: a plugin's files,
    /// the library's details and the document cache are written to it, so a
    /// script should use a scratch card.
    pub simulated: bool,
    /// Where registration leaves the transport of `core.net` and core's
    /// reports to it, for the device's loop ([`Net`]); `None` inside where
    /// there is no card.
    pub net: Rc<RefCell<Option<Net>>>,
    /// The card itself (a second handle on what the kernel reads), which
    /// `core.document`, `core.library` and `core.files` serve the plugins
    /// from; `None` where it cannot be opened (none of them is wired then).
    /// Not `storage`: on simulated time that one is private to the run.
    pub card: Option<Rc<dyn Storage>>,
}

/// The transport of `core.net` on the device and the handle core gave for
/// its reports.
#[derive(Debug)]
pub struct Net {
    pub transport: Rc<crate::native_net::NativeNet>,
    pub reports: NetReports,
    /// TCP and UDP broadcast (`core.sockets`), where wired.
    pub sockets: Option<(Rc<crate::native_sock::NativeSock>, SockReports)>,
}

impl std::fmt::Debug for Inputs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inputs")
            .field("view", &self.view)
            .field("simulated", &self.simulated)
            .finish_non_exhaustive()
    }
}

/// What the card cannot tell the catalog reads, on the desktop: no walk
/// shown, today's date from the computer's clock (none on simulated time, so
/// no book is new in a run that must not depend on the day).
struct Catalog {
    simulated: bool,
}

impl CatalogEnv for Catalog {
    fn walking(&self) -> Option<u32> {
        None
    }
    fn today(&self) -> Option<u32> {
        if self.simulated {
            return None;
        }
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        u32::try_from(seconds / 86_400).ok()
    }
    /// The wall clock in milliseconds: the idle time of an annotation
    /// transaction and the life of its receipt. A simulated clock is none.
    fn now_ms(&self) -> u64 {
        if self.simulated {
            return 0;
        }
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(0))
    }
}

/// The sealer of the secrets, or why there is none (then `core.secrets` is
/// not wired and a plugin that needs it is refused: nothing is stored in the
/// clear).
fn sealer(simulated: bool) -> Result<Rc<crate::sealing::KeySealer>, std::io::Error> {
    let sealer = if simulated {
        crate::sealing::KeySealer::for_the_process()?
    } else {
        crate::sealing::KeySealer::for_the_user()?
    };
    Ok(Rc::new(sealer))
}

/// The step of a teardown that failed, once, without calling plugin code.
fn teardown_report(
    _: &DesktopServices,
    id: &str,
    step: TeardownStep,
    why: paperpaw_core::native_host::Error,
) {
    eprintln!("native plugin {id}: teardown step {step:?} failed: {why:?}");
}

/// Why the linked plugins could not be registered at all (a plugin the host
/// refuses is said on stderr and does not stop the others).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unavailable {
    /// No room for the shared table of handles or the dispatch budget.
    NoRoom,
    /// The wiring of `core.ui` or `core.device` was refused.
    Wiring(i32),
}

/// `core.document`, `core.library` and `core.files` over the card itself.
/// The files come with the library: a book its move job carries is in use
/// for the library's writes.
fn on_the_card(
    wiring: HostWiring,
    profile: &'static Profile,
    card: Rc<dyn Storage>,
    simulated: bool,
) -> HostWiring {
    let display = profile.device.display;
    wiring
        .documents(
            profile.device.memory,
            u32::from(display.width.max(display.height)),
        )
        .library(card.clone(), Rc::new(Catalog { simulated }))
        .files(card)
}

/// A value the linker puts in this program's read-only data (the same segment
/// as the plugins' declaration bytes), to find that segment by.
static RODATA_ANCHOR: [u8; 16] = *b"paperpaw-rodata\0";

/// The program's read-only data as a mapping (address and length) that stays
/// mapped for the whole run, or `(null, 0)` where it cannot be found: the
/// linked plugins keep their declarations as offsets into it, not as copies
/// (`Kernel::register_linked`).
///
/// Found as the mapping of this process that holds `RODATA_ANCHOR` and is
/// readable, not writable and not executable (`/proc/self/maps`, Linux).
pub fn rodata_image() -> (*const u8, usize) {
    #[cfg(target_os = "linux")]
    {
        let anchor = std::ptr::addr_of!(RODATA_ANCHOR) as usize;
        if let Some((start, len)) = std::fs::read_to_string("/proc/self/maps")
            .ok()
            .and_then(|maps| mapping_of(&maps, anchor))
        {
            return (start as *const u8, len);
        }
    }
    (std::ptr::null(), 0)
}

/// The `(start, length)` of the read-only, non-executable mapping in `maps`
/// (the text of `/proc/self/maps`) that holds `address`.
#[cfg(any(target_os = "linux", test))]
fn mapping_of(maps: &str, address: usize) -> Option<(usize, usize)> {
    for line in maps.lines() {
        let mut parts = line.split_ascii_whitespace();
        let (range, perms) = (parts.next()?, parts.next()?);
        let (start, end) = range.split_once('-')?;
        let start = usize::from_str_radix(start, 16).ok()?;
        let end = usize::from_str_radix(end, 16).ok()?;
        if address < start || address >= end {
            continue;
        }
        return (perms.starts_with("r--")).then_some((start, end - start));
    }
    None
}

/// What is wired: the wiring's own answer for the services that may be left
/// out, and the card's families (`on_card`).
fn wired_services(wired: &WiredHosts, on_card: Wired) -> Wired {
    Wired {
        secrets: wired.secrets().is_some(),
        work: true,
        net: wired.net().is_some(),
        sockets: wired.sockets().is_some(),
        lookup: true,
        ..on_card
    }
}

/// Registers every plugin of `paperpaw_app::NATIVE_PLUGINS` with `kernel` for
/// the device of `profile`; the number registered. Call before the kernel
/// starts, once.
///
/// # Errors
/// [`Unavailable`] where the shared host views cannot be made; then nothing is
/// registered.
pub fn register(
    kernel: &mut Kernel,
    profile: &'static Profile,
    inputs: Inputs,
) -> Result<usize, Unavailable> {
    let resources = HostResources::try_new().map_err(|_| Unavailable::NoRoom)?;
    let device: Rc<DesktopDevice> = Rc::new(DesktopDevice::new(profile));
    let mut wiring = HostWiring::new()
        .ui(profile.device.memory)
        .device(device)
        .lookup()
        .work(WorkInputs::new(inputs.view.clone(), profile.device.memory));
    // HTTP over the card (saved and sent files); no card, no `core.net`.
    let transport = inputs.storage.clone().map(|storage| {
        let transport = Rc::new(crate::native_net::NativeNet::new(
            storage.clone(),
            inputs.view.clone(),
        ));
        // TCP with TLS (`paperpaw_serve::secure`) and UDP broadcast. The
        // same transport tells core of the network for both services: core
        // combines their demand.
        let sockets = Rc::new(crate::native_sock::NativeSock::new());
        wiring = std::mem::take(&mut wiring)
            .net(NetInputs::new(
                storage.clone(),
                transport.clone(),
                transport.clone(),
                // The one pool and its TLS sessions are the same in both
                // wirings (`SockLimits::large`: 4).
                NetLimits::new(0, 8).tls_sessions(4),
            ))
            .sockets(
                SockInputs::new(
                    storage,
                    sockets.clone(),
                    transport.clone(),
                    SockLimits::large(0),
                )
                .tls(sockets.clone()),
            );
        (transport, sockets)
    });
    // The books, the catalog and the card's files, from the card itself.
    let mut on_card = Wired::default();
    if let Some(card) = inputs.card {
        wiring = on_the_card(wiring, profile, card, inputs.simulated);
        on_card.documents = true;
        on_card.library = true;
        on_card.files = true;
    }
    on_card.wifi = profile.wifi;
    match (inputs.storage, sealer(inputs.simulated)) {
        (None, _) => eprintln!("native plugins: no secrets (the card cannot be opened)"),
        (Some(storage), Ok(sealer)) => {
            wiring = wiring.secrets(
                SecretsInputs::new(storage, sealer)
                    .with_rng(Rc::new(crate::sealing::SystemRng))
                    .on_unreadable(Rc::new(|| {
                        eprintln!("the secrets file could not be read");
                    })),
            );
        }
        (Some(_), Err(why)) => eprintln!("native plugins: no secrets (no key: {why})"),
    }
    let wired = resources.wire(wiring).map_err(Unavailable::Wiring)?;
    // Values an older version kept veiled are sealed now (none to do on a
    // fresh card).
    if let Some(secrets) = wired.secrets()
        && let Err(status) = secrets.upgrade()
    {
        eprintln!("native plugins: the secrets file could not be upgraded ({status})");
    }
    // Interface 6's passwords are opened and written through the sealer from
    // here on, whether or not a native plugin is ever admitted (decision 140);
    // before `Kernel::start`, as `Kernel::wire_secrets` asks.
    if let Some(secrets) = wired.secrets() {
        kernel.wire_secrets(secrets);
    }
    let on = wired_services(&wired, on_card);
    *inputs.net.borrow_mut() =
        transport
            .zip(wired.net().cloned())
            .map(|((transport, sockets), reports)| Net {
                transport,
                reports,
                sockets: wired.sockets().cloned().map(|wired| (sockets, wired)),
            });
    let depth = FlatDepth::try_new().map_err(|_| Unavailable::NoRoom)?;
    let host = metadata::Host {
        abi_major: 7,
        minor: paperpaw_native::host_glue::MINOR,
        profile: crate::native_runtime::host_profile(),
        services: offered(on),
        features: features(on),
    };
    // SAFETY: the offered services are the ones wired above (`offered`), and
    // the plugins are the app's linked ones, as the helper's contract says.
    let outcome = unsafe {
        paperpaw_app::register_native_plugins(
            kernel,
            &host,
            &resources,
            &depth,
            DesktopServices::new,
            teardown_report,
            linked_report,
            |id| paperpaw_app::native_default_on(id, paperpaw_app::default_store()),
            rodata_image(),
        )
    };
    let mut registered = 0;
    for (id, status) in outcome {
        match status {
            Ok(()) => registered += 1,
            Err(why) => eprintln!("native plugin {id}: not registered: {why:?}"),
        }
    }
    Ok(registered)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A linked plugin's declaration bytes are read-only statics like its
    /// metadata; if the metadata lay outside the image found here the
    /// declarations would be copied again, unseen, so none may.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_linked_plugins_read_only_statics_lie_in_the_image() {
        let (start, len) = rodata_image();
        assert!(len > 0, "no image");
        let (start, end) = (start as usize, start as usize + len);
        for (id, linked) in paperpaw_app::NATIVE_PLUGINS.iter() {
            let at = linked.metadata().as_ptr() as usize;
            assert!(
                (start..end).contains(&at),
                "{id}: its metadata is outside the image"
            );
        }
    }

    #[test]
    fn the_read_only_mapping_is_the_one_that_holds_the_address() {
        let maps = "\
55d0c8a00000-55d0c8a50000 r--p 00000000 08:01 123 /bin/x
55d0c8a50000-55d0c8b50000 r-xp 00050000 08:01 123 /bin/x
55d0c8b50000-55d0c8b60000 rw-p 00150000 08:01 123 /bin/x
";
        assert_eq!(
            mapping_of(maps, 0x55d0_c8a0_1000),
            Some((0x55d0_c8a0_0000, 0x5_0000))
        );
        // Executable or writable: none; no mapping: none.
        assert_eq!(mapping_of(maps, 0x55d0_c8a6_0000), None);
        assert_eq!(mapping_of(maps, 0x55d0_c8b5_0000), None);
        assert_eq!(mapping_of(maps, 0x1000), None);
        assert_eq!(mapping_of("garbage", 0x1000), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn this_programs_read_only_data_is_found_and_holds_the_anchor() {
        let (start, len) = rodata_image();
        assert!(len > 0, "no read-only mapping found");
        let anchor = std::ptr::addr_of!(RODATA_ANCHOR) as usize;
        assert!(start as usize <= anchor && anchor < start as usize + len);
    }

    fn keys(wired: Wired) -> Vec<&'static str> {
        offered(wired).iter().map(|service| service.key).collect()
    }

    fn feature_keys(wired: Wired) -> Vec<&'static str> {
        features(wired).iter().map(|feature| feature.key).collect()
    }

    #[test]
    fn what_the_card_serves_is_offered_only_where_it_is_wired() {
        let bare = Wired::default();
        for key in ["core.document", "core.library", "core.files"] {
            assert!(!keys(bare).contains(&key), "{key}");
        }
        assert!(!feature_keys(bare).contains(&"uses.storage"));
        assert!(!feature_keys(bare).contains(&"uses.library"));
        let card = Wired {
            documents: true,
            library: true,
            files: true,
            ..Wired::default()
        };
        for key in ["core.document", "core.library", "core.files"] {
            assert!(keys(card).contains(&key), "{key}");
        }
        let wanted = feature_keys(card);
        assert!(wanted.contains(&"uses.storage") && wanted.contains(&"uses.library"));
        // The rest is offered as before.
        assert!(keys(card).contains(&"core.ui") && wanted.contains(&"uses.registry"));
        assert!(!wanted.contains(&"uses.secrets") && !wanted.contains(&"uses.network"));
    }

    #[test]
    fn the_lookup_is_offered_only_where_it_is_wired() {
        assert!(!keys(Wired::default()).contains(&"core.lookup"));
        let lookup = Wired {
            lookup: true,
            ..Wired::default()
        };
        assert!(keys(lookup).contains(&"core.lookup"));
        assert_ne!(lookup.index(), Wired::default().index());
    }

    #[test]
    fn wifi_is_offered_only_on_a_device_with_the_radio() {
        assert!(!feature_keys(Wired::default()).contains(&"uses.wifi"));
        let radio = Wired {
            wifi: true,
            ..Wired::default()
        };
        assert!(feature_keys(radio).contains(&"uses.wifi"));
        assert_ne!(radio.index(), Wired::default().index());
    }

    /// A plugin that asks for the radio (diagnostics: `uses.wifi`) is granted
    /// it where the radio is offered; where it is not, the plugin is still
    /// admitted (the feature is optional for it) but without the radio
    /// commands.
    #[test]
    fn a_plugin_asking_for_the_radio_is_granted_it_only_where_it_is_offered() {
        let bytes = include_bytes!("../../../plugins/diagnostics/native/metadata.host.bin");
        let granted = |wired: Wired| {
            let host = metadata::Host {
                abi_major: 7,
                minor: paperpaw_native::host_glue::MINOR,
                profile: crate::native_runtime::host_profile(),
                services: offered(wired),
                features: features(wired),
            };
            let checked = metadata::check(bytes, &host).expect("admitted");
            checked.available_features().any(|key| key == "uses.wifi")
        };
        let radio = Wired {
            wifi: true,
            ..Wired::default()
        };
        assert!(granted(radio));
        assert!(!granted(Wired::default()));
    }

    #[test]
    fn a_combination_is_made_once() {
        let card = Wired {
            files: true,
            ..Wired::default()
        };
        assert!(std::ptr::eq(offered(card), offered(card)));
        assert!(std::ptr::eq(features(card), features(card)));
        assert_ne!(card.index(), Wired::default().index());
    }

    #[test]
    fn the_catalog_has_no_walk_and_no_day_on_simulated_time() {
        let simulated = Catalog { simulated: true };
        assert_eq!((simulated.walking(), simulated.today()), (None, None));
        let real = Catalog { simulated: false };
        assert!(real.today().is_some_and(|day| day > 20_000));
    }
}
