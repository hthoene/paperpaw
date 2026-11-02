//! Staying awake on outer power (#96): a developer's choice the shell
//! passes on while a cable is in. Only the sleep after `core.sleep-after`
//! is held; an explicit sleep still reaches the shell.

use super::*;
use paperpaw_platform::{Battery, SystemStatus};

fn kernel() -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .sleep(true)
        .setting_default(&settings::SLEEP_AFTER, 1)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

fn battery(kernel: &mut Kernel, at: u64, percent: u8, charging: bool) {
    kernel.set_status(
        Instant(at),
        SystemStatus::new(None, Some(Battery::new(percent, charging, None))),
    );
}

#[test]
fn the_timer_does_not_fire_while_it_stays_awake_on_usb() {
    let mut kernel = kernel();
    kernel.set_awake_on_usb(Instant(1_000), true);
    // Far past the minute: nothing, and no wake-up is asked for.
    assert!(!kernel.tick(Instant(61_000)).sleep());
    assert!(!kernel.tick(Instant(3_600_000)).sleep());
    assert_eq!(kernel.tick(Instant(3_600_100)).wake_at(), None);
}

#[test]
fn an_explicit_sleep_still_reaches_the_shell() {
    let mut kernel = kernel();
    kernel.set_awake_on_usb(Instant(1_000), true);
    assert!(press(&mut kernel, 2_000, "power").sleep());
}

#[test]
fn the_timer_counts_from_the_cable_coming_out() {
    let mut kernel = kernel();
    kernel.set_awake_on_usb(Instant(1_000), true);
    kernel.tick(Instant(3_600_000));
    let effects = kernel.set_awake_on_usb(Instant(3_600_100), false);
    // A whole minute from now, not at once for the hour of idleness.
    assert_eq!(effects.wake_at(), Some(Instant(3_660_100)));
    assert!(!kernel.tick(Instant(3_660_000)).sleep());
    assert!(kernel.tick(Instant(3_660_200)).sleep());
}

#[test]
fn off_changes_nothing_where_it_was_never_on() {
    let mut kernel = kernel();
    kernel.set_awake_on_usb(Instant(30_000), false);
    // The minute counts from the start, as before.
    assert!(kernel.tick(Instant(60_100)).sleep());
}

#[test]
fn a_nearly_empty_battery_that_does_not_charge_sleeps_as_usual() {
    let mut kernel = kernel();
    kernel.set_awake_on_usb(Instant(1_000), true);
    battery(&mut kernel, 2_000, 8, false);
    assert!(kernel.tick(Instant(61_000)).sleep());
    // Charging, it stays awake again.
    battery(&mut kernel, 62_000, 8, true);
    assert!(!kernel.tick(Instant(130_000)).sleep());
}
