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
/// How long the reader sleeps before it switches itself off (the choices
/// are in `paperpaw_x4::sleeping`).
#[cfg(feature = "esp32c3")]
pub const POWER_OFF_AFTER: Setting<paperpaw_sdk::setting::Text> =
    Setting::new("x4.power-off-after");
/// The choices of [`BUTTON_RESPONSE`].
pub const RESPONSE_NORMAL: &str = "x4.button-response.normal";
pub const RESPONSE_FASTER: &str = "x4.button-response.faster";
pub const RESPONSE_SAVING: &str = "x4.button-response.saving";
/// How the X4 shows fast changes (menus, a focus moved): the panel's own
/// waveform, or a quicker one with more or less ghosting (see `firmware`).
#[cfg(feature = "x4")]
pub const FAST_REFRESH: Setting<paperpaw_sdk::setting::Text> = Setting::new("x4.fast-refresh");
/// Whether the panel's controller is an SSD1677: only its waveforms can be
/// made here, so only then the settings of them are offered. Set at start,
/// once the controller is known (`main`).
#[cfg(feature = "x4")]
static SSD1677: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Says whether the panel's controller is an SSD1677.
#[cfg(feature = "x4")]
pub fn set_ssd1677(yes: bool) {
    SSD1677.store(yes, core::sync::atomic::Ordering::Relaxed);
}

/// Whether the X4 Pro's controller is a UC8179, whose waveforms from the
/// registers its menus take (see `firmware`): then the settings of them are
/// offered. Set at start (`main`).
static UC8179: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Says whether the menus take the UC8179's waveform of the registers.
pub fn set_uc8179(yes: bool) {
    UC8179.store(yes, core::sync::atomic::Ordering::Relaxed);
}

/// Whether the menus take the UC8179's waveform of the registers.
pub fn uc8179() -> bool {
    UC8179.load(core::sync::atomic::Ordering::Relaxed)
}

/// How the X4 Pro shows fast changes (menus, a focus moved): the panel's own
/// waveform, or a quicker one of the registers: more or less frames and the
/// pixels that stay held to their colour, which keeps the ghosts down.
pub const UC_REFRESH: Setting<paperpaw_sdk::setting::Text> = Setting::new("x4.uc8179.fast-refresh");
/// The choices of [`UC_REFRESH`], the slowest and cleanest first.
pub const UC_PANEL: &str = "x4.uc8179.fast-refresh.panel";
pub const UC_CLEAN: &str = "x4.uc8179.fast-refresh.clean";
pub const UC_BALANCED: &str = "x4.uc8179.fast-refresh.balanced";
pub const UC_QUICK: &str = "x4.uc8179.fast-refresh.quick";
/// The frames, the held frames, the kick and the frame rate (Hz) of a fast
/// refresh, used where [`UC_REFRESH`] is [`UC_CUSTOM`].
pub const UC_CUSTOM: &str = "x4.uc8179.fast-refresh.custom";
pub const UC_FRAMES: Setting<i64> = Setting::new("x4.uc8179.fast-frames");
pub const UC_HOLD: Setting<i64> = Setting::new("x4.uc8179.fast-hold");
pub const UC_KICK: Setting<i64> = Setting::new("x4.uc8179.fast-kick");
pub const UC_RATE: Setting<i64> = Setting::new("x4.uc8179.fast-rate");
/// The same for the fastest refresh (screens that ask for `fastest`, games).
pub const UC_MONO_FRAMES: Setting<i64> = Setting::new("x4.uc8179.mono-frames");
pub const UC_MONO_HOLD: Setting<i64> = Setting::new("x4.uc8179.mono-hold");
pub const UC_MONO_RATE: Setting<i64> = Setting::new("x4.uc8179.mono-rate");
/// How much of the picture goes to the controller for a fast refresh.
pub const UC_TRANSFER: Setting<paperpaw_sdk::setting::Text> = Setting::new("x4.uc8179.transfer");
pub const UC_TRANSFER_WHOLE: &str = "x4.uc8179.transfer.whole";
pub const UC_TRANSFER_CHANGED: &str = "x4.uc8179.transfer.changed";
pub const UC_TRANSFER_COPIED: &str = "x4.uc8179.transfer.copied";

/// The frames and the kick of a fast refresh, used where [`FAST_REFRESH`] is
/// [`FAST_CUSTOM`]; and of a black-and-white refresh, the one screens get that
/// ask for the fastest (games), always. A data plugin sets them in its
/// `settings.ini` or by an action (see `waveform::ordered` for how they are
/// kept in order).
#[cfg(feature = "x4")]
pub const FAST_FRAMES: Setting<i64> = Setting::new("x4.ssd1677.fast-frames");
#[cfg(feature = "x4")]
pub const FAST_KICK: Setting<i64> = Setting::new("x4.ssd1677.fast-kick");
#[cfg(feature = "x4")]
pub const MONO_FRAMES: Setting<i64> = Setting::new("x4.ssd1677.mono-frames");
#[cfg(feature = "x4")]
pub const MONO_KICK: Setting<i64> = Setting::new("x4.ssd1677.mono-kick");
/// The choices of [`FAST_REFRESH`], the slowest and cleanest first.
#[cfg(feature = "x4")]
pub const FAST_PANEL: &str = "x4.fast-refresh.panel";
#[cfg(feature = "x4")]
pub const FAST_CLEAN: &str = "x4.fast-refresh.clean";
#[cfg(feature = "x4")]
pub const FAST_BALANCED: &str = "x4.fast-refresh.balanced";
#[cfg(feature = "x4")]
pub const FAST_QUICK: &str = "x4.fast-refresh.quick";
/// The frames and the kick of [`FAST_FRAMES`] and [`FAST_KICK`].
#[cfg(feature = "x4")]
pub const FAST_CUSTOM: &str = "x4.fast-refresh.custom";
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

