//! The book being read: opening it, laying out its pages, where the
//! reader is (kept in `progress.ini`), the page index, chapters and
//! links.

use super::*;

/// The book being read.
pub(super) struct Book {
    pub(super) path: String,
    /// Directory for this book's data.
    pub(super) dir: String,
    pub(super) title: String,
    pub(super) document: Box<dyn Document>,
    /// The book's language, or the UI language if it declares none.
    pub(super) language: String,
    pub(super) typography: Typography,
    pub(super) page: Page,
    /// The words of the page, once measured (see [`Book::words`]).
    pub(super) words: core::cell::OnceCell<Vec<WordBox>>,
    /// Its lines as page layers see them, while there are layers (see
    /// `Book::lines`).
    pub(super) lines: core::cell::OnceCell<Vec<paperpaw_sdk::layers::LineBox>>,
    /// The starts of the pages turned forward to this one, one after
    /// another (none before a jump).
    pub(super) history: Vec<Locator>,
    /// Whether the page shown has changed since the position was
    /// written, since when, and when it is written ([`Book::save_due`]).
    pub(super) unsaved: bool,
    pub(super) unsaved_since: Option<paperpaw_sdk::Instant>,
    pub(super) save_at: Option<paperpaw_sdk::Instant>,
    /// Whether it could not be written once (and the reader was told).
    pub(super) save_failed: bool,
    /// The position kept is there but could not be read: not written over
    /// until it is moved aside (see [`Book::set_aside`]).
    pub(super) held: bool,
    /// The position was read from the reader's `progress.ini` of before:
    /// removed once the position is kept where it is now.
    pub(super) from_progress: bool,
    /// The conversion of the book its positions are in (see
    /// `reading::conversion`), and whether the position kept holds its
    /// XPointer too, to find it in another; the XPointer found last.
    pub(super) conversion: u64,
    pub(super) pointed: bool,
    pub(super) xpointer: Option<String>,
    /// What the book is by its content, kept with its position and
    /// annotations: taken once when it opens (a few reads of 1 KiB), not
    /// at every page; `None` where the file cannot be read for it.
    pub(super) fingerprint: Option<paperpaw_sdk::library::Fingerprint>,
    pub(super) index: Option<PageIndex>,
    /// Where background pagination continues.
    pub(super) indexing: Option<Locator>,
    /// Whether pagination stopped for this layout (a page that cannot be
    /// laid out, a storage that takes no more).
    pub(super) index_stopped: bool,
    /// Where the bookmarks and highlights are.
    pub(super) marks: Vec<Mark>,
    /// Text to point out on this page, e.g. a search hit.
    pub(super) found: Option<(Locator, Locator)>,
    /// The chapter of the page, for status lines and the reading events:
    /// where it starts, where the next one starts and its title (empty
    /// before the first).
    pub(super) chapter: Option<(Locator, Option<Locator>, String)>,
    /// The chapter's last page as the page index has it: where the next
    /// chapter starts, the index's pages then, and the page (found once
    /// a chapter, not every page turned).
    pub(super) chapter_end: core::cell::Cell<Option<(Locator, u32, u32)>>,
    /// The same for the page the chapter begins on.
    pub(super) chapter_first: core::cell::Cell<Option<(Locator, u32, u32)>>,
    /// The book's own fonts, if used.
    pub(super) embedded: Option<Embedded>,
    /// The links on the page.
    pub(super) links: Vec<Link>,
    /// Where links were followed from, the last last.
    pub(super) returns: Vec<Locator>,
    /// The status lines chosen last, top then bottom.
    pub(super) status_lines: [String; 2],
    /// Where the top-level chapters begin, as thousandths of the book
    /// (see `status_context::CHAPTERS`).
    pub(super) chapter_marks: String,
    /// The page the last link led to: it offers the way back.
    pub(super) landed: Option<Locator>,
    /// A page a plugin shows one after another (see
    /// [`paperpaw_sdk::readers::open_page`]): it offers the way back
    /// always.
    pub(super) page_back: bool,
    /// Read aloud (`volume`) or turning its pages by itself (`clock`):
    /// the icon shown in the status line.
    pub(super) hands_free: Option<&'static str>,
    /// Its last page was reached ([`events::BOOK_FINISHED`] was emitted).
    pub(super) finished: bool,
    /// What plugins' decorators mark on the page, and what they were
    /// asked for: the page's start and their signature.
    pub(super) decorated: Vec<paperpaw_sdk::decorations::Mark>,
    pub(super) decorated_for: Option<(Locator, u64)>,
    /// Where runs of blocks the text filters leave out end, read once.
    pub(super) dropped: paperpaw_sdk::reading::DroppedRuns,
}

/// What the buttons choose on a page, as links are chosen: a link, or a
/// mark whose plugin hears taps on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    /// Of [`Book::links`].
    Link(usize),
    /// Of [`Book::decorated`].
    Mark(usize),
    /// The room of a page layer that hears taps, of the type's `rooms`.
    Layer(usize),
    /// An item a page layer drew, of [`Book::layer_items`].
    Item(usize),
}

/// Why a book did not open: the message to show instead, and whether it
/// is only not there yet (`DocumentError::NotReady`), to be opened once
/// it is.
#[derive(Debug)]
pub(super) enum Unopened {
    Failed(String),
    NotReady(String),
}

impl From<String> for Unopened {
    fn from(message: String) -> Self {
        Unopened::Failed(message)
    }
}

impl Unopened {
    pub(super) fn not_ready(&self) -> bool {
        matches!(self, Unopened::NotReady(_))
    }

