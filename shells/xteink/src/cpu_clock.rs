//! The processor's clock across a light sleep (#434): esp-hal 1.2.2 keeps the
//! ESP32-C3's CPU on the 160 MHz PLL across a light sleep. The sleep powers
//! the PLL down, and the wake runs on it without configuring it again: the
//! first code after the wake (the ROM's `ets_delay_us`) then traps with
//! instructions read wrong. Nothing on the ESP32-S3 devices: there the
//! function only sleeps.

/// Runs `sleep`, a light sleep, with the CPU on the crystal and the PLL
/// powered down, and configures the PLL again after the wake, as ESP-IDF's
/// `esp_sleep_start` does (`rtc_clk_cpu_freq_set_xtal` before,
/// `rtc_clk_cpu_freq_set_config` after) and esp-hal does from its PR #6558
/// on.
pub fn across_sleep<R>(sleep: impl FnOnce() -> R) -> R {
    chip::across_sleep(sleep)
}

#[cfg(feature = "esp32c3")]
mod chip {
    /// See [`super::across_sleep`]. The switches go through esp-hal's clock
    /// tree, the owner of the PLL.
    pub fn across_sleep<R>(sleep: impl FnOnce() -> R) -> R {
        use esp_hal::clock::ll::{ClockTree, CpuClkConfig, configure_cpu_clk};

        let before = ClockTree::with(|clocks| {
            let before = clocks.cpu_clk();
            configure_cpu_clk(clocks, CpuClkConfig::Xtal);
            before
        });
        let woken = sleep();
        if let Some(before) = before {
            // Back on the PLL, the PLL configured again; with the interrupts
            // off, so none sees the clock half switched.
            critical_section::with(|_| ClockTree::with(|clocks| configure_cpu_clk(clocks, before)));
        }
        woken
    }
}

#[cfg(not(feature = "esp32c3"))]
mod chip {
    pub fn across_sleep<R>(sleep: impl FnOnce() -> R) -> R {
        sleep()
    }
}
