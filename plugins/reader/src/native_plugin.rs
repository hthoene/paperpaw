//! The interface-7 reader (entry slice): what the reader declares reaches the
//! host (settings, the six screens and the slots they fill, actions, menu
//! entries, key bindings, the events heard and the three stages). The screens
//! have no view yet: an opened screen draws nothing and takes no input, and
//! the plugin is not in the app's `NATIVE_PLUGINS` until they have one
//! (`PORTING-7.md`). What the reader decides without a host is in the
//! `native_*` models, tested without one.
use crate::{
    declarations::{
        ANNOTATIONS, BOOK_MOVED, DOCUMENT_CHANGED, FOOTNOTE, SCREEN, SEARCH, SEARCH_WORK,
        SETTING_CHANGED, SKIM, SPOKEN, STACK_CHANGED, STARTED, STOPPING, SUSPEND, TOC, WORK_DUE,
    },
    native_annotations_view::{AnnotationsView, Outcome as NotesOutcome},
    native_footnote::{Footnote, Pressed},
    native_handsfree::{self, Due, Hands, Kind},
    native_index, native_pages,
    native_search::MAX_NEEDLE,
    native_search_view::{Outcome as SearchOutcome, SearchView},
    native_skim_view,
    native_skim_view::SkimView,
    native_toc_view::{Outcome, TocView},
    native_view::{Handover, View, read_handover},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        commands::RedrawMode,
        document::Documents,
        events::{AnswerOutcome, Answered, Event, Incoming, SpeechStatus, kinds},
        library::Library,
        registrations,
        reply::Reply,
        ui::{Instance, Question, RequestId as AskId, Ui, UiCursor},
        work::{Work, WorkSpec},
    },
};
use paperpaw_ui_kit::{
    Messages, Screens,
    mem::{self, SERVICE_BUFFER, buffer},
    read_event, text,
};

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
/// The key of the work that reads the next blocks of a search, and the pause
/// between two steps (the page is drawn between them).
const SEARCH_KEY: &str = "search";
const SEARCH_STEP_MS: u32 = 50;

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
    /// The annotations screens.
    annotations: Screens<AnnotationsView>,
    searches: Screens<SearchView>,
    /// The question for the text to search for, while it is open.
    asking: Option<AskId>,
    /// The text answered, until the results screen it opened takes it.
    query: Option<String>,
    /// The notes over the pages.
    notes: Screens<Footnote>,
    /// The skim screens over the pages.
    skims: Screens<SkimView>,
    /// Reading screens that gave their document back for another book's (the
    /// host lets a plugin keep few documents open, one on the X4), with the
    /// book they showed (path, root code): each opens it again, at its saved
    /// place, when it is on top again (`wake`).
    resting: Vec<(Instance, String, &'static str)>,
    /// *Go to the note* pressed: the book and the link, followed when its
    /// reading screen is shown again.
    follow: Option<(String, String)>,
    /// The book read last was asked to open at the start (its key,
    /// `native_start::book_key`) and no reading screen was shown since.
    reopening: Option<u64>,
    /// When *Read aloud* came while that book was opening: it is read aloud once
    /// its screen is shown (Beta 3 opened the book before any input). Dropped by
    /// any other move (`native_start::reads_aloud`).
    aloud_asked: Option<u64>,
}

