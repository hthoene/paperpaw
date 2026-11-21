//! The processor's clock on demand (#452): 160 MHz while the X4 works
//! (events, layout, drawing, decoding, the radio, USB), a slow clock from the
//! crystal while it only waits (the buttons' pause between two readings, the
//! panel's refresh once the picture is sent), and back at the first sign of
//! work.
//!
//! esp-hal has no runtime clock API, so the ESP32-C3's two registers are
//! written here as `rtc_clk_cpu_freq_set` does (ESP-IDF): the CPU's source
//! (`SYSTEM.sysclk_conf`: PLL, or the crystal divided by `pre_div_cnt`), the
//! ROM's ticks per microsecond and the APB frequency the ROM reads from the
//! RTC's `STORE5`. The PLL stays on (the USB serial port and the radio live
//! on it); only what the CPU and the APB bus run from changes, so only what
//! counts the APB's cycles is slower while the clock is low: nothing that is
//! used then. The system timer, which time and `Delay` count with, and the
//! timer groups run from the crystal. The SPI buses, the card and the radio
//! are only used at the full clock ([`busy`] is the state everything but a
//! wait starts in).
//!
//! The level (Normal: a slow clock while waiting; Saving, Offline: a slower
//! one; Performance: always 160 MHz) is the power mode's, set by the main
//! loop ([`set_policy`]); plugins do not know. Not on the ESP32-S3 devices:
//! there everything here does nothing.

use core::sync::atomic::{AtomicU8, Ordering};

/// How the clock follows the work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Always 160 MHz (the Performance mode, and chips this does not drive).
    Full,
    /// Normal: the crystal's 40 MHz divided by two while waiting.
    OnDemand,
    /// Saving and Offline: the crystal's 40 MHz divided by four while waiting.
    Saving,
}

const FULL: u8 = 0;
const LOW: u8 = 1;
const LOW_SAVING: u8 = 2;

/// The state of the clock and the policy, one byte each: the main loop
/// writes, the interrupt handlers do not look.
static STATE: AtomicU8 = AtomicU8::new(FULL);
static POLICY: AtomicU8 = AtomicU8::new(0);
static REPORTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Sets the policy; where it asks for the full clock the clock goes there at
/// once.
pub fn set_policy(policy: Policy) {
    let byte = match policy {
        Policy::Full => 0,
        Policy::OnDemand => 1,
        Policy::Saving => 2,
    };
    POLICY.store(byte, Ordering::Relaxed);
    if policy == Policy::Full {
        busy();
    }
}

/// The policy now.
pub fn policy() -> Policy {
    match POLICY.load(Ordering::Relaxed) {
        1 => Policy::OnDemand,
        2 => Policy::Saving,
        _ => Policy::Full,
    }
}

/// Work starts: the full clock. Cheap where it is there already.
#[inline]
pub fn busy() {
    if STATE.load(Ordering::Relaxed) != FULL {
        STATE.store(FULL, Ordering::Relaxed);
        chip::full();
        #[cfg(feature = "remote")]
        stats::left_low();
    }
}

/// A wait starts (`wait_ms` long at least a little): the slow clock the
/// policy gives. The caller is back to [`busy`] when it is over; no
/// peripheral but the system timer, a GPIO's input and the interrupt that
/// reads the buttons may be used before then.
#[inline]
pub fn idle(wait_ms: u64) {
    if wait_ms < MIN_WAIT_MS {
        return;
    }
    let (state, div) = match policy() {
        Policy::Full => return,
        Policy::OnDemand => (LOW, 2),
        Policy::Saving => (LOW_SAVING, 4),
    };
    if STATE.load(Ordering::Relaxed) != state {
        if !REPORTED.load(Ordering::Relaxed) {
            REPORTED.store(true, Ordering::Relaxed);
            chip::report();
        }
        STATE.store(state, Ordering::Relaxed);
        chip::low(div);
        #[cfg(feature = "remote")]
        stats::entered_low();
    }
}

/// Waits shorter than this stay at the clock they have.
const MIN_WAIT_MS: u64 = 2;

#[cfg(feature = "esp32c3")]
mod chip {
    use esp_hal::peripherals::{LPWR, SYSTEM};

    unsafe extern "C" {
        /// ROM: ticks per microsecond for `ets_delay_us` (ESP-IDF's rtc_clk does the same).
        fn ets_update_cpu_frequency(ticks_per_us: u32);
    }

