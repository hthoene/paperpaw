//! Random bytes from the platform's own source, for what others must not
//! guess: keys in addresses, tokens, nonces (`Env::random`). What a
//! person reads off the screen and types (a password, a key in an
//! address) is drawn from letters not taken for one another
//! ([`Random::readable`], [`READABLE`]) and shown in groups of five
//! ([`grouped`]).
//!
//! Each shell gives the kernel its source
//! (`KernelBuilder::random`): the operating system's on Android and on
//! desktops, the chip's random number generator on the X4. That one is
//! fully random while the radio is on; a key for the network is best
//! drawn once the network is up (e.g. when a server has started). Where a
//! shell gives none (tests), nothing is drawn: there is no weaker stand-in
//! to fall back to unnoticed.
//!

use alloc::string::String;
use core::fmt;

/// The letters of [`Random::token`]: digits and lower case letters
/// without `i`, `l`, `o` and `u`, which are easily taken for others
/// (Crockford's base 32).
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// The letters of [`Random::readable`]: lower case letters and digits a
/// person reads off a screen and types without taking one for another,
/// on paper as on a phone's keyboard: without 0 and o, 1, l, i and j, 2
/// and z, 5 and s, 6 and b, 8, 9, g and q, u and v.
pub const READABLE: &str = "acdefhkmnprtwxy347";

/// How many letters of [`READABLE`] hold at least `bits` bits (each
/// about 4.17, the logarithm of 18).
pub const fn readable_letters(bits: u32) -> usize {
    // 41 699 / 10 000 bits a letter, a little less than they hold.
    (bits as usize * 10_000).div_ceil(41_699)
}

