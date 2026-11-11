//! The interface-7 reader (entry slice): what the reader declares reaches the
//! host (settings, the six screens and the slots they fill, actions, menu
//! entries, key bindings, the events heard and the three stages). The screens
//! have no view yet: an opened screen draws nothing and takes no input, and
//! the plugin is not in the app's `NATIVE_PLUGINS` until they have one
//! (`PORTING-7.md`). What the reader decides without a host is in the
//! `native_*` models, tested without one.
use crate::{
    declarations::{SCREEN, SETTING_CHANGED, STOPPING, SUSPEND, TOC, WORK_DUE},
    native_handsfree::{self, Due, Hands, Kind},
    native_pages,
    native_toc_view::{Outcome, TocView},
    native_view::View,
};
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
    /// The timer that turns the pages by itself (`native_handsfree`).
    hands: Hands,
    /// The host has no work for the reader: not asked again.
    no_work: bool,
    screens: Screens<View>,
    tocs: Screens<TocView>,
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
        self.open_work(host);
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

    /// The work service, opened at the first need; a host without it is asked once.
    fn open_work(&mut self, host: &Host<'_>) {
        if self.work.is_none() && !self.no_work {
            self.work = Work::open(host).ok();
            self.no_work = self.work.is_none();
        }
    }

    /// The action `reader.auto-turn` ran: the timer starts or stops for the
    /// reading screen shown.
    fn auto_turn_toggle(&mut self, host: &Host<'_>) {
        self.open_work(host);
        let top = self.screens.top_instance(SCREEN);
        self.hands.toggle(host, top, self.work.as_ref());
    }

    /// The reader is left (a screen opens, the device sleeps or stops, another
    /// action runs): the timer stops.
    fn auto_turn_leave(&mut self, host: &Host<'_>) {
        self.hands.leave(host, self.work.as_ref());
    }

    /// The timer is due: the screen turns the page, as the next-page action
    /// does, and the quiet time starts again; at the end of the book it stops.
    fn auto_turn_due(&mut self, host: &Host<'_>, now_ms: u64) {
        let (Some(docs), Some(ui)) = (&self.docs, &self.ui) else {
            return self.auto_turn_leave(host);
        };
        let (library, screens) = (self.library.as_ref(), &mut self.screens);
        let due = self
            .hands
            .due(host, (now_ms, self.work.as_ref()), |instance, input| {
                let view = screens.get(instance).ok_or(Error::State)?;
                view.input(host, (docs, library, ui), input)?;
                Ok(view.take_turned())
            });
        if due == Due::Turned {
            self.start_quiet(host);
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
            kinds::ActionRun,
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
            // The timer of the pages that turn by themselves (a quiet run: the
            // host refuses messages in it).
            Event::WorkDue(due) if token == WORK_DUE && due.key() == native_handsfree::KEY => {
                self.auto_turn_due(host, due.now_ms());
                Ok(())
            }
            // The action `reader.auto-turn`; any other action of the reader stops it.
            Event::ActionRun { .. } if token == native_handsfree::action_token() => {
                self.auto_turn_toggle(host);
                Ok(())
            }
            Event::ActionRun { .. } if native_handsfree::is_reader_action(token) => {
                self.auto_turn_leave(host);
                Ok(())
            }
            // The quiet time has come, or the device sleeps or stops.
            Event::WorkDue(_) | Event::Suspend { .. } | Event::Stopping { .. }
                if matches!(token, WORK_DUE | SUSPEND | STOPPING) =>
            {
                if token != WORK_DUE {
                    self.auto_turn_leave(host);
                }
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
            } => {
                // A screen opened over the reading screen: the pages stop turning.
                self.auto_turn_leave(host);
                match token {
                    SCREEN => self.open(host, instance, handover),
                    TOC => self.open_toc(host, instance, handover),
                    _ => Ok(()),
                }
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => match token {
                SCREEN => {
                    self.screens.shown(SCREEN, instance)?;
                    if let (Some(view), Some(docs)) = (self.screens.get(instance), &self.docs) {
                        view.shown(host, docs);
                    }
                    Ok(())
                }
                TOC => self.tocs.shown(TOC, instance),
                _ => Ok(()),
            },
            Event::ScreenClosed { instance, .. } => {
                match token {
                    SCREEN => {
                        self.hands.closed(host, instance, self.work.as_ref());
                        self.close(host, instance);
                    }
                    TOC => self.close_toc(host, instance),
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
        let (Some(view), Some(docs)) = (self.screens.top(token), &self.docs) else {
            return Err(Error::State);
        };
        view.draw(host, docs, &mut frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        if token == TOC {
            return self.toc_input(host, &input);
        }
        if token == SCREEN {
            // The timer sees the input first: it stops, or counts anew.
            if native_handsfree::classify(&input) == Kind::Toggle {
                self.open_work(host);
            }
            let top = self.screens.top_instance(token);
            if let Some(handled) = self.hands.input(host, top, &input, self.work.as_ref()) {
                return Ok(handled);
            }
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
