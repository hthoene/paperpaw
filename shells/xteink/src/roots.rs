//! The root certificates TLS trusts, as a list in flash and a longer one on
//! the card (#387). The firmware has about twenty roots built in
//! (`roots-builtin.bin`, made from `tools/roots/builtin.txt`: those of
//! PaperPaw's own services, of GitHub, of the services the first-party plugins
//! use and of the largest hosts); Mozilla's whole list is the data plugin
//! `tls-roots` ("All certificates" in the store, off at first), a file
//! `plugins/tls-roots/roots.pack` signed with PaperPaw's plugin key. Both are
//! looked up by name (`paperpaw_x4::tls_roots`) and never parsed into
//! memory, as a list of trusted certificates for mbedTLS would be (well over
//! 100 KB): mbedTLS checks the chain a server sends, and when its top is
//! not signed by a certificate it knows, [`is_root`] and [`signed`] look it
//! up here: a root itself, or signed by one (found by name, the signature
//! checked with its key), as ESP-IDF's certificate bundle does. The built-in
//! list is asked first; the card's is read only where it has no answer, and
//! its signature is checked, as the file is read, once for all the
//! certificates of a chain (a [`Scope`] of one step of the main loop). No
//! file, or one that is not signed by the plugin key, means the built-in
//! list alone: a server under another root fails as untrusted.
//!
//! Name constraints and path-length limits of roots are not applied:
//! mbedTLS is given no roots (see `tls::Contexts::no_roots`), so it never
//! sees them, and a root here only vouches for the certificate it signed.
//! (None of Mozilla's TLS roots has a name constraint at present, and a few
//! carry path-length limits. To apply them, the roots would have to be
//! given to `mbedtls_ssl_conf_ca_chain`, which costs memory for all of
//! them or a loader for the one found.)

use alloc::vec::Vec;

use core::cell::Cell;
use mbedtls_rs::sys::{
    mbedtls_md, mbedtls_md_get_size, mbedtls_md_info_from_type, mbedtls_pk_context,
    mbedtls_pk_free, mbedtls_pk_init, mbedtls_pk_parse_public_key, mbedtls_pk_verify_ext,
    mbedtls_x509_crt,
};

use paperpaw_platform::Storage;
use paperpaw_x4::tls_roots::{Roots, Source};

/// The roots built in.
static BUILTIN: &[u8] = include_bytes!("../roots-builtin.bin");
/// Where the data plugin `tls-roots` keeps Mozilla's whole list.
const PACK: &str = "plugins/tls-roots/roots.pack";
/// The public key PaperPaw's own plugins are signed with
/// (`plugins/store/src/native_item.rs`, `FIRST_PARTY_KEY`; a test of
/// `paperpaw_x4::tls_roots` holds the two alike).
const PLUGIN_KEY: [u8; 32] = paperpaw_x4::tls_roots::PLUGIN_KEY;

/// What the card's list is while a step of the main loop runs: the storage to
/// read it from, and whether it was found sound (`None` before it was
/// looked at).
struct Card {
    storage: Cell<Option<*const (dyn Storage + 'static)>>,
    sound: Cell<Option<bool>>,
}

// SAFETY: the firmware's main loop is the only code that uses it, and no
// interrupt handler does.
unsafe impl Sync for Card {}

static CARD: Card = Card {
    storage: Cell::new(None),
    sound: Cell::new(None),
};

/// While it lives, TLS can read the list on the card from `storage`: a step
/// of the network (`Network::step`) holds one, for the handshakes it moves
/// on. Without one, only the built-in roots count.
pub struct Scope;

impl Scope {
    pub fn enter(storage: &dyn Storage) -> Self {
        // SAFETY: the pointer is cleared when the scope ends, which is
        // before `storage` can be dropped; nothing keeps it longer.
        let storage: &'static dyn Storage = unsafe { core::mem::transmute(storage) };
        CARD.storage.set(Some(storage));
        CARD.sound.set(None);
        Scope
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        CARD.storage.set(None);
        CARD.sound.set(None);
    }
}

/// The file on the card, as a source of bytes.
struct File(&'static dyn Storage);

impl core::fmt::Debug for File {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("File")
    }
}

impl Source for File {
    fn size(&self) -> u64 {
        self.0.size(PACK).unwrap_or(0)
    }

