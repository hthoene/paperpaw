//! The settings every widget shares (frame, paper, ink, corners, line, shadow,
//! padding, size, alignment) and the few a widget adds (a switch for a part
//! it shows, a choice, a date format), as the declarations a widget plugin
//! submits: one [`Opt`] per setting, declared statically, never at run time.
//! A plugin's build script and its tests read the same list.
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EditorDecl, ExtraDecl, Kind, SpecDecl,
};

/// The slot that chooses a folder for a setting.
const CHOOSE_FOLDER: &str = "core.choose-folder";

pub const FRAMES: &[&str] = &[
    "core.frame.none",
    "core.frame.line",
    "core.frame.card",
    "core.frame.dark",
];
pub const SIZES: &[&str] = &[
    "core.size.small",
    "core.size.medium",
    "core.size.large",
    "core.size.fit",
];
pub const PADDINGS: &[&str] = &[
    "core.padding.auto",
    "core.padding.none",
    "core.padding.small",
    "core.padding.normal",
    "core.padding.large",
];
pub const ALIGNMENTS: &[&str] = &["core.align.left", "core.align.center", "core.align.right"];
pub const PAPERS: &[&str] = &[
    "core.plate.normal",
    "core.plate.inverted",
    "core.plate.light-gray",
    "core.plate.dark-gray",
    "core.plate.none",
];
pub const INKS: &[&str] = &["core.ink.auto", "core.tone.black", "core.tone.white"];
pub const CORNERS: &[&str] = &[
    "core.corners.auto",
    "core.corners.square",
    "core.corners.small",
    "core.corners.round",
    "core.corners.soft",
];
pub const LINES: &[&str] = &[
    "core.lines.auto",
    "core.lines.thin",
    "core.lines.medium",
    "core.lines.thick",
];
pub const SHADOWS: &[&str] = &["core.shadow.none", "core.shadow.hard"];

/// The values of a widget's font setting: the menus' font, the book's, the
/// book's italic.
pub const FACES: &[&str] = &[
    "widgets.face.ui",
    "widgets.face.book",
    "widgets.face.italic",
];

/// The values of a date-format setting.
pub const DATE_FORMATS: &[&str] = &[
    "widgets.date-format.long",
    "widgets.date-format.weekday",
    "widgets.date-format.numeric",
    "widgets.date-format.iso",
];