/// The callback token of the action `reader.read-aloud`.
const READ_ALOUD_TOKEN: u32 = native_handsfree::token_of(b"read-aloud");

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
        let found = match handover {
            Some(handover) => read_handover(host, ui, handover).unwrap_or_default(),
            None => Handover::default(),
        };
        let library = self.library.as_ref();
        // The screen opened again (the reader asked to open while it shows a
        // book): the book it shows stays open; another book's document is
        // given back first, so the host has room for the new one.
        if let Some(view) = self.screens.get(instance) {
            if view.keeps(&found)? {
                self.start_count(host);
                return Ok(());
            }
            view.close(host, (docs, library));
        }
        rest(
            host,
            (docs, library),
            (&mut self.screens, &mut self.resting),
            instance,
        )?;
        let mut view = View::open(host, (docs, library), found).or_else(View::when_full)?;
        view.bind(instance.get());
        self.resting.retain(|(at, ..)| *at != instance);
        self.screens.open(instance, view)?;
        self.start_count(host);
        Ok(())
    }

    /// A reading screen that gave its document back is on top again: it opens
    /// its book again, at its saved place (the others' documents are given
    /// back first).
    fn wake(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let Some(at) = self.resting.iter().position(|(i, ..)| *i == instance) else {
            return Ok(());
        };
        let (_, path, root) = self.resting.swap_remove(at);
        let Some(docs) = &self.docs else {
            return Err(Error::State);
        };
        let library = self.library.as_ref();
        rest(
            host,
            (docs, library),
            (&mut self.screens, &mut self.resting),
            instance,
        )?;
        let mut view = Handover::of_book(path, root)
            .and_then(|found| View::open(host, (docs, library), found))
            .or_else(View::when_full)?;
        view.bind(instance.get());
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

    /// A screen of the reader opened: over the reading screen the pages stop
    /// turning, then the screen's own view is opened.
    fn screen_opened(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: paperpaw_sdk::native::ui::Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        self.auto_turn_leave(host);
        // Another screen of the reader: the user went elsewhere first.
        if token != SCREEN {
            self.aloud_asked = None;
        }
        match token {
            SCREEN => self.open(host, instance, handover),
            TOC => self.open_toc(host, instance, handover),
            ANNOTATIONS => self.open_notes(host, instance, handover),
            SEARCH => self.open_search(host, instance, handover),
            FOOTNOTE => self.open_note(host, instance, handover),
            SKIM => self.open_skim(host, instance, handover),
            _ => Ok(()),
        }
    }

    /// A screen of the reader came to the top.
    fn screen_shown(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: paperpaw_sdk::native::ui::Instance,
        now_ms: u64,
    ) -> Result<(), Error> {
        match token {
            SCREEN => self.reading_shown(host, instance, now_ms),
            TOC => self.tocs.shown(TOC, instance),
            SEARCH => self.searches.shown(SEARCH, instance),
            ANNOTATIONS => self.annotations.shown(ANNOTATIONS, instance),
            FOOTNOTE => self.notes.shown(FOOTNOTE, instance),
            SKIM => self.skims.shown(SKIM, instance),
            _ => Ok(()),
        }
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

    /// The skim screen opened over the page.
    fn open_skim(
        &mut self,
        host: &Host<'_>,
        instance: paperpaw_sdk::native::ui::Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let view = SkimView::open(host, ui, handover)?;
        self.skims.open(instance, view)
    }

    /// An input of the skim screen. The move it asks for belongs to the reading
    /// screen, which does not carry it out yet (`PORTING-7.md`, R6b).
    fn skim_input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.skims.top(SKIM) else {
            return Ok(Handled::No);
        };
        let (handled, _move) = view.input(host, input)?;
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

    /// The search screen opened: the text answered is searched for.
    fn open_search(
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
        let query = self.query.take().unwrap_or_default();
        let (Some(ui), Some(docs)) = (&self.ui, &self.docs) else {
            return Err(Error::State);
        };
        let shared = self.screens.top(SCREEN).and_then(|v| v.doc_id());
        let view = SearchView::open(host, ui, docs, (handover, shared), &query)?;
        let searching = view.searching();
        self.searches.open(instance, view)?;
        if searching {
            self.declare_work(host, (SEARCH_KEY, SEARCH_WORK), SEARCH_STEP_MS);
        }
        Ok(())
    }

    /// The work of the search is due: the next blocks of the book are read by
    /// every search under way, and the page is drawn where it changed.
    fn search_due(&mut self, host: &Host<'_>) {
        let Some(docs) = &self.docs else {
            return;
        };
        let (mut redraw, mut again) = (false, false);
        for view in self.searches.all() {
            match view.step(host, docs) {
                Ok(stepped) => {
                    redraw |= stepped.redraw;
                    again |= stepped.again;
                }
                // A step that fails ends its search, and the screen says so.
                Err(error) => {
                    view.fail(error);
                    redraw = true;
                }
            }
        }
        if redraw {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch)
                .queue_redraw(RedrawMode::Partial)
                .ok();
        }
        if again {
            self.declare_work(host, (SEARCH_KEY, SEARCH_WORK), SEARCH_STEP_MS);
        }
    }

    /// An input of the search results; a choice goes to the reading screen
    /// below and closes this one.
    fn search_input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.searches.top(SEARCH) else {
            return Ok(Handled::No);
        };
        let (handled, outcome) = view.input(host, input)?;
        if let SearchOutcome::Go(place) = outcome {
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

    /// Asks for the text to search for (the keyboard answers).
    fn ask_search(&mut self, host: &Host<'_>) -> Result<Handled, Error> {
        let (Some(view), Some(ui)) = (self.screens.top(SCREEN), &self.ui) else {
            return Ok(Handled::No);
        };
        if view.doc_id().is_none() {
            return Ok(Handled::No);
        }
        let title = Messages::new()?.copy(host, "reader.search.prompt")?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = ui.ask(host, Question::text(&title, "")?, &mut input, &mut out)?;
        self.asking = Some(request);
        Ok(Handled::Yes)
    }

    /// An answer to the question for the text: the search screen opens for it.
    fn answered(&mut self, host: &Host<'_>, answer: &Answered) -> Result<(), Error> {
        if answer.outcome() == AnswerOutcome::Typing || self.asking != Some(answer.request()) {
            return Ok(());
        }
        self.asking = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let Some(ui) = &self.ui else {
            return Err(Error::State);
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let part = ui.text_read(host, answer.request().text(), 0, &mut input, &mut out)?;
        // What a query counts is its first characters: the screen shows the
        // text it searches for.
        let typed = part.data();
        let query = paperpaw_ui_kit::text::copy(cut(typed.trim(), MAX_NEEDLE))?;
        if query.is_empty() {
            return Ok(());
        }
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        let Some(book) = view.toc_params() else {
            return Ok(());
        };
        let mut params = Vec::new();
        params.try_reserve_exact(5).map_err(|_| Error::NoMemory)?;
        params.extend(book);
        params.push(("query", paperpaw_ui_kit::text::copy(&query)?));
        self.query = Some(query);
        crate::native_annotations_view::open_screen(host, ui, "reader.search-results", &params)
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

    /// Ends the reading aloud of every reading screen.
    fn stop_speech(&mut self, host: &Host<'_>) {
        let Some(docs) = &self.docs else {
            return;
        };
        let library = self.library.as_ref();
        for view in self.screens.all() {
            if view.reading_aloud() {
                view.stop_aloud(host, (docs, library));
            }
        }
    }

    /// *Read aloud* kept while a book was prepared starts on its reading screen
    /// now open, only where that screen is on top of the stack; a screen covered
    /// keeps it until it is shown again (`reading_shown`).
    fn aloud_when_on_top(&mut self, host: &Host<'_>) {
        // The stack is asked only for a screen that kept it (its buffers would
        // add to the peak of opening a book).
        if !self.screens.all().any(|view| view.aloud_kept()) {
            return;
        }
        let Some(docs) = &self.docs else {
            return;
        };
        let Ok(Some(top)) = self.stack_top(host) else {
            return;
        };
        let library = self.library.as_ref();
        if let Some(view) = self.screens.get(top)
            && view.take_aloud_kept()
        {
            view.start_aloud(host, (docs, library));
        }
    }

    /// The instance on top of the whole screen stack (any plugin's).
    fn stack_top(
        &self,
        host: &Host<'_>,
    ) -> Result<Option<paperpaw_sdk::native::ui::Instance>, Error> {
        let Some(ui) = &self.ui else {
            return Ok(None);
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let rows = ui.stack(host, UiCursor::start(), &mut input, &mut out)?;
        match rows.into_iter().next() {
            Some(row) => Ok(Some(row?.instance())),
            None => Ok(None),
        }
    }

    /// The screen stack changed: where the reading screen is no longer on top,
    /// nothing is read aloud (the table of contents, a menu, another plugin's
    /// screen, the library). Asked only while something is read aloud.
    fn stacked(&mut self, host: &Host<'_>) -> Result<(), Error> {
        // *Read aloud* kept for the book opening at the start goes with any other
        // screen on top of its reading screen.
        let kept = self.aloud_asked.is_some() && self.screens.top_instance(SCREEN).is_some();
        if !kept && !self.screens.all().any(|view| view.reading_aloud()) {
            return Ok(());
        }
        if self.ui.is_none() {
            return Ok(());
        }
        let top = self.stack_top(host)?;
        if top.is_none() || top != self.screens.top_instance(SCREEN) {
            self.aloud_asked = None;
            self.stop_speech(host);
        }
        Ok(())
    }

    /// A sentence was spoken: the screen that handed it over goes on, which can
    /// turn its page (the place is then written after the quiet time).
    fn spoken(&mut self, host: &Host<'_>, cookie: u32, status: SpeechStatus) {
        let Some(docs) = &self.docs else {
            return;
        };
        let library = self.library.as_ref();
        let mut turned = false;
        for view in self.screens.all() {
            if view.owns_speech(cookie) {
                view.spoken(host, (docs, library), cookie, status);
                turned |= view.take_turned();
            }
        }
        if turned {
            self.start_quiet(host);
        }
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
        self.declare_work(host, (QUIET_KEY, WORK_DUE), QUIET_MS);
    }

    /// The pages of the reading screen on top are counted in the background:
    /// a step now, then one after the other.
    fn start_count(&mut self, host: &Host<'_>) {
        if self
            .screens
            .top(SCREEN)
            .is_some_and(|view| view.counting() || view.marks_due())
        {
            self.declare_work(
                host,
                (native_index::WORK_KEY, WORK_DUE),
                native_index::DELAY_MS,
            );
        }
    }

    /// One step of the count of the pages; the next is declared while pages remain.
    fn count_step(&mut self, host: &Host<'_>) {
        let more = match (self.screens.top(SCREEN), &self.docs) {
            (Some(view), Some(docs)) => {
                // The first page's marks are asked here, after it is shown.
                view.marks_step(host, docs);
                view.count_step(host, docs)
            }
            _ => None,
        };
        if let Some(delay) = more {
            self.declare_work(host, (native_index::WORK_KEY, WORK_DUE), delay);
        }
    }

    /// Declares the work `(key, token)` to be due after `after` milliseconds.
    fn declare_work(&mut self, host: &Host<'_>, (key, token): (&str, u32), after: u32) {
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
        if let Ok(spec) = WorkSpec::new(key, token) {
            work.declare(host, &spec.after_ms(after), &mut input, &mut out)
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

    /// A book moved: the last book read follows it.
    fn moved(&mut self, host: &Host<'_>, from: &str, to: &str) -> Result<(), Error> {
        crate::native_start::follow(host, from, to);
        Ok(())
    }

    /// The reading screen is on top: its view is told, and a note it was asked to
    /// follow is shown.
    fn reading_shown(
        &mut self,
        host: &Host<'_>,
        instance: paperpaw_sdk::native::ui::Instance,
        now_ms: u64,
    ) -> Result<(), Error> {
        self.screens.shown(SCREEN, instance)?;
        self.wake(host, instance)?;
        let (asked, reopened) = (self.aloud_asked.take(), self.reopening.take());
        if let (Some(view), Some(docs)) = (self.screens.get(instance), &self.docs) {
            view.shown(host, docs);
            // Read aloud kept at the start for the book reopened, or kept while
            // the book was prepared and covered since it opened.
            if crate::native_start::reads_aloud(asked, reopened, now_ms, view.book_key())
                || view.take_aloud_kept()
            {
                view.start_aloud(host, (docs, self.library.as_ref()));
            }
        }
        // A book still prepared asks its state again: a hint the host lost
        // (`event.document_changed`) never leaves the screen waiting.
        if self
            .screens
            .get(instance)
            .is_some_and(|view| view.preparing())
        {
            self.document_changed(host);
        }
        self.follow_note(host, instance)
    }

    /// An action of the reader that reached the plugin, not a screen:
    /// `reader.auto-turn` starts or stops the timer and any other stops it; *Read
    /// aloud* before the book read last is on show is kept for it, and any other
    /// action drops that. The action may come before the start is told (and the
    /// book read last asked to open): it is kept while no screen shows a book,
    /// and read only for the book reopened, soon (`native_start::reads_aloud`).
    fn action_run(&mut self, host: &Host<'_>, token: u32, now_ms: u64) {
        if token == native_handsfree::action_token() {
            return self.auto_turn_toggle(host);
        }
        let no_book = !self.screens.all().any(|view| view.book().is_some());
        if token == READ_ALOUD_TOKEN && (self.reopening.is_some() || no_book) {
            self.aloud_asked = Some(now_ms);
            return;
        }
        self.aloud_asked = None;
        self.auto_turn_leave(host);
    }

    /// The start of the device (`Event::Started`): the book read last opens again where the setting
    /// asks for it.
    fn reopen_last(&mut self, host: &Host<'_>) -> Result<(), Error> {
        // A book is on show already (opened before the start was told): it
        // stays, and nothing opens over it.
        if self.screens.all().any(|view| view.book().is_some()) {
            return Ok(());
        }
        // A convenience at the start never ends the callback with an error (that
        // would drop what other plugins staged in it); only a refused allocation does.
        let opened = self.open_ui(host).and_then(|()| {
            let ui = self.ui.as_ref().ok_or(Error::State)?;
            crate::native_start::reopen_last(host, ui)
        });
        match opened {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Ok(book) => {
                self.reopening = book;
                Ok(())
            }
            Err(_) => Ok(()),
        }
    }

    /// A document's state moved (`event.document_changed`, a hint): the reading
    /// screens that wait for their book to be prepared ask its state; one that
    /// opened or failed is drawn again. A refusal on the way (no memory) is
    /// said on the screen: the callback ends well, so the redraw staged here
    /// is kept.
    fn document_changed(&mut self, host: &Host<'_>) {
        let Some(docs) = &self.docs else {
            return;
        };
        let library = self.library.as_ref();
        let mut redraw = false;
        for view in self.screens.all().filter(|view| view.preparing()) {
            // An error has set the screen's message already.
            redraw |= view.prepared(host, (docs, library)).unwrap_or(true);
        }
        if redraw {
            self.aloud_when_on_top(host);
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch)
                .queue_redraw(RedrawMode::Partial)
                .ok();
            self.start_count(host);
        }
    }

    /// The UI service, opened at the first need.
    fn open_ui(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    /// The screen closed: its document is given back.
    fn close(&mut self, host: &Host<'_>, instance: paperpaw_sdk::native::ui::Instance) {
        if let (Some(view), Some(docs)) = (self.screens.get(instance), &self.docs) {
            view.close(host, (docs, self.library.as_ref()));
        }
        self.screens.close(instance);
        self.resting.retain(|(at, ..)| *at != instance);
    }
}

/// Gives back the documents of the reading screens other than `instance`: the
/// host lets a plugin keep few open (one on the X4). Each keeps its book in
/// `resting` and opens it again when it is on top again.
fn rest(
    host: &Host<'_>,
    (docs, library): (&Documents, Option<&Library>),
    (screens, resting): (
        &mut Screens<View>,
        &mut Vec<(Instance, String, &'static str)>,
    ),
    instance: Instance,
) -> Result<(), Error> {
    for (other, view) in screens.all_with_instance() {
        if other == instance {
            continue;
        }
        let Some((path, root)) = view.book() else {
            continue;
        };
        let path = text::copy(path)?;
        mem::push(resting, (other, path, root))?;
        view.close(host, (docs, library));
    }
    Ok(())
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
            kinds::Started,
            kinds::BookMoved,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed,
            kinds::Suspend,
            kinds::Stopping,
            kinds::WorkDue,
            kinds::Answered,
            kinds::SettingChanged,
            kinds::Spoken,
            kinds::StackChanged,
            kinds::DocumentChanged
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
            Event::ActionRun { now_ms, .. } if native_handsfree::is_reader_action(token) => {
                self.action_run(host, token, now_ms);
                Ok(())
            }
            Event::Started { .. } if token == STARTED => self.reopen_last(host),
            Event::DocumentChanged { .. } if token == DOCUMENT_CHANGED => {
                self.document_changed(host);
                Ok(())
            }
            Event::BookMoved { from, path, .. } if token == BOOK_MOVED => {
                self.moved(host, from, path)
            }
            // The search's work is due (the work's own token, as declared).
            Event::WorkDue(due) if token == WORK_DUE && due.token() == SEARCH_WORK => {
                self.search_due(host);
                Ok(())
            }
            Event::Answered { answer, .. } => self.answered(host, &answer),
            // The quiet time has come, or the device sleeps or stops.
            Event::WorkDue(due) if token == WORK_DUE && due.key() == native_index::WORK_KEY => {
                self.count_step(host);
                Ok(())
            }
            Event::WorkDue(_) | Event::Suspend { .. } | Event::Stopping { .. }
                if matches!(token, WORK_DUE | SUSPEND | STOPPING) =>
            {
                // A device that sleeps or stops reads no more aloud and turns no
                // more pages by itself.
                if token != WORK_DUE {
                    self.stop_speech(host);
                    self.auto_turn_leave(host);
                }
                self.save_now(host);
                Ok(())
            }
            // The host's answer to a sentence handed over (the cookie says whose).
            Event::Spoken { cookie, status, .. } if token == SPOKEN => {
                self.spoken(host, cookie, status);
                Ok(())
            }
            // Reading aloud ends when another screen is on top of the book.
            Event::StackChanged { .. } if token == STACK_CHANGED => self.stacked(host),
            // A reading setting changed: the pages on show are laid out again (or
            // drawn again where only the ink changed).
            Event::SettingChanged { key, .. } if token == SETTING_CHANGED => {
                self.setting_changed(host, key)
            }
            // The reading screen and the table of contents have views; the
            // other screens come with their slices (`PORTING-7.md`).
            Event::ScreenOpened {
                instance, handover, ..
            } => self.screen_opened(host, token, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                now_ms,
                ..
            } => self.screen_shown(host, token, instance, now_ms),
            Event::ScreenClosed { instance, .. } => {
                match token {
                    SCREEN => {
                        self.hands.closed(host, instance, self.work.as_ref());
                        self.close(host, instance);
                    }
                    TOC => self.close_toc(host, instance),
                    SEARCH => {
                        if let Some(view) = self.searches.get(instance) {
                            view.close();
                        }
                        self.searches.close(instance);
                    }
                    ANNOTATIONS => self.annotations.close(instance),
                    FOOTNOTE => self.notes.close(instance),
                    SKIM => self.skims.close(instance),
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
        if token == SEARCH {
            let view = self.searches.top(token).ok_or(Error::State)?;
            return view.draw(host, &mut frame);
        }
        if token == FOOTNOTE {
            let note = self.notes.top(token).ok_or(Error::State)?;
            return note.draw(host, &mut frame);
        }
        if token == SKIM {
            let skim = self.skims.top(token).ok_or(Error::State)?;
            return skim.draw(host, &mut frame);
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
        if token == SEARCH {
            return self.search_input(host, &input);
        }
        if token == SCREEN
            && matches!(
                input,
                Input::Action {
                    action: "reader.search",
                    ..
                }
            )
        {
            return self.ask_search(host);
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
        if token == SKIM {
            return self.skim_input(host, &input);
        }
        if token == SCREEN
            && matches!(
                input,
                Input::Action {
                    action: "reader.skim-book",
                    ..
                }
            )
            && let Some(ui) = &self.ui
        {
            native_skim_view::show(host, ui, ("", "", "0", ""))?;
            return Ok(Handled::Yes);
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

/// `text` cut to at most `most` characters.
fn cut(text: &str, most: usize) -> &str {
    text.char_indices()
        .nth(most)
        .map_or(text, |(end, _)| text.get(..end).unwrap_or(text))
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
