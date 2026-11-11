#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The sealer's rules with a real AES-256-GCM (RustCrypto's, as the desktop
//! uses; the firmware's is mbedTLS) and a flash sector in memory.
extern crate std;

use super::*;
use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce, Tag};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_IO, ERR_UNSUPPORTED};
use std::collections::HashSet;
use std::vec::Vec;

struct Gcm;

impl Aead for Gcm {
    fn seal(
        &self,
        key: &[u8; KEY],
        nonce: &[u8; NONCE],
        aad: &[u8],
        plain: &[u8],
        cipher: &mut [u8],
        tag: &mut [u8; TAG],
    ) -> Result<(), i32> {
        let gcm = Aes256Gcm::new(key.into());
        cipher.copy_from_slice(plain);
        let made = gcm
            .encrypt_in_place_detached(Nonce::from_slice(nonce), aad, cipher)
            .map_err(|_| ERR_IO)?;
        tag.copy_from_slice(&made);
        Ok(())
    }

    fn open(
        &self,
        key: &[u8; KEY],
        nonce: &[u8; NONCE],
        aad: &[u8],
        cipher: &[u8],
        tag: &[u8; TAG],
        plain: &mut [u8],
    ) -> Result<(), i32> {
        let gcm = Aes256Gcm::new(key.into());
        plain.copy_from_slice(cipher);
        gcm.decrypt_in_place_detached(Nonce::from_slice(nonce), aad, plain, Tag::from_slice(tag))
            .map_err(|_| ERR_IO)
    }
}

/// How the flash's next erase and write go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Write {
    Fine,
    /// Power goes after the erase: the sector stays erased.
    CutAfterErase,
    /// Power goes in the middle of the record.
    CutHalfway,
    /// The flash writes something else than it was given.
    Garbles,
}

/// The key sector in memory, and what the sealer did to it.
struct Flash {
    sector: RefCell<Vec<u8>>,
    /// What `read` answers instead of the bytes.
    read_error: Cell<Option<i32>>,
    write: Cell<Write>,
    replaced: Cell<u32>,
}

impl Flash {
    fn erased() -> Rc<Self> {
        Rc::new(Self {
            sector: RefCell::new(std::vec![0xFF; dk::SECTOR]),
            read_error: Cell::new(None),
            write: Cell::new(Write::Fine),
            replaced: Cell::new(0),
        })
    }

    fn holding(bytes: &[u8]) -> Rc<Self> {
        let flash = Self::erased();
        flash.sector.borrow_mut()[..bytes.len()].copy_from_slice(bytes);
        flash
    }

    fn record(&self) -> [u8; dk::LEN] {
        self.sector.borrow()[..dk::LEN].try_into().unwrap()
    }
}

impl KeyStore for Rc<Flash> {
    fn read(&self, out: &mut [u8; dk::LEN]) -> Result<(), i32> {
        if let Some(error) = self.read_error.get() {
            return Err(error);
        }
        out.copy_from_slice(&self.sector.borrow()[..dk::LEN]);
        Ok(())
    }

    fn replace(&self, record: &[u8; dk::LEN]) -> Result<(), i32> {
        self.replaced.set(self.replaced.get() + 1);
        let mut sector = self.sector.borrow_mut();
        sector.fill(0xFF);
        match self.write.replace(Write::Fine) {
            Write::Fine => sector[..dk::LEN].copy_from_slice(record),
            Write::CutAfterErase => return Err(ERR_IO),
            Write::CutHalfway => {
                sector[..dk::LEN / 2].copy_from_slice(&record[..dk::LEN / 2]);
                return Err(ERR_IO);
            }
            Write::Garbles => {
                sector[..dk::LEN].copy_from_slice(record);
                sector[dk::LEN - 1] ^= 1;
            }
        }
        Ok(())
    }
}

/// A random source: distinct numbers from a counter, strong unless told,
/// or stuck on one value.
struct Random {
    next: Cell<u64>,
    strong: Cell<bool>,
    stuck: Cell<Option<u8>>,
    draws: Cell<u32>,
}

impl Random {
    fn new(seed: u64) -> Rc<Self> {
        Rc::new(Self {
            next: Cell::new(seed),
            strong: Cell::new(true),
            stuck: Cell::new(None),
            draws: Cell::new(0),
        })
    }
}

impl Rng for Random {
    fn fill(&self, out: &mut [u8]) -> Result<bool, i32> {
        self.draws.set(self.draws.get() + 1);
        for byte in out.iter_mut() {
            if let Some(value) = self.stuck.get() {
                *byte = value;
                continue;
            }
            // A 64-bit LCG, its top byte: distinct enough for a test.
            let n = self
                .next
                .get()
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.next.set(n);
            *byte = (n >> 56) as u8;
        }
        Ok(self.strong.get())
    }
}

fn sealer(flash: &Rc<Flash>, random: &Rc<Random>) -> KeySealer<Rc<Flash>, Gcm> {
    KeySealer::new(flash.clone(), Gcm, random.clone())
}

const AAD: &[u8] = b"wallabag\0token";

