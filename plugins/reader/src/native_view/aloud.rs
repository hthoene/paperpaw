//! Reading aloud on the reading screen: the [`Scene`] `native_speech` runs
//! on (the page on show, the host's speech commands of `docs/abi/ui.md`
//! section 7, the notices and the underline) and what the screen does with
//! it: the action `reader.read-aloud` starts it, any other action or gesture
//! pauses it, the book closing or the screen being left ends it, and the
//! host's `event.spoken` goes on to the next sentence.
use super::{Body, Open, Setup, View, integer, redraw};
use crate::{
    native_aloud::timer_minutes,
    native_speech::{self, Options, Pages, Place, Scene, Speaker},
};
use core::ops::Range;
use paperpaw_sdk::{
    Handled,
    layout::Page,
    native::{
        Error, Host,
        callbacks::Input,
        clock,
        commands::{CommandSender, RedrawMode},
        document::Documents,
        events::SpeechStatus,
        formatting,
        library::Library,
        settings::{self, SettingValue},
        ui::{Notify, SpeechRequest},
    },
};
use paperpaw_ui_kit::mem::buffer;

/// The action that starts reading aloud (`declarations::ACTIONS`).
const READ_ALOUD: &str = "reader.read-aloud";

/// The settings of reading aloud.
const SPEED: &str = "reader.aloud-speed";
const HIGHLIGHT: &str = "reader.aloud-highlight";
const TURN: &str = "reader.aloud-turn-pages";
const TIMER: &str = "reader.aloud-timer";

/// Bytes of the speech command around its text (the frame, the bitmap, the
/// cookie, the rate and the flags) with room to spare.
const COMMAND_EXTRA: usize = 128;

/// A setting that is on unless it says it is off (the settings default on).
fn on(host: &Host<'_>, key: &str) -> bool {
    let mut bytes = [0; 32];
    !matches!(
        settings::read_setting(host, key, &mut bytes),
        Ok(SettingValue::Bool(false))
    )
}

/// The page on show and the host, for the length of one call.
pub(super) struct Stage<'a, 'h> {
    open: &'a mut Open,
    host: &'a Host<'h>,
    docs: &'a Documents,
    library: Option<&'a Library>,
    /// The reading settings and the display, read once for the call.
    setup: Option<Setup>,
}

impl Pages for Stage<'_, '_> {
    fn with_page<R>(&mut self, start: Place, f: impl FnOnce(&Page) -> R) -> Result<R, Error> {
        if self.setup.is_none() {
            self.setup = Some(Setup::read(self.host)?);
        }
        let setup = self.setup.as_ref().ok_or(Error::State)?;
        if start == self.open.at {
            if self.open.page.is_none() {
                let page = self.open.lay(self.host, self.docs, setup, start)?;
                self.open.page = Some(page);
                self.open.laid = Some(setup.key());
            }
            return self.open.page.as_ref().map(f).ok_or(Error::State);
        }
        let page = self.open.lay(self.host, self.docs, setup, start)?;
        Ok(f(&page))
    }
}

impl Stage<'_, '_> {
    /// The page on show is known by where its layout starts, as the sentences
    /// name their pages (`native_speech`): a place the layout moved on from (a
    /// text the format starts after a header, a section that ended) becomes that
    /// start. A page that cannot be laid out leaves the place as it is.
    fn align(&mut self) {
        let at = self.open.at;
        if let Ok(start) = self.with_page(at, |page| (page.start.section, page.start.offset))
            && start != at
        {
            self.open.at = start;
        }
    }
}

impl Scene for Stage<'_, '_> {
    fn shown(&self) -> Place {
        self.open.at
    }

    fn turn_to(&mut self, start: Place) -> Result<bool, Error> {
        let at = self.open.at;
        let end = self.with_page(at, |page| (page.end.section, page.end.offset))?;
        if end != start || !self.open.next()? {
            return Ok(false);
        }
        self.open.after_turn(self.host, (self.docs, self.library))?;
        Ok(true)
    }

    fn speak(&mut self, text: &str, cookie: u32, queue: bool) -> Result<(), Error> {
        let rate = match integer(self.host, SPEED).unwrap_or(0) {
            0 => 100,
            value => value.clamp(50, 300) as u16,
        };
        // The book's language; a tag the host cannot take is the system's.
        let language = self.open.language.as_str();
        let mut request = SpeechRequest::new(cookie, text)?;
        if let Ok(with) = request.language(language) {
            request = with;
        }
        request = request.rate(rate)?;
        if queue {
            request = request.queue();
        }
        let mut scratch = buffer(text.len() + language.len() + COMMAND_EXTRA)?;
        CommandSender::new(self.host, &mut scratch).speak(request)
    }

    fn stop(&mut self) {
        let mut scratch = [0; 64];
        CommandSender::new(self.host, &mut scratch)
            .stop_speaking()
            .ok();
    }

    fn mark(&mut self, words: Option<Range<usize>>) {
        // The page is drawn again for each sentence, not more than that.
        if self.open.mark != words {
            self.open.mark = words;
            redraw(self.host, RedrawMode::Fast).ok();
        }
    }