/// One setting of a widget's group; the key is relative to the widget.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum Opt {
    /// Where the text sits; the default is one of the alignments.
    Align(&'static str),
    /// The frame, with its default.
    Frame(&'static str),
    Padding,
    Paper,
    Ink,
    Corners,
    FrameLine,
    Shadow,
    /// The text size, with its default and whether "as large as fits" is offered.
    Size(&'static str, bool),
    /// A switch for a part of what the widget shows: key, title, default.
    Part(&'static str, &'static str, bool),
    /// A switch for a part with a help text of its own: key, title, default, help.
    PartHelp(&'static str, &'static str, bool, &'static str),
    /// A number: key, title, smallest, largest, default, help.
    Number(&'static str, &'static str, i32, i32, i64, &'static str),
    /// A choice: key, title, values, default, help.
    Choice(
        &'static str,
        &'static str,
        &'static [&'static str],
        &'static str,
        &'static str,
    ),
    /// Free text: key, title, default, help.
    Text(&'static str, &'static str, &'static str, &'static str),
    /// A folder on the card, chosen on the folder chooser (the slot
    /// `core.choose-folder`): key, title, default, help.
    Folder(&'static str, &'static str, &'static str, &'static str),
    /// How dates are written: the default is one of [`DATE_FORMATS`].
    DateFormat(&'static str),
}

/// A setting as declared: its key after the widget's, kind, title and help,
/// and whether the choice keeps values its options lack (colours).
#[derive(Debug)]
pub struct Row<'a> {
    pub suffix: &'a str,
    pub kind: Kind<'a>,
    pub title: &'a str,
    pub help: &'a str,
    pub keeps: bool,
}

fn choice<'a>(
    suffix: &'a str,
    (default, choices): (&'a str, &'a [&'a str]),
    title: &'a str,
    help: &'a str,
) -> Row<'a> {
    Row {
        suffix,
        kind: Kind::Choice { default, choices },
        title,
        help,
        keeps: false,
    }
}

pub fn row(option: Opt) -> Row<'static> {
    match option {
        Opt::Align(default) => choice(
            "align",
            (default, ALIGNMENTS),
            "core.setting.align",
            "core.setting.align.help",
        ),
        Opt::Frame(default) => choice(
            "frame",
            (default, FRAMES),
            "core.setting.frame",
            "core.setting.frame.help",
        ),
        Opt::Padding => choice(
            "padding",
            (PADDINGS[0], PADDINGS),
            "core.setting.padding",
            "core.setting.padding.help",
        ),
        Opt::Paper => Row {
            keeps: true,
            ..choice(
                "paper",
                (PAPERS[0], PAPERS),
                "core.setting.paper",
                "core.setting.paper.help",
            )
        },
        Opt::Ink => Row {
            keeps: true,
            ..choice(
                "ink",
                (INKS[0], INKS),
                "core.setting.ink",
                "core.setting.ink.help",
            )
        },
        Opt::Corners => choice(
            "corners",
            (CORNERS[0], CORNERS),
            "core.setting.corners",
            "core.setting.corners.help",
        ),
        Opt::FrameLine => choice(
            "frame-line",
            (LINES[0], LINES),
            "core.setting.frame-line",
            "core.setting.frame-line.help",
        ),
        Opt::Shadow => choice(
            "drop-shadow",
            (SHADOWS[0], SHADOWS),
            "core.setting.shadow",
            "core.setting.shadow.help",
        ),
        Opt::Size(default, fit) => choice(
            "size",
            (default, if fit { SIZES } else { &SIZES[..3] }),
            "core.setting.size",
            "core.setting.size.help",
        ),
        Opt::Part(key, title, default) => Row {
            suffix: key,
            kind: Kind::Bool(default),
            title,
            help: "core.setting.part.help",
            keeps: false,
        },
        Opt::PartHelp(key, title, default, help) => Row {
            suffix: key,
            kind: Kind::Bool(default),
            title,
            help,
            keeps: false,
        },
        Opt::Number(key, title, min, max, default, help) => Row {
            suffix: key,
            kind: Kind::Int { default, min, max },
            title,
            help,
            keeps: false,
        },
        Opt::Choice(key, title, values, default, help) => {
            choice(key, (default, values), title, help)
        }
        Opt::Text(key, title, default, help) | Opt::Folder(key, title, default, help) => Row {
            suffix: key,
            kind: Kind::Text(default),
            title,
            help,
            keeps: false,
        },
        Opt::DateFormat(default) => choice(
            "date-format",
            (default, DATE_FORMATS),
            "widgets.setting.date-format",
            "widgets.setting.date-format.help",
        ),
    }
}

/// Declares the setting `option` of `widget` (`day`) in `group`
/// (`widgets.day`), and what its choice keeps.
///
/// # Errors
/// What the declaration encoder refuses.
pub fn declare_option(
    d: &mut Declarations<impl FnMut(&[u8]) -> Result<(), i32>>,
    widget: &str,
    group: &str,
    option: Opt,
) -> Result<(), DeclareError> {
    let row = row(option);
    let relative = [widget, ".", row.suffix].concat();
    let mut spec = SpecDecl::new(&relative, row.kind, group, row.title);
    spec.help = row.help;
    d.spec(spec)?;
    if row.keeps {
        let mut extra = ExtraDecl::new(&relative);
        extra.keeps_unknown = true;
        d.extra(extra)?;
    }
    if matches!(option, Opt::Folder(..)) {
        let mut extra = ExtraDecl::new(&relative);
        extra.editor = Some(EditorDecl::Slot(CHOOSE_FOLDER));
        d.extra(extra)?;
    }
    Ok(())
}

/// Declares every setting of `options` for `widget` in `group`.
///
/// # Errors
/// What the declaration encoder refuses.
pub fn declare_all(
    d: &mut Declarations<impl FnMut(&[u8]) -> Result<(), i32>>,
    widget: &str,
    group: &str,
    options: impl IntoIterator<Item = Opt>,
) -> Result<(), DeclareError> {
    for option in options {
        declare_option(d, widget, group, option)?;
    }
    Ok(())
}

/// The look settings every widget shares, in their order: alignment (with its
/// default), frame (none), padding, paper, ink, corners, frame line, shadow.
#[must_use]
pub const fn shared_look(align: &'static str) -> [Opt; 8] {
    [
        Opt::Align(align),
        Opt::Frame("core.frame.none"),
        Opt::Padding,
        Opt::Paper,
        Opt::Ink,
        Opt::Corners,
        Opt::FrameLine,
        Opt::Shadow,
    ]
}
