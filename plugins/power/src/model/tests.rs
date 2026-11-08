//! The decisions of the interface-6 Power tests (`modes.rs`, `menu.rs`,
//! `off_screen.rs`) that need no host.
use super::*;

const POLICY: Policy<'static> = Policy {
    mode: AUTOMATIC,
    saving_below: 20,
    offline_below: 10,
    normal_while_charging: true,
};

fn battery(percent: u8, charging: bool) -> Option<Battery> {
    Some(Battery {
        percent,
        charging,
        saver: false,
    })
}

/// The level Automatic wears, going on from the one worn so far.
fn run(readings: &[(u8, bool)]) -> alloc::vec::Vec<Level> {
    let mut worn = Level::Normal;
    readings
        .iter()
        .map(|&(percent, charging)| {
            worn = choose(&POLICY, battery(percent, charging), worn).level;
            worn
        })
        .collect()
}
extern crate alloc;

#[test]
fn automatic_goes_by_the_battery_and_back_with_a_margin() {
    use Level::{Normal, Offline, Saving};
    assert_eq!(
        run(&[
            (80, false),
            (19, false),
            // Just above the level: still saving, not to and fro.
            (22, false),
            (9, false),
            (12, false),
            // Charging: Normal.
            (12, true),
            (30, false),
        ]),
        [Normal, Saving, Saving, Offline, Offline, Normal, Normal]
    );
    // Clearly above the level, Saving ends.
    assert_eq!(run(&[(19, false), (25, false)]), [Saving, Normal]);
}

#[test]
fn automatic_marks_the_choice_as_the_batterys() {
    let choice = choose(&POLICY, battery(80, false), Level::Normal);
    assert_eq!(
        choice,
        Choice {
            level: Level::Normal,
            automatic: true
        }
    );
    assert!(!must_wear(choice, "core.normal", true));
    assert!(must_wear(choice, "core.normal", false), "was by hand");
    assert!(must_wear(choice, "core.saving", true));
}

#[test]
fn normal_while_charging_can_be_switched_off() {
    let policy = Policy {
        normal_while_charging: false,
        ..POLICY
    };
    assert_eq!(
        choose(&policy, battery(12, true), Level::Normal).level,
        Level::Saving
    );
}

/// The system's own battery saver (Android's) on: Automatic wears Saving
/// however full the battery, Offline still below its level, and Normal again
/// once the saver is off.
#[test]
fn automatic_follows_the_system_saver() {
    let saver = |percent, on| {
        Some(Battery {
            percent,
            charging: false,
            saver: on,
        })
    };
    assert_eq!(
        choose(&POLICY, saver(90, true), Level::Normal).level,
        Level::Saving
    );
    assert_eq!(
        choose(&POLICY, saver(5, true), Level::Saving).level,
        Level::Offline
    );
    assert_eq!(
        choose(&POLICY, saver(90, false), Level::Offline).level,
        Level::Normal
    );
}

#[test]
fn no_battery_means_normal() {
    assert_eq!(
        choose(&POLICY, None, Level::Offline),
        Choice {
            level: Level::Normal,
            automatic: true
        }
    );
}

#[test]
fn a_mode_chosen_by_hand_stays_whatever_the_battery() {
    for (mode, level) in [
        ("power.mode.normal", Level::Normal),
        ("power.mode.saving", Level::Saving),
        ("power.mode.offline", Level::Offline),
    ] {
        let policy = Policy { mode, ..POLICY };
        for percent in [1, 50, 100] {
            assert_eq!(
                choose(&policy, battery(percent, true), Level::Normal),
                Choice {
                    level,
                    automatic: false
                }
            );
        }
    }
    // A value this version does not know is Automatic.
    let policy = Policy {
        mode: "power.mode.future",
        ..POLICY
    };
    assert!(choose(&policy, None, Level::Normal).automatic);
}

#[test]
fn thresholds_at_the_edge_of_the_ranges_do_not_overflow() {
    let policy = Policy {
        saving_below: i64::MAX,
        offline_below: i64::MIN,
        ..POLICY
    };
    assert_eq!(
        choose(&policy, battery(100, false), Level::Saving).level,
        Level::Saving
    );
}

fn caps(sleep: bool, power_off: bool, touch: bool) -> Caps {
    Caps {
        sleep,
        power_off,
        touch,
    }
}

fn labels(options: Options) -> alloc::vec::Vec<&'static str> {
    options.iter().map(Item::label).collect()
}

#[test]
fn what_the_menu_offers_goes_by_the_device() {
    // Buttons only: no touch lock.
    assert_eq!(
        labels(menu_options(caps(true, true, false), Some(false))),
        ["power.menu.sleep", "power.menu.off", "power.menu.restart"]
    );
    // Switching off only where PaperPaw does it.
    assert_eq!(
        labels(menu_options(caps(true, false, true), Some(false))),
        ["power.menu.sleep", "power.menu.restart", "power.menu.lock"]
    );
    // No touch lock plugin (no setting), no entry for it.
    assert_eq!(
        labels(menu_options(caps(true, true, true), None)),
        ["power.menu.sleep", "power.menu.off", "power.menu.restart"]
    );
}

#[test]
fn the_menu_unlocks_touch_by_the_buttons_while_it_is_locked() {
    let options = menu_options(caps(true, true, true), Some(true));
    assert_eq!(
        labels(options),
        [
            "power.menu.sleep",
            "power.menu.off",
            "power.menu.restart",
            "power.menu.unlock"
        ]
    );
    assert_eq!(options.len(), 4);
    assert_eq!(options.get(3), Some(Item::Unlock));
    assert_eq!(options.get(4), None);
}

#[test]
fn each_option_does_what_it_says() {
    let all = caps(true, true, true);
    assert_eq!(plan(Item::Sleep, all), Step::SleepWithScreen);
    assert_eq!(plan(Item::Off, all), Step::OffWithScreen);
    assert_eq!(plan(Item::Restart, all), Step::Restart);
    assert_eq!(plan(Item::Lock, all), Step::ToggleTouchLock);
    assert_eq!(plan(Item::Unlock, all), Step::ToggleTouchLock);
}

/// Sleeping and switching off show the sleep screen first (#146), but only
/// where the device does it.
#[test]
fn the_sleep_screen_is_shown_only_where_the_device_sleeps_or_switches_off() {
    let none = caps(false, false, false);
    assert_eq!(plan(Item::Sleep, none), Step::Sleep);
    assert_eq!(plan(Item::Off, none), Step::Off);
}