    /// Logs once what the timer groups count with: from the crystal they are
    /// the same at every CPU clock (the APB's would not be).
    pub fn report() {
        use esp_hal::peripherals::{TIMG0, TIMG1};
        log::info!(
            "cpu clock on demand: timer groups on the crystal: t0 {} {}, wdt {} {}",
            TIMG0::regs().t(0).config().read().use_xtal().bit(),
            TIMG1::regs().t(0).config().read().use_xtal().bit(),
            TIMG0::regs().wdtconfig0().read().wdt_use_xtal().bit(),
            TIMG1::regs().wdtconfig0().read().wdt_use_xtal().bit(),
        );
    }

    /// The CPU on the crystal divided by `div`.
    #[esp_hal::ram]
    pub fn low(div: u32) {
        let apb = SYSTEM::regs();
        let xtal_mhz = esp_hal::clock::xtal_clock().as_mhz();
        let mhz = xtal_mhz / div;
        // SAFETY: the C3's clock registers, as esp-hal's own clock tree writes
        // them; the PLL stays on.
        unsafe {
            ets_update_cpu_frequency(mhz);
            apb.sysclk_conf().modify(|_, w| {
                w.pre_div_cnt().bits((div - 1) as u16);
                w.soc_clk_sel().bits(0)
            });
            store_apb(mhz * 1_000_000);
        }
    }

    /// The CPU on the PLL, 160 MHz, as esp-hal set it up.
    #[esp_hal::ram]
    pub fn full() {
        let apb = SYSTEM::regs();
        let mhz = esp_hal::clock::cpu_clock().as_mhz();
        // SAFETY: as in `low`.
        unsafe {
            apb.sysclk_conf().modify(|_, w| {
                w.pre_div_cnt().bits(0);
                w.soc_clk_sel().bits(1)
            });
            ets_update_cpu_frequency(mhz);
            store_apb(80_000_000);
        }
    }

    /// The APB frequency the ROM's code reads (`rtc_clk_apb_freq_update`).
    #[esp_hal::ram]
    fn store_apb(hz: u32) {
        let shifted = (hz >> 12) & 0xFFFF;
        LPWR::regs()
            .store5()
            .modify(|_, w| unsafe { w.data().bits(shifted | (shifted << 16)) });
    }
}

/// Not the ESP32-C3: the clock stays where esp-hal put it.
#[cfg(not(feature = "esp32c3"))]
mod chip {
    pub fn report() {}
    pub fn low(_div: u32) {}
    pub fn full() {}
}

/// How long the clock was low, for the test image's measurements (#452).
#[cfg(feature = "remote")]
mod stats {
    use core::sync::atomic::{AtomicU32, Ordering};

    static SINCE: AtomicU32 = AtomicU32::new(0);
    static LOW_MS: AtomicU32 = AtomicU32::new(0);
    static REPORTED_AT: AtomicU32 = AtomicU32::new(0);
    static REPORTED_LOW: AtomicU32 = AtomicU32::new(0);

    pub fn entered_low() {
        SINCE.store(crate::now().0 as u32, Ordering::Relaxed);
    }

    pub fn left_low() {
        let ms = (crate::now().0 as u32).wrapping_sub(SINCE.load(Ordering::Relaxed));
        LOW_MS.store(
            LOW_MS.load(Ordering::Relaxed).wrapping_add(ms),
            Ordering::Relaxed,
        );
    }

    /// `PP:CLOCK low_ms span_ms`: the time at the slow clock in the last
    /// `span_ms`, printed at most every 10 s.
    pub fn report() {
        let now = crate::now().0 as u32;
        let span = now.wrapping_sub(REPORTED_AT.load(Ordering::Relaxed));
        if span < 10_000 {
            return;
        }
        let low = LOW_MS.load(Ordering::Relaxed);
        let in_span = low.wrapping_sub(REPORTED_LOW.load(Ordering::Relaxed));
        esp_println::println!("PP:CLOCK {in_span} {span}");
        REPORTED_AT.store(now, Ordering::Relaxed);
        REPORTED_LOW.store(low, Ordering::Relaxed);
    }
}

/// Prints the share of time at the slow clock now and then (test builds).
#[cfg(feature = "remote")]
pub fn report() {
    stats::report();
}
