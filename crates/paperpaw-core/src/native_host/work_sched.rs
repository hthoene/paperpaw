//! The work table and scheduler behind `core.work@1` (`docs/abi/work.md`).
//!
//! A plugin declares named work (a key, a token, what it needs and when); the
//! host delivers `event.work_due` when it is due, while the device is awake,
//! and the work lives for one start. This module is the whole decision: the
//! table and its limits, the rules of `declare`, when work comes due (times,
//! moments, deadlines and their stretch, exact timers), what holds it (the
//! offline and saving modes, a real-time screen, the network), the
//! device-wide 15-minute radio gap, and the runs a delivery opens (at most 10
//! minutes). It never calls plugin code and never reads a clock itself: a
//! [`WorkEnv`] tells it the time and the device's state at each pass, so every
//! timing rule is tested against a fake clock.
//!
//! Memory: one entry per work (about 100 bytes with its key), allocated
//! fallibly at `declare` (`ERR_NO_MEMORY`), at most 8 per plugin and 32
//! device-wide on the X4 (about 3.2 KiB) and 32 / 256 elsewhere; a run is 32
//! bytes, at most as many as the device's works. Nothing allocates in a
//! pass: it only marks entries.
use alloc::{boxed::Box, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{
    ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED,
};

/// The longest a run lasts, from its `work_due`.
pub(crate) const RUN_MAX_MS: u64 = 600_000;
/// The device-wide gap between two radio wishes from deadlines.
pub(crate) const NETWORK_GAP_MS: u64 = 900_000;
/// How long a run past its longest time may wait for the end events of what
/// it started to be delivered; then it is over whatever it still counts (a
/// service that never delivers must not keep a run, and so a work, for ever).
pub(crate) const RUN_END_GRACE_MS: u64 = 30_000;
/// Work declared from a run's callbacks is due no sooner than this later.
pub(crate) const RUN_DECLARE_FLOOR_MS: u64 = 1_000;
/// The longest `after_ms`, `within_ms` and `every_ms` (7 days).
pub(crate) const MAX_SPAN_MS: u32 = 604_800_000;
/// The shortest `every_ms`.
pub(crate) const MIN_EVERY_MS: u32 = 60_000;
/// The longest key.
pub(crate) const MAX_KEY: usize = 64;

/// `work_row.reason` and `work_refused.reason`: numbers of `network.md` 4.1.
pub(crate) const REASON_OFFLINE: u32 = 11;
pub(crate) const REASON_SAVING: u32 = 12;
pub(crate) const REFUSED_NETWORK_OFF: u32 = 13;
/// How a run's requests and sockets end when the run does.
pub(crate) const REQUEST_WORK_ENDED: u32 = 14;
pub(crate) const SOCKET_WORK_ENDED: u32 = 27;

/// `event.work_due.why`.
pub(crate) mod why {
    pub(crate) const TIME: u32 = 0;
    pub(crate) const DEADLINE: u32 = 1;
    pub(crate) const WAKE: u32 = 2;
    pub(crate) const ONLINE: u32 = 3;
    pub(crate) const CHARGING: u32 = 4;
}

/// A moment (`work_spec.moments` bit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Moment {
    Wake,
    Online,
    Charging,
}

impl Moment {
    const fn bit(self) -> u8 {
        match self {
            Self::Wake => 1,
            Self::Online => 2,
            Self::Charging => 4,
        }
    }
    const fn why(self) -> u32 {
        match self {
            Self::Wake => why::WAKE,
            Self::Online => why::ONLINE,
            Self::Charging => why::CHARGING,
        }
    }
}

/// The power level (`event.power_changed.level`); others count as normal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Power {
    /// 0 normal, 1 saving, 2 offline.
    pub(crate) level: u32,
    /// How many times longer a deadline may wait (0 and 1: not at all).
    pub(crate) stretch: u32,
    /// The radio is on anyway (`dev_power.flags` bit 1).
    pub(crate) radio_on: bool,
}

