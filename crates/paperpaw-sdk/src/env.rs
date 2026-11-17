
use paperpaw_platform::Storage;


/// What the device and its shell can do: plugins offer only what works
/// here, and menu entries that need more are left out
/// ([`MenuNeed`](crate::MenuNeed)).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Capabilities {
    /// A touch screen.
    pub touch: bool,
    /// The network can be reached: HTTP requests
    /// ([`Command::Http`](crate::Command::Http)), servers and connections.
    /// Not whether it is connected now: where PaperPaw manages Wi-Fi
    /// ([`Env::wifi`]), work waits for a network a while.
    pub network: bool,
    /// PaperPaw manages Wi-Fi itself ([`Env::wifi`]).
    pub wifi: bool,
    /// A Bluetooth Low Energy radio the kernel uses ([`Env::bluetooth`]).
    pub bluetooth: bool,
    /// PaperPaw is the device's firmware and updates it
    /// ([`Env::firmware`]).
    pub firmware: bool,
    /// Text is spoken aloud ([`Command::Speak`](crate::Command::Speak)):
    /// by the system, or a plugin's voice (see `voices`).
    pub speech: bool,
    /// PaperPaw shows a screen while the device sleeps
    /// ([`actions::SLEEP`](crate::builtin::actions::SLEEP)); where the
    /// system sleeps on its own (Android), it shows its own.
    pub sleep: bool,
    /// The screen can be turned
    /// ([`settings::ROTATION`](crate::builtin::settings::ROTATION)).
    pub rotation: bool,
    /// The screen is shown in the colours of
    /// [`settings::TEXT_COLOR`](crate::builtin::settings::TEXT_COLOR) and
    /// [`settings::PAPER_COLOR`](crate::builtin::settings::PAPER_COLOR).
    pub colors: bool,
    /// Buttons lie in a row below the screen, with a row of hints above
    /// them ([`settings::BUTTON_HINTS`](crate::builtin::settings::BUTTON_HINTS)).
    pub button_row: bool,
    /// The system has a clipboard of its own, shared with other apps.
    pub system_clipboard: bool,
    /// A front light PaperPaw sets
    /// ([`settings::FRONTLIGHT`](crate::builtin::settings::FRONTLIGHT)).
    pub frontlight: bool,
    /// The panel shows black and white alone (no grays), e.g. the X4.
    pub black_and_white: bool,
    /// PaperPaw switches the device off on its own
    /// ([`Command::PowerOff`](crate::Command::PowerOff)), e.g. the X4; not
    /// where the system decides (Android) or that never switches off (the
    /// emulator).
    pub power_off: bool,
    /// How many TLS connections may be open at once, HTTPS requests among
    /// them: sockets wait for one to be free ([`Tls::timeout_ms`](crate::Tls::timeout_ms)).
    /// One on the X4, two on the X4 Pro; 0 where the shell does no TLS for
    /// sockets ([`Command::connect_tls`](crate::Command::connect_tls) fails,
    /// [`SocketError::Unsupported`](crate::SocketError::Unsupported)).
    pub tls_sessions: u8,
    /// What the shell changes by the power level (ADR 0025): the power
    /// settings show only what applies here.
    pub power: crate::power::PowerControls,
    /// More of what the device can do, by [`Capability`] (new ones come as
    /// constants, without a new plugin interface).
    pub(crate) more: u64,
}

/// What a device can do beyond [`Capabilities`]' fields
/// ([`Capabilities::has`]): constants of one type, so more can come
/// without a new plugin interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Capability(u64);

impl Capability {
    /// Capability bits used by the generated menu declaration codec.
    pub(crate) const fn menu_declaration_bits(self) -> u64 {
        self.0
    }

