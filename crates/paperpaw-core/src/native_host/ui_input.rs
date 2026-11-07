//! The `core.input-filter` chain (ui.md section 4.1): every key, pointer and
//! gesture goes to the filters first, one stage call per filter, before
//! screens, gestures and key maps.
//!
//! Pure policy plus one adapter. [`InputFilters`] holds the captures (at most
//! [`MAX_CAPTURES`], fixed, no allocation) and runs the chain through a
//! [`FilterRunner`]; [`BrokerRunner`] is the production runner over the stage
//! broker (one subscription per run, so a `replace` feeds the filters after
//! it). Nothing here keeps a kernel borrow, and no plugin code runs while the
//! table is borrowed by anything else: the router applies [`Routed::Run`] and
//! [`Routed::Raise`] after this returns.
//!
//! Rules kept (all from ui.md):
//! - the hold-Power escape never gets here (the router checks it first);
//! - at most [`MAX_FILTERS`] filters, in the order given (registration order);
//!   on the X4 key inputs only (no pointer, no gesture);
//! - a filter the guard rule skips is not called (`Policy::runs`);
//! - verdicts: 0 pass, 1 take, 2 run, 3 cancel, 4 replace, 5 raise. Anything
//!   else, an empty or invalid reply, a failed callback, an over-cap capture,
//!   a run without an action, a replace that is not one key, pointer or
//!   gesture record, a raise of a name the filter's plugin did not declare:
//!   pass, counted in [`Dispatched::refused`] for the log;
//! - take and cancel of a pointer down or key press capture it whole: its
//!   moves, up or release go to nothing else. The capturing filter alone is
//!   called with them (its verdict ends nothing: the input is taken); if it
//!   stops or its callback fails, the capture is released and the rest still
//!   goes nowhere, until the up or release, and the next input starts fresh
//!   with all filters;
//! - `raise` and `run` end the chain; `raise` is checked against the
//!   plugin's own declared inputs and gestures; raised inputs pass no filter
//!   (the router applies them as a chain step).
use super::{
    DispatchDepth,
    native_instances::NativeInstances,
    registrations::{self as wire, Record},
    stage_broker::{Run, Schema, Subscription},
    stages_wire::core_input_filter_v1 as stage,
};
use paperpaw_native::host_glue::{ERR_CANCELLED, ERR_FULL, ERR_INVALID, ERR_NO_SPACE};
use paperpaw_sdk::Env;

/// Filters called per input (the stage's subscriber cap).
pub(crate) const MAX_FILTERS: usize = stage::MAX_SUBSCRIBERS;
/// Pointer contacts and keys held under a capture at once.
pub(crate) const MAX_CAPTURES: usize = 8;
/// One input record, framed (`filter.input.input`, `filter.verdict.input`).
const INPUT_BYTES: usize = 128;
/// A `filter.input` batch: the input, the flags and the framing.
const PAYLOAD_BYTES: usize = 160;
/// A `filter.verdict` batch: kind, action, input, raise and the framing.
const REPLY_BYTES: usize = 384;
const KEY_BYTES: usize = 32;

/// The stage the broker runs: bounds sized to one input and one verdict, so a
/// call allocates well under a kilobyte (the stage's own caps are 3,840).
pub(crate) fn schema() -> Schema {
    Schema {
        key: stage::KEY,
        major: u32::from(stage::MAJOR),
        max_subscribers: MAX_FILTERS,
        max_input: PAYLOAD_BYTES,
        max_reply: REPLY_BYTES,
        reply_rows: 1,
        input: |bytes| stage::input_decode(bytes).map(|_| ()),
        reply: |bytes| stage::reply_decode(bytes).map(|_| ()),
    }
}

/// A short text kept by value (an action or an input name).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Short<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> Short<N> {
    fn new(text: &str) -> Option<Self> {
        let mut bytes = [0; N];
        bytes
            .get_mut(..text.len())?
            .copy_from_slice(text.as_bytes());
        Some(Self {
            bytes,
            len: text.len(),
        })
    }

    pub(crate) fn as_str(&self) -> &str {
        self.bytes
            .get(..self.len)
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .unwrap_or("")
    }
}

/// One input record, encoded as a one-record batch.
#[derive(Clone, Copy)]
pub(crate) struct Encoded {
    bytes: [u8; INPUT_BYTES],
    len: usize,
}

