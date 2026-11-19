//! The Boox Picco's board (experimental): which of the ESP32-S3's pins and
//! units does what, the one place that says so (as `board_pro.rs` for the
//! X4 Pro, which the Picco is built as: the feature `picco` is `x4-pro`
//! with this board). From the FreeInk SDK's board table (`BoardConfig.h`,
//! MIT), which was read from the Picco's stock firmware; **not tried on a
//! device by PaperPaw**. Where the SDK is sure, it does as the SDK says.
//!
//! | Part | Connection |
//! |---|---|
//! | 3.97" panel, 800 × 480, an SSD1677 or the "SSD2677" (by unit, found at start) | SPI at 20 MHz: clock GPIO 48, data out GPIO 47, select GPIO 18; data/command GPIO 17, reset GPIO 16, busy GPIO 15 |
//! | microSD card | SDMMC, driven on one data line: clock GPIO 41, command GPIO 42, data GPIO 40 (the other three, GPIO 39, 44 and 43, pulled up) |
//! | Touch (TMA525C), front light (0x38), charger (SGM41562), clock (PCF8563/BM8563) | I²C at 400 kHz: data GPIO 7, clock GPIO 8; the touch's interrupt GPIO 11 (low while a report waits), its reset GPIO 10 |
//! | Side buttons (up, down) | GPIO 21 and GPIO 9, low while pressed |
//! | Power button | GPIO 2, low while pressed |
//! | Battery | half its voltage on ADC GPIO 1 |
//! | The card's, panel's, clock's and touch's power | GPIO 0, on while low |
//! | The battery's power | held on by GPIO 4, high from the start and through the deep sleep |
//!
//! Not used: the slide switch (GPIO 6), the motion sensor (QMI8658, I²C
//! 0x6A) and GPIO 5, whose part is not known. The charger is only read
//! (`paperpaw_x4::picco`). It has 16 MB of flash and 8 MB of PSRAM (`main`
//! makes it the heap).

use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::Blocking;
use esp_hal::analog::adc::{Adc, AdcCalCurve, AdcConfig, AdcPin, Attenuation};
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::peripherals::{
    ADC1, GPIO0, GPIO1, GPIO10, GPIO16, GPIO40, GPIO41, GPIO42, LPWR, Peripherals, RTC_TIMER,
    SDHOST,
};
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use paperpaw_x4::buttons::State;
use paperpaw_x4::panel::Device;
use paperpaw_x4::probe::{Controller, Probe};
use paperpaw_x4::{bm8563, picco, tma525c};

use crate::input::Touch;
pub use crate::usb_power::UsbPower;

/// The panel's SPI speed: the stock firmware's.
const PANEL_MHZ: u32 = 20;
/// The I²C bus's speed: the stock firmware's. The touch screen is read in
/// the input interrupt, where a report must take well under a reading's
/// 5 ms.
const I2C_KHZ: u32 = 400;
/// A finger counts as lifted when the controller said nothing of it for
/// this long (it goes quiet under a resting finger; a lift it did not
/// report must not hold the screen for good).
const TOUCH_STALE_MS: u64 = 3000;

/// The panel, on its own SPI bus.
pub type Panel = paperpaw_x4::panel::Panel<
    ExclusiveDevice<Spi<'static, Blocking>, Output<'static>, Delay>,
    Output<'static>,
    Output<'static>,
    Input<'static>,
    Delay,
>;

/// The SD card's parts: its bus (its power is on from the start).
pub struct CardParts {
    pub host: SDHOST<'static>,
    pub clock: GPIO41<'static>,
    pub command: GPIO42<'static>,
    pub data: GPIO40<'static>,
    pub delay: Delay,
}

/// Before the deep sleep, as the stock firmware sleeps (the FreeInk SDK's
/// `PowerManager`): GPIO 4 high, the parts' power off (GPIO 0 high) and
/// the panel's and the touch's resets low beside their cut power, each
/// held through the sleep ([`Board::take`] lets them go). The power button
/// wakes the chip. Nothing drives GPIO 4 low: whether a press would start
/// a Picco whose latch was let go is not known.
pub fn hold_for_sleep(latch: &mut Output<'static>) {
    latch.set_high();
    latch.set_pad_hold(true);
    // SAFETY: the panel, the card and the touch screen are not driven
    // again before the deep sleep; their lines are taken back here, once,
    // to be held.
    let (rail, panel, touch) = unsafe { (GPIO0::steal(), GPIO16::steal(), GPIO10::steal()) };
    Output::new(panel, Level::Low, OutputConfig::default()).set_pad_hold(true);
    Output::new(touch, Level::Low, OutputConfig::default()).set_pad_hold(true);
    Output::new(rail, Level::High, OutputConfig::default()).set_pad_hold(true);
}

