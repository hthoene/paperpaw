//! What the firmware offers the plugins of interface 7 that are linked into
//! it (`paperpaw_app::NATIVE_PLUGINS`): the services and the consent
//! features, as the families the start wired (the kernel's admission refuses
//! a plugin that needs one that is not in the list), and the sealer of a
//! device that does not seal yet.
//!
//! The same lists are the ones the firmware gives the kernel
//! (`shells/xteink/src/native/ppn5.rs`) and the ones the host tests admit
//! every linked plugin against (`shells/desktop/tests/x4_offer.rs`), so a
//! plugin that would be refused on the device is found on a computer.
use alloc::{boxed::Box, vec::Vec};
use paperpaw_core::native_host::{
    device_service::DEVICE_SERVICES, shell::DATA_SERVICES, shell_wiring::Sealer,
};
use paperpaw_native::{
    host_glue::ERR_STATE,
    metadata::{Feature, Service},
};

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
    /// `core.secrets` (the device's, which seals or does not: [`NoSeal`]).
    pub secrets: bool,
    /// The radio's commands (`uses.wifi`): the kernel answers them from its
    /// Wi-Fi model, which a device with a radio has.
    pub wifi: bool,
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
/// families that back them. Not offered: `uses.bluetooth`, `uses.install` and
/// `uses.firmware` (not backed yet: dual boot, todo 450): a plugin that needs
/// one is refused at admission.
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
    list
}

/// The lists as the kernel keeps them for as long as the plugins run: made
/// once for a start and left.
#[must_use]
pub fn leak<T>(list: Vec<T>) -> &'static [T] {
    Box::leak(list.into_boxed_slice())
}

/// The sealer of a device that does not seal yet (decision 156, the first
/// test image): `core.secrets` is offered, so the plugins that declare it are
/// admitted, but nothing is written or read: a plugin that saves a value gets
/// `ERR_STATE` (Wi-Fi asks for its password each time), and interface 6's
/// values stay veiled as in Beta 3 (the kernel is not told the device seals,
/// `Kernel::wire_secrets`). Replaced by the AES-256-GCM sealer with the
/// device key (decision 138, todo 64) before beta.1.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSeal;

impl Sealer for NoSeal {
    fn seals(&self) -> bool {
        false
    }

    fn seal(&self, _aad: &[u8], _plain: &[u8], _out: &mut [u8]) -> Result<usize, i32> {
        Err(ERR_STATE)
    }

    fn open(&self, _aad: &[u8], _sealed: &[u8], _out: &mut [u8]) -> Result<usize, i32> {
        Err(ERR_STATE)
    }
}

/// The veiling sealer of the TEST image only (decision 156, Manager's ruling
/// for #2460): values are written in the `s1:` form but only veiled, as
/// Beta 3 veils them, so Wi-Fi, settings and the lock keep their passwords on
/// a test device. It is not encryption. It exists only with the feature
/// `test-veil`, which the firmware's `release` feature refuses at compile time
/// (`shells/xteink/src/main.rs`) and `tools/check-test-image-guard.py`
/// refuses in the shipped feature lists. Replaced by the AES-256-GCM sealer
/// (decision 138, todo 64) before beta.1; values it wrote do not open then
/// and count as absent (decision 138).
#[cfg(feature = "test-veil")]
#[derive(Clone, Copy, Debug, Default)]
pub struct VeilSeal;

/// The first byte of a veiled value: no real sealer's output starts with it
/// by design (a real one starts with its nonce, which a veiled value never
/// passes: it fails the tag check there and the value is absent).
#[cfg(feature = "test-veil")]
const VEIL_TAG: u8 = 0xFE;

#[cfg(feature = "test-veil")]
fn veil_byte(aad: &[u8], at: usize) -> u8 {
    let salt = if aad.is_empty() {
        0
    } else {
        aad[at % aad.len()]
    };
    (at as u8).wrapping_mul(37).wrapping_add(0x5A) ^ salt
}

#[cfg(feature = "test-veil")]
impl Sealer for VeilSeal {
    fn seals(&self) -> bool {
        true
    }

    fn seal(&self, aad: &[u8], plain: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let total = plain.len() + 1;
        let out = out.get_mut(..total).ok_or(ERR_STATE)?;
        out[0] = VEIL_TAG;
        for (at, (o, p)) in out[1..].iter_mut().zip(plain).enumerate() {
            *o = p ^ veil_byte(aad, at);
        }
        Ok(total)
    }

    fn open(&self, aad: &[u8], sealed: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let (&tag, body) = sealed.split_first().ok_or(ERR_STATE)?;
        if tag != VEIL_TAG {
            return Err(ERR_STATE);
        }
        let out = out.get_mut(..body.len()).ok_or(ERR_STATE)?;
        for (at, (o, s)) in out.iter_mut().zip(body).enumerate() {
            *o = s ^ veil_byte(aad, at);
        }
        Ok(body.len())
    }
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
    fn a_device_that_does_not_seal_says_so_and_writes_nothing() {
        let mut out = [0; 8];
        assert!(!NoSeal.seals());
        assert_eq!(NoSeal.seal(b"", b"x", &mut out), Err(ERR_STATE));
        assert_eq!(NoSeal.open(b"", b"x", &mut out), Err(ERR_STATE));
    }

    #[cfg(feature = "test-veil")]
    #[test]
    fn the_test_veil_round_trips_and_is_bound_to_the_row() {
        let mut blob = [0; 64];
        let n = VeilSeal.seal(b"wifi.pw", b"hunter2", &mut blob).unwrap();
        assert_ne!(&blob[1..n], b"hunter2");
        let mut back = [0; 64];
        let m = VeilSeal.open(b"wifi.pw", &blob[..n], &mut back).unwrap();
        assert_eq!(&back[..m], b"hunter2");
        let m = VeilSeal.open(b"other", &blob[..n], &mut back).unwrap();
        assert_ne!(&back[..m], b"hunter2");
        assert_eq!(VeilSeal.open(b"", &[0x00, 1], &mut back), Err(ERR_STATE));
        assert!(VeilSeal.seals());
    }
}