impl core::fmt::Debug for Encoded {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Encoded").field("len", &self.len).finish()
    }
}

impl Encoded {
    /// A key, pointer or gesture record; anything else is `ERR_INVALID`.
    fn from_record(record: &Record<'_>) -> Result<Self, i32> {
        classify(record)?;
        let mut bytes = [0; INPUT_BYTES];
        let len = wire::records_encode(core::slice::from_ref(record), &mut bytes)?;
        Ok(Self { bytes, len })
    }

    /// The same, from the bytes of a verdict's `input` (one record).
    /// It is input made by a plugin, so a key name is checked: 1 to 32 bytes
    /// without control characters (otherwise `ERR_INVALID`).
    fn from_batch(batch: &[u8]) -> Result<Self, i32> {
        let record = wire::records_decode_single(batch)?;
        if let Record::InputKey(key) = &record
            && (key.key.is_empty() || key.key.chars().any(char::is_control))
        {
            return Err(ERR_INVALID);
        }
        Self::from_record(&record)
    }

    fn batch(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or(&[])
    }

    /// The record, to hand on to the router.
    pub(crate) fn record(&self) -> Result<Record<'_>, i32> {
        wire::records_decode_single(self.batch())
    }
}

/// What an input is for capture: a key (by name), a pointer contact, or a
/// gesture (never held).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Subject {
    Key { name: [u8; KEY_BYTES], len: usize },
    Pointer(u32),
    Gesture,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Start,
    Mid,
    End,
    Once,
}

fn classify(record: &Record<'_>) -> Result<(Subject, Phase), i32> {
    match record {
        Record::InputKey(key) => {
            let mut name = [0; KEY_BYTES];
            name.get_mut(..key.key.len())
                .ok_or(ERR_INVALID)?
                .copy_from_slice(key.key.as_bytes());
            let phase = match key.state {
                0 => Phase::Start,
                1 => Phase::End,
                // A repeat is more of the held key (screens-input.md 2.2).
                2 => Phase::Mid,
                _ => return Err(ERR_INVALID),
            };
            Ok((
                Subject::Key {
                    name,
                    len: key.key.len(),
                },
                phase,
            ))
        }
        Record::InputPointer(pointer) => {
            let phase = match pointer.phase {
                0 => Phase::Start,
                1 => Phase::Mid,
                2 | 3 => Phase::End,
                _ => return Err(ERR_INVALID),
            };
            Ok((Subject::Pointer(pointer.contact.unwrap_or(0)), phase))
        }
        Record::InputGesture(_) => Ok((Subject::Gesture, Phase::Once)),
        _ => Err(ERR_INVALID),
    }
}

/// What the router does with the input.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Routed {
    /// Unchanged: on to screens, gestures and key maps.
    Pass,
    /// Changed by a `replace` (the last one): on as this input.
    Replaced(Encoded),
    /// Nothing else sees it.
    Taken,
    /// Nothing else sees it, and the router drops what follows the finger
    /// or the key (`cancel_input`).
    Cancelled,
    /// Run this action as if bound to the input.
    Run(Short<128>),
    /// Raise `input` (one of `owner`'s declared inputs or gestures).
    Raise { owner: u64, input: Short<64> },
}

/// The verdict of a dispatch and how many filters were refused (failed or
/// invalid callbacks, replies and verdicts), for the log.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Dispatched {
    pub(crate) routed: Routed,
    pub(crate) refused: usize,
}

/// What the router knows and the chain does not.
pub(crate) struct Policy<'a> {
    /// Touch inputs (pointers, gestures) are filtered; the X4 filters keys only.
    pub(crate) touch: bool,
    /// The guard rule: whether the filter of this plugin is called now
    /// (`ui_guard::filter_runs` over the stack rows).
    pub(crate) runs: &'a dyn Fn(u64) -> bool,
    /// Whether the plugin declared this `input.<plugin>.<name>` or
    /// `gesture.<plugin>.<name>`.
    pub(crate) declared: &'a dyn Fn(u64, &str) -> bool,
}

/// One stage call: the reply batch is written to `reply`, its length returned.
/// `Err` is a failed, invalid or missing callback.
pub(crate) trait FilterRunner {
    fn call(
        &mut self,
        filter: &Subscription<'_>,
        payload: &[u8],
        reply: &mut [u8],
    ) -> Result<usize, i32>;
}

