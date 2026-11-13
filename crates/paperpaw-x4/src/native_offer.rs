//! What the firmware offers the plugins of interface 7 that are linked into
//! it (`paperpaw_app::NATIVE_PLUGINS`): the services and the consent
//! features, as the families the start wired (the kernel's admission refuses
//! a plugin that needs one that is not in the list), and nothing else.
//!
//! The same lists are the ones the firmware gives the kernel
//! (`shells/xteink/src/native/ppn5.rs`) and the ones the host tests admit
//! every linked plugin against (`shells/desktop/tests/x4_offer.rs`), so a
//! plugin that would be refused on the device is found on a computer.
use paperpaw_core::native_host::{device_service::DEVICE_SERVICES, shell::DATA_SERVICES};
use paperpaw_native::metadata::{Feature, Service};

/// Which families a start wired. A family is offered only where it is wired:
/// a service offered and not backed is a plugin that fails later. The
/// families come in the groups the device wires them in (`wired_families`),
/// so every combination has its lists in the program's read-only data, not
/// made on the heap at the start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wired {
    /// `core.work`, `core.net` and `core.sockets` (the network and its
    /// bridge).
    pub work: bool,
    /// `core.document`, `core.library` and `core.files` (the card's three).
    pub card: bool,
    /// `core.secrets` (the device's, sealed with its key).
    pub secrets: bool,
    /// The radio's commands (`uses.wifi`): the kernel answers them from its
    /// Wi-Fi model, which a device with a radio has.
    pub wifi: bool,
}

impl Wired {
    /// Every family the firmware wires for linked plugins.
    pub const ALL: Self = Self {
        work: true,
        card: true,
        secrets: true,
        wifi: true,
    };
}

