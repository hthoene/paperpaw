//! The reader's menu as a bar at the top (Select or Menu on the reading
//! screen), as Beta 3 opened it: the menu `core.reader-menu` in the menu slot,
//! with the book's title, its chapter and where one is in it, and without what
//! a book of page pictures cannot do.

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    commands::CommandSender,
    ui::{Open as NavOpen, Param, Ui},
};
use paperpaw_ui_kit::{
    mem::{SERVICE_BUFFER, buffer},
    text,
};

use super::{Error, Host, Open};

/// The menu the bar shows (`menus::READER`).
const MENU: &str = "core.reader-menu";
/// What a book of page pictures leaves out: no text to search, hear or set.
const PICTURES_HIDE: &str = "reader.search,reader.read-aloud,reader.quick-settings";
/// The longest title and chapter the bar is handed (cut on a character).
const MAX_TEXT: usize = 256;
/// The hand-over request: its keys and values (at most about 650 bytes with a
/// title and a chapter of [`MAX_TEXT`]) and 9 bytes of fields and framing a row
/// (`ui.md` section 6.3); the reply stays the operation's 4,096.
const HANDOVER_INPUT: usize = 1280;
/// Rows of the hand-over.
const ROWS: usize = 7;

impl Open {
    /// Opens the reader's menu bar over the page on show.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals.
    pub(super) fn menu_bar(&self, host: &Host<'_>, ui: &Ui) -> Result<Handled, Error> {
        let mut digits = ([0; 20], [0; 20]);
        let place = match self.page_numbers(host) {
            Some((page, pages)) => text::concat(&[
                text::decimal_u64(u64::from(page), &mut digits.0),
                " / ",
                text::decimal_u64(u64::from(pages), &mut digits.1),
            ])?,
            None => {
                let ppm = self
                    .progress
                    .as_ref()
                    .map_or(self.stored_ppm, |p| p.ppm(self.at.0, self.at.1));
                text::concat(&[
                    text::decimal_u64(u64::from(ppm / 10_000), &mut digits.0),
                    " %",
                ])?
            }
        };
        let chapter = self.chapter.as_ref().map_or("", |c| c.title());
        let values = bar_values(&self.title, chapter, &place, self.whole_pages);
        let [
            (k0, v0),
            (k1, v1),
            (k2, v2),
            (k3, v3),
            (k4, v4),
            (k5, v5),
            (k6, v6),
        ] = values;
        let params = [
            Param::new(k0, v0)?,
            Param::new(k1, v1)?,
            Param::new(k2, v2)?,
            Param::new(k3, v3)?,
            Param::new(k4, v4)?,
            Param::new(k5, v5)?,
            Param::new(k6, v6)?,
        ];
        let (mut input, mut out) = (buffer(HANDOVER_INPUT)?, buffer(SERVICE_BUFFER)?);
        let handover = ui.handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch)
            .open(NavOpen::slot("core.menu")?.handover(handover))?;
        Ok(Handled::Yes)
    }
}

/// The keys and values the bar is handed: the menu, the bar's look, what a book
/// of page pictures leaves out, the book's title and chapter (cut), where one is
/// and what a tap on the title runs.
fn bar_values<'a>(
    title: &'a str,
    chapter: &'a str,
    place: &'a str,
    whole_pages: bool,
) -> [(&'static str, &'a str); ROWS] {
    [
        ("menu", MENU),
        ("mode", "bar"),
        ("hide", if whole_pages { PICTURES_HIDE } else { "" }),
        ("title", cut(title)),
        ("text", cut(chapter)),
        ("page", place),
        ("action", "reader.book-info"),
    ]
}

/// `value` cut to [`MAX_TEXT`] bytes on a character boundary.
fn cut(value: &str) -> &str {
    let mut end = value.len().min(MAX_TEXT);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.get(..end).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The largest request the bar makes: a picture book whose title and chapter
    /// are longer than kept, of letters of two and three bytes. It is within the
    /// contract's 1,024 bytes of keys and values and the input buffer.
    #[test]
    fn the_largest_request_fits_the_input_buffer() {
        let long = "äöü€".repeat(200);
        let place = "99999 / 99999";
        let values = bar_values(&long, &long, place, true);
        let bytes: usize = values.iter().map(|(k, v)| k.len() + v.len()).sum();
        assert!(bytes <= 1024, "{bytes}");
        // Each row: its two lengths, its tag and frame (9 bytes, `ui.md` 6.3); the count.
        let request = bytes + ROWS * 9 + 1;
        assert!(request <= HANDOVER_INPUT, "{request}");
        assert!(values.iter().all(|(_, v)| v.len() <= MAX_TEXT));
        assert!(values[3].1.is_char_boundary(values[3].1.len()));
    }
}
