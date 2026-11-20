//! The X3's board (experimental: not yet tried on one): which of the
//! ESP32-C3's pins and units does what, the one place that says so. The
//! X4's chip, bus, card and buttons; another panel, and a fuel gauge and
//! a clock on I²C where the X4 reads its battery and USB from pins. Its
//! facts come from the FreeInk SDK's `BoardConfig.h` (MIT), read, not run.
//!
//! | Part | Connection |
//! |---|---|
//! | 3.68" panel, 792 × 528, UC8253 controller | SPI: clock GPIO 8, data out GPIO 10, select GPIO 21; data/command GPIO 4, reset GPIO 5, busy GPIO 6 |
//! | microSD card | the same SPI (data in GPIO 7), select GPIO 12 |
//! | Back, confirm, left, right | resistor ladder on ADC GPIO 1 |
//! | The side buttons (up, down) | resistor ladder on ADC GPIO 2 |
//! | Power button | GPIO 3, low while pressed |
//! | Fuel gauge BQ27220 (0x55), clock DS3231 (0x68) | I²C: data GPIO 20, clock GPIO 0 |
//! | The card's power | GPIO 13, on while high |
//!
//! GPIO 13 is the card's power rail, not a battery latch as on the X4
//! (Papyrix's `BoardProfiles.cpp` and `PowerPolicy.cpp`, MIT): on from the
//! start and while the card is used, off only before the deep sleep that
//! is the X3's off (the firmware's sleep sets it low, nothing else does).
//! Uncertain without a device: whether the ladders' levels and the
//! up/down order are the X4's (the SDK has the same ranges). The tilt
//! sensor (QMI8658, 0x6B) is left out, and NFC, which the maker mentions,
//! is not used. Newer X3s have a UC8279d (told at start by the probe: the
//! revision's third byte 0x66) with its own set-up and waveforms (see
//! `paperpaw_x4::uc8253`); the bus stays at 10 MHz.

use alloc::boxed::Box;
use core::cell::RefCell;

use embedded_hal_bus::spi::RefCellDevice;
use esp_hal::Blocking;
use esp_hal::analog::adc::{Adc, AdcCalBasic, AdcConfig, AdcPin, Attenuation};
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::peripherals::{ADC1, GPIO1, GPIO2, LPWR, Peripherals, RTC_TIMER};
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, Spi, SpiDma};
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use paperpaw_x4::buttons::State;
use paperpaw_x4::panel::Device;
use paperpaw_x4::probe::{Controller, Probe};
use paperpaw_x4::{bq27220, ds3231};

use crate::input::Touch;
pub use crate::usb_power::UsbPower;

/// SPI speeds: an SD card starts slowly; the X3's panel takes 10 MHz (the
/// SDK's; whether it takes the X4's 20 is not tried).
const CARD_START_KHZ: u32 = 400;
pub const SPI_MHZ: u32 = 10;
/// Transfers this long and longer go by DMA; shorter ones are quicker
/// without.
const DMA_FROM: usize = 32;
/// Bytes of each buffer DMA copies through where it cannot reach the
/// data where it is (e.g. in flash).
const DMA_COPY: usize = 1024;
/// The gauge's and the clock's bus (the SDK's).
const I2C_KHZ: u32 = 400;

/// The SPI bus the panel and the card share.
pub type Bus = SpiDma<'static, Blocking>;
/// The panel, on its share of the bus.
pub type Panel = paperpaw_x4::panel::Panel<
    RefCellDevice<'static, Bus, Output<'static>, Delay>,
    Output<'static>,
    Output<'static>,
    Input<'static>,
    Delay,
>;

/// The SD card's parts: the bus it shares with the panel, its settings
/// at the card's starting speed, and its select line.
pub struct CardParts {
    pub bus: &'static RefCell<Bus>,
    pub spi: SpiConfig,
    pub select: Output<'static>,
    pub delay: Delay,
}

/// How the buttons are read (the ADC and its pins), and the I²C bus, on
/// which the main loop reads the fuel gauge and sets the clock (see
/// `input`).
pub struct Keys {
    pub adc: Adc<'static, ADC1<'static>, Blocking>,
    /// Back, confirm, left and right.
    pub front: AdcPin<GPIO1<'static>, ADC1<'static>, AdcCalBasic<ADC1<'static>>>,
    /// The side buttons.
    pub side: AdcPin<GPIO2<'static>, ADC1<'static>, AdcCalBasic<ADC1<'static>>>,
    pub i2c: I2c<'static, Blocking>,
}

impl Keys {
    /// The battery's charge (percent) and voltage (millivolts), from the
    /// fuel gauge.
    pub fn battery(&mut self) -> Option<(u8, u16)> {
        // Whether it charges, for USB power (`usb_power`), read with it.
        crate::usb_power::gauge_says(bq27220::charging(&mut self.i2c));
        bq27220::read(&mut self.i2c).map(|r| (r.percent, r.millivolts))
    }

    /// The buttons down, with the power button's `power_down`, and the
    /// ladders' readings (front, side) to look into.
    pub fn read(&mut self, power_down: bool) -> (State, u32) {
        let front = nb::block!(self.adc.read_oneshot(&mut self.front)).unwrap_or(4095);
        let side = nb::block!(self.adc.read_oneshot(&mut self.side)).unwrap_or(4095);
        (
            State::read(front, side, power_down),
            u32::from(front) << 16 | u32::from(side),
        )
    }

