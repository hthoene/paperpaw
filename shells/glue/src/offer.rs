//! What is offered to the linked plugins: the services and consent features of
//! the host table, for each combination of wired families.

use paperpaw_core::native_host::device_service::DEVICE_SERVICES;
use paperpaw_core::native_host::shell::DATA_SERVICES;
use paperpaw_core::native_host::shell_wiring::WiredHosts;
use paperpaw_native::metadata::{Feature, Service};

/// The services this shell wires for linked plugins: the data services of the
/// shared host table, `core.device` over the profile's device, and the ones
/// the table serves from the kernel's own views (`core.ui`, `core.registry`).
/// A service not named here is not offered: a plugin that needs it is refused
/// at admission.
pub fn offered(wired: Wired) -> &'static [Service<'static>] {
    let offer = offer(wired);
    &offer.services[..offer.services_len]
}

/// The most services a list holds: the data services, `core.device`, the
/// kernel's two own and every wired family.
pub const MAX_SERVICES: usize = 16;
// A list that outgrows its room fails the build, never the start: the data
// services, the device's, the kernel's two own and the eight wired families.
const _: () = assert!(
    DATA_SERVICES.len() + DEVICE_SERVICES.len() + 2 + 8 <= MAX_SERVICES,
    "MAX_SERVICES is too small for the all-wired offer"
);
/// The most features a list holds.
pub const MAX_FEATURES: usize = 8;

/// What is offered to the linked plugins for one combination of wired
/// services: the services and the consent features.
#[derive(Debug)]
pub struct Offer {
    services: [Service<'static>; MAX_SERVICES],
    services_len: usize,
    features: [Feature<'static>; MAX_FEATURES],
    features_len: usize,
}

impl Offer {
    /// The services offered.
    #[must_use]
    pub fn services(&self) -> &[Service<'static>] {
        &self.services[..self.services_len]
    }

    /// The consent features offered.
    #[must_use]
    pub fn features(&self) -> &[Feature<'static>] {
        &self.features[..self.features_len]
    }
}

/// The offer for `wired`, held as long as the plugins run (the kernel's
/// admission keeps the lists). A run meets one combination: its offer is in
/// static memory (no heap). Another combination in the same process (the
/// tests) is made once and left on the heap. A restart registers again.
pub fn offer(wired: Wired) -> &'static Offer {
    use std::sync::OnceLock;
    static FIRST: OnceLock<(usize, Offer)> = OnceLock::new();
    static OTHERS: [OnceLock<&'static Offer>; Wired::COMBINATIONS] =
        [const { OnceLock::new() }; Wired::COMBINATIONS];
    let first = FIRST.get_or_init(|| (wired.index(), make_offer(wired)));
    if first.0 == wired.index() {
        return &first.1;
    }
    OTHERS[wired.index()].get_or_init(|| Box::leak(Box::new(make_offer(wired))))
}

/// The offer for one combination of wired services.
pub fn make_offer(wired: Wired) -> Offer {
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
    let mut offer = Offer {
        services: [Service {
            key: "",
            major: 0,
            semantic_revision: 0,
            min_table_bytes: 0,
        }; MAX_SERVICES],
        services_len: 0,
        features: [Feature {
            key: "",
            revision: 0,
        }; MAX_FEATURES],
        features_len: 0,
    };
    let services = DATA_SERVICES.iter().chain(DEVICE_SERVICES).copied().chain(
        OWN.iter().copied().chain(wired_keys).map(|key| Service {
            key,
            major: 1,
            semantic_revision: 0,
            min_table_bytes: 0,
        }),
    );
    for service in services {
        offer.services[offer.services_len] = service;
        offer.services_len += 1;
    }
    let features = features_of(wired);
    for (feature, on) in features {
        if on {
            offer.features[offer.features_len] = Feature {
                key: feature,
                revision: 1,
            };
            offer.features_len += 1;
        }
    }
    offer
}

/// Which of the services that are wired only when the shell can are wired.
#[derive(Clone, Copy, Debug, Default)]
pub struct Wired {
    pub secrets: bool,
    pub work: bool,
    pub net: bool,
    pub sockets: bool,
    pub documents: bool,
    pub library: bool,
    pub files: bool,
    pub lookup: bool,
    /// The profile's device has the radio the pretend Wi-Fi answers for
    /// (`wifi.rs`): `uses.wifi` and the radio commands are backed.
    pub wifi: bool,
    /// Plugin jobs are taken (the emulated device installs plugins from the
    /// card: [`card_plugins`]): `uses.install` is backed.
    pub install: bool,
}

impl Wired {
    /// How many combinations there are (one bit a service).
    pub const COMBINATIONS: usize = 1 << 10;

    pub fn index(self) -> usize {
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
            self.install,
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
/// `uses.wifi` (the pretend radio answers the radio commands) and, where the
/// emulated device installs plugins ([`card_plugins`]), `uses.install`. Not
/// offered, because the desktop does not back them yet: `uses.bluetooth`,
/// `uses.firmware`. A plugin that needs one is refused at admission.
pub fn features(wired: Wired) -> &'static [Feature<'static>] {
    let offer = offer(wired);
    &offer.features[..offer.features_len]
}

/// Every feature a shell may offer, with whether `wired` backs it.
pub fn features_of(wired: Wired) -> [(&'static str, bool); MAX_FEATURES] {
    [
        ("uses.registry", true),
        ("uses.power", true),
        ("uses.secrets", wired.secrets),
        // `uses.network` is the one feature of HTTP and sockets
        // (`network.md` 1).
        ("uses.network", wired.net || wired.sockets),
        // Root 1 (the card's files) and the library are signed features of
        // their own services (`files.md`, `library.md`).
        ("uses.storage", wired.files),
        ("uses.library", wired.library),
        // The radio commands (`device.md`) are answered by the pretend radio.
        ("uses.wifi", wired.wifi),
        // Plugin installs, checks and removals (`device.md` 5.1, 5.2).
        ("uses.install", wired.install),
    ]
}

/// What is wired: the wiring's own answer for the services that may be left
/// out, and the card's families (`on_card`).
pub fn wired_services(wired: &WiredHosts, on_card: Wired) -> Wired {
    Wired {
        secrets: wired.secrets().is_some(),
        work: true,
        net: wired.net().is_some(),
        sockets: wired.sockets().is_some(),
        lookup: true,
        ..on_card
    }
}
