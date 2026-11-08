//! SHA-256, for checking that files came whole: native plugin files
//! (`paperpaw-native`) and what the plugin library lists by it. Signatures
//! are Ed25519's, with its own digest.
//!
//! Our own rather than RustCrypto's `sha2`, which the tests check it
//! against: built as the X4's firmware is (optimised for size, with fat
//! LTO, which a package's own `opt-level` does not get through), `sha2`
//! took the X4 2.7 times as long to place a 62 KB plugin file (1.23 s
//! against 0.45 s, hashing it as it is written) and 10 KB more flash.

use alloc::string::String;

/// The first 32 bits of the fractional parts of the cube roots of the
/// first 64 primes (FIPS 180-4, 4.2.2).
const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// A SHA-256 digest computed piece by piece.
#[derive(Debug, Clone)]
pub struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    used: usize,
    length: u64,
}

crate::wipe::wiped_on_drop!(Sha256, 8);

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub const fn new() -> Self {
        Self {
            state: [
                0x6a09_e667,
                0xbb67_ae85,
                0x3c6e_f372,
                0xa54f_f53a,
                0x510e_527f,
                0x9b05_688c,
                0x1f83_d9ab,
                0x5be0_cd19,
            ],
            block: [0; 64],
            used: 0,
            length: 0,
        }
    }

    /// Adds `data`.
    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        while !data.is_empty() {
            let take = (64 - self.used).min(data.len());
            self.block[self.used..self.used + take].copy_from_slice(&data[..take]);
            self.used += take;
            data = &data[take..];
            if self.used == 64 {
                let block = self.block;
                self.compress(&block);
                self.used = 0;
            }
        }
    }

    /// The digest of everything added.
    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.length.wrapping_mul(8);
        self.update(&[0x80]);
        while self.used != 56 {
            self.update(&[0]);
        }
        // The length goes in without counting itself.
        self.block[56..].copy_from_slice(&bits.to_be_bytes());
        let block = self.block;
        self.compress(&block);
        let mut out = [0u8; 32];
        for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    /// The digest as 64 lower-case hex digits.
    pub fn hex(self) -> String {
        crate::hex::encode(&self.finish())
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for (&k, &word) in K.iter().zip(&w) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(k)
                .wrapping_add(word);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (s, v) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }
}

/// The SHA-256 of `data`.
pub fn digest(data: &[u8]) -> [u8; 32] {
    let mut sha = Sha256::new();
    sha.update(data);
    sha.finish()
}

/// The SHA-256 of `data` as hex.
pub fn hex(data: &[u8]) -> String {
    crate::hex::encode(&digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `Drop` runs leaves no trace of what was hashed.
    #[test]
    fn the_state_is_wiped() {
        let mut digest = Sha256::new();
        digest
            .update(b"a password, and more than a block of it to fill the state: ................");
        assert!(digest.length != 0 && digest.state.iter().any(|w| *w != 0));
        digest.wipe();
        assert!(digest.state.iter().all(|w| *w == 0));
        assert!(digest.block.iter().all(|b| *b == 0));
        assert_eq!((digest.used, digest.length), (0, 0));
    }

    #[test]
    fn known_digests() {
        // FIPS 180-4's examples and the empty message.
        assert_eq!(
            hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn pieces_make_the_same_digest() {
        let data = alloc::vec![b'a'; 1_000_000];
        let mut sha = Sha256::new();
        for chunk in data.chunks(997) {
            sha.update(chunk);
        }
        // A million times "a" (FIPS 180-4's long example).
        assert_eq!(
            sha.hex(),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
        assert_eq!(hex(&data[..1000]), hex(&alloc::vec![b'a'; 1000]));
    }

    #[test]
    fn the_longer_example() {
        // FIPS 180-4's two-block example (896 bits).
        let message = b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmn\
            hijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu";
        assert_eq!(message.len(), 112);
        assert_eq!(
            hex(message),
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1"
        );
    }

    /// RustCrypto's digest of `data`.
    fn theirs(data: &[u8]) -> [u8; 32] {
        use sha2::Digest;
        sha2::Sha256::digest(data).into()
    }

    /// Bytes that look random, the same every run.
    fn bytes(len: usize, mut seed: u64) -> alloc::vec::Vec<u8> {
        (0..len)
            .map(|_| {
                seed = seed
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                (seed >> 56) as u8
            })
            .collect()
    }

    #[test]
    fn every_length_as_rustcryptos() {
        // Each length up to some blocks: the padding's every case (a
        // block's last 9 bytes, a block more), whole and in pieces.
        let data = bytes(1100, 1);
        for len in 0..=data.len() {
            let message = &data[..len];
            assert_eq!(digest(message), theirs(message), "{len} bytes");
            let mut sha = Sha256::new();
            for piece in message.chunks(len % 67 + 1) {
                sha.update(piece);
            }
            assert_eq!(sha.finish(), theirs(message), "{len} bytes in pieces");
        }
    }

    #[test]
    fn long_messages_as_rustcryptos() {
        // As NIST's long messages: some kilobytes to a megabyte, fed in
        // pieces of every size.
        for (len, seed) in [(4_097, 2), (65_536, 3), (100_003, 4), (1 << 20, 5)] {
            let data = bytes(len, seed);
            let mut sha = Sha256::new();
            let mut at = 0;
            let mut piece = 1;
            while at < data.len() {
                let end = (at + piece).min(data.len());
                sha.update(&data[at..end]);
                at = end;
                piece = piece * 7 % 1021 + 1;
            }
            assert_eq!(sha.finish(), theirs(&data), "{len} bytes");
        }
    }
}
