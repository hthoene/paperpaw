//! Firmware updates for devices whose firmware PaperPaw is (the X4): the
//! update file, what its image says about itself, and its signature,
//! checked while it is read.
//!
//! An update file is the app image as `espflash save-image` writes it,
//! followed by a [`Trailer`] of [`TRAILER_LEN`] bytes:
//!
//! | Bytes | What |
//! |-------|------|
//! | 0..8 | `PaperPaw` |
//! | 8..12 | the format, 1 (little-endian) |
//! | 12..16 | the image's length (little-endian) |
//! | 16..48 | the signer's Ed25519 public key |
//! | 48..112 | the signature |
//! | 112..128 | zero |
//!
//! The signature (Ed25519) covers the image and then the trailer's first
//! 48 bytes, so neither the length nor the key can be changed. A device
//! checks it with [`Check`] as it writes the image, a piece at a time, and
//! starts the new image only if it holds.
//!
//! A whole flash image, as the web flasher writes it from address 0, is
//! signed apart: its signature file (`.sig`) is the trailer alone, made
//! and checked the same way ([`sign`], [`check_apart`]).
//!
//! An image also says which code plugins it has built in ([`Plugins`],
//! right after its application descriptor), so that an update can bring
//! the same ones.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use ed25519_compact::{PublicKey, Signature, VerifyingState};

mod plugins;

pub use plugins::{
    PLUGINS_AT, PLUGINS_FORMAT, PLUGINS_HEAD_LEN, PLUGINS_MAGIC, PLUGINS_MAX_LEN, PluginCrate,
    Plugins,
};

/// The trailer's length.
pub const TRAILER_LEN: usize = 128;
/// The trailer's first bytes.
pub const MAGIC: [u8; 8] = *b"PaperPaw";
/// The trailer's format this crate reads and writes.
pub const FORMAT: u32 = 1;
/// The bytes of the trailer the signature covers.
const SIGNED_LEN: usize = 48;

/// How many of an image's first bytes [`AppInfo::read`] needs.
pub const APP_HEAD_LEN: usize = 208;
/// The chip id of the ESP32-C3 (the X4's) in an image's header.
pub const CHIP_ESP32_C3: u16 = 5;
/// The chip id of the ESP32-S3 (the X4 Pro's and the X4 Classic's) in an
/// image's header.
pub const CHIP_ESP32_S3: u16 = 9;

/// What is wrong with an update file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Problem {
    /// No trailer: not an update file (e.g. a whole flash image for the
    /// web flasher).
    NotAnUpdate,
    /// A trailer of a newer format than this firmware reads.
    NewerFormat,
    /// The image is not one the bootloader starts.
    NotAnImage,
    /// The file is not as long as its trailer says.
    Length,
    /// The signature does not hold: the file was changed or damaged.
    Signature,
    /// A cause this build has no variant for, by its code (0: not said).
    /// Codes are listed here as they are given and never reused; a build
    /// that doesn't know one treats it as the most general failure of its
    /// kind.
    Other(u16),
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Problem::NotAnUpdate => "not an update file",
            Problem::NewerFormat => "an update file of a newer format",
            Problem::NotAnImage => "no firmware image",
            Problem::Length => "not as long as it says",
            Problem::Signature => "the signature does not hold",
            Problem::Other(code) => return write!(f, "update problem {code}"),
        })
    }
}

/// The end of an update file: the image's length, the signer's key and
/// the signature (see the [crate documentation](crate)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trailer {
    /// The image's length in bytes.
    pub image_len: u32,
    /// The signer's Ed25519 public key.
    pub key: [u8; 32],
    pub signature: [u8; 64],
}

