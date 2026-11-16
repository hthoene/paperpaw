//! The front light of profiles whose devices have one: its brightness
//! and warmth as settings set with sliders, in the reader's panel, the
//! quick panel and with the display's settings, registered as the
//! devices' shells register theirs. The emulated screen does not light
//! up.

use std::borrow::Cow;

use paperpaw_core::Items;
use paperpaw_core::sdk::builtin::{groups, menus};
use paperpaw_core::sdk::setting::Setting;
use paperpaw_core::sdk::{Id, Manifest, MenuEntry, RegisterError, SettingSpec, Version};

/// How bright the light is (0 off).
pub const FRONTLIGHT: Setting<i64> = Setting::new("emulator.frontlight");
/// How warm it is.
pub const WARMTH: Setting<i64> = Setting::new("emulator.warmth");

/// A device's front light, as its shell offers it.
#[derive(Debug, Clone, Copy)]
pub struct Light {
    /// The brightest level; 100 where levels are in percent.
    pub max: i64,
    /// The step of the light's `−` and `+`.
    pub step: i64,
    /// Levels in percent, as the X4 Pro has them.
    pub percent: bool,
    /// The step of its warmth, where that can be set too.
    pub warmth: Option<i64>,
}

const EN: &str = "[meta]
language = en
name = English
plural = one-other

[messages]
emulator.frontlight = Front light
emulator.frontlight.help = How bright the front light shines; 0 is off. The emulated screen stays dark.
emulator.warmth = Light colour
emulator.warmth.help = How warm (yellow) the light is, kinder to the eyes in the evening.
";

const DE: &str = "[meta]
language = de
name = Deutsch
plural = one-other

[messages]
emulator.frontlight = Beleuchtung
emulator.frontlight.help = Wie hell die Beleuchtung ist; 0 ist aus. Der nachgebildete Bildschirm leuchtet nicht.
emulator.warmth = Lichtfarbe
emulator.warmth.help = Wie warm (gelb) das Licht ist, angenehmer für die Augen am Abend.
";

/// What [`emulated`] registers under.
pub fn manifest() -> Manifest {
    Manifest::new(
        Id::new_static("emulator"),
        "Front light",
        Version::new(0, 1, 0),
    )
    .description("The emulated device's front light, set as on the device.")
    .uses(paperpaw_core::sdk::Uses::NOTHING)
}

/// Registers a profile's light (`KernelBuilder::items` with [`manifest`]).
pub fn emulated(light: Light) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        // The light's range and steps come from the device's profile, so
        // its extra is made now.
        let number = |setting: &Setting<i64>, title: &'static str, default: i64, step: i64| {
            let mut extra = paperpaw_core::sdk::SettingExtra::new().step(step).slider();
            if light.percent {
                extra.unit = Some("core.unit.percent".into());
            }
            let mut spec = SettingSpec::new(
                Id::from(setting),
                title,
                paperpaw_core::sdk::SettingKind::Int {
                    min: 0,
                    max: light.max,
                },
                paperpaw_core::sdk::SettingValue::Int(default),
                groups::DISPLAY,
            )
            .with_owned_extra(extra);
            spec.description = Some(format!("{title}.help").into());
            spec
        };
        r.add_setting(number(&FRONTLIGHT, "emulator.frontlight", 0, light.step))?;
        let mut items = vec![FRONTLIGHT];
        if let Some(step) = light.warmth {
            let default = light.max * 3 / 10;
            r.add_setting(number(&WARMTH, "emulator.warmth", default, step))?;
            items.push(WARMTH);
        }
        // First in the reader's panel, and in the quick panel, as on the
        // devices.
        for (n, item) in items.into_iter().enumerate() {
            for (menu, order) in [(menus::READER_PANEL, -10), (menus::QUICK, 20)] {
                r.add_menu_entry(
                    MenuEntry::new(menu, item.clone().into()).order(order + n as i16),
                )?;
            }
        }
        r.add_icon(&FRONTLIGHT, Cow::Borrowed("bulb"))?;
        r.add_icon(&WARMTH, Cow::Borrowed("temperature"))?;
        r.add_translations(Cow::Borrowed(EN))?;
        r.add_translations(Cow::Borrowed(DE))?;
        Ok(())
    }
}
