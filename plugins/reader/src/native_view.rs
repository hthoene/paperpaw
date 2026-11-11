//! The reading screen (`core.reader`): the book named by the hand-over is
//! opened through the document service, a page is laid out with the reading
//! settings and drawn, and the page actions turn it. One view for each open
//! instance; the document is closed with it. What is decided without a host is
//! in `native_pages`, `native_open`, `native_typeset` and `native_draw`; what is
//! not ported yet (the place saved last time, progress, menus, selection,
//! links, annotations) is in `PORTING-7.md`.
use crate::{
    native_actions,
    native_chapter::{self, Chapter},
    native_document::HostDocument,
    native_draw::{HostInk, draw_lines_marked},
    native_index::Counter,
    native_links::DocRef,
    native_look::Look,
    native_marking::{Ctx, Marking, Outcome},
    native_open::{self, Place, Start},
    native_pages::{self, Reading, Trail, Turn},
    native_place::{self, Progress},
    native_speech::Speaker,
    native_status,
    native_typeset::{Faces, HostFonts},
    native_wordbox,
};
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;
use paperpaw_sdk::{
    Color, Handled, NoStorage,
    document::{DocumentError, Locator},
    layout::{LayoutContext, Page, Style, layout_page},
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input},
        clock,
        commands::{CommandSender, RedrawMode},
        document::{DocId, DocPosition, Documents, OpenBook, Preparation, Report, Root},
        fonts::{Font, FontRole},
        library::Library,
        settings::{self, SettingValue},
        snapshot::{self, Snapshot},
        ui::{HandoverId, Notify, Open as NavOpen, Param, Ui},
    },
    reading::WordBox,
};
use paperpaw_ui_kit::{
    Messages, Style as UiStyle,
    mem::{SERVICE_BUFFER, buffer, push},
    paint, text,
};

/// Longest path taken from a hand-over, in bytes.
pub const MAX_PATH: usize = 512;
/// Most pages laid out to find the page before one.
pub const BACK_LIMIT: usize = 4096;

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
    Open(Open),
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
    /// Where the page on show starts.
    at: (u32, u32),
    /// The page on show, laid out at the first draw after it changed.
    page: Option<Page>,
    /// The words of `page` with their boxes (made with it): what a touch hits.
    words: Vec<WordBox>,
    /// What is chosen, looked up and followed on the page.
    marking: Marking,
    /// What the page on show was laid out with: a change of the reading
    /// settings or of the screen lays it out again.
    laid: Option<LayoutKey>,
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
    /// A jump from another screen is not yet reported to the host.
    report_due: bool,
    /// The book's title as a report carries it.
    title: String,
    /// The book's language (a BCP 47 tag), empty where the format names none:
    /// the voice reading aloud speaks it.
    language: String,
    /// The words of the page on show the sentence read aloud underlines.
    mark: Option<Range<usize>>,
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
    /// The status lines chosen at the top and the bottom.
    status: native_status::State,
}

