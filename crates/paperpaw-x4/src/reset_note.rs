//! The crash log's line for a restart the firmware did not say anything
//! about: a watchdog (the RTC's, a timer group's, the super watchdog) or a
//! brownout reset. A panic leaves its message in the RTC memory
//! (`rtc_guard`); a watchdog leaves only the chip's reason for the reset,
//! and the place the firmware last noted being at (`power::mark`). The
//! next start writes both to `crashes.txt`, so that a hang is no less
//! complete a report than a panic.
//!
//! ```text
//! WATCHDOG SysRtcWdt (RTC watchdog, digital core and RTC): last at refreshing the screen, naps 3
//! ```

use core::fmt;

/// What the chip's reset reason says, if it is a watchdog or a brownout:
/// `reason` is the reason's code (`esp_hal::rtc_cntl::SocResetReason as
/// u32`, the same on the ESP32-C3 and the ESP32-S3). Other resets (a
/// power-up, a sleep, a software reset: a panic has its own message) are
/// `None`. A power-up and a brownout share a code on these chips: only a
/// brownout the chip names (0x0F) is told.
pub fn watchdog(reason: u32) -> Option<(&'static str, &'static str)> {
    Some(match reason {
        0x07 => ("CoreMwdt0", "timer group 0 watchdog, digital core"),
        0x08 => ("CoreMwdt1", "timer group 1 watchdog, digital core"),
        0x09 => ("CoreRtcWdt", "RTC watchdog, digital core"),
        0x0B => ("Cpu0Mwdt0", "timer group 0 watchdog, CPU"),
        0x0D => ("Cpu0RtcWdt", "RTC watchdog, CPU"),
        0x0F => ("SysBrownOut", "brownout: the supply voltage dropped"),
        0x10 => ("SysRtcWdt", "RTC watchdog, digital core and RTC"),
        0x11 => ("Cpu0Mwdt1", "timer group 1 watchdog, CPU"),
        0x12 => ("SysSuperWdt", "super watchdog"),
        _ => return None,
    })
}

/// The line for the crash log: the reset, where the firmware was last and
/// how many light sleeps the run had.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    pub name: &'a str,
    pub meaning: &'a str,
    pub place: &'a str,
    pub naps: u32,
}

impl fmt::Display for Entry<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WATCHDOG {} ({}): last at {}, naps {}",
            self.name, self.meaning, self.place, self.naps
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchdog_and_brownout_resets_are_told_others_are_not() {
        assert_eq!(watchdog(0x10).map(|w| w.0), Some("SysRtcWdt"));
        assert_eq!(watchdog(0x09).map(|w| w.0), Some("CoreRtcWdt"));
        assert_eq!(watchdog(0x0B).map(|w| w.0), Some("Cpu0Mwdt0"));
        assert_eq!(watchdog(0x12).map(|w| w.0), Some("SysSuperWdt"));
        assert_eq!(watchdog(0x0F).map(|w| w.0), Some("SysBrownOut"));
        // Power-up, software reset (a panic), deep sleep, the escape's reset.
        for code in [0x01, 0x03, 0x05, 0x0C, 0x15] {
            assert_eq!(watchdog(code), None, "{code:#x}");
        }
    }

    #[test]
    fn the_line_names_the_reset_the_place_and_the_naps() {
        let (name, meaning) = watchdog(0x10).unwrap();
        let line = alloc::format!(
            "{}",
            Entry {
                name,
                meaning,
                place: "refreshing the screen",
                naps: 3
            }
        );
        assert_eq!(
            line,
            "WATCHDOG SysRtcWdt (RTC watchdog, digital core and RTC): last at refreshing the screen, naps 3"
        );
    }
}
