//! No screen of the standard app reads the storage while it draws.
//! The X4 draws a frame in bands, a screen's `draw`
//! once for each, so what `draw` reads is read again for every band: the
//! home screen once took 1.9 s to draw instead of 0.33 s for it. Each
//! screen is opened here as on an X4 on which a book was read, and what
//! drawing it does on the card is noted band by band, below the kernel as
//! the X4's `trace` sees it: nothing after the first band of the frame it
//! comes up with, and nothing at all when it is drawn again. A screen
//! loads what it shows when it opens or that changes, and keeps it.
//!
//! The one thing read while drawing is the rows of pictures too large to
//! keep (`ui::draw_image` reads those a band at a time, the X4 having no
//! memory for a frame or such a picture), each row once a frame. Which
//! pictures are small enough to keep (`ui::Thumbnail`) this cannot tell,
//! as it does not see how large they are drawn.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::fmt::Write as _;
use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DirEntry, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
    Storage, StorageError,
};
use paperpaw_sdk::annotations::{self, Annotation, Kind, Mark};
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::widgets::categories;
use paperpaw_sdk::{
    Canvas, Cx, Env, Handled, Id, Manifest, MemoryStorage, Params, Plugin, RegisterError,
    Registrar, SDK_VERSION, Screen, ScreenSpec, library,
};

/// An X4 as the emulator's profile and the firmware have it: buttons, no
/// touch, frames drawn in bands of 192 rows (five a frame).
const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);
const FRAME_ROWS: u16 = 192;

/// Pages of widgets of a kind looked at, at most.
const GALLERY_PAGES: usize = 8;

const EPUB: &str = "books/alice-in-wonderland.epub";
const TXT: &str = "books/die-verwandlung.txt";

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// A call on the card while a frame was drawn: in which band, what was
/// done, on what, the bytes read.
#[derive(Debug)]
struct Call {
    band: usize,
    op: &'static str,
    path: String,
    bytes: usize,
}

/// What is noted while a frame is drawn.
#[derive(Debug, Default)]
struct Log {
    drawing: bool,
    band: usize,
    calls: Vec<Call>,
}

/// The card, with what is done on it noted while a frame is drawn.
struct Noted {
    inner: MemoryStorage,
    log: Rc<RefCell<Log>>,
}

impl Noted {
    fn note(&self, op: &'static str, path: &str, bytes: usize) {
        let mut log = self.log.borrow_mut();
        if log.drawing {
            let band = log.band;
            log.calls.push(Call {
                band,
                op,
                path: path.to_string(),
                bytes,
            });
        }
    }
}