    /// Sound can be played.
    pub const AUDIO: Self = Self(1);
    /// The front light's warmth can be set.
    pub const WARMTH: Self = Self(1 << 1);
    /// Charging can be stopped at a limit.
    pub const CHARGE_LIMIT: Self = Self(1 << 2);
    /// A keyboard of keys (not one on the screen) is there.
    pub const KEYBOARD: Self = Self(1 << 3);
    /// Code plugins can mark hot code that runs from internal RAM
    /// (`paperpaw_sdk::hot!`).
    pub const HOT_CODE: Self = Self(1 << 4);
    /// Keys that turn pages (page forward and back, or left and right).
    pub const PAGE_KEYS: Self = Self(1 << 5);
    /// Buttons enough for every screen without touch: *Back*, *Confirm*
    /// and keys to move the focus (up and down, or left and right). Where
    /// a device has page keys alone, screens offer by touch what they
    /// would by buttons.
    pub const BUTTONS: Self = Self(1 << 6);
    /// A USB link to a computer that developer tools may use
    /// ([`developer`](crate::developer)), once the reader allows them.
    pub const USB_LINK: Self = Self(1 << 7);
    /// The kernel calls render hooks (`render`). Where it
    /// is missing a hook is accepted and never run; the devices say so
    /// from the release that runs them.
    pub const RENDER_HOOK: Self = Self(1 << 8);
    /// The kernel fills `BandView::regions`
    /// where it knows what a band holds; where it is missing they are
    /// empty, which means unknown.
    pub const RENDER_REGIONS: Self = Self(1 << 9);
    /// The SDK asks a look for parts of kinds beyond the named ones
    /// (`Painters::extras`); where it
    /// is missing none is asked.
    pub const LOOK_EXTRAS: Self = Self(1 << 10);
    /// The card can be shown to a computer as a USB drive
    /// ([`Command::UsbDrive`](crate::Command::UsbDrive)), e.g. the X4 Pro.
    pub const USB_DRIVE: Self = Self(1 << 11);
    /// Buttons pressed together are told apart, so key maps may bind
    /// chords (`chord.<button>_<button>`). Reserved: no shell says so yet,
    /// and the kernel reads such bindings but raises none.
    pub const CHORDS: Self = Self(1 << 12);
    /// Bluetooth page turners and keyboards connect as buttons (their keys
    /// `hid.<name>` in key maps). Reserved: no shell says so yet.
    pub const PAGE_TURNER: Self = Self(1 << 13);
    /// The device opens a Wi-Fi network of its own to be reached without
    /// a router (transfer, the phone keyboard). Reserved: no shell says so
    /// yet.
    pub const HOTSPOT: Self = Self(1 << 14);
    /// A pen that tells how it leans
    /// ([`PointerEvent::tilt_x`](paperpaw_platform::PointerEvent::tilt_x)).
    /// Reserved: no shell says so yet.
    pub const PEN_TILT: Self = Self(1 << 15);
    /// A pen seen above the screen before it touches
    /// ([`PointerPhase::Hover`](paperpaw_platform::PointerPhase::Hover)).
    /// Reserved: no shell says so yet, and the kernel passes it to no
    /// screen.
    pub const PEN_HOVER: Self = Self(1 << 16);
}

impl Capabilities {
    /// Whether the device can do `capability`.
    pub const fn has(&self, capability: Capability) -> bool {
        self.more & capability.0 == capability.0
    }

    /// With `capability` too (the kernel and shells say so).
    #[cfg(feature = "kernel")]
    #[must_use]
    pub const fn with(mut self, capability: Capability) -> Self {
        self.more |= capability.0;
        self
    }
}








/// Converts physical lengths to pixels for the current screen, so layouts
/// look the same on low- and high-density displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Metrics {
    pub dpi: u16,
}

impl Metrics {
    pub const fn new(dpi: u16) -> Self {
        Self { dpi }
    }

    /// Pixels for `mm` millimetres (rounded, at least 1 for non-zero input).
    pub const fn mm(&self, mm: u32) -> u32 {
        self.tenth_mm(mm * 10)
    }

    /// Pixels for a font size in typographic points (1/72 inch).
    pub fn pt(&self, points: f32) -> f32 {
        points * f32::from(self.dpi) / 72.0
    }

    /// Pixels for `tenths` tenths of a millimetre.
    pub const fn tenth_mm(&self, tenths: u32) -> u32 {
        let px = (tenths * self.dpi as u32 + 127) / 254;
        if px == 0 && tenths > 0 { 1 } else { px }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_millimetres() {
        let metrics = Metrics::new(254);
        assert_eq!(metrics.mm(1), 10);
        assert_eq!(metrics.tenth_mm(5), 5);
        assert_eq!(Metrics::new(100).tenth_mm(1), 1);
        assert_eq!(Metrics::new(100).mm(0), 0);
    }
}
