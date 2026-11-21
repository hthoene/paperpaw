//! The processor's clock on demand (#452), and the one place that switches
//! it: 160 MHz while the main loop works (events, layout, drawing, decoding,
//! the card, the radio, USB, an update), the crystal divided down while it
//! only waits (its rest between two turns, the panel's refresh), and back at
//! once when the wait is over. The power mode chooses how
//! ([`paperpaw_x4::controls::clock_policy`]): Normal waits at 20 MHz, Saving
//! and Offline at 10 MHz, Performance at 160 MHz.
//!
//! esp-hal has no clock API at run time, so on the ESP32-C3 the registers
//! are written as ESP-IDF's `rtc_clk_cpu_freq_set_config` does: the CPU's
//! source in `SYSTEM.sysclk_conf` (the PLL, or the crystal divided by
//! `pre_div_cnt + 1`), the ROM's ticks per microsecond, and the APB
//! frequency the ROM reads from the RTC's `STORE5`. The PLL stays on: the
//! USB serial port runs from it, and the way back takes no relock.
//!
//! On the crystal the APB bus runs at the CPU's clock, so whatever counts
//! APB cycles is slower while waiting:
//! - the SPI bus of the panel and the card, the I²C bus: used only while
//!   working, never in a wait;
//! - the radio's tasks need an 80 MHz APB: the caller never waits slow while
//!   a radio is on (`Firmware::wait_clock`);
//! - the timer group's watchdog: its time stretches by the divider while
//!   waiting; the RTC's (on the slow RC clock) is not touched.
//!
//! The system timer (time, `Delay`) and the timer that reads the buttons
//! count the crystal: they keep their time at every clock. Only the main
//! loop calls [`work`] and [`wait`]; the switch itself runs with interrupts
//! held off, so no interrupt sees the clock half switched. Nothing on the
//! ESP32-S3 devices: there the functions do nothing.

use core::sync::atomic::{AtomicU32, Ordering};

use paperpaw_x4::controls::ClockPolicy;

/// The crystal's divider now; 0: the PLL, the full clock. Written by the
/// main loop only.
static DIVIDER: AtomicU32 = AtomicU32::new(0);
/// The crystal's divider for waits; 0: waits keep the full clock.
static WAITING: AtomicU32 = AtomicU32::new(0);

/// Sets the policy of the power mode worn; one asking for the full clock
/// takes effect at the next wait.
pub fn set_policy(policy: ClockPolicy) {
    WAITING.store(policy.waiting_divider().unwrap_or(0), Ordering::Relaxed);
}

/// Work starts: the full clock (nothing to do where it is there).
#[inline]
pub fn work() {
    if DIVIDER.load(Ordering::Relaxed) != 0 {
        switch(0);
    }
}

/// A wait starts: the slow clock of the policy, until [`work`]. Meanwhile
/// nothing may be used that counts APB cycles (see the module).
#[inline]
pub fn wait() {
    let divider = WAITING.load(Ordering::Relaxed);
    if divider != DIVIDER.load(Ordering::Relaxed) {
        switch(divider);
    }
}

fn switch(divider: u32) {
    critical_section::with(|_| chip::set(divider));
    #[cfg(feature = "remote")]
    stats::switched(divider != 0);
    DIVIDER.store(divider, Ordering::Relaxed);
}

#[cfg(feature = "esp32c3")]
mod chip {
    use esp_hal::peripherals::{LPWR, SYSTEM};

    unsafe extern "C" {
        /// ROM: the CPU's ticks per microsecond, for `ets_delay_us`.
        fn ets_update_cpu_frequency(ticks_per_us: u32);
    }

    /// The CPU on the crystal divided by `divider`, or on the PLL (as
    /// esp-hal set it up at the start) for 0. Kept in RAM: the flash's
    /// cache is not read while the clock changes under it.
    #[esp_hal::ram]
    pub fn set(divider: u32) {
        let system = SYSTEM::regs();
        let Some(mhz) = esp_hal::clock::xtal_clock().as_mhz().checked_div(divider) else {
            // rtc_clk_cpu_freq_to_pll_mhz: the PLL's 160 MHz (`cpuperiod_sel`
            // stays as set at the start), the APB at 80 MHz.
            system.sysclk_conf().modify(|_, w| {
                // SAFETY: 0 is a valid divider; 1 selects the PLL.
                unsafe { w.pre_div_cnt().bits(0).soc_clk_sel().bits(1) }
            });
            store_apb_hz(80_000_000);
            // SAFETY: a ROM function that only stores the number.
            unsafe { ets_update_cpu_frequency(esp_hal::clock::cpu_clock().as_mhz()) };
            return;
        };
        // rtc_clk_cpu_freq_to_xtal: the ticks first, the divider set to 1
        // before the one wanted, then the source; the APB follows the CPU's
        // clock.
        // SAFETY: as above.
        unsafe { ets_update_cpu_frequency(mhz) };
        for bits in [0, (divider - 1) as u16] {
            // SAFETY: the divider is 2 or 4, within the field's 10 bits.
            system
                .sysclk_conf()
                .modify(|_, w| unsafe { w.pre_div_cnt().bits(bits) });
        }
        // SAFETY: 0 selects the crystal.
        system
            .sysclk_conf()
            .modify(|_, w| unsafe { w.soc_clk_sel().bits(0) });
        store_apb_hz(mhz * 1_000_000);
    }

    /// rtc_clk_apb_freq_update: the APB frequency where the ROM reads it.
    #[esp_hal::ram]
    fn store_apb_hz(hz: u32) {
        let shifted = (hz >> 12) & 0xFFFF;
        LPWR::regs()
            .store5()
            // SAFETY: the register only holds the number, in both halves.
            .modify(|_, w| unsafe { w.data().bits(shifted | (shifted << 16)) });
    }
}

/// The ESP32-S3 devices: the clock stays where esp-hal put it.
#[cfg(not(feature = "esp32c3"))]
mod chip {
    pub fn set(_divider: u32) {}
}

/// The time at the slow clock, for measuring it on a device (`remote`
/// builds, the `boot` command).
#[cfg(feature = "remote")]
mod stats {
    use core::cell::Cell;
    use critical_section::Mutex;

    /// When the clock went slow (ms since start); the slow milliseconds
    /// before; the switches to slow.
    static SLOW: Mutex<Cell<(Option<u64>, u64, u32)>> = Mutex::new(Cell::new((None, 0, 0)));

    pub fn switched(slow: bool) {
        let now = crate::now().0;
        critical_section::with(|cs| {
            let (since, total, switches) = SLOW.borrow(cs).get();
            let total = total + since.map_or(0, |at| now.saturating_sub(at));
            let next = if slow {
                (Some(now), total, switches.wrapping_add(1))
            } else {
                (None, total, switches)
            };
            SLOW.borrow(cs).set(next);
        });
    }

    /// Milliseconds at the slow clock since the start, and how often it went
    /// slow.
    pub fn slow() -> (u64, u32) {
        let now = crate::now().0;
        critical_section::with(|cs| {
            let (since, total, switches) = SLOW.borrow(cs).get();
            (
                total + since.map_or(0, |at| now.saturating_sub(at)),
                switches,
            )
        })
    }
}

#[cfg(feature = "remote")]
pub use stats::slow;
