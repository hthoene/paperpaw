//! The lock without the host: how the code is kept and checked, how wrong
//! tries are counted and waited out, and the steps of unlocking and of setting
//! the lock up. The host's secrets reach it through [`Vault`], so the logic is
//! tested on its own.
use alloc::{format, string::String, vec::Vec};
use paperpaw_sdk::digest::sha1::{self, Sha1};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::text::{concat, copy};

/// The kinds; the setting stores them after [`KIND_PREFIX`].
pub const KINDS: [&str; 4] = ["off", "pin", "pattern", "password"];
pub const KIND_PREFIX: &str = "lock.kind.";
/// The shortest code.
pub const MIN_LENGTH: usize = 4;
/// Wrong tries before the next one waits, and how long.
pub const TRIES: u8 = 5;
pub const WAIT_MS: u64 = 30_000;
/// The most digits of a PIN.
pub const MAX_PIN: usize = 16;
/// How often the hash is taken.
const ROUNDS: u32 = 2000;
/// The start of a salt made at random (not of the code).
const SALT_MARK: &str = "v2.";
/// Random letters of a salt: 80 bits.
const SALT_LETTERS: usize = 16;

/// Where the code is kept: the plugin's own secret, and the source of random
/// letters.
pub trait Vault {
    /// The secret as kept; `None` where there is none.
    fn get(&mut self) -> Result<Option<String>, Error>;
    /// Keeps `value` (a hash: nothing to type anew in a list).
    fn keep(&mut self, value: &str) -> Result<(), Error>;
    /// Forgets the secret.
    fn remove(&mut self) -> Result<(), Error>;
    /// `count` random letters.
    fn token(&mut self, count: usize) -> Result<String, Error>;
}

/// The hash of `code` with `salt`.
#[must_use]
pub fn hash(salt: &str, code: &str) -> String {
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
        digest = sha.finish();
    }
    sha1::hex(&digest)
}

/// What is kept: `kind:salt:hash`.
fn split(stored: &str) -> Option<(&str, &str, &str)> {
    let mut parts = stored.splitn(3, ':');
    Some((parts.next()?, parts.next()?, parts.next()?))
}

/// The kind of the code kept, if one is whole (kind, salt and hash): a damaged
/// one locks nothing, as no code could open it.
pub fn kept_kind(vault: &mut impl Vault) -> Result<Option<String>, Error> {
    let Some(stored) = vault.get()? else {
        return Ok(None);
    };
    let Some((kind, salt, hashed)) = split(&stored) else {
        return Ok(None);
    };
    if salt.is_empty() || hashed.is_empty() || kind == "off" || !KINDS.contains(&kind) {
        return Ok(None);
    }
    Ok(Some(copy(kind)?))
}

/// The kind of the lock in use (`pin`, `pattern`, `password`): one whose code
/// is kept and that the setting `kind_setting` names.
pub fn active(vault: &mut impl Vault, kind_setting: &str) -> Result<Option<String>, Error> {
    let kind = kind_setting
        .strip_prefix(KIND_PREFIX)
        .unwrap_or(kind_setting);
    Ok(kept_kind(vault)?.filter(|kept| kind != "off" && kept == kind))
}

/// Whether `code` is the code in use.
pub fn check(vault: &mut impl Vault, code: &str) -> Result<bool, Error> {
    let Some(stored) = vault.get()? else {
        return Ok(false);
    };
    Ok(split(&stored).is_some_and(|(_, salt, hashed)| hash(salt, code) == hashed))
}

/// Keeps `code` anew with a random salt, where `code` opened the lock and its
/// salt was made from it (kept before salts were random).
pub fn renew(vault: &mut impl Vault, code: &str) -> Result<(), Error> {
    let Some(stored) = vault.get()? else {
        return Ok(());
    };
    let Some((kind, salt, hashed)) = split(&stored) else {
        return Ok(());
    };
    if salt.starts_with(SALT_MARK) || hash(salt, code) != hashed {
        return Ok(());
    }
    let kind = copy(kind)?;
    keep(vault, &kind, code)
}

/// Keeps `code` for a lock of `kind`, with a salt of its own: random, never
/// made of the code.
pub fn keep(vault: &mut impl Vault, kind: &str, code: &str) -> Result<(), Error> {
    let letters = vault.token(SALT_LETTERS)?;
    let salt = concat(&[SALT_MARK, &letters])?;
    let value = format!("{kind}:{salt}:{}", hash(&salt, code));
    vault.keep(&value)
}

/// The wrong tries in a row and the wait that follows [`TRIES`] of them.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tries {
    pub failures: u8,
    /// Until when (host milliseconds) the next try waits.
    pub wait_until: Option<u64>,
}

impl Tries {
    /// The count the card kept, a wait due when PaperPaw stopped waited anew.
    #[must_use]
    pub fn restored(failures: u8, now_ms: u64) -> Self {
        Self {
            failures,
            wait_until: (failures >= TRIES).then(|| now_ms.saturating_add(WAIT_MS)),
        }
    }

    /// Whether the next try must wait at `now_ms`.
    #[must_use]
    pub fn waiting(&self, now_ms: u64) -> bool {
        self.wait_until.is_some_and(|until| now_ms < until)
    }

