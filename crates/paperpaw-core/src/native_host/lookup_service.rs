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
//! scratch (a `lookup_entry` is at most 2,514 bytes).
use super::{
    host_services::Caller,
    registrations::{self as wire, Record},
};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use crate::native_host::{codec, declaration_store::DeclarationStore, host_services::HostServices};
use alloc::{rc::Rc, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_UNSUPPORTED};

mod broker;
mod pending;
mod providers;
mod ready;

pub(crate) use pending::{PendingAsks, ask_hash};
pub(crate) use providers::Providers;
use ready::provider_name;

/// `lookup_entry.flags`: bit 0 more pages follow, bit 1 pending (the provider
/// answers later).
const ENTRY_FLAGS: u32 = 0b11;
/// `lookup_entry.flags` bit 1.
const PENDING: u32 = 0b10;
/// The longest provider name (`lookup_entry.provider`, `event.lookup_ready`).
const NAME_MAX: usize = 64;

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

    /// The owner of the live plugin that provides the lookup `name`, the
    /// first in load order (two plugins may use one name: the wrong one costs
    /// one more ask); `None` where there is none (or the host cannot say: no
    /// ask is then kept as pending, and the asker asks again itself).
    fn provider_owner(&self, frame: &dyn Frame, name: &str) -> Option<u64> {
        let mut owner = None;
        let _ = frame.with_declarations(&mut |any| {
            if let Some(store) = any.downcast_ref::<DeclarationStore>() {
                store.each_lookup(&mut |decl| {
                    if owner.is_none() && decl.spec.name == name {
                        owner = Some(decl.owner);
                    }
                });
            }
        });
        owner
    }

    /// Whether `asker` subscribes to `event.lookup_ready` (110): only such an
    /// asker has its pending asks kept. The host's owner directory says.
    fn subscribed(&self, hosts: &HostServices, asker: u64) -> bool {
        hosts
            .owners()
            .is_ok_and(|owners| owners.subscribed(asker, wire::EVENT_LOOKUP_READY_TAG))
    }

    /// The name `event.lookup_ready` gives the provider `owner`: its first
    /// lookup in `store` ([`provider_name`]); `None` without one.
    fn provider_name(
        &self,
        store: Option<&DeclarationStore>,
        owner: u64,
    ) -> Option<([u8; NAME_MAX], u8)> {
        store.and_then(|store| provider_name(store, owner))
    }

    /// The clock the 60-second deadlines run on, in milliseconds; 0 where the
    /// host keeps none (the kernel's, told by the event pump, then counts).
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
    /// The table of pending asks, made at the first ask that is kept or the
    /// first ready command (`table`): a device that never waits for a
    /// provider keeps none.
    pending: RefCell<Option<alloc::boxed::Box<PendingAsks>>>,
    /// The `event.lookup_ready` waiting for their askers (`ready.rs`): at
    /// most one per pending ask, so at most the table's size; empty (no
    /// allocation) while none waits.
    events: RefCell<Vec<ready::Waiting>>,
    /// The kernel's clock, as the event pump last told it.
    clock: Cell<u64>,
}

impl LookupServices {
    pub(crate) fn new(
        host: Rc<dyn LookupHost>,
        log: crate::native_host::device_log::DeviceLog,
    ) -> Self {
        Self {
            host,
            log,
            pending: RefCell::new(None),
            events: RefCell::new(Vec::new()),
            clock: Cell::new(0),
        }
    }

    /// The table in `slot`, made first if there is none; `None` where it
    /// cannot be allocated.
    fn table(slot: &mut Option<alloc::boxed::Box<PendingAsks>>) -> Option<&mut PendingAsks> {
        if slot.is_none() {
            *slot = crate::native_host::shared::try_box(PendingAsks::new());
        }
        slot.as_deref_mut()
    }