/// What the scheduler asks the device at each pass. The shell implements it.
pub(crate) trait WorkEnv {
    /// The monotonic clock, in milliseconds, valid within one start.
    fn now_ms(&self) -> u64;
    fn power(&self) -> Power;
    /// A real-time screen holds the device (ADR 0030).
    fn screen_hold(&self) -> bool;
    /// The network is up.
    fn network_up(&self) -> bool;
    /// The user has not turned this plugin's network off.
    fn network_allowed(&self, owner: u64) -> bool;
    /// The plugin's own saving setting is held to manual.
    fn saving_manual(&self, owner: u64) -> bool;
    /// Whether the plugin subscribed to the event `tag` (the owner registry's
    /// answer; `false` for an owner that is unknown or stopped).
    fn subscribed(&self, owner: u64, tag: u32) -> bool;
    /// What the kernel holds and the shell does not: the user's power level
    /// (0 normal, 1 saving, 2 offline), its deadline `stretch` and whether a
    /// real-time screen holds the device. Told before every look at the
    /// scheduler; an env that is told nothing keeps its own answers (tests).
    fn kernel_state(&self, level: u32, stretch: u32, screen_hold: bool) {
        let _ = (level, stretch, screen_hold);
    }
}

/// The device's table sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limits {
    pub(crate) per_plugin: usize,
    pub(crate) device: usize,
    /// Runs one plugin may have open at once (a cancelled work's run goes on
    /// to its end, so this is not the same as `per_plugin`): no plugin holds
    /// every run slot for the 10 minutes a run may last.
    pub(crate) runs_per_plugin: usize,
}

impl Limits {
    /// The X4 (small memory class).
    pub(crate) const X4: Self = Self {
        per_plugin: 8,
        device: 32,
        runs_per_plugin: 2,
    };
    pub(crate) const OTHER: Self = Self {
        per_plugin: 32,
        device: 256,
        runs_per_plugin: 8,
    };
}

/// A `declare` after the service decoded and checked its form.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Spec<'a> {
    pub(crate) token: u32,
    pub(crate) needs: u32,
    pub(crate) flags: u32,
    pub(crate) moments: u32,
    pub(crate) after_ms: u32,
    pub(crate) within_ms: u32,
    pub(crate) every_ms: u32,
    pub(crate) at_ms: u64,
    pub(crate) key: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Waiting,
    /// Due and queued for the plugin: delivered when it can take it.
    Pending(u32),
    /// Delivered; its run is open.
    Running(u32),
}

#[derive(Debug)]
struct Entry {
    owner: u64,
    seq: u64,
    key: Box<str>,
    token: u32,
    network: bool,
    optional: bool,
    moments: u8,
    after_ms: u32,
    within_ms: u32,
    every_ms: u32,
    /// An exact time on the monotonic clock.
    at_ms: Option<u64>,
    /// Where `after_ms` counts from.
    base_ms: u64,
    /// Never due before this (the pass rule, the run floor, a finished run).
    earliest_ms: u64,
    /// When it came due (for the order of delivery).
    due_at: u64,
    /// A moment or time that came and waits for the network.
    trigger: Option<u32>,
    phase: Phase,
    /// Declared again while its run was open.
    rearm: bool,
    held: Option<u32>,
    /// The user turned the plugin's network off while this network work was
    /// not running: it is taken out for good with `work_refused`, whatever the
    /// setting says by the time the plugin can take the event.
    refused: bool,
}

impl Entry {
    /// When `after_ms` has passed (or the exact time).
    fn ready_ms(&self) -> u64 {
        self.at_ms
            .unwrap_or_else(|| self.base_ms + u64::from(self.after_ms))
            .max(self.earliest_ms)
    }

    /// Whether the deadline counts now (not for an optional work in saving).
    fn has_deadline(&self, power: Power) -> bool {
        self.at_ms.is_none() && self.within_ms != 0 && !(self.optional && power.level == 1)
    }

    fn deadline_ms(&self, power: Power) -> u64 {
        let stretch = u64::from(power.stretch.max(1));
        self.ready_ms()
            .saturating_add(u64::from(self.within_ms).saturating_mul(stretch))
    }

    /// `state.due_ms`: when it is due by time, 0 when only a moment brings it.
    fn due_ms(&self, power: Power) -> u64 {
        if self.at_ms.is_some() || self.moments == 0 {
            self.ready_ms()
        } else if self.has_deadline(power) {
            self.deadline_ms(power)
        } else {
            0
        }
    }
}

