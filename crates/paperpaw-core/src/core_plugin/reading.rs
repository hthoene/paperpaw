//! The kernel's reading settings: the reading font, the page's text
//! (justified, hyphenated, indented, kept in lines) and whether a book's
//! settings are its own.

use paperpaw_sdk::builtin::{group_sections, groups, settings};
use paperpaw_sdk::{Id, RegisterError, Registrar, SettingExtra, SettingSpec};

use super::{FONT, FONT_PT, IN_APPS, IN_PAGE, LINES, MARGINS, MARGINS_ENDS, WEIGHT, WORDS};

static LINES_KEPT: SettingExtra = SettingExtra::new()
    .unit("core.unit.lines")
    .section(group_sections::PAGE);
static LETTERS_KEPT: SettingExtra = SettingExtra::new()
    .unit("core.unit.letters")
    .section(group_sections::PAGE);

/// The reading font, and whether a book's settings are its own. The
/// status lines' font: the menus', the book's or one of its own (the
/// families are offered once all fonts are known).
static READING: [SettingSpec; 21] = [
    SettingSpec::choice(
        settings::STATUS_FONT.static_str(),
        "core.setting.status-font",
        &[settings::STATUS_FONT_UI, settings::STATUS_FONT_BOOK],
        settings::STATUS_FONT_UI,
        groups::STATUS_LINES,
    )
    .description("core.setting.status-font.help"),
    SettingSpec::choice(
        settings::READING_FONT.static_str(),
        "core.setting.reading-font",
        &[settings::DEFAULT_FONT],
        settings::DEFAULT_FONT,
        groups::READING_TEXT,
    )
    .with_extra(&FONT)
    .scoped()
    .description("core.setting.reading-font.help"),
    SettingSpec::number(
        settings::FONT_SIZE.static_str(),
        "core.setting.font-size",
        crate::plugins::FONT_SIZES.0,
        crate::plugins::FONT_SIZES.1,
        11,
        groups::READING_TEXT,
    )
    .with_extra(&FONT_PT)
    .scoped()
    .description("core.setting.font-size.help"),
    SettingSpec::number(
        settings::LINE_SPACING.static_str(),
        "core.setting.line-spacing",
        80,
        250,
        135,
        groups::READING_LAYOUT,
    )
    .with_extra(&LINES)
    .scoped()
    .description("core.setting.line-spacing.help"),
    SettingSpec::number(
        settings::FONT_WEIGHT.static_str(),
        "core.setting.font-weight",
        0,
        4,
        0,
        groups::READING_TEXT,
    )
    .with_extra(&WEIGHT)
    .scoped()
    .description("core.setting.font-weight.help"),
    SettingSpec::number(
        settings::WORD_SPACING.static_str(),
        "core.setting.word-spacing",
        50,
        200,
        100,
        groups::READING_TEXT,
    )
    .with_extra(&WORDS)
    .scoped()
    .description("core.setting.word-spacing.help"),
    SettingSpec::number(
        settings::MARGIN.static_str(),
        "core.setting.margin",
        0,
        30,
        4,
        groups::READING_LAYOUT,
    )
    .with_extra(&MARGINS)
    .scoped()
    .description("core.setting.margin.help"),
    SettingSpec::number(
        settings::MARGIN_ENDS.static_str(),
        "core.setting.margin-ends",
        0,
        30,
        4,
        groups::READING_LAYOUT,
    )
    .with_extra(&MARGINS_ENDS)
    .advanced()
    .scoped()
    .description("core.setting.margin-ends.help"),
    SettingSpec::switch(
        settings::JUSTIFY.static_str(),
        "core.setting.justify",
        true,
        groups::READING_LAYOUT,
    )
    .scoped()
    .with_extra(&IN_PAGE)
    .description("core.setting.justify.help"),
    SettingSpec::switch(
        settings::HYPHENATE.static_str(),
        "core.setting.hyphenate",
        true,
        groups::READING_HYPHENATION,
    )
    .scoped()
    .with_extra(&IN_PAGE)
    .description("core.setting.hyphenate.help"),
    SettingSpec::number(
        settings::HYPHEN_MIN_LEFT.static_str(),
        "core.setting.hyphen-min-left",
        0,
        5,
        0,
        groups::READING_HYPHENATION,
    )
    .advanced()
    .scoped()
    .with_extra(&LETTERS_KEPT)
    .description("core.setting.hyphen-min-left.help"),
    SettingSpec::number(
        settings::HYPHEN_MIN_RIGHT.static_str(),
        "core.setting.hyphen-min-right",
        0,
        5,
        0,
        groups::READING_HYPHENATION,
    )
    .advanced()
    .scoped()
    .with_extra(&LETTERS_KEPT)
    .description("core.setting.hyphen-min-right.help"),
    SettingSpec::switch(
        settings::PUBLISHER_STYLES.static_str(),
        "core.setting.publisher-styles",
        true,
        groups::READING_BOOK_STYLES,
    )
    .scoped()
    .with_extra(&IN_PAGE)
    .description("core.setting.publisher-styles.help"),
    SettingSpec::switch(
        settings::INDENT.static_str(),
        "core.setting.indent",
        true,
        groups::READING_LAYOUT,
    )
    .scoped()
    .with_extra(&IN_PAGE)
    .description("core.setting.indent.help"),
    SettingSpec::switch(
        settings::PARAGRAPH_SPACING.static_str(),
        "core.setting.paragraph-spacing",
        false,
        groups::READING_LAYOUT,
    )
    .scoped()
    .with_extra(&IN_PAGE)
    .description("core.setting.paragraph-spacing.help"),
    SettingSpec::number(
        settings::WIDOWS.static_str(),
        "core.setting.widows",
        1,
        4,
        1,
        groups::READING_LAYOUT,
    )
    .advanced()
    .scoped()
    .with_extra(&LINES_KEPT)
    .description("core.setting.widows.help"),
    SettingSpec::number(
        settings::ORPHANS.static_str(),
        "core.setting.orphans",
        1,
        4,
        1,
        groups::READING_LAYOUT,
    )
    .advanced()
    .scoped()
    .with_extra(&LINES_KEPT)
    .description("core.setting.orphans.help"),
    SettingSpec::choice(
        settings::READING_APP.static_str(),
        "core.setting.reading-app",
        &[settings::READING_APP_DEFAULT],
        settings::READING_APP_DEFAULT,
        groups::READING_BEHAVIOUR,
    )
    .with_extra(&IN_APPS)
    .description("core.setting.reading-app.help"),
    SettingSpec::choice(
        settings::FOOTNOTES.static_str(),
        "core.setting.footnotes",
        &[settings::FOOTNOTES_POPUP, settings::FOOTNOTES_JUMP],
        settings::FOOTNOTES_POPUP,
        groups::READING_BEHAVIOUR,
    )
    .advanced()
    .description("core.setting.footnotes.help"),
    SettingSpec::choice(
        settings::PAGE_FRAME.static_str(),
        "core.setting.page-frame",
        &[settings::PAGE_FRAME_SINGLE],
        settings::PAGE_FRAME_SINGLE,
        groups::READING_LAYOUT,
    )
    .scoped()
    .with_extra(&IN_PAGE)
    .description("core.setting.page-frame.help"),
    SettingSpec::switch(
        settings::SCOPE_ONLY.static_str(),
        "core.setting.scope-only",
        false,
        groups::READING,
    )
    .scoped()
    .description("core.setting.scope-only.help"),
];

