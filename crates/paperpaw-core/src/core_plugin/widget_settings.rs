//! The look of every widget whose own the reader did not choose (in
//! home's group of settings): its frame, colours and shape, the gap
//! between widgets (#206) and the font (#262).

use paperpaw_sdk::builtin::settings;
use paperpaw_sdk::widgets::{self, ALL_FRAMES, OWN_FRAME};
use paperpaw_sdk::{Id, SettingSpec};

use crate::font_refresh::widget_fonts;

const GROUP: Id = Id::new_static("home.group");

pub(super) static ALL_WIDGETS: [SettingSpec; 9] = [
    SettingSpec::choice(
        settings::WIDGET_FRAME.static_str(),
        "core.setting.widget-frame",
        ALL_FRAMES,
        OWN_FRAME,
        GROUP,
    )
    .advanced()
    .description("core.setting.widget-frame.help"),
    widgets::all_paper_spec(GROUP),
    widgets::all_ink_spec(GROUP),
    widgets::all_corners_spec(GROUP),
    widgets::all_frame_line_spec(GROUP),
    widgets::all_shadow_spec(GROUP),
    widgets::all_padding_spec(GROUP),
    widgets::all_gap_spec(GROUP),
    // The families are offered once all fonts are known.
    SettingSpec::choice(
        widget_fonts::GLOBAL,
        "core.setting.widget-font",
        &[widget_fonts::MENUS, widget_fonts::BOOK],
        widget_fonts::MENUS,
        GROUP,
    )
    .advanced()
    .description("core.setting.widget-font.help"),
];