/// An event the scheduler has for a plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    /// `event.work_due` and its run.
    Due(Due),
    /// `event.work_refused`.
    Refused(Refused),
}

/// A key held on the stack (64 bytes at most).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KeyBuf {
    len: u8,
    bytes: [u8; MAX_KEY],
}

impl KeyBuf {
    fn new(key: &str) -> Self {
        let mut bytes = [0; MAX_KEY];
        let len = key.len().min(MAX_KEY);
        bytes[..len].copy_from_slice(&key.as_bytes()[..len]);
        Self {
            len: len as u8,
            bytes,
        }
    }
    pub(crate) fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Due {
    pub(crate) owner: u64,
    pub(crate) run: u32,
    pub(crate) token: u32,
    pub(crate) why: u32,
    pub(crate) key: KeyBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Refused {
    pub(crate) owner: u64,
    pub(crate) token: u32,
    pub(crate) reason: u32,
    pub(crate) key: KeyBuf,
}

/// One row of `state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) token: u32,
    /// 0 waiting, 1 held, 2 running.
    pub(crate) state: u32,
    pub(crate) reason: u32,
    pub(crate) due_ms: u64,
    pub(crate) key: KeyBuf,
}

/// An open run: what the host ends at its limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Run {
    pub(crate) id: u32,
    pub(crate) owner: u64,
    pub(crate) token: u32,
    started_ms: u64,
    callback_open: bool,
    resources: u32,
    ending: bool,
    /// When the run was told to end (the grace counts from here).
    ending_at: u64,
}

/// What a pass found for the shell to act on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Pass {
    /// Ask the network manager (`event.network_wanted`).
    pub(crate) network_wanted: bool,
    /// Work left a hold: tell its plugins `event.work_allowed` (a hint).
    pub(crate) allowed: bool,
}

#[derive(Debug)]
struct State {
    entries: Vec<Entry>,
    runs: Vec<Run>,
    next_seq: u64,
    next_run: u32,
    last_wanted_ms: Option<u64>,
    /// Runs that were over when something started in the callback of one of
    /// their late end events (owner, run): the pump ends it at once.
    late: Vec<(u64, u32)>,
}

/// The scheduler.
pub(crate) struct WorkSched {
    limits: Limits,
    state: RefCell<State>,
    /// The run whose callback is on the stack: `(owner, run)`.
    in_run: Cell<Option<(u64, u32)>>,
}

impl core::fmt::Debug for WorkSched {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WorkSched")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl WorkSched {
    pub(crate) const fn new(limits: Limits) -> Self {
        Self {
            limits,
            state: RefCell::new(State {
                entries: Vec::new(),
                runs: Vec::new(),
                next_seq: 1,
                next_run: 1,
                last_wanted_ms: None,
                late: Vec::new(),
            }),
            in_run: Cell::new(None),
        }
    }

    /// The form rules of `declare` (section 1.1): unknown bits first
    /// (`ERR_UNSUPPORTED`), then the rules (`ERR_INVALID`).
    pub(crate) fn check_form(spec: &Spec<'_>) -> Result<(), i32> {
        if spec.needs & !1 != 0 || spec.flags & !1 != 0 || spec.moments & !7 != 0 {
            return Err(ERR_UNSUPPORTED);
        }
        let key_ok = !spec.key.is_empty()
            && spec.key.len() <= MAX_KEY
            && !spec.key.chars().any(char::is_control);
        let spans_ok = spec.after_ms <= MAX_SPAN_MS
            && spec.within_ms <= MAX_SPAN_MS
            && spec.every_ms <= MAX_SPAN_MS
            && (spec.every_ms == 0 || spec.every_ms >= MIN_EVERY_MS);
        let exact_ok = spec.at_ms == 0
            || (spec.after_ms == 0
                && spec.within_ms == 0
                && spec.moments == 0
                && spec.flags & 1 == 0);
        if key_ok && spans_ok && exact_ok {
            Ok(())
        } else {
            Err(ERR_INVALID)
        }
    }

