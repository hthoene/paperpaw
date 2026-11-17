/// What a menu entry needs to be offered (see
/// `Items::set_menu_need`), so
/// menus stay short on every device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MenuNeed {
    /// A device that speaks ([`Capabilities::speech`](crate::Capabilities)): hidden
    /// elsewhere, also where the user listed it.
    Speech,
    /// A device where PaperPaw shows the sleep screen
    /// ([`Capabilities::sleep`](crate::Capabilities)), hidden elsewhere.
    Sleep,
    /// A device whose screen PaperPaw can turn
    /// ([`settings::ROTATION`](crate::builtin::settings::ROTATION)),
    /// hidden elsewhere.
    Rotation,
    /// A device without a touch screen, where it is the way to do what a
    /// tap or a long press does elsewhere (e.g. choosing a link). On touch
    /// devices it is left out of the menus plugins define; users can still
    /// list it.
    NoTouch,
    /// A device with a touch screen, e.g. for what a tap or a swipe does:
    /// hidden elsewhere, also where the user listed it.
    Touch,
    /// A device with buttons in a row below the screen, whose row of
    /// hints shows what they do
    /// ([`settings::BUTTON_HINTS`](crate::builtin::settings::BUTTON_HINTS)),
    /// hidden elsewhere.
    ButtonRow,
    /// A device whose shell shows the screen in other colours than black
    /// and white ([`settings::TEXT_COLOR`](crate::builtin::settings::TEXT_COLOR)),
    /// hidden elsewhere.
    Colors,
    /// A device where PaperPaw manages Wi-Fi itself
    /// (`Env::wifi`), hidden elsewhere.
    Wifi,
    /// A device whose firmware PaperPaw is, updated by PaperPaw
    /// (`Env::firmware`), hidden elsewhere.
    Firmware,
    /// A device whose panel shows black and white alone
    /// ([`Capabilities::black_and_white`](crate::Capabilities)), hidden
    /// elsewhere.
    BlackAndWhite,
    /// A front light PaperPaw sets ([`Capabilities::frontlight`](crate::Capabilities)).
    Frontlight,
    /// A Bluetooth radio the kernel uses (`Env::bluetooth`),
    /// hidden elsewhere.
    Bluetooth,
    /// A device that can do a [`Capability`](crate::Capability) (new
    /// needs come as capabilities, without a new plugin interface).
    Has(crate::Capability),
}

impl MenuNeed {
    /// Whether a device that can do `caps` has what it needs (also for
    /// the kernel's `ChromeSpec::need`).
    pub fn met(self, caps: crate::Capabilities) -> bool {
        match self {
            MenuNeed::Speech => caps.speech,
            MenuNeed::Sleep => caps.sleep,
            MenuNeed::Rotation => caps.rotation,
            MenuNeed::Colors => caps.colors,
            MenuNeed::ButtonRow => caps.button_row,
            MenuNeed::Wifi => caps.wifi,
            MenuNeed::Bluetooth => caps.bluetooth,
            MenuNeed::Firmware => caps.firmware,
            MenuNeed::BlackAndWhite => caps.black_and_white,
            MenuNeed::NoTouch => !caps.touch,
            MenuNeed::Touch => caps.touch,
            MenuNeed::Frontlight => caps.frontlight,
            MenuNeed::Has(capability) => caps.has(capability),
        }
    }
}
