//! The reading screen (`core.reader`): the book named by the hand-over is
//! opened through the document service, a page is laid out with the reading
//! settings and drawn, and the page actions turn it. One view for each open
//! instance; the document is closed with it. What is decided without a host is
//! in `native_pages`, `native_open`, `native_typeset` and `native_draw`; what is
//! not ported yet (the place saved last time, progress, menus, selection,
//! links, annotations) is in `PORTING-7.md`.
use crate::native_word::Word;
use crate::{
    native_actions,
    native_breaks::Breaks,
    native_chapter::{self, Chapter},
    native_document::HostDocument,
    native_draw::{HostInk, draw_page},
    native_index::Counter,
    native_links::DocRef,
    native_marking::{Book, Ctx, Marking, Outcome},
    native_marks::{self, CanvasPen, Gauge, Marks, Painted},
    native_open::{self, Place, Start},
    native_pages::{self, Trail, Turn},
    native_place::{self, Progress, Refused},
    native_speech::Speaker,
    native_status,
    native_typeset::{Faces, HostFonts},
    native_wordbox,
};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;
use paperpaw_sdk::{
    Color, Handled, NoStorage,
    document::Locator,
    layout::{Hyphenate, LayoutContext, Page, layout_page},
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input},
        clock,
        commands::{CommandSender, RedrawMode},
        document::{DocId, DocPosition, Documents, Preparation, Report, Root},
        library::Library,
        snapshot,
        ui::{HandoverId, Notify, Open as NavOpen, Param, Ui},
    },
};
use paperpaw_ui_kit::{
    Messages,
    mem::{SERVICE_BUFFER, buffer},
    text,
};

/// Longest path taken from a hand-over, in bytes.
pub const MAX_PATH: usize = 512;
pub use pictures::{AFTER_MS as PICTURES_AFTER_MS, KEY as PICTURES_KEY};
/// Most pages laid out to find the page before one.
pub const BACK_LIMIT: usize = 4096;

/// The progress of a book, where it opens and the progress stored with that
/// place.
type Beginning = (Option<Progress>, (u32, u32), u32);

/// What the screen shows.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "one body per open screen; boxing it would be an allocation that can fail"
)]
enum Body {
    /// Why there is no page (the key of the text).
    Message(&'static str),
    /// The book, open.
    Open(Box<Open>),
    /// The book opened for the first time, its document still prepared by the
    /// format (`preparing.rs`): kept open until it is ready or failed, or the
    /// screen closes.
    Preparing(Preparing),
}

/// Why the place is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum When {
    /// A page was turned: not more often than the interval.
    Turn,
    /// The quiet time, sleep or stop: now.
    Now,
    /// The screen closed: now, and said to the device if it fails.
    Closing,
}

/// The root as a hand-over writes it.
const fn root_code(root: Root) -> &'static str {
    match root {
        Root::Data => "0",
        Root::Handed => "5",
        _ => "1",
    }
}

/// An open book and the page on show.
#[derive(Debug)]
struct Open {
    id: DocId,
    sections: u32,
    whole_pages: bool,
    /// The pages run right to left (manga): the page turns are mirrored.
    rtl: bool,
    /// Where the page on show starts.
    at: (u32, u32),
    /// The page on show, laid out at the first draw after it changed.
    page: Option<Page>,
    /// The words of `page` with their boxes (made with it): what a touch hits.
    words: Vec<Word>,
    /// What is chosen, looked up and followed on the page.
    marking: Marking,
    /// The book's highlights and the search hit shown on its page.
    marks: Marks,
    /// What the page on show was laid out with: a change of the reading
    /// settings or of the screen lays it out again.
    laid: Option<LayoutKey>,
    /// The page after the one on show, laid out in the quiet time after a
    /// turn so that the next turn only takes it (`lay_ahead`): one more page
    /// of heap while the reader is shown, let go of with the page on show.
    ahead: Option<Ahead>,
    trail: Trail,
    /// The book's path, for its saved place.
    path: String,
    /// The root the book was opened on, as a hand-over writes it.
    root: &'static str,
    /// The conversion the open document's places belong to.
    conversion: u64,
    /// The sections' lengths, for progress; none where the book has too many.
    progress: Option<Progress>,
    /// The progress stored with the place, written again where this screen
    /// cannot tell it (a book of too many sections).
    stored_ppm: u32,
    /// When the place was written last (the host's clock, microseconds).
    saved_at: Option<u64>,
    /// The page on show is not the place saved.
    unsaved: bool,
    /// The last input turned the page, not yet told to the plugin.
    turned: bool,
    /// The pictures of the page on show: drawn a frame after its text (#252).
    pictures: pictures::Pass,
    /// Its pictures still to be asked for outside a draw (#438).
    missing: pictures::Missing,
    /// The picture of the page on show full screen, and how it is zoomed
    /// (`zoom.rs`).
    zoom: Option<zoom::Zoom>,
    /// A jump from another screen is not yet reported to the host.
    report_due: bool,
    /// The book's title as a report carries it.
    title: String,
    /// The book's language (a BCP 47 tag), the UI language where the format
    /// names none (`book_language`): the voice reading aloud speaks it.
    language: String,
    /// The words of the page on show the sentence read aloud underlines.
    mark: Option<Range<usize>>,
    /// The words of the page on show being read aloud, underlined or not.
    spoken: Option<Range<usize>>,
    /// The book was reported read to the end.
    finished: bool,
    /// A refused write has been said (once for a book).
    said: bool,
    /// The count of the book's pages in this layout.
    counter: Option<Counter>,
    /// The device's memory class, once asked.
    class: Option<paperpaw_sdk::native::device::DeviceClass>,
    class_asked: bool,
    /// The chapter of the page on show (its start, the next start, its title).
    chapter: Option<Chapter>,
    /// The pages left in the chapter, as last reported, and for which place.
    chapter_left: Option<((u32, u32), u32)>,
    /// The starts of the book's bookmarks, for the ribbon (`ribbon.rs`).
    bookmarks: Vec<(u32, u32)>,
    /// Those of other conversions, shown where the page has their text.
    foreign_bookmarks: Vec<ribbon::ForeignMark>,
    /// The status lines chosen at the top and the bottom.
    status: native_status::State,
    /// The marks plugins gave the page on show (`reader.decorations@1`).
    decorations: Option<crate::native_decorations::Decorations>,
    /// No plugin marks this book's pages (asked again once back on top).
    undecorated: bool,
    /// The rules of `reader.filter@1` for this book (`filters.rs`).
    filters: Vec<crate::native_filter::Filter>,
    /// What the plugins of `reader.text@1` and `reader.layer@1` add (`layers.rs`).
    layers: layers::Layers,
    /// Pages are asked for marks: from the step after the first is shown.
    ask_marks: bool,
    /// The packs of letters this book asked for while open, and what its first
    /// blocks lack while it is still to be kept (`script_hint.rs`).
    packs: script_hint::Packs,
    /// The reading settings as last read, until one changes (`setup::Kept`).
    settings: Kept,
}

