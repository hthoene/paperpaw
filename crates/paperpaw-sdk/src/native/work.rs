//! Deferred and background work from `core.work` major 1 (`docs/abi/work.md`).
//!
//! Work is a named wish to be called later: a plugin declares a key, a token
//! and when it should run, and the host delivers `event.work_due` with that
//! token when it is due. There are no handles and no stored code; a plugin
//! declares again for a next time, and again at start for what it still has
//! to do. The queue lives for one start of the device.
//!
//! Every call validates what the contract validates before the host call, so a
//! refusal costs no round trip, and uses only the caller's buffers: nothing is
//! allocated.
//!
//! ```
//! use paperpaw_sdk::native::work::WorkSpec;
//! // The weekly look: network work, only when cheap, at most a week apart.
//! let spec = WorkSpec::when_cheap("update.look", 7)
//!     .unwrap()
//!     .after_ms(86_400_000)
//!     .every_ms(7 * 86_400_000);
//! assert!(spec.check().is_ok());
//! ```
use super::wire::services_wire::{ServiceOp, core_work_v1 as schema};
use super::{
    Error, Host,
    declare::records_wire::{self as wire, RawRecord, WireEncode, WireRecord},
    services::Service,
};
use core::fmt;

/// Longest key, in bytes.
pub const KEY_MAX_BYTES: usize = 64;
/// Longest `after_ms`, `within_ms` and `every_ms`: seven days.
pub const SPAN_MAX_MS: u32 = 604_800_000;
/// Shortest `every_ms`, one minute. Exact timers (`at_ms`) repeat more often.
pub const EVERY_MIN_MS: u32 = 60_000;
/// Most rows `state` returns (the most one plugin may have on a large device).
pub const STATE_MAX_ROWS: usize = 32;
/// The first wait after a failed attempt, in milliseconds.
pub const RETRY_FIRST_MS: u32 = 30_000;
/// The longest wait between attempts, in milliseconds (six hours).
pub const RETRY_MOST_MS: u32 = 21_600_000;

const NEEDS_NETWORK: u32 = 1;
const FLAG_OPTIONAL: u32 = 1;
const MOMENT_WAKE: u32 = 1;
const MOMENT_ONLINE: u32 = 1 << 1;
const MOMENT_CHARGING: u32 = 1 << 2;

/// A moment at which work with no deadline of its own may come due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Moment {
    /// The device woke (`event.resume`).
    Wake,
    /// The network came up.
    Online,
    /// Charging began.
    Charging,
}
impl Moment {
    const fn bit(self) -> u32 {
        match self {
            Self::Wake => MOMENT_WAKE,
            Self::Online => MOMENT_ONLINE,
            Self::Charging => MOMENT_CHARGING,
        }
    }
}

/// One piece of work to declare: when it comes due and what it needs.
///
/// Built with the setters, checked by [`WorkSpec::check`] and by
/// [`Work::declare`] with the contract's rules (`docs/abi/work.md` section
/// 1.1). A key declared again replaces the work, its times counted from now.
#[derive(Clone, Copy, PartialEq, Eq)]
#[must_use = "declare the work with `Work::declare`"]
pub struct WorkSpec<'a> {
    key: &'a str,
    token: u32,
    network: bool,
    optional: bool,
    moments: u32,
    after_ms: u32,
    within_ms: u32,
    every_ms: u32,
    at_ms: u64,
}
impl fmt::Debug for WorkSpec<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkSpec")
            .field("key", &self.key)
            .field("token", &self.token)
            .field("network", &self.network)
            .field("optional", &self.optional)
            .field("moments", &self.moments)
            .field("after_ms", &self.after_ms)
            .field("within_ms", &self.within_ms)
            .field("every_ms", &self.every_ms)
            .field("at_ms", &self.at_ms)
            .finish()
    }
}
impl<'a> WorkSpec<'a> {
    /// Work due at once: `key` names it (1 to [`KEY_MAX_BYTES`] bytes, no
    /// control characters) and `token` comes back in `event.work_due`.
    ///
    /// # Errors
    /// [`Error::Invalid`] for a bad key.
    pub fn new(key: &'a str, token: u32) -> Result<Self, Error> {
        if key.is_empty() || key.len() > KEY_MAX_BYTES || key.chars().any(char::is_control) {
            return Err(Error::Invalid);
        }
        Ok(Self {
            key,
            token,
            network: false,
            optional: false,
            moments: 0,
            after_ms: 0,
            within_ms: 0,
            every_ms: 0,
            at_ms: 0,
        })
    }