/// What the input interrupt reads: the side buttons, and on the I²C bus
/// the touch screen; the main loop reads the battery and the charger, sets
/// the clock and drives the front light on the bus too (see `input`).
pub struct Keys {
    pub up: Input<'static>,
    pub down: Input<'static>,
    pub i2c: I2c<'static, Blocking>,
    /// Low while the touch controller has a report.
    touch_ready: Input<'static>,
    /// The touch controller runs its application: it reports touches.
    touch_on: bool,
    /// The finger followed: its id and where it was (on the screen).
    finger: Option<(u8, u16, u16)>,
    /// When the touch controller last reported (ms since the start).
    reported: u64,
    adc: Adc<'static, ADC1<'static>, Blocking>,
    /// Half the battery's voltage.
    cell: AdcPin<GPIO1<'static>, ADC1<'static>, AdcCalCurve<ADC1<'static>>>,
    /// The front light's driver is awake and set up.
    light_awake: bool,
}

fn millis() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_millis()
}

impl Keys {
    /// The battery's charge (percent) and voltage (millivolts): the middle
    /// of three readings, by the stock firmware's table for a cell that
    /// charges or does not, as the charger says.
    pub fn battery(&mut self) -> Option<(u8, u16)> {
        let charger = picco::charger(&mut self.i2c);
        // Whether it charges, for USB power (`usb_power`), read with it.
        crate::usb_power::gauge_says(charger.map(|c| c.charging));
        let mut readings = [0u16; 3];
        for reading in &mut readings {
            *reading = nb::block!(self.adc.read_oneshot(&mut self.cell)).ok()?;
        }
        readings.sort_unstable();
        let millivolts = paperpaw_x4::battery::millivolts(readings[1]);
        // Outside what a cell can have: no reading.
        (2500..=4500).contains(&millivolts).then(|| {
            let charging = charger.is_some_and(|c| c.charging);
            (picco::percent(millivolts, charging), millivolts)
        })
    }

    /// The side buttons and the power button down (`power_down`); no
    /// readings to look into.
    pub fn read(&mut self, power_down: bool) -> (State, u32) {
        (
            State::pins(self.up.is_low(), self.down.is_low(), power_down),
            0,
        )
    }

    /// The Picco has no home key.
    pub fn home_down(&self) -> bool {
        false
    }

    /// Sets the clock chip, which keeps the time while the Picco sleeps.
    pub fn set_clock(&mut self, unix_ms: i64) {
        let _ = bm8563::write(&mut self.i2c, &bm8563::Time::from_unix_ms(unix_ms));
    }

    /// The front light: `level` percent of brightness, `warmth` percent of
    /// it warm; off without, or at 0.
    pub fn light(&mut self, show: Option<(u8, u8)>) {
        match show {
            Some((level, warmth)) if level > 0 => {
                if !self.light_awake {
                    if picco::light_wake(&mut self.i2c).is_none() {
                        return;
                    }
                    Delay::new().delay_millis(2);
                    if picco::light_set_up(&mut self.i2c).is_none() {
                        return;
                    }
                    self.light_awake = true;
                }
                let _ = picco::light_show(&mut self.i2c, picco::drive(level, warmth));
            }
            _ => {
                let _ = picco::light_off(&mut self.i2c);
                self.light_awake = false;
            }
        }
    }

    /// Puts the touch controller to sleep, before the deep sleep.
    pub fn sleep_touch(&mut self) {
        self.touch_on = false;
        let _ = tma525c::sleep(&mut self.i2c);
    }

    /// Where a finger went down, moved or was lifted since the last report,
    /// for `push`. One finger is followed: the first.
    pub fn touch(&mut self, push: &mut dyn FnMut(Touch)) {
        if !self.touch_on {
            return;
        }
        if self.touch_ready.is_high() {
            // Nothing new. A finger not heard of for long is taken as lifted.
            if let Some((_, x, y)) = self.finger
                && millis().saturating_sub(self.reported) >= TOUCH_STALE_MS
            {
                self.finger = None;
                push(Touch::Up(x, y));
            }
            return;
        }
        let mut report = [0u8; tma525c::REPORT];
        let Ok(length) = tma525c::report(&mut self.i2c, &mut report) else {
            return;
        };
        let mut points = [tma525c::Point { id: 0, x: 0, y: 0 }; tma525c::MAX_POINTS];
        let Some(count) = tma525c::touches(&report[..length], &mut points) else {
            return;
        };
        self.reported = millis();
        let first = (count > 0).then(|| points[0]);
        match (self.finger, first) {
            (Some((_, x, y)), None) => {
                self.finger = None;
                push(Touch::Up(x, y));
            }
            (finger, Some(p)) => {
                let (x, y) = tma525c::upright(p.x, p.y);
                match finger {
                    None => push(Touch::Down(x, y, (p.x, p.y))),
                    Some((id, fx, fy)) if id == p.id && (fx, fy) != (x, y) => {
                        push(Touch::Move(x, y))
                    }
                    Some((id, fx, fy)) if id != p.id => {
                        // Another finger: the first was lifted.
                        push(Touch::Up(fx, fy));
                        push(Touch::Down(x, y, (p.x, p.y)));
                    }
                    Some(_) => {}
                }
                self.finger = Some((p.id, x, y));
            }
            (None, None) => {}
        }
    }
}

