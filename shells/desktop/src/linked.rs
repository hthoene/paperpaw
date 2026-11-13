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

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_core::native_host::HostWiring;
use paperpaw_core::native_host::shell_wiring::{
    NetInputs, NetLimits, NetReports, Sealer, SecretsInputs,
};
#[cfg(not(target_arch = "wasm32"))]
use paperpaw_core::native_host::shell_wiring::{SockInputs, SockLimits, SockReports};
use paperpaw_native::metadata;
use paperpaw_platform::Storage;
use paperpaw_shell_glue::{PluginJobs, Shell};

pub use paperpaw_shell_glue::Unavailable;
pub use paperpaw_shell_glue::image::rodata_image;
pub use paperpaw_shell_glue::view::DeviceView;

use crate::device_host::DesktopDevice;
use crate::profiles::Profile;

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
    /// TCP and UDP broadcast (`core.sockets`), where wired: not in a browser,
    /// which has no sockets of its own to offer.
    #[cfg(not(target_arch = "wasm32"))]
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

/// The sealer of the secrets, or why there is none (then `core.secrets` is
/// not wired and a plugin that needs it is refused: nothing is stored in the
/// clear). Made once for the process and kept in static memory (the cipher's
/// tables take 1 KiB); the key is the process's or the user's, which never
/// change in a run (`sealing`). It is read once per process: `HOME` and
/// `XDG_*` are not read again, so a test that changes them after the first
/// call still gets the first key.
fn sealer(simulated: bool) -> Result<&'static crate::sealing::KeySealer, std::io::Error> {
    use crate::sealing::KeySealer;
    use std::sync::OnceLock;
    static SIMULATED: OnceLock<KeySealer> = OnceLock::new();
    static USER: OnceLock<KeySealer> = OnceLock::new();
    // A browser has no folder for a user's key: the page's own, as a simulated
    // run's (secrets last as long as the page).
    let (cell, make): (_, fn() -> std::io::Result<KeySealer>) =
        if simulated || cfg!(target_arch = "wasm32") {
            (&SIMULATED, KeySealer::for_the_process)
        } else {
            (&USER, KeySealer::for_the_user)
        };
    if let Some(sealer) = cell.get() {
        return Ok(sealer);
    }
    let made = make()?;
    Ok(cell.get_or_init(|| made))
}