    /// Network work that runs only when that is cheap: `network()` and
    /// `optional()`. The host runs it at a moment, while the radio is on
    /// anyway.
    ///
    /// # Errors
    /// [`Error::Invalid`] for a bad key.
    pub fn when_cheap(key: &'a str, token: u32) -> Result<Self, Error> {
        Ok(Self::new(key, token)?.network().optional())
    }

    /// An exact timer: due at `at_ms` on the host's monotonic clock
    /// (`now_ms` of events; a time in the past is due at once). It excludes
    /// every other timing and `optional`.
    ///
    /// `at_ms` 0 means "no exact time" on the wire, so `at(.., 0)` is a work
    /// with nothing set: it is due at once, like [`WorkSpec::new`]. Use a
    /// real `now_ms` (or any non-zero time in the past) for "due now".
    ///
    /// # Errors
    /// [`Error::Invalid`] for a bad key.
    pub fn at(key: &'a str, token: u32, at_ms: u64) -> Result<Self, Error> {
        let mut spec = Self::new(key, token)?;
        spec.at_ms = at_ms;
        Ok(spec)
    }

    /// The work needs the network (and the plugin `uses.network`).
    pub const fn network(mut self) -> Self {
        self.network = true;
        self
    }
    /// Only when cheap: no deadline, and at a moment while the radio is on.
    pub const fn optional(mut self) -> Self {
        self.optional = true;
        self
    }
    /// Not before `ms` from now.
    pub const fn after_ms(mut self, ms: u32) -> Self {
        self.after_ms = ms;
        self
    }
    /// At the latest `ms` after `after_ms`, even without a moment.
    pub const fn within_ms(mut self, ms: u32) -> Self {
        self.within_ms = ms;
        self
    }
    /// Due again `ms` after a run ended.
    pub const fn every_ms(mut self, ms: u32) -> Self {
        self.every_ms = ms;
        self
    }
    /// Also comes due at this moment (the first one after `after_ms`).
    pub const fn at_moment(mut self, moment: Moment) -> Self {
        self.moments |= moment.bit();
        self
    }
    /// Not before `retry_delay_ms(attempt, jitter)` from now (see there).
    pub const fn retry(self, attempt: u32, jitter: u8) -> Self {
        self.after_ms(retry_delay_ms(attempt, jitter))
    }

    /// The key.
    #[must_use]
    pub const fn key(&self) -> &'a str {
        self.key
    }
    /// The token given back in the event.
    #[must_use]
    pub const fn token(&self) -> u32 {
        self.token
    }

    /// Check the contract's rules without calling the host.
    ///
    /// # Errors
    /// [`Error::Invalid`] when an exact timer is combined with another timing
    /// or `optional`, when `after_ms`, `within_ms` or `every_ms` is over
    /// [`SPAN_MAX_MS`], or `every_ms` is under [`EVERY_MIN_MS`] and not 0.
    pub fn check(&self) -> Result<(), Error> {
        let timed = self.after_ms != 0 || self.within_ms != 0 || self.moments != 0 || self.optional;
        if self.at_ms != 0 && timed {
            return Err(Error::Invalid);
        }
        if self.after_ms > SPAN_MAX_MS
            || self.within_ms > SPAN_MAX_MS
            || self.every_ms > SPAN_MAX_MS
            || (self.every_ms != 0 && self.every_ms < EVERY_MIN_MS)
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    fn record(&self) -> wire::ServiceWorkSpec<'a> {
        wire::ServiceWorkSpec {
            token: self.token,
            needs: if self.network { NEEDS_NETWORK } else { 0 },
            flags: if self.optional { FLAG_OPTIONAL } else { 0 },
            moments: self.moments,
            after_ms: self.after_ms,
            within_ms: self.within_ms,
            every_ms: self.every_ms,
            at_ms: self.at_ms,
            key: self.key,
        }
    }
}

/// How long to wait before attempt `attempt` (the first is 1) after a
/// failure: [`RETRY_FIRST_MS`] doubling up to [`RETRY_MOST_MS`], plus up to a
/// tenth more by `jitter` (0 to 255, for instance a random byte), so that
/// devices do not all come back at once.
#[must_use]
pub const fn retry_delay_ms(attempt: u32, jitter: u8) -> u32 {
    let shift = attempt.saturating_sub(1);
    let shift = if shift > 20 { 20 } else { shift };
    let doubled = (RETRY_FIRST_MS as u64) << shift;
    let base = if doubled > RETRY_MOST_MS as u64 {
        RETRY_MOST_MS as u64
    } else {
        doubled
    };
    (base + base / 10 * jitter as u64 / 255) as u32
}

