//! The tables of the kernel's general, look, device and colour settings,
//! and how they are registered.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the core plugin, which shares its imports and setting extras"
)]
use super::*;

/// The language, night mode, speech and the edges for swipes.
static GENERAL: [SettingSpec; 10] = [
    SettingSpec::switch(
        settings::DEVELOPER.static_str(),
        "core.setting.developer",
        false,
        groups::SYSTEM,
    )
    .advanced()
    .description("core.setting.developer.help"),
    // The row that asks on the device (`developer_link.rs`): off at every
    // start, never stored; only where USB can carry it.
    SettingSpec::switch(
        crate::developer_link::SETTING,
        "core.setting.usb-link",
        false,
        groups::DEVELOPER,
    )
    .description("core.setting.usb-link.help"),
    SettingSpec::new(
        paperpaw_sdk::Id::new_static(settings::LANGUAGE.static_str()),
        "core.setting.language",
        SettingKind::Language,
        SettingValue::Text(Cow::Borrowed(paperpaw_i18n::FALLBACK_LANGUAGE)),
        groups::SYSTEM,
    )
    .description("core.setting.language.help"),
    SettingSpec::switch(
        settings::NIGHT_MODE.static_str(),
        "core.setting.night-mode",
        false,
        groups::READING_DISPLAY,
    )
    .description("core.setting.night-mode.help"),
    SettingSpec::switch(
        settings::NIGHT_PICTURES.static_str(),
        "core.setting.night-pictures",
        false,
        groups::READING_DISPLAY,
    )
    .description("core.setting.night-pictures.help"),
    SettingSpec::choice(
        settings::MONO_TEXT.static_str(),
        "core.setting.mono-text",
        settings::MONO_TEXT_LEVELS,
        settings::MONO_TEXT_LEVELS[1],
        groups::READING_DISPLAY,
    )
    .description("core.setting.mono-text.help"),
    SettingSpec::switch(
        settings::SPEAK_FOCUS.static_str(),
        "core.setting.speak-focus",
        false,
        groups::SYSTEM,
    )
    .description("core.setting.speak-focus.help"),
    SettingSpec::choice(
        settings::VOICE.static_str(),
        "core.setting.voice",
        &[settings::VOICE_SYSTEM],
        settings::VOICE_SYSTEM,
        groups::READING_ALOUD,
    )
    .description("core.setting.voice.help"),
    SettingSpec::number(
        settings::EDGE_ENDS.static_str(),
        "core.setting.edge-ends",
        0,
        25,
        10,
        groups::CONTROLS,
    )
    .with_extra(&MM)
    .advanced()
    .description("core.setting.edge.help"),
    SettingSpec::number(
        settings::EDGE_SIDES.static_str(),
        "core.setting.edge-sides",
        0,
        25,
        5,
        groups::CONTROLS,
    )
    .with_extra(&MM)
    .advanced()
    .description("core.setting.edge.help"),
];

/// The library folder, chosen with the folder chooser: the reader's own,
/// kept when the settings are reset.
static FOLDER_EDITOR: SettingExtra = SettingExtra::new().editor_slot(slots::CHOOSE_FOLDER);
static LIBRARY: [SettingSpec; 1] = [SettingSpec::text(
    settings::LIBRARY_FOLDER.static_str(),
    "core.setting.library-folder",
    "books",
    groups::LIBRARY,
)
.with_extra(&FOLDER_EDITOR)
.kept_on_reset()
.description("core.setting.library-folder.help")];

pub(super) fn register_general_settings(r: &mut Staging) -> Result<(), RegisterError> {
    r.add_settings(&GENERAL)?;
    r.add_settings(&LIBRARY)?;
    r.set_menu_need(
        Id::new_static(crate::developer_link::SETTING),
        paperpaw_sdk::MenuNeed::Has(paperpaw_sdk::Capability::USB_LINK),
    )?;
    r.set_menu_need(settings::MONO_TEXT, paperpaw_sdk::MenuNeed::BlackAndWhite)?;
    r.set_menu_need(settings::SPEAK_FOCUS, paperpaw_sdk::MenuNeed::Speech)?;
    r.set_menu_need(settings::VOICE, paperpaw_sdk::MenuNeed::Speech)?;
    Ok(())
}