    fn say(&mut self, key: &'static str) {
        let mut text = [0; 256];
        let Ok(text) = formatting::copy_message(self.host, key, &mut text) else {
            return;
        };
        let Ok(note) = Notify::new(text) else {
            return;
        };
        let mut scratch = [0; 320];
        CommandSender::new(self.host, &mut scratch)
            .notify(note)
            .ok();
    }

    fn options(&self) -> Options {
        let mut bytes = [0; 32];
        let minutes = match settings::read_setting(self.host, TIMER, &mut bytes) {
            Ok(SettingValue::Text(choice)) => timer_minutes(choice),
            _ => None,
        };
        Options {
            turn: on(self.host, TURN),
            highlight: on(self.host, HIGHLIGHT),
            minutes,
        }
    }

    fn now_us(&self) -> Option<u64> {
        clock::micros(self.host).ok()
    }
}

impl View {
    /// This screen's reading aloud answers to the cookies of `instance`.
    pub fn bind(&mut self, instance: u32) {
        self.speaker = Speaker::new(instance);
    }

    /// Runs `f` with the speaker and the screen's stage, if a book is open.
    fn staged<R>(
        &mut self,
        host: &Host<'_>,
        (docs, library): (&Documents, Option<&Library>),
        f: impl FnOnce(&mut Speaker, &mut Stage<'_, '_>) -> R,
    ) -> Option<R> {
        let Body::Open(open) = &mut self.body else {
            return None;
        };
        let mut stage = Stage {
            open,
            host,
            docs,
            library,
            setup: None,
        };
        Some(f(&mut self.speaker, &mut stage))
    }

    /// What an input does to reading aloud: any action or gesture pauses it
    /// (and does nothing else), "Read aloud" starts it; `None` for an input
    /// that is not of reading aloud.
    pub(super) fn aloud_input(
        &mut self,
        host: &Host<'_>,
        shared: (&Documents, Option<&Library>),
        input: &Input<'_>,
    ) -> Option<Result<Handled, Error>> {
        if self.speaker.active() && native_speech::pauses(input) {
            self.staged(host, shared, |speaker, stage| speaker.pause(stage, true))?;
            return Some(Ok(Handled::Yes));
        }
        let Input::Action {
            action: READ_ALOUD, ..
        } = input
        else {
            return None;
        };
        self.start_aloud(host, shared);
        Some(Ok(Handled::Yes))
    }

    /// The key of the book on show, open or still prepared
    /// (`native_start::book_key`); none without one.
    #[must_use]
    pub fn book_key(&self) -> Option<u64> {
        match &self.body {
            Body::Open(open) => Some(crate::native_start::book_key(&open.path)),
            Body::Preparing(book) => Some(crate::native_start::book_key(&book.found.path)),
            Body::Message(_) => None,
        }
    }

    /// Starts reading aloud on the page on show; for a book still prepared it
    /// is kept until the book is open (`take_aloud_kept`); nothing without a
    /// book.
    pub fn start_aloud(&mut self, host: &Host<'_>, shared: (&Documents, Option<&Library>)) {
        if self.preparing() {
            self.aloud_kept = true;
            return;
        }
        self.staged(host, shared, |speaker, stage| {
            stage.align();
            speaker.start(stage);
        });
    }

    /// Whether *Read aloud* was kept while the book was prepared.
    #[must_use]
    pub const fn aloud_kept(&self) -> bool {
        self.aloud_kept
    }

    /// Whether *Read aloud* kept while the book was prepared is due now: the
    /// book is open (asked once; a book that failed forgets it). A book still
    /// prepared keeps it.
    pub fn take_aloud_kept(&mut self) -> bool {
        if self.preparing() {
            return false;
        }
        core::mem::take(&mut self.aloud_kept) && matches!(self.body, Body::Open(_))
    }

    /// Pauses reading aloud (with the notice if `notice`): the layout changed.
    pub(super) fn pause_aloud(
        &mut self,
        host: &Host<'_>,
        shared: (&Documents, Option<&Library>),
        notice: bool,
    ) {
        if self.speaker.active() {
            self.staged(host, shared, |speaker, stage| speaker.pause(stage, notice));
        }
    }

    /// Ends reading aloud and forgets where it was: the book closed or the
    /// screen was left.
    pub fn stop_aloud(&mut self, host: &Host<'_>, shared: (&Documents, Option<&Library>)) {
        self.staged(host, shared, |speaker, stage| speaker.stop(stage));
    }

    /// Whether `cookie` is of a sentence this screen handed over.
    #[must_use]
    pub fn owns_speech(&self, cookie: u32) -> bool {
        self.speaker.owns(cookie)
    }

    /// Whether the book is being read aloud.
    #[must_use]
    pub const fn reading_aloud(&self) -> bool {
        self.speaker.active()
    }

    /// The host says a sentence was spoken (or could not be).
    pub fn spoken(
        &mut self,
        host: &Host<'_>,
        shared: (&Documents, Option<&Library>),
        cookie: u32,
        status: SpeechStatus,
    ) {
        self.staged(host, shared, |speaker, stage| {
            stage.align();
            speaker.spoken(stage, cookie, status);
        });
    }
}