/// Why held work waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Hold {
    /// The offline mode holds network work until it is left.
    Offline,
    /// The saving mode, with the plugin's saving setting on manual.
    Saving,
    /// A reason this SDK does not know, as the host numbered it.
    Other(u32),
}

/// Where a declared work is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Phase {
    /// Declared and not yet due.
    Waiting,
    /// Held back (see [`WorkRow::hold`]).
    Held,
    /// Its due callback ran and its requests or sockets are still open.
    Running,
}

/// One declared work of this plugin, read from [`Work::state`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WorkRow<'a> {
    key: &'a str,
    token: u32,
    phase: Phase,
    hold: Option<Hold>,
    due_ms: Option<u64>,
}
impl fmt::Debug for WorkRow<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkRow")
            .field("key", &self.key)
            .field("token", &self.token)
            .field("phase", &self.phase)
            .field("hold", &self.hold)
            .field("due_ms", &self.due_ms)
            .finish()
    }
}
impl<'a> WorkRow<'a> {
    fn decode(record: RawRecord<'a>) -> Result<Self, Error> {
        let row = record
            .decode::<wire::ServiceWorkRow<'a>>()
            .map_err(|_| Error::Invalid)?;
        let phase = match row.state {
            0 => Phase::Waiting,
            1 => Phase::Held,
            2 => Phase::Running,
            _ => return Err(Error::Invalid),
        };
        let hold = match (phase, row.reason) {
            (Phase::Held, 11) => Some(Hold::Offline),
            (Phase::Held, 12) => Some(Hold::Saving),
            (Phase::Held, other) => Some(Hold::Other(other)),
            // A reason belongs to held work only.
            (_, 0) => None,
            _ => return Err(Error::Invalid),
        };
        Ok(Self {
            key: row.key,
            token: row.token,
            phase,
            hold,
            due_ms: (row.due_ms != 0).then_some(row.due_ms),
        })
    }
    /// The key it was declared with.
    #[must_use]
    pub const fn key(&self) -> &'a str {
        self.key
    }
    /// The token it was declared with.
    #[must_use]
    pub const fn token(&self) -> u32 {
        self.token
    }
    /// Where it is.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }
    /// Why it is held; `None` unless [`Phase::Held`].
    #[must_use]
    pub const fn hold(&self) -> Option<Hold> {
        self.hold
    }
    /// When it comes due by time, on the host's monotonic clock; `None` when
    /// only a moment brings it.
    #[must_use]
    pub const fn due_ms(&self) -> Option<u64> {
        self.due_ms
    }
}

/// The plugin's declared work, completely validated, borrowing the reply.
///
/// ```compile_fail
/// use paperpaw_sdk::native::{Host, work::{Work, WorkList}};
/// fn keep<'a>(work: &Work, host: &Host<'_>, input: &mut [u8], out: &'a mut [u8])
///     -> WorkList<'a> {
///     let list = work.state(host, input, out).unwrap();
///     out.fill(0); // The list still borrows this reply buffer.
///     list
/// }
/// ```
#[derive(Clone, Copy)]
pub struct WorkList<'a> {
    bytes: &'a [u8],
    len: usize,
}
impl fmt::Debug for WorkList<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkList")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}
impl<'a> WorkList<'a> {
    /// Number of declared works.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }
    /// Whether the plugin has no work declared.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// The works in declaration order.
    pub fn rows(&self) -> WorkRows<'a> {
        WorkRows {
            bytes: self.bytes,
            remaining: self.len,
        }
    }
    /// The row declared with `key`, if any.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<WorkRow<'a>> {
        self.rows().find(|row| row.key == key)
    }
}

/// Iterator over already validated rows.
#[derive(Clone)]
pub struct WorkRows<'a> {
    bytes: &'a [u8],
    remaining: usize,
}
impl fmt::Debug for WorkRows<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkRows")
            .field("remaining", &self.remaining)
            .finish_non_exhaustive()
    }
}
impl<'a> Iterator for WorkRows<'a> {
    type Item = WorkRow<'a>;
    fn next(&mut self) -> Option<WorkRow<'a>> {
        if self.remaining == 0 {
            return None;
        }
        let (record, len) = RawRecord::prefix(self.bytes).ok()?;
        self.bytes = self.bytes.get(len..)?;
        self.remaining -= 1;
        WorkRow::decode(record).ok()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}
impl ExactSizeIterator for WorkRows<'_> {}

