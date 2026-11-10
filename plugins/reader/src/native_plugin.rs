//! The interface-7 reader (entry slice): what the reader declares reaches the
//! host (settings, the six screens and the slots they fill, actions, menu
//! entries, key bindings, the events heard and the three stages). The screens
//! have no view yet: an opened screen draws nothing and takes no input, and
//! the plugin is not in the app's `NATIVE_PLUGINS` until they have one
//! (`PORTING-7.md`). What the reader decides without a host is in the
//! `native_*` models, tested without one.
use crate::{
    declarations::{FOOTNOTE, SCREEN, STOPPING, SUSPEND, WORK_DUE},
    native_footnote::{Footnote, Pressed},
    native_view::View,
};
use alloc::string::String;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        document::Documents,
        events::{Event, Incoming, kinds},
        library::Library,
        registrations,
        reply::Reply,
        ui::Ui,
        work::{Work, WorkSpec},
    },
};
use paperpaw_ui_kit::{Screens, read_event};

mod image {
    include!(concat!(env!("OUT_DIR"), "/reader_declarations.rs"));
}

/// The batches the reader submits, in the order the host needs them: the
/// settings and screens before what refers to them. The events are heard for
/// what saves the place (sleep, stop, the quiet time); the start and a moved
/// book have no handler yet (`PORTING-7.md`).
const BATCHES: [&[&[u8]]; 7] = [
    image::SETTINGS,
    image::SCREENS,
    image::NAVIGATION,
    image::KEYS,
    image::HOLD_KEYS,
    image::EVENTS,
    image::STAGES,
];

/// The key of the work that writes the place, and how long a page rests first
/// (Beta 3's `SAVE_QUIET_MS`).
const QUIET_KEY: &str = "place";
const QUIET_MS: u32 = 2_000;

/// The interface-7 reader.
#[derive(Debug, Default)]
pub struct NativeReader {
    ui: Option<Ui>,
    docs: Option<Documents>,
    library: Option<Library>,
    work: Option<Work>,
    /// The host has no work for the reader: not asked again.
    no_work: bool,
    screens: Screens<View>,
    /// The notes over the pages.
    notes: Screens<Footnote>,
    /// *Go to the note* pressed: the book and the link, followed when its
    /// reading screen is shown again.
    follow: Option<(String, String)>,
}

impl NativeReader {
    /// The reading screen opened: its book is opened with it.
    fn open(
        &mut self,
        host: &Host<'_>,
        instance: paperpaw_sdk::native::ui::Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        if self.docs.is_none() {
            self.docs = Some(Documents::open(host)?);
        }
        // Without the library there is no saved place, and none is written.
        if self.library.is_none() {
            self.library = Library::open(host).ok();
        }
        let (Some(ui), Some(docs)) = (&self.ui, &self.docs) else {
            return Err(Error::State);
        };
        let view = View::open(host, ui, (docs, self.library.as_ref()), handover)?;
        self.screens.open(instance, view)
    }

    /// A note opened over the page.
    fn open_note(
        &mut self,
        host: &Host<'_>,
        instance: paperpaw_sdk::native::ui::Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let note = Footnote::open(host, ui, handover)?;
        self.notes.open(instance, note)
    }

    /// The reading screen is on show again: a note's *Go to the note* is followed
    /// in it.
    fn follow_note(
        &mut self,
        host: &Host<'_>,
        instance: paperpaw_sdk::native::ui::Instance,
    ) -> Result<(), Error> {
        let Some((path, href)) = self.follow.take() else {
            return Ok(());
        };
        let (Some(view), Some(docs)) = (self.screens.get(instance), &self.docs) else {
            return Ok(());
        };
        view.follow(host, docs, (&path, &href))
    }

    /// Writes the place of every reading screen not saved.
    fn save_now(&mut self, host: &Host<'_>) {
        let library = self.library.as_ref();
        for view in self.screens.all() {
            view.save_now(host, library);
        }
    }

