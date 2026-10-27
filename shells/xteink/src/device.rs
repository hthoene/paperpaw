//! The device's own plugin: what only the X4, the X4 Pro, the X4
//! Classic or the X3 has to set.
//! All sleep lightly between readings of their buttons (for developers,
//! in their group of settings); the X4 Pro's front light has its
//! brightness and warmth, in the quick panel and the reader's panel too.

use alloc::borrow::Cow;

use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::{Id, Manifest, Plugin, RegisterError, Registrar, SettingSpec, Version};

pub const PLUGIN: Id = Id::new_static("x4");
/// The processor sleeps lightly while idle on battery (see `power`).
pub const LIGHT_SLEEP: Setting<bool> = Setting::new("x4.light-sleep");
/// How quickly presses are answered on battery: *Faster* sleeps the
/// processor less between readings of the buttons, and the X4 Pro's panel
/// stays powered; *Saving* sleeps it longer, for a little less current
/// (see `firmware`).
pub const BUTTON_RESPONSE: Setting<paperpaw_sdk::setting::Text> =
    Setting::new("x4.button-response");
/// The choices of [`BUTTON_RESPONSE`].
pub const RESPONSE_NORMAL: &str = "x4.button-response.normal";
pub const RESPONSE_FASTER: &str = "x4.button-response.faster";
pub const RESPONSE_SAVING: &str = "x4.button-response.saving";
/// The X4 Pro's front light: how bright, in percent (0 off).
#[cfg_attr(
    not(feature = "x4-pro"),
    expect(dead_code, reason = "only the X4 Pro has a light")
)]
pub const FRONTLIGHT: Setting<i64> = Setting::new("x4.frontlight");
/// How much of the light is warm, in percent.
#[cfg_attr(
    not(feature = "x4-pro"),
    expect(dead_code, reason = "only the X4 Pro has a light")
)]
pub const WARMTH: Setting<i64> = Setting::new("x4.warmth");

#[derive(Debug, Default)]
pub struct X4;

/// How quickly presses are answered; whether it sleeps lightly between
/// button reads.
static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::choice(
        BUTTON_RESPONSE.static_str(),
        "x4.button-response",
        &[RESPONSE_SAVING, RESPONSE_NORMAL, RESPONSE_FASTER],
        RESPONSE_NORMAL,
        groups::CONTROLS,
    )
    .description("x4.button-response.help"),
    SettingSpec::switch(
        LIGHT_SLEEP.static_str(),
        "x4.light-sleep",
        true,
        groups::DEVELOPER,
    )
    .description("x4.light-sleep.help"),
];

/// The X4 Pro's front light, set with sliders: how bright in single
/// percents, so that `−` and `+` set the dim end finely (see `light`), its
/// colour in fives.
#[cfg(feature = "x4-pro")]
static LIGHT_SETTINGS: [SettingSpec; 2] = {
    use paperpaw_sdk::SettingExtra;
    static BRIGHTNESS: SettingExtra = SettingExtra::new().unit("core.unit.percent").slider();
    static PERCENT: SettingExtra = SettingExtra::new()
        .unit("core.unit.percent")
        .step(5)
        .slider();
    [
        SettingSpec::number(
            FRONTLIGHT.static_str(),
            "x4.frontlight",
            0,
            100,
            0,
            groups::DISPLAY,
        )
        .with_extra(&BRIGHTNESS)
        .description("x4.frontlight.help"),
        SettingSpec::number(
            WARMTH.static_str(),
            "x4.warmth",
            0,
            100,
            30,
            groups::DISPLAY,
        )
        .with_extra(&PERCENT)
        .description("x4.warmth.help"),
    ]
};

impl Plugin for X4 {
    fn manifest(&self) -> Manifest {
        #[cfg(feature = "x4")]
        let (name, description) = ("Xteink X4", "What only the Xteink X4 has to set.");
        #[cfg(feature = "x4-pro")]
        let (name, description) = (
            "Xteink X4 Pro",
            "What only the Xteink X4 Pro has to set: its front light.",
        );
        #[cfg(feature = "x4-classic")]
        let (name, description) = (
            "Xteink X4 Classic",
            "What only the Xteink X4 Classic has to set.",
        );
        #[cfg(feature = "x3")]
        let (name, description) = ("Xteink X3", "What only the Xteink X3 has to set.");
        Manifest::new(PLUGIN, name, Version::new(0, 1, 0))
            .description(description)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        #[cfg(feature = "x4-pro")]
        register_light(r)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// The front light's settings, also first in the reader's panel and in
/// the quick panel.
#[cfg(feature = "x4-pro")]
fn register_light(r: &mut Registrar) -> Result<(), RegisterError> {
    use paperpaw_sdk::MenuEntry;
    use paperpaw_sdk::builtin::menus;
    r.add_settings(&LIGHT_SETTINGS)?;
    for (n, item) in [FRONTLIGHT, WARMTH].into_iter().enumerate() {
        for (menu, order) in [(menus::READER_PANEL, -10), (menus::QUICK, 20)] {
            r.add_menu_entry(MenuEntry::new(menu, item.clone().into()).order(order + n as i16))?;
        }
    }
    r.add_icon(&FRONTLIGHT, Cow::Borrowed("bulb"))?;
    r.add_icon(&WARMTH, Cow::Borrowed("temperature"))?;
    Ok(())
}
