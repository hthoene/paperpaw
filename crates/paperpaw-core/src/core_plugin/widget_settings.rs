//! The look of every widget whose own the reader did not choose (in
//! home's group of settings): its frame, colours and shape, and the gap
//! between widgets (#206).

use paperpaw_sdk::builtin::settings;
use paperpaw_sdk::widgets::{self, ALL_FRAMES, OWN_FRAME};
use paperpaw_sdk::{Id, SettingSpec};

const GROUP: Id = Id::new_static("home.group");

pub(super) static ALL_WIDGETS: [SettingSpec; 8] = [
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
];