/// How long asleep before it switches off (next to *Sleep after*).
#[cfg(feature = "esp32c3")]
static SLEEP_SETTINGS: [SettingSpec; 1] = [SettingSpec::choice(
    POWER_OFF_AFTER.static_str(),
    "x4.power-off-after",
    &paperpaw_x4::sleeping::CHOICES,
    paperpaw_x4::sleeping::DEFAULT,
    Id::new_static("power.group"),
)
.description("x4.power-off-after.help")];

/// How fast changes are shown.
#[cfg(feature = "x4")]
static FAST_SETTINGS: [SettingSpec; 5] = [
    SettingSpec::choice(
        FAST_REFRESH.static_str(),
        "x4.fast-refresh",
        &[
            FAST_PANEL,
            FAST_CLEAN,
            FAST_BALANCED,
            FAST_QUICK,
            FAST_CUSTOM,
        ],
        FAST_PANEL,
        groups::DISPLAY,
    )
    .description("x4.fast-refresh.help"),
    SettingSpec::number(
        FAST_FRAMES.static_str(),
        "x4.ssd1677.fast-frames",
        1,
        30,
        6,
        groups::DEVELOPER,
    )
    .description("x4.ssd1677.fast-frames.help"),
    SettingSpec::number(
        FAST_KICK.static_str(),
        "x4.ssd1677.fast-kick",
        0,
        3,
        0,
        groups::DEVELOPER,
    )
    .description("x4.ssd1677.fast-kick.help"),
    SettingSpec::number(
        MONO_FRAMES.static_str(),
        "x4.ssd1677.mono-frames",
        1,
        30,
        1,
        groups::DEVELOPER,
    )
    .description("x4.ssd1677.mono-frames.help"),
    SettingSpec::number(
        MONO_KICK.static_str(),
        "x4.ssd1677.mono-kick",
        0,
        3,
        0,
        groups::DEVELOPER,
    )
    .description("x4.ssd1677.mono-kick.help"),
];

/// How the X4 Pro shows fast changes.
static UC_SETTINGS: [SettingSpec; 9] = {
    use paperpaw_sdk::SettingExtra;
    static BY_TEN: SettingExtra = SettingExtra::new().step(10);
    [
        SettingSpec::choice(
            UC_REFRESH.static_str(),
            "x4.uc8179.fast-refresh",
            &[UC_PANEL, UC_CLEAN, UC_BALANCED, UC_QUICK, UC_CUSTOM],
            UC_PANEL,
            groups::DISPLAY,
        )
        .description("x4.uc8179.fast-refresh.help"),
        SettingSpec::number(
            UC_FRAMES.static_str(),
            "x4.uc8179.fast-frames",
            1,
            30,
            7,
            groups::DEVELOPER,
        )
        .description("x4.uc8179.fast-frames.help"),
        SettingSpec::number(
            UC_HOLD.static_str(),
            "x4.uc8179.fast-hold",
            0,
            3,
            3,
            groups::DEVELOPER,
        )
        .description("x4.uc8179.fast-hold.help"),
        SettingSpec::number(
            UC_KICK.static_str(),
            "x4.uc8179.fast-kick",
            0,
            3,
            0,
            groups::DEVELOPER,
        )
        .description("x4.uc8179.fast-kick.help"),
        SettingSpec::number(
            UC_RATE.static_str(),
            "x4.uc8179.fast-rate",
            20,
            200,
            50,
            groups::DEVELOPER,
        )
        .with_extra(&BY_TEN)
        .description("x4.uc8179.fast-rate.help"),
        SettingSpec::choice(
            UC_TRANSFER.static_str(),
            "x4.uc8179.transfer",
            &[UC_TRANSFER_COPIED, UC_TRANSFER_CHANGED, UC_TRANSFER_WHOLE],
            UC_TRANSFER_COPIED,
            groups::DEVELOPER,
        )
        .description("x4.uc8179.transfer.help"),
        SettingSpec::number(
            UC_MONO_FRAMES.static_str(),
            "x4.uc8179.mono-frames",
            1,
            30,
            12,
            groups::DEVELOPER,
        )
        .description("x4.uc8179.mono-frames.help"),
        SettingSpec::number(
            UC_MONO_HOLD.static_str(),
            "x4.uc8179.mono-hold",
            0,
            3,
            0,
            groups::DEVELOPER,
        )
        .description("x4.uc8179.mono-hold.help"),
        SettingSpec::number(
            UC_MONO_RATE.static_str(),
            "x4.uc8179.mono-rate",
            20,
            200,
            200,
            groups::DEVELOPER,
        )
        .with_extra(&BY_TEN)
        .description("x4.uc8179.mono-rate.help"),
    ]
};

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
        #[cfg(feature = "esp32c3")]
        r.add_settings(&SLEEP_SETTINGS)?;
        #[cfg(feature = "x4")]
        if SSD1677.load(core::sync::atomic::Ordering::Relaxed) {
            r.add_settings(&FAST_SETTINGS)?;
        }
        if uc8179() {
            r.add_settings(&UC_SETTINGS)?;
        }
        #[cfg(feature = "x4-pro")]
        register_light(r)?;
        #[cfg(feature = "waveform-lab")]
        {
            r.add_settings(&crate::lab::SETTINGS)?;
            r.add_screens(&crate::lab::SCREENS)?;
            r.add_actions(&crate::lab::ACTIONS)?;
            r.add_menu_entries(&crate::lab::ENTRIES)?;
        }
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