fn seal(s: &KeySealer<Rc<Flash>, Gcm>, plain: &[u8]) -> Result<Vec<u8>, i32> {
    let mut out = std::vec![0u8; dk::MAX_SEALED];
    let n = s.seal(AAD, plain, &mut out)?;
    out.truncate(n);
    Ok(out)
}

fn open(s: &KeySealer<Rc<Flash>, Gcm>, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, i32> {
    let mut out = std::vec![0xAA_u8; dk::MAX_PLAIN];
    match s.open(aad, sealed, &mut out) {
        Ok(n) => {
            out.truncate(n);
            Ok(out)
        }
        Err(error) => {
            // A value that is no value is refused before anything is
            // written; any other failure leaves the output wiped.
            if error != ERR_INVALID {
                let len = sealed.len().saturating_sub(NONCE + TAG).min(out.len());
                assert!(
                    out[..len].iter().all(|&b| b == 0),
                    "a failed open wipes its output"
                );
            }
            Err(error)
        }
    }
}

#[test]
fn a_value_comes_back_and_is_never_in_the_clear() {
    let (flash, random) = (Flash::erased(), Random::new(1));
    let s = sealer(&flash, &random);
    let plain = b"correct horse battery staple";
    let sealed = seal(&s, plain).unwrap();
    assert_eq!(sealed.len(), NONCE + plain.len() + TAG);
    assert!(!sealed.windows(plain.len()).any(|w| w == plain));
    assert_eq!(open(&s, AAD, &sealed).unwrap(), plain);
    // The empty value and the longest one.
    assert_eq!(open(&s, AAD, &seal(&s, b"").unwrap()).unwrap(), b"");
    let long = [7u8; dk::MAX_PLAIN];
    assert_eq!(open(&s, AAD, &seal(&s, &long).unwrap()).unwrap(), long);
    assert_eq!(seal(&s, &[0u8; dk::MAX_PLAIN + 1]), Err(ERR_INVALID));
    let mut short = [0u8; NONCE + 3 + TAG - 1];
    assert_eq!(s.seal(AAD, b"abc", &mut short), Err(ERR_NO_SPACE));
    // A key was made once, at the first seal, and is kept.
    assert_eq!(flash.replaced.get(), 1);
    assert!(matches!(dk::read(&flash.record()), Sector::Key(_)));
}

#[test]
fn another_devices_key_opens_nothing() {
    let (mine, theirs) = (Flash::erased(), Flash::erased());
    let sealed = seal(&sealer(&theirs, &Random::new(2)), b"secret").unwrap();
    let s = sealer(&mine, &Random::new(3));
    // This device has no key yet: absent, and none is made by opening.
    assert_eq!(open(&s, AAD, &sealed), Err(ERR_NOT_FOUND));
    assert_eq!(mine.replaced.get(), 0);
    // With a key of its own: still absent.
    seal(&s, b"mine").unwrap();
    assert_eq!(open(&s, AAD, &sealed), Err(ERR_NOT_FOUND));
}

#[test]
fn any_changed_byte_or_other_owner_opens_nothing() {
    let (flash, random) = (Flash::erased(), Random::new(4));
    let s = sealer(&flash, &random);
    let sealed = seal(&s, b"a password").unwrap();
    for at in 0..sealed.len() {
        let mut bad = sealed.clone();
        bad[at] ^= 0x01;
        assert_eq!(open(&s, AAD, &bad), Err(ERR_NOT_FOUND), "byte {at}");
    }
    // The line copied to another owner or name.
    assert_eq!(open(&s, b"wallabag\0other", &sealed), Err(ERR_NOT_FOUND));
    assert_eq!(open(&s, b"news\0token", &sealed), Err(ERR_NOT_FOUND));
    // Cut short or too long to be a value.
    assert_eq!(open(&s, AAD, &sealed[..NONCE + TAG - 1]), Err(ERR_INVALID));
    assert_eq!(open(&s, AAD, &[0u8; dk::MAX_SEALED + 1]), Err(ERR_INVALID));
}

#[test]
fn every_seal_has_a_nonce_of_its_own() {
    let (flash, random) = (Flash::erased(), Random::new(5));
    let s = sealer(&flash, &random);
    let mut seen = HashSet::new();
    for _ in 0..2_000 {
        let sealed = seal(&s, b"same value").unwrap();
        assert!(seen.insert(sealed[..NONCE].to_vec()), "a nonce came twice");
    }
}

#[test]
fn a_stuck_generator_seals_nothing() {
    let (flash, random) = (Flash::erased(), Random::new(6));
    let s = sealer(&flash, &random);
    seal(&s, b"first").unwrap();
    // The generator gives the same bytes again and again.
    random.stuck.set(Some(0x42));
    assert_eq!(
        seal(&s, b"one").map(|_| ()),
        Ok(()),
        "the first such nonce is new"
    );
    assert_eq!(seal(&s, b"two"), Err(ERR_STATE), "the same nonce again");
    // All zeros or all ones are never a nonce.
    for value in [0x00, 0xFF] {
        random.stuck.set(Some(value));
        assert_eq!(seal(&s, b"x"), Err(ERR_STATE));
    }
    random.stuck.set(None);
    assert!(seal(&s, b"back").is_ok());
    assert_eq!(flash.replaced.get(), 1, "the key stayed");
}

#[test]
fn weak_numbers_make_no_key_and_no_nonce() {
    let (flash, random) = (Flash::erased(), Random::new(7));
    random.strong.set(false);
    let s = sealer(&flash, &random);
    assert_eq!(seal(&s, b"x"), Err(ERR_STATE));
    assert_eq!(flash.replaced.get(), 0, "no key from weak numbers");
    // With a key there, a weak nonce is refused all the same.
    random.strong.set(true);
    let sealed = seal(&s, b"x").unwrap();
    random.strong.set(false);
    assert_eq!(seal(&s, b"y"), Err(ERR_STATE));
    // Opening needs no random numbers.
    assert_eq!(open(&s, AAD, &sealed).unwrap(), b"x");
}

#[test]
fn a_key_that_reads_is_never_replaced() {
    let key: [u8; KEY] = core::array::from_fn(|i| i as u8);
    let flash = Flash::holding(&dk::record(&key));
    let random = Random::new(8);
    let s = sealer(&flash, &random);
    let sealed = seal(&s, b"v").unwrap();
    assert_eq!(flash.replaced.get(), 0);
    assert_eq!(dk::read(&flash.record()), Sector::Key(key));
    // A second sealer of the same device (after a restart) opens it.
    assert_eq!(
        open(&sealer(&flash, &Random::new(9)), AAD, &sealed).unwrap(),
        b"v"
    );
}

#[test]
fn a_sector_that_is_not_ours_is_never_touched() {
    // An NVS page of the stock firmware or of a dual-boot partner (#920).
    let mut page = [0xFF_u8; 64];
    page[..8].copy_from_slice(&[0xFE, 0xFF, 0xFF, 0xFF, 1, 0, 0, 0]);
    let flash = Flash::holding(&page);
    let before = flash.sector.borrow().clone();
    let s = sealer(&flash, &Random::new(10));
    assert_eq!(seal(&s, b"x"), Err(ERR_STATE));
    assert_eq!(open(&s, AAD, &[0u8; NONCE + 4 + TAG]), Err(ERR_NOT_FOUND));
    assert_eq!(flash.replaced.get(), 0);
    assert_eq!(*flash.sector.borrow(), before);
}

#[test]
fn a_flash_that_does_not_read_is_never_erased() {
    let flash = Flash::erased();
    let s = sealer(&flash, &Random::new(11));
    for error in [ERR_IO, ERR_BUSY] {
        flash.read_error.set(Some(error));
        assert_eq!(seal(&s, b"x"), Err(error));
        assert_eq!(open(&s, AAD, &[0u8; NONCE + 1 + TAG]), Err(error));
    }
    assert_eq!(flash.replaced.get(), 0);
}

#[test]
fn a_device_without_a_key_place_seals_nothing_and_finds_nothing() {
    let flash = Flash::erased();
    let s = sealer(&flash, &Random::new(12));
    // No `nvs` partition: the host's "this device keeps no key" (ERR_STATE).
    flash.read_error.set(Some(ERR_NOT_FOUND));
    assert_eq!(seal(&s, b"x"), Err(ERR_STATE));
    assert_eq!(open(&s, AAD, &[0u8; NONCE + 1 + TAG]), Err(ERR_NOT_FOUND));
    // An X4 Classic on CrossPoint's table (decision 141): reinstall once.
    flash.read_error.set(Some(ERR_UNSUPPORTED));
    assert_eq!(seal(&s, b"x"), Err(ERR_UNSUPPORTED));
    assert_eq!(open(&s, AAD, &[0u8; NONCE + 1 + TAG]), Err(ERR_NOT_FOUND));
    assert_eq!(flash.replaced.get(), 0);
}

#[test]
fn power_lost_while_the_key_is_written_costs_nothing_sealed() {
    for cut in [Write::CutAfterErase, Write::CutHalfway] {
        let (flash, random) = (Flash::erased(), Random::new(13));
        let s = sealer(&flash, &random);
        flash.write.set(cut);
        assert_eq!(seal(&s, b"x"), Err(ERR_IO), "nothing was sealed");
        // What the cut left is no key, and ours to erase.
        assert!(matches!(
            dk::read(&flash.record()),
            Sector::Erased | Sector::Damaged
        ));
        // The next start (a new sealer) makes a key and seals.
        let s = sealer(&flash, &random);
        let sealed = seal(&s, b"x").unwrap();
        assert_eq!(open(&s, AAD, &sealed).unwrap(), b"x");
        assert_eq!(flash.replaced.get(), 2);
    }
}

#[test]
fn a_key_that_does_not_read_back_is_not_used() {
    let (flash, random) = (Flash::erased(), Random::new(14));
    let s = sealer(&flash, &random);
    flash.write.set(Write::Garbles);
    assert_eq!(seal(&s, b"x"), Err(ERR_IO));
    // Its record fails its checksum: ours, and replaced at the next seal.
    assert_eq!(dk::read(&flash.record()), Sector::Damaged);
    assert!(seal(&s, b"x").is_ok());
}
