//! The reading screen's status lines (R2b-2): what the book tells the reader's
//! own items, where the lines are painted, and the tap on a line (the next
//! line, or an item's action). What is decided without a host is in
//! `native_status`.
use paperpaw_sdk::native::{callbacks::InputGestureKind, storage::DataStorage};

use super::{
    CommandSender, DrawFrame, Error, Handled, Host, Input, Open, RedrawMode, Setup, Ui,
    native_pages, redraw,
};
use crate::native_status::{self, Config, Facts, Place, Tapped};

impl Open {
    /// Where the status lines are for `setup` and the content box `area`: none
    /// in a book of whole pages.
    fn status_place<'s>(&self, setup: &'s Setup, area: &native_pages::Box) -> Option<Place<'s>> {
        (!self.whole_pages).then_some(Place {
            room: setup.room,
            across: (area.x, area.width),
            display: &setup.display,
        })
    }

    /// What the book tells the items of the reader: its title, the chapter of
    /// the page (the title where it is in none or the book has no contents),
    /// how far the reader is, the pages once they are counted.
    fn with_facts<T>(
        &self,
        host: &Host<'_>,
        then: impl FnOnce(&Facts<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let facts = Facts {
            title: &self.title,
            chapter: crate::native_chapter::shown_title(self.chapter.as_ref(), self.at),
            ppm: self
                .progress
                .as_ref()
                .map(|progress| progress.ppm(self.at.0, self.at.1)),
            page: self.page_numbers(host),
            marks: &[],
        };
        then(&facts)
    }

    /// Paints the status lines that are in the rows of this draw.
    pub(super) fn paint_status(
        &mut self,
        host: &Host<'_>,
        ui: Option<&Ui>,
        frame: &mut DrawFrame<'_>,
        (setup, area): (&Setup, &native_pages::Box),
    ) -> Result<(), Error> {
        let (Some(ui), Some(place)) = (ui, self.status_place(setup, area)) else {
            return Ok(());
        };
        if !place.room.top && !place.room.bottom {
            return Ok(());
        }
        self.status.load(&DataStorage::new(host))?;
        self.with_facts(host, |facts| {
            native_status::paint(host, ui, frame, (&place, &self.status, facts))
        })
    }

    /// A tap on a status line, or the action that shows the next one.
    pub(super) fn status_input(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        input: &Input<'_>,
    ) -> Result<Option<Handled>, Error> {
        let (point, shown) = match *input {
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => (Some(position), None),
            Input::Action {
                action: "reader.next-status-line",
                ..
            } => (None, Some(false)),
            Input::Action {
                action: "reader.next-top-status-line",
                ..
            } => (None, Some(true)),
            _ => return Ok(None),
        };
        let setup = Setup::read(host)?;
        let area = self.area(&setup).ok_or(Error::NoSpace)?;
        let Some(place) = self.status_place(&setup, &area) else {
            return Ok(None);
        };
        let storage = DataStorage::new(host);
        self.status.load(&storage)?;
        let tapped = match (point, shown) {
            (Some(point), _) => self.with_facts(host, |facts| {
                native_status::tap(host, ui, point, (&place, &self.status, facts))
            })?,
            // The bottom line, or the top where there is none below.
            (None, top) => match top.unwrap_or(!place.room.bottom) {
                top if (top && place.room.top) || (!top && place.room.bottom) => Tapped::Next(top),
                _ => return Ok(Some(Handled::Yes)),
            },
        };
        match tapped {
            Tapped::Missed => return Ok(None),
            Tapped::Next(top) => {
                let config = Config::read(host)?;
                self.status
                    .advance(top, &config.lines[usize::from(!top)], &storage)?;
                redraw(host, RedrawMode::Partial)?;
            }
            Tapped::Action(action) => {
                let mut scratch = paperpaw_ui_kit::mem::buffer(512)?;
                CommandSender::new(host, &mut scratch).run_action(&action)?;
            }
        }
        Ok(Some(Handled::Yes))
    }
}