/// Shown while the book's own styles are followed at all.
static BOOK_STYLE: SettingExtra = SettingExtra::new().section(group_sections::PAGE).shown_if(
    Id::new_static(settings::PUBLISHER_STYLES.static_str()),
    "true",
);
static LETTERS: SettingExtra = SettingExtra::new()
    .unit("core.unit.percent")
    .section(group_sections::FONT)
    .slider();
/// Kerning and ligatures, beside letter spacing.
static FONT_PAIRS: SettingExtra = SettingExtra::new().section(group_sections::FONT);
static INDENT_SIZE: SettingExtra = SettingExtra::new()
    .unit("core.unit.percent")
    .step(25)
    .section(group_sections::PAGE)
    .shown_if(Id::new_static(settings::INDENT.static_str()), "true");
static PARAGRAPH_SPACE: SettingExtra = SettingExtra::new()
    .unit("core.unit.percent")
    .step(10)
    .section(group_sections::PAGE)
    .shown_if(
        Id::new_static(settings::PARAGRAPH_SPACING.static_str()),
        "true",
    );

/// A switch for one of the book's own styles.
const fn book_style(id: &'static str, title: &'static str, on: bool) -> SettingSpec {
    SettingSpec::switch(id, title, on, groups::READING_BOOK_STYLES)
        .scoped()
        .with_extra(&BOOK_STYLE)
}

