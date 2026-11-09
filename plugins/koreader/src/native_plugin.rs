//! The interface-7 KOReader exchange: bookmarks and highlights go back and
//! forth with KOReader's files beside a book. The work starts from a book
//! opened and from a change of its annotations (`library_changed`); the
//! rules are `native_exchange`'s, this file reads what they need from the
//! library, the document and the card, and does what they say, in this
//! order: a copy aside, the edits of the library's annotations (one
//! transaction), KOReader's file, what both have. What is not here yet is
//! in `PORTING-7.md`.
use crate::{
    declarations::EVENTS_TOKEN,
    native_exchange::{self as exchange, Edit, Mine, Placer, Removal, Setup, Stop, Theirs},
    native_model::{self as model, Last},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    annotations::Kind,
    document::Locator,
    native::{
        Error, Host, Plugin,
        device::{Device, DeviceClass},
        document::{DocId, DocPosition, Documents, OpenBook, Preparation},
        events::{AnswerOutcome, Answered, Event, Incoming, kinds},
        library::{
            AnnotationCursor, AnnotationEdit, AnnotationKind, AnnotationTextKind,
            AnnotationTextQuery, BookRef, Library,
        },
        registrations,
        reply::Reply,
        snapshot::capture_snapshot,
        storage::{self, Root},
        ui::{Question, RequestId, Ui},
    },
};
use paperpaw_ui_kit::{
    Messages, files,
    mem::{SERVICE_BUFFER, buffer, push},
    text::{concat, copy},
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/koreader_declarations.rs"));
}

/// Longest KOReader file read (Beta 3's, for devices that have the memory: a
/// refused allocation ends the exchange, never aborts).
pub const MAX_FILE: usize = 4 << 20;
/// Longest KOReader file read on a small device (some fifty annotations); the
/// books of larger ones are not exchanged there.
pub const MAX_FILE_SMALL: usize = 32 << 10;

/// The longest file read on a device of `class`: the large bound only where
/// the device is known to have the memory.
pub const fn file_cap(class: Result<DeviceClass, Error>) -> usize {
    match class {
        Ok(DeviceClass::Medium | DeviceClass::Large) => MAX_FILE,
        _ => MAX_FILE_SMALL,
    }
}

/// The class of the device, read once through `core.device`.
fn read_class(host: &Host<'_>) -> Result<DeviceClass, Error> {
    let device = Device::open(host)?;
    let class = (|| {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        Ok(device.device(host, &mut input, &mut out)?.class())
    })();
    // A handle that cannot be released is no reason to lose the class.
    #[expect(
        clippy::let_underscore_must_use,
        reason = "the class was read; a refused release changes nothing for the exchange"
    )]
    let _ = device.release(host);
    class
}
/// Longest file of what is known read.
const MAX_KNOWN: usize = 1 << 20;
/// Longest text or note of an annotation read whole.
const MAX_TEXT: usize = 4096;
/// Annotations read of a book at most (the host's file cap is 1 MiB).
const MOST_ANNOTATIONS: usize = 2048;

/// What an exchange of a book came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exchanged {
    Same,
    Changed,
    /// KOReader's file has none any more of what both had: the reader is to
    /// say whether to remove them here too.
    Ask,
}

/// What a question asked for.
#[derive(Debug)]
struct Asked {
    request: RequestId,
    book: String,
}

/// The interface-7 KOReader exchange.
#[derive(Debug, Default)]
pub struct NativeKoReader {
    library: Option<Library>,
    /// The longest KOReader file read on this device.
    file_cap: Option<usize>,
    documents: Option<Documents>,
    ui: Option<Ui>,
    /// The question for the reader whether to remove what KOReader's file
    /// has no more, while it is open.
    asked: Option<Asked>,
    /// Books the reader was asked about in this run (once each).
    asked_books: Vec<String>,
    /// The book whose annotations this plugin changed last: the change
    /// comes back as an event and needs no exchange.
    own: Option<String>,
}