/// One open reading screen.
#[derive(Debug)]
pub struct View {
    body: Body,
    /// Reading aloud, where the book is read aloud (`native_view/aloud.rs`).
    speaker: Speaker,
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
    pub(crate) fn root(&self) -> Root {
        match self.root.as_deref() {
            Some("0") => Root::Data,
            Some("5") => Root::Handed,
            _ => Root::Library,
        }
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
    /// The screen of the book `handover` names, opened.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals other than the book's own (a book that
    /// does not open is a screen that says so).
    pub fn open(
        host: &Host<'_>,
        ui: &Ui,
        (docs, library): (&Documents, Option<&Library>),
        handover: Option<HandoverId>,
    ) -> Result<Self, Error> {
        let found = match handover {
            Some(handover) => read_handover(host, ui, handover).unwrap_or_default(),
            None => Handover::default(),
        };
        if found.path.is_empty() {
            return Ok(Self::message("reader.no-book"));
        }
        let (mut input, mut out) = (buffer(256)?, buffer(4096)?);
        let root = found.root();
        let book = OpenBook::new(&found.path).root(root).publisher_styles();
        let state = match docs.open_book(host, &book, &mut input, &mut out) {
            Ok(state) => (
                state.id(),
                state.preparation(),
                state.whole_pages(),
                state.conversion(),
            ),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(Error::Busy) => return Ok(Self::message("reader.not-ready")),
            Err(_) => return Ok(Self::message("reader.error")),
        };
        let (id, preparation, whole_pages, conversion) = state;
        match preparation {
            Preparation::Ready => {}
            Preparation::Preparing => {
                Self::forget(host, docs, id);
                return Ok(Self::message("reader.preparing"));
            }
            _ => {
                Self::forget(host, docs, id);
                return Ok(Self::message("reader.error"));
            }
        }
        let sections = match docs.metadata(host, id, &mut input, &mut out) {
            Ok(meta) => (
                meta.sections().max(1),
                // The path stands for a title the format does not give.
                text::copy(native_place::title_of(meta.title(), &found.path)),
                text::copy(meta.language()),
            ),
            Err(Error::NoMemory) => {
                Self::forget(host, docs, id);
                return Err(Error::NoMemory);
            }
            Err(_) => {
                Self::forget(host, docs, id);
                return Ok(Self::message("reader.error"));
            }
        };
        let (sections, title, language) = sections;
        let (Ok(title), Ok(language)) = (title, language) else {
            Self::forget(host, docs, id);
            return Err(Error::NoMemory);
        };
        // The place the hand-over names, if the book has it; else the place
        // saved last time; else the start (a page number needs the page index:
        // `PORTING-7.md`).
        let progress = match Progress::read(host, docs, id, sections, (&mut input, &mut out)) {
            Ok(progress) => progress,
            Err(error) => {
                Self::forget(host, docs, id);
                return Err(error);
            }
        };
        let (at, stored_ppm) =
            Self::starting_place(host, library, &found, (sections, conversion), &mut out)?;
        let mut open = Open {
            id,
            sections,
            whole_pages,
            at,
            page: None,
            words: Vec::new(),
            marking: Marking::new(),
            laid: None,
            trail: Trail::new(),
            root: root_code(root),
            path: found.path,
            conversion,
            progress,
            stored_ppm,
            saved_at: None,
            unsaved: false,
            turned: false,
            report_due: false,
            said: false,
            title,
            language,
            mark: None,
            finished: false,
            counter: None,
            class: None,
            class_asked: false,
            chapter: None,
            status: native_status::State::default(),
        };
        if let Ok(setup) = Setup::read(host) {
            open.ensure_counter(host, &setup);
        }
        // The host's book events start here (`document.md` 2.4); a book whose
        // first page cannot be laid out is told with its first draw.
        if root == Root::Library {
            crate::native_start::remember(host, &open.path);
        }
        open.announce(host, docs);
        Ok(Self {
            body: Body::Open(open),
            speaker: Speaker::new(0),
        })
    }

    /// Where the book opens and the progress stored with its place: the place
    /// the hand-over names, if the book has it; else the place saved last time;
    /// else the start (a page number needs the page index: `PORTING-7.md`).
    fn starting_place(
        host: &Host<'_>,
        library: Option<&Library>,
        found: &Handover,
        (sections, conversion): (u32, u64),
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
                    (
                        stored.and_then(|s| s.at).unwrap_or((0, 0)),
                        stored.map_or(0, |s| s.ppm),
                    )
                }
                _ => ((0, 0), 0),
            },
        )
    }

    const fn message(key: &'static str) -> Self {
        Self {
            body: Body::Message(key),
            speaker: Speaker::new(0),
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
            open.laid = None;
            open.trail.clear();
            if let Ok(setup) = Setup::read(host) {
                open.ensure_counter(host, &setup);
            }
            open.announce(host, docs);
        }
        redraw(host, RedrawMode::Full)
    }

    /// Only the way the text is inked changed: the page is drawn again.
    ///
    /// # Errors
    /// The host's refusal to redraw.
    pub fn repaint(&self, host: &Host<'_>) -> Result<(), Error> {
        match &self.body {
            Body::Open(_) => redraw(host, RedrawMode::Full),
            Body::Message(_) => Ok(()),
        }
    }

    /// Whether the last input turned the page (the quiet time is to be
    /// started again, as Beta 3 did on a turn only); asked once.
    pub fn take_turned(&mut self) -> bool {
        match &mut self.body {
            Body::Open(open) => core::mem::take(&mut open.turned),
            Body::Message(_) => false,
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

    /// Goes to `place` (a jump from the table of contents): the page before is
    /// kept to turn back to, the place is reported and written as any turn's.
    ///
    /// # Errors
    /// `NoMemory`, and the host's refusal to redraw.
    pub fn go_to(&mut self, place: (u32, u32)) -> Result<(), Error> {
        let Body::Open(open) = &mut self.body else {
            return Ok(());
        };
        if place.0 >= open.sections || place == open.at {
            return Ok(());
        }
        open.trail.push(open.at)?;
        open.at = place;
        open.page = None;
        open.unsaved = true;
        open.turned = true;
        // Told when this screen is shown again: the host drops the report of
        // a screen that is not on top (`document.md` 2.4).
        open.report_due = true;
        Ok(())
    }

    /// This screen is on top again: a jump made from a screen above is told to
    /// the host now.
    pub fn shown(&mut self, host: &Host<'_>, docs: &Documents) {
        if let Body::Open(open) = &mut self.body
            && core::mem::take(&mut open.report_due)
        {
            open.announce(host, docs);
        }
    }

    /// The id of the book's document, for a screen of this plugin that reads
    /// it too.
    #[must_use]
    pub const fn doc_id(&self) -> Option<DocId> {
        match &self.body {
            Body::Open(open) => Some(open.id),
            Body::Message(_) => None,
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
        if let Body::Open(open) = &mut self.body {
            open.save(host, library, When::Closing);
            Self::forget(host, docs, open.id);
        }
        self.body = Body::Message("reader.no-book");
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
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
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
        let Body::Open(open) = &mut self.body else {
            return Ok(Handled::No);
        };
        if let Some(handled) = open.status_input(host, ui, input)? {
            return Ok(handled);
        }
        let marked = open.mark(host, (docs, Some(ui)), input)?;
        let choosing = matches!(marked, Outcome::Turn { .. });
        let moved = match marked {
            Outcome::Handled | Outcome::Redraw => return Ok(Handled::Yes),
            Outcome::Jump { section, offset } => {
                open.jump((section, offset));
                true
            }
            Outcome::Turn { forward: true } => open.next()?,
            Outcome::Turn { forward: false } => open.previous(host, docs)?,
            _ => match native_pages::turn(input) {
                Turn::Ignored => return Ok(Handled::No),
                Turn::Next => open.next()?,
                Turn::Previous => open.previous(host, docs)?,
            },
        };
        // A turn that did not move (the last page) leaves the link focus where it was.
        if choosing && !moved {
            open.marking.stay();
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

/// What a page is laid out with: the reading settings and the display's size
/// and density (a turned screen is another size).
#[derive(Clone, Copy, Debug, PartialEq)]
struct LayoutKey {
    reading: Reading,
    display: (u16, u16, u16),
}

/// The text the screen reads its page from: reading settings and the display.
struct Setup {
    reading: Reading,
    display: Snapshot,
    /// The room of the status lines (their size and places).
    room: native_status::Room,
}

fn integer(host: &Host<'_>, key: &str) -> Result<i64, Error> {
    let mut bytes = [0; 32];
    Ok(match settings::read_setting(host, key, &mut bytes)? {
        SettingValue::Int(value) => value,
        _ => 0,
    })
}

fn flag(host: &Host<'_>, key: &str) -> Result<bool, Error> {
    let mut bytes = [0; 32];
    Ok(matches!(
        settings::read_setting(host, key, &mut bytes)?,
        SettingValue::Bool(true)
    ))
}

impl Setup {
    fn read(host: &Host<'_>) -> Result<Self, Error> {
        let display = snapshot::capture_snapshot(host)?;
        let room = native_status::room(host, display.display_dpi())?;
        let (top, bottom) = room.reserved();
        Ok(Self {
            reading: Reading::new(
                integer(host, "core.font-size")?,
                integer(host, "core.line-spacing")?,
                integer(host, "core.word-spacing")?,
                integer(host, "core.margin")?,
                integer(host, "core.margin-ends")?,
                flag(host, "core.justify")?,
            )
            .with_status(top, bottom),
            display,
            room,
        })
    }

    /// What a page laid out with this setup depends on.
    fn key(&self) -> LayoutKey {
        LayoutKey {
            reading: self.reading,
            display: (
                self.display.display_width(),
                self.display.display_height(),
                self.display.display_dpi(),
            ),
        }
    }

    /// The size of body text in pixels.
    fn base(&self) -> f32 {
        self.reading.size_pt * f32::from(self.display.display_dpi()) / 72.0
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
                let setup = Setup::read(host)?;
                self.page = Some(self.lay(host, docs, &setup, self.at)?);
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
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
        Ok(())
    }

    /// Says that the place could not be saved; after the screen closed the
    /// device's message (`announce`) carries it, as nothing else would. Whether
    /// the host took the notice: a quiet work run (the auto-turn's tick) is
    /// refused one, and the next save tries again.
    fn say_not_saved(&self, host: &Host<'_>, closing: bool) -> bool {
        let Ok(text) = Messages::new().and_then(|mut m| m.copy(host, "reader.not-saved")) else {
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
    /// write. A refused write is said once and tried again at the next time.
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
        let (Ok(mut input), Ok(mut out)) = (buffer(2048), buffer(256)) else {
            return;
        };
        let kept = native_place::save(
            host,
            library,
            &self.path,
            self.at,
            (ppm, self.conversion, native_place::wall_seconds(host)),
            (&mut input, &mut out),
        );
        if kept {
            self.unsaved = false;
            self.saved_at = now;
        } else if !self.said {
            self.said = self.say_not_saved(host, when == When::Closing);
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
        let Ok(setup) = Setup::read(host) else {
            return;
        };
        if self.page.is_none() {
            match self.lay(host, docs, &setup, self.at) {
                Ok(page) => {
                    self.page = Some(page);
                    self.laid = Some(setup.key());
                }
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

    /// The content box: the whole display for whole-page books (as Beta 3's
    /// margins of none), else inside the reading margins.
    fn area(&self, setup: &Setup) -> Option<native_pages::Box> {
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
        redraw(host, RedrawMode::Partial)
    }

    /// What the marking makes of `input` on the page on show (nothing where the
    /// page has not been drawn).
    fn mark(
        &mut self,
        host: &Host<'_>,
        (docs, ui): (&Documents, Option<&Ui>),
        input: &Input<'_>,
    ) -> Result<Outcome, Error> {
        if self.page.is_none() {
            return Ok(Outcome::Ignored);
        }
        let setup = Setup::read(host)?;
        let area = self.area(&setup).ok_or(Error::NoSpace)?;
        let ctx = Ctx::new(
            Locator::new(self.at.0, self.at.1),
            &self.words,
            &self.path,
            &self.title,
            "",
            area,
        )
        .doc(DocRef::new(docs, self.id, self.sections, self.whole_pages));
        let ctx = match ui {
            Some(ui) => ctx.ui(ui),
            None => ctx,
        };
        self.marking.input(host, &ctx, input)
    }

    /// Follows the link `href` of the book (*Go to the note*): goes to its
    /// target, the page left kept for *Back*; whether the page moved.
    fn follow(&mut self, host: &Host<'_>, docs: &Documents, href: &str) -> Result<bool, Error> {
        let setup = Setup::read(host)?;
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
                self.jump((section, offset));
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Goes to the page that starts at `to` (a followed link, or the way back).
    fn jump(&mut self, to: (u32, u32)) {
        self.at = to;
        self.page = None;
        self.trail.clear();
        self.marking.reset();
        self.marking.landed(Locator::new(to.0, to.1));
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
                let setup = Setup::read(host)?;
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

    /// The page starting at `start`, laid out with `setup`.
    fn lay(
        &self,
        host: &Host<'_>,
        docs: &Documents,
        setup: &Setup,
        start: (u32, u32),
    ) -> Result<Page, Error> {
        self.lay_flagged(host, docs, setup, start)
            .map(|(page, _)| page)
    }

    /// The page starting at `start`, and whether the host refused a font read
    /// meanwhile (letters measured 0 wide: the page is not to be trusted).
    fn lay_flagged(
        &self,
        host: &Host<'_>,
        docs: &Documents,
        setup: &Setup,
        start: (u32, u32),
    ) -> Result<(Page, bool), Error> {
        let area = self.area(setup).ok_or(Error::NoSpace)?;
        let fonts = HostFonts::acquire(host)?;
        let page =
            Faces::new(&fonts, setup.base(), setup.reading.spacing_factor()).and_then(|faces| {
                let faces = faces.word_spacing(setup.reading.word_factor());
                let page = self.lay_with(host, docs, &faces, setup, start, area)?;
                Ok((page, faces.take_failed()))
            });
        let released = fonts.release();
        page.and_then(|page| released.map(|()| page))
    }

    fn lay_with(
        &self,
        host: &Host<'_>,
        docs: &Documents,
        faces: &Faces<&HostFonts<'_>>,
        setup: &Setup,
        start: (u32, u32),
        area: native_pages::Box,
    ) -> Result<Page, Error> {
        let document = HostDocument::new(host, docs, self.id, self.sections, self.whole_pages)?;
        let style = Style::new(0.0, 0.0, setup.base(), setup.reading.justify);
        let cx = LayoutContext::new(&document, &NoStorage, f32::from(area.width), &style, faces);
        layout_page(&cx, Locator::new(start.0, start.1), f32::from(area.height)).map_err(|error| {
            match document_error_kind(&error) {
                Some(kind) => kind,
                None => Error::Invalid,
            }
        })
    }

    fn draw(
        &mut self,
        host: &Host<'_>,
        (docs, ui): (&Documents, Option<&Ui>),
        frame: &mut DrawFrame<'_>,
    ) -> Result<(), Error> {
        let setup = Setup::read(host)?;
        let area = self.area(&setup).ok_or(Error::NoSpace)?;
        let fonts = HostFonts::acquire(host)?;
        let drawn = self.draw_with(host, (docs, ui), frame, (&setup, &fonts, area));
        let released = fonts.release();
        drawn.and(released)
    }

    fn draw_with(
        &mut self,
        host: &Host<'_>,
        (docs, ui): (&Documents, Option<&Ui>),
        frame: &mut DrawFrame<'_>,
        (setup, fonts, area): (&Setup, &HostFonts<'_>, native_pages::Box),
    ) -> Result<(), Error> {
        let faces = Faces::new(fonts, setup.base(), setup.reading.spacing_factor())?
            .word_spacing(setup.reading.word_factor());
        // A turned screen or a changed reading setting (the host may draw before
        // the plugin is told): the page is laid out again, and the pages before
        // it are found anew (their starts were those of another layout).
        let key = setup.key();
        if self.page.is_some() && self.laid != Some(key) {
            self.page = None;
            self.trail.clear();
        }
        if self.page.is_none() {
            let page = self.lay_with(host, docs, &faces, setup, self.at, area)?;
            self.page = Some(page);
            self.words = Vec::new();
            self.laid = Some(key);
        }
        let page = self.page.as_ref().ok_or(Error::State)?;
        if self.words.is_empty() {
            self.words = native_wordbox::word_boxes(&faces, page, area)?;
        }
        let rect = frame.rect;
        let canvas = frame.draw_canvas()?;
        canvas.fill_rectangle(rect, Color::WHITE)?;
        let look = Look::read(host, setup.display.display_gray_levels())?;
        let mut ink = HostInk::new(fonts, &canvas, Color::BLACK, look);
        let origin = (
            rect.x as f32 + f32::from(area.x),
            rect.y as f32 + f32::from(area.y),
        );
        draw_lines_marked(&mut ink, &faces, page, origin, self.mark.clone())?;
        self.paint_status(host, ui, frame, (setup, &area))?;
        let ctx = Ctx::new(
            Locator::new(self.at.0, self.at.1),
            &self.words,
            &self.path,
            &self.title,
            "",
            area,
        )
        .doc(DocRef::new(docs, self.id, self.sections, self.whole_pages));
        self.marking.draw_overlay(host, &ctx, frame)
    }
}

fn redraw(host: &Host<'_>, mode: RedrawMode) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(mode)
}

/// What a layout's refusal is as a host error.
fn document_error_kind(error: &DocumentError) -> Option<Error> {
    match error {
        DocumentError::OutOfMemory => Some(Error::NoMemory),
        DocumentError::NotReady => Some(Error::Busy),
        _ => None,
    }
}

/// Says why there is no page: the text over the whole screen.
fn say(host: &Host<'_>, frame: &mut DrawFrame<'_>, key: &str) -> Result<(), Error> {
    let font = Font::acquire_role(host, FontRole::Ui)?;
    let said = say_with(host, frame, &font, key);
    let released = font.release();
    said.and(released)
}

fn say_with(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    key: &str,
) -> Result<(), Error> {
    let display = snapshot::capture_snapshot(host)?;
    let style = UiStyle::read(host, &display, font)?;
    let area = paperpaw_sdk::Rect::new(0, 0, display.display_width(), display.display_height());
    let mut messages = Messages::new()?;
    let shown = messages.copy(host, key)?;
    let mut body = alloc::vec::Vec::new();
    push(&mut body, paint::Paint::Fill(area, style.paper))?;
    text::message(&mut body, font, &style, area, &shown)?;
    paint::replay(frame, font, body.iter())
}

mod aloud;
mod count;
mod r3;
mod status;

#[cfg(test)]
mod tests;
