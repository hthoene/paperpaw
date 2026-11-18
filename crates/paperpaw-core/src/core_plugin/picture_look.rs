//! How pictures look: contrast, gamma and dither (#82), one setting each
//! for every picture the host draws (the reader's pages and covers, comics,
//! sleep screens). The kit turns them into one 256-entry table
//! ([`crate::kit::ui::picture_look`]).

use crate::kit::ui::picture_look as look;
use paperpaw_sdk::SettingSpec;
use paperpaw_sdk::builtin::groups;

pub(super) static SETTINGS: [SettingSpec; 3] = [
    SettingSpec::choice(
        look::CONTRAST,
        "core.setting.picture-contrast",
        look::CONTRAST_LEVELS,
        look::CONTRAST_LEVELS[1],
        groups::READING_DISPLAY,
    )
    .description("core.setting.picture-contrast.help"),
    SettingSpec::choice(
        look::GAMMA,
        "core.setting.picture-gamma",
        look::GAMMA_LEVELS,
        look::GAMMA_LEVELS[2],
        groups::READING_DISPLAY,
    )
    .description("core.setting.picture-gamma.help"),
    SettingSpec::choice(
        look::DITHER,
        "core.setting.picture-dither",
        look::DITHER_LEVELS,
        look::DITHER_LEVELS[0],
        groups::READING_DISPLAY,
    )
    .description("core.setting.picture-dither.help"),
];