/// One open reading screen.
#[derive(Debug)]
pub struct View {
    body: Body,
    /// Reading aloud, where the book is read aloud (`native_view/aloud.rs`).
    speaker: Speaker,
    /// *Read aloud* was asked while the book was prepared: it starts once the
    /// book is open and the reading screen is on top (`take_aloud_kept`).
    aloud_kept: bool,
}

/// The hand-over's values, as far as the screen reads them.
#[derive(Debug, Default)]
pub(crate) struct Handover {
    pub(crate) path: String,
    pub(crate) root: Option<String>,
    pub(crate) section: Option<String>,
    pub(crate) offset: Option<String>,
    page: Option<String>,
}

impl Handover {
    /// The root the book is opened on (the library's unless the hand-over says).
    ///
    /// A plugin that hands a file of its own over names its folder: `0` (data)
    /// or `2` (install), with the `path` (`document.md` 1, root 5). The reader
    /// reads it as the handed file (`doc_open` root 5), never from its own
    /// data folder. `5` is the reader's own code for a book it opened that
    /// way (`root_code`, for its table-of-contents screen).
    pub(crate) fn root(&self) -> Root {
        match self.root.as_deref() {
            Some("0" | "2" | "5") => Root::Handed,
            _ => Root::Library,
        }
    }

    /// The hand-over of a book this screen showed (its path and root code):
    /// opened again, at its saved place.
    pub(crate) fn of_book(path: String, root: &str) -> Result<Self, Error> {
        Ok(Self {
            path,
            root: Some(text::copy(root)?),
            ..Self::default()
        })
    }

    fn get(&self, key: &str) -> Option<&str> {
        match key {
            native_open::SECTION => self.section.as_deref(),
            native_open::OFFSET => self.offset.as_deref(),
            native_open::PAGE => self.page.as_deref(),
            _ => None,
        }
    }
}

pub(crate) fn read_handover(
    host: &Host<'_>,
    ui: &Ui,
    handover: HandoverId,
) -> Result<Handover, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut found = Handover::default();
    for param in ui.params(host, handover, &mut input, &mut out)? {
        let param = param?;
        let value = param.value();
        match param.key() {
            "path" if value.len() <= MAX_PATH => found.path = text::copy(value)?,
            "root" => found.root = Some(text::copy(value)?),
            native_open::SECTION => found.section = Some(text::copy(value)?),
            native_open::OFFSET => found.offset = Some(text::copy(value)?),
            native_open::PAGE => found.page = Some(text::copy(value)?),
            _ => {}
        }
    }
    Ok(found)
}

impl View {
    /// The screen of the book the hand-over `found` names, opened.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals other than the book's own (a book that
    /// does not open is a screen that says so).
    pub(crate) fn open(
        host: &Host<'_>,
        (docs, library): (&Documents, Option<&Library>),
        found: Handover,
    ) -> Result<Self, Error> {
        if found.path.is_empty() {
            return Ok(Self::message("reader.no-book"));
        }
        let root = found.root();
        let state = {
            let (mut input, mut out) = (buffer(256)?, buffer(4096)?);
            let colour =
                snapshot::capture_snapshot(host).is_ok_and(|display| display.display_is_colour());
            let book = open_request(&found.path, root, (publisher_styles(host)?, colour));
            match docs.open_book(host, &book, &mut input, &mut out) {
                Ok(state) => (
                    state.id(),
                    state.preparation(),
                    state.whole_pages(),
                    state.conversion(),
                    state.status(),
                ),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(Error::Busy) => return Ok(Self::message("reader.not-ready")),
                // The file is gone from the card (Beta 3's "no longer there").
                Err(Error::NotFound) => return Ok(Self::message("reader.missing")),
                // No format of this app reads it: the next reading app may
                // (#369 G). Where the host does not pass it on, it says so here.
                Err(Error::Unsupported) => {
                    let mut scratch = [0; 64];
                    CommandSender::new(host, &mut scratch)
                        .pass_on(Error::Unsupported)
                        .ok();
                    return Ok(Self::message("reader.error"));
                }
                Err(_) => return Ok(Self::message("reader.error")),
            }
        };
        let (id, preparation, whole_pages, conversion, status) = state;
        let book = Preparing {
            id,
            found,
            whole_pages,
            conversion,
        };
        match preparation {
            Preparation::Ready => Self::opened(host, (docs, library), book),
            // Opened for the first time: the format prepares the book (Beta 3's
            // "Opening the book for the first time"); the document stays open
            // and the book opens when the host says it is ready.
            Preparation::Preparing => Ok(Self {
                body: Body::Preparing(book),
                speaker: Speaker::new(0),
                aloud_kept: false,
            }),
            // Failed in the open: the document's status says why (no room, the
            // file gone), as it does when it fails later.
            _ => {
                Self::forget(host, docs, id);
                Ok(Self::message(preparing::failed_message(status)))
            }
        }
    }

