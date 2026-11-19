//! The root certificates the X4's TLS trusts, as a file: a list of roots
//! (each one's name and public key, as `webpki-roots` keeps them) that is
//! looked up by name and never read whole (#387). The firmware has a small
//! list built in (`shells/xteink/roots-builtin.bin`, about twenty roots);
//! the full list of Mozilla's roots comes as a data plugin on the card
//! (`plugins/tls-roots/roots.pack`), signed with PaperPaw's own plugin key,
//! and is read only where a server's chain ends in a root the built-in list
//! does not have: a lookup by name takes a handful of reads of a few bytes
//! each, and the signature is checked piece by piece as the file is read,
//! so no more than a small buffer is ever held.
//!
//! | Bytes | What |
//! |-------|------|
//! | 0..4 | `PPRT` |
//! | 4..6 | the format, 1 |
//! | 6..8 | the number of roots, `n` |
//! | 8..12 | the length of all that precedes the signature, `data_len` |
//! | 12.. | `n` index entries of 12 bytes: the first 8 bytes of the SHA-256 of a root's name, and the offset of its record; sorted by the hash |
//! | then | the records: the name's length (2 bytes), the key's length (2 bytes), the name, the key |
//! | `data_len`.. | the signature (`paperpaw_native::signature`, 112 bytes), in a file from the card; the built-in list has none |
//!
//! A name is a root's subject and a key its `SubjectPublicKeyInfo`, both
//! without the `SEQUENCE` header they come in, as `webpki-roots` has them.

use alloc::vec::Vec;

use paperpaw_native::signature::{SIGNATURE_LEN, Signature};

/// The file's first bytes.
pub const MAGIC: [u8; 4] = *b"PPRT";
/// The format this reads and writes.
pub const FORMAT: u16 = 1;
/// The public key PaperPaw's own plugins are signed with, which the list on
/// the card must be signed by (the store's `FIRST_PARTY_KEY`).
pub const PLUGIN_KEY: [u8; 32] = [
    0x14, 0x8c, 0xcf, 0x3e, 0xbc, 0x30, 0x50, 0x6b, 0x35, 0x5c, 0x6b, 0x5c, 0xb2, 0xba, 0xf8, 0x7d,
    0x92, 0x65, 0x60, 0x6c, 0xc6, 0xa8, 0x1f, 0x08, 0x5b, 0xb3, 0xb9, 0x74, 0x0a, 0x95, 0xae, 0xbd,
];
const HEADER: usize = 12;
const ENTRY: usize = 12;
/// A name or key longer than this is not a certificate's.
const MAX_FIELD: usize = 1024;

/// Where a list's bytes are: memory (the built-in list) or a file.
pub trait Source {
    /// The number of bytes.
    fn size(&self) -> u64;
    /// Fills `buf` from `offset`; `false` where it cannot be.
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> bool;
}

