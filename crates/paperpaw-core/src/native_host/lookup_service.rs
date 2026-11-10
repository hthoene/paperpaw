//! Host side of the service `core.lookup` (`docs/abi/document.md` 7).
//!
//! One op, `lookup`: the asker names a text and gets the entry of one
//! provider per call, `cursor` the index of the provider page (0 first). The
//! host walks the providers in the user's order through its stage
//! `core.lookup`, one provider per call, no provider seeing another's answer;
//! the broker, the user's order and the language and network filters are the
//! [`LookupHost`]'s. This file owns the input rules and the check of what a
//! provider answered: exactly one `lookup_entry`, its flags known, its fields
//! within the contract. A malformed provider answer is `ERR_INVALID`; running
//! out of providers is `ERR_NOT_FOUND`, which ends the asker's loop.
//!
//! Memory: nothing is kept; the call runs in the dispatcher's 4,096-byte
//! scratch (a `lookup_entry` is at most 2,384 bytes).
use super::{
    host_services::Caller,
    registrations::{self as wire, Record},
};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use crate::native_host::codec;
use alloc::rc::Rc;
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_UNSUPPORTED};

mod broker;
mod pending;
mod providers;

pub(crate) use pending::PendingAsks;
pub(crate) use providers::Providers;

/// `lookup_entry.flags`: bit 0 more pages follow, bit 1 pending (the provider
/// answers later).
const ENTRY_FLAGS: u32 = 0b11;
/// `lookup_entry.flags` bit 1.
const PENDING: u32 = 0b10;

/// What the shell provides under the key `core.lookup`.
pub(crate) trait LookupHost {
    /// The entry of the `cursor`th provider page for this ask, in the user's
    /// order, written into `out` as one `lookup_entry`; `ERR_NOT_FOUND` past the
    /// last provider. A provider that fails answers its own status, but **a
    /// provider's own `ERR_NOT_FOUND` never reaches the asker**: it would end
    /// the asker's loop, so the broker turns it into an empty entry
    /// ([`broker`]).
    fn ask(
        &self,
        frame: &dyn Frame,
        owner: u64,
        ask: &wire::ServiceLookupAsk<'_>,
        out: &mut [u8],
    ) -> Result<usize, i32>;

    /// The owner of the live plugin that provides the lookup `name`; `None`
    /// where there is none (or the host cannot say: no ask is then kept as
    /// pending, and the asker asks again itself).
    fn provider_owner(&self, _name: &str) -> Option<u64> {
        None
    }

    /// Whether `asker` subscribes to `event.lookup_ready` (110): only such an
    /// asker has its pending asks kept.
    fn subscribed(&self, _asker: u64) -> bool {
        false
    }

    /// The clock the 60-second deadlines run on, in milliseconds.
    fn now_ms(&self) -> u64 {
        0
    }

    /// One line for the host's own capture, in addition to the device log
    /// (`KernelBuilder::device_log`), which the service writes itself: a
    /// shell must not forward it to the device log (every line would print
    /// twice).
    fn log(&self, _line: &str) {}
}

/// What the shell hands the frame under the key `core.lookup`: the host and
/// the table of pending asks (`pending.rs`).
pub(crate) struct LookupServices {
    host: Rc<dyn LookupHost>,
    /// The shell's device log: the lines of [`Self::note`] reach it.
    log: crate::native_host::device_log::DeviceLog,
    pending: RefCell<PendingAsks>,
}

impl LookupServices {
    pub(crate) fn new(
        host: Rc<dyn LookupHost>,
        log: crate::native_host::device_log::DeviceLog,
    ) -> Self {
        Self {
            host,
            log,
            pending: RefCell::new(PendingAsks::new()),
        }
    }

    /// Whether the full-table line is due (once a minute).
    fn pending_full_log_due(&self) -> bool {
        let now = self.host.now_ms();
        self.pending
            .try_borrow_mut()
            .is_ok_and(|mut table| table.full_log_due(now))
    }

    /// One fixed line: to the device log, and to the host's own
    /// [`LookupHost::log`] (a capture, a second sink; no-op by default).
    fn note(&self, line: &'static str) {
        self.log.line(format_args!("{line}"));
        self.host.log(line);
    }

    /// The provider `provider` kept its result for (`language`, `text`)
    /// (`command.lookup_ready`): its pending asks end and `deliver` is called
    /// with each asker, which gets `event.lookup_ready` ([`ready_event`]).
    /// Never an error to the provider: a command that matches no ask is
    /// counted and logged at most once a minute.
    pub(crate) fn ready(
        &self,
        provider: u64,
        language: &str,
        text: &str,
        deliver: &mut dyn FnMut(u64),
    ) {
        let now = self.host.now_ms();
        let hash = pending::ask_hash(language, text);
        // Take the askers out and let go of the table before delivering, so
        // delivery may stop a plugin or record a new ask.
        let (askers, count, log_due) = {
            let Ok(mut table) = self.pending.try_borrow_mut() else {
                return;
            };
            let (askers, count) = table.take(provider, hash, now);
            (askers, count, count == 0 && table.log_due(now))
        };
        if log_due {
            self.note("lookup_ready matched no pending ask");
        }
        for asker in askers.iter().take(count) {
            deliver(*asker);
        }
    }

