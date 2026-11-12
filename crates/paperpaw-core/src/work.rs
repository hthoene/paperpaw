//! Declared work (ADR 0025): one queue for plugins' timers, deadlines,
//! natural moments and jobs.
//!
//! A work runs at its exact time, at one of its moments (waking, a book
//! opened or closed, going to sleep, the radio on anyway, charging, idle)
//! or when its deadline comes, whichever is first and allowed by the power
//! level. Work that needs the network and is only nice to have runs only
//! where that is cheap: with the radio on anyway, or where PaperPaw does
//! not manage Wi-Fi. A work with a step function ([`WorkSpec`]) is a job,
//! stepped in [`Kernel::tick`], one step a call.

use alloc::string::String;
use alloc::vec::Vec;
use core::mem;

mod native_network;
pub(crate) use native_network::USES_NETWORK;
mod runs;

use paperpaw_platform::Instant;
use paperpaw_sdk::builtin::{events, params};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::power::{self, Power, PowerLevel, Wear};
use paperpaw_sdk::work::Needs;
use paperpaw_sdk::work::{
    Importance, Moments, Progress, Reason, Refusal, Retry, Step, Why, Work, WorkRefused, WorkSpec,
    WorkState,
};
use paperpaw_sdk::{
    Command, Env, HttpReply, Id, Params, ServerNotice, SocketError, SocketNotice, Uses,
};

use crate::Kernel;

/// The settings group of every plugin's network.
pub(crate) const NETWORK_GROUP: Id = Id::new_static("core.group.network");
/// The extra of every plugin's network settings: shown under the plugin's
/// name.
static NETWORK_EXTRA: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().section_of_plugin();

/// Where the worn power mode is kept across starts (on the X4, every
/// wake is a start): its id, whether Automatic chose it, the values the
/// reader took back.
const POWER_FILE: &str = ".paperpaw/power.ini";

/// Rounds of work in one call: work run may bring more due (a moment of
/// its own, a job ready), at most this often.
pub(crate) const MAX_ROUNDS: usize = 4;

/// Without input this long, the screen still on: the `IDLE` moment.
pub(crate) const IDLE_MS: u64 = 10_000;
/// A job's progress is told at most this often.
const PROGRESS_MS: u64 = 1_000;

/// Where a job is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Job {
    /// A step to take.
    Ready,
    /// Nothing to do before then.
    Until(Instant),
    /// Nothing to do before this event.
    For(Id),
}

#[derive(Debug)]
struct Entry {
    work: Work,
    /// The plugin it is of: its id's namespace.
    owner: String,
    /// When it was declared: `after` and `within` count from here.
    declared: Instant,
    /// Failed attempts of a job, for its backoff.
    attempt: u32,
    /// A job running.
    job: Option<Job>,
    progress: Option<Progress>,
    /// When its progress was told last.
    told: Option<Instant>,
}

impl Entry {
    /// Whether it is the work `id`, `key`, a job going on.
    fn going(&self, id: &Id, key: &str) -> bool {
        self.job.is_some() && self.work.id == *id && self.work.key == key
    }

    fn after(&self) -> Instant {
        self.declared.add_millis(millis(self.work.after))
    }

    /// When it is due at the latest, at this power: none for work that
    /// waits for a cheap moment.
    fn deadline(&self, power: &Power) -> Option<Instant> {
        let within = self.work.within?;
        let saving = power.level != PowerLevel::Normal;
        if self.work.importance == Importance::Optional && saving {
            return None;
        }
        let stretch = if saving { u64::from(power.stretch) } else { 1 };
        Some(
            self.after()
                .add_millis(millis(Some(within)).saturating_mul(stretch)),
        )
    }

    /// Held back by the power level.
    fn held(&self, power: &Power) -> bool {
        self.work.needs.contains(Needs::NETWORK) && power.level == PowerLevel::Offline
    }
}

fn millis(duration: Option<core::time::Duration>) -> u64 {
    duration.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// What the reader set for a plugin that uses the network (ADR 0025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exempt {
    /// Its work follows the mode.
    Follow,
    /// Its work keeps Normal's limits in Saving (not in Offline).
    Normal,
    /// No background network at all, in every mode.
    Manual,
}

/// The setting of a plugin's network, and of its work in saving modes.
pub(crate) fn network_setting(plugin: &str) -> String {
    alloc::format!("core.network.{plugin}")
}

pub(crate) fn saving_setting(plugin: &str) -> String {
    alloc::format!("core.saving.{plugin}")
}

/// The choices of [`saving_setting`], translation keys too.
pub(crate) const SAVING_CHOICES: [&str; 3] = [
    "core.saving.follow",
    "core.saving.normal",
    "core.saving.manual",
];

/// The two settings of a networked plugin: whether it may use the network
/// ([`network_setting`]), and how its work saves energy
/// ([`saving_setting`]).
pub(crate) const fn network_specs(network: Id, saving: Id) -> [paperpaw_sdk::SettingSpec; 2] {
    [
        paperpaw_sdk::SettingSpec::new(
            network,
            "core.network.allowed",
            paperpaw_sdk::SettingKind::Bool,
            paperpaw_sdk::SettingValue::Bool(true),
            NETWORK_GROUP,
        )
        .description("core.network.allowed.help")
        .with_extra(&NETWORK_EXTRA),
        paperpaw_sdk::SettingSpec::new(
            saving,
            "core.saving.title",
            paperpaw_sdk::SettingKind::Choice(paperpaw_sdk::Choices::Fixed(&SAVING_CHOICES)),
            paperpaw_sdk::SettingValue::Text(alloc::borrow::Cow::Borrowed(SAVING_CHOICES[0])),
            NETWORK_GROUP,
        )
        .description("core.saving.help")
        .with_extra(&NETWORK_EXTRA),
    ]
}

