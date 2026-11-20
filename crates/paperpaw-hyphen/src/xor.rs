//! A xor filter (Graf and Lemire) of the keys of a table's patterns: which
//! keys the table may have, in a few bits each (#405). `compile` builds it and
//! writes it after the exceptions; the card reader keeps it instead of making
//! a Bloom filter of eight bits a pattern while it opens the table. A key
//! that is a pattern is always found; one that is not is found by chance, one
//! in `2^bits` (about 1 in 16 at four bits: 5.0 bits a pattern kept against
//! the Bloom filter's 8).
//!
//! The trailer after the exceptions: `PPXF`, version 1, the bits of a
//! fingerprint (4 to 8), the seed of the hash (4 bytes), the entries of each
//! of the three segments (4 bytes), then `3 * entries` fingerprints of `bits`
//! bits each, packed from the low bit of the first byte.

use alloc::vec::Vec;

/// Starts the trailer.
pub(crate) const MAGIC: &[u8; 4] = b"PPXF";
/// Magic, version, bits, seed and entries of a segment.
pub(crate) const HEADER: usize = 14;
/// The most fingerprint bytes a table may bring: the filter of 8,192 patterns
/// (the most a table with a filter has) takes at most 10 KB.
#[cfg(any(feature = "kernel", test))]
pub(crate) const MAX_DATA: usize = 12 * 1024;
/// The most keys (patterns) a table with a filter has: 512 blocks of 16.
pub(crate) const MAX_KEYS: usize = 8_192;
/// Bits of a fingerprint `compile` writes.
pub(crate) const BITS: u8 = 5;

/// What the header of a trailer says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) bits: u8,
    pub(crate) seed: u32,
    pub(crate) segment: u32,
}

impl Header {
    #[cfg(any(feature = "kernel", test))]
    /// The header in the first [`HEADER`] bytes of `bytes`, if it is one a
    /// reader accepts: the bytes of fingerprints it asks for follow, and are
    /// at most [`MAX_DATA`].
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        let head = bytes.get(..HEADER)?;
        if head[..4] != MAGIC[..] || head[4] != 1 || !(4..=8).contains(&head[5]) {
            return None;
        }
        let word =
            |at: usize| u32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
        let this = Self {
            bits: head[5],
            seed: word(6),
            segment: word(10),
        };
        (this.segment > 0 && this.data_len().is_some_and(|len| len <= MAX_DATA)).then_some(this)
    }

    /// Bytes of fingerprints, `None` where they would not fit a `usize`.
    pub(crate) fn data_len(&self) -> Option<usize> {
        let entries = (self.segment as usize).checked_mul(3)?;
        Some(entries.checked_mul(usize::from(self.bits))?.div_ceil(8))
    }

    pub(crate) fn write(&self, to: &mut Vec<u8>) {
        to.extend_from_slice(MAGIC);
        to.extend_from_slice(&[1, self.bits]);
        to.extend_from_slice(&self.seed.to_le_bytes());
        to.extend_from_slice(&self.segment.to_le_bytes());
    }
}