    /// The screen of a book whose document is ready: its title, sections and the
    /// place it opens at. A refusal on the way gives the document back.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals other than the book's own.
    fn opened(
        host: &Host<'_>,
        (docs, library): (&Documents, Option<&Library>),
        book: Preparing,
    ) -> Result<Self, Error> {
        let Preparing {
            id,
            found,
            whole_pages,
            conversion,
        } = book;
        let root = found.root();
        // A refusal on the way gives the document back.
        let fail = |error| {
            Self::forget(host, docs, id);
            error
        };
        let (Ok(mut input), Ok(mut out)) = (buffer(256), buffer(4096)) else {
            return Err(fail(Error::NoMemory));
        };
        let sections = match docs.metadata(host, id, &mut input, &mut out) {
            Ok(meta) => (
                meta.sections().max(1),
                // The path stands for a title the format does not give.
                text::copy(native_place::title_of(meta.title(), &found.path)),
                book_language(host, meta.language()),
                crate::native_script_hint::declared(meta.language()),
                meta.right_to_left(),
            ),
            Err(Error::NoMemory) => return Err(fail(Error::NoMemory)),
            Err(_) => {
                fail(Error::Invalid);
                return Ok(Self::message("reader.error"));
            }
        };
        let (sections, title, language, script, rtl) = sections;
        let (Ok(title), Ok(language)) = (title, language) else {
            return Err(fail(Error::NoMemory));
        };
        // The place the hand-over names, if the book has it; else the place
        // saved last time; else the start (a page number needs the page index:
        // `PORTING-7.md`).
        let (progress, at, stored_ppm) = Self::place_and_progress(
            host,
            (docs, library),
            (&found, id, (sections, conversion)),
            (&mut input, &mut out),
        )
        .map_err(fail)?;
        let marks = Self::load_marks(host, library, &found.path, conversion);
        // On the heap at once, apart from the screen: the many moves of an `Open` on
        // the stack are what the S3's frame limit counts (`open`, `wake`; #92).
        let mut open = paperpaw_ui_kit::mem::boxed(Open {
            id,
            sections,
            whole_pages,
            rtl,
            at,
            page: None,
            words: Vec::new(),
            marking: Marking::new(),
            marks,
            laid: None,
            ahead: None,
            trail: Trail::new(),
            root: root_code(root),
            unsaved: native_open::names_place(|k| found.get(k), sections),
            path: found.path,
            conversion,
            progress,
            stored_ppm,
            saved_at: None,
            turned: false,
            pictures: pictures::Pass::Whole,
            missing: pictures::Missing::default(),
            zoom: None,
            report_due: false,
            said: false,
            title,
            language,
            mark: None,
            spoken: None,
            finished: false,
            counter: None,
            class: None,
            class_asked: false,
            chapter: None,
            chapter_left: None,
            bookmarks: Vec::new(),
            foreign_bookmarks: Vec::new(),
            status: native_status::State::default(),
            decorations: None,
            undecorated: false,
            filters: Vec::new(),
            layers: layers::Layers::default(),
            ask_marks: false,
            packs: script_hint::Packs::default(),
            settings: Kept::default(),
        })
        .map_err(fail)?;
        open.ask_filters(host);
        open.ask_layers(host);
        open.begin(host, root == Root::Library);
        open.load_bookmarks(host, library);
        open.ask_packs(host, docs, script);
        // The host's book events start here (`document.md` 2.4); a book whose
        // first page cannot be laid out is told with its first draw.
        open.announce(host, docs);
        // Its pictures follow its text: their boxes are asked for when the
        // screen is shown (its place on the screen is the host's then).
        open.ask_when_shown();
        Ok(Self {
            body: Body::Open(open),
            speaker: Speaker::new(0),
            aloud_kept: false,
        })
    }

    /// The sections' lengths for progress, and where the book opens with the
    /// progress stored with that place.
    fn place_and_progress(
        host: &Host<'_>,
        (docs, library): (&Documents, Option<&Library>),
        (found, id, (sections, conversion)): (&Handover, DocId, (u32, u64)),
        (input, out): (&mut [u8], &mut [u8]),
    ) -> Result<Beginning, Error> {
        let progress = Progress::read(host, docs, id, sections, (input, out))?;
        let (at, stored) = Self::starting_place(
            host,
            library,
            found,
            (sections, conversion, progress.as_ref()),
            out,
        )?;
        Ok((progress, at, stored))
    }

    /// The highlights of the book; a book the library does not know has none,
    /// and where there is no room to read them the page is drawn without
    /// highlights, the book still opens.
    fn load_marks(
        host: &Host<'_>,
        library: Option<&Library>,
        path: &str,
        conversion: u64,
    ) -> Marks {
        match library.map(|l| native_marks::load(host, l, path, conversion)) {
            Some(Ok((own, foreign))) => Marks::new(own).with_foreign(foreign),
            _ => Marks::default(),
        }
    }

    /// Where the book opens and the progress stored with its place: the place
    /// the hand-over names, if the book has it; else the place saved last time;
    /// else the start (a page number needs the page index: `PORTING-7.md`).
    fn starting_place(
        host: &Host<'_>,
        library: Option<&Library>,
        found: &Handover,
        (sections, conversion, progress): (u32, u64, Option<&Progress>),
        out: &mut [u8],
    ) -> Result<((u32, u32), u32), Error> {
        Ok(
            match native_open::opening(|k| found.get(k), |p: Place| p.section < sections) {
                Start::At(place) => ((place.section, place.offset), 0),
                Start::Saved => {
                    // The library's calls take 1,024 bytes of input.
                    let mut ask = buffer(1024)?;
                    let stored = library.map(|library| {
                        native_place::saved(
                            host,
                            library,
                            &found.path,
                            (sections, conversion),
                            (&mut ask, out),
                        )
                    });
                    // A place of another conversion is found again by its
                    // progress; the file stays as it is until the next save.
                    let at = stored.and_then(|s| {
                        s.at.or_else(|| {
                            s.other
                                .and_then(|other| native_place::relocate(other, s.ppm, progress))
                        })
                    });
                    (at.unwrap_or((0, 0)), stored.map_or(0, |s| s.ppm))
                }
                _ => ((0, 0), 0),
            },
        )
    }