/// The network settings of a plugin, kept in the program: what the
/// kernel would otherwise make on the heap at every start (two settings,
/// some hundred and fifty bytes each). Handed to the kernel by who
/// composes the firmware ([`network_settings!`](crate::network_settings),
/// [`KernelBuilder::network_settings`](crate::KernelBuilder::network_settings)).
pub type NetworkSettings = (&'static str, [paperpaw_sdk::SettingSpec; 2]);

/// [`NetworkSettings`] of the plugins named, the same as the kernel makes
/// for any other networked plugin: e.g.
/// `static NETWORK: &[NetworkSettings] = network_settings!("my-plugin");`.
#[macro_export]
macro_rules! network_settings {
    ($($plugin:literal),* $(,)?) => {
        &[$((
            $plugin,
            $crate::network_specs_of(
                $crate::sdk::Id::new_static(concat!("core.network.", $plugin)),
                $crate::sdk::Id::new_static(concat!("core.saving.", $plugin)),
            ),
        )),*]
    };
}

/// [`network_specs`], for [`network_settings!`](crate::network_settings).
#[doc(hidden)]
pub const fn network_specs_of(network: Id, saving: Id) -> [paperpaw_sdk::SettingSpec; 2] {
    network_specs(network, saving)
}

/// The work plugins declared, and the jobs' steps.
#[derive(Debug, Default)]
pub(crate) struct Queue {
    entries: Vec<Entry>,
    specs: Vec<WorkSpec>,
    /// Most works a plugin may have declared.
    per_plugin: usize,
    /// Moments seen since work last ran.
    moments: Moments,
    /// The radio was on (or the network there, where PaperPaw does not
    /// manage Wi-Fi) when work last ran.
    online: bool,
    charging: bool,
    /// The input `IDLE` was given for, so it comes once each time.
    idle_since: Option<Instant>,
    /// The call is a tick: times and deadlines come due, and jobs may
    /// take a step (never while input waits). Moments come at any call.
    ticking: bool,
    /// Notices of background work, as notes, not yet kept.
    notes: Vec<(String, paperpaw_sdk::work::Note)>,
    /// Native plugins with `uses.network` (admitted with their network
    /// settings, `native_network.rs`).
    natives: Vec<String>,
    /// The network settings kept in the program (see
    /// [`Kernel::define_network_settings`]), for the native plugins too.
    fixed: &'static [NetworkSettings],
    /// Plugins whose network the reader switched off.
    off: Vec<String>,
    /// Plugins whose work does not follow the mode.
    exempt: Vec<(String, Exempt)>,
    /// What the reader set per plugin changed: read again.
    stale: bool,
    /// How many times it was read again (the network policy of the native
    /// services is made again when it changes).
    policy_version: u32,
    /// The screen the reader chose *Go online* on while Offline: the
    /// network for it until it closes ([`Command::GoOnline`]).
    online_for: Option<Id>,
    /// Until when the reader chose *Go online* outside such a screen
    /// ([`Command::GoOnlineFor`]).
    pub(crate) online_until: Option<Instant>,
    /// Works or their states changed since `Env::work` was made.
    changed: bool,
    /// Plugins refused for a full queue, told `WORK_ALLOWED` once there is
    /// room again.
    full: Vec<String>,
    /// The device goes to sleep or PaperPaw stops: the worn mode is
    /// written down.
    keep: bool,
    /// Held back to make room for a screen (ADR 0030): nothing runs, and
    /// nothing wakes the device for work, until let go.
    held: bool,
    /// Plugins that declared work under another's name, to warn of.
    foreign: Vec<String>,
    /// The runs of work, and the sockets they connect.
    pub(crate) runs: runs::Runs,
}

impl Queue {
    pub(crate) fn new(per_plugin: usize) -> Self {
        Self {
            per_plugin,
            stale: true,
            ..Self::default()
        }
    }

    /// Holds all work back (to make room for a screen), or lets it go on.
    pub(crate) fn hold(&mut self, held: bool) {
        self.held = held;
    }

    /// Whether the job of the work `id` takes steps now: not waiting for
    /// an event or a time, and not held back for a screen.
    pub(crate) fn stepping(&self, id: &Id) -> bool {
        !self.held
            && self
                .entries
                .iter()
                .any(|e| e.work.id == *id && matches!(e.job, Some(Job::Ready)))
    }

    /// Whether work is held back.
    #[cfg(test)]
    pub(crate) fn is_held(&self) -> bool {
        self.held
    }

    /// Whether the reader switched `plugin`'s network off.
    pub(crate) fn network_off(&self, plugin: &str) -> bool {
        self.off.iter().any(|p| p == plugin)
    }

    /// The power `owner`'s work goes by: the mode's, unless the reader
    /// exempted it.
    fn power_of(&self, power: &Power, owner: &str) -> Power {
        let exempt = self
            .exempt
            .iter()
            .find(|(p, _)| p == owner)
            .map_or(Exempt::Follow, |(_, e)| *e);
        match exempt {
            Exempt::Follow => power.clone(),
            Exempt::Normal if power.level == PowerLevel::Saving => {
                Power::new(PowerLevel::Normal, power.mode.clone(), power.automatic, 1)
            }
            Exempt::Normal => power.clone(),
            Exempt::Manual => Power::new(
                PowerLevel::Offline,
                power.mode.clone(),
                power.automatic,
                power.stretch,
            ),
        }
    }

    /// A job's steps, from a plugin's registration.
    pub(crate) fn add_spec(&mut self, spec: WorkSpec) {
        self.specs.retain(|s| s.id != spec.id);
        self.specs.push(spec);
    }

    /// Declares `work`, in place of one with the same id and key.
    fn declare(&mut self, work: Work, now: Instant, out: &mut Vec<Command>) {
        self.changed = true;
        let owner = String::from(work.id.namespace());
        let same = |e: &Entry| e.work.id == work.id && e.work.key == work.key;
        if let Some(entry) = self.entries.iter_mut().find(|e| same(e)) {
            // Declared again: its time counts from the first declaring (a
            // timer moves to its new time), a running job goes on.
            let declared = if work.at.is_some() {
                now
            } else {
                entry.declared
            };
            let old = mem::replace(entry, Entry::new(work, owner, declared));
            entry.job = old.job;
            entry.progress = old.progress;
            entry.attempt = old.attempt;
            return;
        }
        if self.entries.iter().filter(|e| e.owner == owner).count() >= self.per_plugin {
            if !self.full.contains(&owner) {
                self.full.push(owner);
            }
            let refused =
                WorkRefused::new(work.id, Refusal::new(Reason::TooOften, Retry::WhenAllowed));
            out.push(events::WORK_REFUSED.emit(refused));
            return;
        }
        self.entries.push(Entry::new(work, owner, now));
    }

    /// Tells plugins refused for a full queue that there is room again.
    fn allow_full(&mut self, out: &mut Vec<Command>) {
        let (entries, per_plugin) = (&self.entries, self.per_plugin);
        let before = self.full.len();
        self.full
            .retain(|owner| entries.iter().filter(|e| e.owner == *owner).count() >= per_plugin);
        if self.full.len() < before {
            out.push(events::WORK_ALLOWED.emit(()));
        }
    }

    /// Takes back the work with this id and key (any key: `None`).
    fn cancel(&mut self, id: &Id, key: Option<&str>) {
        self.changed = true;
        self.runs.end_work(id, key);
        self.entries
            .retain(|e| !(e.work.id == *id && key.is_none_or(|k| e.work.key == k)));
    }

    /// An event was emitted: a moment, what a job waited for, or a
    /// setting of a plugin's network changed.
    // Its own frame: not inlined into `Kernel::emit_from` (#250).
    #[inline(never)]
    pub(crate) fn saw(&mut self, event: &Id, params: &Params) {
        if *event == events::SETTING_CHANGED
            && params
                .get(params::ID)
                .is_some_and(|id| id.starts_with("core.network.") || id.starts_with("core.saving."))
        {
            self.stale = true;
        }
        let moment = match event {
            e if *e == events::STARTED || *e == events::RESUME => Moments::WAKE,
            e if *e == events::BOOK_OPENED => Moments::BOOK_OPENED,
            e if *e == events::BOOK_CLOSED => Moments::BOOK_CLOSED,
            e if *e == events::SUSPEND => Moments::SLEEP,
            _ => Moments::NONE,
        };
        self.moments = self.moments | moment;
        // Stopping is no moment, no work runs for it; but the worn mode is
        // written down as at sleep.
        if *event == events::STOPPING {
            self.keep = true;
        }
        for entry in &mut self.entries {
            if entry.job.as_ref() == Some(&Job::For(event.clone())) {
                entry.job = Some(Job::Ready);
            }
        }
    }

    /// The call is a tick ([`Kernel::tick`]).
    pub(crate) fn ticking(&mut self) {
        self.ticking = true;
    }

    /// When the kernel must look at the work again, at the latest.
    pub(crate) fn next_wake(&self, power: &Power, last_activity: Instant) -> Option<Instant> {
        if self.held {
            return None;
        }
        let mut next: Option<Instant> = None;
        let mut earliest = |at: Instant| next = Some(next.map_or(at, |n: Instant| n.min(at)));
        if !self.moments.is_empty() {
            earliest(Instant(0));
        }
        for entry in &self.entries {
            match &entry.job {
                Some(Job::Ready) => earliest(Instant(0)),
                Some(Job::Until(at)) => earliest(*at),
                Some(Job::For(_)) => {}
                None if entry.held(&self.power_of(power, &entry.owner)) => {}
                None => {
                    // Only what can run then wakes the device: no work that
                    // waits for charging or for the radio to be on anyway.
                    let waits = (entry.work.needs.contains(Needs::CHARGING) && !self.charging)
                        || (entry.work.importance == Importance::Optional
                            && entry.work.needs.contains(Needs::NETWORK)
                            && !self.online);
                    if let Some(at) = entry.work.at {
                        earliest(at);
                    } else if !waits
                        && let Some(due) = entry.deadline(&self.power_of(power, &entry.owner))
                    {
                        earliest(due);
                    }
                    if entry.work.moments.contains(Moments::IDLE) && entry.job.is_none() {
                        let idle = last_activity.add_millis(IDLE_MS);
                        if self.idle_since != Some(last_activity) {
                            earliest(idle.max(entry.after()));
                        }
                    }
                }
            }
        }
        next
    }

    /// Where each work stands.
    fn states(&self, power: &Power) -> Vec<(Id, String, WorkState)> {
        self.entries
            .iter()
            .map(|e| {
                let power = &self.power_of(power, &e.owner);
                let state = if e.job.is_some() {
                    WorkState::Running {
                        progress: e.progress,
                    }
                } else if e.held(power) {
                    WorkState::Held(Refusal::new(Reason::Offline, Retry::WhenAllowed))
                } else {
                    WorkState::Waiting {
                        due: e.work.at.or_else(|| e.deadline(power)),
                    }
                };
                (e.work.id.clone(), e.work.key.clone(), state)
            })
            .collect()
    }

    /// The moments since the last run, with the radio, charging and idle
    /// looked at now.
    fn take_moments(&mut self, env: &Env<'_>, last_activity: Instant) -> Moments {
        let mut moments = mem::take(&mut self.moments);
        let online = env
            .wifi()
            .map_or(env.caps().network, |w| w.state.is_online());
        if online && (!self.online || moments.contains(Moments::WAKE)) {
            moments = moments | Moments::ONLINE;
        }
        self.online = online;
        let charging = env.status().battery.is_some_and(|b| b.charging);
        if charging && !self.charging {
            moments = moments | Moments::CHARGING;
        }
        self.charging = charging;
        if env.now().since(last_activity) >= IDLE_MS && self.idle_since != Some(last_activity) {
            self.idle_since = Some(last_activity);
            moments = moments | Moments::IDLE;
        }
        moments
    }

    /// Why `entry` runs now, if it does.
    fn why(&self, entry: &Entry, moments: Moments, env: &Env<'_>, base: &Power) -> Option<Why> {
        let power = &self.power_of(base, &entry.owner);
        if entry.job.is_some() || entry.held(power) {
            return None;
        }
        if let Some(at) = entry.work.at {
            return (self.ticking && at <= env.now()).then_some(Why::Time);
        }
        if entry.after() > env.now()
            || (entry.work.needs.contains(Needs::CHARGING) && !self.charging)
        {
            return None;
        }
        // Work only nice to have never brings the radio on.
        let cheap = !entry.work.needs.contains(Needs::NETWORK) || self.online;
        let mut hit = Moments::from_bits(moments.bits() & entry.work.moments.bits());
        if entry.work.importance == Importance::Optional && !cheap {
            hit = Moments::NONE;
        }
        if !hit.is_empty() {
            return Some(Why::Moment(hit));
        }
        let optional_now = entry.work.importance == Importance::Optional && cheap;
        let due = self.ticking && entry.deadline(power).is_some_and(|due| due <= env.now());
        (due && (entry.work.importance == Importance::Expected || optional_now))
            .then_some(Why::Deadline)
    }
}

impl Entry {
    fn new(work: Work, owner: String, now: Instant) -> Self {
        Self {
            work,
            owner,
            declared: now,
            attempt: 0,
            job: None,
            progress: None,
            told: None,
        }
    }
}

impl Kernel {
    /// Declares or takes back work ([`Command::Work`],
    /// [`Command::CancelWork`]), takes back requests
    /// ([`Command::CancelHttp`]), wears a power mode ([`Command::Wear`]),
    /// goes online for the screen in front ([`Command::GoOnline`]).
    pub(crate) fn apply_work(&mut self, command: Command) {
        match command {
            Command::Work(work) => self
                .works
                .declare(*work, self.now, &mut self.output.commands),
            Command::CancelWork(id, key) => {
                self.works.cancel(&id, key.as_deref());
                self.close_ended_runs();
            }
            Command::CancelHttp(reply) => self.io.http.cancel(&reply, &mut self.output.commands),
            Command::Wear(wear) => {
                self.wear(wear);
                self.sync_net_policy();
            }
            Command::GoOnline => {
                self.works.online_for = self.stack.last().map(|top| top.id.clone());
                self.sync_net_policy();
                // What waited may go on this connection.
                self.output.commands.push(events::WORK_ALLOWED.emit(()));
            }
            // The reader's choice alone: what background work sends is
            // not (it would lift Offline for itself).
            Command::GoOnlineFor(how_long) if self.shared.reader => {
                let how_long = how_long.min(paperpaw_sdk::power::MAX_ONLINE_FOR);
                let millis = u64::try_from(how_long.as_millis()).unwrap_or(u64::MAX);
                self.works.online_until = Some(self.now.add_millis(millis));
                self.sync_net_policy();
                self.output.commands.push(events::WORK_ALLOWED.emit(()));
            }
            _ => {}
        }
    }

    /// Wears a power mode: its level and how much longer work may wait,
    /// its values and limits over the reader's own. Plugins hear of the
    /// mode and of every setting whose value shows otherwise now; work held
    /// back by Offline may go on once another mode is worn.
    fn wear(&mut self, wear: Wear) {
        self.wear_mode(wear, true);
        self.keep_power();
    }

    /// Writes the worn mode down, to wear again at the next start.
    pub(crate) fn keep_power(&mut self) {
        let power = &self.shared.power;
        let released: Vec<&str> = self
            .shared
            .settings
            .released()
            .iter()
            .map(|id| id.as_str())
            .collect();
        let text = alloc::format!(
            "mode = {}\nautomatic = {}\nreleased = {}\n",
            power.mode,
            if power.automatic { "on" } else { "off" },
            released.join(", ")
        );
        let _ = self.shared.storage.write(POWER_FILE, text.as_bytes());
    }

    /// Wears the mode kept at the last start again, with the values the
    /// reader took back, before plugins start: the plugin that chooses
    /// then finds it worn and changes nothing over the reader's choice.
    pub(crate) fn restore_power(&mut self) {
        let Ok(bytes) = paperpaw_sdk::read_file(self.shared.storage.as_ref(), POWER_FILE, 4096)
        else {
            return;
        };
        let text = String::from_utf8_lossy(&bytes);
        let value = |key: &str| {
            text.lines()
                .filter_map(|line| line.split_once('='))
                .find(|(k, _)| k.trim() == key)
                .map(|(_, v)| v.trim())
        };
        let Some(mode) = value("mode").and_then(|m| Id::parse(m).ok()) else {
            return;
        };
        let wear = Wear::new(mode);
        let wear = if value("automatic") == Some("on") {
            wear.automatic()
        } else {
            wear
        };
        self.wear_mode(wear, false);
        for id in value("released").unwrap_or_default().split(',') {
            if let Ok(id) = Id::parse(id.trim()) {
                self.shared.settings.release(&id);
            }
        }
    }

    /// Whether setting `id` is guarded ([`SettingSpec::guarded`]): no
    /// power mode sets it (the lock's settings).
    ///
    /// [`SettingSpec::guarded`]: paperpaw_sdk::SettingSpec::guarded
    fn guarded(&self, id: &Id) -> bool {
        self.shared
            .settings
            .spec(id.as_str())
            .is_some_and(|spec| spec.guarded)
    }

    /// Wears `wear`'s mode; `tell` plugins of it (not before they start).
    /// Guarded settings stay as they are: a mode is any plugin's preset.
    fn wear_mode(&mut self, wear: Wear, tell: bool) {
        let Some(preset) = self
            .shared
            .settings
            .presets()
            .get(wear.mode.as_str())
            .filter(|p| p.kind == power::MODE_KIND)
        else {
            self.warn_runtime(alloc::format!("no power mode `{}`", wear.mode));
            return;
        };
        let mut level = PowerLevel::Normal;
        let mut stretch = 1;
        let mut values = Vec::new();
        let mut limits = Vec::new();
        for (id, text) in preset.values() {
            if id == power::LEVEL.as_str() {
                level = PowerLevel::parse(text).unwrap_or(PowerLevel::Normal);
            } else if id == power::STRETCH.as_str() {
                stretch = text.parse().unwrap_or(1);
            } else if let Some(id) = id.strip_suffix('<') {
                // `id <= number`: a limit.
                if let (Ok(id), Ok(most)) = (Id::parse(id.trim()), text.parse())
                    && !self.guarded(&id)
                {
                    limits.push((id, most));
                }
            } else if let (Ok(id), Some(spec)) = (Id::parse(id), self.shared.settings.spec(id))
                && !spec.guarded
                && let Some(value) = spec.kind.parse(text)
            {
                values.push((id, value));
            }
        }
        let before = self.shared.power.clone();
        let touched = self.shared.settings.wear(values, limits);
        self.shared.power = Power::new(level, wear.mode, wear.automatic, stretch);
        // Deadlines and holds go by the level.
        self.works.changed = true;
        if !tell {
            return;
        }
        for id in touched {
            let changed = events::SettingChanged::new(id);
            self.output
                .commands
                .push(events::SETTING_CHANGED.emit(changed));
        }
        if self.shared.power != before {
            self.output.commands.push(events::POWER_CHANGED.emit(()));
            self.native_hint(crate::native_host::registrations::EVENT_POWER_CHANGED_TAG);
        }
        if before.level == PowerLevel::Offline && level != PowerLevel::Offline {
            self.output.commands.push(events::WORK_ALLOWED.emit(()));
        }
    }

    /// Keeps what background work said, each plugin's last as its note,
    /// and warns of work declared under another plugin's name.
    pub(crate) fn keep_notes(&mut self) {
        for plugin in mem::take(&mut self.works.foreign) {
            self.warn_runtime(alloc::format!(
                "{plugin}: declared work under another plugin's name; it was dropped"
            ));
        }
        for (owner, note) in mem::take(&mut self.works.notes) {
            if let Ok(owner) = Id::parse(&owner) {
                self.shared.set_work_note(owner, note);
            }
        }
    }

    /// Whether the reader chose *Go online* on a screen still open, or
    /// for a while that has not passed.
    fn went_online(&self) -> bool {
        self.works
            .online_for
            .as_ref()
            .is_some_and(|id| self.stack.iter().any(|s| s.id == *id))
            || self
                .works
                .online_until
                .is_some_and(|until| self.now < until)
    }

    /// When the while the reader went online for ends, to look again.
    pub(crate) fn online_ends(&self) -> Option<Instant> {
        self.works.online_until.filter(|until| self.now < *until)
    }

    /// The power work goes by: the mode's, but while the reader went
    /// online in Offline, as Saving (what waited rides along).
    pub(crate) fn base_power(&self) -> Power {
        let power = &self.shared.power;
        if power.level == PowerLevel::Offline && self.went_online() {
            Power::new(
                PowerLevel::Saving,
                power.mode.clone(),
                power.automatic,
                power.stretch,
            )
        } else {
            power.clone()
        }
    }

    /// Reads what the reader set per plugin again, if it changed.
    pub(crate) fn read_policy(&mut self) {
        if !mem::take(&mut self.works.stale) {
            return;
        }
        let settings = &self.shared.settings;
        let networked = self
            .shared
            .plugins
            .iter()
            .filter(|p| p.manifest.uses.contains(Uses::NETWORK))
            .map(|p| String::from(p.manifest.id.as_str()))
            .chain(self.works.natives.iter().cloned());
        let (mut off, mut exempt) = (Vec::new(), Vec::new());
        for plugin in networked {
            let network = settings.get(&network_setting(&plugin));
            if network == Some(&paperpaw_sdk::SettingValue::Bool(false)) {
                off.push(plugin.clone());
            }
            let choice = match settings.text(&saving_setting(&plugin)) {
                "core.saving.normal" => Exempt::Normal,
                "core.saving.manual" => Exempt::Manual,
                _ => Exempt::Follow,
            };
            if choice != Exempt::Follow {
                exempt.push((plugin, choice));
            }
        }
        self.works.off = off;
        self.works.exempt = exempt;
        self.works.policy_version = self.works.policy_version.wrapping_add(1);
    }

    /// Gives the native services the network policy as it is now. Called
    /// wherever what it follows changes (a setting, the power mode, going
    /// online) and at the end of every call, so that the next callback of a
    /// plugin, in this call or the next, is answered by what the user has
    /// just decided, never by what he had decided before.
    pub(crate) fn sync_net_policy(&mut self) {
        if let Some(policy) = self.shared.native_instances.net_policy() {
            self.refresh_net_policy(&policy);
        }
    }

    /// Gives the network policy of the native services (`core.net`,
    /// `core.sockets`) what the user decided: the mode and each plugin's own
    /// setting, as interface 6 applies them to a plugin's network use. Cheap
    /// when nothing changed.
    pub(crate) fn refresh_net_policy(
        &mut self,
        policy: &crate::native_host::net_policy::NetPolicy,
    ) {
        self.read_policy();
        let mode_offline = self.shared.power.level == PowerLevel::Offline;
        let offline = mode_offline && !self.went_online();
        let version = self.works.policy_version;
        if !policy.needs(version, offline, mode_offline) {
            return;
        }
        let off = self.works.off.iter().map(String::as_str);
        let held = self
            .works
            .exempt
            .iter()
            .filter(|(_, e)| *e == Exempt::Manual)
            .map(|(p, _)| p.as_str());
        policy.update(version, offline, mode_offline, off, held);
    }

    /// Runs the work that is due, as background work: its events emitted
    /// and their commands carried out now, outside the reader's call (what
    /// they say becomes notes, their requests are not the reader's); and a
    /// job's step where allowed. Whether anything ran, or moments wait.
    pub(crate) fn run_work(&mut self) -> bool {
        if self.works.held {
            return false;
        }
        self.read_policy();
        // Network work of a plugin whose network is off can never run.
        let mut index = 0;
        while index < self.works.entries.len() {
            let entry = &self.works.entries[index];
            if entry.work.needs.contains(Needs::NETWORK) && self.works.network_off(&entry.owner) {
                let entry = self.works.entries.remove(index);
                self.works.changed = true;
                self.works
                    .runs
                    .end_work(&entry.work.id, Some(&entry.work.key));
                let refused = WorkRefused::new(
                    entry.work.id,
                    Refusal::new(Reason::NetworkOff, Retry::Never),
                );
                self.output
                    .commands
                    .push(events::WORK_REFUSED.emit(refused));
            } else {
                index += 1;
            }
        }
        let base = self.base_power();
        // Each work due: its id, parameters, key and plugin.
        let mut due: Vec<(Id, Params, String, String)> = Vec::new();
        let mut notes: Vec<(Id, paperpaw_sdk::work::Note)> = Vec::new();
        let mut stepped = false;
        {
            let env = self.shared.env(self.now);
            let moments = self.works.take_moments(&env, self.last_activity);
            if moments.contains(Moments::SLEEP) {
                self.works.keep = true;
            }
            let mut index = 0;
            while index < self.works.entries.len() {
                let Some(why) = self
                    .works
                    .why(&self.works.entries[index], moments, &env, &base)
                else {
                    index += 1;
                    continue;
                };
                self.works.changed = true;
                let entry = &mut self.works.entries[index];
                if self.works.specs.iter().any(|s| s.id == entry.work.id) {
                    entry.job = Some(Job::Ready);
                    entry.progress = None;
                    self.works
                        .runs
                        .start(&entry.work.id, &entry.work.key, &entry.owner);
                    index += 1;
                    continue;
                }
                let mut params = entry.work.params.clone();
                // A timer's event comes as it was asked for.
                if why != Why::Time {
                    why.write(&mut params);
                }
                due.push((
                    entry.work.id.clone(),
                    params,
                    entry.work.key.clone(),
                    entry.owner.clone(),
                ));
                if again(entry, self.now) {
                    index += 1;
                } else {
                    self.works.entries.remove(index);
                }
            }
            if mem::take(&mut self.works.ticking) {
                let meter = &self.shared.meter;
                let top = self.stack.last().map(|s| s.id.namespace());
                let out = &mut self.output.commands;
                stepped = step_job(&mut self.works, top, &env, meter, out, &mut notes);
            }
        }
        self.works.allow_full(&mut self.output.commands);
        if mem::take(&mut self.works.keep) {
            self.keep_power();
        }
        if mem::take(&mut self.works.changed) {
            let states = self.works.states(&base);
            self.shared.work_states.set_works(states);
        }
        for (owner, note) in notes {
            self.shared.set_work_note(owner, note);
        }
        // Background work, whatever call brought it due.
        let ran = !due.is_empty() || stepped;
        if ran {
            let reader = mem::replace(&mut self.shared.reader, false);
            for (id, params, key, owner) in due {
                self.emit_run(&id, &params, &key, &owner);
            }
            self.run_commands();
            self.shared.reader = reader;
        }
        self.settle_runs();
        ran || !self.works.moments.is_empty()
    }
}

impl Kernel {
    /// Network use of a plugin whose manifest does not declare it
    /// ([`Uses::NETWORK`]): refused and answered as such. The command
    /// back where it may go on. A request's plugin is its reply's (a
    /// server's, a socket's): their ids are in the plugin's namespace.
    pub(crate) fn declared_network(&mut self, command: Command) -> Option<Command> {
        let owner = match &command {
            Command::Http(request) => request.reply().namespace(),
            Command::Serve { server, .. } => server.namespace(),
            Command::Connect { socket, .. } => socket.namespace(),
            Command::Broadcast(broadcast) => broadcast.id().namespace(),
            _ => return Some(command),
        };
        // The kernel's own, and what no plugin it knows asked for, go on.
        let declared = owner == crate::core_plugin::CORE.as_str()
            || self
                .shared
                .plugins
                .get(owner)
                .is_none_or(|p| p.manifest.uses.contains(Uses::NETWORK));
        let owner = String::from(owner);
        self.read_policy();
        let offline = self.base_power().level == PowerLevel::Offline;
        let manual = self.works.power_of(&self.shared.power, &owner).level == PowerLevel::Offline
            && self.shared.power.level != PowerLevel::Offline;
        let refusal = if !declared {
            Refusal::new(Reason::NotDeclared, Retry::Never)
        } else if self.works.network_off(&owner) {
            Refusal::new(Reason::NetworkOff, Retry::Never)
        } else if offline && !(self.shared.reader && self.went_online()) {
            // Offline: only what the reader went online for goes out.
            Refusal::new(Reason::Offline, Retry::WhenAllowed)
        } else if manual && !self.shared.reader {
            // Only when the reader asks, for this plugin.
            Refusal::new(Reason::Saving, Retry::WhenAllowed)
        } else {
            self.shared.meter.network(&owner, false);
            return Some(command);
        };
        self.shared.meter.network(&owner, true);
        let reason = String::from(refusal.reason.as_str());
        let (event, params) = match command {
            Command::Http(request) => {
                let request = request.into_parts();
                let reply = HttpReply::refused(refusal, request.params);
                (request.reply, reply.into_params())
            }
            Command::Serve { server, events, .. } => {
                (events, ServerNotice::Failed { server, reason }.to_params())
            }
            Command::Connect { socket, events, .. } => (
                events,
                SocketNotice::Failed {
                    socket,
                    reason,
                    error: SocketError::NotAllowed,
                }
                .to_params(),
            ),
            Command::Broadcast(broadcast) => {
                let socket = broadcast.id().clone();
                let failed = SocketNotice::Failed {
                    socket,
                    reason,
                    error: SocketError::NotAllowed,
                };
                (broadcast.events().clone(), failed.to_params())
            }
            _ => return None,
        };
        self.output.commands.push(Command::Emit(event, params));
        if refusal.reason == Reason::NotDeclared {
            self.warn_runtime(alloc::format!(
                "{owner}: uses the network without declaring it (`Manifest::uses`)"
            ));
        }
        None
    }

    /// The reader's settings for each plugin that uses the network: its
    /// network on or off, and its work in saving modes (ADR 0025), a
    /// part of the group *Network per plugin* each, named as the plugin.
    pub(crate) fn define_network_settings(&mut self, fixed: &'static [NetworkSettings]) {
        let networked: Vec<String> = self
            .shared
            .plugins
            .iter()
            .filter(|p| {
                p.manifest.uses.contains(Uses::NETWORK) && p.manifest.id != crate::core_plugin::CORE
            })
            .map(|p| String::from(p.manifest.id.as_str()))
            .collect();
        self.works.fixed = fixed;
        for plugin in networked {
            if let Err(e) = self.define_network_pair(&plugin, false) {
                self.warn_runtime(alloc::format!("{plugin}: {e:?}"));
            }
        }
        self.works.stale = true;
    }
}

/// One step of the first job ready.
fn step_job(
    works: &mut Queue,
    top: Option<&str>,
    env: &Env<'_>,
    meter: &alloc::rc::Rc<crate::meter::Meter>,
    out: &mut Vec<Command>,
    notes: &mut Vec<(Id, paperpaw_sdk::work::Note)>,
) -> bool {
    let now = env.now();
    for entry in &mut works.entries {
        if let Some(Job::Until(at)) = entry.job
            && at <= now
        {
            entry.job = Some(Job::Ready);
        }
    }
    let Some(index) = works.entries.iter().position(|e| e.job == Some(Job::Ready)) else {
        return false;
    };
    works.changed = true;
    let Some(spec) = works
        .specs
        .iter()
        .find(|s| s.id == works.entries[index].work.id)
    else {
        works.entries.remove(index);
        return false;
    };
    let entry = &mut works.entries[index];
    let from = out.len();
    let step = {
        let _running = meter.enter(&entry.owner);
        crate::commands::handing(out, |out| (spec.step)(env, &entry.work.params, out))
    };
    // A job is background work: its notices become its plugin's note.
    let mut said = Queue::default();
    quiet(false, top, now, from, &entry.owner, out, &mut said);
    if let Some(run) = works.runs.of(&entry.work.id, &entry.work.key) {
        works.runs.claim(run, out.get(from..).unwrap_or_default());
    }
    works.foreign.append(&mut said.foreign);
    for (owner, note) in said.notes {
        if let Ok(owner) = Id::parse(&owner) {
            notes.push((owner, note));
        }
    }
    let tell = |entry: &mut Entry, out: &mut Vec<Command>| {
        entry.told = Some(now);
        let params = Params::new().with(params::ID, String::from(entry.work.id.as_str()));
        out.push(Command::Emit(events::WORK_CHANGED.id().clone(), params));
    };
    match step {
        Step::Continue => {}
        Step::Progress(progress) => {
            entry.progress = Some(progress);
            if entry.told.is_none_or(|t| now.since(t) >= PROGRESS_MS) {
                tell(entry, out);
            }
        }
        Step::Wait(at) => entry.job = Some(Job::Until(at)),
        Step::WaitFor(event) => entry.job = Some(Job::For(event)),
        Step::Done => {
            works.runs.end_work(&entry.work.id, Some(&entry.work.key));
            entry.job = None;
            entry.attempt = 0;
            tell(entry, out);
            if !again(entry, now) {
                works.entries.remove(index);
            }
        }
        Step::Failed(text) => {
            works.runs.end_work(&entry.work.id, Some(&entry.work.key));
            entry.job = None;
            entry.attempt = entry.attempt.saturating_add(1);
            entry.declared = now;
            let mut jitter = [0u8];
            env.random().fill(&mut jitter);
            let jitter = jitter[0];
            let wait = paperpaw_sdk::work::backoff(entry.attempt, jitter);
            // A timed job tries again at a new time, others after the wait.
            if entry.work.at.is_some() {
                entry.work.at = Some(now.add_millis(millis(Some(wait))));
            } else {
                entry.work.after = Some(wait);
            }
            let owner = Id::parse(&entry.owner).unwrap_or_else(|_| entry.work.id.clone());
            tell(entry, out);
            notes.push((owner, paperpaw_sdk::work::Note::new(now, text)));
        }
        // A step this kernel does not know yet: the job goes on.
        _ => {}
    }
    true
}

/// What a plugin's handler sent, from `from` on in `out`: work declared
/// under another plugin's name is taken out (each plugin's limit is its
/// own; the name kept in `works.foreign` for a warning); and background
/// work says nothing over reading: notices made by `owner` outside the
/// reader's call and while another plugin's screen is on top (`top`)
/// become its note (ADR 0025). On its own screen a plugin may say what it
/// likes.
pub(crate) fn quiet(
    reader: bool,
    top: Option<&str>,
    now: Instant,
    from: usize,
    owner: &str,
    out: &mut Vec<Command>,
    works: &mut Queue,
) {
    let mut index = from;
    while index < out.len() {
        if matches!(&out[index], Command::Work(work) if work.id.namespace() != owner) {
            out.remove(index);
            works.foreign.push(String::from(owner));
        } else {
            index += 1;
        }
    }
    if reader || top == Some(owner) {
        return;
    }
    let mut index = from;
    while index < out.len() {
        if let Command::Notify(text) = &out[index] {
            let note = paperpaw_sdk::work::Note::new(now, text.clone());
            works.notes.push((String::from(owner), note));
            out.remove(index);
        } else {
            index += 1;
        }
    }
}

/// Declares `entry` again after its period, if it has one; whether it
/// stays.
fn again(entry: &mut Entry, now: Instant) -> bool {
    let Some(every) = entry.work.every else {
        return false;
    };
    entry.declared = now;
    // A timer comes again a period later; other work after its period.
    if entry.work.at.is_some() {
        entry.work.at = Some(now.add_millis(millis(Some(every))));
    } else {
        entry.work.after = Some(every);
    }
    entry.job = None;
    true
}

impl crate::Shared {
    /// Bound retained note text once, so all readers see the same text.
    pub(crate) fn set_work_note(&mut self, owner: Id, mut note: paperpaw_sdk::work::Note) {
        let mut end = note.text.len().min(256);
        while !note.text.is_char_boundary(end) {
            end -= 1;
        }
        if note.text.capacity() > 256 {
            let mut bounded = String::new();
            // A transient notice must not retain a large allocation. If the
            // bounded copy cannot fit, leave the previous note in place.
            if bounded.try_reserve_exact(end).is_err() {
                return;
            }
            bounded.push_str(&note.text[..end]);
            note.text = bounded;
        } else {
            note.text.truncate(end);
        }
        self.work_states.set_note(owner, note);
    }
}

/// The network's group, its icon and the network manager setting, on the
/// core plugin (`core_plugin.rs`).
///
/// The manager is the plugin that brings a network up when work or a plugin
/// wants one (`event.network_wanted`/`idle` go to it alone): the Wi-Fi plugin,
/// as in Beta 3. Where there is none by that id the events go nowhere; empty
/// (or another id) is the user's choice, and a reset brings the default back.
pub(crate) fn register_network(
    r: &mut paperpaw_sdk::Registrar,
) -> Result<(), paperpaw_sdk::RegisterError> {
    use paperpaw_sdk::{Id, SettingGroupSpec, SettingSpec, builtin::groups};
    // Each plugin's network (its settings made by the kernel).
    static NETWORK: SettingGroupSpec = SettingGroupSpec::new(NETWORK_GROUP, "core.group.network")
        .order(87)
        .within(groups::PLUGINS);
    r.add_setting_group(&NETWORK)?;
    static NETWORK_ICON: [(Id, &str); 1] = [(NETWORK_GROUP, "world")];
    r.add_icons(&NETWORK_ICON)?;
    static MANAGER: [SettingSpec; 1] = [SettingSpec::text(
        crate::native_host::commands::dev::NETWORK_MANAGER,
        "core.setting.network-manager",
        "wifi",
        NETWORK_GROUP,
    )
    // Guarded: the manager joins and hosts without the user's input and hears
    // `network_wanted`, so only the settings editor (the user) may name it, never
    // another plugin by command, default or preset.
    .guarded()
    .advanced()
    .description("core.setting.network-manager.help")];
    r.add_settings(&MANAGER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    fn power(level: PowerLevel) -> Power {
        Power::new(level, paperpaw_sdk::power::NORMAL, false, 4)
    }

    #[test]
    fn deadlines_stretch_in_saving_and_optional_ones_wait() {
        let work = Work::network(Id::new_static("a.b")).within(Duration::from_secs(60));
        let entry = Entry::new(work.clone(), String::from("a"), Instant(1000));
        assert_eq!(
            entry.deadline(&power(PowerLevel::Normal)),
            Some(Instant(61_000))
        );
        assert_eq!(
            entry.deadline(&power(PowerLevel::Saving)),
            Some(Instant(241_000))
        );
        let optional = Entry::new(work.optional(), String::from("a"), Instant(1000));
        assert_eq!(optional.deadline(&power(PowerLevel::Saving)), None);
        assert!(entry.held(&power(PowerLevel::Offline)));
        assert!(!entry.held(&power(PowerLevel::Saving)));
    }

    #[test]
    fn the_same_id_and_key_replace_and_a_plugin_has_a_limit() {
        let mut queue = Queue::new(2);
        let mut out = Vec::new();
        let id = Id::new_static("a.b");
        queue.declare(Work::new(id.clone()), Instant(0), &mut out);
        queue.declare(Work::new(id.clone()), Instant(5), &mut out);
        assert_eq!(queue.entries.len(), 1);
        queue.declare(Work::new(id.clone()).key("2"), Instant(5), &mut out);
        queue.declare(Work::new(id.clone()).key("3"), Instant(5), &mut out);
        assert_eq!(queue.entries.len(), 2);
        assert_eq!(out.len(), 1, "the third is refused");
        queue.cancel(&id, Some("2"));
        assert_eq!(queue.entries.len(), 1);
        queue.cancel(&id, None);
        assert!(queue.entries.is_empty());
    }

    #[test]
    fn moments_come_from_events() {
        let mut queue = Queue::new(8);
        queue.saw(&events::STARTED.id().clone(), &Params::new());
        queue.saw(&events::BOOK_CLOSED.id().clone(), &Params::new());
        assert_eq!(queue.moments, Moments::WAKE | Moments::BOOK_CLOSED);
    }
}