/// `code` as it is shown to be read and typed: in groups of five letters,
/// a space between them ("acdef hkmnp rt").
pub fn grouped(code: &str) -> String {
    let mut out = String::with_capacity(code.len() + code.len() / 5);
    for (i, c) in code.chars().enumerate() {
        if i > 0 && i % 5 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// The platform's source of random bytes; see the [module
/// documentation](self).
#[derive(Clone, Copy, Default)]
pub struct Random {
    fill: Option<fn(&mut [u8]) -> bool>,
}

impl Random {
    /// No source: nothing is drawn.
    pub const NONE: Self = Self { fill: None };

    /// The source `fill`, which fills a buffer with random bytes and says
    /// whether it could (for shells).
    pub const fn new(fill: fn(&mut [u8]) -> bool) -> Self {
        Self { fill: Some(fill) }
    }

    /// Whether there is a source.
    pub fn available(&self) -> bool {
        self.fill.is_some()
    }

    /// Fills `buf` with random bytes; `false` where there is no source or
    /// it failed, and `buf` is not to be used then.
    pub fn fill(&self, buf: &mut [u8]) -> bool {
        self.fill.is_some_and(|fill| fill(buf))
    }

    /// A random text of `chars` digits and lower case letters, 5 bits
    /// each (so 13 for at least 64 bits), easy to read and type; `None`
    /// where nothing can be drawn.
    pub fn token(&self, chars: usize) -> Option<String> {
        let mut bytes = alloc::vec![0u8; (chars * 5).div_ceil(8)];
        if !self.fill(&mut bytes) {
            return None;
        }
        let token = (0..chars)
            .map(|i| {
                // Bits 5i to 5i + 4, the first byte's highest first.
                let at = i * 5;
                let pair = u16::from(bytes[at / 8]) << 8
                    | u16::from(bytes.get(at / 8 + 1).copied().unwrap_or(0));
                let index = (pair >> (11 - at % 8)) & 0x1f;
                char::from(ALPHABET[usize::from(index)])
            })
            .collect();
        Some(token)
    }

    /// A random text of `chars` letters of [`READABLE`], for what a
    /// person reads off the screen and types (each about 4.17 bits, see
    /// [`readable_letters`]); `None` where nothing can be drawn. Every
    /// letter is as likely as any other.
    pub fn readable(&self, chars: usize) -> Option<String> {
        let letters = READABLE.as_bytes();
        let count = letters.len();
        // Bytes below the last whole multiple of the letters, so that
        // each letter is as likely; the others are drawn again.
        let below = 256 - 256 % count;
        let mut out = String::with_capacity(chars);
        let mut bytes = alloc::vec![0u8; chars.max(1) * 2];
        // A source that never gives a usable byte gives no text rather
        // than a short one.
        for _ in 0..16 {
            if out.len() == chars {
                break;
            }
            if !self.fill(&mut bytes) {
                return None;
            }
            for &byte in &bytes {
                if out.len() == chars {
                    break;
                }
                if usize::from(byte) < below {
                    out.push(char::from(letters[usize::from(byte) % count]));
                }
            }
        }
        (out.len() == chars).then_some(out)
    }
}

impl fmt::Debug for Random {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Random")
            .field("available", &self.available())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_take_five_bits_a_letter() {
        let ones = Random::new(|buf| {
            buf.fill(0xff);
            true
        });
        assert_eq!(ones.token(13).as_deref(), Some("zzzzzzzzzzzzz"));
        // 0b00001_00010_00011_00100_00101_00110_00111_01000 …
        let counting = Random::new(|buf| {
            let bits = [0x08, 0x86, 0x42, 0x98, 0xe8];
            buf.copy_from_slice(&bits[..buf.len()]);
            true
        });
        assert_eq!(counting.token(8).as_deref(), Some("12345678"));
        assert_eq!(counting.token(1).as_deref(), Some("1"));
        assert_eq!(counting.token(0).as_deref(), Some(""));
    }

    /// Readable texts have only letters of the set, each as likely; a
    /// source that never gives a usable byte gives none.
    #[test]
    fn readable_texts_keep_to_their_letters() {
        assert_eq!(READABLE.len(), 18);
        for c in "0o1lij2z5s6b89gquv".chars() {
            assert!(!READABLE.contains(c), "{c}");
        }
        let counting = Random::new(|buf| {
            for (i, b) in buf.iter_mut().enumerate() {
                *b = i as u8;
            }
            true
        });
        let text = counting.readable(252).unwrap();
        assert!(text.chars().all(|c| READABLE.contains(c)));
        // Bytes 0 to 251 once each: every letter 14 times.
        for c in READABLE.chars() {
            assert_eq!(text.chars().filter(|t| *t == c).count(), 14, "{c}");
        }
        let high = Random::new(|buf| {
            buf.fill(0xff);
            true
        });
        assert_eq!(high.readable(10), None);
        assert_eq!(Random::NONE.readable(10), None);
        assert_eq!(counting.readable(0).as_deref(), Some(""));
        // 45 bits in 11 letters, 64 in 16.
        assert_eq!(readable_letters(45), 11);
        assert_eq!(readable_letters(64), 16);
        assert_eq!(grouped("acdefhkmnprt"), "acdef hkmnp rt");
        assert_eq!(grouped("acdef"), "acdef");
    }

    proptest::proptest! {
        /// Whatever the source gives, never a letter outside the set.
        #[test]
        fn readable_texts_never_leave_their_letters(seed: u64, chars in 0usize..40) {
            use core::sync::atomic::{AtomicU64, Ordering};
            static STATE: AtomicU64 = AtomicU64::new(0);
            STATE.store(seed | 1, Ordering::Relaxed);
            let random = Random::new(|buf| {
                for b in buf.iter_mut() {
                    // xorshift, enough to vary the bytes.
                    let mut x = STATE.load(Ordering::Relaxed);
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    STATE.store(x, Ordering::Relaxed);
                    *b = x as u8;
                }
                true
            });
            if let Some(text) = random.readable(chars) {
                proptest::prop_assert_eq!(text.chars().count(), chars);
                proptest::prop_assert!(text.chars().all(|c| READABLE.contains(c)));
            }
        }
    }

    #[test]
    fn nothing_is_drawn_without_a_source() {
        let mut buf = [0u8; 8];
        assert!(!Random::NONE.available());
        assert!(!Random::NONE.fill(&mut buf));
        assert_eq!(Random::NONE.token(13), None);
        let failing = Random::new(|_| false);
        assert!(failing.available());
        assert_eq!(failing.token(13), None);
    }
}
