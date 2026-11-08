//! The screen lock's rules for the native plugin, without host calls: how the
//! code is kept and checked, the wrong tries and the wait after them, and what
//! the keypad and the pattern make of what is pressed. They are the interface-6
//! `code` module and the logic of the lock screen made fallible (a refused
//! allocation is `NoMemory`, never an abort) and free of host state, tested to
//! give the same answers as the old code. What is secret is wiped as soon as it
//! is done with.
use alloc::string::String;
use paperpaw_sdk::digest::sha1::Sha1;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    mem::wipe,
    text::{concat, copy, decimal_u64},
};

/// The kinds; the setting stores them after [`KIND_PREFIX`].
pub const KINDS: [&str; 4] = ["off", "pin", "pattern", "password"];
pub const KIND_PREFIX: &str = "lock.kind.";
/// The kinds as the setting stores them: [`KIND_PREFIX`] and the kind.
pub const KIND_VALUES: [&str; KINDS.len()] = [
    "lock.kind.off",
    "lock.kind.pin",
    "lock.kind.pattern",
    "lock.kind.password",
];
/// The shortest code.
pub const MIN_LENGTH: usize = 4;
/// Wrong tries before the next one waits, and how long.
pub const TRIES: u8 = 5;
pub const WAIT_MS: u64 = 30_000;
/// The most digits of a PIN.
pub const MAX_PIN: usize = 16;
/// The keypad of a PIN, row by row.
pub const KEYS: [&str; 12] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", DELETE, "0", "OK",
];
/// The key that deletes the last digit.
pub const DELETE: &str = "delete";
/// The dots of a pattern.
pub const DOTS: usize = 9;
/// The secret's id.
pub const SECRET: &str = "lock.code";
/// How often the hash is taken.
const ROUNDS: u32 = 2000;
/// The start of a salt made at random (not of the code).
pub const SALT_MARK: &str = "v2.";
/// Random letters of a salt: 80 bits.
pub const SALT_LETTERS: usize = 16;

/// The 40 lower-case hex digits of `digest`, written into `out`.
fn hex_into<'a>(digest: &[u8; 20], out: &'a mut [u8; 40]) -> &'a str {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (pair, byte) in out.as_chunks_mut::<2>().0.iter_mut().zip(digest.iter()) {
        pair[0] = HEX[usize::from(byte >> 4)];
        pair[1] = HEX[usize::from(byte & 15)];
    }
    core::str::from_utf8(out).unwrap_or_default()
}

/// The hash of `code` with `salt`: a salted SHA-1 taken `ROUNDS` times and
/// the SHA-1 of the last digest in hex, as the codes kept so far are. The
/// digests on the way are wiped.
///
/// # Errors
/// `NoMemory` when the hash cannot be kept.
pub fn hash(salt: &str, code: &str) -> Result<String, Error> {
    let mut digest = {
        let mut sha = Sha1::new();
        sha.update(salt.as_bytes());
        sha.update(code.as_bytes());
        sha.finish()
    };
    for _ in 1..ROUNDS {
        let mut sha = Sha1::new();
        sha.update(salt.as_bytes());
        sha.update(&digest);
        let next = sha.finish();
        wipe(&mut digest);
        digest = next;
    }
    // The code is kept as the SHA-1 of the last digest, in hex (what the codes
    // already kept on devices are): one more hash, then the digits.
    let mut last = {
        let mut sha = Sha1::new();
        sha.update(&digest);
        sha.finish()
    };
    let mut digits = [0; 40];
    let text = copy(hex_into(&last, &mut digits));
    wipe(&mut digest);
    wipe(&mut last);
    wipe(&mut digits);
    text
}

/// A code kept: `kind:salt:hash`, all three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stored<'a> {
    pub kind: &'a str,
    pub salt: &'a str,
    pub hash: &'a str,
}

/// `stored` split into its parts, if it has all three.
#[must_use]
pub fn split(stored: &str) -> Option<Stored<'_>> {
    let mut parts = stored.splitn(3, ':');
    Some(Stored {
        kind: parts.next()?,
        salt: parts.next()?,
        hash: parts.next()?,
    })
}

/// The kind of the code kept, if one is whole (kind, salt and hash): a damaged
/// one locks nothing, as no code could open it.
#[must_use]
pub fn kept_kind(stored: Option<&str>) -> Option<&'static str> {
    let Stored { kind, salt, hash } = split(stored?)?;
    if salt.is_empty() || hash.is_empty() {
        return None;
    }
    KINDS.iter().copied().find(|k| *k == kind && *k != "off")
}

/// The kind of the lock in use (`pin`, `pattern`, `password`): one whose code
/// is kept and that the setting (`value`, with or without its prefix) names.
#[must_use]
pub fn active_kind(value: &str, stored: Option<&str>) -> Option<&'static str> {
    let kind = value.strip_prefix(KIND_PREFIX).unwrap_or(value);
    kept_kind(stored).filter(|kept| kind != "off" && *kept == kind)
}