/// Which of the book's own styles are followed, and the finer measures
/// of the type: letter spacing, kerning and ligatures, the indent's and
/// the paragraph space's size. Their defaults keep pages as they were,
/// but kerning and ligatures are on, as other readers set them.
static TYPE: [SettingSpec; 14] = [
    SettingSpec::choice(
        settings::LINE_BREAKING.static_str(),
        "core.setting.line-breaking",
        &[
            settings::LINE_BREAKING_BALANCED,
            settings::LINE_BREAKING_FAST,
        ],
        settings::LINE_BREAKING_BALANCED,
        groups::READING_LAYOUT,
    )
    .with_extra(&IN_PAGE)
    .advanced()
    .scoped()
    .description("core.setting.line-breaking.help"),
    SettingSpec::choice(
        settings::KOREAN_BREAKS.static_str(),
        "core.setting.korean-breaks",
        &[
            settings::KOREAN_BREAKS_SYLLABLE,
            settings::KOREAN_BREAKS_WORD,
        ],
        settings::KOREAN_BREAKS_SYLLABLE,
        groups::READING_LAYOUT,
    )
    .with_extra(&IN_PAGE)
    .advanced()
    .scoped()
    .description("core.setting.korean-breaks.help"),
    book_style(
        settings::BOOK_ALIGN.static_str(),
        "core.setting.book-align",
        true,
    )
    .description("core.setting.book-align.help"),
    book_style(
        settings::BOOK_INDENT.static_str(),
        "core.setting.book-indent",
        true,
    )
    .description("core.setting.book-indent.help"),
    book_style(
        settings::BOOK_SIZES.static_str(),
        "core.setting.book-sizes",
        true,
    )
    .description("core.setting.book-sizes.help"),
    book_style(
        settings::BOOK_SPACING.static_str(),
        "core.setting.book-spacing",
        true,
    )
    .description("core.setting.book-spacing.help"),
    book_style(
        settings::BOOK_FONTS.static_str(),
        "core.setting.book-fonts",
        true,
    )
    .description("core.setting.book-fonts.help"),
    book_style(
        settings::BOOK_MARGINS.static_str(),
        "core.setting.book-margins",
        true,
    )
    .description("core.setting.book-margins.help"),
    book_style(
        settings::BOOK_LINE_HEIGHT.static_str(),
        "core.setting.book-line-height",
        false,
    )
    .description("core.setting.book-line-height.help"),
    SettingSpec::number(
        settings::LETTER_SPACING.static_str(),
        "core.setting.letter-spacing",
        -5,
        20,
        0,
        groups::READING_TEXT,
    )
    .with_extra(&LETTERS)
    .advanced()
    .scoped()
    .description("core.setting.letter-spacing.help"),
    SettingSpec::switch(
        settings::KERNING.static_str(),
        "core.setting.kerning",
        true,
        groups::READING_TEXT,
    )
    .with_extra(&FONT_PAIRS)
    .advanced()
    .scoped()
    .description("core.setting.kerning.help"),
    SettingSpec::switch(
        settings::LIGATURES.static_str(),
        "core.setting.ligatures",
        true,
        groups::READING_TEXT,
    )
    .with_extra(&FONT_PAIRS)
    .advanced()
    .scoped()
    .description("core.setting.ligatures.help"),
    SettingSpec::number(
        settings::INDENT_SIZE.static_str(),
        "core.setting.indent-size",
        50,
        400,
        150,
        groups::READING_LAYOUT,
    )
    .with_extra(&INDENT_SIZE)
    .advanced()
    .scoped()
    .description("core.setting.indent-size.help"),
    SettingSpec::number(
        settings::PARAGRAPH_SPACE.static_str(),
        "core.setting.paragraph-space",
        20,
        150,
        60,
        groups::READING_LAYOUT,
    )
    .with_extra(&PARAGRAPH_SPACE)
    .advanced()
    .scoped()
    .description("core.setting.paragraph-space.help"),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    // The parts of the reading settings, each a page of its own.
    for (id, title, order) in [
        (groups::READING_TEXT, "core.group.reading-text", 1),
        (groups::READING_LAYOUT, "core.group.reading-layout", 2),
        (
            groups::READING_HYPHENATION,
            "core.group.reading-hyphenation",
            3,
        ),
        (
            groups::READING_BOOK_STYLES,
            "core.group.reading-book-styles",
            4,
        ),
        (groups::READING_DISPLAY, "core.group.reading-display", 5),
        (groups::STATUS_LINES, "core.group.status", 6),
        (groups::MARKS, "core.group.marks", 7),
        (groups::READING_ALOUD, "core.group.reading-aloud", 8),
        (groups::READING_AIDS, "core.group.reading-aids", 9),
        (
            groups::READING_BEHAVIOUR,
            "core.group.reading-behaviour",
            10,
        ),
    ] {
        r.add_setting_group(
            paperpaw_sdk::SettingGroupSpec::new(id, title)
                .order(order)
                .within(groups::READING),
        )?;
    }
    r.add_settings(&READING)?;
    r.add_settings(&TYPE)?;
    r.add_settings(&super::text_look::SETTINGS)
}
