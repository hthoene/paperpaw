//! What the firmware offers the plugins of interface 7 that are linked into
//! it (`paperpaw_app::NATIVE_PLUGINS`): the services and the consent
//! features, as the families the start wired (the kernel's admission refuses
//! a plugin that needs one that is not in the list), and nothing else.
//!
//! The same lists are the ones the firmware gives the kernel
//! (`shells/xteink/src/native/ppn5.rs`) and the ones the host tests admit
//! every linked plugin against (`shells/desktop/tests/x4_offer.rs`), so a
//! plugin that would be refused on the device is found on a computer.
use alloc::{boxed::Box, vec::Vec};
use paperpaw_core::native_host::{device_service::DEVICE_SERVICES, shell::DATA_SERVICES};
use paperpaw_native::metadata::{Feature, Service};

/// Which families a start wired. A family is offered only where it is wired:
/// a service offered and not backed is a plugin that fails later.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wired {
    /// `core.work` (with the network's bridge).
    pub work: bool,
    /// `core.net`.
    pub net: bool,
    /// `core.sockets`.
    pub sockets: bool,
    /// `core.document`.
    pub documents: bool,
    /// `core.library`.
    pub library: bool,
    /// `core.files`.
    pub files: bool,
    /// `core.secrets` (the device's, sealed with its key).
    pub secrets: bool,
    /// The radio's commands (`uses.wifi`): the kernel answers them from its
    /// Wi-Fi model, which a device with a radio has.
    pub wifi: bool,
    /// Plugin installs (`uses.install`): the kernel's plugin jobs over the
    /// card, with the firmware host their queue shares (#2654).
    pub installs: bool,
}

impl Wired {
    /// Every family the firmware wires for linked plugins.
    pub const ALL: Self = Self {
        work: true,
        net: true,
        sockets: true,
        documents: true,
        library: true,
        files: true,
        secrets: true,
        wifi: true,
        installs: true,
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

/// The services a linked plugin is offered: the data services, `core.device`,
/// the kernel's own `core.ui` and `core.registry`, and the wired families.
#[must_use]
pub fn services(wired: Wired) -> Vec<Service<'static>> {
    let mut list: Vec<Service<'static>> = Vec::new();
    list.extend_from_slice(DATA_SERVICES);
    list.extend_from_slice(DEVICE_SERVICES);
    list.push(service("core.ui"));
    list.push(service("core.registry"));
    for (on, key) in [
        (wired.secrets, "core.secrets"),
        (wired.work, "core.work"),
        (wired.net, "core.net"),
        (wired.sockets, "core.sockets"),
        (wired.documents, "core.document"),
        (wired.library, "core.library"),
        (wired.files, "core.files"),
    ] {
        if on {
            list.push(service(key));
        }
    }
    list
}

/// The consent features a linked plugin may ask for (`uses.*`), with the
/// families that back them. Not offered: `uses.bluetooth` and `uses.firmware`
/// (not backed yet: dual boot, todo 450): a plugin that needs one is refused at
/// admission. `uses.install` only where the plugin jobs are wired.
#[must_use]
pub fn features(wired: Wired) -> Vec<Feature<'static>> {
    let mut list = alloc::vec![feature("uses.registry"), feature("uses.power")];
    if wired.secrets {
        list.push(feature("uses.secrets"));
    }
    // One feature for HTTP and sockets (`network.md` 1).
    if wired.net || wired.sockets {
        list.push(feature("uses.network"));
    }
    if wired.files {
        list.push(feature("uses.storage"));
    }
    if wired.library {
        list.push(feature("uses.library"));
    }
    // The radio commands of `core.device` (ops 8, 10, 11, 12), answered by the
    // kernel's Wi-Fi model.
    if wired.wifi {
        list.push(feature("uses.wifi"));
    }
    // Installing plugins: the jobs over the card (`core.device` ops of the
    // `uses.install` family), where the firmware wired them.
    if wired.installs {
        list.push(feature("uses.install"));
    }
    list
}

/// The lists as the kernel keeps them for as long as the plugins run: made
/// once for a start and left.
#[must_use]
pub fn leak<T>(list: Vec<T>) -> &'static [T] {
    Box::leak(list.into_boxed_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn installs_are_offered_only_where_the_plugin_jobs_are_wired() {
        assert!(features(Wired::ALL).iter().any(|f| f.key == "uses.install"));
        assert!(
            !features(Wired::default())
                .iter()
                .any(|f| f.key == "uses.install")
        );
    }
}
