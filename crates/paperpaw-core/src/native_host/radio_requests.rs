//! The device-wide table of Wi-Fi `join` and `host` requests of native plugins
//! (`docs/abi/device.md` sections 3 and 4).
//!
//! Every request has one entry from its submit to the delivery of its single
//! `event.wifi_done`. The entry is fixed-size and carries the room for its
//! outcome, so reserving it at submit (fallibly, at most `MAX_REQUESTS`
//! device-wide, else `ERR_FULL`) is also reserving the outcome: an accepted
//! request always ends and is never dropped for lack of room.
//!
//! Memory: at most four entries, each an owner id (a clone of the plugin id,
//! at most 64 bytes of heap), a 32-byte ssid copy and 24 bytes of state, so
//! about 0.5 KiB device-wide. No password is ever stored here: passwords go
//! from the staged command to the radio command and nowhere else, so no
//! `Debug` output of this table can show one.
//!
//! A request ends EXACTLY once (`Phase::Done` is entered from `Reserved` or
//! `Waiting` only and never left except by release): success, a failure the
//! radio reported, superseded by a newer join or host, unsupported here,
//! needs the user, or the host's backstop timeout. When the sender stops the
//! entry is released with no delivery.

use super::registrations as wire;
use crate::io::WifiEvent;
use crate::native_host::codec;
use alloc::vec::Vec;
use core::fmt;
use paperpaw_native::host_glue::{
    ERR_CANCELLED, ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NOT_FOUND,
    ERR_TIMEOUT, ERR_UNSUPPORTED, OK,
};
use paperpaw_sdk::{Id, wifi::JoinFailure, wifi::WifiState};

/// Most radio requests waiting or holding an undelivered outcome, device-wide.
pub const MAX_REQUESTS: usize = 4;

/// Longest ssid in `command.wifi_join`, `command.wifi_host` and `event.wifi_done`.
pub const SSID_MAX: usize = 32;

/// How long a waiting request may wait for the radio's own report before the
/// host ends it with the timeout reason (a backstop: the shell reports a
/// failed join well before).
pub const BACKSTOP_MS: u64 = 120_000;

/// `event.wifi_done.reason` values (`docs/abi/device.md` section 4; unknown
/// values read as [`reason::OTHER`]).
pub mod reason {
    /// Any other failure (`ERR_IO`); also what an unknown reason reads as.
    pub const OTHER: u32 = 0;
    /// The network was not found (`ERR_NOT_FOUND`).
    pub const NOT_FOUND: u32 = 1;
    /// The password was not accepted (`ERR_DENIED`).
    pub const PASSWORD: u32 = 2;
    /// No answer in time (`ERR_TIMEOUT`).
    pub const TIMEOUT: u32 = 3;
    /// A newer join or host replaced this request (`ERR_CANCELLED`).
    pub const SUPERSEDED: u32 = 4;
    /// The device does not manage Wi-Fi (`ERR_UNSUPPORTED`). Section 4 names
    /// this reason 5; section 0 and 5.5 speak of 19 for the install family.
    /// 5 is used because it is the one `wifi_done` defines (19 would read as
    /// 0 there); the status is `ERR_UNSUPPORTED` either way.
    pub const UNSUPPORTED: u32 = 5;
    /// Needs the user: no input just before and no stored password
    /// (`ERR_DENIED`).
    pub const NEEDS_USER: u32 = 6;
}

/// `event.wifi_done.state` after a successful join (`dev_wifi.state` 3).
pub const STATE_ONLINE: u32 = 3;
/// `event.wifi_done.state` after a successful host (`dev_wifi.state` 4).
pub const STATE_HOSTING: u32 = 4;

/// How a request ended: the fields of `event.wifi_done` after the ssid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// `OK` or the error status of the reason.
    pub status: i32,
    /// 0 on success, else a [`reason`].
    pub reason: u32,
    /// The `dev_wifi` state reached (3 or 4) on success, else 0.
    pub state: u32,
}

impl Outcome {
    /// The network is up: `state` 3 (online) or 4 (hosting).
    #[must_use]
    pub const fn success(state: u32) -> Self {
        Self {
            status: OK,
            reason: 0,
            state,
        }
    }