    pub(super) fn message(self) -> String {
        match self {
            Unopened::Failed(message) | Unopened::NotReady(message) => message,
        }
    }
}

/// Remembers `path` as the book read last (reopened at start-up).
pub(crate) fn remember_last(storage: &dyn paperpaw_sdk::Storage, path: &str) {
    let state = format!("[reader]\nlast = {}\n", paperpaw_sdk::config::quote(path));
    // Mostly the same book again: a read is quicker than a write.
    if paperpaw_sdk::read_file(storage, &state_path(), state.len())
        .is_ok_and(|kept| kept == state.as_bytes())
    {
        return;
    }
    #[expect(
        clippy::let_underscore_must_use,
        reason = "only which book opens at the start; not kept, the library opens instead"
    )]
    let _ = storage.write(&state_path(), state.as_bytes());
}

/// Moves the annotations the reader kept with its data of the book at
/// `path` before plugins shared them where they are now, if any are
/// there. Where the book has annotations there too, both stay: a move
/// would replace them.
fn move_old_annotations(storage: &dyn paperpaw_sdk::Storage, dir: &str, path: &str) {
    let old = format!("{dir}/annotations.txt");
    let new = paperpaw_sdk::annotations::file(path);
    if storage.exists(&old)
        && matches!(
            storage.size(&new),
            Err(paperpaw_sdk::StorageError::NotFound)
        )
    {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "the file of an earlier version: where it stays, it is moved again at the next opening"
        )]
        let _ = storage.rename(&old, &new);
    }
}

/// The data folder of the book at `path`.
pub(crate) fn book_dir(path: &str) -> String {
    format!(
        "{}/books/{:016x}",
        Env::plugin_dir(PLUGIN.as_str()),
        fnv1a64(path)
    )
}

impl Book {
    /// Opens the book named by the `path` parameter; errors are messages
    /// to show instead of the book.
    pub(super) fn open(env: &Env<'_>, params: &Params) -> Result<Book, Unopened> {
        let path = params
            .get(params::PATH)
            .ok_or_else(|| env.i18n().tr("reader.no-book").to_string())?;
        let tr = |key| env.i18n().tr(key).to_string();
        let error = |e: DocumentError| match e {
            DocumentError::NotReady => Unopened::NotReady(tr("reader.not-ready")),
            // Not the book's fault: it opens with more memory free.
            DocumentError::OutOfMemory => Unopened::Failed(tr("reader.no-memory")),
            DocumentError::Storage(paperpaw_sdk::StorageError::NotFound) => {
                Unopened::Failed(tr("reader.missing"))
            }
            e => Unopened::Failed(format!("{}\n{e}", tr("reader.error"))),
        };
        let options = paperpaw_sdk::reading::open_options(env);
        let document = env
            .formats()
            .open_with(env.storage(), path, &options)
            .map_err(error)?;
        let font = env
            .fonts()
            .role(fonts::READING.as_str())
            .ok_or_else(|| env.i18n().tr("reader.no-font").to_string())?;
        let language = document
            .metadata()
            .language
            .clone()
            .unwrap_or_else(|| env.i18n().language().to_string());
        let typography = typography(env, &font, &language, document.whole_pages());
        let dir = book_dir(path);
        move_old_annotations(env.storage(), &dir, path);
        let title = document
            .metadata()
            .title
            .clone()
            .unwrap_or_else(|| path.to_string());

        let mut book = Book {
            path: path.to_string(),
            index: None,
            indexing: None,
            index_stopped: false,
            page: Page::default(),
            words: core::cell::OnceCell::new(),
            lines: core::cell::OnceCell::new(),
            history: Vec::new(),
            unsaved: false,
            unsaved_since: None,
            save_at: None,
            save_failed: false,
            held: false,
            from_progress: false,
            conversion: paperpaw_sdk::reading::conversion(env),
            pointed: true,
            xpointer: None,
            fingerprint: paperpaw_sdk::library::Fingerprint::of(env.storage(), path),
            dir,
            title,
            document,
            language,
            typography,
            marks: Vec::new(),
            found: None,
            chapter: None,
            chapter_end: core::cell::Cell::new(None),
            chapter_first: core::cell::Cell::new(None),
            embedded: None,
            links: Vec::new(),
            returns: Vec::new(),
            status_lines: status::load_choice(env.storage()),
            chapter_marks: String::new(),
            landed: None,
            page_back: params.get(paperpaw_sdk::readers::PAGE).is_some(),
            hands_free: None,
            finished: false,
            decorated: Vec::new(),
            decorated_for: None,
            dropped: paperpaw_sdk::reading::DroppedRuns::new(),
        };
        book.reload_marks(env);
        book.chapter_marks = book.chapter_marks(env);
        book.embedded = Embedded::load(env, book.document.as_ref());
        // Kept with another layout maybe: from the start of its line.
        // Where the book cannot be shown there (it changed, say), from its
        // start.
        let saved = book.saved_position(env);
        let start = saved.map_or(Locator::START, |(at, _)| book.line_start(env, at));
        // Found in another conversion, or kept before: kept anew.
        book.unsaved = saved.is_some_and(|(_, anew)| anew);
        book.page = match book.layout(env, start) {
            Ok(page) if !page.is_past_end() => page,
            // Not from the start instead: the place read to would be
            // kept as the start.
            Err(DocumentError::OutOfMemory) => return Err(error(DocumentError::OutOfMemory)),
            _ if start != Locator::START => book.layout(env, Locator::START).map_err(error)?,
            page => page.map_err(error)?,
        };
        book.prepare_images(env);
        book.index = Some(book.open_index(env));
        book.update_chapter(env);
        book.update_links(env);
        if !book.web() {
            remember_last(env.storage(), &book.path);
        }
        Ok(book)
    }

