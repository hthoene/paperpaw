//! Power levels and modes (ADR 0025).
//!
//! PaperPaw saves by default; a **mode** trades features for battery on
//! top of that. Readers choose one (Normal, Saving, Offline, Automatic by
//! the battery, or one of their own), and every mode has a **level**,
//! one of three steps plugins understand:
//!
//! - [`PowerLevel::Normal`]: everything as configured.
//! - [`PowerLevel::Saving`]: every feature kept, but on demand: declared
//!   work waits longer ([`Power::stretch`]), optional work runs only
//!   when the radio is on anyway or the device charges.
//! - [`PowerLevel::Offline`]: reading only; no network work runs on its
//!   own, it waits for a connection the reader makes.
//!
//! Most plugins never read the level: work declared with its needs and a
//! deadline ([`work`](crate::work)) is limited by the kernel. The level
//! is for what declared work does not cover, e.g. how often a widget
//! changes ([`Power::every`]). The kernel emits
//! [`events::POWER_CHANGED`](crate::builtin::events::POWER_CHANGED) when
//! the level or the mode changes.
//!
//! A mode is a preset of the kind [`MODE_KIND`] (see
//! [`presets`](crate::presets)) with its level among its values
//! (`power.level = saving`), so plugins and data plugins can bring more.

use alloc::borrow::Cow;
use core::time::Duration;

use crate::id::Id;

/// What the shell changes by the power level (ADR 0025), so that the
/// power settings show only what applies on this device
/// ([`Capabilities::power`](crate::Capabilities::power)). Everything else a
/// mode does (declared work, the radio's natural moments) applies on
/// every device.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PowerControls {
    /// The radio's own power saving follows the level.
    pub radio: bool,
    /// The processor's clock follows the level.
    pub cpu_clock: bool,
    /// How often the buttons are read follows the level (less often in
    /// Saving and Offline).
    pub buttons: bool,
    /// The front light follows the level (dimmer, off sooner).
    pub frontlight: bool,
    /// The system has a battery saver of its own (Android's), which the
    /// shell reports ([`Battery::saver`](crate::Battery)): Automatic wears
    /// Saving while it is on.
    pub system_saver: bool,
}

impl PowerControls {
    /// Whether the shell changes anything by the level.
    pub const fn any(&self) -> bool {
        self.radio || self.cpu_clock || self.buttons || self.frontlight || self.system_saver
    }
}

/// The longest the reader goes online for with
/// [`Command::GoOnlineFor`](crate::Command::GoOnlineFor): what Offline
/// held back runs within it.
pub const MAX_ONLINE_FOR: Duration = Duration::from_secs(10 * 60);

/// The longest a [`WakeHold`] lasts: one wanted longer is renewed before
/// it ends, so that a hold forgotten cannot empty the battery.
pub const MAX_HOLD: Duration = Duration::from_secs(30 * 60);

/// Bits of [`WakeHold`]'s flags.
mod hold_flags {
    /// The front light kept on too, where it would go off by itself.
    pub const LIGHT: u32 = 1;
}

/// Keeps the device from going to sleep by itself for a while
/// ([`Command::KeepAwake`](crate::Command::KeepAwake), or
/// [`Cx::keep_awake`](crate::Cx::keep_awake) for a screen): a clock or a
/// timer shown, a slideshow, a game watched, a long download. It holds
/// only the sleep after
/// [`settings::SLEEP_AFTER`](crate::builtin::settings::SLEEP_AFTER)
/// without input: the processor still naps between events, and the power
/// button or a battery nearly empty still put the device to sleep. The
/// reader can let no plugin hold it (*Let plugins keep the device
/// awake*).
///
/// Holds of the same id are one: the latest counts. Each ends after
/// [`WakeHold::lasting`] (at most [`MAX_HOLD`]) unless renewed, when
/// let go ([`Command::LetSleep`](crate::Command::LetSleep)), and, for a
/// screen's, when the screen closes; a screen's counts only while it is
/// on top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeHold {
    id: Id,
    reason: Cow<'static, str>,
    /// Milliseconds it lasts.
    lasting: u32,
    flags: u32,
    /// The screen it is for, set by `Cx::keep_awake`.
    screen: Option<crate::ScreenKey>,
    /// Room for numbers to come.
    numbers: [u32; 2],
}

impl WakeHold {
    /// A hold named `id` (in the plugin's namespace, like every id), for
    /// `reason` (a translation key or text, for the usage and
    /// diagnostics), lasting [`MAX_HOLD`].
    pub fn new(id: impl Into<Id>, reason: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id: id.into(),
            reason: reason.into(),
            lasting: MAX_HOLD.as_millis() as u32,
            flags: 0,
            screen: None,
            numbers: [0; 2],
        }
    }

    /// Lasting `lasting` unless renewed (at most [`MAX_HOLD`]).
    pub fn lasting(mut self, lasting: Duration) -> Self {
        let most = MAX_HOLD.as_millis() as u32;
        self.lasting = u32::try_from(lasting.as_millis()).unwrap_or(most).min(most);
        self
    }

    /// Keeps the front light on too, where it would go off by itself.
    pub fn light(mut self) -> Self {
        self.flags |= hold_flags::LIGHT;
        self
    }

    /// For the screen of `key` (see `Cx::keep_awake`).
    pub fn of_screen(mut self, key: Option<crate::ScreenKey>) -> Self {
        self.screen = key;
        self
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// How long it lasts unless renewed.
    pub fn duration(&self) -> Duration {
        Duration::from_millis(u64::from(self.lasting))
    }

    /// Whether it keeps the front light on too.
    pub fn keeps_light(&self) -> bool {
        self.flags & hold_flags::LIGHT != 0
    }

    /// The screen it is for, if a screen holds it.
    pub fn screen(&self) -> Option<crate::ScreenKey> {
        self.screen
    }
}