/// Why work came due (`event.work_due.why`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Why {
    /// Its time came (`at_ms`, or `after_ms` with no moment and no deadline).
    Time,
    /// Its deadline (`within_ms`) came.
    Deadline,
    /// The device woke.
    Wake,
    /// The network came up.
    Online,
    /// Charging began.
    Charging,
    /// A reason a newer host added.
    Other(u32),
}

impl Why {
    const fn from_wire(value: u32) -> Self {
        match value {
            0 => Self::Time,
            1 => Self::Deadline,
            2 => Self::Wake,
            3 => Self::Online,
            4 => Self::Charging,
            other => Self::Other(other),
        }
    }
}

/// Work came due: `event.work_due`. Opens a run (`docs/abi/work.md` 2.3): the
/// requests and sockets started in this callback belong to it. A once-work is
/// removed when delivered; a repeating one waits for its next time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkDue<'a> {
    now_ms: u64,
    token: u32,
    why: Why,
    key: &'a str,
}

impl<'a> WorkDue<'a> {
    pub(crate) fn decode(r: wire::EventWorkDue<'a>) -> Self {
        Self {
            now_ms: r.now_ms,
            token: r.token,
            why: Why::from_wire(r.why),
            key: r.key,
        }
    }

    /// The host's clock.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// The token the work was declared with.
    #[must_use]
    pub const fn token(&self) -> u32 {
        self.token
    }

    /// Why it came due.
    #[must_use]
    pub const fn why(&self) -> Why {
        self.why
    }

    /// The key the work was declared with.
    #[must_use]
    pub const fn key(&self) -> &'a str {
        self.key
    }
}

/// Why work was taken out for good (`event.work_refused.reason`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The user turned the plugin's network off while the work waited.
    NetworkOff,
    /// A reason a newer host added.
    Other(u32),
}

/// Work was taken out for good: `event.work_refused`. The only refusal after
/// `declare`; declare again if the work is still wanted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkRefused<'a> {
    now_ms: u64,
    token: u32,
    reason: Refusal,
    key: &'a str,
}

impl<'a> WorkRefused<'a> {
    pub(crate) fn decode(r: wire::EventWorkRefused<'a>) -> Self {
        Self {
            now_ms: r.now_ms,
            token: r.token,
            reason: match r.reason {
                13 => Refusal::NetworkOff,
                other => Refusal::Other(other),
            },
            key: r.key,
        }
    }

    /// The host's clock.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// The token the work was declared with.
    #[must_use]
    pub const fn token(&self) -> u32 {
        self.token
    }

    /// Why it was taken out.
    #[must_use]
    pub const fn reason(&self) -> Refusal {
        self.reason
    }

    /// The key the work was declared with.
    #[must_use]
    pub const fn key(&self) -> &'a str {
        self.key
    }
}

/// A work service handle, reusable across callbacks until explicitly released.
#[derive(Debug)]
#[must_use = "release the work service when done"]
pub struct Work {
    service: Service,
}
impl Work {
    /// Open the work service this plugin's metadata declares.
    ///
    /// # Errors
    /// The host's refusal for schema, permission or resources.
    pub fn open(host: &Host<'_>) -> Result<Self, Error> {
        Ok(Self {
            service: Service::open(host, schema::KEY, schema::MAJOR, 0)?,
        })
    }

    /// Release the handle using this callback's host.
    ///
    /// # Errors
    /// The host's refusal for a stale or foreign handle.
    pub fn release(self, host: &Host<'_>) -> Result<(), Error> {
        self.service.release(host)
    }

    /// Declare `spec`, replacing the work with the same key. It runs only
    /// while the device is awake, and is lost at the next start: declare it
    /// again then.
    ///
    /// `input` holds the encoded record (256 bytes are enough). The reply is
    /// empty, but `out` must hold at least 64 bytes: the host checks the
    /// op's `max_reply` (64, `work.md` 1) and answers `NoSpace` for less.
    ///
    /// The plugin must have subscribed to
    /// [`EventKind::WorkDue`](super::declare::EventKind::WorkDue) with its
    /// registration first; without it the host answers `State` (`work.md` 3).
    ///
    /// Declared while work is delivered (in a `work_due` callback or any other
    /// callback of that pass), the work is due at the next pass at the
    /// earliest, never in the same one. Any work declared from a run's
    /// callbacks (its own key or another) is due no sooner than 1 s after the
    /// declaration, whatever its times say.
    ///
    /// # Errors
    /// [`Error::Invalid`] for what [`WorkSpec::check`] refuses (nothing is
    /// sent); [`Error::NoSpace`] when `input` is too small or `out` under 64
    /// bytes; the host's refusals: `Denied` for network work without
    /// `uses.network` or with the plugin's network turned off, `Full` above
    /// the plugin's or the device's limits, `NoMemory`, `Unsupported` for a
    /// bit this host does not know, `State` outside a callback or without the
    /// `WorkDue` subscription.
    pub fn declare(
        &self,
        host: &Host<'_>,
        spec: &WorkSpec<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        spec.check()?;
        self.mutate(host, schema::OP_DECLARE, &spec.record(), input, out)
    }