impl Trailer {
    /// The trailer in an update file's last [`TRAILER_LEN`] bytes, given
    /// the file's length.
    pub fn read(bytes: &[u8], file_len: u64) -> Result<Self, Problem> {
        if bytes.len() != TRAILER_LEN || bytes[..8] != MAGIC {
            return Err(Problem::NotAnUpdate);
        }
        if le32(&bytes[8..12]) != FORMAT {
            return Err(Problem::NewerFormat);
        }
        let image_len = le32(&bytes[12..16]);
        if u64::from(image_len) + TRAILER_LEN as u64 != file_len {
            return Err(Problem::Length);
        }
        let mut key = [0; 32];
        key.copy_from_slice(&bytes[16..48]);
        let mut signature = [0; 64];
        signature.copy_from_slice(&bytes[48..112]);
        Ok(Self {
            image_len,
            key,
            signature,
        })
    }

    /// The trailer's bytes.
    pub fn to_bytes(&self) -> [u8; TRAILER_LEN] {
        let mut bytes = [0; TRAILER_LEN];
        bytes[..SIGNED_LEN].copy_from_slice(&signed_part(self.image_len, &self.key));
        bytes[48..112].copy_from_slice(&self.signature);
        bytes
    }
}

/// The trailer's bytes the signature covers.
fn signed_part(image_len: u32, key: &[u8; 32]) -> [u8; SIGNED_LEN] {
    let mut bytes = [0; SIGNED_LEN];
    bytes[..8].copy_from_slice(&MAGIC);
    bytes[8..12].copy_from_slice(&FORMAT.to_le_bytes());
    bytes[12..16].copy_from_slice(&image_len.to_le_bytes());
    bytes[16..48].copy_from_slice(key);
    bytes
}

fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// The signature, checked as the image is read: [`Check::absorb`] every
/// byte of it in order, then [`Check::finish`].
pub struct Check {
    state: VerifyingState,
    signed: [u8; SIGNED_LEN],
    image_len: u32,
    absorbed: u64,
}

impl fmt::Debug for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Check")
            .field("image_len", &self.image_len)
            .field("absorbed", &self.absorbed)
            .finish_non_exhaustive()
    }
}

impl Check {
    pub fn new(trailer: &Trailer) -> Result<Self, Problem> {
        let key = PublicKey::new(trailer.key);
        let signature = Signature::new(trailer.signature);
        let state = key
            .verify_incremental(&signature)
            .map_err(|_| Problem::Signature)?;
        Ok(Self {
            state,
            signed: signed_part(trailer.image_len, &trailer.key),
            image_len: trailer.image_len,
            absorbed: 0,
        })
    }

    /// The image's next bytes.
    pub fn absorb(&mut self, chunk: &[u8]) {
        self.state.absorb(chunk);
        self.absorbed += chunk.len() as u64;
    }

    /// Whether the signature holds for all that was absorbed.
    pub fn finish(mut self) -> Result<(), Problem> {
        if self.absorbed != u64::from(self.image_len) {
            return Err(Problem::Length);
        }
        self.state.absorb(self.signed);
        self.state.verify().map_err(|_| Problem::Signature)
    }
}

/// Signs `image` with the Ed25519 key made from `seed`: the trailer to
/// append to it.
pub fn sign(image: &[u8], seed: &[u8; 32]) -> Result<Trailer, Problem> {
    let image_len = u32::try_from(image.len()).map_err(|_| Problem::Length)?;
    let pair = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new(*seed));
    let key = *pair.pk;
    // Deterministic Ed25519 over the whole message: the streaming signer
    // would need noise that never repeats.
    let mut message = Vec::with_capacity(image.len() + SIGNED_LEN);
    message.extend_from_slice(image);
    message.extend_from_slice(&signed_part(image_len, &key));
    Ok(Trailer {
        image_len,
        key,
        signature: *pair.sk.sign(&message, None),
    })
}

/// Checks `image` against its signature file `signature` (the trailer
/// alone, see the [crate documentation](crate)): the trailer if the
/// signature holds; who signed it is for the caller to judge
/// ([`Trailer::key`]).
pub fn check_apart(image: &[u8], signature: &[u8]) -> Result<Trailer, Problem> {
    let trailer = Trailer::read(signature, image.len() as u64 + TRAILER_LEN as u64)?;
    let mut check = Check::new(&trailer)?;
    check.absorb(image);
    check.finish()?;
    Ok(trailer)
}