    /// A screen that had no room to open its book says so in a few words, as
    /// Beta 3 did (the callback then ends well and the screen is kept, so the
    /// run goes on); other errors stay.
    ///
    /// # Errors
    /// `error` itself, unless it is `NoMemory`.
    pub fn when_full(error: Error) -> Result<Self, Error> {
        match error {
            Error::NoMemory => Ok(Self::message("reader.no-memory")),
            other => Err(other),
        }
    }

    const fn message(key: &'static str) -> Self {
        Self {
            body: Body::Message(key),
            speaker: Speaker::new(0),
            aloud_kept: false,
        }
    }

    /// Gives the document back (the screen closed); a refusal leaves it to the
    /// host's teardown.
    fn forget(host: &Host<'_>, docs: &Documents, id: DocId) {
        if let (Ok(mut input), Ok(mut out)) = (buffer(64), buffer(64)) {
            docs.close(host, id, &mut input, &mut out).ok();
        }
    }

    /// A reading setting changed (or the screen turned): the page on show is laid
    /// out again at the next draw from the place it starts at, the trail of the
    /// pages before it is forgotten (they are found anew), and the host is told
    /// where the reader is on the page seen again (`announce`). A screen that
    /// shows no book has nothing to lay out.
    ///
    /// # Errors
    /// The host's refusal to redraw.
    pub fn reflow(&mut self, host: &Host<'_>, docs: &Documents) -> Result<(), Error> {
        // The words of the sentence read aloud are other ones now: it pauses.
        self.pause_aloud(host, (docs, None), true);
        if let Body::Open(open) = &mut self.body {
            open.page = None;
            open.ahead = None;
            open.laid = None;
            open.trail.clear();
            open.pictures = pictures::Pass::Whole;
            if let Ok(setup) = open.settings.setup(host) {
                open.ensure_counter(host, &setup);
            }
            open.announce(host, docs);
            return open.redraw_page(host, docs, RedrawMode::Full); // asks for new pictures (#250)
        }
        redraw(host, RedrawMode::Full)
    }

    /// Whether the open book holds tables of the fonts' letters.
    #[must_use]
    pub fn holds_tables(&self) -> bool {
        matches!(&self.body, Body::Open(open) if open.settings.has_tables())
    }

    /// Lets go of the tables of the fonts' letters (the reading screen is
    /// covered); the next page makes them again.
    pub fn free_tables(&self) {
        if let Body::Open(open) = &self.body {
            open.settings.drop_tables();
        }
    }

    /// A setting changed: the reading settings are read again at the next use.
    pub fn forget_settings(&self) {
        if let Body::Open(open) = &self.body {
            open.settings.forget();
        }
    }

    /// Only the way the text is inked changed: the page is drawn again.
    ///
    /// # Errors
    /// The host's refusal to redraw.
    pub fn repaint(&self, host: &Host<'_>) -> Result<(), Error> {
        match &self.body {
            Body::Open(_) => redraw(host, RedrawMode::Full),
            Body::Message(_) | Body::Preparing(_) => Ok(()),
        }
    }

    /// Whether the last input turned the page (the quiet time is to be
    /// started again, as Beta 3 did on a turn only); asked once.
    pub fn take_turned(&mut self) -> bool {
        match &mut self.body {
            Body::Open(open) => core::mem::take(&mut open.turned),
            Body::Message(_) | Body::Preparing(_) => false,
        }
    }

    /// Writes the place now if it is not saved: the quiet time has come, or
    /// the device sleeps or stops.
    pub fn save_now(&mut self, host: &Host<'_>, library: Option<&Library>) {
        if let Body::Open(open) = &mut self.body {
            open.save(host, library, When::Now);
        }
    }

    /// The actions besides the page turns: the font size, the justification
    /// and the chapters; `None` for any other.
    fn other_action(
        &mut self,
        host: &Host<'_>,
        (docs, library): (&Documents, Option<&Library>),
        action: &str,
    ) -> Result<Option<Handled>, Error> {
        match action {
            "reader.font-larger" => native_actions::font_step(host, 1)?,
            "reader.font-smaller" => native_actions::font_step(host, -1)?,
            "reader.toggle-justify" => native_actions::toggle_justify(host)?,
            "reader.next-chapter" | "reader.prev-chapter" => {
                if let Body::Open(open) = &mut self.body {
                    open.step_chapter(host, (docs, library), action == "reader.next-chapter")?;
                }
            }
            _ => return Ok(None),
        }
        Ok(Some(Handled::Yes))
    }

    /// Opens the table of contents of the book on show.
    fn show_toc(&self, host: &Host<'_>, ui: &Ui) -> Result<Handled, Error> {
        let Some(params) = self.toc_params() else {
            return Ok(Handled::No);
        };
        let [(k0, v0), (k1, v1), (k2, v2), (k3, v3)] = &params;
        let list = [
            Param::new(k0, v0)?,
            Param::new(k1, v1)?,
            Param::new(k2, v2)?,
            Param::new(k3, v3)?,
        ];
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let handover = ui.handover(host, &list, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch)
            .open(NavOpen::screen("reader.toc")?.handover(handover))?;
        Ok(Handled::Yes)
    }

    /// Goes to `place`, far from here (the table of contents, a search hit, a
    /// highlight, a page number): the page shown starts with the whole line
    /// `place` is in, and *Back* returns to the page left, as in Beta 3 (#441).
    /// The place is reported and written as any turn's.
    ///
    /// # Errors
    /// `NoMemory`, and the host's refusal to redraw.
    pub fn go_to(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        place: (u32, u32),
    ) -> Result<(), Error> {
        let Body::Open(open) = &mut self.body else {
            return Ok(());
        };
        if place.0 >= open.sections || place == open.at {
            return Ok(());
        }
        let place = match open.settings.setup(host) {
            Ok(setup) => open.line_start(host, docs, &setup, place).unwrap_or(place),
            Err(_) => place,
        };
        if place == open.at {
            return Ok(());
        }
        open.marking.jumped(Locator::new(open.at.0, open.at.1))?;
        open.marking.reset();
        open.marking.landed(Locator::new(place.0, place.1));
        open.name_way_back(host);
        open.trail.clear();
        open.at = place;
        open.page = None;
        open.unsaved = true;
        open.turned = true;
        // Told when this screen is shown again: the host drops the report of
        // a screen that is not on top (`document.md` 2.4).
        open.report_due = true;
        Ok(())
    }

