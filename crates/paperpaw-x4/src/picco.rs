//! What the Boox Picco has beside its panel and touch screen: its charger,
//! its front light and its battery's charge by its voltage. The registers
//! and tables are the FreeInk SDK's (`BatteryMonitor`, `FrontlightManager`;
//! MIT licence, Copyright (c) 2026 FreeInk), read there from the Picco's
//! stock firmware (build 2912). Not tried on a device by PaperPaw.

use embedded_hal::i2c::I2c;

/// The charger (an SGM41562) on the I²C bus.
pub const CHARGER: u8 = 0x03;
/// Its status register: bit 1 a cable's power, bits 3 and 4 how it charges.
const CHARGER_STATUS: u8 = 0x08;

/// What the charger says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Charger {
    /// Outer power is there.
    pub cable: bool,
    /// The cell takes it (before or at its full current; not once done).
    pub charging: bool,
}

/// The charger's status; `None` if it does not answer. Only read: its
/// settings stay as it has them (the stock firmware raises its charge
/// voltage to 4.35 V; PaperPaw does not).
pub fn charger<I: I2c>(i2c: &mut I) -> Option<Charger> {
    let mut status = [0u8];
    i2c.write_read(CHARGER, &[CHARGER_STATUS], &mut status)
        .ok()?;
    Some(charger_of(status[0]))
}

fn charger_of(status: u8) -> Charger {
    let cable = status & 0x02 != 0;
    Charger {
        cable,
        charging: cable && matches!((status >> 3) & 0x03, 1 | 2),
    }
}

/// The cell's voltage (millivolts) at 100 %, 95 %, … 0 %, while charging
/// (the voltage runs high under the current) and while not: the stock
/// firmware's tables for its high-voltage cell.
const CHARGING_MV: [u16; 21] = [
    4305, 4305, 4300, 4290, 4235, 4187, 4124, 4078, 4045, 4013, 3984, 3961, 3941, 3923, 3906, 3894,
    3876, 3840, 3805, 3760, 3500,
];
const DISCHARGING_MV: [u16; 21] = [
    4305, 4229, 4171, 4118, 4065, 4019, 3955, 3915, 3876, 3841, 3812, 3786, 3764, 3745, 3730, 3717,
    3697, 3667, 3636, 3565, 3300,
];

/// The charge left, in percent, from the cell's voltage, between the
/// table's steps in a straight line.
pub fn percent(millivolts: u16, charging: bool) -> u8 {
    let table = if charging {
        &CHARGING_MV
    } else {
        &DISCHARGING_MV
    };
    if millivolts <= table[20] {
        return 0;
    }
    if millivolts >= table[0] {
        return 100;
    }
    for i in 1..=20usize {
        if millivolts < table[i] {
            continue;
        }
        let (high, low) = (u32::from(table[i - 1]), u32::from(table[i]));
        // The charging table has one voltage for 95 % and 100 %: that
        // step spans from the step below to 100 %.
        let span = if table[i - 1] == table[0] {
            5 * i as u32
        } else {
            5
        };
        let above = if high > low {
            span * (u32::from(millivolts) - low) / (high - low)
        } else {
            span
        };
        return (100 - 5 * i as u32 + above).min(100) as u8;
    }
    0
}

/// The front light's driver on the I²C bus: two channels, cool and warm
/// LEDs, each with a PWM value and a current limit.
pub const LIGHT: u8 = 0x38;