/// Waits up to `ms` for the touch controller's interrupt line to go low.
fn touch_answers(ready: &Input<'static>, delay: Delay, ms: u32) -> bool {
    for _ in 0..ms {
        if ready.is_low() {
            return true;
        }
        delay.delay_millis(1);
    }
    ready.is_low()
}

/// Resets the touch controller and starts its application, as the stock
/// firmware does; whether it reports touches now.
fn start_touch(
    i2c: &mut I2c<'static, Blocking>,
    reset: GPIO10<'static>,
    ready: &Input<'static>,
    delay: Delay,
) -> bool {
    let mut reset = Output::new(reset, Level::High, OutputConfig::default());
    // Held low through the deep sleep (see `hold_for_sleep`): let go.
    reset.set_pad_hold(false);
    delay.delay_millis(1);
    reset.set_low();
    delay.delay_millis(10);
    reset.set_high();
    delay.delay_millis(50);
    // The line keeps its level once its driver is gone.
    let _ = reset;
    let mut report = [0u8; tma525c::REPORT];
    // What it says after its reset, read away.
    for _ in 0..4 {
        if ready.is_high() {
            break;
        }
        delay.delay_millis(100);
        let _ = tma525c::report(i2c, &mut report);
    }
    let mut mode = tma525c::Mode::Unknown;
    for _ in 0..3 {
        if tma525c::ask_mode(i2c).is_err() {
            delay.delay_millis(120);
            continue;
        }
        if touch_answers(ready, delay, 500)
            && let Ok(length) = tma525c::report(i2c, &mut report)
        {
            mode = tma525c::mode(&report[..length]);
        }
        if mode != tma525c::Mode::Unknown {
            break;
        }
    }
    if mode == tma525c::Mode::Boot {
        if tma525c::launch(i2c).is_ok() && touch_answers(ready, delay, 500) {
            let _ = tma525c::report(i2c, &mut report);
        }
        // As the stock firmware: on once the launch was sent.
        mode = tma525c::Mode::App;
    }
    mode == tma525c::Mode::App
}

/// The Picco's parts, each once.
pub struct Board {
    /// Holds the battery's power on.
    pub power_latch: Output<'static>,
    /// The panel, still to be started.
    pub panel: Panel,
    /// What its controller answered at start.
    pub display: Probe,
    pub card: CardParts,
    pub keys: Keys,
    pub power_button: Input<'static>,
    pub usb_power: UsbPower,
    /// The front light, off.
    pub light: Option<crate::light::Light>,
    /// The time the clock chip kept while the Picco slept (ms since 1970,
    /// UTC), if it kept one.
    pub clock_ms: Option<i64>,
    pub timers: TimerGroup<'static, esp_hal::peripherals::TIMG0<'static>>,
    pub timers1: TimerGroup<'static, esp_hal::peripherals::TIMG1<'static>>,
    pub rtc: RTC_TIMER<'static>,
    pub low_power: LPWR<'static>,
    /// The USB serial unit: the log, the cable transfer, the remote.
    pub usb: esp_hal::peripherals::USB_DEVICE<'static>,
    pub radio: (
        esp_hal::peripherals::WIFI<'static>,
        esp_hal::peripherals::FROM_CPU_INTR0<'static>,
    ),
    pub delay: Delay,
}