/// The public key of the Ed25519 key made from `seed`.
pub fn public_key(seed: &[u8; 32]) -> [u8; 32] {
    *ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new(*seed)).pk
}

/// A key as 64 hexadecimal digits, as kept in lists of trusted keys.
pub fn key_hex(key: &[u8; 32]) -> String {
    let mut text = String::with_capacity(64);
    for b in key {
        push_hex(&mut text, *b);
    }
    text
}

/// A key from 64 hexadecimal digits.
pub fn parse_key(text: &str) -> Option<[u8; 32]> {
    let text = text.trim().as_bytes();
    if text.len() != 64 {
        return None;
    }
    let mut key = [0; 32];
    for (i, pair) in text.chunks(2).enumerate() {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        key[i] = (hi * 16 + lo) as u8;
    }
    Some(key)
}

/// A key short enough to compare by eye: its first eight bytes in four
/// groups, e.g. `3F2A 91C0 77D4 0B1E`.
pub fn fingerprint(key: &[u8; 32]) -> String {
    let mut text = String::with_capacity(19);
    for (i, b) in key[..8].iter().enumerate() {
        if i > 0 && i % 2 == 0 {
            text.push(' ');
        }
        push_hex(&mut text, *b);
    }
    text.make_ascii_uppercase();
    text
}

fn push_hex(text: &mut String, b: u8) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    text.push(DIGITS[usize::from(b >> 4)] as char);
    text.push(DIGITS[usize::from(b & 15)] as char);
}

/// What an app image says about itself: its header and the application
/// descriptor (`esp_app_desc_t`) at the start of its first segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo {
    /// The chip it is for, e.g. [`CHIP_ESP32_C3`].
    pub chip: u16,
    /// Its version, e.g. `0.4.0`.
    pub version: String,
    /// Its project, e.g. `paperpaw-xteink`: which firmware it is.
    pub project: String,
}