/// The sealer in static memory, as the host takes it (`Rc<dyn Sealer>`): the
/// handle is a pointer, the cipher stays where it is.
struct StaticSealer(&'static crate::sealing::KeySealer);

impl Sealer for StaticSealer {
    fn seals(&self) -> bool {
        self.0.seals()
    }
    fn seal(&self, aad: &[u8], plain: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        self.0.seal(aad, plain, out)
    }
    fn open(&self, aad: &[u8], sealed: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        self.0.open(aad, sealed, out)
    }
}
/// What a plugin file for the emulated device is checked against: the loader
/// of its chip and the metadata profile of its firmware (`device.md` 5.1), as
/// the device's firmware has them. `None` for a device PaperPaw installs no
/// plugin files on. The emulator runs no such file itself (an installed
/// plugin is the device's from its next start), but its store installs,
/// checks and removes them as the device's does.
pub fn card_plugins(
    profile: &Profile,
) -> Option<(
    paperpaw_native::header5::Host,
    metadata::Profile<'static>,
    u16,
)> {
    use crate::no_firmware::{CHIP_ESP32_C3, CHIP_ESP32_S3};
    use paperpaw_native::header5::{Host, S3_PROFILE, X4_PROFILE};
    let minor = paperpaw_native::host_glue::MINOR;
    match profile.catalog_device? {
        "x4" => Some((Host::x4(minor), X4_PROFILE, CHIP_ESP32_C3)),
        "x4-pro" | "x4-classic" => Some((Host::x4_pro(minor), S3_PROFILE, CHIP_ESP32_S3)),
        _ => None,
    }
}

/// Whether `profile`'s device installs data plugins from the card and loads
/// no plugin code (Boox, Android: it has a catalogue name, but no loader of
/// the kinds of [`card_plugins`]): plugin jobs install, check and remove data
/// plugins there, and a plugin with a code file ends with `ERR_UNSUPPORTED`.
pub fn data_plugins(profile: &Profile) -> bool {
    profile.catalog_device.is_some() && card_plugins(profile).is_none()
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
    paperpaw_shell_glue::clock::set_micros(crate::host::micros);
    let device: Rc<DesktopDevice> = Rc::new(DesktopDevice::new(profile));
    let mut wiring = HostWiring::new();
    // HTTP over the card (saved and sent files); no card, no `core.net`. In a
    // browser the requests go through the page's own route (`http::set_route`).
    let transport = inputs.storage.clone().map(|storage| {
        let transport = Rc::new(crate::native_net::NativeNet::new(
            storage.clone(),
            inputs.view.clone(),
        ));
        wiring = std::mem::take(&mut wiring).net(NetInputs::new(
            storage.clone(),
            transport.clone(),
            transport.clone(),
            // The one pool and its TLS sessions are the same in both
            // wirings (`SockLimits::large`: 4).
            NetLimits::new(0, 8).tls_sessions(4),
        ));
        // TCP with TLS (`paperpaw_serve::secure`) and UDP broadcast. The
        // same transport tells core of the network for both services: core
        // combines their demand.
        #[cfg(not(target_arch = "wasm32"))]
        let sockets = {
            let sockets = Rc::new(crate::native_sock::NativeSock::new());
            wiring = std::mem::take(&mut wiring).sockets(
                SockInputs::new(
                    storage,
                    sockets.clone(),
                    transport.clone(),
                    SockLimits::large(0),
                )
                .tls(sockets.clone()),
            );
            sockets
        };
        #[cfg(target_arch = "wasm32")]
        let sockets = ();
        (transport, sockets)
    });
    match (inputs.storage, sealer(inputs.simulated)) {
        (None, _) => eprintln!("native plugins: no secrets (the card cannot be opened)"),
        (Some(storage), Ok(sealer)) => {
            wiring = wiring.secrets(
                SecretsInputs::new(storage, Rc::new(StaticSealer(sealer)))
                    .with_rng(Rc::new(crate::sealing::SystemRng))
                    .on_unreadable(Rc::new(|| {
                        eprintln!("the secrets file could not be read");
                    })),
            );
        }
        (Some(_), Err(why)) => eprintln!("native plugins: no secrets (no key: {why})"),
    }
    let jobs = match card_plugins(profile) {
        Some((loader, card, _)) => PluginJobs::Code { loader, card },
        None if data_plugins(profile) => PluginJobs::Data,
        None => PluginJobs::None,
    };
    let display = profile.device.display;
    let registered = paperpaw_shell_glue::register(
        kernel,
        Shell {
            memory: profile.device.memory,
            widest: u32::from(display.width.max(display.height)),
            device,
            view: inputs.view,
            card: inputs.card,
            simulated: inputs.simulated,
            wifi: profile.wifi,
            jobs,
            wiring,
        },
    )?;
    let wired = &registered.wired;
    *inputs.net.borrow_mut() =
        transport
            .zip(wired.net().cloned())
            .map(|((transport, sockets), reports)| {
                #[cfg(target_arch = "wasm32")]
                let () = sockets;
                Net {
                    transport,
                    reports,
                    #[cfg(not(target_arch = "wasm32"))]
                    sockets: wired.sockets().cloned().map(|wired| (sockets, wired)),
                }
            });
    Ok(registered.count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_core::native_host::shell_wiring::CatalogEnv;
    use paperpaw_shell_glue::image::{host_profile, mapping_of};
    use paperpaw_shell_glue::offer::{Wired, features, make_offer, offered};
    use paperpaw_shell_glue::view::Catalog;

    /// A linked plugin's declaration bytes are read-only statics like its
    /// metadata; if the metadata lay outside the image found here the
    /// declarations would be copied again, unseen, so none may.
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
                profile: host_profile(),
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

    /// The offer of a run is in static memory and the sealer too (#248, P3):
    /// making an offer allocates nothing, and the handle the host takes a
    /// static sealer by is a pointer, not the cipher's 1 KiB.
    #[test]
    fn an_offer_and_a_sealer_take_no_heap() {
        let _one = crate::one_device_at_a_time();
        let wired = Wired {
            secrets: true,
            net: true,
            ..Wired::default()
        };
        let before = crate::budget::stats().live;
        let offer = crate::budget::track(|| make_offer(wired));
        assert_eq!(crate::budget::stats().live, before, "an offer allocated");
        assert!(offer.services().len() > 8 && offer.features().len() >= 3);
        let first = sealer(true).expect("a key for the process");
        let second = sealer(true).expect("a key for the process");
        assert!(std::ptr::eq(first, second), "a second sealer was made");
        let handle = crate::budget::track(|| Rc::new(StaticSealer(first)));
        let held = crate::budget::stats().live - before;
        drop(handle);
        assert!(held < 128, "the sealer's handle holds {held} B");
        assert_eq!(crate::budget::stats().live, before);
    }

    /// What is offered follows what is wired, for any combination met in a
    /// process (the first is in static memory, the others made once).
    #[test]
    fn the_offer_follows_what_is_wired() {
        let keys = |wired: Wired| -> (Vec<&str>, Vec<&str>) {
            (
                offered(wired).iter().map(|service| service.key).collect(),
                features(wired).iter().map(|feature| feature.key).collect(),
            )
        };
        let (bare_services, bare_features) = keys(Wired::default());
        assert!(bare_services.contains(&"core.ui"));
        assert!(bare_services.contains(&"core.registry"));
        assert!(!bare_services.contains(&"core.secrets"));
        assert_eq!(bare_features, ["uses.registry", "uses.power"]);
        let all = Wired {
            secrets: true,
            work: true,
            net: true,
            sockets: true,
            documents: true,
            library: true,
            files: true,
            lookup: true,
            wifi: true,
            install: true,
        };
        let (services, features) = keys(all);
        assert_eq!(services.len(), bare_services.len() + 8);
        for key in [
            "core.secrets",
            "core.work",
            "core.net",
            "core.sockets",
            "core.lookup",
        ] {
            assert!(services.contains(&key), "{key}");
        }
        assert_eq!(
            features,
            [
                "uses.registry",
                "uses.power",
                "uses.secrets",
                "uses.network",
                "uses.storage",
                "uses.library",
                "uses.wifi",
                "uses.install",
            ]
        );
        // The same combination again is the same list.
        assert!(std::ptr::eq(offered(all), offered(all)));
    }

    #[test]
    fn the_catalog_has_no_walk_and_no_day_on_simulated_time() {
        let simulated = Catalog { simulated: true };
        assert_eq!((simulated.walking(), simulated.today()), (None, None));
        let real = Catalog { simulated: false };
        assert!(real.today().is_some_and(|day| day > 20_000));
    }
}
