//! The panel of an X4 or X4 Pro, whichever controller drives it: the
//! driver for the controller found at start ([`probe`](crate::probe)),
//! with the calls the firmware makes of any.

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use paperpaw_platform::{FrameView, Rotation};

use crate::display::{self, Error, Planes, Refresh};
use crate::probe::Controller;
use crate::uc8179::{self, Chip, Frame};
use crate::uc8253;

/// Which device the panel is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    /// The X4: no memory for a frame of its own; turned clockwise on its
    /// side ([`Rotation::Deg90`], its buttons on the left).
    X4,
    /// The X4 Pro: a frame kept in its PSRAM; turned the other way on
    /// [`Rotation::Deg90`] (see [`uc8179::turned`]).
    X4Pro,
    /// The X4 Classic: the X4 Pro's glass and PSRAM, driven as the X4
    /// Pro's (its turning not yet seen on one), but a UC8279 keeps its
    /// factory frame rate, as its stock firmware does (FreeInk SDK's
    /// `Uc8279X4Driver`, Papyrix's notes, MIT).
    X4Classic,
    /// The X3: a UC8253 at 792 × 528, whichever controller was found (see
    /// [`uc8253`]); no frame of its own, as the X4.
    X3,
}

/// A way to show fast refreshes other than the panel's own, for the
/// waveform lab, by the controller it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Experiment {
    Ssd1677(display::Experiment),
    Uc8179(uc8179::Experiment),
}

/// A panel, its controller's driver.
#[derive(Debug)]
pub enum Panel<SPI, DC, RST, BUSY, D> {
    Ssd1677 {
        display: display::Display<SPI, DC, RST, BUSY, D>,
        /// The X4 Pro's sides: [`Rotation::Deg90`] and
        /// [`Rotation::Deg270`] are the other way round.
        sides_swapped: bool,
    },
    UltraChip(uc8179::Display<SPI, DC, RST, BUSY, D>),
    Uc8253(uc8253::Display<SPI, DC, RST, BUSY, D>),
}

/// The same call to whichever driver.
macro_rules! each {
    ($panel:expr, $display:ident => $call:expr) => {
        match $panel {
            Panel::Ssd1677 {
                display: $display, ..
            } => $call,
            Panel::UltraChip($display) => $call,
            Panel::Uc8253($display) => $call,
        }
    };
}