/// Ten steps of brightness for each of ten steps of warmth (cool first):
/// the cool and the warm PWM, then the cool and the warm current.
const LIGHT_STEPS: [[[u8; 4]; 10]; 10] = [
    [
        [1, 0, 24, 0],
        [14, 0, 31, 0],
        [34, 0, 31, 0],
        [55, 0, 31, 0],
        [75, 0, 31, 0],
        [95, 0, 31, 0],
        [116, 0, 31, 0],
        [136, 0, 31, 0],
        [157, 0, 31, 0],
        [177, 0, 31, 0],
    ],
    [
        [1, 1, 19, 0],
        [10, 3, 31, 1],
        [30, 1, 31, 8],
        [50, 1, 31, 19],
        [71, 8, 31, 31],
        [91, 28, 31, 31],
        [112, 48, 31, 31],
        [132, 69, 31, 31],
        [152, 89, 31, 31],
        [173, 110, 31, 31],
    ],
    [
        [1, 3, 17, 2],
        [5, 1, 31, 10],
        [25, 1, 31, 22],
        [46, 12, 31, 31],
        [66, 32, 31, 31],
        [86, 53, 31, 31],
        [107, 73, 31, 31],
        [127, 94, 31, 31],
        [148, 114, 31, 31],
        [168, 134, 31, 31],
    ],
    [
        [1, 2, 14, 7],
        [1, 1, 29, 18],
        [20, 6, 31, 31],
        [40, 27, 31, 31],
        [61, 47, 31, 31],
        [81, 67, 31, 31],
        [101, 88, 31, 31],
        [122, 108, 31, 31],
        [142, 129, 31, 31],
        [162, 149, 31, 31],
    ],
    [
        [2, 1, 10, 12],
        [1, 1, 23, 26],
        [13, 16, 31, 31],
        [34, 37, 31, 31],
        [54, 57, 31, 31],
        [74, 78, 31, 31],
        [95, 98, 31, 31],
        [115, 118, 31, 31],
        [136, 139, 31, 31],
        [156, 159, 31, 31],
    ],
    [
        [1, 2, 7, 16],
        [1, 4, 17, 31],
        [5, 24, 31, 31],
        [26, 45, 31, 31],
        [46, 65, 31, 31],
        [66, 86, 31, 31],
        [87, 106, 31, 31],
        [107, 126, 31, 31],
        [128, 147, 31, 31],
        [148, 167, 31, 31],
    ],
    [
        [3, 1, 3, 21],
        [2, 11, 11, 31],
        [1, 31, 25, 31],
        [15, 51, 31, 31],
        [36, 72, 31, 31],
        [56, 92, 31, 31],
        [76, 113, 31, 31],
        [97, 133, 31, 31],
        [117, 153, 31, 31],
        [138, 174, 31, 31],
    ],
    [
        [2, 1, 0, 26],
        [2, 16, 5, 31],
        [2, 37, 14, 31],
        [1, 57, 31, 31],
        [21, 77, 31, 31],
        [42, 98, 31, 31],
        [62, 118, 31, 31],
        [82, 138, 31, 31],
        [103, 159, 31, 31],
        [123, 179, 31, 31],
    ],
    [
        [1, 1, 0, 26],
        [1, 21, 0, 31],
        [1, 41, 4, 31],
        [1, 62, 12, 31],
        [1, 82, 26, 31],
        [17, 102, 31, 31],
        [37, 123, 31, 31],
        [58, 143, 31, 31],
        [78, 164, 31, 31],
        [98, 184, 31, 31],
    ],
    [
        [0, 5, 0, 31],
        [0, 25, 0, 31],
        [0, 46, 0, 31],
        [0, 66, 0, 31],
        [0, 86, 0, 31],
        [0, 107, 0, 31],
        [0, 127, 0, 31],
        [0, 147, 0, 31],
        [0, 167, 0, 31],
        [0, 184, 0, 31],
    ],
];

/// What the light's driver is given: each channel's PWM and current.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drive {
    pub cool_pwm: u8,
    pub warm_pwm: u8,
    pub cool_current: u8,
    pub warm_current: u8,
}

/// The drive for `level` percent of brightness (1 to 100), `warmth` percent
/// of it warm: between the table's steps in a straight line, the steps of
/// brightness spread evenly over the percents. A channel is off only at the
/// end of the warmth (cool at 100 %, warm at 0 %); between, each stays at
/// 1 at least, so that the colour does not jump.
pub fn drive(level: u8, warmth: u8) -> Drive {
    let level = u32::from(level.clamp(1, 100));
    let warmth = u32::from(warmth.min(100));
    // Positions in the table, in 1/1024 of a step.
    let b = (level - 1) * 9 * 1024 / 99;
    let w = warmth * 9 * 1024 / 100;
    let (bi, bf) = ((b / 1024).min(8) as usize, b - (b / 1024).min(8) * 1024);
    let (wi, wf) = ((w / 1024).min(8) as usize, w - (w / 1024).min(8) * 1024);
    let blend = |field: usize| -> u8 {
        let at = |w: usize, b: usize| u32::from(LIGHT_STEPS[w][b][field]);
        let low = (1024 - bf) * at(wi, bi) + bf * at(wi, bi + 1);
        let high = (1024 - bf) * at(wi + 1, bi) + bf * at(wi + 1, bi + 1);
        (((1024 - wf) * low + wf * high + 1024 * 512) / (1024 * 1024)) as u8
    };
    let (cool, warm) = (warmth < 100, warmth > 0);
    Drive {
        cool_pwm: if cool { blend(0).max(1) } else { 0 },
        warm_pwm: if warm { blend(1).max(1) } else { 0 },
        cool_current: if cool { blend(2) } else { 0 },
        warm_current: if warm { blend(3) } else { 0 },
    }
}

