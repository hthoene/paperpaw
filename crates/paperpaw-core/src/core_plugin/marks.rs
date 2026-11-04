//! The roles of the marks every reader draws: highlights, links, a
//! search hit, the sentence read aloud (see `paperpaw_sdk::decorations`).

use paperpaw_sdk::decorations::{Decoration, LineKind, Tone};
use paperpaw_sdk::role_settings;
use paperpaw_sdk::{RegisterError, Registrar, SettingSpec};

pub(super) static HIGHLIGHT: [SettingSpec; 7] = role_settings!(
    "core.highlight",
    "core.role.highlight",
    Decoration::NONE.background(Tone::LightGray)
);
pub(super) static LINK: [SettingSpec; 7] = role_settings!(
    "core.link",
    "core.role.link",
    Decoration::NONE
        .line(LineKind::Solid)
        .line_tone(Tone::DarkGray)
        .thickness(1)
);
pub(super) static SEARCH_HIT: [SettingSpec; 7] = role_settings!(
    "core.search-hit",
    "core.role.search-hit",
    Decoration::NONE.line(LineKind::Solid).thickness(5)
);
pub(super) static ALOUD: [SettingSpec; 7] = role_settings!(
    "core.aloud",
    "core.role.aloud",
    Decoration::NONE.line(LineKind::Solid).thickness(5)
);

/// The marks' settings, each role's.
pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    for role in [&HIGHLIGHT, &LINK, &SEARCH_HIT, &ALOUD] {
        r.add_settings(role)?;
    }
    Ok(())
}
