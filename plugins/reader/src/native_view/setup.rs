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
    /// The signature of the text filters the blocks went through (0: none).
    pub(super) filtered: u64,
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

/// The reading font's name as read, or none where it is no text or too long
/// for the buffer: the page is then set in the default font, not refused.
fn font_name<'a>(read: Result<SettingValue<'a>, Error>) -> Result<&'a str, Error> {
    match read {
        Ok(SettingValue::Text(font)) => Ok(font),
        Ok(_) | Err(Error::NoSpace) => Ok(""),
        Err(other) => Err(other),
    }
}

/// `reading` with the reading font and the hyphenation settings it is laid
/// out with (`Reading::with_text`).
fn text_of(host: &Host<'_>, reading: Reading) -> Result<Reading, Error> {
    let mut name = [0; 96];
    let font = font_name(settings::read_setting(host, "core.reading-font", &mut name))?;
    Ok(reading.with_text(
        font,
        (
            flag(host, "core.hyphenate")?,
            integer(host, "core.hyphen-min-left")?,
            integer(host, "core.hyphen-min-right")?,
        ),
    ))
}

impl Setup {
    pub(super) fn read(host: &Host<'_>) -> Result<Self, Error> {
        let display = snapshot::capture_snapshot(host)?;
        let room = native_status::room(host, display.display_dpi())?;
        let (top, bottom) = room.reserved();
        Ok(Self {
            reading: text_of(
                host,
                Reading::new(
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
            )?,
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
            filtered: 0,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_font_name_too_long_for_the_buffer_is_the_default_font() {
        assert_eq!(
            font_name(Ok(SettingValue::Text("Literata"))),
            Ok("Literata")
        );
        assert_eq!(font_name(Ok(SettingValue::Int(3))), Ok(""));
        // The host says the whole name does not fit: no refusal of the page.
        assert_eq!(font_name(Err(Error::NoSpace)), Ok(""));
        // Another failure is still one.
        assert_eq!(font_name(Err(Error::NoMemory)), Err(Error::NoMemory));
    }
}