    /// Where the book is read, for [`events::BOOK_OPENED`] and the other
    /// standard reading events; the page and the number of pages once
    /// they are counted.
    pub(super) fn reading(&self, env: &Env<'_>) -> events::Reading {
        let counted = self
            .index
            .as_ref()
            .filter(|i| i.is_complete())
            .and_then(|index| match index.locate(env.storage(), self.page.start) {
                Some((page, true)) => Some((page + 1, index.pages())),
                _ => None,
            });
        let chapter = self.chapter.as_ref().map(|(_, _, title)| title.clone());
        events::Reading::new(
            self.path.clone(),
            self.title.clone(),
            // One decimal, as written before; 100 once the last page was
            // reached (the page's start is how far the book is read only
            // until then).
            if self.finished {
                100.0
            } else {
                (self.percent() * 10.0 + 0.5) as u32 as f32 / 10.0
            },
            self.page.start.section,
            self.page.start.offset,
            chapter.unwrap_or_default(),
        )
        .page(counted.map(|(page, _)| page))
        .pages(counted.map(|(_, pages)| pages))
        .chapter_left(counted.and_then(|(page, _)| self.chapter_left(env, page - 1)))
    }

    /// The pages after page `page` (from 0) up to the chapter's end, where
    /// the chapter is known and the pages are counted.
    fn chapter_left(&self, env: &Env<'_>, page: u32) -> Option<u32> {
        let index = self.index.as_ref().filter(|i| i.is_complete())?;
        let (_, next, title) = self.chapter.as_ref()?;
        if title.is_empty() {
            return None;
        }
        let last = match *next {
            // The last chapter ends with the book.
            None => index.pages().checked_sub(1)?,
            Some(next) => match self.chapter_end.get() {
                Some((at, pages, last)) if at == next && pages == index.pages() => last,
                _ => {
                    // The page the next chapter starts on, or the one
                    // before if it starts with its page.
                    let last = match index.locate(env.storage(), next)? {
                        (page, true) => page.checked_sub(1)?,
                        (page, false) => page,
                    };
                    self.chapter_end.set(Some((next, index.pages(), last)));
                    last
                }
            },
        };
        Some(last.saturating_sub(page))
    }

    /// The page (from 0) the chapter begins on, where the chapter is
    /// known and the pages are counted.
    pub(super) fn chapter_first(&self, env: &Env<'_>) -> Option<u32> {
        let index = self.index.as_ref().filter(|i| i.is_complete())?;
        let (start, _, title) = self.chapter.as_ref()?;
        if title.is_empty() {
            return None;
        }
        if let Some((at, pages, first)) = self.chapter_first.get()
            && at == *start
            && pages == index.pages()
        {
            return Some(first);
        }
        let (first, _) = index.locate(env.storage(), *start)?;
        self.chapter_first.set(Some((*start, index.pages(), first)));
        Some(first)
    }

