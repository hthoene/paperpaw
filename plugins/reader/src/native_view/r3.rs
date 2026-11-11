//! The reader's actions that hand over to other screens or to the library: the
//! book's information (`core.book-info`), the font and layout panel
//! (`core.panel`) and the bookmark of the page (`native_bookmark`).
use super::{
    Body, CommandSender, Documents, Error, Handled, Host, Library, NavOpen, Open, Param,
    RedrawMode, SERVICE_BUFFER, Ui, View, buffer, native_place, redraw,
};
use crate::native_bookmark;

/// The slot that shows a book's information (parameter `path`).
const BOOK_INFO: &str = "core.book-info";
/// The slot of a panel of controls (parameter `menu`: its menu).
const PANEL: &str = "core.panel";
/// The menu of the font and layout panel.
const LAYOUT_MENU: &str = "core.reader-panel";

impl View {
    /// The actions of this slice; `None` for any other.
    pub(super) fn r3_action(
        &mut self,
        host: &Host<'_>,
        (_docs, library, ui): (&Documents, Option<&Library>, &Ui),
        action: &str,
    ) -> Result<Option<Handled>, Error> {
        match action {
            "reader.book-info" => self.book_info(host, ui)?,
            "reader.quick-settings" => Self::quick_settings(host, ui)?,
            "reader.toggle-bookmark" => self.toggle_bookmark(host, library)?,
            _ => return Ok(None),
        }
        Ok(Some(Handled::Yes))
    }

    /// Opens the information of the book on show: its slot, with the path.
    fn book_info(&self, host: &Host<'_>, ui: &Ui) -> Result<(), Error> {
        let Body::Open(open) = &self.body else {
            return Ok(());
        };
        Self::open_slot(host, ui, BOOK_INFO, ("path", &open.path))
    }

    /// Opens the panel of the font and layout settings.
    fn quick_settings(host: &Host<'_>, ui: &Ui) -> Result<(), Error> {
        Self::open_slot(host, ui, PANEL, ("menu", LAYOUT_MENU))
    }

    /// Opens the slot `target` with one hand-over parameter.
    fn open_slot(
        host: &Host<'_>,
        ui: &Ui,
        target: &str,
        (key, value): (&str, &str),
    ) -> Result<(), Error> {
        let params = [Param::new(key, value)?];
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let handover = ui.handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(NavOpen::slot(target)?.handover(handover))
    }

    /// Sets or removes the bookmark of the page on show; a refused change is said
    /// (the bookmarks could not be kept), nothing changes then.
    pub(super) fn toggle_bookmark(
        &mut self,
        host: &Host<'_>,
        library: Option<&Library>,
    ) -> Result<(), Error> {
        let (Body::Open(open), Some(library)) = (&mut self.body, library) else {
            return Ok(());
        };
        match open.toggle_bookmark(host, library) {
            Ok(()) => Ok(()),
            // Short memory is the host's to hear, not a bookmark that failed.
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => {
                self.say_marks_not_saved(host);
                Ok(())
            }
        }
    }

    /// Says that a bookmark, highlight or note could not be kept.
    fn say_marks_not_saved(&self, host: &Host<'_>) {
        native_bookmark::say_not_saved(host);
    }
}

impl Open {
    /// The bookmark toggled for the page on show.
    fn toggle_bookmark(&mut self, host: &Host<'_>, library: &Library) -> Result<(), Error> {
        let Some(page) = &self.page else {
            // Not drawn yet: there is no page to mark.
            return Ok(());
        };
        let start = (page.start.section, page.start.offset);
        if start == (page.end.section, page.end.offset) {
            // An empty page has nothing to mark: no bookmark, and no notice.
            return Ok(());
        }
        let text = native_bookmark::page_text(&page.lines)?;
        let ppm = self
            .progress
            .as_ref()
            .map_or(self.stored_ppm, |p| p.ppm(start.0, start.1));
        let shown = native_bookmark::Page {
            range: native_bookmark::Range {
                start,
                end: (page.end.section, page.end.offset),
            },
            text: &text,
            percent: native_bookmark::percent_of(ppm),
            conversion: self.conversion,
            now: native_place::wall_seconds(host),
        };
        let done = native_bookmark::toggle(host, library, &self.path, &shown)?;
        self.bookmarks_toggled(shown.range, done);
        // The ribbon comes or goes; a refused redraw is no lost bookmark (the
        // next draw shows it).
        redraw(host, RedrawMode::Partial).ok();
        Ok(())
    }
}