    /// [`go_to`](Self::go_to) the match from `start` to `end` (a search hit
    /// chosen), marked on the page; the mark goes with the next page.
    ///
    /// # Errors
    /// `NoMemory`, and the host's refusal to redraw.
    pub fn go_to_match(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        start: (u32, u32),
        end: (u32, u32),
    ) -> Result<(), Error> {
        self.go_to(host, docs, start)?;
        if let Body::Open(open) = &mut self.body
            && start.0 < open.sections
            && start < end
        {
            open.marks.found(
                open.at,
                native_marks::Span {
                    start,
                    end,
                    noted: false,
                },
            );
        }
        Ok(())
    }

    /// This screen is on top again: a jump made from a screen above is told to
    /// the host now.
    pub fn shown(&mut self, host: &Host<'_>, docs: &Documents) {
        if let Body::Open(open) = &mut self.body {
            open.decorate_again();
            open.missing_when_shown();
        }
        if let Body::Open(open) = &mut self.body
            && core::mem::take(&mut open.report_due)
        {
            open.announce(host, docs);
            // The book or a place in it is shown: its pictures follow its text
            // (the show draws it anyway: asked for only where there are any).
            if open.has_pictures() {
                open.redraw_page(host, docs, RedrawMode::Partial).ok();
            }
        } else if let Body::Open(open) = &mut self.body
            && open.pictures_pending()
        {
            // Opened with pictures, or shown again while they waited (the
            // host dropped their follow-up when the screen left the top): they
            // are asked for together with a whole redraw, so the host schedules
            // a follow-up frame that draws the pictures in their places.
            open.show_pictures(host, docs).ok();
        }
    }

    /// The book on show, as a hand-over names it: its path and root code.
    #[must_use]
    pub fn book(&self) -> Option<(&str, &'static str)> {
        match &self.body {
            Body::Open(open) => Some((open.path.as_str(), open.root)),
            // Its document is open while it is prepared: the same book stays,
            // and it is given back for another one.
            Body::Preparing(book) => Some((book.found.path.as_str(), root_code(book.found.root()))),
            Body::Message(_) => None,
        }
    }

    /// The screen is opened again with `found`: whether it shows that book
    /// already. It then stays as it is (no second open of the document), at
    /// the place `found` names, if it names one.
    ///
    /// # Errors
    /// `NoMemory`.
    pub(crate) fn keeps(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        found: &Handover,
    ) -> Result<bool, Error> {
        let same = self
            .book()
            .is_some_and(|(path, root)| path == found.path && root == root_code(found.root()));
        if !same {
            return Ok(false);
        }
        if let native_open::Start::At(place) = native_open::start(|key| found.get(key)) {
            self.go_to(host, docs, (place.section, place.offset))?;
        }
        Ok(true)
    }

    /// The id of the book's document, for a screen of this plugin that reads
    /// it too.
    #[must_use]
    pub const fn doc_id(&self) -> Option<DocId> {
        match &self.body {
            Body::Open(open) => Some(open.id),
            Body::Message(_) | Body::Preparing(_) => None,
        }
    }

    /// The open document of the book at `path`, if this screen shows it.
    #[must_use]
    pub fn doc_of(&self, path: &str) -> Option<DocId> {
        match &self.body {
            Body::Open(open) if open.path == path => Some(open.id),
            _ => None,
        }
    }

    /// A screen of this plugin that needs room covers this one (the table of
    /// contents finding headings): the page on show and its words are let go
    /// of, and laid out again when the page is drawn next (#248). Kept while
    /// the book is read aloud, which follows the words.
    pub fn release_page(&mut self) {
        if self.reading_aloud() {
            return;
        }
        if let Body::Open(open) = &mut self.body {
            open.page = None;
            open.ahead = None;
            open.words = Vec::new();
        }
    }

    /// The open document and the conversion its places belong to.
    #[must_use]
    pub const fn document(&self) -> Option<(DocId, u64)> {
        match &self.body {
            Body::Open(open) => Some((open.id, open.conversion)),
            Body::Message(_) | Body::Preparing(_) => None,
        }
    }