    /// Whether the full-table line is due (once a minute).
    fn pending_full_log_due(&self) -> bool {
        let now = self.now_ms();
        self.pending
            .try_borrow_mut()
            .is_ok_and(|mut table| table.as_mut().is_some_and(|table| table.full_log_due(now)))
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
    #[cfg(test)]
    pub(crate) fn ready(
        &self,
        provider: u64,
        language: &str,
        text: &str,
        deliver: &mut dyn FnMut(u64),
    ) {
        self.ready_hash(provider, ask_hash(language, text), deliver);
    }

    /// As [`Self::ready`], for the ask's [`ask_hash`] (what the command
    /// stage keeps of `command.lookup_ready`).
    pub(crate) fn ready_hash(&self, provider: u64, hash: u64, deliver: &mut dyn FnMut(u64)) {
        let now = self.now_ms();
        // Take the askers out and let go of the table before delivering, so
        // delivery may stop a plugin or record a new ask.
        let (askers, count, log_due) = {
            let Ok(mut slot) = self.pending.try_borrow_mut() else {
                return;
            };
            let Some(table) = Self::table(&mut slot) else {
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
        if let Ok(mut slot) = self.pending.try_borrow_mut()
            && let Some(table) = slot.as_mut()
        {
            table.stopped(owner);
        }
        self.drop_events_of(owner);
    }

    /// The name the events of `owner`'s ready commands carry, as the host
    /// finds it in `store`.
    pub(crate) fn provider_name_of(
        &self,
        store: Option<&DeclarationStore>,
        owner: u64,
    ) -> Option<([u8; NAME_MAX], u8)> {
        self.host.provider_name(store, owner)
    }

    /// The clock of the deadlines: the host's, else the kernel's.
    fn now_ms(&self) -> u64 {
        match self.host.now_ms() {
            0 => self.clock.get(),
            now => now,
        }
    }

    /// How many ready commands matched no pending ask, since the start.
    #[cfg(test)]
    pub(crate) fn unmatched(&self) -> u32 {
        self.pending
            .borrow()
            .as_ref()
            .map_or(0, |table| table.unmatched())
    }

    /// The pending asks kept now.
    #[cfg(test)]
    pub(crate) fn pending_len(&self) -> usize {
        self.pending
            .borrow()
            .as_ref()
            .map_or(0, |table| table.len())
    }

    /// Keeps the ask whose answer the provider gave as pending, for an asker
    /// that subscribes to `event.lookup_ready`. `Err(())`: the table has no
    /// room (the asker gets the empty entry, and the host logs it).
    fn keep_pending(
        &self,
        frame: &dyn Frame,
        who: &Caller<'_>,
        provider: &str,
        ask: &wire::ServiceLookupAsk<'_>,
    ) -> Result<(), ()> {
        let asker = who.owner;
        if !self.host.subscribed(who.hosts, asker) {
            return Ok(());
        }
        let Some(provider) = self.host.provider_owner(frame, provider) else {
            return Ok(());
        };
        let hash = ask_hash(ask.language, ask.text);
        let now = self.now_ms();
        let Ok(mut slot) = self.pending.try_borrow_mut() else {
            return Err(());
        };
        // No room for the table is no room for the ask.
        let table = Self::table(&mut slot).ok_or(())?;
        table.record(asker, provider, hash, now).map_err(|_| ())
    }
}

/// `event.lookup_ready` (110) for one asker: `now_ms` and the provider's
/// name, as the one-record batch a callback decodes (`codec::encode_event`;
/// the event pump reads its tag to find the subscriber).
pub(crate) fn ready_event(now_ms: u64, provider: &str, out: &mut [u8]) -> Result<usize, i32> {
    codec::encode_event(
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
        .keep_pending(frame, who, entry.provider, &ask)
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
pub(crate) fn language_ok(tag: &str) -> bool {
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
#[path = "lookup_service/ready_tests.rs"]
mod ready_tests;
#[cfg(test)]
#[path = "lookup_service/tests.rs"]
mod tests;