/// How the interface looks: its size and contrast for everyone; with
/// advanced settings its header, corners, highlight, lines, groups,
/// titles and room. The defaults are PaperPaw's look (soft corners,
/// pills, plain headers, faint lines); themes (data plugins) set others.
static LOOK: [SettingSpec; 14] = [
    SettingSpec::number(
        settings::UI_SIZE.static_str(),
        "core.setting.ui-size",
        7,
        24,
        8,
        groups::LOOK,
    )
    .with_extra(&PT_SLIDER)
    .description("core.setting.ui-size.help"),
    SettingSpec::choice(
        settings::UI_CONTRAST.static_str(),
        "core.setting.ui-contrast",
        &[settings::CONTRAST_NORMAL, settings::CONTRAST_STRONG],
        settings::CONTRAST_NORMAL,
        groups::LOOK,
    )
    .description("core.setting.ui-contrast.help"),
    SettingSpec::choice(
        settings::UI_HEADER.static_str(),
        "core.setting.ui-header",
        &[
            settings::HEADER_PLAIN,
            settings::HEADER_LIGHT,
            settings::HEADER_DARK,
        ],
        settings::HEADER_PLAIN,
        groups::LOOK,
    )
    .advanced()
    .description("core.setting.ui-header.help"),
    SettingSpec::choice(
        settings::UI_CORNERS.static_str(),
        "core.setting.ui-corners",
        settings::CORNERS,
        settings::CORNERS[3],
        groups::LOOK,
    )
    .advanced()
    .description("core.setting.ui-corners.help"),
    SettingSpec::choice(
        settings::UI_FOCUS.static_str(),
        "core.setting.ui-focus",
        &[
            settings::FOCUS_FILL,
            settings::FOCUS_RING,
            settings::FOCUS_SOFT,
        ],
        settings::FOCUS_FILL,
        groups::LOOK,
    )
    .advanced()
    .description("core.setting.ui-focus.help"),
    SettingSpec::choice(
        settings::UI_SEPARATORS.static_str(),
        "core.setting.ui-separators",
        &[
            settings::SEPARATORS_INSET,
            settings::SEPARATORS_LINE,
            settings::SEPARATORS_NONE,
        ],
        settings::SEPARATORS_INSET,
        groups::LOOK,
    )
    .advanced()
    .description("core.setting.ui-separators.help"),
    SettingSpec::choice(
        settings::UI_GROUPS.static_str(),
        "core.setting.ui-groups",
        &[settings::GROUPS_PLAIN, settings::GROUPS_CARDS],
        settings::GROUPS_PLAIN,
        groups::LOOK,
    )
    .advanced()
    .description("core.setting.ui-groups.help"),
    SettingSpec::number(
        settings::UI_TITLE.static_str(),
        "core.setting.ui-title",
        100,
        200,
        115,
        groups::LOOK,
    )
    .with_extra(&PERCENT_5_SLIDER)
    .advanced()
    .description("core.setting.ui-title.help"),
    SettingSpec::number(
        settings::UI_MARGIN.static_str(),
        "core.setting.ui-margin",
        0,
        15,
        4,
        groups::LOOK,
    )
    .with_extra(&MM_SLIDER)
    .advanced()
    .description("core.setting.ui-margin.help"),
    SettingSpec::number(
        settings::UI_ROW.static_str(),
        "core.setting.ui-row",
        120,
        300,
        160,
        groups::LOOK,
    )
    .with_extra(&PERCENT_10_SLIDER)
    .advanced()
    .description("core.setting.ui-row.help"),
    SettingSpec::number(
        settings::UI_LINES.static_str(),
        "core.setting.ui-lines",
        1,
        10,
        3,
        groups::LOOK,
    )
    .with_extra(&TENTH_MM)
    .advanced()
    .description("core.setting.ui-lines.help"),
    // A picture in place of PaperPaw's logo: for data plugins, which set
    // it in their settings.ini, and those who type a path.
    SettingSpec::text(
        crate::kit::logo::LOGO.static_str(),
        "core.setting.logo",
        "",
        Id::new_static("sleep.group"),
    )
    .advanced()
    .description("core.setting.logo.help"),
    // The interface's grays by what they are for: for themes, which set
    // it in their settings.ini.
    SettingSpec::text(
        settings::UI_PALETTE.static_str(),
        "core.setting.ui-palette",
        "",
        groups::LOOK,
    )
    .advanced()
    .description("core.setting.ui-palette.help"),
    // How the edges of the interface's letters are inked: for themes.
    SettingSpec::text(
        settings::UI_TEXT_CURVE.static_str(),
        "core.setting.ui-text-curve",
        "",
        groups::LOOK,
    )
    .advanced()
    .description("core.setting.ui-text-curve.help"),
];