    fn read_exact_at(&self, mut offset: u64, mut buf: &mut [u8]) -> bool {
        while !buf.is_empty() {
            match self.0.read_at(PACK, offset, buf) {
                Ok(n) if n > 0 => {
                    offset += n as u64;
                    buf = &mut buf[n..];
                }
                _ => return false,
            }
        }
        true
    }
}

/// The list on the card where there is one, signed by the plugin key. The
/// signature is checked at the first use in a [`Scope`] only: a handshake
/// looks up a root twice or more, each check reads and hashes the whole file.
fn card() -> Option<Roots<File>> {
    // SAFETY: set by a live `Scope` (see there).
    let storage: &'static dyn Storage = unsafe { &*CARD.storage.get()? };
    match CARD.sound.get() {
        Some(false) => None,
        Some(true) => Roots::open_signed_again(File(storage)),
        None => {
            let roots = Roots::open_signed(File(storage), &PLUGIN_KEY);
            CARD.sound.set(Some(roots.is_some()));
            roots
        }
    }
}

/// Whether a root with this name and key is in the built-in list or on the card.
fn known(name: &[u8], key: &[u8]) -> bool {
    Roots::open(BUILTIN).is_some_and(|roots| roots.has(name, key))
        || card().is_some_and(|roots| roots.has(name, key))
}

/// Whether a root named `name` has a key for which `check` says `true`.
fn any_key(name: &[u8], mut check: impl FnMut(&[u8]) -> bool) -> bool {
    Roots::open(BUILTIN).is_some_and(|roots| roots.any_key(name, &mut check))
        || card().is_some_and(|roots| roots.any_key(name, &mut check))
}

/// The contents of a DER `SEQUENCE` (its tag and length taken off), if
/// `der` is exactly one.
fn sequence_contents(der: &[u8]) -> Option<&[u8]> {
    let (&tag, rest) = der.split_first()?;
    if tag != 0x30 {
        return None;
    }
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let bytes = usize::from(first & 0x7f);
        if bytes == 0 || bytes > 4 || rest.len() < bytes {
            return None;
        }
        let (len, rest) = rest.split_at(bytes);
        (
            len.iter().fold(0usize, |n, &b| n << 8 | usize::from(b)),
            rest,
        )
    };
    (rest.len() == len).then_some(rest)
}

/// `contents` as a DER `SEQUENCE`.
fn sequence(contents: &[u8]) -> Vec<u8> {
    let len = contents.len();
    let mut der = Vec::with_capacity(len + 4);
    der.push(0x30);
    if len < 0x80 {
        der.push(len as u8);
    } else if len < 0x100 {
        der.extend_from_slice(&[0x81, len as u8]);
    } else {
        der.extend_from_slice(&[0x82, (len >> 8) as u8, len as u8]);
    }
    der.extend_from_slice(contents);
    der
}

/// The bytes of an mbedTLS buffer.
///
/// # Safety
/// `p` and `len` must describe memory that lives as long as `'a`.
unsafe fn bytes<'a>(p: *const u8, len: usize) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        // SAFETY: as promised by the caller.
        unsafe { core::slice::from_raw_parts(p, len) }
    }
}

/// Whether `crt` is one of the roots itself: its name and key are a
/// root's (whoever else signed it).
///
/// # Safety
/// As for [`signed`].
pub unsafe fn is_root(crt: &mbedtls_x509_crt) -> bool {
    // SAFETY: mbedTLS's buffers of the certificate, alive while it is.
    let (subject, key) = unsafe {
        (
            bytes(crt.subject_raw.p, crt.subject_raw.len),
            bytes(crt.pk_raw.p, crt.pk_raw.len),
        )
    };
    let (Some(subject), Some(key)) = (sequence_contents(subject), sequence_contents(key)) else {
        return false;
    };
    known(subject, key)
}

/// Whether `crt` is signed by one of the roots: one named as its issuer,
/// whose key checks its signature.
///
/// # Safety
/// `crt` must be a certificate mbedTLS parsed (as given to a verify
/// callback).
pub unsafe fn signed(crt: &mbedtls_x509_crt) -> bool {
    // SAFETY: mbedTLS's buffers of the certificate, alive while it is.
    let (issuer, tbs, signature) = unsafe {
        (
            bytes(crt.issuer_raw.p, crt.issuer_raw.len),
            bytes(crt.tbs.p, crt.tbs.len),
            bytes(crt.private_sig.p, crt.private_sig.len),
        )
    };
    let Some(issuer) = sequence_contents(issuer) else {
        return false;
    };
    // SAFETY: a lookup in mbedTLS's static table.
    let md = unsafe { mbedtls_md_info_from_type(crt.private_sig_md) };
    if md.is_null() {
        return false;
    }
    let mut hash = [0u8; 64];
    // SAFETY: `md` is valid; the hash fits (64 bytes is SHA-512's size).
    let hash_len = usize::from(unsafe { mbedtls_md_get_size(md) });
    if hash_len > hash.len()
        || unsafe { mbedtls_md(md, tbs.as_ptr(), tbs.len(), hash.as_mut_ptr()) } != 0
    {
        return false;
    }
    any_key(issuer, |root_key| {
        {
            let key = sequence(root_key);
            // SAFETY: an mbedTLS key context, set up and freed here.
            unsafe {
                let mut pk: mbedtls_pk_context = core::mem::zeroed();
                mbedtls_pk_init(&mut pk);
                let verified = mbedtls_pk_parse_public_key(&mut pk, key.as_ptr(), key.len()) == 0
                    && mbedtls_pk_verify_ext(
                        crt.private_sig_pk,
                        crt.private_sig_opts,
                        &mut pk,
                        crt.private_sig_md,
                        hash.as_ptr(),
                        hash_len,
                        signature.as_ptr(),
                        signature.len(),
                    ) == 0;
                mbedtls_pk_free(&mut pk);
                verified
            }
        }
    })
}
