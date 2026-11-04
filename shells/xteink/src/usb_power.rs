//! Outer power. The X4 has a pin that is high while a cable is in; the
//! ESP32-S3's X4 Pro and X4 Classic (USB through pogo pins) have a pin that
//! says the cell is charging; the X3 has only its fuel gauge. Two different
//! things are asked (#209):
//!
//! - **charging**: the device is on outer power and the cell takes it: the
//!   charging pin (the X3: its fuel gauge, or a computer's USB frames, as
//!   before). It is what the status bar's bolt shows, what the kernel's
//!   low-battery rules and its install check take for "charging", and what a
//!   sleeping reader's switch-off waits for. A computer that merely talks
//!   over the cable, with a full cell, is not charging.
//! - **cable**: a cable seems to be in (charging, or a computer sends USB
//!   frames, or the device has just started): it keeps an awake reader from
//!   napping, because a light sleep cuts the link to the computer, and it
//!   keeps a reader that was told to stay awake on USB awake.
//!
//! [`Sense`] says both for every device; [`UsbPower`] is the S3's and the
//! X3's.

use esp_hal::gpio::Input;

/// What the board knows of outer power.
pub trait Sense {
    /// On outer power and charging (see the module).
    fn charging(&self) -> bool;
    /// A cable seems to be in (see the module).
    fn cable(&self) -> bool;
    /// What the inputs say by themselves, for the diagnostics page; nothing
    /// where the board has but one input.
    fn readings(&self) -> Readings {
        Readings::default()
    }
}

/// What the inputs say, each by itself.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Readings {
    /// The level of the charging pin, high or not (none: the board has no
    /// such pin, or the X4's is the cable's itself).
    pub pin_high: Option<bool>,
    /// The USB frames move on like a bus's, at the last look.
    pub frames: Option<bool>,
}

/// The X4's pin, high while a cable is in.
impl Sense for Input<'static> {
    fn charging(&self) -> bool {
        self.is_high()
    }

    fn cable(&self) -> bool {
        self.is_high()
    }
}

#[cfg(any(feature = "esp32s3", feature = "x3"))]
pub use outer::UsbPower;
#[cfg(feature = "x3")]
pub use outer::gauge_says;

/// The boards that have to work it out from several inputs.
#[cfg(any(feature = "esp32s3", feature = "x3"))]
mod outer {
    use core::cell::Cell;
    #[cfg(feature = "x3")]
    use core::sync::atomic::{AtomicBool, Ordering};

    use esp_hal::delay::Delay;
    use esp_hal::gpio::{Input, Level};
    use paperpaw_x4::usb_link::Link;

    use super::{Readings, Sense};

    /// The X3's fuel gauge said the battery charges, at its last reading
    /// (the main loop reads it with the battery).
    #[cfg(feature = "x3")]
    static GAUGE_CHARGING: AtomicBool = AtomicBool::new(false);

    /// What the X3's gauge said of charging; `None` where it did not answer
    /// (kept as it was).
    #[cfg(feature = "x3")]
    pub fn gauge_says(charging: Option<bool>) {
        if let Some(charging) = charging {
            GAUGE_CHARGING.store(charging, Ordering::Relaxed);
        }
    }

    /// After starting, the device counts as cabled this long: a computer
    /// takes a moment to find it and send frames, and in a light sleep it
    /// could not (the USB unit sleeps too).
    const START_MS: u64 = 8000;
    /// The USB frames are read twice, this many milliseconds apart.
    const GAP_MS: u32 = 2;

    /// Outer power from the charging pin (the X3: its fuel gauge) and the
    /// USB frames.
    pub struct UsbPower {
        /// The charging pin and its level while charging; the X3 has none:
        /// its fuel gauge says it ([`gauge_says`]).
        pin: Option<(Input<'static>, Level)>,
        /// The looks at the USB frames, once a second at most.
        link: Cell<Link>,
    }

    impl UsbPower {
        /// With the charging pin, at `while_charging` while it charges.
        #[cfg(feature = "esp32s3")]
        pub fn new(pin: Input<'static>, while_charging: Level) -> Self {
            Self {
                pin: Some((pin, while_charging)),
                link: Cell::new(Link::new()),
            }
        }

        /// The X3's: charging as its fuel gauge says it.
        #[cfg(feature = "x3")]
        pub fn from_gauge() -> Self {
            Self {
                pin: None,
                link: Cell::new(Link::new()),
            }
        }

        /// The charging pin says charging; none where the board has no pin.
        fn pin_says(&self) -> Option<bool> {
            self.pin.as_ref().map(|(pin, level)| pin.level() == *level)
        }

        /// The looks at the USB frames, made when one is due (once a second at
        /// most; a look busy-waits [`GAP_MS`] twice).
        fn looked(&self) -> Link {
            let now = esp_hal::time::Instant::now()
                .duration_since_epoch()
                .as_millis();
            let mut link = self.link.get();
            if link.due(now) {
                link.look(now, bus());
                self.link.set(link);
            }
            link
        }
    }

    impl Sense for UsbPower {
        fn charging(&self) -> bool {
            match self.pin_says() {
                Some(charging) => charging,
                // The X3 has no pin: its gauge, and a full cell on a
                // cable has no current to show, so a computer too.
                #[cfg(feature = "x3")]
                None => GAUGE_CHARGING.load(Ordering::Relaxed) || self.looked().talking(),
                #[cfg(not(feature = "x3"))]
                None => false,
            }
        }

        fn cable(&self) -> bool {
            // One look that finds a bus is enough here: a nap that cut the
            // link in the first seconds of a computer's session would drop it.
            self.charging()
                || self.looked().moving()
                || esp_hal::time::Instant::now()
                    .duration_since_epoch()
                    .as_millis()
                    < START_MS
        }

        fn readings(&self) -> Readings {
            // Looked at now: once a second at most, and a look busy-waits
            // 2 x GAP_MS (4 ms), so the main loop pays that once a second.
            Readings {
                pin_high: self.pin.as_ref().map(|(pin, _)| pin.is_high()),
                frames: Some(self.looked().moving()),
            }
        }
    }

    /// Whether the USB unit's frame number moves on like a bus's: three
    /// readings [`GAP_MS`] apart, each step a frame or more and no more
    /// than the milliseconds that passed (a computer sends one a
    /// millisecond).
    fn bus() -> bool {
        let regs = esp_hal::peripherals::USB_DEVICE::regs();
        let frame = || regs.fram_num().read().sof_frame_index().bits();
        let delay = Delay::new();
        let first = frame();
        delay.delay_millis(GAP_MS);
        let second = frame();
        delay.delay_millis(GAP_MS);
        let third = frame();
        paperpaw_x4::usb_link::bus(&[first, second, third], GAP_MS)
    }
}