pub(super) fn register_look_settings(r: &mut Staging) -> Result<(), RegisterError> {
    r.add_settings(&LOOK)?;
    r.add_settings(&look_more::LOOK_MORE)?;
    r.add_settings(&widget_settings::ALL_WIDGETS)
}

/// Buttons, the panel's refresh, sleep, rotation, notices and long
/// presses.
static DEVICE: [SettingSpec; 17] = [
    SettingSpec::choice(
        settings::KEYMAP_PRESET.static_str(),
        "core.setting.keymap-preset",
        &[keymap_presets::STANDARD_NAME],
        keymap_presets::STANDARD_NAME,
        groups::CONTROLS,
    )
    .description("core.setting.keymap-preset.help"),
    SettingSpec::number(
        settings::FULL_REFRESH_INTERVAL.static_str(),
        "core.setting.full-refresh-interval",
        0,
        1000,
        6,
        groups::READING_DISPLAY,
    )
    .with_extra(&PAGES)
    .scoped()
    .description("core.setting.full-refresh-interval.help"),
    SettingSpec::switch(
        settings::FLASH_SCREENS.static_str(),
        "core.setting.flash-screens",
        true,
        groups::DISPLAY,
    )
    .advanced()
    .description("core.setting.flash-screens.help"),
    SettingSpec::number(
        crate::refresh::UI_FULL_REFRESH_INTERVAL,
        "core.setting.ui-full-refresh-interval",
        0,
        1000,
        0,
        groups::DISPLAY,
    )
    .advanced()
    .description("core.setting.ui-full-refresh-interval.help"),
    SettingSpec::switch(
        settings::PICTURES_IN_GRAYS.static_str(),
        "core.setting.pictures-in-grays",
        true,
        groups::READING_DISPLAY,
    )
    .description("core.setting.pictures-in-grays.help"),
    SettingSpec::number(
        settings::SLEEP_AFTER.static_str(),
        "core.setting.sleep-after",
        0,
        120,
        10,
        Id::new_static("power.group"),
    )
    .with_extra(&MINUTES_5)
    .description("core.setting.sleep-after.help"),
    SettingSpec::choice(
        settings::ROTATION.static_str(),
        "core.setting.rotation",
        settings::ROTATIONS,
        settings::ROTATIONS[0],
        groups::DISPLAY,
    )
    .scoped()
    .description("core.setting.rotation.help"),
    SettingSpec::switch(
        settings::BUTTON_HINTS.static_str(),
        "core.setting.button-hints",
        true,
        groups::CONTROLS,
    )
    .description("core.setting.button-hints.help"),
    SettingSpec::switch(
        settings::SWAP_PAGE_BUTTONS.static_str(),
        "core.setting.swap-page-buttons",
        false,
        groups::CONTROLS,
    )
    .description("core.setting.swap-page-buttons.help"),
    SettingSpec::switch(
        settings::ARROWS_STAY_ON_PAGE.static_str(),
        "core.setting.arrows-stay-on-page",
        false,
        groups::CONTROLS,
    )
    .description("core.setting.arrows-stay-on-page.help"),
    SettingSpec::number(
        settings::NOTICE_SECONDS.static_str(),
        "core.setting.notice-seconds",
        1,
        60,
        4,
        groups::LOOK,
    )
    .advanced()
    .with_extra(&SECONDS)
    .description("core.setting.notice-seconds.help"),
    SettingSpec::number(
        settings::LONG_PRESS_MS.static_str(),
        "core.setting.long-press-ms",
        200,
        3000,
        500,
        groups::CONTROLS,
    )
    .advanced()
    .with_extra(&MS)
    .description("core.setting.long-press-ms.help"),
    SettingSpec::number(
        settings::DOUBLE_TAP_MS.static_str(),
        "core.setting.double-tap-ms",
        150,
        1000,
        300,
        groups::CONTROLS,
    )
    .advanced()
    .with_extra(&MS)
    .description("core.setting.double-tap-ms.help"),
    SettingSpec::number(
        settings::TAP_SLOP.static_str(),
        "core.setting.tap-slop",
        1,
        20,
        3,
        groups::CONTROLS,
    )
    .advanced()
    .with_extra(&MM)
    .description("core.setting.tap-slop.help"),
    SettingSpec::number(
        settings::SWIPE_DISTANCE.static_str(),
        "core.setting.swipe-distance",
        2,
        50,
        8,
        groups::CONTROLS,
    )
    .advanced()
    .with_extra(&MM)
    .description("core.setting.swipe-distance.help"),
    SettingSpec::number(
        settings::FRONTLIGHT.static_str(),
        "core.setting.frontlight",
        0,
        100,
        0,
        groups::DISPLAY,
    )
    .with_extra(&PERCENT_5)
    .description("core.setting.frontlight.help"),
    SettingSpec::number(
        settings::FRONTLIGHT_WARMTH.static_str(),
        "core.setting.frontlight-warmth",
        0,
        100,
        50,
        groups::DISPLAY,
    )
    .with_extra(&PERCENT_5)
    .description("core.setting.frontlight-warmth.help"),
];