impl AppInfo {
    /// Reads an image's first [`APP_HEAD_LEN`] bytes.
    pub fn read(head: &[u8]) -> Result<Self, Problem> {
        // The image header (24 bytes), the first segment's (8), then the
        // descriptor, which begins with its magic word.
        if head.len() < APP_HEAD_LEN || head[0] != 0xE9 || le32(&head[32..36]) != 0xABCD_5432 {
            return Err(Problem::NotAnImage);
        }
        let text = |range: core::ops::Range<usize>| {
            let bytes = &head[range];
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            String::from(core::str::from_utf8(&bytes[..end]).unwrap_or_default())
        };
        Ok(Self {
            chip: u16::from_le_bytes([head[12], head[13]]),
            version: text(48..80),
            project: text(80..112),
        })
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    const SEED: [u8; 32] = [7; 32];

    /// An image header and descriptor as espflash writes them.
    fn image(len: usize) -> Vec<u8> {
        image_for(CHIP_ESP32_C3, len)
    }

    fn image_for(chip: u16, len: usize) -> Vec<u8> {
        let mut image = std::vec![0u8; len];
        image[0] = 0xE9;
        image[12..14].copy_from_slice(&chip.to_le_bytes());
        image[32..36].copy_from_slice(&0xABCD_5432u32.to_le_bytes());
        image[48..53].copy_from_slice(b"0.4.0");
        image[80..95].copy_from_slice(b"paperpaw-xteink");
        for (i, b) in image.iter_mut().enumerate().skip(APP_HEAD_LEN) {
            *b = i as u8;
        }
        image
    }

    fn update(image: &[u8]) -> Vec<u8> {
        let mut file = image.to_vec();
        file.extend_from_slice(&sign(image, &SEED).unwrap().to_bytes());
        file
    }

    fn check(file: &[u8], piece: usize) -> Result<(), Problem> {
        let split = file.len() - TRAILER_LEN;
        let trailer = Trailer::read(&file[split..], file.len() as u64)?;
        let mut check = Check::new(&trailer)?;
        for chunk in file[..split].chunks(piece) {
            check.absorb(chunk);
        }
        check.finish()
    }

    #[test]
    fn a_signed_file_holds_in_any_pieces() {
        let file = update(&image(10_000));
        assert_eq!(check(&file, 4096), Ok(()));
        assert_eq!(check(&file, 7), Ok(()));
        let trailer = Trailer::read(&file[10_000..], file.len() as u64).unwrap();
        assert_eq!(trailer.key, public_key(&SEED));
        assert_eq!(trailer.image_len, 10_000);
    }

    #[test]
    fn a_changed_byte_breaks_it() {
        let mut file = update(&image(10_000));
        file[5000] ^= 1;
        assert_eq!(check(&file, 4096), Err(Problem::Signature));

        // Another key in the trailer.
        let mut file = update(&image(10_000));
        file[10_000 + 20] ^= 1;
        assert_eq!(check(&file, 4096), Err(Problem::Signature));
    }

    #[test]
    fn a_signature_file_holds_for_its_image_alone() {
        let flash = image(20_000);
        let signature = sign(&flash, &SEED).unwrap().to_bytes();
        let trailer = check_apart(&flash, &signature).unwrap();
        assert_eq!(trailer.key, public_key(&SEED));
        let mut changed = flash.clone();
        changed[12_345] ^= 1;
        assert_eq!(check_apart(&changed, &signature), Err(Problem::Signature));
        assert_eq!(check_apart(&flash[1..], &signature), Err(Problem::Length));
        assert_eq!(
            check_apart(&flash, &signature[..64]),
            Err(Problem::NotAnUpdate)
        );
    }

    #[test]
    fn length_and_format_are_checked() {
        let file = update(&image(1000));
        let trailer = &file[1000..];
        assert_eq!(
            Trailer::read(trailer, file.len() as u64 + 1),
            Err(Problem::Length)
        );
        let mut newer = trailer.to_vec();
        newer[8] = 2;
        assert_eq!(
            Trailer::read(&newer, file.len() as u64),
            Err(Problem::NewerFormat)
        );
        assert_eq!(
            Trailer::read(&[0; TRAILER_LEN], 1128),
            Err(Problem::NotAnUpdate)
        );
        // Fewer bytes absorbed than the image has.
        let trailer = Trailer::read(trailer, file.len() as u64).unwrap();
        let mut check = Check::new(&trailer).unwrap();
        check.absorb(&file[..999]);
        assert_eq!(check.finish(), Err(Problem::Length));
    }

    #[test]
    fn the_image_says_what_it_is() {
        let info = AppInfo::read(&image(APP_HEAD_LEN)).unwrap();
        assert_eq!(info.chip, CHIP_ESP32_C3);
        assert_eq!(info.version, "0.4.0");
        assert_eq!(info.project, "paperpaw-xteink");
        let s3 = AppInfo::read(&image_for(CHIP_ESP32_S3, APP_HEAD_LEN)).unwrap();
        assert_eq!(s3.chip, CHIP_ESP32_S3);
        let mut other = image(APP_HEAD_LEN);
        other[32] = 0;
        assert_eq!(AppInfo::read(&other), Err(Problem::NotAnImage));
        assert_eq!(AppInfo::read(&other[..100]), Err(Problem::NotAnImage));
    }

    /// A beta's image says its whole version (the descriptor's field has
    /// 32 bytes), and its update is signed and checked as a release's.
    #[test]
    fn a_beta_is_an_image_as_a_release_is() {
        let mut beta = image(APP_HEAD_LEN + 64);
        beta[48..80].fill(0);
        beta[48..61].copy_from_slice(b"0.5.0-beta.10");
        assert_eq!(AppInfo::read(&beta).unwrap().version, "0.5.0-beta.10");
        let file = update(&beta);
        assert_eq!(check(&file, 7), Ok(()));
    }

    #[test]
    fn keys_as_text() {
        let key = public_key(&SEED);
        assert_eq!(parse_key(&key_hex(&key)), Some(key));
        assert_eq!(parse_key("12"), None);
        assert_eq!(parse_key(&"g".repeat(64)), None);
        let fp = fingerprint(&[0xab; 32]);
        assert_eq!(fp, "ABAB ABAB ABAB ABAB");
    }
}