/// The production runner: one subscription per stage-broker run, at depth 0,
/// before routing. The environment, registry and depth are borrowed for the
/// dispatch only.
pub(crate) struct BrokerRunner<'a, 'e> {
    pub(crate) schema: &'a Schema,
    pub(crate) depth: &'a DispatchDepth,
    pub(crate) registry: &'a NativeInstances,
    pub(crate) env: &'a Env<'e>,
}

impl FilterRunner for BrokerRunner<'_, '_> {
    fn call(
        &mut self,
        filter: &Subscription<'_>,
        payload: &[u8],
        reply: &mut [u8],
    ) -> Result<usize, i32> {
        let run = Run::prepare(
            self.schema,
            self.depth,
            payload,
            self.registry,
            core::iter::once(*filter),
        )?;
        let mut written: Option<usize> = None;
        let outcome = run.apply(
            &mut written,
            self.env,
            |target| Ok(*target),
            |target, answer| {
                reply
                    .get_mut(..answer.bytes.len())
                    .ok_or(ERR_NO_SPACE)?
                    .copy_from_slice(answer.bytes);
                *target = Some(answer.bytes.len());
                Ok(())
            },
        )?;
        match written {
            Some(length) if outcome.applied == 1 => Ok(length),
            _ => Err(ERR_CANCELLED),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Capture {
    subject: Subject,
    /// The capturing plugin; `None` once it stopped or failed (the rest of
    /// the input is swallowed without a call).
    owner: Option<u64>,
}

/// The captures; the only state of the chain.
#[derive(Debug, Default)]
pub(crate) struct InputFilters {
    captures: [Option<Capture>; MAX_CAPTURES],
}

impl InputFilters {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Captures held now.
    pub(crate) fn held(&self) -> usize {
        self.captures.iter().flatten().count()
    }

    /// The plugin stopped: whatever it captured is swallowed to its up or
    /// release, without a call.
    pub(crate) fn owner_stopped(&mut self, owner: u64) {
        for capture in self.captures.iter_mut().flatten() {
            if capture.owner == Some(owner) {
                capture.owner = None;
            }
        }
    }

    /// Forgets every capture (a reset of the input path).
    pub(crate) fn clear(&mut self) {
        self.captures = [None; MAX_CAPTURES];
    }

    fn find(&self, subject: &Subject) -> Option<usize> {
        self.captures
            .iter()
            .position(|c| c.is_some_and(|c| c.subject == *subject))
    }

    fn capture(&mut self, subject: Subject, owner: u64) -> Result<(), i32> {
        let at = self
            .find(&subject)
            .or_else(|| self.captures.iter().position(Option::is_none))
            .ok_or(ERR_FULL)?;
        self.captures[at] = Some(Capture {
            subject,
            owner: Some(owner),
        });
        Ok(())
    }

    /// Sends `input` through the filters (`filters` in registration order).
    /// `Err` only when the input is not a key, pointer or gesture record
    /// (`ERR_INVALID`): the router passes it on unfiltered.
    pub(crate) fn dispatch(
        &mut self,
        runner: &mut dyn FilterRunner,
        filters: &[Subscription<'_>],
        policy: &Policy<'_>,
        input: &Record<'_>,
    ) -> Result<Dispatched, i32> {
        let original = Encoded::from_record(input)?;
        let (subject, phase) = classify(input)?;
        let pass = |routed| Ok(Dispatched { routed, refused: 0 });
        // Moves, up and release of a captured contact or key go nowhere else.
        if matches!(phase, Phase::Mid | Phase::End)
            && let Some(at) = self.find(&subject)
        {
            return Ok(self.captured_tail(at, phase, runner, filters, policy, &original));
        }
        // A press or down while one is held for it: the up was lost; fresh start.
        if phase == Phase::Start
            && let Some(at) = self.find(&subject)
        {
            self.captures[at] = None;
        }
        if !policy.touch && !matches!(subject, Subject::Key { .. }) {
            return pass(Routed::Pass);
        }
        let mut current = original;
        let mut subject = subject;
        let mut phase = phase;
        let mut changed = false;
        let mut refused = 0;
        for filter in filters.iter().filter(|f| f.enabled).take(MAX_FILTERS) {
            if !(policy.runs)(filter.plugin) {
                continue;
            }
            let Some(verdict) = ask(runner, filter, &current) else {
                refused += 1;
                continue;
            };
            let routed = match verdict {
                Verdict::Pass => continue,
                Verdict::Take | Verdict::Cancel => {
                    if phase == Phase::Start && self.capture(subject, filter.plugin).is_err() {
                        refused += 1;
                        continue;
                    }
                    if matches!(verdict, Verdict::Take) {
                        Routed::Taken
                    } else {
                        Routed::Cancelled
                    }
                }
                Verdict::Run(action) => match Short::new(action.as_str()) {
                    Some(action) if !action.as_str().is_empty() => Routed::Run(action),
                    _ => {
                        refused += 1;
                        continue;
                    }
                },
                Verdict::Replace(next) => {
                    match next.record().and_then(|record| classify(&record)) {
                        Ok((next_subject, next_phase)) => {
                            current = next;
                            subject = next_subject;
                            phase = next_phase;
                            changed = true;
                        }
                        Err(_) => refused += 1,
                    }
                    continue;
                }
                Verdict::Raise(name) => {
                    if name.as_str().is_empty() || !(policy.declared)(filter.plugin, name.as_str())
                    {
                        refused += 1;
                        continue;
                    }
                    Routed::Raise {
                        owner: filter.plugin,
                        input: name,
                    }
                }
                Verdict::Invalid => {
                    refused += 1;
                    continue;
                }
            };
            return Ok(Dispatched { routed, refused });
        }
        Ok(Dispatched {
            routed: if changed {
                Routed::Replaced(current)
            } else {
                Routed::Pass
            },
            refused,
        })
    }

    /// The rest of a captured input: only the capturing filter hears it (to
    /// follow the finger or key); the input is taken whatever it answers. A
    /// failed call, or a filter no longer there, releases the capture to the
    /// up or release.
    fn captured_tail(
        &mut self,
        at: usize,
        phase: Phase,
        runner: &mut dyn FilterRunner,
        filters: &[Subscription<'_>],
        policy: &Policy<'_>,
        input: &Encoded,
    ) -> Dispatched {
        let mut refused = 0;
        if let Some(capture) = self.captures[at].as_mut()
            && let Some(owner) = capture.owner
        {
            let filter = filters
                .iter()
                .filter(|f| f.enabled)
                .take(MAX_FILTERS)
                .find(|f| f.plugin == owner);
            match filter {
                Some(filter) if (policy.runs)(owner) => {
                    if ask(runner, filter, input).is_none() {
                        refused += 1;
                        capture.owner = None;
                    }
                }
                Some(_) => {}
                None => capture.owner = None,
            }
        }
        if phase == Phase::End {
            self.captures[at] = None;
        }
        Dispatched {
            routed: Routed::Taken,
            refused,
        }
    }
}

enum Verdict {
    Pass,
    Take,
    Run(Short<128>),
    Cancel,
    Replace(Encoded),
    Raise(Short<64>),
    /// Not a verdict the host knows, or one that does not fit.
    Invalid,
}

/// One filter call and the verdict it answers; `None` for a failed callback or
/// a reply that is not exactly one `filter.verdict`.
fn ask(
    runner: &mut dyn FilterRunner,
    filter: &Subscription<'_>,
    input: &Encoded,
) -> Option<Verdict> {
    let mut payload = [0; PAYLOAD_BYTES];
    let length = stage::FilterInput::new(input.batch(), 0)
        .encode(&mut payload)
        .ok()?;
    let mut reply = [0; REPLY_BYTES];
    let replied = runner
        .call(filter, payload.get(..length)?, &mut reply)
        .ok()?;
    let verdict = stage::FilterVerdict::decode(reply.get(..replied)?).ok()?;
    Some(match verdict.kind {
        0 => Verdict::Pass,
        1 => Verdict::Take,
        2 => Short::new(verdict.action).map_or(Verdict::Invalid, Verdict::Run),
        3 => Verdict::Cancel,
        4 => Encoded::from_batch(verdict.input).map_or(Verdict::Invalid, Verdict::Replace),
        5 => Short::new(verdict.raise).map_or(Verdict::Invalid, Verdict::Raise),
        _ => Verdict::Invalid,
    })
}

#[cfg(test)]
#[path = "ui_input_tests.rs"]
mod tests;