pub(super) fn register_device_settings(r: &mut Staging) -> Result<(), RegisterError> {
    r.add_settings(&DEVICE)?;
    r.set_menu_need(settings::SLEEP_AFTER, paperpaw_sdk::MenuNeed::Sleep)?;
    r.set_menu_need(settings::ROTATION, paperpaw_sdk::MenuNeed::Rotation)?;
    r.set_menu_need(settings::BUTTON_HINTS, paperpaw_sdk::MenuNeed::ButtonRow)?;
    r.set_menu_need(
        settings::PICTURES_IN_GRAYS,
        paperpaw_sdk::MenuNeed::BlackAndWhite,
    )?;
    let touch = [
        settings::EDGE_ENDS,
        settings::EDGE_SIDES,
        settings::TAP_SLOP,
        settings::SWIPE_DISTANCE,
    ];
    for setting in touch {
        r.set_menu_need(setting, paperpaw_sdk::MenuNeed::Touch)?;
    }
    for light in [settings::FRONTLIGHT, settings::FRONTLIGHT_WARMTH] {
        r.set_menu_need(light, paperpaw_sdk::MenuNeed::Frontlight)?;
    }
    Ok(())
}

/// The colours of text and paper, each with its own colour code.
static COLOR_SETTINGS: [SettingSpec; 4] = [
    SettingSpec::choice(
        settings::TEXT_COLOR.static_str(),
        "core.setting.text-color",
        settings::TEXT_COLORS,
        settings::TEXT_COLORS[0],
        groups::READING_TEXT,
    )
    .with_extra(&COLORS)
    .description("core.setting.colors.help"),
    SettingSpec::text(
        settings::TEXT_COLOR_OWN.static_str(),
        "core.setting.text-color-own",
        "#5b4636",
        groups::READING_TEXT,
    )
    .with_extra(&COLORS)
    .description("core.setting.color-own.help"),
    SettingSpec::choice(
        settings::PAPER_COLOR.static_str(),
        "core.setting.paper-color",
        settings::PAPER_COLORS,
        settings::PAPER_COLORS[0],
        groups::READING_TEXT,
    )
    .with_extra(&COLORS)
    .description("core.setting.colors.help"),
    SettingSpec::text(
        settings::PAPER_COLOR_OWN.static_str(),
        "core.setting.paper-color-own",
        "#f4ecd8",
        groups::READING_TEXT,
    )
    .with_extra(&COLORS)
    .description("core.setting.color-own.help"),
];

pub(super) fn register_colors(r: &mut Staging) -> Result<(), RegisterError> {
    r.add_settings(&COLOR_SETTINGS)?;
    for id in [
        settings::TEXT_COLOR,
        settings::TEXT_COLOR_OWN,
        settings::PAPER_COLOR,
        settings::PAPER_COLOR_OWN,
    ] {
        r.set_menu_need(id, paperpaw_sdk::MenuNeed::Colors)?;
    }
    Ok(())
}