    /// The page index of this layout of the book as it is now: its name
    /// tells layouts and editions of the book (by size) apart. A new one
    /// removes those of other layouts once more than a few are kept;
    /// switching back finds the others.
    pub(super) fn open_index(&self, env: &Env<'_>) -> PageIndex {
        let storage = env.storage();
        let size = storage.size(&self.path).unwrap_or(0);
        let name = format!(
            "pages-{:016x}.bin",
            fnv1a64(format!("{}|{size}", self.typography.key))
        );
        let path = format!("{}/{name}", self.dir);
        if !storage.exists(&path) {
            let mut others = Vec::new();
            #[expect(
                clippy::let_underscore_must_use,
                reason = "a folder that cannot be listed keeps its page indexes"
            )]
            let _ = storage.list(&self.dir, &mut |entry| {
                if entry.name.starts_with("pages-") && !entry.is_dir {
                    others.push(format!("{}/{}", self.dir, entry.name));
                }
            });
            if others.len() >= INDEXES_KEPT {
                for other in others {
                    #[expect(
                        clippy::let_underscore_must_use,
                        reason = "an old page index, made again when needed: where it stays, it takes room, not data"
                    )]
                    let _ = storage.remove(&other);
                }
            }
        }
        PageIndex::open(storage, path)
    }

    /// Applies changed reading settings: new typography, the current
    /// position laid out again, and a page index for the new layout.
    pub(super) fn relayout(&mut self, env: &Env<'_>) {
        let Some(font) = self.font(env) else {
            return;
        };
        self.typography = typography(env, &font, &self.language, self.document.whole_pages());
        // The filters may leave out other blocks now.
        self.dropped.clear();
        self.words = core::cell::OnceCell::new();
        self.lines = core::cell::OnceCell::new();
        self.xpointer = None;
        let start = self.line_start(env, self.page.start);
        if let Ok(page) = self.layout(env, start) {
            self.page = page;
            self.prepare_images(env);
        }
        self.update_chapter(env);
        self.update_links(env);
        self.history.clear();
        self.index = Some(self.open_index(env));
        self.chapter_end.set(None);
        self.chapter_first.set(None);
        self.indexing = None;
        self.index_stopped = false;
    }

    /// Whether the page's type is another now than the pages were laid
    /// out in (a frame chosen, a layer's room); asked only where there
    /// are layers or frames.
    pub(super) fn retyped(&self, env: &Env<'_>) -> bool {
        !env.page_layers().is_idle()
            && self.font(env).is_some_and(|font| {
                let whole = self.document.whole_pages();
                typography(env, &font, &self.language, whole).key != self.typography.key
            })
    }

    /// Decodes the pictures of the page shown that are not yet: books
    /// decode them when their page shows, not while pages are laid out
    /// in the background.
    pub(super) fn prepare_images(&mut self, env: &Env<'_>) {
        let document = &self.document;
        for shown in &mut self.page.images {
            if let Some(image) = document.prepare_image(env.storage(), &shown.image) {
                shown.image = image;
            }
        }
    }

    /// Whether the page shown has pictures: shown in grays where the
    /// panel can ([`Screen::grays`](paperpaw_sdk::Screen::grays)).
    pub(super) fn pictured(&self) -> bool {
        !self.page.images.is_empty()
    }

    pub(super) fn font<'a>(&self, env: &Env<'a>) -> Option<FontHandle<'a>> {
        env.fonts().role(fonts::READING.as_str())
    }

    /// The start of the line `at` is in, as this book is laid out now
    /// (see `layout::line_start`); in columns as the first one breaks
    /// lines.
    pub(super) fn line_start(&self, env: &Env<'_>, at: Locator) -> Locator {
        let width = self.typography.line_width();
        self.laid_out(env, width, |cx| line_start(cx, at).unwrap_or(at))
            .unwrap_or(at)
    }

    /// The page that starts at `at`, as this book is laid out now: in
    /// columns where a plugin's frame sets them (see
    /// `paperpaw_sdk::layers`), one page of the page index for all.
    pub(super) fn layout(&self, env: &Env<'_>, at: Locator) -> Result<Page, DocumentError> {
        self.typography.lay_out(at, |width, height, at| {
            self.laid_out(env, width, |cx| layout_page(cx, at, height))
                .unwrap_or_else(|| Ok(Page::default()))
        })
    }

    /// `f` with the context this book is laid out in now, its lines
    /// `width` wide: its type, its faces and hyphenation, the text through
    /// the plugins' filters; `None` without the reading font.
    fn laid_out<R>(
        &self,
        env: &Env<'_>,
        width: f32,
        f: impl FnOnce(&LayoutContext<'_>) -> R,
    ) -> Option<R> {
        let t = &self.typography;
        let faces = Faces::new(env, t, self.embedded.as_ref())?;
        let patterns = t.patterns(env);
        let hyphenate = |word: &str, out: &mut dyn FnMut(usize)| {
            if let Some(patterns) = patterns {
                patterns.hyphenate(word, out);
            }
        };
        let filtered = FilteredDocument::new(self.document.as_ref(), env)
            .in_book(&self.path, &self.language)
            .remembering(&self.dropped);
        let cx = LayoutContext::new(&filtered, env.storage(), width, &t.style, &faces)
            .hyphenate(patterns.is_some().then_some(&hyphenate as Hyphenate<'_>));
        Some(f(&cx))
    }

    /// Where the reader kept the position before positions were shared
    /// (`positions`): read while there is no position of the book.
    fn progress_path(&self) -> String {
        format!("{}/progress.ini", self.dir)
    }

    /// The position kept, and whether to keep it anew: found by its
    /// XPointer (kept in another conversion of the book, its own styles
    /// switched since), or kept where the reader kept it before. One that
    /// is there but cannot be read, also at a second try, is held: not
    /// written over.
    pub(super) fn saved_position(&mut self, env: &Env<'_>) -> Option<(Locator, bool)> {
        let load = || paperpaw_sdk::positions::load_for_update(env.storage(), &self.path);
        // A card that failed to read it once may read it now.
        let loaded = load().or_else(|_| load());
        let (kept, before) = match loaded {
            Ok(Some(kept)) => (kept, false),
            Ok(None) => {
                let data =
                    paperpaw_sdk::read_file(env.storage(), &self.progress_path(), 4096).ok()?;
                let text = core::str::from_utf8(&data).ok()?;
                (paperpaw_sdk::positions::Position::parse(text)?, true)
            }
            Err(_) => {
                self.held = true;
                return None;
            }
        };
        self.from_progress = before;
        let found = match (kept.conversion, &kept.xpointer) {
            (Some(conversion), Some(xpointer)) if conversion != self.conversion => {
                self.document.locate_xpointer(env.storage(), xpointer)
            }
            _ => None,
        };
        Some(found.map_or((kept.at, before), |at| (at, true)))
    }

    /// Reads where the bookmarks and highlights are, those made in another
    /// conversion of the book (its own styles switched since, say) found
    /// by their XPointers first, and those added since taken as of this
    /// one; `true` where they changed. Where they cannot be read or
    /// written now, they are shown where they are kept.
    pub(super) fn reload_marks(&mut self, env: &Env<'_>) -> bool {
        let doc = self.document.as_ref();
        let moved =
            annotations::relocate(env.storage(), &self.path, self.conversion, doc).unwrap_or(false);
        if let Some(fingerprint) = &self.fingerprint {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "which book they are of: where it cannot be written now, it is at the next opening"
            )]
            let _ = annotations::stamp(env.storage(), &self.path, fingerprint);
        }
        let marks = annotations::marks(env.storage(), &self.path);
        let changed = moved || marks != self.marks;
        self.marks = marks;
        changed
    }

    /// Whether the page ends the book: it is read to the end.
    pub(super) fn at_end(&self) -> bool {
        self.document.progress(self.page.end) >= 0.999
    }

    pub(super) fn percent(&self) -> f32 {
        self.document.progress(self.page.start) * 100.0
    }

    /// Notes that the position is to be written, not now: once the pages
    /// rest a moment, or a while after the first page not written yet
    /// while they turn on. Returns when ([`Book::save_due`]).
    pub(super) fn position_changed(&mut self, cx: &mut Cx<'_, '_>) -> paperpaw_sdk::Instant {
        // Read on: the new place is kept, the one that could not be read
        // moved aside first.
        if self.held {
            self.set_aside(cx);
        }
        let now = cx.env().now();
        let since = *self.unsaved_since.get_or_insert(now);
        let at = now
            .add_millis(SAVE_QUIET_MS)
            .min(since.add_millis(SAVE_EVERY_MS));
        self.unsaved = true;
        self.save_at = Some(at);
        at
    }

    /// Moves the position that could not be read aside, to `<file>.bad`
    /// (or `.bad2` and on, where that is taken), so that the place read on
    /// is kept without losing it; says so once. Where it cannot be moved,
    /// it stays held and the reader is told the place is not kept.
    fn set_aside(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let file = paperpaw_sdk::positions::file(&self.path);
        if !env.storage().exists(&file) {
            self.held = false;
            return;
        }
        let moved = (1..10)
            .map(|n| match n {
                1 => format!("{file}.bad"),
                n => format!("{file}.bad{n}"),
            })
            .find(|aside| !env.storage().exists(aside))
            .is_some_and(|aside| env.storage().rename(&file, &aside).is_ok());
        let said = if moved {
            self.held = false;
            "reader.position-set-aside"
        } else if !self.save_failed {
            self.save_failed = true;
            "reader.not-saved"
        } else {
            return;
        };
        cx.command(Command::Notify(env.i18n().tr(said).to_string()));
    }

    /// Writes the position if it waits; `force` also before its time, and
    /// with its XPointer (the book is closed, the device goes to sleep).
    pub(super) fn save_due(&mut self, cx: &mut Cx<'_, '_>, force: bool) {
        let now = cx.env().now();
        let due = self.save_at.is_none_or(|at| at <= now);
        if (self.unsaved || force && !self.pointed) && (force || due) {
            self.unsaved = false;
            self.save_position(cx, force);
        } else if self.unsaved
            && let Some(at) = self.save_at
        {
            // Pages turned since it was asked for: later.
            cx.wake_at(at);
        }
    }

    /// Writes the position, with its XPointer if `pointed` (finding it
    /// takes a while on the X4, so not at every page); where the storage
    /// refuses, says so once a book (not at every page). A position held
    /// ([`Book::held`]) is not written over.
    pub(super) fn save_position(&mut self, cx: &mut Cx<'_, '_>, pointed: bool) {
        self.unsaved_since = None;
        self.save_at = None;
        if self.held {
            return;
        }
        let env = *cx.env();
        let start = self.page.start;
        if pointed {
            self.xpointer = self.document.xpointer(env.storage(), start);
        }
        self.pointed = pointed;
        let position = paperpaw_sdk::positions::Position::new(start, self.percent())
            .conversion(self.conversion)
            .xpointer(self.xpointer.clone().filter(|_| pointed))
            .updated(paperpaw_sdk::time::unix_seconds(&env).unwrap_or(0));
        let saved = paperpaw_sdk::positions::save_with_book(
            env.storage(),
            &self.path,
            &position,
            self.fingerprint.as_ref(),
        );
        if saved.is_err() && !self.save_failed {
            self.save_failed = true;
            cx.command(Command::Notify(
                env.i18n().tr("reader.not-saved").to_string(),
            ));
        }
        // Kept where it is now: the old file would only come back over it.
        if saved.is_ok() && self.from_progress {
            self.from_progress = env.storage().remove(&self.progress_path()).is_err();
        }
    }

    /// The start of the previous page: the page shown before (this
    /// session), else from the page index, else about a page back.
    pub(super) fn previous_start(&mut self, env: &Env<'_>) -> Option<Locator> {
        if let Some(start) = self.history.pop() {
            return Some(start);
        }
        let here = self.page.start;
        if let Some(index) = &self.index {
            match index.locate(env.storage(), here) {
                Some((0, true)) => return None,
                Some((page, true)) => return index.entry(env.storage(), page - 1),
                // Inside a page whose end the index knows: its start.
                Some((page, false)) if index.is_complete() || page + 1 < index.pages() => {
                    return index.entry(env.storage(), page);
                }
                // Past the pages counted so far.
                _ => {}
            }
        }
        self.page_back(env, here)
    }

    /// About a page before `here`, where the page index does not reach
    /// yet: as much text back as the page shown holds, from the start of
    /// its line. The page there may show a line again, but leaves nothing
    /// out.
    fn page_back(&self, env: &Env<'_>, here: Locator) -> Option<Locator> {
        let document = &self.document;
        let page = &self.page;
        let end = match page.end.section == page.start.section {
            true => page.end.offset,
            false => document.section_len(page.start.section),
        };
        let length = end.saturating_sub(page.start.offset).max(MIN_PAGE_BACK);
        // At the start of its section, the end of the one before.
        let first = document
            .block_at(env.storage(), Locator::new(here.section, 0))
            .ok()
            .flatten()
            .map_or(0, |block| block.start);
        let (section, end) = if here.offset > first {
            (here.section, here.offset)
        } else {
            let section = (0..here.section)
                .rev()
                .find(|&s| document.section_len(s) > 0)?;
            (section, document.section_len(section))
        };
        let mut from = self.line_start(env, Locator::new(section, end.saturating_sub(length)));
        // Where the text before is looser (talk, headings), a page from
        // there ends short of the page left: begin later by what it
        // leaves out.
        for _ in 0..PAGE_BACK_TRIES {
            let Ok(page) = self.layout(env, from) else {
                break;
            };
            let short = match page.end.section == section && !page.is_past_end() {
                true => end.saturating_sub(page.end.offset),
                false => 0,
            };
            let later = self.line_start(env, Locator::new(section, from.offset + short));
            if short == 0 || later <= from {
                break;
            }
            from = later;
        }
        Some(from)
    }

    pub(super) fn show(&mut self, cx: &mut Cx<'_, '_>, start: Locator, forward: bool) -> bool {
        let env = *cx.env();
        let Ok(page) = self.layout(&env, start) else {
            return false;
        };
        if page.is_past_end() || page.start == self.page.start {
            return false;
        }
        self.found = None;
        // The pages turned one after another, to turn back the same way;
        // after a jump the page before is the index's, not the page left.
        if forward && page.start == self.page.end {
            if self.history.len() == HISTORY {
                self.history.remove(0);
            }
            self.history.push(self.page.start);
        } else if page.end != self.page.start {
            self.history.clear();
        }
        self.page = page;
        self.words = core::cell::OnceCell::new();
        self.lines = core::cell::OnceCell::new();
        self.xpointer = None;
        self.prepare_images(&env);
        self.update_chapter(&env);
        // Found now, so the status lines read nothing while drawn.
        self.chapter_first(&env);
        self.update_links(&env);
        let save_at = self.position_changed(cx);
        cx.wake_at(save_at);
        if !self.web() {
            cx.command(events::PAGE_TURNED.emit(self.reading(&env)));
            // The last page: the book is read to the end.
            if !self.finished && self.at_end() {
                self.finished = true;
                cx.command(events::BOOK_FINISHED.emit(self.reading(&env)));
            }
        }
        cx.request_redraw();
        true
    }

    /// One step of background pagination. Returns whether more work
    /// remains. A page that cannot be laid out or a storage that takes no
    /// more end it for this layout; the pages counted so far are used.
    pub(super) fn index_step(&mut self, env: &Env<'_>) -> bool {
        if self.index_stopped || self.index.as_ref().is_none_or(PageIndex::is_complete) {
            return false;
        }
        let more = self.index_pages(env);
        self.index_stopped = more.is_none();
        more == Some(true)
    }

    /// Lays out the next pages and adds them to the index: whether more
    /// remain, `None` if it cannot go on.
    fn index_pages(&mut self, env: &Env<'_>) -> Option<bool> {
        let index = self.index.as_ref()?;
        let mut next = match self.indexing {
            Some(next) => next,
            None => match index.last(env.storage()) {
                Some(last) => self.layout(env, last).ok()?.end,
                None => Locator::START,
            },
        };
        let per_step = match env.device().memory {
            paperpaw_sdk::MemoryClass::Small => PAGES_PER_STEP_SMALL,
            _ => PAGES_PER_STEP,
        };
        let mut starts = Vec::with_capacity(per_step);
        let mut done = false;
        for _ in 0..per_step {
            // Unreadable: reading still works.
            let page = self.layout(env, next).ok()?;
            if page.is_past_end() {
                done = true;
                break;
            }
            starts.push(page.start);
            if page.end <= page.start {
                done = true;
                break;
            }
            next = page.end;
        }
        let index = self.index.as_mut()?;
        // A storage that is read-only or full.
        if !index.append(env.storage(), &starts) {
            return None;
        }
        if done {
            index.finish(env.storage());
        }
        self.indexing = Some(next);
        Some(!done)
    }

    /// "12 / 340" once the index covers the page exactly, else a percentage.
    pub(super) fn position_label(&self, env: &Env<'_>) -> String {
        if let Some(index) = &self.index
            && index.is_complete()
            && let Some((page, true)) = index.locate(env.storage(), self.page.start)
        {
            return env.i18n().format(
                "reader.page",
                &[
                    ("page", Arg::Int(i64::from(page) + 1)),
                    ("pages", Arg::Int(i64::from(index.pages()))),
                ],
            );
        }
        env.i18n().format(
            "reader.percent",
            &[("percent", Arg::Int(self.percent() as i64))],
        )
    }

    /// Whether this is no book but a document a plugin keeps for itself,
    /// e.g. a web page: it is not reported as read (see
    /// [`events::BOOK_OPENED`]) nor opened again at start-up.
    pub(super) fn web(&self) -> bool {
        paperpaw_sdk::files::is_hidden(&self.path)
    }

    /// Reads the links of the page.
    pub(super) fn update_links(&mut self, env: &Env<'_>) {
        let (start, end) = (self.page.start, self.page.end);
        let to = if end.section == start.section {
            end.offset
        } else {
            u32::MAX
        };
        self.links = self
            .document
            .links(env.storage(), start.section, start.offset, to);
    }

    /// What the buttons choose on the page, in reading order: the links
    /// that can be followed (to the web only if a browser is there) and
    /// the marks whose plugins hear taps on them.
    pub(super) fn targets(&self, env: &Env<'_>) -> Vec<Target> {
        let web = env.has_slot(&slots::BROWSER);
        let section = self.page.start.section;
        let mut order: Vec<(Locator, Target)> = (0..self.links.len())
            .filter(|&i| web || !paperpaw_sdk::document::is_external(&self.links[i].href))
            .map(|i| (Locator::new(section, self.links[i].start), Target::Link(i)))
            .collect();
        // Marks whose plugins hear taps on them, chosen as links are.
        order.extend(
            self.decorated
                .iter()
                .enumerate()
                .filter(|(_, m)| m.taps)
                .map(|(i, m)| (m.start, Target::Mark(i))),
        );
        order.sort_by_cached_key(|(at, _)| *at);
        let mut targets: Vec<Target> = order.into_iter().map(|(_, target)| target).collect();
        // Then the items layers drew, and the rooms of those that hear
        // taps.
        targets.extend((0..self.layer_items(env).len()).map(Target::Item));
        targets.extend(
            self.typography
                .rooms
                .iter()
                .enumerate()
                .filter(|(_, (id, _))| env.page_layers().on(env).any(|l| l.taps && l.id == *id))
                .map(|(i, _)| Target::Layer(i)),
        );
        targets
    }

    /// Where target `target` is on the page.
    pub(super) fn target_range(&self, target: Target) -> Option<(Locator, Locator)> {
        let section = self.page.start.section;
        match target {
            Target::Link(i) => self
                .links
                .get(i)
                .map(|l| (Locator::new(section, l.start), Locator::new(section, l.end))),
            Target::Mark(i) => self.decorated.get(i).map(|m| (m.start, m.end)),
            Target::Layer(_) | Target::Item(_) => None,
        }
    }

    /// The text at the target of link `href`, as a note shows it: the
    /// block there and the ones after it, up to a heading or a few hundred
    /// characters.
    pub(super) fn note_text(&self, env: &Env<'_>, href: &str) -> Option<String> {
        // As the page shows it.
        let filtered =
            FilteredDocument::new(self.document.as_ref(), env).in_book(&self.path, &self.language);
        paperpaw_sdk::reading::note_text(&filtered, env.storage(), href)
    }

    /// The link `word` is part of, if any.
    pub(super) fn link_of(&self, word: &WordBox) -> Option<&Link> {
        if word.start.section != self.page.start.section {
            return None;
        }
        self.links
            .iter()
            .find(|l| word.end.offset > l.start && word.start.offset < l.end)
    }

    /// Shows the page that starts at `start`, far from here (a link, the
    /// contents, a search hit, a page number): *Back* returns here.
    pub(super) fn jump(&mut self, cx: &mut Cx<'_, '_>, start: Locator) -> bool {
        let from = self.page.start;
        // A search hit or a position from elsewhere: its whole line.
        let start = self.line_start(cx.env(), start);
        if !self.show(cx, start, true) {
            return false;
        }
        if self.returns.len() == RETURNS {
            self.returns.remove(0);
        }
        self.returns.push(from);
        self.landed = Some(self.page.start);
        true
    }

    /// Back to where the last jump was made from.
    pub(super) fn go_back(&mut self, cx: &mut Cx<'_, '_>) {
        if let Some(from) = self.returns.pop() {
            self.landed = None;
            self.show(cx, from, false);
        }
    }

    /// Whether the page has a bookmark.
    pub(super) fn bookmarked(&self) -> bool {
        let (start, end) = (self.page.start, self.page.end);
        self.marks
            .iter()
            .any(|m| m.kind == Kind::Bookmark && m.start >= start && m.start < end)
    }

    /// Where the book is, for the skimming overlay: the page, the chapter
    /// and how far, in thousandths.
    pub(super) fn skimmed(&self, env: &Env<'_>) -> events::Skimmed {
        let chapter = self
            .chapter
            .as_ref()
            .map(|(_, _, title)| title.clone())
            .unwrap_or_default();
        events::Skimmed::new(
            self.path.clone(),
            self.position_label(env),
            chapter,
            (self.percent() * 10.0) as u32,
        )
    }

    /// Where `to` leads: `page:±n` pages on (or as many tenths of a
    /// percent while pages are counted), `permille:n` that far into the
    /// book, `chapter:±1` the chapter before or after.
    pub(super) fn skim_target(&self, env: &Env<'_>, to: &str) -> Option<Locator> {
        let (kind, value) = to.split_once(':')?;
        let value: i64 = value.trim_start_matches('+').parse().ok()?;
        let pages = self
            .index
            .as_ref()
            .filter(|i| i.is_complete() && i.pages() > 0);
        match kind {
            "page" => {
                if let Some(index) = pages
                    && let Some((page, _)) = index.locate(env.storage(), self.page.start)
                {
                    let target = (i64::from(page) + value).clamp(0, i64::from(index.pages()) - 1);
                    return index.entry(env.storage(), target as u32);
                }
                let at = (self.percent() as f64 * 10.0 + value as f64).clamp(0.0, 999.0);
                Some(self.locator_at(at / 1000.0))
            }
            "permille" => {
                let fraction = value.clamp(0, 999) as f64 / 1000.0;
                if let Some(index) = pages {
                    let page = (fraction * f64::from(index.pages())) as u32;
                    return index.entry(env.storage(), page.min(index.pages() - 1));
                }
                Some(self.locator_at(fraction))
            }
            "chapter" => {
                let storage = env.storage();
                let entries = self.document.toc_len(storage).min(2000);
                // With several sections, exact starts are found only where
                // the start of an entry's section does not settle it: an
                // exact start (an anchor) may need its chapter converted.
                let (document, page) = (&*self.document, (self.page.start, self.page.end));
                match (document.sections() > 1, value > 0) {
                    (true, true) => next_chapter(document, storage, entries, page.1),
                    (true, false) => previous_chapter(document, storage, entries, page.0),
                    (false, on) => chapter_by_starts(document, storage, entries, page, on),
                }
            }
            _ => None,
        }
    }

    /// The position `fraction` of the way through the book, by the
    /// sections' lengths.
    pub(super) fn locator_at(&self, fraction: f64) -> Locator {
        let sections = self.document.sections();
        let total: u64 = (0..sections)
            .map(|s| u64::from(self.document.section_len(s)))
            .sum();
        let mut left = (fraction.clamp(0.0, 1.0) * total as f64) as u64;
        for section in 0..sections {
            let len = u64::from(self.document.section_len(section));
            if left < len {
                return Locator::new(section, left as u32);
            }
            left -= len;
        }
        Locator::new(sections.saturating_sub(1), 0)
    }

    /// The start of the page that contains `at`, as the page index has it
    /// (so pages look the same as when paging there); `at` itself where
    /// the index does not reach yet.
    pub(super) fn page_start(&self, env: &Env<'_>, at: Locator) -> Locator {
        let Some(index) = &self.index else {
            return at;
        };
        match index.locate(env.storage(), at) {
            Some((page, _)) if index.is_complete() || page + 1 < index.pages() => {
                index.entry(env.storage(), page).unwrap_or(at)
            }
            _ => at,
        }
    }

    /// Finds the chapter of the page, before the page is shown and said
    /// ([`events::PAGE_TURNED`]); the table of contents is read only when
    /// the page leaves the chapter.
    pub(super) fn update_chapter(&mut self, env: &Env<'_>) {
        let at = self.page.start;
        if let Some((from, to, _)) = &self.chapter
            && *from <= at
            && to.is_none_or(|to| at < to)
        {
            return;
        }
        self.chapter = None;
        let (document, storage) = (&self.document, env.storage());
        let mut chapter = (Locator::START, None, String::new());
        for index in 0..document.toc_len(storage) {
            let Some(entry) = document.toc_entry(storage, index) else {
                continue;
            };
            // Entries in the page's section need their exact position.
            let start = if entry.at.section == at.section {
                document.toc_target(storage, index).unwrap_or(entry.at)
            } else {
                entry.at
            };
            if start <= at {
                chapter = (start, None, entry.title);
            } else {
                chapter.1 = Some(start);
                break;
            }
        }
        self.chapter = Some(chapter);
    }
}