impl Board {
    /// Keeps the battery's power on before memory and OTA are initialized.
    /// The level is set while a hold from the deep sleep still keeps the
    /// pad, and the hold stays: the latch never dips (the FreeInk SDK found
    /// a dip cuts the power on waking).
    pub fn hold_power() -> Output<'static> {
        // SAFETY: called once after HAL initialization. The GPIO4 in the
        // Peripherals passed to take is never used; this is its only driver.
        let pin = unsafe { esp_hal::peripherals::GPIO4::steal() };
        let mut power_latch = Output::new(pin, Level::High, OutputConfig::default());
        power_latch.set_pad_hold(true);
        power_latch
    }

    /// Takes the chip's parts apart, keeping the output made by hold_power.
    #[expect(
        clippy::expect_used,
        reason = "without its buses the Picco can do nothing; a panic shows why"
    )]
    // A frame of its own, off the stack before the kernel is built.
    #[inline(never)]
    pub fn take(mut p: Peripherals, power_latch: Output<'static>) -> Self {
        let delay = Delay::new();

        // The parts' power, on (low) for good: the panel, the card, the
        // clock and the touch screen need it. Held off through the deep
        // sleep (see `hold_for_sleep`): let go, as the panel's reset.
        let mut rail = Output::new(p.GPIO0, Level::Low, OutputConfig::default());
        rail.set_pad_hold(false);
        // The line keeps its level once its driver is gone.
        let _ = rail;
        Output::new(p.GPIO16.reborrow(), Level::High, OutputConfig::default()).set_pad_hold(false);
        delay.delay_millis(50);

        // Which controller the panel has, asked on its lines before the
        // bus takes them: the "SSD2677" by its revision, as the stock
        // firmware tells it; every other panel of the Picco has an SSD1677.
        let display = crate::probe::ask(
            p.GPIO48.reborrow(),
            p.GPIO47.reborrow(),
            p.GPIO18.reborrow(),
            p.GPIO17.reborrow(),
            p.GPIO16.reborrow(),
            p.GPIO15.reborrow(),
            delay,
            paperpaw_x4::probe::RESET_MS,
        );
        let controller = if display.picco_ssd2677() {
            Controller::Ssd2677
        } else {
            Controller::Ssd1677
        };
        let spi = Spi::new(
            p.SPI2,
            SpiConfig::default()
                .with_frequency(Rate::from_mhz(PANEL_MHZ))
                .with_mode(Mode::_0),
        )
        .expect("SPI")
        .with_sck(p.GPIO48)
        .with_mosi(p.GPIO47);
        let select = Output::new(p.GPIO18, Level::High, OutputConfig::default());
        let panel = Panel::new(
            Device::Picco,
            controller,
            ExclusiveDevice::new(spi, select, delay).expect("display SPI"),
            Output::new(p.GPIO17, Level::High, OutputConfig::default()),
            Output::new(p.GPIO16, Level::High, OutputConfig::default()),
            Input::new(p.GPIO15, InputConfig::default()),
            delay,
        );

        let pulled = InputConfig::default().with_pull(Pull::Up);
        let mut i2c = I2c::new(
            p.I2C0,
            I2cConfig::default().with_frequency(Rate::from_khz(I2C_KHZ)),
        )
        .expect("I2C")
        .with_sda(p.GPIO7)
        .with_scl(p.GPIO8);
        let clock_ms = bm8563::read(&mut i2c).map(|t| t.unix_ms());
        let touch_ready = Input::new(p.GPIO11, pulled);
        let touch_on = start_touch(&mut i2c, p.GPIO10, &touch_ready, delay);
        if !touch_on {
            log::warn!("touch: the controller does not answer");
        }
        // A light left on by a run that ended without its sleep.
        let _ = picco::light_off(&mut i2c);

        // The card's three other data lines, pulled up: it is driven on
        // one. Pins keep their setting once their driver is gone.
        let _ = Input::new(p.GPIO39, pulled);
        let _ = Input::new(p.GPIO44, pulled);
        let _ = Input::new(p.GPIO43, pulled);

        let mut adc = AdcConfig::new();
        let cell = adc.enable_pin_with_cal::<_, AdcCalCurve<ADC1<'_>>>(p.GPIO1, Attenuation::_11dB);
        Self {
            power_latch,
            panel,
            display,
            card: CardParts {
                host: p.SDHOST,
                clock: p.GPIO41,
                command: p.GPIO42,
                data: p.GPIO40,
                delay,
            },
            keys: Keys {
                up: Input::new(p.GPIO21, pulled),
                down: Input::new(p.GPIO9, pulled),
                i2c,
                touch_ready,
                touch_on,
                finger: None,
                reported: 0,
                adc: Adc::new(p.ADC1, adc),
                cell,
                light_awake: false,
            },
            power_button: Input::new(p.GPIO2, pulled),
            light: Some(crate::light::Light::new()),
            clock_ms,
            // No pin says it: the charger does, read with the battery.
            usb_power: UsbPower::from_gauge(),
            timers: TimerGroup::new(p.TIMG0),
            timers1: TimerGroup::new(p.TIMG1),
            rtc: p.RTC_TIMER,
            low_power: p.LPWR,
            usb: p.USB_DEVICE,
            radio: (p.WIFI, p.FROM_CPU_INTR0),
            delay,
        }
    }
}
