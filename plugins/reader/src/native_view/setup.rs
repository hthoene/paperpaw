//! The reading settings and display a page is laid out with (`Setup`), and
//! the key that says when a laid-out page is stale (`LayoutKey`).
use crate::{native_pages::Reading, native_status};
use paperpaw_sdk::{
    layout::{BookStyles, Style},
    native::{
        Error, Host,
        settings::{self, SettingValue},
        snapshot::{self, Snapshot},
    },
};

/// What a page is laid out with: the reading settings and the display's size
/// and density (a turned screen is another size).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LayoutKey {
    pub(super) reading: Reading,
    pub(super) display: (u16, u16, u16),
}

/// The text the screen reads its page from: reading settings and the display.
pub(super) struct Setup {
    pub(super) reading: Reading,
    pub(super) display: Snapshot,
    /// The room of the status lines (their size and places).
    pub(super) room: native_status::Room,
}

pub(super) fn integer(host: &Host<'_>, key: &str) -> Result<i64, Error> {
    let mut bytes = [0; 32];
    Ok(match settings::read_setting(host, key, &mut bytes)? {
        SettingValue::Int(value) => value,
        _ => 0,
    })
}

pub(super) fn flag(host: &Host<'_>, key: &str) -> Result<bool, Error> {
    let mut bytes = [0; 32];
    Ok(matches!(
        settings::read_setting(host, key, &mut bytes)?,
        SettingValue::Bool(true)
    ))
}

/// Whether paragraphs are broken over all their lines (`core.line-breaking`
/// is not the fast choice), as in Beta 3.
fn balanced(host: &Host<'_>) -> Result<bool, Error> {
    let mut bytes = [0; 64];
    Ok(!matches!(
        settings::read_setting(host, "core.line-breaking", &mut bytes)?,
        SettingValue::Text("core.line-breaking.fast")
    ))
}

/// The book's own styles followed, by their switches (`core.book-…`), as the
/// bits of `BookStyles`.
fn book_styles(host: &Host<'_>) -> Result<u16, Error> {
    let mut book = BookStyles::NONE;
    for (key, style) in [
        ("core.book-align", BookStyles::ALIGN),
        ("core.book-indent", BookStyles::INDENT),
        ("core.book-sizes", BookStyles::SIZE),
        ("core.book-spacing", BookStyles::SPACING),
        ("core.book-fonts", BookStyles::FONTS),
        ("core.book-margins", BookStyles::MARGINS),
        ("core.book-line-height", BookStyles::LINE_HEIGHT),
    ] {
        if flag(host, key)? {
            book = book.with(style);
        }
    }
    Ok(book.bits())
}

impl Setup {
    pub(super) fn read(host: &Host<'_>) -> Result<Self, Error> {
        let display = snapshot::capture_snapshot(host)?;
        let room = native_status::room(host, display.display_dpi())?;
        let (top, bottom) = room.reserved();
        Ok(Self {
            reading: Reading::new(
                integer(host, "core.font-size")?,
                integer(host, "core.line-spacing")?,
                integer(host, "core.word-spacing")?,
                integer(host, "core.margin")?,
                integer(host, "core.margin-ends")?,
                flag(host, "core.justify")?,
            )
            .with_status(top, bottom)
            .with_paragraphs(
                (
                    flag(host, "core.indent")?,
                    integer(host, "core.indent-size")?,
                ),
                (
                    flag(host, "core.paragraph-spacing")?,
                    integer(host, "core.paragraph-space")?,
                ),
            )
            .with_lines(
                (
                    integer(host, "core.widows")?,
                    integer(host, "core.orphans")?,
                ),
                balanced(host)?,
                book_styles(host)?,
            ),
            display,
            room,
        })
    }

    /// What a page laid out with this setup depends on.
    pub(super) fn key(&self) -> LayoutKey {
        LayoutKey {
            reading: self.reading,
            display: (
                self.display.display_width(),
                self.display.display_height(),
                self.display.display_dpi(),
            ),
        }
    }

    /// The size of body text in pixels.
    pub(super) fn base(&self) -> f32 {
        self.reading.size_pt * f32::from(self.display.display_dpi()) / 72.0
    }

    /// How the text is set, with paragraphs spaced by parts of the body's
    /// `line` height (`Reading::style`).
    pub(super) fn style(&self, line: f32) -> Style {
        self.reading.style(self.base(), line)
    }
}
