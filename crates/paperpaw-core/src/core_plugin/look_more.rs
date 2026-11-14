//! The look settings that shape the shared parts: a hard shadow, the frame
//! of sheets, dialogs and chips, the spacing and how buttons not chosen look.
//! Each leaves the look as it is until a theme sets it (#114).

use paperpaw_sdk::SettingSpec;
use paperpaw_sdk::builtin::{groups, settings};
use paperpaw_sdk::widgets;

/// After the look settings of the core plugin (`LOOK`), in the same group.
pub(super) static LOOK_MORE: [SettingSpec; 4] = [
    SettingSpec::choice(
        settings::UI_SHADOW.static_str(),
        "core.setting.ui-shadow",
        widgets::SHADOWS,
        widgets::SHADOWS[0],
        groups::LOOK,
    )
    .advanced()
    .description("core.setting.ui-shadow.help"),
    SettingSpec::choice(
        settings::UI_FRAME.static_str(),
        "core.setting.ui-frame",
        widgets::ALL_FRAMES,
        settings::FRAME_OWN,
        groups::LOOK,
    )
    .advanced()
    .description("core.setting.ui-frame.help"),
    SettingSpec::choice(
        settings::UI_DENSITY.static_str(),
        "core.setting.ui-density",
        &[
            settings::DENSITY_COMPACT,
            settings::DENSITY_REGULAR,
            settings::DENSITY_ROOMY,
        ],
        settings::DENSITY_REGULAR,
        groups::LOOK,
    )
    .advanced()
    .description("core.setting.ui-density.help"),
    SettingSpec::choice(
        settings::UI_BUTTON.static_str(),
        "core.setting.ui-button",
        &[
            settings::BUTTON_OUTLINED,
            settings::BUTTON_FILLED,
            settings::BUTTON_FLAT,
        ],
        settings::BUTTON_OUTLINED,
        groups::LOOK,
    )
    .advanced()
    .description("core.setting.ui-button.help"),
];
