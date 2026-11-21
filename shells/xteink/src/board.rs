//! The X4's board: which of the ESP32-C3's pins and units does what, the
//! one place that says so. The rest of the firmware takes its parts from
//! [`Board`]; the X4 Pro and the X4 Classic (ESP32-S3s) have boards of
//! their own.
//!
//! | Part | Connection |
//! |---|---|
//! | 4.26" panel, 800 × 480, SSD1677 controller (an UltraChip found at start is driven too) | SPI: clock GPIO 8, data out GPIO 10, select GPIO 21; data/command GPIO 4, reset GPIO 5, busy GPIO 6 |
//! | microSD card | the same SPI (data in GPIO 7), select GPIO 12 |
//! | Back, confirm, left, right | resistor ladder on ADC GPIO 1 |
//! | The side buttons (up, down) | resistor ladder on ADC GPIO 2 |
//! | Power button | GPIO 3, low while pressed |
//! | Battery | half its voltage on ADC GPIO 0 |
//! | USB connected | GPIO 20, high |
//! | The battery's power | held on by GPIO 13, cut when it goes low |

use alloc::boxed::Box;
use core::cell::RefCell;

use embedded_hal_bus::spi::RefCellDevice;
use esp_hal::Blocking;
use esp_hal::analog::adc::{Adc, AdcCalBasic, AdcCalCurve, AdcConfig, AdcPin, Attenuation};
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::peripherals::{ADC1, GPIO0, GPIO1, GPIO2, LPWR, Peripherals, RTC_TIMER};
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, Spi, SpiDma};
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use paperpaw_x4::buttons::State;
use paperpaw_x4::panel::Device;
use paperpaw_x4::probe::{Controller, Probe};

use crate::input::Touch;

/// SPI speeds: an SD card starts slowly.
const CARD_START_KHZ: u32 = 400;
pub const SPI_MHZ: u32 = 20;
/// Transfers this long and longer go by DMA; shorter ones (commands,
/// the card's answers) are quicker without.
const DMA_FROM: usize = 32;
/// Bytes of each buffer DMA copies through where it cannot reach the
/// data where it is (e.g. in flash).
const DMA_COPY: usize = 1024;

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

/// On USB power while high.
pub type UsbPower = Input<'static>;

/// How the buttons and the battery are read: the ADC and its pins.
pub struct Keys {
    pub adc: Adc<'static, ADC1<'static>, Blocking>,
    /// Back, confirm, left and right.
    pub front: AdcPin<GPIO1<'static>, ADC1<'static>, AdcCalBasic<ADC1<'static>>>,
    /// The side buttons.
    pub side: AdcPin<GPIO2<'static>, ADC1<'static>, AdcCalBasic<ADC1<'static>>>,
    /// Half the battery's voltage.
    pub cell: AdcPin<GPIO0<'static>, ADC1<'static>, AdcCalCurve<ADC1<'static>>>,
}

impl Keys {
    /// The battery's charge (percent) and voltage (millivolts).
    pub fn battery(&mut self) -> Option<(u8, u16)> {
        let raw = nb::block!(self.adc.read_oneshot(&mut self.cell)).ok()?;
        let millivolts = paperpaw_x4::battery::millivolts(raw);
        Some((paperpaw_x4::battery::percent(millivolts), millivolts))
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

    /// The X4 has no home key (its Back is a button).
    pub fn home_down(&self) -> bool {
        false
    }

    /// The X4 has no touch screen.
    pub fn touch(&mut self, _: &mut dyn FnMut(Touch)) {}

    /// The X4 has no clock that runs while it is off.
    pub fn set_clock(&mut self, _: i64) {}
}

/// The X4's parts, each once.
pub struct Board {
    /// Holds the battery's power on; low, it is cut.
    pub power_latch: Output<'static>,
    /// The panel, still to be started.
    pub panel: Panel,
    /// What its controller answered at start.
    pub display: Probe,
    pub card: CardParts,
    pub keys: Keys,
    pub power_button: Input<'static>,
    /// High while on USB power.
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
    /// Takes the chip's parts apart. The battery's power is held at once:
    /// it stays on only while the power button is down otherwise.
    #[expect(
        clippy::expect_used,
        reason = "without its bus the X4 can do nothing; a panic shows why"
    )]
    // A frame of its own, off the stack before the kernel is built.
    #[inline(never)]
    pub fn take(mut p: Peripherals) -> Self {
        // The battery's latch (the flash's free WP pad) is never pad-held
        // (#451): a light sleep leaves it driving as awake (esp-hal selects
        // no sleep settings for it, and VDD_SPI, its supply, stays on). A
        // latch held through a sleep is what hung CrossPoint's X4 when a
        // cable came in.
        let power_latch = Output::new(p.GPIO13, Level::High, OutputConfig::default());
        let delay = Delay::new();
        // Which controller the panel has, asked on its lines before the
        // bus takes them (the card not selected meanwhile); without a sure
        // answer, the X4's SSD1677.
        let card_select = Output::new(p.GPIO12, Level::High, OutputConfig::default());
        let display = crate::probe::ask(
            p.GPIO8.reborrow(),
            p.GPIO10.reborrow(),
            p.GPIO21.reborrow(),
            p.GPIO4.reborrow(),
            p.GPIO5.reborrow(),
            p.GPIO6.reborrow(),
            delay,
            paperpaw_x4::probe::RESET_MS,
        );
        // One SPI bus for the display and the card, with DMA for all but
        // short transfers: the processor fed the bus a word at a time and
        // waited, a third of the time a book took to open.
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
            Device::X4,
            display.controller().unwrap_or(Controller::Ssd1677),
            RefCellDevice::new(bus, display_select, delay).expect("display SPI"),
            Output::new(p.GPIO4, Level::High, OutputConfig::default()),
            Output::new(p.GPIO5, Level::High, OutputConfig::default()),
            Input::new(p.GPIO6, InputConfig::default()),
            delay,
        );

        let mut adc = AdcConfig::new();
        // Calibrated for the ADC's offset: uncalibrated, readings came out
        // some 1600 too high, and the two highest buttons of each ladder
        // were lost above the top of the range.
        let front =
            adc.enable_pin_with_cal::<_, AdcCalBasic<ADC1<'_>>>(p.GPIO1, Attenuation::_11dB);
        let side = adc.enable_pin_with_cal::<_, AdcCalBasic<ADC1<'_>>>(p.GPIO2, Attenuation::_11dB);
        let cell = adc.enable_pin_with_cal::<_, AdcCalCurve<ADC1<'_>>>(p.GPIO0, Attenuation::_11dB);
        let keys = Keys {
            adc: Adc::new(p.ADC1, adc),
            front,
            side,
            cell,
        };

        Self {
            power_latch,
            panel,
            display,
            card: CardParts {
                bus,
                spi,
                select: card_select,
                delay,
            },
            keys,
            power_button: Input::new(p.GPIO3, InputConfig::default().with_pull(Pull::Up)),
            usb_power: Input::new(p.GPIO20, InputConfig::default()),
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
