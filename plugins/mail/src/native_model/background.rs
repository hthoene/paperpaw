//! The background check for mail (the setting *Check for new mail in the
//! background*, owner decision 52): how often, and which accounts a round
//! has still to check. The host-free part; the work, the socket and the
//! connection are `native_background.rs`.
use super::mem::{Error, push};
use alloc::vec::Vec;

/// The choices of the setting *How often*, as the translation keys the
/// setting keeps them by, with the minutes each means.
pub const EVERY: [(&str, u32); 3] = [
    ("mail.every.15", 15),
    ("mail.every.30", 30),
    ("mail.every.60", 60),
];

/// The minutes of the default choice.
pub const DEFAULT_MINUTES: u32 = 30;

/// The key of the work.
pub const KEY: &str = "mail.check";

/// How long after the setting is turned on, or after a start, the first check
/// comes (a minute: the radio and the clock are up by then).
pub const FIRST_MS: u32 = 60_000;

/// The minutes a choice of *How often* means (the default for a value that is
/// none of the three).
#[must_use]
pub fn minutes(choice: &str) -> u32 {
    EVERY
        .iter()
        .find_map(|(key, minutes)| (*key == choice).then_some(*minutes))
        .unwrap_or(DEFAULT_MINUTES)
}

/// The time between two checks, in milliseconds, for a choice of *How often*.
#[must_use]
pub fn every_ms(choice: &str) -> u32 {
    minutes(choice) * 60_000
}

/// One round of the background check: the accounts still to check, the one
/// at the front being checked now.
#[derive(Debug, Default)]
pub struct Round {
    queue: Vec<u8>,
}

impl Round {
    /// A round over `accounts`, in the order given.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(accounts: &[u8]) -> Result<Self, Error> {
        let mut queue = Vec::new();
        queue
            .try_reserve_exact(accounts.len())
            .map_err(|_| Error::NoMemory)?;
        for account in accounts {
            push(&mut queue, *account)?;
        }
        Ok(Self { queue })
    }

    /// The account to check now, none once the round is over.
    #[must_use]
    pub fn front(&self) -> Option<u8> {
        self.queue.first().copied()
    }

    /// The account at the front is done, or passed by.
    pub fn pass(&mut self) {
        if !self.queue.is_empty() {
            self.queue.remove(0);
        }
    }

    /// The round ends where it is (a failure ends it for all: a connection that
    /// fails once is not tried again at once for the next account).
    pub fn end(&mut self) {
        self.queue.clear();
    }

    /// Whether every account was checked or the round was ended.
    #[must_use]
    pub fn is_over(&self) -> bool {
        self.queue.is_empty()
    }
}