impl Plugin for NativeKoReader {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::NAVIGATION {
            registrations::submit(host, batch)?;
        }
        // The services are opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        if let Some(Event::Answered { answer, .. }) = incoming.decode_as::<kinds::Answered>()? {
            return self.answered(host, answer);
        }
        if token != EVENTS_TOKEN {
            return Ok(());
        }
        if let Some(Event::Book { reading, .. }) = incoming.decode_as::<kinds::BookOpened>()? {
            return self.exchange_or_ask(host, reading.path(), Removal::Ask);
        }
        if let Some(Event::LibraryChanged { what, path, .. }) =
            incoming.decode_as::<kinds::LibraryChanged>()?
            && what.annotations()
            && !path.is_empty()
        {
            if self.own.as_deref() == Some(path) {
                self.own = None;
                return Ok(());
            }
            self.note_emptied(host, path)?;
            return self.exchange_or_ask(host, path, Removal::Ask);
        }
        Ok(())
    }
}

/// The host's document as KOReader's positions see it.
struct HostBook<'a, 'h> {
    host: &'a Host<'h>,
    documents: &'a Documents,
    id: DocId,
    input: Vec<u8>,
    out: Vec<u8>,
}

impl Placer for HostBook<'_, '_> {
    fn xpointer(&mut self, at: Locator) -> Result<Option<String>, Error> {
        match self.documents.xpointer(
            self.host,
            self.id,
            DocPosition::new(at.section, at.offset),
            &mut self.input,
            &mut self.out,
        ) {
            Ok(text) => copy(text).map(Some),
            Err(Error::NotFound | Error::Unsupported | Error::Invalid) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn locate(&mut self, xpointer: &str) -> Result<Option<Locator>, Error> {
        match self
            .documents
            .locate(self.host, self.id, xpointer, &mut self.input, &mut self.out)
        {
            Ok(at) => Ok(Some(Locator::new(at.section, at.offset))),
            Err(Error::NotFound | Error::Invalid | Error::NoSpace) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// The key the plugin's per-book folder is named by, and the files in it.
fn known_name(key: &str) -> Result<String, Error> {
    concat(&[
        "books/",
        crate::declarations::BOOK_DATA,
        "/",
        key,
        "/known.txt",
    ])
}

impl NativeKoReader {
    fn ensure(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.file_cap.is_none() {
            self.file_cap = Some(file_cap(read_class(host)));
        }
        if self.library.is_none() {
            self.library = Some(Library::open(host)?);
        }
        if self.documents.is_none() {
            self.documents = Some(Documents::open(host)?);
        }
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    fn library(&self) -> Result<&Library, Error> {
        self.library.as_ref().ok_or(Error::State)
    }

    /// Exchanges `book` as `removal` says; where the reader is to decide,
    /// asks once in this run (the question's *Back* keeps them).
    fn exchange_or_ask(
        &mut self,
        host: &Host<'_>,
        book: &str,
        removal: Removal,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        match self.exchange_book(host, book, removal)? {
            Exchanged::Changed => {
                self.own = Some(copy(book)?);
                Ok(())
            }
            Exchanged::Ask if !self.asked_books.iter().any(|b| b == book) => {
                push(&mut self.asked_books, copy(book)?)?;
                self.ask_removal(host, book)
            }
            Exchanged::Ask | Exchanged::Same => Ok(()),
        }
    }

    fn ask_removal(&mut self, host: &Host<'_>, book: &str) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let title = say.copy(host, "koreader.removed-there")?;
        let text = say.copy(host, "koreader.removed-there.text")?;
        let yes = say.copy(host, "koreader.removed-there.remove")?;
        let no = say.copy(host, "koreader.removed-there.keep")?;
        let labels = concat(&[&yes, "\n", &no])?;
        let question = Question::confirm(&title, &text)?.confirm_labels(&labels)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .ask(host, question, &mut input, &mut out)?;
        self.asked = Some(Asked {
            request,
            book: copy(book)?,
        });
        Ok(())
    }

    /// The reader's answer: *Remove* removes here what KOReader's file has no
    /// more (a copy kept aside), anything else keeps them and offers them to
    /// KOReader again.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        if self.asked.as_ref().map(|a| a.request) != Some(answer.request())
            || answer.outcome() == AnswerOutcome::Typing
        {
            return Ok(());
        }
        let Some(asked) = self.asked.take() else {
            return Ok(());
        };
        let removal = if answer.outcome() == AnswerOutcome::Answered && answer.choice() == 1 {
            Removal::Remove
        } else {
            Removal::Keep
        };
        self.exchange_or_ask(host, &asked.book, removal)
    }

    /// The annotations here changed: where none are left of what both had,
    /// the user removed them all, which is kept for the next exchange to pass
    /// on. None here when a book is opened is rather its data moved halfway.
    fn note_emptied(&mut self, host: &Host<'_>, book: &str) -> Result<(), Error> {
        self.ensure(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let page = match self.library()?.annotations(
            host,
            BookRef::path(book)?,
            AnnotationCursor::start(),
            &mut input,
            &mut out,
        ) {
            Ok(page) => page,
            Err(Error::NotFound | Error::State) => return Ok(()),
            Err(error) => return Err(error),
        };
        if page.end().total() > 0 {
            return Ok(());
        }
        let Some((name, last)) = self.known(host, book)? else {
            return Ok(());
        };
        if last.emptied || last.known.is_empty() {
            return Ok(());
        }
        // Not kept: the next exchange changes nothing on either side, as
        // before the removal was told.
        storage::append(
            host,
            Root::Data,
            &name,
            concat(&[model::EMPTIED, "\n"])?.as_bytes(),
        )
        .ok();
        Ok(())
    }

    /// The name of what is known of `book` and what it says; none where the
    /// library does not know the book.
    fn known(&self, host: &Host<'_>, book: &str) -> Result<Option<(String, Last)>, Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let key = match self
            .library()?
            .book_key(host, BookRef::path(book)?, &mut input, &mut out)
        {
            Ok(key) => known_name(key.key())?,
            Err(Error::NotFound | Error::State) => return Ok(None),
            Err(error) => return Err(error),
        };
        let last = match files::read_all(host, Root::Data, &key, MAX_KNOWN)? {
            Some(bytes) => {
                let text = core::str::from_utf8(&bytes).map_err(|_| Error::Utf8)?;
                model::parse_known(text)?
            }
            None => Last::default(),
        };
        Ok(Some((key, last)))
    }

    /// KOReader's side of `book`: its annotations file for KOReader while
    /// that waits, else KOReader's own; none while it keeps no file, also
    /// while a sync has brought its folder but not yet the file in it.
    fn read_theirs(host: &Host<'_>, book: &str, cap: usize) -> Result<Option<Theirs>, Stop> {
        let waiting = model::exchange_file(book)?;
        let pending = match storage::size(host, Root::Files, &waiting) {
            Ok(_) => true,
            Err(Error::NotFound) => false,
            Err(_) => return Err(Stop::Unreadable),
        };
        let path = if pending {
            waiting
        } else {
            model::metadata_file(book)?
        };
        let bytes = match files::read_all(host, Root::Files, &path, cap) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(None),
            Err(Error::NoMemory) => return Err(Stop::Fail(Error::NoMemory)),
            Err(_) => return Err(Stop::Unreadable),
        };
        let text = core::str::from_utf8(&bytes).map_err(|_| Stop::Unreadable)?;
        let (items, listed) = exchange::items_of(text)?;
        Ok(Some(Theirs {
            items,
            pending,
            listed,
        }))
    }

    /// Exchanges the annotations of `book` with KOReader's, if KOReader keeps
    /// any for it.
    fn exchange_book(
        &self,
        host: &Host<'_>,
        book: &str,
        removal: Removal,
    ) -> Result<Exchanged, Error> {
        // KOReader's side first: most books have none, and then the book
        // need not be opened.
        let theirs = match Self::read_theirs(host, book, self.file_cap.unwrap_or(MAX_FILE_SMALL)) {
            Ok(Some(theirs)) => theirs,
            Ok(None) | Err(Stop::Unreadable) => return Ok(Exchanged::Same),
            Err(Stop::Fail(error)) => return Err(error),
        };
        let Some((known_file, last)) = self.known(host, book)? else {
            return Ok(Exchanged::Same);
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let library = self.library.as_ref().ok_or(Error::State)?;
        let documents = self.documents.as_ref().ok_or(Error::State)?;
        let state = documents.open_book(host, &OpenBook::new(book), &mut input, &mut out)?;
        let (id, ready, conversion) = (
            state.id(),
            state.preparation() == Preparation::Ready && state.has_xpointers(),
            state.conversion(),
        );
        let result = if ready {
            Self::run(
                host,
                library,
                documents,
                id,
                conversion,
                book,
                removal,
                &known_file,
                &last,
                theirs,
            )
        } else {
            // Still being prepared (a later event tries again), or a format
            // whose places are not KOReader's.
            Ok(Exchanged::Same)
        };
        let closed = documents.close(host, id, &mut input, &mut out);
        let result = result?;
        closed?;
        Ok(result)
    }

    #[expect(clippy::too_many_arguments, reason = "one exchange's inputs, named")]
    fn run(
        host: &Host<'_>,
        library: &Library,
        documents: &Documents,
        id: DocId,
        conversion: u64,
        book: &str,
        removal: Removal,
        known_file: &str,
        last: &Last,
        theirs: Theirs,
    ) -> Result<Exchanged, Error> {
        let mut doc = HostBook {
            host,
            documents,
            id,
            input: buffer(SERVICE_BUFFER)?,
            out: buffer(SERVICE_BUFFER)?,
        };
        // Asked before the book's annotations are read: nothing changes.
        let Some((mut ours, generation)) = read_ours(host, library, book, conversion, &mut doc)?
        else {
            return Ok(Exchanged::Same);
        };
        if removal == Removal::Ask && exchange::removed_there(last, &theirs, ours.len()) {
            return Ok(Exchanged::Ask);
        }
        let snapshot = capture_snapshot(host)?;
        let setup = Setup {
            removal,
            now: snapshot.unix_seconds().unwrap_or(0),
            offset_minutes: i64::from(snapshot.utc_offset_minutes().unwrap_or(0)),
            // How highlights look here is not asked of the reader yet.
            drawer: "lighten",
        };
        let outcome = match exchange::exchange(&setup, &mut doc, last, &mut ours, theirs) {
            Ok(outcome) => outcome,
            Err(Stop::Unreadable) => return Ok(Exchanged::Same),
            Err(Stop::Fail(error)) => return Err(error),
        };
        if outcome.ours_changed {
            if outcome.keep_aside && !keep_aside(host, known_file, &ours) {
                return Ok(Exchanged::Same);
            }
            for a in ours.iter_mut().filter(|a| a.edit == Edit::Add) {
                a.conversion = Some(conversion);
                if let (Some(start), Some(end)) = (doc.xpointer(a.start)?, doc.xpointer(a.end)?) {
                    a.xpointers = Some((start, end));
                }
            }
            // Not kept here, what came from KOReader must not count as known:
            // the next exchange would take it for removed here.
            match commit(host, library, book, generation, &ours) {
                Ok(()) => {}
                Err(Error::State) => return Ok(Exchanged::Same),
                Err(error) => return Err(error),
            }
        }
        // What was offered must not count as known where it was not written:
        // the next exchange would take it for removed in KOReader.
        let passed = match &outcome.file {
            Some(file) => files::replace(
                host,
                Root::Files,
                &model::exchange_file(book)?,
                file.as_bytes(),
            )
            .is_ok(),
            None => true,
        };
        if passed && outcome.save {
            let text = model::write_known(&outcome.known, outcome.seen)?;
            // Where this fails the next exchange takes it up again.
            files::replace(host, Root::Data, known_file, text.as_bytes()).ok();
        }
        Ok(if outcome.ours_changed {
            Exchanged::Changed
        } else {
            Exchanged::Same
        })
    }
}

/// What to do where a page of annotations is read and a next one remains.
#[derive(Debug, PartialEq, Eq)]
enum More {
    Read,
    /// `MOST_ANNOTATIONS` are read and more remain.
    TooMany,
}

/// Whether the next page is read, `read` annotations being in hand.
const fn after_page(read: usize) -> More {
    if read < MOST_ANNOTATIONS {
        More::Read
    } else {
        More::TooMany
    }
}

/// The annotations of `book`, in the conversion the reader shows it in, and
/// the generation they were read in; `None` where the book has more than
/// `MOST_ANNOTATIONS` (the exchange then leaves everything as it is). Those made in another conversion are
/// placed through their XPointers, so they are found where KOReader's are.
fn read_ours(
    host: &Host<'_>,
    library: &Library,
    book: &str,
    conversion: u64,
    doc: &mut HostBook<'_, '_>,
) -> Result<Option<(Vec<Mine>, u32)>, Error> {
    let reference = BookRef::path(book)?;
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut ours: Vec<Mine> = Vec::new();
    let mut cursor = AnnotationCursor::start();
    let mut generation;
    loop {
        let page = library.annotations(host, reference, cursor, &mut input, &mut out)?;
        generation = page.end().generation();
        let next = page.end().next();
        // The texts are asked whole below, with the buffers free again.
        let mut rows: Vec<(Mine, bool, bool)> = Vec::new();
        for row in page.rows() {
            let a = row?;
            let kind = match a.kind() {
                AnnotationKind::Bookmark => Kind::Bookmark,
                AnnotationKind::Highlight => Kind::Highlight,
                _ => continue,
            };
            push(
                &mut rows,
                (
                    Mine {
                        id: a.id(),
                        kind,
                        start: Locator::new(a.start_section(), a.start_offset()),
                        end: Locator::new(a.end_section(), a.end_offset()),
                        percent: u8::try_from(a.percent()).unwrap_or(0),
                        created: i64::try_from(a.created()).unwrap_or(0),
                        updated: i64::try_from(a.updated()).unwrap_or(0),
                        text: copy(a.text())?,
                        note: copy(a.note())?,
                        client_key: copy(a.client_key())?,
                        conversion: a.conversion(),
                        xpointers: None,
                        edit: Edit::None,
                    },
                    a.text_cut(),
                    a.note_cut(),
                ),
            )?;
        }
        for (mut mine, text_cut, note_cut) in rows {
            if text_cut {
                mine.text = whole(host, library, reference, mine.id, AnnotationTextKind::Text)?;
            }
            if note_cut {
                mine.note = whole(host, library, reference, mine.id, AnnotationTextKind::Note)?;
            }
            push(&mut ours, mine)?;
        }
        match next {
            None => break,
            Some(next) => match after_page(ours.len()) {
                More::Read => cursor = next,
                // Cut here, KOReader's items missing from the list would be
                // taken for removed here: nothing is exchanged.
                More::TooMany => return Ok(None),
            },
        }
    }
    place(host, library, reference, conversion, doc, &mut ours)?;
    Ok(Some((ours, generation)))
}

/// A text or note of annotation `id` whole, up to `MAX_TEXT` bytes.
fn whole(
    host: &Host<'_>,
    library: &Library,
    book: BookRef<'_>,
    id: u32,
    kind: AnnotationTextKind,
) -> Result<String, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut text = String::new();
    let mut offset = 0;
    loop {
        let part = library.annotation_text(
            host,
            book,
            AnnotationTextQuery::new(id, kind).from(offset),
            &mut input,
            &mut out,
        )?;
        if text.len() + part.text().len() > MAX_TEXT {
            return Ok(text);
        }
        text.try_reserve(part.text().len())
            .map_err(|_| Error::NoMemory)?;
        text.push_str(part.text());
        match part.next_offset() {
            Some(next) => offset = next,
            None => return Ok(text),
        }
    }
}

/// Places the annotations made in another conversion than `conversion`
/// through their XPointers (those the document does not find stay as they
/// are); the XPointers of the rest are not needed.
fn place(
    host: &Host<'_>,
    library: &Library,
    book: BookRef<'_>,
    conversion: u64,
    doc: &mut HostBook<'_, '_>,
    ours: &mut [Mine],
) -> Result<(), Error> {
    if ours
        .iter()
        .all(|a| a.conversion.is_none_or(|c| c == conversion))
    {
        return Ok(());
    }
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut cursor = AnnotationCursor::start();
    loop {
        let page = library.anchors(host, book, cursor, &mut input, &mut out)?;
        for anchor in page.rows() {
            let Some((start, end)) = anchor.xpointers() else {
                continue;
            };
            let Some(a) = ours
                .iter_mut()
                .find(|a| a.id == anchor.id() && a.conversion.is_some_and(|c| c != conversion))
            else {
                continue;
            };
            if let (Some(from), Some(to)) = (doc.locate(start)?, doc.locate(end)?) {
                (a.start, a.end, a.conversion) = (from, to, Some(conversion));
                a.xpointers = Some((copy(start)?, copy(end)?));
            }
        }
        match page.end().next() {
            Some(next) if !page.is_empty() => cursor = next,
            _ => return Ok(()),
        }
    }
}

/// The annotations changed as `ours` says, in one transaction: all or none.
fn commit(
    host: &Host<'_>,
    library: &Library,
    book: &str,
    generation: u32,
    ours: &[Mine],
) -> Result<(), Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut edits: Vec<AnnotationEdit<'_>> = Vec::new();
    for a in ours.iter().filter(|a| a.edit != Edit::None) {
        push(&mut edits, edit_of(a)?)?;
    }
    if edits.is_empty() {
        return Ok(());
    }
    let mut transaction =
        library.edit_begin(host, BookRef::path(book)?, generation, &mut input, &mut out)?;
    for chunk in edits.chunks(3) {
        if let Err(error) = library.edit_add(host, &mut transaction, chunk, &mut input, &mut out) {
            // The transaction is given up; nothing is applied.
            library
                .edit_abort(host, &mut transaction, &mut input, &mut out)
                .ok();
            return Err(error);
        }
    }
    library
        .edit_commit(host, &mut transaction, &mut input, &mut out)
        .map(|_| ())
}

/// The edit that makes the library's annotation what `a` is.
fn edit_of(a: &Mine) -> Result<AnnotationEdit<'_>, Error> {
    let kind = match a.kind {
        Kind::Bookmark => AnnotationKind::Bookmark,
        _ => AnnotationKind::Highlight,
    };
    let (start, end) = (
        (a.start.section, a.start.offset),
        (a.end.section, a.end.offset),
    );
    let edit = match a.edit {
        Edit::Remove => return AnnotationEdit::remove(a.id),
        Edit::Replace => AnnotationEdit::replace(a.id, kind, start, end)?,
        // KOReader's own name for it: a repeated import adds nothing twice.
        _ if !a.client_key.is_empty() => AnnotationEdit::upsert(&a.client_key, kind, start, end),
        _ => AnnotationEdit::add(kind, start, end),
    };
    let mut edit = edit
        .percent(u32::from(a.percent))
        .times(
            u64::try_from(a.created).unwrap_or(0),
            u64::try_from(a.updated).unwrap_or(0),
        )
        .text(&a.text)
        .note(&a.note);
    if let Some(conversion) = a.conversion {
        edit = edit.conversion(conversion);
    }
    if let Some((from, to)) = &a.xpointers {
        edit = edit.xpointers(from, to);
    }
    Ok(edit)
}

/// Keeps a copy of the annotations beside what is known of the book
/// (`<known file>.removed`, numbered where there is one), never over
/// another; `false` where it could not be made.
fn keep_aside(host: &Host<'_>, known_file: &str, ours: &[Mine]) -> bool {
    let Ok(text) = exchange::aside(ours) else {
        return false;
    };
    for n in 0..100 {
        let mut digits = [0; 20];
        let name = if n == 0 {
            concat(&[known_file, ".removed"])
        } else {
            concat(&[
                known_file,
                ".removed.",
                paperpaw_ui_kit::text::decimal(n, &mut digits),
            ])
        };
        let Ok(name) = name else {
            return false;
        };
        match storage::size(host, Root::Data, &name) {
            Err(Error::NotFound) => {
                return files::replace(host, Root::Data, &name, text.as_bytes()).is_ok();
            }
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_book_with_more_annotations_than_are_read_is_not_exchanged() {
        assert_eq!(after_page(0), More::Read);
        assert_eq!(after_page(MOST_ANNOTATIONS - 1), More::Read);
        // At the bound with a page still to come: not exchanged, never cut.
        assert_eq!(after_page(MOST_ANNOTATIONS), More::TooMany);
        assert_eq!(after_page(MOST_ANNOTATIONS + 5), More::TooMany);
    }

    #[test]
    fn the_file_bound_follows_the_device_class() {
        assert_eq!(file_cap(Ok(DeviceClass::Small)), 32 << 10);
        assert_eq!(file_cap(Ok(DeviceClass::Medium)), 4 << 20);
        assert_eq!(file_cap(Ok(DeviceClass::Large)), 4 << 20);
        // Unknown: the small bound.
        assert_eq!(file_cap(Err(Error::Unsupported)), 32 << 10);
    }
}