    /// A wrong code: counted; after [`TRIES`] the next one waits. Returns the
    /// wait's end where one began.
    pub fn failed(&mut self, now_ms: u64) -> Option<u64> {
        self.failures = self.failures.saturating_add(1).min(TRIES);
        if self.failures >= TRIES {
            let until = now_ms.saturating_add(WAIT_MS);
            self.wait_until = Some(until);
            return Some(until);
        }
        None
    }

    /// The right code: the tries forgotten. Whether there were any to forget.
    pub fn succeeded(&mut self) -> bool {
        self.wait_until = None;
        core::mem::replace(&mut self.failures, 0) > 0
    }
}

/// The count as the card keeps it (a number); anything else counts as none.
#[must_use]
pub fn parse_tries(bytes: &[u8]) -> u8 {
    core::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.trim().parse::<u8>().ok())
        .unwrap_or(0)
}

/// The steps of unlocking and of setting the lock up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The code to unlock.
    Unlock,
    /// The code in use, before it is changed.
    Verify,
    /// Which kind of lock, listed.
    Choose,
    /// A new code, then the same again.
    New,
    Confirm(String),
}

/// What to tell the reader under the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    Wrong,
    TooShort,
    Mismatch,
}

/// What a code entered asks the host to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    /// Unlocked: the screen closes.
    Unlock,
    /// The code is kept: the kind is set, the reader told, the screen closes.
    Done,
    /// The lock is off: the code forgotten, the kind set to off, the reader
    /// told, the screen closes.
    Removed,
    /// A wrong try began a wait that ends at this host time.
    WaitUntil(u64),
    /// The tries are to be kept on the card.
    KeepTries,
    /// The password is to be asked for on the keyboard.
    AskPassword,
}

/// The lock screen's state without the screen.
#[derive(Debug)]
pub struct Machine {
    pub step: Step,
    /// What is entered: `pin`, `pattern` or `password`.
    pub kind: String,
    pub note: Option<Note>,
    /// Whether the keyboard was asked for already.
    pub asked: bool,
}

impl Machine {
    /// Unlocking, verifying the code in use, or choosing a kind.
    pub fn new(unlock: bool, active: Option<String>) -> Result<Self, Error> {
        let step = if unlock {
            Step::Unlock
        } else if active.is_some() {
            Step::Verify
        } else {
            Step::Choose
        };
        let kind = match active {
            Some(kind) => kind,
            None => copy("pin")?,
        };
        Ok(Self {
            step,
            kind,
            note: None,
            asked: false,
        })
    }

    /// Kind `kind` was chosen: off at once, else the new code.
    pub fn choose_kind(&mut self, kind: &str, vault: &mut impl Vault) -> Result<Vec<Do>, Error> {
        let mut out = Vec::new();
        if kind == "off" {
            vault.remove()?;
            push(&mut out, Do::Removed)?;
            return Ok(out);
        }
        self.kind = copy(kind)?;
        self.step = Step::New;
        self.asked = false;
        if self.kind == "password" {
            self.asked = true;
            push(&mut out, Do::AskPassword)?;
        }
        Ok(out)
    }

    /// A code was entered. `released` is true where unlocking has no code kept
    /// any more; `now_ms` the host's clock.
    pub fn submit(
        &mut self,
        entered: String,
        vault: &mut impl Vault,
        tries: &mut Tries,
        now_ms: u64,
        released: bool,
    ) -> Result<Vec<Do>, Error> {
        let mut out = Vec::new();
        self.note = None;
        match self.step.clone() {
            Step::Unlock | Step::Verify => {
                if released && self.step == Step::Unlock {
                    push(&mut out, Do::Unlock)?;
                    return Ok(out);
                }
                if tries.waiting(now_ms) {
                    return Ok(out);
                }
                if check(vault, &entered)? {
                    renew(vault, &entered)?;
                    if tries.succeeded() {
                        push(&mut out, Do::KeepTries)?;
                    }
                    if self.step == Step::Unlock {
                        push(&mut out, Do::Unlock)?;
                        return Ok(out);
                    }
                    self.step = Step::Choose;
                } else {
                    if let Some(until) = tries.failed(now_ms) {
                        push(&mut out, Do::WaitUntil(until))?;
                    }
                    push(&mut out, Do::KeepTries)?;
                    self.note = Some(Note::Wrong);
                }
            }
            Step::New => {
                if entered.chars().count() < MIN_LENGTH {
                    self.note = Some(Note::TooShort);
                } else {
                    self.step = Step::Confirm(entered);
                    self.asked = false;
                }
            }
            Step::Confirm(first) => {
                if entered == first {
                    keep(vault, &self.kind, &entered)?;
                    push(&mut out, Do::Done)?;
                    return Ok(out);
                }
                self.step = Step::New;
                self.asked = false;
                self.note = Some(Note::Mismatch);
            }
            Step::Choose => {}
        }
        if self.kind == "password" && !self.asked && self.step != Step::Choose {
            self.asked = true;
            push(&mut out, Do::AskPassword)?;
        }
        Ok(out)
    }
}

fn push<T>(items: &mut Vec<T>, item: T) -> Result<(), Error> {
    items.try_reserve(1).map_err(|_| Error::NoMemory)?;
    items.push(item);
    Ok(())
}

#[cfg(test)]
mod tests;