    /// The place is to be written after the quiet time: declared at every
    /// turn, which replaces the work and starts the time again. Where the host
    /// has no work, the place is written at sleep, stop and close.
    fn start_quiet(&mut self, host: &Host<'_>) {
        if self.work.is_none() && !self.no_work {
            self.work = Work::open(host).ok();
            self.no_work = self.work.is_none();
        }
        let Some(work) = &self.work else {
            return;
        };
        let (Ok(mut input), Ok(mut out)) = (
            paperpaw_ui_kit::mem::buffer(256),
            paperpaw_ui_kit::mem::buffer(64),
        ) else {
            return;
        };
        if let Ok(spec) = WorkSpec::new(QUIET_KEY, WORK_DUE) {
            work.declare(host, &spec.after_ms(QUIET_MS), &mut input, &mut out)
                .ok();
        }
    }

    /// The screen closed: its document is given back.
    fn close(&mut self, host: &Host<'_>, instance: paperpaw_sdk::native::ui::Instance) {
        if let (Some(view), Some(docs)) = (self.screens.get(instance), &self.docs) {
            view.close(host, (docs, self.library.as_ref()));
        }
        self.screens.close(instance);
    }
}

impl Plugin for NativeReader {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batches in BATCHES {
            for batch in batches {
                registrations::submit(host, batch)?;
            }
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(
            incoming;
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed,
            kinds::Suspend,
            kinds::Stopping,
            kinds::WorkDue
        ) else {
            return Ok(());
        };
        match event {
            // The quiet time has come, or the device sleeps or stops.
            Event::WorkDue(_) | Event::Suspend { .. } | Event::Stopping { .. }
                if matches!(token, WORK_DUE | SUSPEND | STOPPING) =>
            {
                self.save_now(host);
                Ok(())
            }
            // Only the reading screen and the note have a view yet (`PORTING-7.md`).
            _ if token != SCREEN && token != FOOTNOTE => Ok(()),
            Event::ScreenOpened {
                instance, handover, ..
            } if token == FOOTNOTE => self.open_note(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == FOOTNOTE => self.notes.shown(FOOTNOTE, instance),
            Event::ScreenClosed { instance, .. } if token == FOOTNOTE => {
                self.notes.close(instance);
                Ok(())
            }
            Event::ScreenOpened {
                instance, handover, ..
            } => self.open(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => {
                self.screens.shown(SCREEN, instance)?;
                self.follow_note(host, instance)
            }
            Event::ScreenClosed { instance, .. } => {
                self.close(host, instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if token == FOOTNOTE {
            return self
                .notes
                .top(token)
                .ok_or(Error::State)?
                .draw(host, &mut frame);
        }
        let (Some(view), Some(docs)) = (self.screens.top(token), &self.docs) else {
            return Err(Error::State);
        };
        view.draw(host, docs, &mut frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        if token == FOOTNOTE {
            let Some(note) = self.notes.top(token) else {
                return Ok(Handled::No);
            };
            let (handled, pressed) = note.input(host, &input)?;
            if let Pressed::Go { path, href } = pressed {
                self.follow = Some((path, href));
            }
            return Ok(handled);
        }
        let (handled, turned) = match (self.screens.top(token), &self.docs) {
            (Some(view), Some(docs)) => {
                let handled = view.input(
                    host,
                    (docs, self.library.as_ref()),
                    self.ui.as_ref(),
                    &input,
                )?;
                (handled, view.take_turned())
            }
            _ => return Ok(Handled::No),
        };
        if turned {
            self.start_quiet(host);
        }
        Ok(handled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Beta 3 saved the place 2 seconds after the last turn.
    #[test]
    fn the_quiet_time_is_beta_3s_and_the_tokens_differ() {
        assert_eq!(QUIET_MS, 2_000);
        assert!(WorkSpec::new(QUIET_KEY, WORK_DUE).is_ok());
        let tokens = [SCREEN, SUSPEND, STOPPING, WORK_DUE];
        for (i, a) in tokens.iter().enumerate() {
            assert!(tokens[i + 1..].iter().all(|b| a != b));
        }
    }
}