const fn service(key: &'static str) -> Service<'static> {
    Service {
        key,
        major: 1,
        semantic_revision: 0,
        min_table_bytes: 0,
    }
}

const fn feature(key: &'static str) -> Feature<'static> {
    Feature { key, revision: 1 }
}

/// How many services a list holds: the data services, `core.device`, the
/// kernel's own `core.ui` and `core.registry`, and the wired families.
const fn services_len(secrets: bool, work: bool, card: bool) -> usize {
    DATA_SERVICES.len()
        + DEVICE_SERVICES.len()
        + 2
        + secrets as usize
        + 3 * work as usize
        + 3 * card as usize
}

/// The services a linked plugin is offered, as one list of `N`.
const fn build_services<const N: usize>(
    secrets: bool,
    work: bool,
    card: bool,
) -> [Service<'static>; N] {
    let mut list = [service("core.ui"); N];
    let mut at = 0;
    let mut i = 0;
    while i < DATA_SERVICES.len() {
        list[at] = DATA_SERVICES[i];
        at += 1;
        i += 1;
    }
    let mut i = 0;
    while i < DEVICE_SERVICES.len() {
        list[at] = DEVICE_SERVICES[i];
        at += 1;
        i += 1;
    }
    let keys: [(bool, &str); 9] = [
        (true, "core.ui"),
        (true, "core.registry"),
        (secrets, "core.secrets"),
        (work, "core.work"),
        (work, "core.net"),
        (work, "core.sockets"),
        (card, "core.document"),
        (card, "core.library"),
        (card, "core.files"),
    ];
    let mut i = 0;
    while i < keys.len() {
        if keys[i].0 {
            list[at] = service(keys[i].1);
            at += 1;
        }
        i += 1;
    }
    assert!(at == N, "the length of a list");
    list
}

/// How many features a list holds.
const fn features_len(secrets: bool, work: bool, card: bool, wifi: bool) -> usize {
    2 + secrets as usize + work as usize + 2 * card as usize + wifi as usize
}

/// The consent features a linked plugin may ask for, as one list of `N`.
const fn build_features<const N: usize>(
    secrets: bool,
    work: bool,
    card: bool,
    wifi: bool,
) -> [Feature<'static>; N] {
    let mut list = [feature("uses.registry"); N];
    let keys: [(bool, &str); 7] = [
        (true, "uses.registry"),
        (true, "uses.power"),
        (secrets, "uses.secrets"),
        // One feature for HTTP and sockets (`network.md` 1).
        (work, "uses.network"),
        (card, "uses.storage"),
        (card, "uses.library"),
        // The radio commands of `core.device` (ops 8, 10, 11, 12), answered
        // by the kernel's Wi-Fi model.
        (wifi, "uses.wifi"),
    ];
    let mut at = 0;
    let mut i = 0;
    while i < keys.len() {
        if keys[i].0 {
            list[at] = feature(keys[i].1);
            at += 1;
        }
        i += 1;
    }
    assert!(at == N, "the length of a list");
    list
}

/// One static list for each combination of the three service groups.
macro_rules! service_lists {
    ($(($name:ident, $secrets:expr, $work:expr, $card:expr)),* $(,)?) => {
        $(static $name: [Service<'static>; services_len($secrets, $work, $card)] =
            build_services::<{ services_len($secrets, $work, $card) }>($secrets, $work, $card);)*
    };
}

service_lists!(
    (S000, false, false, false),
    (S001, false, false, true),
    (S010, false, true, false),
    (S011, false, true, true),
    (S100, true, false, false),
    (S101, true, false, true),
    (S110, true, true, false),
    (S111, true, true, true),
);

/// One static list for each combination of the four feature groups.
macro_rules! feature_lists {
    ($(($name:ident, $secrets:expr, $work:expr, $card:expr, $wifi:expr)),* $(,)?) => {
        $(static $name: [Feature<'static>; features_len($secrets, $work, $card, $wifi)] =
            build_features::<{ features_len($secrets, $work, $card, $wifi) }>(
                $secrets, $work, $card, $wifi,
            );)*
    };
}

feature_lists!(
    (F0000, false, false, false, false),
    (F0001, false, false, false, true),
    (F0010, false, false, true, false),
    (F0011, false, false, true, true),
    (F0100, false, true, false, false),
    (F0101, false, true, false, true),
    (F0110, false, true, true, false),
    (F0111, false, true, true, true),
    (F1000, true, false, false, false),
    (F1001, true, false, false, true),
    (F1010, true, false, true, false),
    (F1011, true, false, true, true),
    (F1100, true, true, false, false),
    (F1101, true, true, false, true),
    (F1110, true, true, true, false),
    (F1111, true, true, true, true),
);

/// The services a linked plugin is offered: the data services, `core.device`,
/// the kernel's own `core.ui` and `core.registry`, and the wired families.
/// A list in the program's read-only data (no heap).
#[must_use]
pub fn services(wired: Wired) -> &'static [Service<'static>] {
    match (wired.secrets, wired.work, wired.card) {
        (false, false, false) => &S000,
        (false, false, true) => &S001,
        (false, true, false) => &S010,
        (false, true, true) => &S011,
        (true, false, false) => &S100,
        (true, false, true) => &S101,
        (true, true, false) => &S110,
        (true, true, true) => &S111,
    }
}

/// The consent features a plugin file on the card (a card plugin) may ask
/// for: the families it is allowed to reach, no more. `uses.network` (HTTP and
/// sockets), `uses.storage` and `uses.library` where those are wired. Not
/// offered, whatever is wired, so a card plugin that asks for one does not get
/// it (refused at admission only if it requires it, which no plugin does of
/// these): `uses.secrets` (it reaches other plugins' secrets, their names and
/// labels, never the values), `uses.registry`, `uses.power`, `uses.wifi`, and
/// those `features` leaves out too (`uses.install`, `uses.firmware`,
/// `uses.bluetooth`). The first-party plugins that are linked into the firmware
/// have those ([`features`]). A list in the program's read-only data.
#[must_use]
pub fn card_features(wired: Wired) -> &'static [Feature<'static>] {
    static NONE: [Feature<'static>; 0] = [];
    static NETWORK: [Feature<'static>; 1] = [feature("uses.network")];
    static CARD: [Feature<'static>; 2] = [feature("uses.storage"), feature("uses.library")];
    static NETWORK_AND_CARD: [Feature<'static>; 3] = [
        feature("uses.network"),
        feature("uses.storage"),
        feature("uses.library"),
    ];
    match (wired.work, wired.card) {
        (false, false) => &NONE,
        (true, false) => &NETWORK,
        (false, true) => &CARD,
        (true, true) => &NETWORK_AND_CARD,
    }
}

/// The consent features a linked plugin may ask for (`uses.*`), with the
/// families that back them. Not offered: `uses.bluetooth`, `uses.install` and
/// `uses.firmware` (not backed yet: dual boot, todo 450): a plugin that needs
/// one is refused at admission. A list in the program's read-only data (no
/// heap).
#[must_use]
pub fn features(wired: Wired) -> &'static [Feature<'static>] {
    match (wired.secrets, wired.work, wired.card, wired.wifi) {
        (false, false, false, false) => &F0000,
        (false, false, false, true) => &F0001,
        (false, false, true, false) => &F0010,
        (false, false, true, true) => &F0011,
        (false, true, false, false) => &F0100,
        (false, true, false, true) => &F0101,
        (false, true, true, false) => &F0110,
        (false, true, true, true) => &F0111,
        (true, false, false, false) => &F1000,
        (true, false, false, true) => &F1001,
        (true, false, true, false) => &F1010,
        (true, false, true, true) => &F1011,
        (true, true, false, false) => &F1100,
        (true, true, false, true) => &F1101,
        (true, true, true, false) => &F1110,
        (true, true, true, true) => &F1111,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};

    /// Every combination of what a start wires, as the four groups.
    fn every_combination() -> impl Iterator<Item = Wired> {
        (0..16).map(|bits: u8| Wired {
            work: bits & 1 != 0,
            card: bits & 2 != 0,
            secrets: bits & 4 != 0,
            wifi: bits & 8 != 0,
        })
    }

    /// A list's keys with their major version or revision.
    type Keyed = Vec<(&'static str, u16)>;

    /// The lists as the firmware made them on the heap before they were
    /// static: one family at a time, in this order (the reference).
    fn made_by_hand(wired: Wired) -> (Keyed, Keyed) {
        let mut services: Vec<(&str, u16)> =
            DATA_SERVICES.iter().map(|s| (s.key, s.major)).collect();
        services.extend(DEVICE_SERVICES.iter().map(|s| (s.key, s.major)));
        services.push(("core.ui", 1));
        services.push(("core.registry", 1));
        for (on, keys) in [
            (wired.secrets, &["core.secrets"][..]),
            (wired.work, &["core.work", "core.net", "core.sockets"][..]),
            (
                wired.card,
                &["core.document", "core.library", "core.files"][..],
            ),
        ] {
            if on {
                services.extend(keys.iter().map(|key| (*key, 1)));
            }
        }
        let mut features = vec![("uses.registry", 1), ("uses.power", 1)];
        for (on, keys) in [
            (wired.secrets, &["uses.secrets"][..]),
            (wired.work, &["uses.network"][..]),
            (wired.card, &["uses.storage", "uses.library"][..]),
            (wired.wifi, &["uses.wifi"][..]),
        ] {
            if on {
                features.extend(keys.iter().map(|key| (*key, 1)));
            }
        }
        (services, features)
    }

    #[test]
    fn every_combination_has_the_list_the_families_make() {
        for wired in every_combination() {
            let (services_by_hand, features_by_hand) = made_by_hand(wired);
            let listed: Vec<_> = services(wired).iter().map(|s| (s.key, s.major)).collect();
            assert_eq!(listed, services_by_hand, "{wired:?}");
            let listed: Vec<_> = features(wired)
                .iter()
                .map(|f| (f.key, f.revision))
                .collect();
            assert_eq!(listed, features_by_hand, "{wired:?}");
        }
    }

    #[test]
    fn nothing_wired_offers_no_family_and_every_one_wired_offers_all() {
        let bare = services(Wired::default());
        let all = services(Wired::ALL);
        assert_eq!(all.len(), bare.len() + 7);
        assert!(bare.iter().any(|s| s.key == "core.ui"));
        assert!(!bare.iter().any(|s| s.key == "core.secrets"));
        assert!(all.iter().any(|s| s.key == "core.library"));
        assert!(features(Wired::ALL).iter().any(|f| f.key == "uses.library"));
        assert!(
            !features(Wired::default())
                .iter()
                .any(|f| f.key == "uses.secrets")
        );
    }

    #[test]
    fn the_lists_are_in_static_memory_and_the_same_each_time() {
        let all = Wired::ALL;
        assert!(core::ptr::eq(services(all), services(all)));
        assert!(core::ptr::eq(features(all), features(all)));
        assert!(!core::ptr::eq(services(all), services(Wired::default())));
    }

    #[test]
    fn a_card_plugin_is_offered_only_the_families_it_may_reach() {
        for wired in every_combination() {
            let card = card_features(wired);
            // Never what reaches another plugin's secrets or the device's
            // own controls, whatever is wired.
            for refused in [
                "uses.secrets",
                "uses.registry",
                "uses.power",
                "uses.wifi",
                "uses.install",
                "uses.firmware",
                "uses.bluetooth",
            ] {
                assert!(
                    !card.iter().any(|f| f.key == refused),
                    "{refused}: {wired:?}"
                );
            }
            // What is offered is also one a linked plugin has, and only where
            // its family is wired.
            for f in card {
                assert!(features(wired).iter().any(|l| l.key == f.key), "{wired:?}");
            }
            assert_eq!(
                card.iter().any(|f| f.key == "uses.network"),
                wired.work,
                "{wired:?}"
            );
            assert_eq!(
                card.iter().any(|f| f.key == "uses.storage"),
                wired.card,
                "{wired:?}"
            );
            assert_eq!(
                card.iter().any(|f| f.key == "uses.library"),
                wired.card,
                "{wired:?}"
            );
        }
    }
}