impl<SPI, DC, RST, BUSY, D> Panel<SPI, DC, RST, BUSY, D>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    /// The driver for `controller` in `device`, on the panel's SPI device
    /// and pins.
    pub fn new(
        device: Device,
        controller: Controller,
        spi: SPI,
        dc: DC,
        reset: RST,
        busy: BUSY,
        delay: D,
    ) -> Self {
        if device == Device::X3 {
            return Panel::Uc8253(uc8253::Display::new(spi, dc, reset, busy, delay));
        }
        let chip = match controller {
            Controller::Ssd1677 => {
                return Panel::Ssd1677 {
                    display: display::Display::new(spi, dc, reset, busy, delay),
                    sides_swapped: matches!(device, Device::X4Pro | Device::X4Classic),
                };
            }
            Controller::Uc8179 => Chip::Uc8179,
            Controller::Uc8279 | Controller::Uc8253 => Chip::Uc8279,
        };
        let frame = match device {
            Device::X4 | Device::X3 => Frame::InController,
            Device::X4Pro | Device::X4Classic => Frame::Kept,
        };
        let display = uc8179::Display::new(chip, frame, spi, dc, reset, busy, delay);
        Panel::UltraChip(if device == Device::X4Classic {
            display.with_factory_pll()
        } else {
            display
        })
    }

    /// The controller driven.
    pub fn controller(&self) -> Controller {
        match self {
            Panel::Ssd1677 { .. } => Controller::Ssd1677,
            Panel::UltraChip(display) => match display.chip() {
                Chip::Uc8179 => Controller::Uc8179,
                Chip::Uc8279 => Controller::Uc8279,
            },
            Panel::Uc8253(_) => Controller::Uc8253,
        }
    }

    /// How the screen PaperPaw draws is turned, as the device's setting
    /// says (see [`Device`]).
    pub fn set_rotation(&mut self, rotation: Rotation) {
        match self {
            Panel::Ssd1677 {
                display,
                sides_swapped,
            } => display.set_rotation(swapped(rotation, *sides_swapped)),
            Panel::UltraChip(display) => display.set_rotation(rotation),
            Panel::Uc8253(display) => display.set_rotation(rotation),
        }
    }

    pub fn rotation(&self) -> Rotation {
        match self {
            Panel::Ssd1677 {
                display,
                sides_swapped,
            } => swapped(display.rotation(), *sides_swapped),
            Panel::UltraChip(display) => display.rotation(),
            Panel::Uc8253(display) => display.rotation(),
        }
    }

    /// The width and height of the screen PaperPaw draws, turned.
    pub fn size(&self) -> (u16, u16) {
        each!(self, d => d.size())
    }

    pub fn init(&mut self) -> Result<(), Error> {
        each!(self, d => d.init())
    }

    pub fn write_rows(
        &mut self,
        top: u16,
        rows: FrameView<'_>,
        planes: Planes,
    ) -> Result<(), Error> {
        each!(self, d => d.write_rows(top, rows, planes))
    }

    /// Whether the driver keeps the frame and sends it as the one shown
    /// after every refresh by itself (the X4 Pro's and the X4 Classic's
    /// UltraChip controllers): what was sent need not go again.
    pub fn keeps_frame(&self) -> bool {
        matches!(self, Panel::UltraChip(display) if display.keeps_frame())
    }

    pub fn write_gray_rows(&mut self, top: u16, rows: FrameView<'_>) -> Result<(), Error> {
        each!(self, d => d.write_gray_rows(top, rows))
    }

    pub fn refresh_gray(&mut self) -> Result<(), Error> {
        each!(self, d => d.refresh_gray())
    }

    /// As [`Panel::refresh_gray`], for a screen that cleans (the sleep
    /// screen): where the controller needs a refresh of every pixel before
    /// the grays, it makes one (the UC8179); the others show them as they
    /// do.
    pub fn refresh_gray_clean(&mut self) -> Result<(), Error> {
        match self {
            Panel::UltraChip(display) => display.refresh_gray_clean(),
            _ => self.refresh_gray(),
        }
    }

    /// How dark the grays between come out on the X4's controller (the X4
    /// Pro's lightens them from black, as dark as they are).
    pub fn set_gray_tone(&mut self, tone: display::GrayTone) {
        if let Panel::Ssd1677 { display, .. } = self {
            display.set_gray_tone(tone);
        }
    }

    /// Shows fast refreshes as `experiment` says; `false` where this
    /// controller cannot (only the SSD1677's and the UC8179's waveforms can
    /// be tried), or where `experiment` is for the other, which then show
    /// them their own way as before.
    pub fn set_experiment(&mut self, experiment: Option<Experiment>) -> bool {
        match self {
            Panel::Ssd1677 { display, .. } => {
                display.set_experiment(match experiment {
                    Some(Experiment::Ssd1677(experiment)) => Some(experiment),
                    _ => None,
                });
                true
            }
            Panel::UltraChip(display) => display.set_experiment(match experiment {
                Some(Experiment::Uc8179(experiment)) => Some(experiment),
                _ => None,
            }),
            Panel::Uc8253(_) => false,
        }
    }

    /// The voltages the UC8179's waveforms from the registers run at (those
    /// of the OTP's own waveform, see `uc8179::Power`); no other
    /// controller has them.
    pub fn set_trial_power(&mut self, power: Option<uc8179::Power>) {
        if let Panel::UltraChip(display) = self {
            display.set_trial_power(power);
        }
    }

    /// How much of the frame the UC8179 sends for a fast refresh (see
    /// `uc8179::Transfer`); no other controller has a choice.
    pub fn set_transfer(&mut self, transfer: uc8179::Transfer) {
        if let Panel::UltraChip(display) = self {
            display.set_transfer(transfer);
        }
    }

    /// The milliseconds the UC8179 waited for its busy pin since the last
    /// call (0 for the others).
    pub fn take_busy_ms(&mut self) -> u32 {
        match self {
            Panel::UltraChip(display) => display.take_busy_ms(),
            _ => 0,
        }
    }

    /// Whether the driver can keep the grays on the panel while a small
    /// part of the screen changes (#218, an experiment): only the
    /// SSD1677's can, with the memories of both frames.
    pub fn can_change_in_grays(&self) -> bool {
        matches!(self, Panel::Ssd1677 { .. })
    }

    /// The part of a band in `rect` (`x0, y0, x1, y1`) into `planes`, the
    /// rest of the memory as it is, inverted where asked (see
    /// [`display::Display::write_rect`]); `Error::Rows` where the driver
    /// cannot (see [`Panel::can_change_in_grays`]).
    pub fn write_rect(
        &mut self,
        top: u16,
        rows: FrameView<'_>,
        rect: (u16, u16, u16, u16),
        planes: Planes,
        invert: bool,
    ) -> Result<(), Error> {
        match self {
            Panel::Ssd1677 { display, .. } => display.write_rect(top, rows, rect, planes, invert),
            _ => Err(Error::Rows),
        }
    }

    /// Starts a fast refresh that leaves the grays where both memories
    /// agree (see [`display::Display::start_refresh_keeping_grays`]).
    pub fn start_refresh_keeping_grays(&mut self, power_off: bool) -> Result<(), Error> {
        match self {
            Panel::Ssd1677 { display, .. } => display.start_refresh_keeping_grays(power_off),
            _ => Err(Error::Rows),
        }
    }

    pub fn refresh(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        each!(self, d => d.refresh(refresh, power_off))
    }

    pub fn start_refresh(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        each!(self, d => d.start_refresh(refresh, power_off))
    }

    pub fn wait(&mut self) -> Result<(), Error> {
        each!(self, d => d.wait())
    }

    /// As [`Panel::wait`], calling `meanwhile` each round of the wait (about
    /// a millisecond), for work the processor can do while the panel
    /// refreshes: keeping the radio's queue of five frames emptied.
    pub fn wait_with(&mut self, meanwhile: impl FnMut()) -> Result<(), Error> {
        each!(self, d => d.wait_with(meanwhile))
    }

    /// Whether a refresh started still goes on, without waiting;
    /// [`Panel::wait`] ends it once it is over.
    pub fn refreshing(&mut self) -> Result<bool, Error> {
        each!(self, d => d.refreshing())
    }

    pub fn sleep(&mut self) -> Result<(), Error> {
        each!(self, d => d.sleep())
    }

    /// Turns the panel's power off, the picture staying (see the
    /// controllers' `power_down`).
    pub fn power_down(&mut self) -> Result<(), Error> {
        each!(self, d => d.power_down())
    }

    /// Whether the panel's power is on.
    pub fn powered(&self) -> bool {
        each!(self, d => d.powered())
    }

    /// How long a refresh takes longer once the power was off, in
    /// milliseconds: the SSD1677's sequences power up anyway, the
    /// UltraChip controllers' take about an eighth of a second more.
    pub fn power_up_ms(&self) -> u64 {
        match self {
            Panel::Ssd1677 { .. } => 0,
            Panel::UltraChip(_) | Panel::Uc8253(_) => 130,
        }
    }
}