/// The chapter after the page or the one it is in (or before), `on` or
/// back, by the exact starts of the first `entries` entries; the page
/// from `page.0` to `page.1`.
fn chapter_by_starts(
    document: &dyn Document,
    storage: &dyn paperpaw_sdk::Storage,
    entries: u32,
    (here, end): (Locator, Locator),
    on: bool,
) -> Option<Locator> {
    // Where the entries begin, exactly (their anchors): the chapters the
    // status lines name, also those in parts.
    let mut starts: Vec<Locator> = (0..entries)
        .filter_map(|i| document.toc_target(storage, i))
        .collect();
    starts.sort_unstable();
    starts.dedup();
    if on {
        // On: the first chapter that begins on a later page.
        return starts.into_iter().find(|&s| s >= end);
    }
    // Back: to the start of this chapter, or the one before.
    let current = starts.iter().rposition(|&s| s <= here)?;
    match starts[current] < here {
        true => Some(starts[current]),
        false => current.checked_sub(1).map(|i| starts[i]),
    }
}

/// As [`chapter_by_starts`] on: the first exact start at or after the
/// page's `end`. An entry's locator is in the section of its exact start
/// and not after it, so entries of earlier sections, and those not
/// before the start found, need not be looked at closer.
fn next_chapter(
    document: &dyn Document,
    storage: &dyn paperpaw_sdk::Storage,
    entries: u32,
    end: Locator,
) -> Option<Locator> {
    let mut found: Option<Locator> = None;
    for i in 0..entries {
        if let Some(entry) = document.toc_entry(storage, i)
            && (entry.at.section < end.section || found.is_some_and(|f| entry.at >= f))
        {
            continue;
        }
        if let Some(start) = document.toc_target(storage, i)
            && start >= end
            && found.is_none_or(|f| start < f)
        {
            found = Some(start);
        }
    }
    found
}