impl Storage for Noted {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let read = self.inner.read_at(path, offset, buf);
        self.note("read", path, *read.as_ref().unwrap_or(&0));
        read
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.note("size", path, 0);
        self.inner.size(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.note("exists", path, 0);
        self.inner.exists(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.note("write", path, 0);
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.note("append", path, 0);
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.note("list", dir, 0);
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.note("remove", path, 0);
        self.inner.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.note("rename", from, 0);
        self.inner.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.note("create_dir", path, 0);
        self.inner.create_dir(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.note("remove_dir_all", path, 0);
        self.inner.remove_dir_all(path)
    }
}

/// A frame drawn: how many bands, and the calls drawing it made.
#[derive(Debug, Default)]
struct Frame {
    bands: usize,
    calls: Vec<Call>,
}

struct App {
    kernel: Kernel,
    now: u64,
    log: Rc<RefCell<Log>>,
    /// The last frame drawn: the one the screen shown came up with.
    shown: Frame,
}

impl App {
    /// The app on a card holding `files`.
    fn on(files: &[(String, Vec<u8>)]) -> Self {
        Self::with(files, |builder| builder)
    }

    /// The same, with what `add` adds to the app.
    fn with(
        files: &[(String, Vec<u8>)],
        add: impl FnOnce(paperpaw_core::KernelBuilder) -> paperpaw_core::KernelBuilder,
    ) -> Self {
        let inner = files.iter().fold(MemoryStorage::new(), |s, (path, data)| {
            s.with(path, data.clone())
        });
        let log = Rc::new(RefCell::new(Log::default()));
        let storage = Noted {
            inner,
            log: log.clone(),
        };
        let builder = Kernel::builder(DEVICE)
            .storage(storage)
            .frame_rows(FRAME_ROWS)
            .row_align(8)
            .user_config("[help]\nshow = false\n[core]\nlanguage = en\n");
        let mut kernel = add(paperpaw_app::install(builder, fonts()))
            .build()
            .unwrap();
        let effects = kernel.start(Instant(0));
        let mut app = Self {
            kernel,
            now: 0,
            log,
            shown: Frame::default(),
        };
        app.settle(effects);
        app
    }

    /// Draws what changed, band by band, noting what that does on the
    /// card; `None` where nothing changed.
    fn render(&mut self) -> Option<Frame> {
        {
            let mut log = self.log.borrow_mut();
            log.drawing = true;
            log.band = 0;
            log.calls.clear();
        }
        let log = self.log.clone();
        self.kernel.render(&mut |_, _| log.borrow_mut().band += 1);
        let mut log = self.log.borrow_mut();
        log.drawing = false;
        let frame = Frame {
            bands: log.band,
            calls: std::mem::take(&mut log.calls),
        };
        (frame.bands > 0).then_some(frame)
    }

    /// Lets timers run until nothing is due within a second, drawing what
    /// changes as a device would.
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            if let Some(frame) = self.render() {
                self.shown = frame;
            }
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn set(&mut self, id: &str, value: &str) {
        self.now += 100;
        let effects = self
            .kernel
            .set_setting(Instant(self.now), id, value)
            .unwrap();
        self.settle(effects);
    }

    fn open_book(&mut self, path: &str) {
        self.now += 100;
        let params = Params::new().with(params::PATH, path);
        let effects = self
            .kernel
            .open_slot(Instant(self.now), slots::READER, params);
        self.settle(effects);
    }

    fn open(&mut self, screen: &Id, params: Params) {
        self.now += 100;
        let effects = self
            .kernel
            .open_screen(Instant(self.now), screen.clone(), params);
        self.settle(effects);
    }

    /// The whole screen drawn anew, nothing on it changed.
    fn again(&mut self) -> Frame {
        self.now += 100;
        self.kernel.draw_again(Instant(self.now));
        self.render().unwrap_or_default()
    }

    /// What drawing the screen shown did on the card that it should not
    /// have: anything after the first band of the frame it came up with,
    /// anything at all drawn again.
    fn reads_in_draw(&mut self) -> Vec<String> {
        let shown = std::mem::take(&mut self.shown);
        let again = self.again();
        let mut found = self.faults(&shown, 1, "came up");
        for fault in self.faults(&again, 0, "drawn again") {
            if !found.contains(&fault) {
                found.push(fault);
            }
        }
        found
    }

    /// The calls of `frame` from band `from` on that are no picture's
    /// rows, and pictures whose rows were read more than once.
    fn faults(&self, frame: &Frame, from: usize, when: &str) -> Vec<String> {
        let storage = self.kernel.storage();
        // Decoded pictures, gray or in colour (`paperpaw_image`).
        let picture = |path: &str| {
            let mut magic = [0u8; 4];
            matches!(storage.read_at(path, 0, &mut magic), Ok(4))
                && (&magic == b"PPGI" || &magic == b"PPCI")
        };
        let mut found = Vec::new();
        let mut pictures: Vec<(&str, usize)> = Vec::new();
        for call in frame.calls.iter().filter(|c| c.band >= from) {
            if matches!(call.op, "read" | "size") && picture(&call.path) {
                match pictures.iter_mut().find(|(p, _)| *p == call.path) {
                    Some((_, bytes)) => *bytes += call.bytes,
                    None => pictures.push((&call.path, call.bytes)),
                }
                continue;
            }
            let fault = format!("{when}, band {}: {} {}", call.band + 1, call.op, call.path);
            if !found.contains(&fault) {
                found.push(fault);
            }
        }
        // Each row once a frame; in each band it lies in its header, and
        // rows read with those of the band before (8 KiB at a time).
        for (path, bytes) in pictures {
            let size = storage.size(path).unwrap_or(0) as usize;
            if bytes > size + frame.bands * (8192 + 64) {
                found.push(format!(
                    "{when}: {bytes} bytes of the picture {path} of {size}"
                ));
            }
        }
        found
    }

    fn shown(&self) -> String {
        self.kernel
            .active_screen()
            .map_or_else(String::new, ToString::to_string)
    }

    /// Every file on the card, to start other apps with.
    fn card(&self) -> Vec<(String, Vec<u8>)> {
        let mut files = Vec::new();
        copy(self.kernel.storage(), "", &mut files);
        files
    }
}

fn copy(storage: &dyn Storage, dir: &str, files: &mut Vec<(String, Vec<u8>)>) {
    let mut entries = Vec::new();
    storage
        .list(dir, &mut |entry| {
            entries.push((entry.name.to_string(), entry.is_dir));
        })
        .unwrap();
    for (name, is_dir) in entries {
        let path = if dir.is_empty() {
            name
        } else {
            format!("{dir}/{name}")
        };
        if is_dir {
            copy(storage, &path, files);
        } else {
            let data = paperpaw_sdk::read_file(storage, &path, usize::MAX).unwrap();
            files.push((path, data));
        }
    }
}

/// A card past the first-run setup on which two books were read: the home
/// screen has a book to continue, with its cover, the library recent
/// books, the statistics a day; with favourites, a highlight with a note,
/// words looked up and things to do.
fn card() -> Vec<(String, Vec<u8>)> {
    let files = [
        (
            EPUB,
            &include_bytes!("../../../samples/library/books/alice-in-wonderland.epub")[..],
        ),
        (
            TXT,
            include_bytes!("../../../samples/library/books/die-verwandlung.txt"),
        ),
        (".paperpaw/plugins/setup/done", b""),
    ]
    .map(|(path, data)| (path.to_string(), data.to_vec()));
    let mut app = App::on(&files);
    app.open_book(TXT);
    app.run(actions::NEXT_PAGE);
    app.open_book(EPUB);
    app.run(actions::NEXT_PAGE);
    app.run(actions::NEXT_PAGE);
    app.run(actions::HOME);
    let storage = app.kernel.storage();
    for book in [EPUB, TXT] {
        library::set_favourite(storage, book, true);
    }
    let text = "Alice was beginning to get very tired of sitting by her sister on         the bank, and of having nothing to do: once or twice she had peeped into         the book her sister was reading, but it had no pictures or conversations         in it, and what is the use of a book, thought Alice, without pictures or         conversations?";
    let highlight = Annotation::new(
        Mark::new(
            Kind::Highlight,
            Locator::new(1, 0),
            Locator::new(1, text.len() as u32),
            true,
        ),
        1,
        1_791_900_000,
        1_791_900_000,
        annotations::snippet(text),
        "The very beginning".to_string(),
    );
    annotations::add(storage, EPUB, highlight).unwrap();
    let words = [("peeped", "en"), ("Ungeziefer", "de")]
        .map(|(word, language)| {
            format!(
                "{word}	{language}	1	120	{EPUB}	Alice	{text}
"
            )
        })
        .concat();
    let plugin = |id: &str, name: &str| format!("{}/{name}", Env::plugin_dir(id));
    storage
        .write(&plugin("vocabulary", "words.txt"), words.as_bytes())
        .unwrap();
    storage
        .write(
            &plugin("widgets", "todo.txt"),
            b"Buy bread
Return the library books
x 2026-10-13 Water the plants
",
        )
        .unwrap();
    app.card()
}

#[derive(Clone)]
enum Step {
    Run(Id),
    Book(&'static str),
    Open(Id, Params),
    /// A setting set, as the settings screen would.
    Set(&'static str, &'static str),
}

/// Screens reached as people reach them, with what they open with.
fn walks() -> Vec<(String, Vec<Step>)> {
    use Step::{Book, Open, Run, Set};
    let reader = || Book(EPUB);
    let mut walks = vec![
        ("home", vec![Run(actions::HOME)]),
        ("library", vec![Run(actions::LIBRARY)]),
        ("main menu", vec![Run(actions::MENU)]),
        ("tools", vec![Run(actions::TOOLS)]),
        ("settings", vec![Run(actions::SETTINGS)]),
        (
            "a settings group",
            vec![
                Run(actions::SETTINGS),
                Run(actions::DOWN),
                Run(actions::CONFIRM),
            ],
        ),
        ("reader", vec![reader()]),
        ("reader menu", vec![reader(), Run(actions::CONFIRM)]),
        (
            "reader layout",
            vec![reader(), Run(Id::new_static("reader.quick-settings"))],
        ),
        ("a text book", vec![Book(TXT)]),
        (
            "a book with the words looked up marked",
            vec![Set("vocabulary.mark", "on"), reader()],
        ),
        (
            "the next chapter, its page in the status line",
            vec![
                Set(
                    "reader.status.bottom-left",
                    "reader.status.chapter-page, reader.status.chapter-left",
                ),
                reader(),
                Run(Id::new_static("reader.next-chapter")),
            ],
        ),
    ];
    // The widgets of each kind as the home screen offers them, each drawn
    // as it would show, page by page.
    let add = Id::new_static("home.add-widget");
    for kind in [
        categories::TIME,
        categories::BOOKS,
        categories::READING,
        categories::WORDS,
        categories::PERSONAL,
        categories::WORLD,
        categories::DEVICE,
    ] {
        let mut steps = vec![Open(add.clone(), Params::new().with("kind", kind))];
        for _ in 0..GALLERY_PAGES {
            walks.push((kind, steps.clone()));
            steps.push(Run(actions::NEXT_PAGE));
        }
    }
    walks
        .into_iter()
        .map(|(name, steps)| (name.to_string(), steps))
        .collect()
}

#[test]
fn no_screen_reads_while_it_draws() {
    let card = card();
    let mut found = String::new();
    for (name, steps) in walks() {
        let mut app = App::on(&card);
        for step in steps {
            match step {
                Step::Run(action) => app.run(action),
                Step::Book(path) => app.open_book(path),
                Step::Set(id, value) => app.set(id, value),
                Step::Open(screen, params) => {
                    app.open(&screen, params);
                    assert_eq!(app.shown(), screen.as_str(), "{name}");
                }
            }
        }
        for fault in app.reads_in_draw() {
            writeln!(found, "{name} ({}): {fault}", app.shown()).unwrap();
        }
    }
    // Every screen, opened as a plugin would open it with nothing.
    let screens: Vec<Id> = App::on(&card).kernel.known_screens().cloned().collect();
    for screen in screens {
        let mut app = App::on(&card);
        app.open(&screen, Params::new());
        for fault in app.reads_in_draw() {
            writeln!(found, "{screen} (shows {}): {fault}", app.shown()).unwrap();
        }
    }
    assert!(
        found.is_empty(),
        "screens that read the storage while drawing:\n{found}"
    );
}

/// A screen that reads a file while it draws, as screens once did.
struct Reads;

impl Screen for Reads {
    fn draw(&mut self, _: &mut Canvas<'_>, env: &Env<'_>) {
        let _ = paperpaw_sdk::read_file(env.storage(), "notes.txt", 1024);
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

struct Probe;

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("probe"), "Probe", SDK_VERSION)
            .description("A screen that reads while it draws.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            Id::new_static("probe.reads"),
            "probe.reads",
            Box::new(|_, _| Box::new(Reads)),
        ))
    }
}

/// What the check above looks for, it finds: a screen reading a small
/// file in every band (kept by the kernel while a frame is drawn, but read
/// again for every frame).
#[test]
fn a_screen_reading_while_it_draws_is_found() {
    let card = [("notes.txt".to_string(), b"Remember the milk".to_vec())];
    let mut app = App::with(&card, |builder| builder.plugin(Probe));
    app.open(&Id::new_static("probe.reads"), Params::new());
    assert_eq!(app.shown(), "probe.reads");
    let found = app.reads_in_draw();
    assert!(
        found
            .iter()
            .any(|f| f.starts_with("drawn again, band 1: read notes.txt")),
        "{found:?}"
    );
}
