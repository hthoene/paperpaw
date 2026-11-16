//! Pebble: a theme for touch screens, shipped switched off. Lists sit on
//! rounded cards, as groups in a phone's settings do, and the chosen row
//! is a light bar (a frame on panels of black and white alone). It only
//! sets defaults, as a theme's `settings.ini` would; what the user
//! chooses still wins.

use alloc::borrow::Cow;

use paperpaw_core::Items;
use paperpaw_core::sdk::{Id, Manifest, RegisterError, Version};

/// The theme.
#[derive(Debug)]
pub struct Pebble;

/// Its name and what it does, in each language.
const TEXTS: [&str; 2] = [
    include_str!("../i18n/pebble/en.ini"),
    include_str!("../i18n/pebble/de.ini"),
];

/// Its settings: the rest of PaperPaw's look stays as it is.
const DEFAULTS: [(&str, &str); 3] = [
    ("core.ui-groups", "core.groups.cards"),
    ("core.ui-focus", "core.focus.soft"),
    ("home.continue.frame", "core.frame.card"),
];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/pebble/en.txt")),
    ("de", include_str!("../how/pebble/de.txt")),
];

impl Pebble {
    /// What it is listed and registered under.
    pub fn manifest() -> Manifest {
        Manifest::new(Id::new_static("pebble"), "Pebble", Version::new(1, 0, 0))
            .description("A theme for touch screens: lists on rounded cards, a light highlight.")
            .how(&HOW)
            .uses(paperpaw_core::sdk::Uses::NOTHING)
    }

    /// Adds its texts and its defaults (`KernelBuilder::items_off`).
    pub fn items(r: &mut Items) -> Result<(), RegisterError> {
        for text in TEXTS {
            r.add_translations(Cow::Borrowed(text))?;
        }
        for (setting, value) in DEFAULTS {
            r.set_setting_default(Id::new_static(setting), Cow::Borrowed(value))?;
        }
        Ok(())
    }
}