    /// Remove the work declared with `key`. A missing key is not an error,
    /// and a work whose event is already delivered is not called back.
    /// `out` must hold at least 64 bytes, as for [`Self::declare`].
    ///
    /// # Errors
    /// [`Error::Invalid`] for a bad key (nothing is sent); the host's
    /// refusals, `NoSpace` for an `out` under 64 bytes.
    pub fn cancel(
        &self,
        host: &Host<'_>,
        key: &str,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        WorkSpec::new(key, 0).map(drop)?;
        self.mutate(
            host,
            schema::OP_CANCEL,
            &wire::ServiceWorkKey { key },
            input,
            out,
        )
    }

    /// Remove all of the plugin's work. `out` must hold at least 64 bytes,
    /// as for [`Self::declare`].
    ///
    /// # Errors
    /// The host's refusals, `NoSpace` for an `out` under 64 bytes.
    pub fn cancel_all(
        &self,
        host: &Host<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        self.mutate(
            host,
            schema::OP_CANCEL,
            &wire::ServiceWorkKey { key: "" },
            input,
            out,
        )
    }

    /// The plugin's own declared work, at most [`STATE_MAX_ROWS`], in
    /// declaration order. `out` needs 4,096 bytes. A plugin checks it before
    /// declaring again what may still wait.
    ///
    /// # Errors
    /// The host's refusals; [`Error::Invalid`] for a reply that is not rows
    /// and one end, or has more rows than the contract allows.
    pub fn state<'a>(
        &self,
        host: &Host<'_>,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<WorkList<'a>, Error> {
        let (bytes, operation) = self.call(host, schema::OP_STATE, &[], input, out)?;
        validate_state(bytes, operation)
    }

    fn mutate(
        &self,
        host: &Host<'_>,
        op: u32,
        record: &dyn WireEncode,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let (reply, _) = self.call(host, op, &[record], input, out)?;
        // The reply of `declare` and `cancel` is empty.
        if reply.is_empty() {
            Ok(())
        } else {
            Err(Error::Invalid)
        }
    }

    fn call<'a>(
        &self,
        host: &Host<'_>,
        op: u32,
        records: &[&dyn WireEncode],
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<(&'a [u8], &ServiceOp), Error> {
        let operation = self.service.operation(op).ok_or(Error::Unsupported)?;
        let cap = input.len().min(operation.max_input);
        let len = wire::records_encode_counted_of(
            records.iter().copied(),
            usize::from(operation.input_rows),
            operation.max_input,
            &mut input[..cap],
        )
        .map_err(Error::from_status)?;
        let len = self.service.call(host, op, &input[..len], out)?;
        Ok((out.get(..len).ok_or(Error::Invalid)?, operation))
    }
}

fn validate_state<'a>(bytes: &'a [u8], op: &ServiceOp) -> Result<WorkList<'a>, Error> {
    let cap = usize::from(op.reply_rows).min(STATE_MAX_ROWS);
    let mut rest = bytes;
    let mut count = 0usize;
    loop {
        let (record, len) = RawRecord::prefix(rest).map_err(|_| Error::Invalid)?;
        rest = rest.get(len..).ok_or(Error::Invalid)?;
        if record.tag() == wire::ServiceEnd::TAG {
            let end = record
                .decode::<wire::ServiceEnd>()
                .map_err(|_| Error::Invalid)?;
            let rows = u32::try_from(count).map_err(|_| Error::Invalid)?;
            // The unpaged end (`stages-services.md` 2.3).
            if !rest.is_empty()
                || end.flags != 0
                || end.next != 0
                || end.generation != 0
                || end.total != rows
            {
                return Err(Error::Invalid);
            }
            return Ok(WorkList { bytes, len: count });
        }
        count = count.checked_add(1).ok_or(Error::Invalid)?;
        if count > cap {
            return Err(Error::Invalid);
        }
        WorkRow::decode(record)?;
    }
}

#[cfg(test)]
mod tests;