impl Source for &[u8] {
    fn size(&self) -> u64 {
        <[u8]>::len(self) as u64
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> bool {
        usize::try_from(offset)
            .ok()
            .and_then(|at| self.get(at..at.checked_add(buf.len())?))
            .map(|from| buf.copy_from_slice(from))
            .is_some()
    }
}

/// A list of roots, opened.
#[derive(Debug)]
pub struct Roots<S> {
    source: S,
    count: u32,
}

/// The first 8 bytes of the SHA-256 of a name.
fn name_hash(name: &[u8]) -> [u8; 8] {
    let digest = paperpaw_digest::sha256::digest(name);
    let mut hash = [0; 8];
    hash.copy_from_slice(&digest[..8]);
    hash
}

fn u16_at(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

impl<S: Source> Roots<S> {
    /// The list in `source`, a file without a signature (the built-in
    /// list); `None` where it is none or is cut.
    pub fn open(source: S) -> Option<Self> {
        Self::open_part(source, false)
    }

    /// The list in `source`, a file the plugin key `key` signed: the
    /// signature is checked over the whole file, in pieces; `None` where it
    /// does not hold, the key is another, or the file is no list.
    pub fn open_signed(source: S, key: &[u8; 32]) -> Option<Self> {
        let roots = Self::open_part(source, true)?;
        let data_len = u64::from(u32_at(&roots.header()?, 8));
        let mut trailer = [0; SIGNATURE_LEN];
        if !roots.source.read_exact_at(data_len, &mut trailer) {
            return None;
        }
        let signature = Signature::read(&trailer)?;
        if signature.key != *key || u64::from(signature.signed_len) != data_len {
            return None;
        }
        let mut check = signature.check().ok()?;
        let mut piece = [0; 256];
        let mut at = 0;
        while at < data_len {
            let n = usize::try_from(data_len - at).map_or(piece.len(), |n| n.min(piece.len()));
            if !roots.source.read_exact_at(at, &mut piece[..n]) {
                return None;
            }
            check.absorb(&piece[..n]);
            at += n as u64;
        }
        check.finish().ok()?;
        Some(roots)
    }

    fn header(&self) -> Option<[u8; HEADER]> {
        let mut header = [0; HEADER];
        self.source.read_exact_at(0, &mut header).then_some(header)
    }

    fn open_part(source: S, signed: bool) -> Option<Self> {
        let mut header = [0; HEADER];
        if !source.read_exact_at(0, &mut header)
            || header[..4] != MAGIC
            || u16_at(&header, 4) != usize::from(FORMAT)
        {
            return None;
        }
        let count = u32::try_from(u16_at(&header, 6)).ok()?;
        let data_len = u64::from(u32_at(&header, 8));
        let whole = source.size();
        let fits = if signed {
            whole == data_len + SIGNATURE_LEN as u64
        } else {
            whole == data_len
        };
        (fits && data_len >= (HEADER + ENTRY * count as usize) as u64)
            .then_some(Self { source, count })
    }

    /// The index entry `i`: its hash and its record's offset.
    fn entry(&self, i: u32) -> Option<([u8; 8], u64)> {
        let mut entry = [0; ENTRY];
        if !self
            .source
            .read_exact_at((HEADER + ENTRY * i as usize) as u64, &mut entry)
        {
            return None;
        }
        let mut hash = [0; 8];
        hash.copy_from_slice(&entry[..8]);
        Some((hash, u64::from(u32_at(&entry, 8))))
    }

    /// Calls `found` with the key of each root named `name` (more than one
    /// where several keys share a name), until it says `true`.
    fn named(&self, name: &[u8], mut found: impl FnMut(u64, usize) -> bool) -> bool {
        let hash = name_hash(name);
        // The first entry not below `hash`.
        let (mut low, mut high) = (0, self.count);
        while low < high {
            let mid = low + (high - low) / 2;
            match self.entry(mid) {
                Some((at, _)) if at < hash => low = mid + 1,
                Some(_) => high = mid,
                None => return false,
            }
        }
        for i in low..self.count {
            let Some((at, offset)) = self.entry(i) else {
                return false;
            };
            if at != hash {
                break;
            }
            let mut lengths = [0; 4];
            if !self.source.read_exact_at(offset, &mut lengths) {
                return false;
            }
            let (name_len, key_len) = (u16_at(&lengths, 0), u16_at(&lengths, 2));
            if name_len == name.len()
                && key_len <= MAX_FIELD
                && self.same(offset + 4, name)
                && found(offset + 4 + name_len as u64, key_len)
            {
                return true;
            }
        }
        false
    }

    /// Whether the bytes at `offset` are `bytes`.
    fn same(&self, mut offset: u64, bytes: &[u8]) -> bool {
        let mut piece = [0; 64];
        for part in bytes.chunks(piece.len()) {
            let piece = &mut piece[..part.len()];
            if !self.source.read_exact_at(offset, piece) || piece != part {
                return false;
            }
            offset += part.len() as u64;
        }
        true
    }

    /// Whether the list has the root with this `name` and `key`.
    pub fn has(&self, name: &[u8], key: &[u8]) -> bool {
        self.named(name, |at, len| len == key.len() && self.same(at, key))
    }

    /// Calls `check` with the key of each root named `name`, until it says
    /// `true` (which this then says too). The key is read into memory for
    /// the call; where there is no memory for it, the root is passed over.
    pub fn any_key(&self, name: &[u8], mut check: impl FnMut(&[u8]) -> bool) -> bool {
        self.named(name, |at, len| {
            let mut key = Vec::new();
            if key.try_reserve_exact(len).is_err() {
                return false;
            }
            key.resize(len, 0);
            self.source.read_exact_at(at, &mut key) && check(&key)
        })
    }
}

/// A list of roots as `(name, key)` made into a file (without a signature;
/// see [`sign`]). Roots with a longer name or key than a certificate has
/// are left out.
#[must_use]
pub fn build(roots: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut roots: Vec<(&[u8], &[u8])> = roots
        .iter()
        .copied()
        .filter(|(name, key)| name.len() <= MAX_FIELD && key.len() <= MAX_FIELD)
        .collect();
    roots.sort_by_key(|(name, _)| name_hash(name));
    let count = u16::try_from(roots.len()).unwrap_or(u16::MAX);
    let mut index = Vec::new();
    let mut records = Vec::new();
    let records_at = HEADER + ENTRY * roots.len();
    for (name, key) in &roots {
        index.extend_from_slice(&name_hash(name));
        index.extend_from_slice(&((records_at + records.len()) as u32).to_le_bytes());
        records.extend_from_slice(&(name.len() as u16).to_le_bytes());
        records.extend_from_slice(&(key.len() as u16).to_le_bytes());
        records.extend_from_slice(name);
        records.extend_from_slice(key);
    }
    let mut file = Vec::with_capacity(records_at + records.len() + SIGNATURE_LEN);
    file.extend_from_slice(&MAGIC);
    file.extend_from_slice(&FORMAT.to_le_bytes());
    file.extend_from_slice(&count.to_le_bytes());
    file.extend_from_slice(&((records_at + records.len()) as u32).to_le_bytes());
    file.extend_from_slice(&index);
    file.extend_from_slice(&records);
    file
}

/// `list` (from [`build`]) with the signature of the plugin key made from
/// `seed` appended.
#[must_use]
pub fn sign(list: &[u8], seed: &[u8; 32]) -> Vec<u8> {
    let mut file = list.to_vec();
    file.extend_from_slice(&paperpaw_native::signature::sign(list, seed).to_bytes());
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: [u8; 32] = [7; 32];

    /// The key here is the store's: a list signed with the plugin key is read.
    #[test]
    fn the_plugin_key_is_the_stores() {
        let store = include_str!("../../../plugins/store/src/native_item.rs");
        let at = store.find("pub const FIRST_PARTY_KEY").unwrap();
        let bytes: Vec<u8> = store[at..]
            .lines()
            .skip(1)
            .take(2)
            .flat_map(|line| {
                line.split(',')
                    .filter_map(|b| u8::from_str_radix(b.trim().strip_prefix("0x")?, 16).ok())
            })
            .collect();
        assert_eq!(bytes, PLUGIN_KEY);
    }

    fn key() -> [u8; 32] {
        paperpaw_native::signature::public_key(&SEED)
    }

    fn list() -> Vec<u8> {
        build(&[
            (b"root one", b"key one"),
            (b"root two", &[2; 600]),
            (b"root two", b"another key"),
            (b"root three", b"key three"),
        ])
    }

    #[test]
    fn a_root_is_found_by_its_name_and_key() {
        let file = list();
        let roots = Roots::open(file.as_slice()).unwrap();
        assert!(roots.has(b"root one", b"key one"));
        assert!(roots.has(b"root three", b"key three"));
        assert!(roots.has(b"root two", &[2; 600]));
        assert!(roots.has(b"root two", b"another key"));
    }

    #[test]
    fn an_unknown_name_or_a_key_of_another_is_not_found() {
        let file = list();
        let roots = Roots::open(file.as_slice()).unwrap();
        assert!(!roots.has(b"root four", b"key one"));
        assert!(!roots.has(b"root one", b"key three"));
        assert!(!roots.has(b"root one", b"key on"));
        assert!(!roots.any_key(b"root four", |_| true));
    }

    #[test]
    fn the_keys_of_a_name_are_offered_until_one_is_taken() {
        let file = list();
        let roots = Roots::open(file.as_slice()).unwrap();
        let mut seen = Vec::new();
        assert!(roots.any_key(b"root two", |key| {
            seen.push(key.len());
            key == b"another key"
        }));
        seen.sort_unstable();
        assert!(seen.contains(&11));
        assert!(!roots.any_key(b"root one", |_| false));
    }

    #[test]
    fn a_signed_file_opens_with_the_key_that_signed_it_only() {
        let file = sign(&list(), &SEED);
        let roots = Roots::open_signed(file.as_slice(), &key()).unwrap();
        assert!(roots.has(b"root one", b"key one"));
        assert!(Roots::open_signed(file.as_slice(), &[9; 32]).is_none());
        // Without its signature it is not what the card must hold, and the
        // built-in list does not take one.
        assert!(Roots::open_signed(list().as_slice(), &key()).is_none());
        assert!(Roots::open(file.as_slice()).is_none());
    }

    #[test]
    fn a_changed_or_cut_file_does_not_open() {
        let file = sign(&list(), &SEED);
        for at in [0, 5, 13, file.len() / 2, file.len() - 200] {
            let mut changed = file.clone();
            changed[at] ^= 1;
            assert!(
                Roots::open_signed(changed.as_slice(), &key()).is_none(),
                "{at}"
            );
        }
        assert!(Roots::open_signed(&file[..file.len() - 1], &key()).is_none());
        assert!(Roots::open(&file[..5]).is_none());
        assert!(Roots::open(b"".as_slice()).is_none());
    }
}