/// Whether `a` and `b` are equal, taking as long whatever they hold.
fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Whether `code` is the code kept in `stored`.
///
/// # Errors
/// `NoMemory` when the hash cannot be made.
pub fn check(stored: Option<&str>, code: &str) -> Result<bool, Error> {
    let Some(Stored {
        salt, hash: kept, ..
    }) = stored.and_then(split)
    else {
        return Ok(false);
    };
    let mut made = hash(salt, code)?;
    let equal = same(&made, kept);
    wipe_string(&mut made);
    Ok(equal)
}

/// The kind of the code kept in `stored`, where `code` opened the lock and its
/// salt was made from it (kept before salts were random): it is to be kept
/// anew with a random salt. `None` where nothing is to change.
///
/// # Errors
/// `NoMemory` when the hash cannot be made.
pub fn renewal<'a>(stored: Option<&'a str>, code: &str) -> Result<Option<&'a str>, Error> {
    let Some(text) = stored else {
        return Ok(None);
    };
    let mut parts = text.splitn(3, ':');
    let (Some(kind), Some(salt)) = (parts.next(), parts.next()) else {
        return Ok(None);
    };
    if salt.starts_with(SALT_MARK) || !check(stored, code)? {
        return Ok(None);
    }
    Ok(Some(kind))
}

/// A salt of its own for a code kept now: the random letters if the device has
/// a source of them, else (on a device without) made of the device's id and the
/// time, never of the code.
///
/// # Errors
/// `NoMemory` when the salt cannot be kept.
pub fn salt(random: Option<&str>, device_id: &str, now_millis: u64) -> Result<String, Error> {
    match random {
        Some(letters) => concat(&[SALT_MARK, letters]),
        None => {
            let mut sha = Sha1::new();
            sha.update(device_id.as_bytes());
            sha.update(&now_millis.to_le_bytes());
            let mut digest = sha.finish();
            let mut digits = [0; 40];
            let hex = hex_into(&digest, &mut digits);
            let letters = hex.get(..SALT_LETTERS).unwrap_or(hex);
            let salt = concat(&[SALT_MARK, letters]);
            wipe(&mut digest);
            wipe(&mut digits);
            salt
        }
    }
}

/// What is kept for a lock of `kind` with `code`: `kind:salt:hash`.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn kept_value(kind: &str, salt: &str, code: &str) -> Result<String, Error> {
    let mut made = hash(salt, code)?;
    let value = concat(&[kind, ":", salt, ":", &made]);
    wipe_string(&mut made);
    value
}

/// Zeroes `text` in place: it keeps its length, all zero bytes (valid text).
pub fn wipe_string(text: &mut String) {
    let mut bytes = core::mem::take(text).into_bytes();
    wipe(&mut bytes);
    *text = String::from_utf8(bytes).unwrap_or_default();
}

/// The longest file of the tries read.
pub const MAX_TRIES_FILE: usize = 16;

/// The wrong tries in a row as the card keeps them (a number); 0 where there is
/// none or it is not one.
#[must_use]
pub fn parse_tries(bytes: &[u8]) -> u8 {
    if bytes.len() > MAX_TRIES_FILE {
        return 0;
    }
    core::str::from_utf8(bytes)
        .ok()
        .and_then(|n| n.trim().parse::<u8>().ok())
        .unwrap_or(0)
}

/// The tries as they are kept; 0 is kept as no file at all.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn format_tries(tries: u8) -> Result<String, Error> {
    let mut digits = [0; 20];
    copy(decimal_u64(u64::from(tries), &mut digits))
}

/// The wrong tries in a row, and until when the next one waits.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Attempts {
    failures: u8,
    wait_until: Option<u64>,
}

/// What a wrong try did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Failed {
    /// The wrong tries in a row now, to keep on the card.
    pub tries: u8,
    /// When the wait is over, if one began.
    pub wake_at: Option<u64>,
}

impl Attempts {
    /// What the card kept when PaperPaw starts: a wait due when it stopped is
    /// waited anew.
    pub fn restore(&mut self, tries: u8, now_ms: u64) {
        self.failures = tries;
        if tries >= TRIES {
            self.wait_until = Some(now_ms.saturating_add(WAIT_MS));
        }
    }

    /// The wrong tries in a row.
    #[must_use]
    pub const fn failures(&self) -> u8 {
        self.failures
    }

    /// A wrong code: counted, and after [`TRIES`] the next one waits.
    pub fn failed(&mut self, now_ms: u64) -> Failed {
        let tries = self.failures.saturating_add(1).min(TRIES);
        self.failures = tries;
        let wake_at = (tries >= TRIES).then(|| now_ms.saturating_add(WAIT_MS));
        if wake_at.is_some() {
            self.wait_until = wake_at;
        }
        Failed { tries, wake_at }
    }

