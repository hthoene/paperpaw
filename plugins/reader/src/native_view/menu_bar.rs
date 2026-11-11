//! The reader's menu as a bar at the top (Select or Menu on the reading
//! screen), as Beta 3 opened it: the menu `core.reader-menu` in the menu slot,
//! with the book's title, its chapter and where one is in it, and without what
//! a book of page pictures cannot do.

use paperpaw_sdk::native::{
    commands::CommandSender,
    ui::{Open as NavOpen, Param, Ui},
};
use paperpaw_sdk::Handled;
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
        let hide = if self.whole_pages { PICTURES_HIDE } else { "" };
        let params = [
            Param::new("menu", MENU)?,
            Param::new("mode", "bar")?,
            Param::new("hide", hide)?,
            Param::new("title", cut(&self.title))?,
            Param::new("text", cut(chapter))?,
            Param::new("page", &place)?,
            Param::new("action", "reader.book-info")?,
        ];
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let handover = ui.handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch)
            .open(NavOpen::slot("core.menu")?.handover(handover))?;
        Ok(Handled::Yes)
    }
}

/// `value` cut to [`MAX_TEXT`] bytes on a character boundary.
fn cut(value: &str) -> &str {
    let mut end = value.len().min(MAX_TEXT);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.get(..end).unwrap_or_default()
}
