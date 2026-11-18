//! The key of the address, drawn anew each time serving starts: the rule of
//! the SDK's `served::key` (letters a person reads off the screen and types
//! without taking one for another, at least 64 bits), host-free. The bytes come
//! from a `draw` the plugin backs with `core.secrets` `random`; a draw that is
//! not fully random (the X4's generator before the radio is on) gives no key,
//! never a weaker one.
use alloc::string::String;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::random::{READABLE, readable_letters};
use paperpaw_ui_kit::mem::{buffer, wipe, wipe_text};

/// Letters of a key (`served::KEY_LETTERS`).
pub const LETTERS: usize = readable_letters(64);
/// Draws of the source before a text that is not whole is given up.
const DRAWS: usize = 16;

/// Fills a buffer (1 to 256 bytes) with random bytes and says whether the
/// source was fully random.
pub type Draw<'a> = dyn FnMut(&mut [u8]) -> Result<bool, Error> + 'a;

/// A key of [`LETTERS`] letters, each as likely as any other (bytes that would
/// favour a letter are drawn again); `None` where a draw was not fully random or
/// sixteen draws gave too few usable bytes. The bytes are wiped.
///
/// # Errors
/// `NoMemory`, or what `draw` says (`Unsupported` where there is no source).
pub fn key(draw: &mut Draw<'_>) -> Result<Option<String>, Error> {
    letters(draw, LETTERS)
}

/// `count` letters of the same kind (the password of the reader's own network
/// is 15 of them), at most [`LETTERS`] times two.
///
/// # Errors
/// As [`key`].
pub fn letters(draw: &mut Draw<'_>, count: usize) -> Result<Option<String>, Error> {
    let count = count.min(LETTERS * 2);
    let letters = READABLE.as_bytes();
    // Bytes below the last whole multiple of the letters, so that each letter
    // is as likely.
    let below = 256 - 256 % letters.len();
    let mut out = String::new();
    out.try_reserve_exact(count).map_err(|_| Error::NoMemory)?;
    let mut bytes = buffer(count * 2)?;
    let mut strong = true;
    let mut failed = None;
    for _ in 0..DRAWS {
        if out.len() == count {
            break;
        }
        match draw(&mut bytes) {
            Ok(this) => strong &= this,
            Err(error) => {
                failed = Some(error);
                break;
            }
        }
        for &byte in &bytes {
            if out.len() < count && usize::from(byte) < below {
                out.push(char::from(letters[usize::from(byte) % letters.len()]));
            }
        }
    }
    wipe(&mut bytes);
    if let Some(error) = failed {
        wipe_text(out);
        return Err(error);
    }
    if strong && out.len() == count {
        Ok(Some(out))
    } else {
        wipe_text(out);
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use paperpaw_sdk::random::Random;

    static COUNT: AtomicUsize = AtomicUsize::new(0);
    /// The tests share `COUNT`: one draws at a time (they run in parallel).
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn byte(i: usize) -> u8 {
        (i as u8).wrapping_mul(97).wrapping_add(13)
    }

    /// The SDK's draw and this one, given the same bytes, give the same key.
    #[test]
    fn the_key_is_the_one_the_sdk_draws_from_the_same_bytes() {
        let random = Random::new(|buf| {
            for slot in buf.iter_mut() {
                *slot = byte(COUNT.fetch_add(1, Ordering::SeqCst));
            }
            true
        });
        let _one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        COUNT.store(0, Ordering::SeqCst);
        let sdk = paperpaw_sdk::served::key(&random).unwrap();
        let mut at = 0;
        let mut draw = |buf: &mut [u8]| {
            for slot in buf.iter_mut() {
                *slot = byte(at);
                at += 1;
            }
            Ok(true)
        };
        let native = key(&mut draw).unwrap().unwrap();
        assert_eq!(native, sdk);
        assert_eq!(native.len(), paperpaw_sdk::served::KEY_LETTERS);
        assert!(native.chars().all(|c| READABLE.contains(c)));
    }

    /// The password of the own network is the SDK's `readable(15)` from the same bytes.
    #[test]
    fn the_password_is_the_one_the_sdk_draws_from_the_same_bytes() {
        let random = Random::new(|buf| {
            for slot in buf.iter_mut() {
                *slot = byte(COUNT.fetch_add(1, Ordering::SeqCst));
            }
            true
        });
        let _one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        COUNT.store(0, Ordering::SeqCst);
        let sdk = random.readable(15).unwrap();
        let mut at = 0;
        let mut draw = |buf: &mut [u8]| {
            for slot in buf.iter_mut() {
                *slot = byte(at);
                at += 1;
            }
            Ok(true)
        };
        let native = letters(&mut draw, 15).unwrap().unwrap();
        assert_eq!((native.len(), native.as_str()), (15, sdk.as_str()));
    }

    #[test]
    fn a_draw_that_is_not_fully_random_gives_no_key() {
        let mut weak = |buf: &mut [u8]| {
            buf.fill(7);
            Ok(false)
        };
        assert_eq!(key(&mut weak), Ok(None));
    }

    #[test]
    fn a_source_of_unusable_bytes_gives_no_key_and_none_is_short() {
        // 255 is above the last whole multiple of 18 (252): never taken.
        let mut high = |buf: &mut [u8]| {
            buf.fill(255);
            Ok(true)
        };
        assert_eq!(key(&mut high), Ok(None));
    }

    #[test]
    fn a_missing_source_is_the_error_it_gives() {
        let mut none = |_: &mut [u8]| Err(Error::Unsupported);
        assert_eq!(key(&mut none), Err(Error::Unsupported));
    }

    #[test]
    fn every_draw_is_asked_for_twice_as_many_bytes_as_letters() {
        let mut asked = 0;
        let mut draw = |buf: &mut [u8]| {
            asked = buf.len();
            buf.fill(1);
            Ok(true)
        };
        assert!(key(&mut draw).unwrap().is_some());
        assert_eq!(asked, 2 * LETTERS);
    }
}
