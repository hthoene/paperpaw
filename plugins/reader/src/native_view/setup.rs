//! The reading settings and display a page is laid out with (`Setup`), and
//! the key that says when a laid-out page is stale (`LayoutKey`).
use crate::{native_look::Look, native_pages::Reading, native_status};
use alloc::boxed::Box;
use alloc::rc::Rc;
use paperpaw_sdk::{
    layout::{BookStyles, Style},
    native::{
        Error, Host,
        fonts::Font,
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
#[derive(Clone, Copy, Debug)]
pub(super) struct Setup {
    pub(super) reading: Reading,
    pub(super) display: Snapshot,
    /// The room of the status lines (their size and places).
    pub(super) room: native_status::Room,
}

/// What the settings said when they were last read, kept so that a page turn
/// does not read some forty settings again for every step and every band it
/// draws (#250). It is read again after any setting changed
/// ([`Kept::forget`], told by the `setting_changed` event) and where the screen
/// is another (a turned display); the display is taken anew every time.
///
/// Behind a `Box`: the open book is built in a frame of `open`, which must stay
/// under 2,048 bytes on the S3, and a `Setup` is large.
#[derive(Debug, Default)]
pub(super) struct Kept(Box<core::cell::RefCell<State>>);

#[derive(Debug, Default)]
struct State {
    setup: Option<Setup>,
    /// How the text is inked, with the grays it was made for.
    look: Option<(Option<u16>, Look)>,
    /// What the status lines read.
    status: Option<Rc<native_status::Shown>>,
}

impl Kept {
    /// The setup now: the kept settings on the display as it is, or read.
    ///
    /// # Errors
    /// What reading a setting refuses.
    // Out of line: a Setup is large, and its copies in the callers' frames made
    // the frames of `open` and `screen_shown` pass the limit of 2,048 bytes.
    #[inline(never)]
    pub(super) fn setup(&self, host: &Host<'_>) -> Result<Setup, Error> {
        let display = snapshot::capture_snapshot(host)?;
        let kept = self.0.borrow().setup;
        if let Some(kept) = kept
            && kept.same_screen(&display)
        {
            return Ok(Setup { display, ..kept });
        }
        let setup = Setup::read_on(host, display)?;
        // Another screen: the colour of the notes is another as well.
        *self.0.borrow_mut() = State {
            setup: Some(setup),
            ..State::default()
        };
        Ok(setup)
    }

    /// How the text is inked for a panel of `grays`: kept like the setup.
    ///
    /// # Errors
    /// What reading a setting refuses.
    // Out of line: a Setup is large, and its copies in the callers' frames made
    // the frames of `open` and `screen_shown` pass the limit of 2,048 bytes.
    #[inline(never)]
    pub(super) fn look(&self, host: &Host<'_>, grays: Option<u16>) -> Result<Look, Error> {
        let kept = self.0.borrow().look;
        if let Some((at, look)) = kept
            && at == grays
        {
            return Ok(look);
        }
        let look = Look::read(host, grays)?;
        self.0.borrow_mut().look = Some((grays, look));
        Ok(look)
    }

    /// What a paint of the status lines reads, kept like the setup.
    ///
    /// # Errors
    /// What reading a setting refuses.
    // Out of line: a Setup is large, and its copies in the callers' frames made
    // the frames of `open` and `screen_shown` pass the limit of 2,048 bytes.
    #[inline(never)]
    pub(super) fn status(
        &self,
        host: &Host<'_>,
        display: &Snapshot,
        font: &Font<'_>,
    ) -> Result<Rc<native_status::Shown>, Error> {
        let kept = self.0.borrow().status.clone();
        if let Some(kept) = kept {
            return Ok(kept);
        }
        let shown = Rc::new(native_status::Shown::read(host, display, font)?);
        self.0.borrow_mut().status = Some(Rc::clone(&shown));
        Ok(shown)
    }

    /// A setting changed: the next read asks the host.
    pub(super) fn forget(&self) {
        *self.0.borrow_mut() = State::default();
    }
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

/// Whether Korean is broken between words only (`core.korean-breaks`).
fn korean_by_word(host: &Host<'_>) -> Result<bool, Error> {
    let mut bytes = [0; 64];
    Ok(matches!(
        settings::read_setting(host, "core.korean-breaks", &mut bytes)?,
        SettingValue::Text("core.korean-breaks.word")
    ))
}

/// `reading` with the reading font and the hyphenation settings it is laid
/// out with (`Reading::with_text`), and the scripts' breaks.
fn text_of(host: &Host<'_>, reading: Reading) -> Result<Reading, Error> {
    let mut name = [0; 96];
    let font = font_name(settings::read_setting(host, "core.reading-font", &mut name))?;
    let scripts = (
        flag(host, "core.hanging-punctuation")?,
        korean_by_word(host)?,
    );
    Ok(reading
        .with_text(
            font,
            (
                flag(host, "core.hyphenate")?,
                integer(host, "core.hyphen-min-left")?,
                integer(host, "core.hyphen-min-right")?,
            ),
        )
        .with_scripts(scripts.0, scripts.1))
}

impl Setup {
    pub(super) fn read(host: &Host<'_>) -> Result<Self, Error> {
        Self::read_on(host, snapshot::capture_snapshot(host)?)
    }

    /// The setup of the settings now on `display`.
    // Out of line: a Setup is large, and its copies in the callers' frames made
    // the frames of `open` and `screen_shown` pass the limit of 2,048 bytes.
    #[inline(never)]
    fn read_on(host: &Host<'_>, display: Snapshot) -> Result<Self, Error> {
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

    /// Whether `other` is the same screen: its size, density and grays.
    fn same_screen(&self, other: &Snapshot) -> bool {
        let mine = &self.display;
        (
            mine.display_width(),
            mine.display_height(),
            mine.display_dpi(),
            mine.display_gray_levels(),
            mine.display_is_colour(),
        ) == (
            other.display_width(),
            other.display_height(),
            other.display_dpi(),
            other.display_gray_levels(),
            other.display_is_colour(),
        )
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

    #[test]
    fn the_kept_settings_are_one_pointer_in_the_open_book() {
        // The open book is built in a frame of `open` (2,048 bytes at most on the
        // S3): what it keeps of the settings stays off it.
        assert_eq!(core::mem::size_of::<Kept>(), core::mem::size_of::<usize>());
    }
}