    /// The X3 has no home key (its Back is a button).
    pub fn home_down(&self) -> bool {
        false
    }

    /// The X3 has no touch screen.
    pub fn touch(&mut self, _: &mut dyn FnMut(Touch)) {}

    /// Sets the clock chip, which keeps the time while the X3 is off.
    pub fn set_clock(&mut self, unix_ms: i64) {
        let _ = ds3231::write(&mut self.i2c, &ds3231::Time::from_unix_ms(unix_ms));
    }
}

/// The X3's parts, each once.
pub struct Board {
    /// The card's power rail (GPIO 13): on, off only before deep sleep
    /// (see the module).
    pub card_power: Output<'static>,
    /// The panel, still to be started.
    pub panel: Panel,
    /// What its controller answered at start.
    pub display: Probe,
    pub card: CardParts,
    pub keys: Keys,
    pub power_button: Input<'static>,
    /// The clock chip's time at start (milliseconds since 1970), if it
    /// kept one.
    pub clock_ms: Option<i64>,
    pub usb_power: UsbPower,
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
    /// Takes the chip's parts apart, the card's power on at once.
    #[expect(
        clippy::expect_used,
        reason = "without its bus the X3 can do nothing; a panic shows why"
    )]
    // A frame of its own, off the stack before the kernel is built.
    #[inline(never)]
    pub fn take(mut p: Peripherals) -> Self {
        let card_power = Output::new(p.GPIO13, Level::High, OutputConfig::default());
        let delay = Delay::new();
        // The controller's answers, for the log and to tell the newer
        // UC8279d from the UC8253 (whatever else they say, an X3 is
        // driven as a UC8253).
        let card_select = Output::new(p.GPIO12, Level::High, OutputConfig::default());
        let display = crate::probe::ask(
            p.GPIO8.reborrow(),
            p.GPIO10.reborrow(),
            p.GPIO21.reborrow(),
            p.GPIO4.reborrow(),
            p.GPIO5.reborrow(),
            p.GPIO6.reborrow(),
            delay,
            // Held as its stock firmware holds it (the FreeInk SDK's).
            paperpaw_x4::probe::STOCK_RESET_MS,
        );
        let spi = SpiConfig::default()
            .with_frequency(Rate::from_khz(CARD_START_KHZ))
            .with_mode(Mode::_0)
            .with_min_async_transfer_size(DMA_FROM);
        let rx = esp_hal::dma_rx_buffer!(DMA_COPY).expect("SPI: DMA buffer");
        let tx = esp_hal::dma_tx_buffer!(DMA_COPY).expect("SPI: DMA buffer");
        let bus = Spi::new(p.SPI2, spi)
            .expect("SPI")
            .with_sck(p.GPIO8)
            .with_mosi(p.GPIO10)
            .with_miso(p.GPIO7)
            .with_dma(p.DMA_CH0)
            .with_buffers(rx, tx);
        let bus: &'static RefCell<Bus> = Box::leak(Box::new(RefCell::new(bus)));
        let display_select = Output::new(p.GPIO21, Level::High, OutputConfig::default());
        let panel = Panel::new(
            Device::X3,
            if display.x3_uc8279d() {
                Controller::Uc8279
            } else {
                Controller::Uc8253
            },
            RefCellDevice::new(bus, display_select, delay).expect("display SPI"),
            Output::new(p.GPIO4, Level::High, OutputConfig::default()),
            Output::new(p.GPIO5, Level::High, OutputConfig::default()),
            Input::new(p.GPIO6, InputConfig::default()),
            delay,
        );

        let mut adc = AdcConfig::new();
        // Calibrated for the ADC's offset, as on the X4.
        let front =
            adc.enable_pin_with_cal::<_, AdcCalBasic<ADC1<'_>>>(p.GPIO1, Attenuation::_11dB);
        let side = adc.enable_pin_with_cal::<_, AdcCalBasic<ADC1<'_>>>(p.GPIO2, Attenuation::_11dB);
        let mut i2c = I2c::new(
            p.I2C0,
            I2cConfig::default().with_frequency(Rate::from_khz(I2C_KHZ)),
        )
        .expect("I2C")
        .with_sda(p.GPIO20)
        .with_scl(p.GPIO0);
        let clock_ms = ds3231::read(&mut i2c).map(|t| t.unix_ms());
        // The gauge forgets the cell's capacity when it loses power: given
        // back here (once, some seconds) where it says TI's default.
        let mut waits = delay;
        match bq27220::load_capacity(&mut i2c, &mut waits, bq27220::X3_CAPACITY_MAH) {
            bq27220::Capacity::Kept => {}
            bq27220::Capacity::Written => {
                log::info!("battery: the gauge has the cell's capacity again")
            }
            bq27220::Capacity::Failed => {
                log::warn!(
                    "battery: the gauge's capacity could not be set (tried again next start)"
                );
            }
        }

        Self {
            card_power,
            panel,
            display,
            card: CardParts {
                bus,
                spi,
                select: card_select,
                delay,
            },
            keys: Keys {
                adc: Adc::new(p.ADC1, adc),
                front,
                side,
                i2c,
            },
            power_button: Input::new(p.GPIO3, InputConfig::default().with_pull(Pull::Up)),
            clock_ms,
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