    /// The plugin `owner` stopped: the asks it made and the asks to it end,
    /// without an event. The production host must call this on every plugin
    /// stop, or a stopped plugin's asks stay until their deadline.
    pub(crate) fn stopped(&self, owner: u64) {
        if let Ok(mut table) = self.pending.try_borrow_mut() {
            table.stopped(owner);
        }
    }

    /// How many ready commands matched no pending ask, since the start.
    #[cfg(test)]
    pub(crate) fn unmatched(&self) -> u32 {
        self.pending.borrow().unmatched()
    }

    /// The pending asks kept now.
    #[cfg(test)]
    pub(crate) fn pending_len(&self) -> usize {
        self.pending.borrow().len()
    }

    /// Keeps the ask whose answer the provider gave as pending, for an asker
    /// that subscribes to `event.lookup_ready`. `Err(())`: the table has no
    /// room (the asker gets the empty entry, and the host logs it).
    fn keep_pending(
        &self,
        asker: u64,
        provider: &str,
        ask: &wire::ServiceLookupAsk<'_>,
    ) -> Result<(), ()> {
        if !self.host.subscribed(asker) {
            return Ok(());
        }
        let Some(provider) = self.host.provider_owner(provider) else {
            return Ok(());
        };
        let hash = pending::ask_hash(ask.language, ask.text);
        let now = self.host.now_ms();
        let Ok(mut table) = self.pending.try_borrow_mut() else {
            return Err(());
        };
        table.record(asker, provider, hash, now).map_err(|_| ())
    }
}

/// `event.lookup_ready` (110) for one asker: `now_ms` and the provider's
/// name, as the event record. The caller (the event delivery) queues it for
/// the subscribed asker only.
pub(crate) fn ready_event(now_ms: u64, provider: &str, out: &mut [u8]) -> Result<usize, i32> {
    wire::record_encode_one(
        &Record::EventLookupReady(wire::EventLookupReady { now_ms, provider }),
        out,
    )
}

/// The `core.lookup` backend.
pub(crate) fn backend(
    frame: &dyn Frame,
    who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let service = who.hosts.lookup().ok_or(ERR_UNSUPPORTED)?;
    if op.id != 1 {
        return Err(ERR_UNSUPPORTED);
    }
    let ask: wire::ServiceLookupAsk<'_> = codec::single(input)?;
    // The text is what is looked up; the language is a BCP 47 tag or empty.
    if ask.text.is_empty() || !language_ok(ask.language) {
        return Err(ERR_INVALID);
    }
    let length = service.host.ask(frame, who.owner, &ask, out)?;
    let reply = out.get(..length).ok_or(ERR_INVALID)?;
    let entry = check(reply)?;
    if entry.flags & PENDING == 0 {
        return Ok(length);
    }
    // A provider that must fetch answered `pending`: the ask is kept for an
    // asker that wants the event; where the table has no room, the page is
    // the empty entry and the host says so in its log.
    let more = entry.flags & 1 != 0;
    if service
        .keep_pending(who.owner, entry.provider, &ask)
        .is_err()
    {
        let mut name = [0_u8; 64];
        let provider = entry.provider.as_bytes();
        let name_bytes = name.get_mut(..provider.len()).ok_or(ERR_INVALID)?;
        name_bytes.copy_from_slice(provider);
        let provider = core::str::from_utf8(name_bytes).map_err(|_| ERR_INVALID)?;
        // At most one line a minute: every ask meets the full table.
        if service.pending_full_log_due() {
            service.note("lookup: the pending table is full, empty entry");
        }
        return broker::empty(provider, more, out);
    }
    Ok(length)
}

/// A BCP 47 tag as the contract spells it (`en`, `pt-BR`, `zh-Hant-TW`):
/// 2 to 35 bytes of ASCII letters and digits in hyphen-separated subtags of
/// 1 to 8, or empty for "any".
fn language_ok(tag: &str) -> bool {
    tag.is_empty()
        || (tag.len() >= 2
            && tag.split('-').all(|subtag| {
                (1..=8).contains(&subtag.len()) && subtag.bytes().all(|b| b.is_ascii_alphanumeric())
            }))
}

fn check(reply: &[u8]) -> Result<wire::ServiceLookupEntry<'_>, i32> {
    let (record, used) = codec::decode_prefix(reply).map_err(|_| ERR_INVALID)?;
    match record {
        Record::ServiceLookupEntry(entry)
            if used == reply.len()
                && entry.flags & !ENTRY_FLAGS == 0
                && !entry.provider.is_empty() =>
        {
            Ok(entry)
        }
        _ => Err(ERR_INVALID),
    }
}

#[cfg(test)]
#[path = "lookup_service/pending_service_tests.rs"]
mod pending_service_tests;
#[cfg(test)]
#[path = "lookup_service/tests.rs"]
mod tests;