/// FNV-1a of `key`, started from `seed`.
fn hash(key: &[u8], seed: u32) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ u64::from(seed).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    for &byte in key {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// A 32-bit mix (what the three places and the fingerprint are made of; no
/// 64-bit arithmetic past the hash, which the small devices do in software).
const fn mix(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ x >> 16
}

/// The three places of a hash, one in each segment of `segment` entries.
fn places(hash: u64, segment: u32) -> [usize; 3] {
    let (a, b) = (hash as u32, (hash >> 32) as u32);
    let at = |i: u32, x: u32| (i * segment + mix(x) % segment) as usize;
    [
        at(0, a),
        at(1, b ^ 0x9e37_79b9),
        at(2, a.rotate_left(13) ^ b),
    ]
}

/// The fingerprint of a hash, of `bits` bits.
fn fingerprint(hash: u64, bits: u8) -> u8 {
    let (a, b) = (hash as u32, (hash >> 32) as u32);
    (mix(a ^ b.rotate_left(7) ^ 0x85eb_ca6b) >> 24) as u8 & (0xff >> (8 - bits))
}

#[cfg(any(feature = "kernel", test))]
/// Entry `i` of the packed fingerprints (0 past their end).
fn entry(data: &[u8], bits: u8, i: usize) -> u8 {
    let at = i * usize::from(bits);
    let low = u16::from(data.get(at / 8).copied().unwrap_or(0));
    let high = u16::from(data.get(at / 8 + 1).copied().unwrap_or(0));
    ((low | high << 8) >> (at % 8)) as u8 & (0xff >> (8 - bits))
}

#[cfg(any(feature = "kernel", test))]
/// Whether `key` may be one of the keys the filter was made of: always when
/// it is, rarely when it is not.
pub(crate) fn contains(data: &[u8], header: &Header, key: &[u8]) -> bool {
    let hash = hash(key, header.seed);
    let [a, b, c] = places(hash, header.segment);
    let bits = header.bits;
    entry(data, bits, a) ^ entry(data, bits, b) ^ entry(data, bits, c) == fingerprint(hash, bits)
}

/// The filter of `keys` (all different), as the header and the packed
/// fingerprints; `None` for no keys, or where no seed made it (then a table
/// has no filter written and the reader makes its own).
pub(crate) fn build(keys: &[&[u8]], bits: u8) -> Option<(Header, Vec<u8>)> {
    if keys.is_empty() {
        return None;
    }
    let segment = (keys.len() * 123 / 100 + 32).div_ceil(3);
    let total = 3 * segment;
    for seed in 0..64 {
        let hashes: Vec<u64> = keys.iter().map(|key| hash(key, seed)).collect();
        let mut count = alloc::vec![0u32; total];
        let mut mask = alloc::vec![0u64; total];
        for &h in &hashes {
            for place in places(h, segment as u32) {
                count[place] += 1;
                mask[place] ^= h;
            }
        }
        let mut queue: Vec<usize> = (0..total).filter(|&i| count[i] == 1).collect();
        let mut peeled: Vec<(u64, usize)> = Vec::with_capacity(keys.len());
        while let Some(i) = queue.pop() {
            if count[i] != 1 {
                continue;
            }
            let h = mask[i];
            peeled.push((h, i));
            for place in places(h, segment as u32) {
                count[place] -= 1;
                mask[place] ^= h;
                if count[place] == 1 {
                    queue.push(place);
                }
            }
        }
        if peeled.len() != keys.len() {
            continue;
        }
        let mut values = alloc::vec![0u8; total];
        for &(h, i) in peeled.iter().rev() {
            let [a, b, c] = places(h, segment as u32);
            values[i] = fingerprint(h, bits) ^ values[a] ^ values[b] ^ values[c];
        }
        let header = Header {
            bits,
            seed,
            segment: segment as u32,
        };
        let mut packed = alloc::vec![0u8; header.data_len()?];
        for (i, &value) in values.iter().enumerate() {
            let at = i * usize::from(bits);
            let wide = u16::from(value) << (at % 8);
            packed[at / 8] |= wide as u8;
            if let Some(next) = packed.get_mut(at / 8 + 1) {
                *next |= (wide >> 8) as u8;
            }
        }
        return Some((header, packed));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::String;

    fn keys(n: usize) -> Vec<String> {
        (0..n)
            .map(|i| format!("k{i:x}é{}", i * 7919 % 1000))
            .collect()
    }

    #[test]
    fn every_key_is_found_and_few_others() {
        for (n, bits) in [(1, 4), (5, 4), (169, 4), (4_944, 4), (4_944, 5), (8_192, 8)] {
            let owned = keys(n);
            let refs: Vec<&[u8]> = owned.iter().map(String::as_bytes).collect();
            let (header, data) = build(&refs, bits).expect("a filter");
            assert_eq!(data.len(), header.data_len().unwrap());
            assert!(refs.iter().all(|key| contains(&data, &header, key)), "{n}");
            let others = 20_000;
            let wrong = (0..others)
                .filter(|i| contains(&data, &header, format!("zz{i}x").as_bytes()))
                .count();
            // One in 2^bits, within a factor of two.
            let expected = others >> bits;
            assert!(
                wrong < expected * 2 + 20,
                "{n} keys, {bits} bits: {wrong} of {others}"
            );
        }
    }

    #[test]
    fn the_header_round_trips_and_a_bad_one_is_refused() {
        let header = Header {
            bits: 4,
            seed: 7,
            segment: 100,
        };
        let mut bytes = Vec::new();
        header.write(&mut bytes);
        assert_eq!(bytes.len(), HEADER);
        assert_eq!(Header::parse(&bytes), Some(header));
        for (at, value) in [(0, b'X'), (4, 2), (5, 3), (5, 9)] {
            let mut bad = bytes.clone();
            bad[at] = value;
            assert_eq!(Header::parse(&bad), None, "{at}");
        }
        let huge = Header {
            segment: u32::MAX,
            ..header
        };
        let mut bytes = Vec::new();
        huge.write(&mut bytes);
        assert_eq!(Header::parse(&bytes), None);
        assert_eq!(build(&[], 4), None);
    }
}