/// As [`chapter_by_starts`] back: the last exact start before the page
/// at `here`. Only the entries of the page's section are looked at
/// closer, then those of the last section before it, where the chapter
/// before begins if not in this one.
fn previous_chapter(
    document: &dyn Document,
    storage: &dyn paperpaw_sdk::Storage,
    entries: u32,
    here: Locator,
) -> Option<Locator> {
    // Entries that may begin in the page's section before it, or whose
    // locator is unknown.
    let mut near = Vec::new();
    // The last section before the page's that has entries, and those.
    let mut earlier = None;
    let mut last = Vec::new();
    for i in 0..entries {
        let Some(entry) = document.toc_entry(storage, i) else {
            near.push(i);
            continue;
        };
        let at = entry.at;
        if at >= here {
            continue;
        }
        if at.section == here.section {
            near.push(i);
        } else if earlier.is_none_or(|section| at.section >= section) {
            if earlier != Some(at.section) {
                last.clear();
                earlier = Some(at.section);
            }
            last.push(i);
        }
    }
    let exact = |list: &[u32]| {
        list.iter()
            .filter_map(|&i| document.toc_target(storage, i))
            .filter(|&start| start < here)
            .max()
    };
    let found = exact(&near);
    if earlier.is_none() || found.is_some_and(|f| f.section == here.section) {
        return found;
    }
    match exact(&last) {
        Some(start) => found.max(Some(start)),
        // None of these has a start: those of sections before it count.
        None => chapter_by_starts(document, storage, entries, (here, here), false),
    }
}

/// Whether the text filters do something else now than when the pages
/// were laid out.
pub(super) fn env_filters_changed(env: &Env<'_>, book: &Book) -> bool {
    book.typography.filters != env.text_filters().signature(env)
}

#[cfg(test)]
mod chapter_tests;
