//! What the Android shell makes of the system's power state, without the
//! system (so that it is tested on the host): the battery as the
//! `ACTION_BATTERY_CHANGED` broadcast says it, with the system's own saver
//! (`PowerManager.isPowerSaveMode`, read in `status.rs`), and what the
//! shell changes by the power level (ADR 0025).

use paperpaw_platform::Battery;
use paperpaw_sdk::power::PowerControls;

/// `BatteryManager.BATTERY_STATUS_CHARGING`.
pub const CHARGING: i32 = 2;

/// What the shell changes by the power level: the system's own battery
/// saver is followed (the status carries it, [`Battery::saver`]), so the
/// power settings show Automatic's use of it. The rest is a phone's own.
pub fn power_controls() -> PowerControls {
    let mut controls = PowerControls::default();
    controls.system_saver = true;
    controls
}

/// The battery of what the broadcast said; `None` where it did not say.
pub fn battery_of(
    level: i32,
    scale: i32,
    status: i32,
    voltage: i32,
    saver: bool,
) -> Option<Battery> {
    if level < 0 || scale <= 0 {
        return None;
    }
    Some(
        Battery::new(
            (level * 100 / scale).clamp(0, 100) as u8,
            status == CHARGING,
            // In millivolts; some devices report volts.
            u16::try_from(voltage).ok().filter(|mv| *mv > 1000),
        )
        .with_saver(saver),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_phone_follows_its_systems_saver_and_nothing_else() {
        let controls = power_controls();
        assert!(controls.any() && controls.system_saver);
        assert!(
            !controls.radio && !controls.cpu_clock && !controls.buttons && !controls.frontlight
        );
    }

    #[test]
    fn the_systems_saver_is_the_batterys() {
        let on = battery_of(40, 100, CHARGING, 3900, true).unwrap();
        assert!(on.saver && on.charging && on.percent == 40);
        assert!(!battery_of(40, 100, 3, 3900, false).unwrap().saver);
    }

    #[test]
    fn what_the_broadcast_did_not_say_is_no_battery() {
        assert_eq!(battery_of(-1, 100, CHARGING, 3900, true), None);
        assert_eq!(battery_of(50, 0, CHARGING, 3900, true), None);
    }

    #[test]
    fn the_level_is_a_percent_of_the_scale_and_volts_are_not_millivolts() {
        let b = battery_of(150, 200, 3, 4, false).unwrap();
        assert_eq!((b.percent, b.millivolts), (75, None));
        assert_eq!(battery_of(3, 2, 3, 3700, false).unwrap().percent, 100);
        assert_eq!(
            battery_of(1, 2, 3, 3700, false).unwrap().millivolts,
            Some(3700)
        );
    }
}