/// How much energy to spend, as plugins see it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum PowerLevel {
    /// Everything as configured.
    #[default]
    Normal,
    /// Every feature kept, but on demand.
    Saving,
    /// Reading only: no network on its own.
    Offline,
}

impl PowerLevel {
    /// Its name in settings and presets: `normal`, `saving`, `offline`.
    pub const fn as_str(self) -> &'static str {
        match self {
            PowerLevel::Normal => "normal",
            PowerLevel::Saving => "saving",
            PowerLevel::Offline => "offline",
        }
    }

    /// The level named `text` ([`as_str`](Self::as_str)).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "normal" => Some(PowerLevel::Normal),
            "saving" => Some(PowerLevel::Saving),
            "offline" => Some(PowerLevel::Offline),
            _ => None,
        }
    }
}

/// Which mode to wear ([`Command::Wear`](crate::Command::Wear)), sent by
/// the plugin that chooses (the `power` plugin).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Wear {
    /// A preset of the kind [`MODE_KIND`].
    pub mode: Id,
    /// Chosen by the battery (Automatic), not by the reader.
    pub automatic: bool,
}

impl Wear {
    pub fn new(mode: Id) -> Self {
        Self {
            mode,
            automatic: false,
        }
    }

    /// Chosen by the battery.
    pub fn automatic(mut self) -> Self {
        self.automatic = true;
        self
    }
}

/// The setting in a mode's values that says how many times longer
/// declared work may wait ([`Power::stretch`]).
pub const STRETCH: Id = Id::new_static("power.stretch");

/// The kind of presets that are power modes.
pub const MODE_KIND: Id = Id::new_static("core.power-mode");
/// The setting in a mode's values that names its level.
pub const LEVEL: Id = Id::new_static("power.level");
/// The built-in modes.
pub const NORMAL: Id = Id::new_static("core.normal");
pub const SAVING: Id = Id::new_static("core.saving");
pub const OFFLINE: Id = Id::new_static("core.offline");
/// The value of the mode setting that lets the battery choose.
pub const AUTOMATIC: &str = "auto";

/// The power level and mode in use.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Power {
    pub level: PowerLevel,
    /// The mode worn: a built-in one ([`NORMAL`] …) or a preset's id.
    pub mode: Id,
    /// Chosen by Automatic, by the battery, not by the reader.
    pub automatic: bool,
    /// How many times longer declared work may wait than it says: 1 in
    /// Normal, 4 in Saving by default (a mode's value).
    pub stretch: u32,
}

impl Default for Power {
    fn default() -> Self {
        Self::new(PowerLevel::Normal, NORMAL, false, 1)
    }
}

/// [`Power::normal`].
static NORMAL_POWER: Power = Power {
    level: PowerLevel::Normal,
    mode: NORMAL,
    automatic: false,
    stretch: 1,
};

impl Power {
    /// Normal, nothing saved beyond the defaults, e.g. for tests.
    pub fn normal() -> &'static Power {
        &NORMAL_POWER
    }

    pub fn new(level: PowerLevel, mode: Id, automatic: bool, stretch: u32) -> Self {
        Self {
            level,
            mode,
            automatic,
            stretch: stretch.max(1),
        }
    }

    /// How often to do something done every `base` milliseconds in
    /// Normal: `stretch` times as seldom in Saving, never in Offline.
    pub fn every(&self, base: u64) -> Option<u64> {
        match self.level {
            PowerLevel::Normal => Some(base),
            PowerLevel::Saving => Some(base.saturating_mul(u64::from(self.stretch))),
            PowerLevel::Offline => None,
        }
    }

    /// Whether the network may be used for work of a plugin's own.
    pub fn online(&self) -> bool {
        self.level != PowerLevel::Offline
    }
}

/// How PaperPaw started this time ([`Env::started`](crate::Env::started)),
/// where the shell can tell: e.g. for a device check, which proves a
/// sleep by the start that ends it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Start {
    /// The shell does not say.
    #[default]
    Unknown,
    /// Switched on: the power came, by the power button or the cable. A
    /// device whose sleep cuts the power (the X4) wakes so too.
    PowerOn,
    /// Woken from a deep sleep that kept the power.
    Wake,
    /// Started again with its power on: by a watchdog, a panic or a
    /// restart asked for.
    Restart,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_named_both_ways() {
        for level in [PowerLevel::Normal, PowerLevel::Saving, PowerLevel::Offline] {
            assert_eq!(PowerLevel::parse(level.as_str()), Some(level));
        }
        assert_eq!(PowerLevel::parse(" saving "), Some(PowerLevel::Saving));
        assert_eq!(PowerLevel::parse("eco"), None);
    }

    #[test]
    fn intervals_follow_the_level() {
        let normal = Power::default();
        assert_eq!(normal.every(60_000), Some(60_000));
        let saving = Power::new(PowerLevel::Saving, SAVING, true, 4);
        assert_eq!(saving.every(60_000), Some(240_000));
        let offline = Power::new(PowerLevel::Offline, OFFLINE, false, 4);
        assert_eq!(offline.every(60_000), None);
        assert!(!offline.online());
    }

    #[test]
    fn a_stretch_is_at_least_one() {
        let power = Power::new(PowerLevel::Saving, SAVING, false, 0);
        assert_eq!(power.every(10), Some(10));
    }
}