fn light_write<I: I2c>(i2c: &mut I, register: u8, value: u8) -> Option<()> {
    i2c.write(LIGHT, &[register, value]).ok()
}

/// Sets the bits of `mask` in `register` to `value`'s, the others kept.
fn light_update<I: I2c>(i2c: &mut I, register: u8, mask: u8, value: u8) -> Option<()> {
    let mut now = [0u8];
    i2c.write_read(LIGHT, &[register], &mut now).ok()?;
    light_write(i2c, register, (now[0] & !mask) | (value & mask))
}

/// Wakes the light's driver and sets it up (the stock firmware's
/// sequence); 2 ms pass before [`light_show`], the caller's to wait.
pub fn light_wake<I: I2c>(i2c: &mut I) -> Option<()> {
    light_update(i2c, 0x00, 0x80, 0x00)
}

/// After [`light_wake`] and its wait: the filter, mode and configuration.
pub fn light_set_up<I: I2c>(i2c: &mut I) -> Option<()> {
    light_write(i2c, 0x50, 0x03)?;
    light_update(i2c, 0x01, 0x07, 0x00)?;
    light_write(i2c, 0x02, 0x38)
}

/// Shows `drive` on the woken driver.
pub fn light_show<I: I2c>(i2c: &mut I, drive: Drive) -> Option<()> {
    light_write(i2c, 0x03, drive.cool_pwm)?;
    light_write(i2c, 0x04, drive.warm_pwm)?;
    light_update(i2c, 0x05, 0x1F, drive.cool_current)?;
    light_update(i2c, 0x06, 0x1F, drive.warm_current)?;
    light_update(i2c, 0x00, 0x14, if drive.cool_pwm > 0 { 0x04 } else { 0 })?;
    light_update(i2c, 0x00, 0x0B, if drive.warm_pwm > 0 { 0x02 } else { 0 })
}

/// Both channels off and the driver in standby; [`light_wake`] before it
/// shows again.
pub fn light_off<I: I2c>(i2c: &mut I) -> Option<()> {
    light_update(i2c, 0x00, 0x14, 0x00)?;
    light_update(i2c, 0x00, 0x0B, 0x00)?;
    light_update(i2c, 0x00, 0x80, 0x80)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_charger_s_status() {
        assert_eq!(
            charger_of(0x00),
            Charger {
                cable: false,
                charging: false
            }
        );
        // A cable, fast charging; then done.
        assert!(charger_of(0x02 | 2 << 3).charging);
        let done = charger_of(0x02 | 3 << 3);
        assert!(done.cable && !done.charging);
        // No cable: never charging, whatever the bits.
        assert!(!charger_of(1 << 3).charging);
    }

    #[test]
    fn charge_by_voltage() {
        assert_eq!(percent(4400, false), 100);
        assert_eq!(percent(3300, false), 0);
        assert_eq!(percent(0, true), 0);
        assert_eq!(percent(3812, false), 50);
        for charging in [false, true] {
            let mut last = 0;
            for mv in (3300..4310).step_by(5) {
                let p = percent(mv, charging);
                assert!(p >= last && p <= 100, "{mv} mV: {p} after {last}");
                last = p;
            }
        }
    }

    #[test]
    fn the_light_follows_its_table() {
        // The table's corners.
        assert_eq!(
            drive(1, 0),
            Drive {
                cool_pwm: 1,
                warm_pwm: 0,
                cool_current: 24,
                warm_current: 0
            }
        );
        assert_eq!(
            drive(100, 100),
            Drive {
                cool_pwm: 0,
                warm_pwm: 184,
                cool_current: 0,
                warm_current: 31
            }
        );
        assert_eq!(drive(100, 0).cool_pwm, 177);
        // Between the ends of the warmth both channels stay lit.
        for warmth in 1..100 {
            for level in 1..=100 {
                let d = drive(level, warmth);
                assert!(d.cool_pwm >= 1 && d.warm_pwm >= 1, "{level} {warmth}");
                assert!(d.cool_current <= 31 && d.warm_current <= 31);
            }
        }
    }
}
