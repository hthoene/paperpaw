//! The sizes and looks every part of the UI is drawn with ([`UiStyle`]),
//! the focus, separators and rounded fills.

/// What a colour is for ([`UiStyle::ink`]): a colour role of the
/// interface, each set by the palette (setting `core.ui-palette`), so a
/// theme changes them all at once and plugins draw in the same colours.
///
/// An `Ink` is a colour role; how text is inked (its tone) is a
/// [`TextInk`]. The roles are constants of one type, so more can come
/// without a new plugin interface: a role this release does not know is
/// drawn in the colour of its [`fallback`](Ink::fallback), the role a
/// theme made for older releases already sets. Roles 0 to 13 have a
/// default of their own, 14 to 18 are named, 19 to 31 are free (the
/// palette has a slot for each), and what is past them still reads a
/// fallback.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ink(u8);

/// The names of the named [`Ink`]s in the palette, by their numbers.
const INKS: [&str; 19] = [
    "text",
    "paper",
    "note",
    "detail",
    "muted",
    "faint",
    "disabled",
    "rule",
    "line",
    "line-faint",
    "on-focus-muted",
    "on-dark-muted",
    "card",
    "section",
    "accent",
    "on-accent",
    "link",
    "selection",
    "warning",
];

impl Ink {
    /// The role of `value`; one this release has no name for is kept, and
    /// reads its [`fallback`](Ink::fallback).
    pub const fn from_value(value: u8) -> Ink {
        Ink(value)
    }

    /// Text and what is drawn in its colour.
    pub const TEXT: Ink = Ink(0);
    /// What the interface is drawn on.
    pub const PAPER: Ink = Ink(1);
    /// Notes below the titles of rows, what an empty screen explains.
    pub const NOTE: Ink = Ink(2);
    /// What is told beside or below a row's title: details, the notes of
    /// lists, hints on a page.
    pub const DETAIL: Ink = Ink(3);
    /// Headings of sections, a widget's lesser text.
    pub const MUTED: Ink = Ink(4);
    /// The least of text, e.g. what an empty screen says.
    pub const FAINT: Ink = Ink(5);
    /// What cannot be used now, e.g. an arrow at the last page.
    pub const DISABLED: Ink = Ink(6);
    /// The line below a light header or above the row of button hints.
    pub const RULE: Ink = Ink(7);
    /// The lines between rows.
    pub const LINE: Ink = Ink(8);
    /// The fainter lines between rows that start at the text.
    pub const LINE_FAINT: Ink = Ink(9);
    /// Gray text on a dark focus.
    pub const ON_FOCUS_MUTED: Ink = Ink(10);
    /// Lesser text on a dark card.
    pub const ON_DARK_MUTED: Ink = Ink(11);
    /// Cards, e.g. the frames of widgets (on panels of black and white
    /// alone, a sparser pattern of it).
    pub const CARD: Ink = Ink(12);
    /// The cards a list's sections are on.
    pub const SECTION: Ink = Ink(13);
    /// What stands out on a colour panel, e.g. a selected tab (text where
    /// a theme sets none).
    pub const ACCENT: Ink = Ink(14);
    /// What is drawn on an [`ACCENT`](Ink::ACCENT) (paper where a theme
    /// sets none).
    pub const ON_ACCENT: Ink = Ink(15);
    /// A link in a text (text where a theme sets none).
    pub const LINK: Ink = Ink(16);
    /// What a selection is marked with (a card's colour where a theme
    /// sets none).
    pub const SELECTION: Ink = Ink(17);
    /// A warning or an error (text where a theme sets none).
    pub const WARNING: Ink = Ink(18);

    /// Every named role, in the order of their numbers.
    pub const ALL: &'static [Ink] = &[
        Ink::TEXT,
        Ink::PAPER,
        Ink::NOTE,
        Ink::DETAIL,
        Ink::MUTED,
        Ink::FAINT,
        Ink::DISABLED,
        Ink::RULE,
        Ink::LINE,
        Ink::LINE_FAINT,
        Ink::ON_FOCUS_MUTED,
        Ink::ON_DARK_MUTED,
        Ink::CARD,
        Ink::SECTION,
        Ink::ACCENT,
        Ink::ON_ACCENT,
        Ink::LINK,
        Ink::SELECTION,
        Ink::WARNING,
    ];

    /// The number the role goes by in data.
    pub const fn value(self) -> u8 {
        self.0
    }

    /// Its name in the palette, e.g. `muted` (empty for a role without
    /// one).
    pub const fn name(self) -> &'static str {
        if (self.0 as usize) < INKS.len() {
            INKS[self.0 as usize]
        } else {
            ""
        }
    }

    /// The role whose colour this one has where a theme does not set it:
    /// one of the first 14 for every role, itself for those; text for a
    /// role this release does not know.
    pub const fn fallback(self) -> Ink {
        match self.0 {
            // The roles with a default of their own.
            0..=13 => self,
            15 => Ink::PAPER,
            17 => Ink::CARD,
            // ACCENT, LINK, WARNING and the roles of later releases.
            _ => Ink::TEXT,
        }
    }
}

impl core::fmt::Debug for Ink {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use core::fmt::Write as _;
        match INKS.get(usize::from(self.0)) {
            Some(name) => {
                f.write_str("Ink::")?;
                for c in name.chars() {
                    f.write_char(if c == '-' {
                        '_'
                    } else {
                        c.to_ascii_uppercase()
                    })?;
                }
                Ok(())
            }
            None => write!(f, "Ink({})", self.0),
        }
    }
}