    /// Op 1: adds the work or replaces the plugin's work with this key (its
    /// times counted from `now_ms`). `ERR_FULL` over the plugin's or the
    /// device's table (a replaced key never counts twice), `ERR_NO_MEMORY`
    /// when the heap refuses; the table is unchanged on any error.
    pub(crate) fn declare(&self, owner: u64, spec: &Spec<'_>, now_ms: u64) -> Result<(), i32> {
        Self::check_form(spec)?;
        let mut state = self
            .state
            .try_borrow_mut()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        let from_run = self.in_run.get().is_some_and(|(o, _)| o == owner);
        let earliest = if from_run {
            now_ms.saturating_add(RUN_DECLARE_FLOOR_MS)
        } else {
            0
        };
        let existing = state
            .entries
            .iter()
            .position(|e| e.owner == owner && *e.key == *spec.key);
        if existing.is_none() {
            let mine = state.entries.iter().filter(|e| e.owner == owner).count();
            if mine >= self.limits.per_plugin || state.entries.len() >= self.limits.device {
                return Err(ERR_FULL);
            }
        }
        let seq = match existing {
            Some(at) => state.entries[at].seq,
            None => state.next_seq,
        };
        let next_seq = seq.checked_add(1).ok_or(ERR_FULL)?;
        let key: Box<str> = {
            let mut text = alloc::string::String::new();
            text.try_reserve_exact(spec.key.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            text.push_str(spec.key);
            text.into_boxed_str()
        };
        let mut entry = Entry {
            owner,
            seq,
            key,
            token: spec.token,
            network: spec.needs & 1 != 0,
            optional: spec.flags & 1 != 0,
            moments: spec.moments as u8,
            after_ms: spec.after_ms,
            within_ms: spec.within_ms,
            every_ms: spec.every_ms,
            at_ms: (spec.at_ms != 0).then_some(spec.at_ms),
            base_ms: now_ms,
            earliest_ms: earliest,
            due_at: 0,
            trigger: None,
            phase: Phase::Waiting,
            rearm: false,
            held: None,
            refused: false,
        };
        match existing {
            Some(at) => {
                if let Phase::Running(run) = state.entries[at].phase {
                    // The open run goes on; the new wish waits for its end.
                    entry.phase = Phase::Running(run);
                    entry.rearm = true;
                }
                state.entries[at] = entry;
            }
            None => {
                state.entries.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
                state.entries.push(entry);
                state.next_seq = next_seq;
            }
        }
        Ok(())
    }

    /// Op 2: removes the work with this key, all the plugin's with `""`.
    /// A missing key is fine; work already delivered is not called back, and
    /// its open run goes on to its end.
    pub(crate) fn cancel(&self, owner: u64, key: &str) -> Result<(), i32> {
        let mut state = self
            .state
            .try_borrow_mut()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        state
            .entries
            .retain(|e| !(e.owner == owner && (key.is_empty() || *e.key == *key)));
        Ok(())
    }

    /// Op 3: the plugin's own work in declaration order, at most 32 rows.
    pub(crate) fn rows(
        &self,
        owner: u64,
        power: Power,
        visit: &mut dyn FnMut(Row) -> bool,
    ) -> Result<(), i32> {
        let state = self
            .state
            .try_borrow()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        let mut done = 0;
        let mut last_seq = 0;
        while let Some(entry) = state
            .entries
            .iter()
            .filter(|e| e.owner == owner && e.seq > last_seq)
            .min_by_key(|e| e.seq)
        {
            if done == 32 {
                break;
            }
            last_seq = entry.seq;
            let (state_no, reason) = match (entry.phase, entry.held) {
                (Phase::Running(_), _) => (2, 0),
                (_, Some(reason)) => (1, reason),
                _ => (0, 0),
            };
            let row = Row {
                token: entry.token,
                state: state_no,
                reason,
                due_ms: entry.due_ms(power),
                key: KeyBuf::new(&entry.key),
            };
            done += 1;
            if !visit(row) {
                break;
            }
        }
        Ok(())
    }

    /// Why network work waits for the device (offline, saving held to manual).
    fn hold_reason(e: &Entry, power: Power, env: &dyn WorkEnv) -> Option<u32> {
        if !e.network {
            None
        } else if power.level == 2 {
            Some(REASON_OFFLINE)
        } else if power.level == 1 && env.saving_manual(e.owner) {
            Some(REASON_SAVING)
        } else {
            None
        }
    }

    /// Network work of a plugin whose network is off (and that is not running)
    /// is refused for good: the mark stays whatever the setting says later.
    fn latch_refusals(state: &mut State, env: &dyn WorkEnv) {
        for e in &mut state.entries {
            if e.network && !matches!(e.phase, Phase::Running(_)) && !env.network_allowed(e.owner) {
                e.refused = true;
            }
        }
    }

    /// Work marked pending by the last pass goes back to waiting when the
    /// device state no longer allows it, so a pending row never outlives its
    /// hold, and `state` shows the hold.
    fn recheck_pending(state: &mut State, env: &dyn WorkEnv) {
        let power = env.power();
        let screen = env.screen_hold();
        let up = env.network_up();
        for e in &mut state.entries {
            if !matches!(e.phase, Phase::Pending(_)) {
                continue;
            }
            let held = Self::hold_reason(e, power, env);
            let blocked = held.is_some()
                || (screen && e.at_ms.is_none())
                || (e.network && !up)
                || (e.optional && e.network && power.level == 1 && !power.radio_on);
            e.held = held;
            if blocked {
                e.phase = Phase::Waiting;
            }
        }
    }

    /// A moment came (the shell feeds them). Waiting work whose `after_ms`
    /// has passed and which asked for it is triggered; an optional work waits
    /// for a moment with the radio on anyway while saving.
    pub(crate) fn moment(&self, moment: Moment, env: &dyn WorkEnv) {
        let now = env.now_ms();
        let power = env.power();
        if let Ok(mut state) = self.state.try_borrow_mut() {
            for e in &mut state.entries {
                let radio_ok = !(e.optional && power.level == 1) || power.radio_on;
                if e.phase == Phase::Waiting
                    && e.at_ms.is_none()
                    && e.moments & moment.bit() != 0
                    && e.trigger.is_none()
                    && now >= e.ready_ms()
                    && radio_ok
                {
                    e.trigger = Some(moment.why());
                }
            }
        }
    }

    /// Starts a pass: finds what is due and marks it pending (delivered by
    /// [`Self::next_event`] in due order), applies the holds and works out the
    /// radio wish. No plugin code runs and nothing is allocated. Work declared
    /// after this call, also by the callbacks of this pass, is only looked at
    /// by the next pass, so a callback that declares itself again at once
    /// cannot loop inside one pass.
    pub(crate) fn begin_pass(&self, env: &dyn WorkEnv) -> Result<Pass, i32> {
        let now = env.now_ms();
        let power = env.power();
        let screen = env.screen_hold();
        let up = env.network_up();
        let mut state = self
            .state
            .try_borrow_mut()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        let mut pass = Pass::default();
        let mut wants = false;
        Self::latch_refusals(&mut state, env);
        // Phase 1: triggers, holds and the entries that are due by themselves.
        for e in &mut state.entries {
            if e.phase != Phase::Waiting || e.refused {
                continue;
            }
            let was_held = e.held.take();
            e.held = Self::hold_reason(e, power, env);
            if was_held.is_some() && e.held.is_none() {
                pass.allowed = true;
            }
            if now < e.ready_ms() {
                continue;
            }
            // Optional network work is only for a radio that is on anyway.
            if e.optional && e.network && power.level == 1 && !power.radio_on {
                continue;
            }
            let mut due_at = now;
            if e.trigger.is_none() {
                if e.at_ms.is_some() || (e.moments == 0 && e.within_ms == 0) {
                    e.trigger = Some(why::TIME);
                    due_at = e.ready_ms();
                } else if e.has_deadline(power) && now >= e.deadline_ms(power) {
                    e.trigger = Some(why::DEADLINE);
                    due_at = e.deadline_ms(power);
                }
            }
            let Some(trigger) = e.trigger else { continue };
            if (screen && e.at_ms.is_none()) || e.held.is_some() {
                continue;
            }
            if e.network && !up {
                // Time or a deadline came while no network is up: wish for
                // one; optional work never asks.
                wants |= !e.optional && (trigger == why::TIME || trigger == why::DEADLINE);
                continue;
            }
            e.phase = Phase::Pending(trigger);
            e.due_at = due_at;
        }
        // Phase 2: network work whose time has passed rides along on a
        // connection that is up for other network work.
        if up
            && state
                .entries
                .iter()
                .any(|e| e.network && matches!(e.phase, Phase::Pending(_)))
        {
            for e in &mut state.entries {
                if e.phase == Phase::Waiting
                    && e.network
                    && e.held.is_none()
                    && e.at_ms.is_none()
                    && !screen
                    && now >= e.ready_ms()
                    && !e.refused
                    && !(e.optional && power.level == 1 && !power.radio_on)
                {
                    e.phase = Phase::Pending(why::ONLINE);
                    e.due_at = now;
                }
            }
        }
        if wants
            && state
                .last_wanted_ms
                .is_none_or(|last| now.saturating_sub(last) >= NETWORK_GAP_MS)
        {
            state.last_wanted_ms = Some(now);
            pass.network_wanted = true;
        }
        Ok(pass)
    }

    /// The next event for a plugin that can take one now, in due order: first
    /// a refusal (the user turned the network off), else due work. Starting a
    /// due work opens its run. `None` when nothing is ready for any
    /// accepting plugin.
    pub(crate) fn next_event(
        &self,
        env: &dyn WorkEnv,
        accepts: &dyn Fn(u64) -> bool,
    ) -> Result<Option<Event>, i32> {
        let now = env.now_ms();
        let mut state = self
            .state
            .try_borrow_mut()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        // A work whose plugin's network was turned off is taken out for good,
        // also when the setting is on again by the time the plugin can take
        // the event.
        Self::latch_refusals(&mut state, env);
        if let Some(at) = state
            .entries
            .iter()
            .position(|e| e.refused && !matches!(e.phase, Phase::Running(_)) && accepts(e.owner))
        {
            let e = state.entries.remove(at);
            return Ok(Some(Event::Refused(Refused {
                owner: e.owner,
                token: e.token,
                reason: REFUSED_NETWORK_OFF,
                key: KeyBuf::new(&e.key),
            })));
        }
        // What was due when the pass ran may not be allowed now (offline, the
        // plugin's saving held to manual, the screen, no network up): it goes
        // back to waiting and is looked at by the next pass.
        Self::recheck_pending(&mut state, env);
        if state.runs.len() >= self.limits.device {
            return Ok(None);
        }
        let runs = &state.runs;
        let Some(at) = state
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                matches!(e.phase, Phase::Pending(_))
                    && accepts(e.owner)
                    && runs.iter().filter(|r| r.owner == e.owner).count()
                        < self.limits.runs_per_plugin
            })
            .min_by_key(|(_, e)| (e.due_at, e.seq))
            .map(|(at, _)| at)
        else {
            return Ok(None);
        };
        let Phase::Pending(why) = state.entries[at].phase else {
            return Ok(None);
        };
        let run = state.next_run;
        let next = run.checked_add(1).ok_or(ERR_FULL)?;
        state.runs.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        state.next_run = next;
        let (owner, token) = (state.entries[at].owner, state.entries[at].token);
        state.runs.push(Run {
            id: run,
            owner,
            token,
            started_ms: now,
            callback_open: true,
            resources: 0,
            ending: false,
            ending_at: 0,
        });
        let e = &mut state.entries[at];
        e.phase = Phase::Running(run);
        e.trigger = None;
        e.rearm = false;
        Ok(Some(Event::Due(Due {
            owner,
            run,
            token,
            why,
            key: KeyBuf::new(&e.key),
        })))
    }

    /// The earliest time in the future something can come due by time (for
    /// the main loop's nap), `None` when nothing waits on a clock. Work that is
    /// held, refused, or already past its time and still waiting is blocked by
    /// the device state, which a moment, a setting or a network change ends:
    /// none of them is a wake time, or the loop would never sleep. Call it
    /// after the pass.
    pub(crate) fn next_wake(&self, env: &dyn WorkEnv) -> Option<u64> {
        let power = env.power();
        let now = env.now_ms();
        let state = self.state.try_borrow().ok()?;
        // A run past its longest time waits only the grace for its end events.
        let runs = state.runs.iter().map(|r| {
            if r.ending {
                r.ending_at.saturating_add(RUN_END_GRACE_MS)
            } else {
                r.started_ms.saturating_add(RUN_MAX_MS)
            }
        });
        let timers = state.entries.iter().filter_map(|e| {
            if e.phase != Phase::Waiting || e.refused || e.held.is_some() {
                return None;
            }
            match e.due_ms(power) {
                due if due > now => Some(due),
                _ => None,
            }
        });
        runs.chain(timers).min()
    }

    // ---- runs ----

    /// A callback of the run begins on the stack: UI questions are refused
    /// and work declared gets the floor while it lasts. Ends when the guard
    /// drops.
    pub(crate) fn enter_run_callback(&self, owner: u64, run: u32) -> RunCallback<'_> {
        let previous = self.in_run.replace(Some((owner, run)));
        RunCallback {
            sched: self,
            previous,
        }
    }

    /// Whether `owner` is in a callback that belongs to a run: dialogs
    /// answer `ERR_STATE` and notices go to its work note (section 2.3).
    pub(crate) fn quiet(&self, owner: u64) -> bool {
        self.in_run.get().is_some_and(|(o, _)| o == owner)
    }

    /// The run whose callback is on the stack for `owner` (`core.net` ties a
    /// request to it); `None` outside a run callback or for another owner. A
    /// read of one cell: no allocation, no borrow of the table.
    pub(crate) fn current_run(&self, owner: u64) -> Option<u64> {
        self.in_run
            .get()
            .filter(|&(running, _)| running == owner)
            .map(|(_, run)| u64::from(run))
    }

    /// The run's `work_due` callback returned.
    pub(crate) fn callback_returned(&self, run: u32, now_ms: u64) {
        self.with_run(run, |r| r.callback_open = false);
        self.maybe_finish(run, now_ms);
    }

    /// A request or socket of the run opened.
    pub(crate) fn resource_opened(&self, run: u32) -> Result<(), i32> {
        let mut state = self
            .state
            .try_borrow_mut()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        if let Some(r) = state.runs.iter_mut().find(|r| r.id == run) {
            r.resources = r.resources.checked_add(1).ok_or(ERR_FULL)?;
            return Ok(());
        }
        // A run that is over (one that never was is `ERR_NOT_FOUND`): its
        // late end event's callback started something. It is kept (never
        // foreground) and not counted: the pump ends it at once with the
        // run's reason (`work.md` 2.3).
        let (owner, current) = self
            .in_run
            .get()
            .ok_or(paperpaw_native::host_glue::ERR_NOT_FOUND)?;
        if current != run || run >= state.next_run {
            return Err(paperpaw_native::host_glue::ERR_NOT_FOUND);
        }
        if !state.late.contains(&(owner, run)) {
            state.late.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
            state.late.push((owner, run));
        }
        Ok(())
    }

    /// The runs that were over when something started in a late end event's
    /// callback, once (owner, run).
    pub(crate) fn take_late(&self) -> Option<(u64, u32)> {
        self.state.try_borrow_mut().ok()?.late.pop()
    }

    /// A request or socket of the run ended. `false`: the scheduler is in use
    /// and nothing was done; the caller asks again.
    pub(crate) fn resource_closed(&self, run: u32, now_ms: u64) -> bool {
        {
            let Ok(mut state) = self.state.try_borrow_mut() else {
                return false;
            };
            if let Some(r) = state.runs.iter_mut().find(|r| r.id == run) {
                r.resources = r.resources.saturating_sub(1);
            }
        }
        self.maybe_finish(run, now_ms);
        true
    }

    /// A run past its limit, once: the shell ends its requests with
    /// [`REQUEST_WORK_ENDED`] and its sockets with [`SOCKET_WORK_ENDED`] and
    /// then calls [`Self::end_run`].
    pub(crate) fn take_expired(&self, now_ms: u64) -> Option<Run> {
        let mut state = self.state.try_borrow_mut().ok()?;
        let r = state
            .runs
            .iter_mut()
            .find(|r| !r.ending && now_ms.saturating_sub(r.started_ms) >= RUN_MAX_MS)?;
        r.ending = true;
        r.ending_at = now_ms;
        Some(*r)
    }

    /// The run is over whatever it still held.
    pub(crate) fn end_run(&self, run: u32, now_ms: u64) {
        self.with_run(run, |r| {
            r.callback_open = false;
            r.resources = 0;
        });
        self.maybe_finish(run, now_ms);
    }

    /// The run reached its longest time and its services were told to end what
    /// it started: its callback is over, but what they ended still counts
    /// until its last event was delivered (`work.md` 2.3), so the run ends
    /// with the last of them (at once when none is left).
    pub(crate) fn expired_told(&self, run: u32, now_ms: u64) {
        self.with_run(run, |r| r.callback_open = false);
        self.maybe_finish(run, now_ms);
    }

    /// A run told to end and the grace after that, once: it is over
    /// whatever it still counts ([`Self::end_run`]).
    pub(crate) fn take_stuck(&self, now_ms: u64) -> Option<Run> {
        let state = self.state.try_borrow().ok()?;
        state
            .runs
            .iter()
            .find(|r| r.ending && now_ms.saturating_sub(r.ending_at) >= RUN_END_GRACE_MS)
            .copied()
    }

    /// Visits the runs past their longest time that still hold something
    /// (`owner`, run): whatever was started there since, from an end-event
    /// callback, ends at once too.
    pub(crate) fn each_ending(&self, visit: &mut dyn FnMut(u64, u32)) {
        let Ok(state) = self.state.try_borrow() else {
            return;
        };
        for r in state.runs.iter().filter(|r| r.ending && r.resources != 0) {
            visit(r.owner, r.id);
        }
    }

    fn with_run(&self, run: u32, f: impl FnOnce(&mut Run)) {
        if let Ok(mut state) = self.state.try_borrow_mut()
            && let Some(r) = state.runs.iter_mut().find(|r| r.id == run)
        {
            f(r);
        }
    }

    fn maybe_finish(&self, run: u32, now_ms: u64) {
        let Ok(mut state) = self.state.try_borrow_mut() else {
            return;
        };
        let Some(at) = state.runs.iter().position(|r| r.id == run) else {
            return;
        };
        if state.runs[at].callback_open || state.runs[at].resources != 0 {
            return;
        }
        state.runs.swap_remove(at);
        let Some(at) = state
            .entries
            .iter()
            .position(|e| e.phase == Phase::Running(run))
        else {
            return;
        };
        let e = &mut state.entries[at];
        if e.every_ms != 0 && !e.rearm {
            // Due again that long after it ran, never before its run ended.
            e.base_ms = now_ms;
            e.after_ms = e.every_ms;
            if let Some(t) = e.at_ms.as_mut() {
                *t = now_ms.saturating_add(u64::from(e.every_ms));
            }
            e.earliest_ms = now_ms;
            e.phase = Phase::Waiting;
            e.trigger = None;
        } else if e.rearm {
            e.phase = Phase::Waiting;
            e.rearm = false;
            e.earliest_ms = e.earliest_ms.max(now_ms);
        } else {
            state.entries.remove(at);
        }
    }

    /// The plugin stopped: its work and its runs are gone (no events).
    ///
    /// # Errors
    /// `ERR_BUSY` while the table is borrowed: nothing was removed and the
    /// caller retries (the plugin's retirement waits).
    pub(crate) fn owner_stopped(&self, owner: u64) -> Result<(), i32> {
        let mut state = self
            .state
            .try_borrow_mut()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        state.entries.retain(|e| e.owner != owner);
        state.runs.retain(|r| r.owner != owner);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn counts(&self) -> (usize, usize) {
        let state = self.state.borrow();
        (state.entries.len(), state.runs.len())
    }
}

/// See [`WorkSched::enter_run_callback`].
pub(crate) struct RunCallback<'a> {
    sched: &'a WorkSched,
    previous: Option<(u64, u32)>,
}

impl Drop for RunCallback<'_> {
    fn drop(&mut self) {
        self.sched.in_run.set(self.previous);
    }
}

/// The status a `declare` of a network work gets when the plugin did not ask
/// for `uses.network` or the user turned its network off.
pub(crate) const fn network_denied() -> i32 {
    ERR_DENIED
}

#[cfg(test)]
#[path = "work_sched_tests.rs"]
mod tests;