    /// Whether a wait is over at `now_ms` (the timer woke at its end): the
    /// wrong tries are forgotten, and the caller clears the tries file. `false`
    /// where no wait was due or it still runs.
    pub fn expire(&mut self, now_ms: u64) -> bool {
        if self.wait_until.is_some() && !self.waiting(now_ms) {
            self.wait_until = None;
            self.failures = 0;
            return true;
        }
        false
    }

    /// The right code: the tries forgotten. Whether there were any to forget
    /// on the card.
    pub fn succeeded(&mut self) -> bool {
        self.wait_until = None;
        core::mem::take(&mut self.failures) > 0
    }

    /// Whether the next try has to wait at `now_ms`.
    #[must_use]
    pub fn waiting(&self, now_ms: u64) -> bool {
        self.wait_until.is_some_and(|until| now_ms < until)
    }

    /// The seconds left to wait at `now_ms`, rounded up.
    #[must_use]
    pub fn seconds_left(&self, now_ms: u64) -> u64 {
        self.wait_until
            .map_or(0, |until| until.saturating_sub(now_ms).div_ceil(1000))
    }
}

/// Whether `code` is too short to be kept (in characters).
#[must_use]
pub fn too_short(code: &str) -> bool {
    code.chars().count() < MIN_LENGTH
}

/// What a key of the keypad did.
#[derive(Debug, PartialEq, Eq)]
pub enum Pressed {
    /// The digits changed (or nothing: a key past the end, a full PIN).
    Changed,
    /// *OK*: the digits entered.
    Submit(String),
}

/// A PIN being entered.
#[derive(Debug, Default)]
pub struct Pin {
    entered: String,
}

impl Pin {
    /// The digits so far, as many as dots to draw.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entered.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entered.is_empty()
    }

    /// Key `key` of [`KEYS`] was pressed.
    ///
    /// # Errors
    /// `NoMemory` when a digit cannot be kept; the digits are as before.
    pub fn press(&mut self, key: usize) -> Result<Pressed, Error> {
        match KEYS.get(key) {
            Some(&DELETE) => {
                self.entered.pop();
            }
            Some(&"OK") => return Ok(Pressed::Submit(core::mem::take(&mut self.entered))),
            Some(digit) if self.entered.len() < MAX_PIN => {
                // Room for all of it at once, so that no growth leaves a copy
                // of the digits behind, unwiped.
                if self.entered.capacity() < MAX_PIN {
                    self.entered
                        .try_reserve_exact(MAX_PIN - self.entered.len())
                        .map_err(|_| Error::NoMemory)?;
                }
                self.entered.push_str(digit);
            }
            _ => {}
        }
        Ok(Pressed::Changed)
    }

    /// Forgets the digits, wiped.
    pub fn clear(&mut self) {
        wipe_string(&mut self.entered);
        self.entered.clear();
    }

    /// The digits so far (zero bytes after [`Pin::clear`] only until it ends).
    #[must_use]
    pub fn digits(&self) -> &str {
        &self.entered
    }
}

impl Drop for Pin {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Adds dot `dot` (0 to 8) to `entered`, once. Whether it was added.
///
/// # Errors
/// `NoMemory` when it cannot be kept.
pub fn add_dot(entered: &mut String, dot: usize) -> Result<bool, Error> {
    let Some(c) = u8::try_from(dot)
        .ok()
        .filter(|d| usize::from(*d) < DOTS)
        .map(|d| char::from(b'0' + d))
    else {
        return Ok(false);
    };
    if entered.contains(c) {
        return Ok(false);
    }
    // Room for all the dots at once, so that no growth leaves a copy behind,
    // unwiped.
    if entered.capacity() < DOTS {
        entered
            .try_reserve_exact(DOTS - entered.len())
            .map_err(|_| Error::NoMemory)?;
    }
    entered.push(c);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The digits never move while they are entered: a growth would leave a copy
    /// of them in the freed memory.
    #[test]
    fn a_pin_is_kept_in_one_place_while_it_is_entered() {
        let mut pin = Pin::default();
        pin.press(0).unwrap();
        let place = pin.entered.as_ptr();
        assert!(pin.entered.capacity() >= MAX_PIN);
        for _ in 0..MAX_PIN * 2 {
            pin.press(1).unwrap();
            assert_eq!(pin.entered.as_ptr(), place);
        }
        assert_eq!(pin.len(), MAX_PIN);
    }

    #[test]
    fn a_pattern_is_kept_in_one_place_while_it_is_drawn() {
        let mut entered = String::new();
        assert!(add_dot(&mut entered, 4).unwrap());
        let place = entered.as_ptr();
        assert!(entered.capacity() >= DOTS);
        for dot in 0..DOTS {
            add_dot(&mut entered, dot).unwrap();
            assert_eq!(entered.as_ptr(), place);
        }
        assert_eq!(entered.len(), DOTS);
    }
}