    /// The hand-over params of the table of contents of this book: where the
    /// book is and where the reader is in it. `None` where no book is open.
    #[must_use]
    pub fn toc_params(&self) -> Option<[(&'static str, String); 4]> {
        let Body::Open(open) = &self.body else {
            return None;
        };
        let place = |n: u32| text::copy(text::decimal_u64(u64::from(n), &mut [0; 20])).ok();
        Some([
            ("path", text::copy(&open.path).ok()?),
            ("root", text::copy(open.root).ok()?),
            ("section", place(open.at.0)?),
            ("offset", place(open.at.1)?),
        ])
    }

    /// Closes the document of the screen.
    pub fn close(&mut self, host: &Host<'_>, (docs, library): (&Documents, Option<&Library>)) {
        // Nothing is read aloud from a book that is closed.
        self.stop_aloud(host, (docs, library));
        // The document is given back once: the body that holds it goes with it.
        match core::mem::replace(&mut self.body, Body::Message("reader.no-book")) {
            Body::Open(mut open) => {
                open.save(host, library, When::Closing);
                Self::forget(host, docs, open.id);
            }
            Body::Preparing(book) => Self::forget(host, docs, book.id),
            Body::Message(_) => {}
        }
    }

    /// *Go to the note* of `path`'s book: follows `href`, and draws again
    /// where the page moved. Another book's note is none of this screen's.
    ///
    /// # Errors
    /// Allocation and the host's refusals.
    pub fn follow(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        (path, href): (&str, &str),
    ) -> Result<(), Error> {
        let Body::Open(open) = &mut self.body else {
            return Ok(());
        };
        if open.path != path || !open.follow(host, docs, href)? {
            return Ok(());
        }
        open.unsaved = true;
        open.turned = true;
        open.announce(host, docs);
        open.redraw_page(host, docs, RedrawMode::Partial)
    }

    /// What an input did: turns the page.
    ///
    /// # Errors
    /// Allocation and the host's refusals.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        (docs, library, ui): (&Documents, Option<&Library>, &Ui),
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        // Any input pauses reading aloud, and "Read aloud" starts it.
        if let Some(handled) = self.aloud_input(host, (docs, library), input) {
            return handled;
        }
        if matches!(
            input,
            Input::Action {
                action: "reader.show-toc",
                ..
            }
        ) {
            return self.show_toc(host, ui);
        }
        if let Some(handled) = self.zoom(host, docs, input)? {
            return Ok(handled);
        }
        if let Input::Action { action, .. } = input
            && let Some(handled) = self.r3_action(host, (docs, library, ui), action)?
        {
            return Ok(handled);
        }
        if let Input::Action { action, .. } = input
            && let Some(handled) = self.other_action(host, (docs, library), action)?
        {
            return Ok(handled);
        }
        if let Some(handled) = self.corner_tap(host, library, input)? {
            return Ok(handled);
        }
        let Body::Open(open) = &mut self.body else {
            return Ok(Handled::No);
        };
        if let Some(handled) = open.status_input(host, ui, input)? {
            return Ok(handled);
        }
        if let Some(handled) = open.layer_input(host, docs, input)? {
            return Ok(handled);
        }
        if let Some(handled) = open.decoration_tap(host, input)? {
            return Ok(handled);
        }
        let marked = open.mark(host, (docs, library, Some(ui)), input)?;
        let choosing = matches!(marked, Outcome::Turn { .. });
        let moved = match marked {
            Outcome::Handled | Outcome::Redraw => return Ok(Handled::Yes),
            Outcome::Edited => {
                open.reload_marks(host, library)?;
                return Ok(Handled::Yes);
            }
            Outcome::Jump { section, offset } => {
                open.jump(host, (section, offset));
                true
            }
            Outcome::Turn { forward: true } => open.forward(host, docs)?,
            Outcome::Turn { forward: false } => open.previous(host, docs)?,
            _ => match native_pages::turn(input, open.rtl) {
                // Select or Menu with nothing chosen: the reader's menu bar.
                Turn::Ignored
                    if matches!(
                        input,
                        Input::Action {
                            action: "core.menu" | "core.confirm",
                            ..
                        }
                    ) =>
                {
                    return open.menu_bar(host, ui);
                }
                Turn::Ignored => return Ok(Handled::No),
                Turn::Next => open.forward(host, docs)?,
                Turn::Previous => open.previous(host, docs)?,
            },
        };
        // A turn that did not move (the last page) leaves the link focus where it was.
        if choosing && !moved {
            open.marking.stay(host);
        }
        if moved {
            open.after_turn(host, (docs, library))?;
        }
        Ok(Handled::Yes)
    }

    /// Draws the screen.
    ///
    /// # Errors
    /// Allocation, font and drawing errors.
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        (docs, ui): (&Documents, Option<&Ui>),
        frame: &mut DrawFrame<'_>,
    ) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let key = match &mut self.body {
            Body::Message(key) => *key,
            Body::Preparing(_) => "reader.preparing",
            Body::Open(open) => match open.draw(host, (docs, ui), frame) {
                Ok(()) => return Ok(()),
                Err(Error::NoMemory) => "reader.no-memory",
                Err(Error::NotFound) => "reader.no-font",
                Err(_) => "reader.error",
            },
        };
        say(host, frame, key)
    }
}

impl Open {
    /// The start of the chapter after (`forward`) or before the page on show,
    /// gone to as a turn goes; nothing happens where there is none.
    fn step_chapter(
        &mut self,
        host: &Host<'_>,
        (docs, library): (&Documents, Option<&Library>),
        forward: bool,
    ) -> Result<(), Error> {
        let from = if forward {
            if self.page.is_none() {
                let setup = self.settings.setup(host)?;
                let page = self.lay(host, docs, &setup, self.at)?;
                self.put_page(page, self.key(&setup));
            }
            let Some(page) = &self.page else {
                return Ok(());
            };
            (page.end.section, page.end.offset)
        } else {
            self.at
        };
        let Some(to) = native_actions::chapter(host, docs, self.id, from, forward)? else {
            return Ok(());
        };
        if to == self.at || to.0 >= self.sections {
            return Ok(());
        }
        self.trail.push(self.at)?;
        self.at = to;
        self.page = None;
        self.unsaved = true;
        self.turned = true;
        self.save(host, library, When::Turn);
        self.announce(host, docs);
        self.redraw_page(host, docs, RedrawMode::Partial)
    }

    /// Says that the place could not be saved (`key`, by the refusal); after the screen closed the
    /// device's message (`announce`) carries it, as nothing else would. Whether
    /// the host took the notice: a quiet work run (the auto-turn's tick) is
    /// refused one, and the next save tries again.
    fn say_not_saved(&self, host: &Host<'_>, key: &str, closing: bool) -> bool {
        let Ok(text) = Messages::new().and_then(|mut m| m.copy(host, key)) else {
            return false;
        };
        let Ok(note) = Notify::new(&text) else {
            return false;
        };
        let note = if closing { note.announce() } else { note };
        let mut scratch = [0; 512];
        CommandSender::new(host, &mut scratch).notify(note).is_ok()
    }