/// `rotation` with its sides the other way round, if `swap`.
fn swapped(rotation: Rotation, swap: bool) -> Rotation {
    match rotation {
        Rotation::Deg90 if swap => Rotation::Deg270,
        Rotation::Deg270 if swap => Rotation::Deg90,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::convert::Infallible;
    use embedded_hal::spi::{ErrorType, Operation};

    struct Spi;
    struct Pin;
    struct Delay;

    impl ErrorType for Spi {
        type Error = Infallible;
    }
    impl SpiDevice for Spi {
        fn transaction(&mut self, _: &mut [Operation<'_, u8>]) -> Result<(), Infallible> {
            Ok(())
        }
    }
    impl embedded_hal::digital::ErrorType for Pin {
        type Error = Infallible;
    }
    impl OutputPin for Pin {
        fn set_low(&mut self) -> Result<(), Infallible> {
            Ok(())
        }
        fn set_high(&mut self) -> Result<(), Infallible> {
            Ok(())
        }
    }
    impl InputPin for Pin {
        fn is_high(&mut self) -> Result<bool, Infallible> {
            Ok(true)
        }
        fn is_low(&mut self) -> Result<bool, Infallible> {
            Ok(false)
        }
    }
    impl DelayNs for Delay {
        fn delay_ns(&mut self, _: u32) {}
    }

    fn panel(device: Device, controller: Controller) -> Panel<Spi, Pin, Pin, Pin, Delay> {
        Panel::new(device, controller, Spi, Pin, Pin, Pin, Delay)
    }

    #[test]
    fn each_controller_its_driver() {
        for device in [Device::X4, Device::X4Pro, Device::X4Classic] {
            for controller in [Controller::Ssd1677, Controller::Uc8179, Controller::Uc8279] {
                let panel = panel(device, controller);
                assert_eq!(panel.controller(), controller);
                let ultra = matches!(panel, Panel::UltraChip(_));
                assert_eq!(ultra, controller != Controller::Ssd1677);
                // Only the X4 Pro's and Classic's UltraChip keep the frame.
                assert_eq!(panel.keeps_frame(), ultra && device != Device::X4);
            }
        }
        assert!(!panel(Device::X3, Controller::Uc8253).keeps_frame());
    }

    /// An X3 gets its UC8253 driver whatever the probe said, and draws
    /// 528 × 792 upright.
    #[test]
    fn an_x3_has_its_uc8253() {
        for controller in [Controller::Ssd1677, Controller::Uc8179, Controller::Uc8253] {
            let panel = panel(Device::X3, controller);
            assert_eq!(panel.controller(), Controller::Uc8253);
            assert_eq!(panel.size(), (528, 792));
            assert_eq!(panel.power_up_ms(), 130);
        }
    }

    #[test]
    fn an_x4_pro_turns_its_way_whichever_its_controller() {
        for device in [Device::X4Pro, Device::X4Classic] {
            let mut ssd = panel(device, Controller::Ssd1677);
            ssd.set_rotation(Rotation::Deg90);
            assert_eq!(ssd.rotation(), Rotation::Deg90);
            assert_eq!(ssd.size(), (800, 480));
            let Panel::Ssd1677 { display, .. } = &ssd else {
                unreachable!()
            };
            assert_eq!(display.rotation(), Rotation::Deg270);
        }
        let mut x4 = panel(Device::X4, Controller::Ssd1677);
        x4.set_rotation(Rotation::Deg90);
        let Panel::Ssd1677 { display, .. } = &x4 else {
            unreachable!()
        };
        assert_eq!(display.rotation(), Rotation::Deg90);
    }
}
