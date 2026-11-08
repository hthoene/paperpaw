//! What Power decides, without a host: which mode the battery calls for, and
//! what the power menu offers and does. Everything here is plain data in,
//! plain data out, so it is tested without a device.

/// How much the device saves. Ordered: Normal < Saving < Offline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Everything as configured.
    #[default]
    Normal,
    /// Every feature kept, but on demand.
    Saving,
    /// Reading only: no network on its own.
    Offline,
}

impl Level {
    /// The preset of the kind `core.power-mode` that wears this level.
    #[must_use]
    pub const fn preset(self) -> &'static str {
        match self {
            Self::Normal => "core.normal",
            Self::Saving => "core.saving",
            Self::Offline => "core.offline",
        }
    }
}

/// Automatic changes back only this far above a level, so that it does not go
/// to and fro.
pub const MARGIN: i64 = 5;
/// The choice of `power.mode` that goes by the battery.
pub const AUTOMATIC: &str = "power.mode.auto";

/// The battery, as far as the choice cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    /// Charge in percent.
    pub percent: u8,
    /// Charging now.
    pub charging: bool,
    /// The system's own saver (Android's) is on.
    pub saver: bool,
}

/// The user's choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy<'a> {
    /// The value of `power.mode`.
    pub mode: &'a str,
    /// Automatic: Saving below this percentage.
    pub saving_below: i64,
    /// Automatic: Offline below this percentage.
    pub offline_below: i64,
    /// Automatic: Normal while charging.
    pub normal_while_charging: bool,
}

/// The mode to wear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    /// How much it saves.
    pub level: Level,
    /// The battery chose it, not the user.
    pub automatic: bool,
}

/// The mode to wear now, and whether the battery chose it. `worn` is the
/// level worn at present (Automatic keeps it until the charge is clearly above).
#[must_use]
pub fn choose(policy: &Policy<'_>, battery: Option<Battery>, worn: Level) -> Choice {
    let by_hand = match policy.mode {
        "power.mode.normal" => Some(Level::Normal),
        "power.mode.saving" => Some(Level::Saving),
        "power.mode.offline" => Some(Level::Offline),
        _ => None,
    };
    match by_hand {
        Some(level) => Choice {
            level,
            automatic: false,
        },
        None => Choice {
            level: automatic(policy, battery, worn),
            automatic: true,
        },
    }
}

/// What Automatic chooses by the battery: Normal without one.
fn automatic(policy: &Policy<'_>, battery: Option<Battery>, worn: Level) -> Level {
    let Some(battery) = battery else {
        return Level::Normal;
    };
    if battery.charging && policy.normal_while_charging {
        return Level::Normal;
    }
    // The system's own saver on: Saving at least.
    let least = if battery.saver {
        Level::Saving
    } else {
        Level::Normal
    };
    let percent = i64::from(battery.percent);
    // Wearing one already: kept until the charge is clearly above it.
    let below = |limit: i64, current: Level| {
        let margin = if worn >= current { MARGIN } else { 0 };
        percent < limit.saturating_add(margin)
    };
    if below(policy.offline_below, Level::Offline) {
        Level::Offline
    } else if below(policy.saving_below, Level::Saving) {
        Level::Saving
    } else {
        least
    }
}

/// Whether wearing `choice` changes anything.
#[must_use]
pub fn must_wear(choice: Choice, worn_mode: &str, worn_automatic: bool) -> bool {
    worn_mode != choice.level.preset() || worn_automatic != choice.automatic
}

/// What the power menu can offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// Show the sleep screen and sleep.
    Sleep,
    /// Show the off screen and switch off.
    Off,
    /// Restart.
    Restart,
    /// Lock touch.
    Lock,
    /// Unlock touch.
    Unlock,
}

impl Item {
    /// The translation key of its label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sleep => "power.menu.sleep",
            Self::Off => "power.menu.off",
            Self::Restart => "power.menu.restart",
            Self::Lock => "power.menu.lock",
            Self::Unlock => "power.menu.unlock",
        }
    }
}

/// What the device can do, as far as the menu cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// PaperPaw puts the device to sleep (and shows the sleep screen first).
    pub sleep: bool,
    /// PaperPaw switches the device off.
    pub power_off: bool,
    /// The device has touch.
    pub touch: bool,
}

/// The options offered, in order (at most five).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    items: [Option<Item>; 5],
}

impl Options {
    /// The options as a slice-like iterator.
    pub fn iter(&self) -> impl Iterator<Item = Item> + '_ {
        self.items.iter().flatten().copied()
    }

    /// The option at `index`.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<Item> {
        self.items.get(index).copied().flatten()
    }

    /// How many.
    #[must_use]
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// Whether there are none (never).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// What the power menu offers here: sleep and restart always, switching off
/// where PaperPaw does so, and on a touch device (with the touch lock about,
/// `touch_locked` being its setting) locking touch or, while it is locked,
/// unlocking it: the buttons work meanwhile, so the menu is the way out.
#[must_use]
pub fn menu_options(caps: Caps, touch_locked: Option<bool>) -> Options {
    let mut items = [None; 5];
    let mut count = 0;
    let mut push = |item| {
        items[count] = Some(item);
        count += 1;
    };
    push(Item::Sleep);
    if caps.power_off {
        push(Item::Off);
    }
    push(Item::Restart);
    if caps.touch
        && let Some(locked) = touch_locked
    {
        push(if locked { Item::Unlock } else { Item::Lock });
    }
    Options { items }
}

/// What a chosen option does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Show the sleep screen, then sleep.
    SleepWithScreen,
    /// Sleep at once (no sleep screen on this device).
    Sleep,
    /// Show the sleep screen as the *off* screen, then switch off.
    OffWithScreen,
    /// Switch off at once.
    Off,
    /// Restart.
    Restart,
    /// Run the touch lock's toggle action.
    ToggleTouchLock,
}

/// What the option chosen in the power menu does. Sleeping and switching off
/// show the sleep screen first, as the power button's short press does: e-ink
/// keeps it, so a device that is off does not look as if it were on (#146).
/// Where the device cannot sleep or switch off the screen is not shown.
#[must_use]
pub fn plan(item: Item, caps: Caps) -> Step {
    match item {
        Item::Sleep if caps.sleep => Step::SleepWithScreen,
        Item::Sleep => Step::Sleep,
        Item::Off if caps.power_off => Step::OffWithScreen,
        Item::Off => Step::Off,
        Item::Restart => Step::Restart,
        Item::Lock | Item::Unlock => Step::ToggleTouchLock,
    }
}

#[cfg(test)]
mod tests;