    /// Writes the place of the page on show if it is not saved: now unless
    /// `when` is a turn, else once Beta 3's interval has gone since the last
    /// write. A refused write is said once (as `native_place::place_refused`
    /// has it) and tried again at the next time.
    fn save(&mut self, host: &Host<'_>, library: Option<&Library>, when: When) {
        let (true, Some(library)) = (self.unsaved, library) else {
            return;
        };
        let now = clock::micros(host).ok();
        let due = when != When::Turn
            || match (self.saved_at, now) {
                (Some(then), Some(now)) => now.saturating_sub(then) >= native_place::SAVE_EVERY_US,
                _ => true,
            };
        if !due {
            return;
        }
        let ppm = self
            .progress
            .as_ref()
            .map_or(self.stored_ppm, |p| p.ppm(self.at.0, self.at.1));
        // The host asks for room for the op's largest answer (`set_position`:
        // `max_reply` 4,096), else it refuses before the op runs.
        let kept = match (buffer(2048), buffer(SERVICE_BUFFER)) {
            (Ok(mut input), Ok(mut out)) => native_place::save(
                host,
                library,
                &self.path,
                self.at,
                (ppm, self.conversion, native_place::wall_seconds(host)),
                (&mut input, &mut out),
            ),
            _ => Err(Error::NoMemory),
        };
        match kept {
            Ok(()) => {
                self.unsaved = false;
                self.saved_at = now;
            }
            Err(error) => {
                let closing = when == When::Closing;
                if let (Refused::Say(key), false) =
                    (native_place::place_refused(error, closing), self.said)
                {
                    self.said = self.say_not_saved(host, key, closing);
                }
            }
        }
    }

    /// Tells the host where the reader is (`core.document` op 22, `document.md`
    /// 2.4): the page on show, laid out here if it is not yet, its place and
    /// progress, and that the book is read to the end when this page ends it.
    /// The host raises the book events from it. A page that cannot be laid out,
    /// or a refusal, is no report (the draw says what is wrong). The chapter and the
    /// pages left in it come from `native_chapter`, the page numbers from the
    /// count, once it is complete (`PORTING-7.md`).
    fn announce(&mut self, host: &Host<'_>, docs: &Documents) {
        let Ok(setup) = self.settings.setup(host) else {
            return;
        };
        if self.page.is_none() {
            match self.lay(host, docs, &setup, self.at) {
                Ok(page) => self.put_page(page, self.key(&setup)),
                Err(_) => return,
            }
        }
        let Some(page) = &self.page else {
            return;
        };
        let ppm = self
            .progress
            .as_ref()
            .map_or(self.stored_ppm, |p| p.ppm(self.at.0, self.at.1));
        let at_end = !page.is_past_end()
            && self.progress.as_ref().is_some_and(|p| {
                native_place::reaches_end(p.ppm(page.end.section, page.end.offset))
            });
        let mut report = Report::new(
            DocPosition::new(self.at.0, self.at.1),
            native_place::report_ppm(self.finished || at_end, ppm),
        )
        .title(&self.title);
        if at_end && !self.finished {
            report = report.finished();
        }
        let numbers = self.page_numbers(host);
        if let Some((page, pages)) = numbers {
            report = report.pages(page, pages);
        }
        // The chapter of the page: asked of the table when the page leaves
        // the one kept (`native_chapter`); no chapter where it cannot be had.
        let at = self.at;
        if !self.chapter.as_ref().is_some_and(|c| c.covers(at)) {
            self.chapter = native_chapter::find(host, docs, self.id, at).ok();
        }
        let left = match (&self.chapter, numbers) {
            (Some(chapter), Some(numbers)) => self.chapter_left(host, chapter, numbers),
            _ => None,
        };
        if let Some(chapter) = &self.chapter {
            report = report.chapter(chapter.title());
        }
        self.chapter_left = left.map(|left| (at, left));
        if let Some(left) = left {
            report = report.chapter_left(left);
        }
        let (Ok(mut input), Ok(mut out)) = (buffer(1024), buffer(64)) else {
            return;
        };
        if docs
            .report(host, self.id, &report, &mut input, &mut out)
            .is_ok()
            && at_end
        {
            self.finished = true;
        }
    }

    /// Where the book's text is laid out: the content box, or the part of it
    /// a page layer's region gives the text (`native_view/layers.rs`).
    fn area(&self, setup: &Setup) -> Option<native_pages::Box> {
        self.page_area(setup)
            .map(|full| self.layers.text_area(full))
    }

    /// The content box: the whole display for whole-page books (as Beta 3's
    /// margins of none), else inside the reading margins.
    fn page_area(&self, setup: &Setup) -> Option<native_pages::Box> {
        let mut reading = setup.reading;
        if self.whole_pages {
            reading.margin = 0;
            reading.margin_ends = 0;
            reading.status = (0, 0);
        }
        native_pages::content_box(
            setup.display.display_width(),
            setup.display.display_height(),
            setup.display.display_dpi(),
            &reading,
        )
    }

    /// The page on show was just changed by a turn: its place is to be saved,
    /// the host is told, and the screen is drawn again.
    fn after_turn(
        &mut self,
        host: &Host<'_>,
        (docs, library): (&Documents, Option<&Library>),
    ) -> Result<(), Error> {
        self.unsaved = true;
        self.turned = true;
        self.save(host, library, When::Turn);
        self.announce(host, docs);
        self.redraw_page(host, docs, RedrawMode::Partial)
    }

    /// What the marking makes of `input` on the page on show (nothing where the
    /// page has not been drawn).
    fn mark(
        &mut self,
        host: &Host<'_>,
        (docs, library, ui): (&Documents, Option<&Library>, Option<&Ui>),
        input: &Input<'_>,
    ) -> Result<Outcome, Error> {
        if self.page.is_none() {
            return Ok(Outcome::Ignored);
        }
        let setup = self.settings.setup(host)?;
        self.complete_end(host, docs, &setup, input);
        let area = self.area(&setup).ok_or(Error::NoSpace)?;
        let ctx = Ctx::new(
            Locator::new(self.at.0, self.at.1),
            &self.words,
            &self.path,
            &self.title,
            "",
            area,
        )
        .doc(DocRef::new(docs, self.id, self.sections, self.whole_pages))
        .book(Book {
            highlights: self.marks.highlights(),
            library,
            conversion: self.conversion,
            progress: self.progress.as_ref(),
            stored_ppm: self.stored_ppm,
        });
        let ctx = match ui {
            Some(ui) => ctx.ui(ui),
            None => ctx,
        };
        self.marking.input(host, &ctx, input)
    }

