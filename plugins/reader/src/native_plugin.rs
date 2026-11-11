//! The interface-7 reader (entry slice): what the reader declares reaches the
//! host (settings, the six screens and the slots they fill, actions, menu
//! entries, key bindings, the events heard and the three stages). The screens
//! have no view yet: an opened screen draws nothing and takes no input, and
//! the plugin is not in the app's `NATIVE_PLUGINS` until they have one
//! (`PORTING-7.md`). What the reader decides without a host is in the
//! `native_*` models, tested without one.
use crate::{
    declarations::{
        ANNOTATIONS, FOOTNOTE, SCREEN, SETTING_CHANGED, STOPPING, SUSPEND, TOC, WORK_DUE,
    },
    native_annotations_view::{AnnotationsView, Outcome as NotesOutcome},
    native_footnote::{Footnote, Pressed},
    native_index, native_pages,
    native_toc_view::{Outcome, TocView},
    native_view::View,
};
use alloc::string::String;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
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
const BATCHES: [&[&[u8]]; 8] = [
    image::SETTINGS,
    image::SCREENS,
    image::STATUS,
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
    tocs: Screens<TocView>,
    /// The annotations screens.
    annotations: Screens<AnnotationsView>,
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
        self.screens.open(instance, view)?;
        self.start_count(host);
        Ok(())
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

    /// The table of contents opened: its book is opened with it.
    fn open_toc(
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
        let (Some(ui), Some(docs)) = (&self.ui, &self.docs) else {
            return Err(Error::State);
        };
        let shared = self.screens.top(SCREEN).and_then(|v| v.doc_id());
        let view = TocView::open(host, ui, docs, (handover, shared))?;
        self.tocs.open(instance, view)
    }

    /// The table of contents closed: its document is given back.
    fn close_toc(&mut self, host: &Host<'_>, instance: paperpaw_sdk::native::ui::Instance) {
        if let (Some(view), Some(docs)) = (self.tocs.get(instance), &self.docs) {
            view.close(host, docs);
        }
        self.tocs.close(instance);
    }

    /// An input of the table of contents; a choice goes to the reading screen
    /// below and closes this one.
    fn toc_input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let (Some(view), Some(docs)) = (self.tocs.top(TOC), &self.docs) else {
            return Ok(Handled::No);
        };
        let (handled, outcome) = view.input(host, docs, input)?;
        if let Outcome::Go(place) = outcome {
            if let Some(reading) = self.screens.top(SCREEN) {
                reading.go_to((place.section, place.offset))?;
                if reading.take_turned() {
                    self.start_quiet(host);
                }
            }
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).back()?;
        }
        Ok(handled)
    }

    /// The annotations opened: their notes are read with it.
    fn open_notes(
        &mut self,
        host: &Host<'_>,
        instance: paperpaw_sdk::native::ui::Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        // Without the library there are no notes to list: the screen says so.
        if self.library.is_none() {
            self.library = Library::open(host).ok();
        }
        let Some(ui) = &self.ui else {
            return Err(Error::State);
        };
        let view = AnnotationsView::open(host, ui, self.library.as_ref(), handover)?;
        self.annotations.open(instance, view)
    }

    /// An input of the annotations; a choice goes to the reading screen below
    /// and closes this one.
    fn notes_input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.annotations.top(ANNOTATIONS) else {
            return Ok(Handled::No);
        };
        let (handled, outcome) = view.input(host, self.library.as_ref(), input)?;
        if let NotesOutcome::Go(place) = outcome {
            if let Some(reading) = self.screens.top(SCREEN) {
                reading.go_to((place.section, place.offset))?;
                if reading.take_turned() {
                    self.start_quiet(host);
                }
            }
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).back()?;
        }
        Ok(handled)
    }

    /// Opens the annotations of the book on show: the same hand-over as the
    /// table of contents (where the book is and where the reader is in it).
    fn show_notes(&mut self, host: &Host<'_>) -> Result<Handled, Error> {
        let (Some(view), Some(ui)) = (self.screens.top(SCREEN), &self.ui) else {
            return Ok(Handled::No);
        };
        let Some(params) = view.toc_params() else {
            return Ok(Handled::No);
        };
        crate::native_annotations_view::show(host, ui, &params)?;
        Ok(Handled::Yes)
    }

    /// A setting changed: every reading screen lays its page out again (a
    /// setting of the layout) or draws it again (a setting of the ink). A refusal
    /// of one screen costs the others nothing.
    fn setting_changed(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        let layout = native_pages::affects_layout(key);
        if !layout && !native_pages::affects_ink(key) {
            return Ok(());
        }
        let Some(docs) = &self.docs else {
            return Ok(());
        };
        let mut result = Ok(());
        for view in self.screens.all() {
            let done = if layout {
                view.reflow(host, docs)
            } else {
                view.repaint(host)
            };
            if let Err(error) = done {
                result = Err(error);
            }
        }
        if layout {
            self.start_count(host);
        }
        result
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
        self.declare(host, QUIET_KEY, QUIET_MS);
    }

    /// The pages of the reading screen on top are counted in the background:
    /// a step now, then one after the other.
    fn start_count(&mut self, host: &Host<'_>) {
        if self.screens.top(SCREEN).is_some_and(|view| view.counting()) {
            self.declare(host, native_index::WORK_KEY, native_index::DELAY_MS);
        }
    }

    /// One step of the count of the pages; the next is declared while pages remain.
    fn count_step(&mut self, host: &Host<'_>) {
        let more = match (self.screens.top(SCREEN), &self.docs) {
            (Some(view), Some(docs)) => view.count_step(host, docs),
            _ => None,
        };
        if let Some(delay) = more {
            self.declare(host, native_index::WORK_KEY, delay);
        }
    }

    /// Work of key `key` after `ms` milliseconds (replacing one of that key).
    fn declare(&mut self, host: &Host<'_>, key: &str, ms: u32) {
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
        if let Ok(spec) = WorkSpec::new(key, WORK_DUE) {
            work.declare(host, &spec.after_ms(ms), &mut input, &mut out)
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
            kinds::WorkDue,
            kinds::SettingChanged
        ) else {
            return Ok(());
        };
        match event {
            // The quiet time has come, or the device sleeps or stops.
            Event::WorkDue(due) if token == WORK_DUE && due.key() == native_index::WORK_KEY => {
                self.count_step(host);
                Ok(())
            }
            Event::WorkDue(_) | Event::Suspend { .. } | Event::Stopping { .. }
                if matches!(token, WORK_DUE | SUSPEND | STOPPING) =>
            {
                self.save_now(host);
                Ok(())
            }
            // A reading setting changed: the pages on show are laid out again (or
            // drawn again where only the ink changed).
            Event::SettingChanged { key, .. } if token == SETTING_CHANGED => {
                self.setting_changed(host, key)
            }
            // The reading screen and the table of contents have views; the
            // other screens come with their slices (`PORTING-7.md`).
            Event::ScreenOpened {
                instance, handover, ..
            } => match token {
                SCREEN => self.open(host, instance, handover),
                TOC => self.open_toc(host, instance, handover),
                ANNOTATIONS => self.open_notes(host, instance, handover),
                FOOTNOTE => self.open_note(host, instance, handover),
                _ => Ok(()),
            },
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => match token {
                SCREEN => {
                    self.screens.shown(SCREEN, instance)?;
                    if let (Some(view), Some(docs)) = (self.screens.get(instance), &self.docs) {
                        view.shown(host, docs);
                    }
                    self.follow_note(host, instance)
                }
                TOC => self.tocs.shown(TOC, instance),
                ANNOTATIONS => self.annotations.shown(ANNOTATIONS, instance),
                FOOTNOTE => self.notes.shown(FOOTNOTE, instance),
                _ => Ok(()),
            },
            Event::ScreenClosed { instance, .. } => {
                match token {
                    SCREEN => self.close(host, instance),
                    TOC => self.close_toc(host, instance),
                    ANNOTATIONS => self.annotations.close(instance),
                    FOOTNOTE => self.notes.close(instance),
                    _ => {}
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if token == TOC {
            let view = self.tocs.top(token).ok_or(Error::State)?;
            return view.draw(host, &mut frame);
        }
        if token == ANNOTATIONS {
            let view = self.annotations.top(token).ok_or(Error::State)?;
            return view.draw(host, &mut frame);
        }
        if token == FOOTNOTE {
            let note = self.notes.top(token).ok_or(Error::State)?;
            return note.draw(host, &mut frame);
        }
        let (Some(view), Some(docs)) = (self.screens.top(token), &self.docs) else {
            return Err(Error::State);
        };
        view.draw(host, (docs, self.ui.as_ref()), &mut frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        if token == TOC {
            return self.toc_input(host, &input);
        }
        if token == ANNOTATIONS {
            return self.notes_input(host, &input);
        }
        if token == SCREEN
            && matches!(
                input,
                Input::Action {
                    action: "reader.show-annotations",
                    ..
                }
            )
        {
            return self.show_notes(host);
        }
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
        let (handled, turned) = match (self.screens.top(token), &self.docs, &self.ui) {
            (Some(view), Some(docs), Some(ui)) => {
                let handled = view.input(host, (docs, self.library.as_ref(), ui), &input)?;
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
        let tokens = [SCREEN, SUSPEND, STOPPING, WORK_DUE, SETTING_CHANGED];
        for (i, a) in tokens.iter().enumerate() {
            assert!(tokens[i + 1..].iter().all(|b| a != b));
        }
    }
}