    /// The failure `reason` with its status; unknown reasons read as 0.
    #[must_use]
    pub const fn failed(reason: u32) -> Self {
        let (status, reason) = match reason {
            reason::NOT_FOUND => (ERR_NOT_FOUND, reason),
            reason::PASSWORD | reason::NEEDS_USER => (ERR_DENIED, reason),
            reason::TIMEOUT => (ERR_TIMEOUT, reason),
            reason::SUPERSEDED => (ERR_CANCELLED, reason),
            reason::UNSUPPORTED => (ERR_UNSUPPORTED, reason),
            _ => (ERR_IO, reason::OTHER),
        };
        Self {
            status,
            reason,
            state: 0,
        }
    }

    fn of_join(failure: JoinFailure) -> Self {
        Self::failed(match failure {
            JoinFailure::NotFound => reason::NOT_FOUND,
            JoinFailure::Password => reason::PASSWORD,
            JoinFailure::Timeout => reason::TIMEOUT,
            _ => reason::OTHER,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Staged by a submit; the drain has not applied it yet.
    Reserved,
    /// Applied; the radio has not reported yet.
    Waiting { since_ms: u64 },
    /// Ended; the outcome waits for its delivery.
    Done(Outcome),
}

/// A copied ssid (at most 32 bytes), so an entry has no per-request heap.
#[derive(Clone, Copy)]
struct Ssid {
    bytes: [u8; SSID_MAX],
    len: u8,
}

impl Ssid {
    fn new(text: &str) -> Option<Self> {
        let source = text.as_bytes();
        let mut bytes = [0; SSID_MAX];
        bytes.get_mut(..source.len())?.copy_from_slice(source);
        Some(Self {
            bytes,
            len: u8::try_from(source.len()).ok()?,
        })
    }

    fn as_str(&self) -> &str {
        self.bytes
            .get(..usize::from(self.len))
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .unwrap_or_default()
    }
}

struct Entry {
    /// The staged batch that reserved it (0: none); see [`Self::new_token`].
    token: u32,
    owner: Id,
    request: u32,
    host: bool,
    ssid: Ssid,
    phase: Phase,
}

/// Attempts the host stopped waiting for (superseded, left, the sender
/// stopped) whose terminal report the radio has not made yet, per ssid. The
/// X4's radio runs its queued commands strictly one after the other and
/// normally ends every join or host with one report (`Failed`, `Online` or
/// `Hosting`; a join that links without an address, and a radio restart, end
/// with none), so such an attempt will usually produce one more report for
/// its ssid that belongs to no waiting request. Counting them lets a late
/// `Failed` of a superseded attempt be told from the replacement's own, as
/// long as the radio keeps that order; the expiry bounds the harm when it
/// does not. This is not per-request correlation.
#[derive(Clone, Copy)]
struct Stale {
    ssid: Ssid,
    count: u8,
    /// When the host first saw it (set at the next tick); it expires after
    /// [`BACKSTOP_MS`], so a report that never comes cannot mute one forever.
    since_ms: Option<u64>,
}

/// The table. See the module documentation.
#[derive(Default)]
pub struct RadioRequests {
    entries: Vec<Entry>,
    stale: [Option<Stale>; MAX_REQUESTS],
    last_token: u32,
    /// No delivery is tried before this time (a plugin could not be called).
    retry_at: Option<u64>,
}

impl fmt::Debug for RadioRequests {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Counts only: no ssid, no password (none is ever held here).
        let waiting = self
            .entries
            .iter()
            .filter(|entry| matches!(entry.phase, Phase::Waiting { .. }))
            .count();
        f.debug_struct("RadioRequests")
            .field("entries", &self.entries.len())
            .field("waiting", &waiting)
            .finish()
    }
}

/// An ended request ready for delivery to its sender only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delivery<'a> {
    /// The sender's request number.
    pub request: u32,
    /// The network the request named.
    pub ssid: &'a str,
    /// How it ended.
    pub outcome: Outcome,
}

impl RadioRequests {
    /// An empty table; allocates nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            stale: [None; MAX_REQUESTS],
            last_token: 0,
            retry_at: None,
        }
    }

    /// A token for one staged batch: the entries it reserves carry it, so
    /// giving the batch back touches only them (never 0). Tokens are issued
    /// once each and never reused, because an old output can outlive its
    /// entries; `None` when all `u32::MAX` are used (then no batch can
    /// reserve a request, and nothing else is affected).
    pub fn new_token(&mut self) -> Option<u32> {
        self.last_token = self.last_token.checked_add(1)?;
        Some(self.last_token)
    }

    /// Whether the batch with `token` still holds `request` of `owner`, not
    /// yet applied: the only entry it may activate or end.
    #[must_use]
    pub fn holds_reserved_in(&self, token: u32, owner: &str, request: u32) -> bool {
        token != 0
            && self.position(owner, request).is_some_and(|at| {
                self.entries
                    .get(at)
                    .is_some_and(|entry| entry.token == token && entry.phase == Phase::Reserved)
            })
    }

    /// Use up every token (a test of the exhaustion rule).
    #[cfg(test)]
    pub(crate) fn exhaust_tokens(&mut self) {
        self.last_token = u32::MAX;
    }

    /// An attempt for `ssid` is no longer waited for but still runs.
    fn note_stale(&mut self, ssid: Ssid) {
        let free = self.stale.iter().position(Option::is_none);
        let same = self
            .stale
            .iter()
            .position(|slot| slot.is_some_and(|stale| stale.ssid.as_str() == ssid.as_str()));
        // Every slot taken by other ssids: the first gives way (not
        // necessarily the oldest), and the new ssid starts at one.
        let at = same.or(free).unwrap_or(0);
        if let Some(slot) = self.stale.get_mut(at) {
            let count = match (same, *slot) {
                (Some(_), Some(stale)) => stale.count,
                _ => 0,
            };
            *slot = Some(Stale {
                ssid,
                count: count.saturating_add(1),
                since_ms: None,
            });
        }
    }

    /// The radio made a terminal report for `ssid`: whether it belongs to a
    /// stale attempt (and is used up).
    fn take_stale(&mut self, ssid: &str) -> bool {
        let Some(slot) = self
            .stale
            .iter_mut()
            .find(|slot| slot.is_some_and(|stale| stale.ssid.as_str() == ssid))
        else {
            return false;
        };
        if let Some(stale) = slot {
            stale.count = stale.count.saturating_sub(1);
            if stale.count == 0 {
                *slot = None;
            }
        }
        true
    }

    /// Entries held (reserved, waiting or ended and undelivered).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no entry is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn position(&self, owner: &str, request: u32) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.request == request && entry.owner.as_str() == owner)
    }

    /// Whether `owner` has `request` held in any phase.
    #[must_use]
    pub fn holds(&self, owner: &str, request: u32) -> bool {
        self.position(owner, request).is_some()
    }

    /// Reserve the entry and its outcome room for one request at submit.
    ///
    /// # Errors
    /// `ERR_INVALID` for request 0 or one `owner` already holds, `ERR_FULL`
    /// when [`MAX_REQUESTS`] entries are held, `ERR_NO_MEMORY` when the
    /// entry cannot be allocated. The table is unchanged on error.
    pub fn reserve(&mut self, owner: &Id, request: u32, host: bool, ssid: &str) -> Result<(), i32> {
        self.reserve_in(0, owner, request, host, ssid)
    }

    /// Like [`Self::reserve`], for the staged batch with `token`.
    ///
    /// # Errors
    /// As [`Self::reserve`].
    pub fn reserve_in(
        &mut self,
        token: u32,
        owner: &Id,
        request: u32,
        host: bool,
        ssid: &str,
    ) -> Result<(), i32> {
        if request == 0 || self.holds(owner.as_str(), request) {
            return Err(ERR_INVALID);
        }
        if self.entries.len() >= MAX_REQUESTS {
            return Err(ERR_FULL);
        }
        let ssid = Ssid::new(ssid).ok_or(ERR_INVALID)?;
        let owner = owner.try_clone().ok_or(ERR_NO_MEMORY)?;
        self.entries.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        self.entries.push(Entry {
            token,
            owner,
            request,
            host,
            ssid,
            phase: Phase::Reserved,
        });
        Ok(())
    }

    /// Give back a reservation the drain never applied (the batch was
    /// refused after it, or the stage was dropped). Only `Reserved` entries.
    pub fn release_reserved(&mut self, owner: &str, request: u32) {
        if let Some(at) = self.position(owner, request)
            && self.entries.get(at).map(|entry| entry.phase) == Some(Phase::Reserved)
        {
            self.entries.remove(at);
        }
    }

    /// Give back every reservation of the batch with `token` that was never
    /// applied. Token 0 (no batch) gives back nothing.
    pub fn discard_reserved(&mut self, token: u32) {
        if token != 0 {
            self.entries
                .retain(|entry| entry.token != token || entry.phase != Phase::Reserved);
        }
    }

    /// The request is applied: it waits for the radio from `now_ms`.
    pub fn activate(&mut self, owner: &str, request: u32, now_ms: u64) {
        if let Some(at) = self.position(owner, request)
            && let Some(entry) = self.entries.get_mut(at)
            && entry.phase == Phase::Reserved
        {
            entry.phase = Phase::Waiting { since_ms: now_ms };
        }
    }

    /// A newer join or host from any plugin replaces the waiting one: its
    /// outcome is [`reason::SUPERSEDED`].
    pub fn supersede_waiting(&mut self) {
        self.cancel_waiting();
    }

    /// Ends every waiting request as superseded; their attempts may still
    /// run at the radio, so each is noted as stale.
    fn cancel_waiting(&mut self) {
        let mut stale = [None; MAX_REQUESTS];
        for (slot, entry) in stale.iter_mut().zip(
            self.entries
                .iter()
                .filter(|entry| matches!(entry.phase, Phase::Waiting { .. })),
        ) {
            *slot = Some(entry.ssid);
        }
        self.end_waiting(Outcome::failed(reason::SUPERSEDED), |_| true);
        for ssid in stale.into_iter().flatten() {
            self.note_stale(ssid);
        }
    }

    /// End one request that has not ended; false when it was not held or had
    /// ended already (so no request ends twice).
    pub fn end(&mut self, owner: &str, request: u32, outcome: Outcome) -> bool {
        let Some(entry) = self
            .position(owner, request)
            .and_then(|at| self.entries.get_mut(at))
        else {
            return false;
        };
        if matches!(entry.phase, Phase::Done(_)) {
            return false;
        }
        entry.phase = Phase::Done(outcome);
        true
    }

    fn end_waiting(&mut self, outcome: Outcome, mut wanted: impl FnMut(&Entry) -> bool) {
        for entry in &mut self.entries {
            if matches!(entry.phase, Phase::Waiting { .. }) && wanted(entry) {
                entry.phase = Phase::Done(outcome);
            }
        }
    }

    /// The radio is told to leave or switch off: what waited is cancelled.
    pub fn radio_stopped(&mut self) {
        self.cancel_waiting();
    }

    /// What the radio reported: the network a waiting join named up (online)
    /// or a waiting host's (hosting) ends it, a failure ends the waiting
    /// request for that ssid, the radio off ends any waiting one as "other".
    ///
    /// The radio's events carry no request number, so they are matched by
    /// ssid. A stale report of the same ssid as the waiting request (join A,
    /// join A again) cannot be told apart: success is the goal state either
    /// way, but a stale *failure* can end the newer attempt early. Closing
    /// that needs a request generation in the radio API (open item before the
    /// feature is activated).
    pub fn on_wifi_event(&mut self, event: &WifiEvent) {
        match event {
            // Only the network the request named completes it: a late report of
            // an earlier, superseded attempt (another ssid) is not its outcome.
            WifiEvent::State(WifiState::Online { ssid, .. }) => {
                // The goal state holds whichever attempt reached it; a stale
                // attempt's report is used up.
                self.take_stale(ssid.as_str());
                self.end_waiting(Outcome::success(STATE_ONLINE), |entry| {
                    !entry.host && entry.ssid.as_str() == ssid.as_str()
                });
            }
            WifiEvent::State(WifiState::Hosting { ssid, .. }) => {
                self.take_stale(ssid.as_str());
                self.end_waiting(Outcome::success(STATE_HOSTING), |entry| {
                    entry.host && entry.ssid.as_str() == ssid.as_str()
                });
            }
            WifiEvent::State(WifiState::Off) => {
                // The queue is cleared with the radio: nothing is still to
                // report.
                self.stale = [None; MAX_REQUESTS];
                self.end_waiting(Outcome::failed(reason::OTHER), |_| true);
            }
            WifiEvent::Failed { ssid, reason } => {
                // The report of an attempt the host stopped waiting for is
                // not the replacement's outcome, also for the same ssid.
                if self.take_stale(ssid.as_str()) {
                    return;
                }
                self.end_waiting(Outcome::of_join(*reason), |entry| {
                    entry.ssid.as_str() == ssid.as_str()
                });
            }
            _ => {}
        }
    }

    /// Time passes: a request that waited [`BACKSTOP_MS`] ends as a timeout.
    pub fn tick(&mut self, now_ms: u64) {
        for slot in &mut self.stale {
            let expired = slot.as_mut().is_some_and(|stale| {
                let since = *stale.since_ms.get_or_insert(now_ms);
                now_ms.saturating_sub(since) >= BACKSTOP_MS
            });
            if expired {
                *slot = None;
            }
        }
        let mut late = |entry: &Entry| {
            matches!(entry.phase, Phase::Waiting { since_ms }
                if now_ms.saturating_sub(since_ms) >= BACKSTOP_MS)
        };
        self.end_waiting(Outcome::failed(reason::TIMEOUT), &mut late);
    }

    /// The sender stopped: its entries are released with no delivery.
    pub fn sender_stopped(&mut self, owner: &str) {
        // A waiting attempt of the dead sender still ends at the radio.
        let mut stale = [None; MAX_REQUESTS];
        for (slot, entry) in stale.iter_mut().zip(
            self.entries
                .iter()
                .filter(|entry| entry.owner.as_str() == owner)
                .filter(|entry| matches!(entry.phase, Phase::Waiting { .. })),
        ) {
            *slot = Some(entry.ssid);
        }
        for ssid in stale.into_iter().flatten() {
            self.note_stale(ssid);
        }
        self.entries.retain(|entry| entry.owner.as_str() != owner);
    }

    /// The oldest ended request of `owner`, not yet delivered.
    #[must_use]
    pub fn next_outcome(&self, owner: &str) -> Option<Delivery<'_>> {
        self.entries.iter().find_map(|entry| match entry.phase {
            Phase::Done(outcome) if entry.owner.as_str() == owner => Some(Delivery {
                request: entry.request,
                ssid: entry.ssid.as_str(),
                outcome,
            }),
            _ => None,
        })
    }

    /// The `nth` ended request, oldest first, with its owner: the pump tells
    /// each plugin its own, one that cannot be called does not hold up another.
    #[must_use]
    pub fn nth_outcome(&self, nth: usize) -> Option<(&str, Delivery<'_>)> {
        self.entries
            .iter()
            .filter_map(|entry| match entry.phase {
                Phase::Done(outcome) => Some((
                    entry.owner.as_str(),
                    Delivery {
                        request: entry.request,
                        ssid: entry.ssid.as_str(),
                        outcome,
                    },
                )),
                _ => None,
            })
            .nth(nth)
    }

    /// Whether an ended request waits for its delivery.
    #[must_use]
    pub fn has_outcomes(&self) -> bool {
        self.nth_outcome(0).is_some()
    }

    /// A plugin could not be called: no new try before `until_ms`.
    pub fn hold_back(&mut self, until_ms: u64) {
        self.retry_at = Some(until_ms);
    }

    /// Whether deliveries wait for a later time.
    #[must_use]
    pub fn held(&self, now_ms: u64) -> bool {
        self.retry_at.is_some_and(|at| now_ms < at)
    }

    /// When the held deliveries are tried again; none without an ended request.
    #[must_use]
    pub fn due(&self) -> Option<u64> {
        self.retry_at.filter(|_| self.has_outcomes())
    }

    /// Releases the `nth` ended request when it is still `request` (its sender
    /// is gone, so nobody is told); never another one.
    pub fn release_ended_at(&mut self, nth: usize, request: u32) -> bool {
        let at = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| matches!(entry.phase, Phase::Done(_)))
            .nth(nth)
            .filter(|(_, entry)| entry.request == request)
            .map(|(at, _)| at);
        at.is_some_and(|at| {
            self.entries.remove(at);
            true
        })
    }

    /// The outcome was delivered: release its room. False when `owner` has no
    /// ended request with this number.
    pub fn delivered(&mut self, owner: &str, request: u32) -> bool {
        match self.position(owner, request) {
            Some(at) if matches!(self.entries.get(at).map(|e| e.phase), Some(Phase::Done(_))) => {
                self.entries.remove(at);
                true
            }
            _ => false,
        }
    }
}

impl Delivery<'_> {
    /// Encode `event.wifi_done` (tag 93) for this outcome.
    ///
    /// # Errors
    /// The codec's status, e.g. `ERR_NO_SPACE` when `out` is too small.
    pub fn encode(self, now_ms: u64, out: &mut [u8]) -> Result<usize, i32> {
        codec::encode_batch(
            &[wire::Record::EventWifiDone(wire::EventWifiDone {
                now_ms,
                request: self.request,
                ssid: self.ssid,
                status: self.outcome.status,
                reason: self.outcome.reason,
                state: self.outcome.state,
            })],
            out,
        )
    }
}

#[cfg(test)]
#[path = "radio_requests_tests.rs"]
mod tests;