    /// Follows the link `href` of the book (*Go to the note*): goes to its
    /// target, the page left kept for *Back*; whether the page moved.
    fn follow(&mut self, host: &Host<'_>, docs: &Documents, href: &str) -> Result<bool, Error> {
        let setup = self.settings.setup(host)?;
        let area = self.area(&setup).ok_or(Error::NoSpace)?;
        let ctx = Ctx::new(
            Locator::new(self.at.0, self.at.1),
            &[],
            &self.path,
            &self.title,
            "",
            area,
        )
        .doc(DocRef::new(docs, self.id, self.sections, self.whole_pages));
        match self.marking.follow_target(host, &ctx, href)? {
            Outcome::Jump { section, offset } => {
                self.jump(host, (section, offset));
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Takes `page`, laid out with `key`, as the page on show. The words (and
    /// with them the marks, the plugins' marks and what can be chosen) are those
    /// of the page before: they are let go of here and made again at the draw,
    /// so a page never shows or saves a mark by another page's words (#265).
    fn put_page(&mut self, page: Page, key: LayoutKey) {
        self.page = Some(page);
        self.missing = pictures::Missing::default();
        self.laid = Some(key);
        self.words = Vec::new();
    }

    /// Goes to the page that starts at `to` (a followed link, or the way back).
    fn jump(&mut self, host: &Host<'_>, to: (u32, u32)) {
        self.at = to;
        self.page = None;
        self.trail.clear();
        self.marking.reset();
        self.marking.landed(Locator::new(to.0, to.1));
        self.name_way_back(host);
    }

    /// The *Back* button names the page it leads to, once the pages are
    /// counted (#448): found as the jump is made, not at each draw.
    fn name_way_back(&mut self, host: &Host<'_>) {
        let page = self.marking.returns_to().and_then(|to| {
            let counter = self.counter.as_ref()?;
            counter.page_at(
                &paperpaw_sdk::native::storage::DataStorage::new(host),
                (to.section, to.offset),
            )
        });
        self.marking.set_back_page(page.map(|(page, _)| page));
    }

    /// A forward turn by hand: the page after the one on show is laid out
    /// first, so on the last page the turn stays (the page after it would be
    /// empty). The laid page is kept for the draw. Whether it moved.
    fn forward(&mut self, host: &Host<'_>, docs: &Documents) -> Result<bool, Error> {
        let Some(page) = &self.page else {
            return Ok(false);
        };
        let end = (page.end.section, page.end.offset);
        let Ok(setup) = self.settings.setup(host) else {
            return self.next();
        };
        // Laid out in the quiet time after the turn before, if still what a
        // layout now would make (the same page start, settings and rules).
        let key = self.key(&setup);
        let ahead = self.ahead.take().filter(|ahead| {
            ahead.from == self.at
                && ahead.key == key
                && (ahead.page.start.section, ahead.page.start.offset) == end
        });
        let laid = match ahead {
            Some(ahead) => Ok(ahead.page),
            None => self.lay(host, docs, &setup, end),
        };
        match onto(laid) {
            Onto::Stay => Ok(false),
            Onto::Unlaid => self.next(),
            Onto::Page(laid) => {
                let moved = self.next()?;
                if moved {
                    self.put_page(*laid, self.key(&setup));
                }
                Ok(moved)
            }
        }
    }

    /// The page after the one on show; whether there is one.
    fn next(&mut self) -> Result<bool, Error> {
        let Some(page) = &self.page else {
            return Ok(false);
        };
        if page.is_past_end() {
            return Ok(false);
        }
        let end = (page.end.section, page.end.offset);
        // A page that ends where it started would never move on.
        if end <= self.at {
            return Ok(false);
        }
        self.trail.push(self.at)?;
        self.at = end;
        self.page = None;
        Ok(true)
    }

    /// The page before the one on show; whether there is one.
    fn previous(&mut self, host: &Host<'_>, docs: &Documents) -> Result<bool, Error> {
        let before = match self.trail.pop() {
            Some(start) => Some(start),
            None => {
                let from = if self.at.1 == 0 {
                    (self.at.0.saturating_sub(1), 0)
                } else {
                    (self.at.0, 0)
                };
                let setup = self.settings.setup(host)?;
                let at = self.at;
                native_pages::page_before(from, at, BACK_LIMIT, |start| {
                    let page = self.lay(host, docs, &setup, start)?;
                    Ok((page.end.section, page.end.offset))
                })?
            }
        };
        match before {
            Some(start) => {
                self.at = start;
                self.page = None;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

mod aloud;
mod count;
mod decorations;
mod filters;
mod goto;
mod helpers;
pub(crate) mod layers;
mod laying;
mod menu_bar;
mod notes;
mod pictures;
mod preparing;
mod ribbon;
mod script_hint;
mod setup;
use helpers::{book_language, document_error_kind, redraw, say};
use preparing::{Preparing, open_request, publisher_styles};
use setup::{Kept, LayoutKey, Setup, flag, integer};

mod r3;
mod status;
mod zoom;

/// The page after the one starting at `from`, laid out with `key` in advance.
#[derive(Debug)]
struct Ahead {
    from: (u32, u32),
    key: LayoutKey,
    page: Page,
}

/// What a forward turn finds after the page on show.
#[derive(Debug)]
enum Onto {
    /// Nothing: the page on show is the book's last.
    Stay,
    /// The page to turn to, laid out.
    Page(Box<Page>),
    /// Not laid out: the turn goes on, and the draw lays it again and says
    /// what went wrong.
    Unlaid,
}

fn onto(laid: Result<Page, Error>) -> Onto {
    match laid {
        Ok(page) if page.is_past_end() => Onto::Stay,
        Ok(page) => Onto::Page(Box::new(page)),
        Err(_) => Onto::Unlaid,
    }
}

#[cfg(test)]
mod tests;
