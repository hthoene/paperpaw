//! The pending asks of the lookup service (`docs/abi/document.md` 7): which
//! asker waits for which provider's answer to which (language, text).
//!
//! A fixed table, no allocation: 16 entries on the X4 and 32 elsewhere, 32
//! bytes each. One entry per (asker, provider, hash); asking again refreshes
//! its deadline (60 s on). At most 4 per asker and 8 (X4) or 16 per provider,
//! so one provider cannot fill the table for the others. The table keeps no
//! text, only the 64-bit FNV-1a hash of the ask.

/// Entries of the table: the X4's and the other devices' sizes.
#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub(super) const CAPACITY: usize = 16;
#[cfg(not(any(target_arch = "riscv32", target_arch = "xtensa")))]
pub(super) const CAPACITY: usize = 32;
/// Entries one provider may hold.
#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub(super) const PROVIDER_MAX: usize = 8;
#[cfg(not(any(target_arch = "riscv32", target_arch = "xtensa")))]
pub(super) const PROVIDER_MAX: usize = 16;
/// Entries one asker may hold.
pub(super) const ASKER_MAX: usize = 4;
/// How long an ask waits (60 s).
pub(super) const TTL_MS: u64 = 60_000;
/// The least time between two log lines about unmatched ready commands.
pub(super) const LOG_EVERY_MS: u64 = 60_000;

/// One pending ask; a `deadline` of 0 is a free place (a live ask always has
/// a deadline of at least [`TTL_MS`]).
#[derive(Clone, Copy)]
struct Entry {
    asker: u64,
    provider: u64,
    hash: u64,
    deadline: u64,
}
const FREE: Entry = Entry {
    asker: 0,
    provider: 0,
    hash: 0,
    deadline: 0,
};

/// 64-bit FNV-1a over the length of `language` as one byte, the `language`
/// bytes and the `text` bytes: the pair maps to one input without ambiguity
/// (`language` is at most 35 bytes).
pub(crate) fn ask_hash(language: &str, text: &str) -> u64 {
    const BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let length = [u8::try_from(language.len()).unwrap_or(u8::MAX)];
    length
        .iter()
        .chain(language.as_bytes())
        .chain(text.as_bytes())
        .fold(BASIS, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
        })
}

/// Why an ask was not recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refused {
    /// The table, the asker's share or the provider's share is full.
    Full,
}

/// The table and its counter of ready commands that matched nothing.
pub(crate) struct PendingAsks {
    entries: [Entry; CAPACITY],
    unmatched: u32,
    /// When the last log line was due (`None`: none yet).
    last_log: Option<u64>,
    /// The same for the full-table line.
    last_full_log: Option<u64>,
}

impl PendingAsks {
    pub(crate) const fn new() -> Self {
        Self {
            entries: [FREE; CAPACITY],
            unmatched: 0,
            last_log: None,
            last_full_log: None,
        }
    }

    /// Drops every ask whose deadline has come.
    fn expire(&mut self, now: u64) {
        for entry in &mut self.entries {
            if entry.deadline != 0 && entry.deadline <= now {
                *entry = FREE;
            }
        }
    }

    /// Records the ask of `asker` to `provider` for `hash`, or refreshes its
    /// deadline if it is there already.
    ///
    /// # Errors
    /// [`Refused::Full`] where the table, the asker's share or the provider's
    /// share has no room: nothing is recorded.
    pub(super) fn record(
        &mut self,
        asker: u64,
        provider: u64,
        hash: u64,
        now: u64,
    ) -> Result<(), Refused> {
        self.expire(now);
        let deadline = now.saturating_add(TTL_MS);
        let live = || self.entries.iter().filter(|entry| entry.deadline != 0);
        if let Some(found) = self.entries.iter().position(|e| {
            e.deadline != 0 && (e.asker, e.provider, e.hash) == (asker, provider, hash)
        }) {
            self.entries[found].deadline = deadline;
            return Ok(());
        }
        let by_asker = live().filter(|entry| entry.asker == asker).count();
        let by_provider = live().filter(|entry| entry.provider == provider).count();
        let Some(place) = self.entries.iter().position(|entry| entry.deadline == 0) else {
            return Err(Refused::Full);
        };
        if by_asker >= ASKER_MAX || by_provider >= PROVIDER_MAX {
            return Err(Refused::Full);
        }
        self.entries[place] = Entry {
            asker,
            provider,
            hash,
            deadline,
        };
        Ok(())
    }

    /// Removes the live asks of `provider` for `hash` and returns their
    /// askers (the first `count` of the array). The caller delivers after it
    /// let go of the table, so delivery may stop or ask again. With none, the
    /// counter of unmatched ready commands goes up.
    pub(super) fn take(&mut self, provider: u64, hash: u64, now: u64) -> ([u64; CAPACITY], usize) {
        self.expire(now);
        let mut askers = [0_u64; CAPACITY];
        let mut count = 0;
        for entry in &mut self.entries {
            if entry.deadline != 0 && entry.provider == provider && entry.hash == hash {
                askers[count] = entry.asker;
                count += 1;
                *entry = FREE;
            }
        }
        if count == 0 {
            self.unmatched = self.unmatched.saturating_add(1);
        }
        (askers, count)
    }

    /// The asks of or to `owner` end: it stopped.
    pub(super) fn stopped(&mut self, owner: u64) {
        for entry in &mut self.entries {
            if entry.deadline != 0 && (entry.asker == owner || entry.provider == owner) {
                *entry = FREE;
            }
        }
    }

    /// How many ready commands matched no pending ask, since the start.
    pub(super) fn unmatched(&self) -> u32 {
        self.unmatched
    }

    /// Whether a log line about the unmatched ready commands is due at `now`
    /// (at most one a minute); taking it starts the next minute.
    pub(super) fn log_due(&mut self, now: u64) -> bool {
        match self.last_log {
            Some(last) if now.saturating_sub(last) < LOG_EVERY_MS => false,
            _ => {
                self.last_log = Some(now);
                true
            }
        }
    }

    /// Whether a log line about a full table is due at `now` (at most one a
    /// minute, apart from the unmatched-ready line); taking it starts the
    /// next minute.
    pub(super) fn full_log_due(&mut self, now: u64) -> bool {
        match self.last_full_log {
            Some(last) if now.saturating_sub(last) < LOG_EVERY_MS => false,
            _ => {
                self.last_full_log = Some(now);
                true
            }
        }
    }

    /// The live asks (for tests and diagnostics).
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.iter().filter(|e| e.deadline != 0).count()
    }
}

const _: () = assert!(core::mem::size_of::<Entry>() == 32);

#[cfg(test)]
#[path = "pending_tests.rs"]
mod tests;
