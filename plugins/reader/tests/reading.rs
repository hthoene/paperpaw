//! Reading a book with the real kernel, the text and EPUB formats and the
//! default fonts: pages turn both ways, the position is
//! kept (also over a restart) and the book continues there, a larger font keeps the place,
//! bookmarks are kept, and plugins hear of it all, the chapter too.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_plugin_reader::{Reader, actions};
use paperpaw_sdk::annotations::Kind;
use paperpaw_sdk::builtin::{
    actions as core_actions, events, keymap_presets, params, settings, slots,
};
use paperpaw_sdk::{
    ButtonState, DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, InputEvent, Instant, Manifest,
    MemoryClass, MemoryStorage, Params, PixelFormat, Plugin, RegisterError, Registrar, Storage,
    StorageError, Version,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test",
    name: "Test device",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Gray4,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Large,
};

const BOOK: &str = "books/alice.txt";
/// The same book with a table of contents.
const EPUB: &str = "books/alice.epub";

/// A storage the test looks into while kernels have it. It refuses to
/// add to page indexes once the appends in `.1` (if set) are used up, to
/// read files whose paths end as `.2` says (as often as `.4` says, if
/// set), and to write files whose paths hold what `.3` says (if set).
#[derive(Clone, Default)]
struct Shared(
    Rc<MemoryStorage>,
    Rc<std::cell::Cell<Option<u32>>>,
    Rc<std::cell::Cell<Option<&'static str>>>,
    Rc<RefCell<Option<String>>>,
    Rc<std::cell::Cell<Option<u32>>>,
);

impl Shared {
    fn refuses(&self, path: &str) -> bool {
        self.3
            .borrow()
            .as_ref()
            .is_some_and(|part| path.contains(part.as_str()))
    }
}

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if self.2.get().is_some_and(|end| path.ends_with(end)) {
            match self.4.get() {
                Some(0) => {}
                Some(left) => {
                    self.4.set(Some(left - 1));
                    return Err(StorageError::Io);
                }
                None => return Err(StorageError::Io),
            }
        }
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if self.refuses(path) {
            return Err(StorageError::Full);
        }
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if self.refuses(path) {
            return Err(StorageError::Full);
        }
        if path.contains("/pages-") {
            match self.1.get() {
                Some(0) => return Err(StorageError::Io),
                Some(left) => self.1.set(Some(left - 1)),
                None => {}
            }
        }
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        if self.refuses(to) {
            return Err(StorageError::Full);
        }
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

/// What a plugin hears of the reading: the event and what it carries.
type Heard = Rc<RefCell<Vec<(&'static str, events::Reading)>>>;

/// A plugin that listens to the reading events.
struct Listener(Heard);

impl Plugin for Listener {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("listener"),
            "Listener",
            Version::new(0, 1, 0),
        )
        .description("Hears the reading events.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for (event, name) in [
            (events::BOOK_OPENED, "opened"),
            (events::PAGE_TURNED, "turned"),
            (events::BOOK_CLOSED, "closed"),
        ] {
            let heard = self.0.clone();
            r.on(event, move |_, reading, _| {
                heard.borrow_mut().push((name, reading.clone()));
            })?;
        }
        Ok(())
    }
}

struct Reading {
    kernel: Kernel,
    storage: Shared,
    heard: Heard,
    now: u64,
    /// The book read.
    book: &'static str,
}

impl Reading {
    /// A kernel over `storage`, with the book on it.
    fn new(storage: Shared) -> Self {
        let text = include_str!("../../../samples/library/books/alice-in-wonderland.txt");
        Self::with_book(storage, BOOK, text.as_bytes())
    }

    /// A kernel over `storage`, with book `data` at `path`.
    fn with_book(storage: Shared, path: &'static str, data: &[u8]) -> Self {
        Self::with_preset(storage, path, data, keymap_presets::STANDARD_NAME)
    }

    /// The same with the buttons laid out as `preset` says.
    fn with_preset(storage: Shared, path: &'static str, data: &[u8], preset: &'static str) -> Self {
        if !storage.exists(path) {
            storage.create_dir("books").unwrap();
            storage.write(path, data).unwrap();
        }
        let heard = Heard::default();
        let kernel = Kernel::builder(DEVICE)
            .setting_default(&settings::KEYMAP_PRESET, preset.into())
            .storage(storage.clone())
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_format_txt::TxtFormat)
            .plugin(paperpaw_plugin_format_epub::EpubFormat)
            .plugin(Reader)
            .plugin(Listener(heard.clone()))
            .build()
            .unwrap();
        let mut reading = Self {
            kernel,
            storage,
            heard,
            now: 0,
            book: path,
        };
        let effects = reading.kernel.start(Instant(0));
        reading.settle(effects);
        reading
    }

    /// Lets the kernel's timers run (the page index, a first opening)
    /// until nothing is due within a second.
    fn settle(&mut self, mut effects: paperpaw_core::Effects) {
        for _ in 0..10_000 {
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

    fn open(&mut self) {
        self.now += 10;
        let params = Params::new().with(params::PATH, self.book);
        let effects = self
            .kernel
            .open_slot(Instant(self.now), slots::READER, params);
        self.settle(effects);
        assert_eq!(
            self.kernel.active_screen().map(Id::as_str),
            Some("reader.screen")
        );
    }

    fn run(&mut self, action: Id) {
        // Past the time positions are written at most this often.
        self.now += 6000;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    /// The position the reader kept: section and offset; the start
    /// while it kept none (a book opened and not read on).
    fn kept(&mut self) -> (u32, u32) {
        self.now += 10;
        let effects = self.kernel.suspend(Instant(self.now));
        self.settle(effects);
        self.written()
    }

    /// The position written to the storage so far, as [`Reading::kept`].
    fn written(&self) -> (u32, u32) {
        let file = paperpaw_sdk::positions::file(self.book);
        let Some(data) = self.storage.0.get(&file) else {
            return (0, 0);
        };
        let text = String::from_utf8(data).unwrap();
        let value = |key: &str| -> u32 {
            let line = text.lines().find(|l| l.starts_with(key)).unwrap();
            line.rsplit(' ').next().unwrap().parse().unwrap()
        };
        (value("section"), value("offset"))
    }

    fn heard(&self) -> Vec<&'static str> {
        self.heard.borrow().iter().map(|(name, _)| *name).collect()
    }
}

#[test]
fn turns_pages_both_ways() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    let second = reading.kept();
    reading.run(core_actions::PREV_PAGE);
    // The first page: where the text begins (a Project Gutenberg header
    // before it is left out).
    let first = reading.kept();
    assert!(first < second, "{first:?} before {second:?}");
    reading.run(core_actions::NEXT_PAGE);
    assert_eq!(reading.kept(), second);
    reading.run(core_actions::NEXT_PAGE);
    let third = reading.kept();
    assert!(third > second, "{third:?} after {second:?}");
    reading.run(core_actions::PREV_PAGE);
    assert_eq!(reading.kept(), second);
    reading.run(core_actions::PREV_PAGE);
    assert_eq!(reading.kept(), first);
    // Not before the first page.
    reading.run(core_actions::PREV_PAGE);
    assert_eq!(reading.kept(), first);
}

#[test]
fn continues_where_it_was_left() {
    let storage = Shared::default();
    let mut reading = Reading::new(storage.clone());
    reading.open();
    for _ in 0..3 {
        reading.run(core_actions::NEXT_PAGE);
    }
    let left = reading.kept();
    assert!(left.1 > 0);
    // Another start of the device, the same card.
    let mut again = Reading::new(storage);
    again.open();
    assert_eq!(again.kept(), left);
}

#[test]
fn a_restart_keeps_the_place() {
    let storage = Shared::default();
    let mut reading = Reading::new(storage.clone());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    let written = reading.written();
    // A page on at once: not written yet, the card is spared.
    reading.now += 10;
    let effects = reading
        .kernel
        .run_action(Instant(reading.now), core_actions::NEXT_PAGE);
    reading.settle(effects);
    assert_eq!(reading.written(), written);
    reading.now += 10;
    let effects = reading.kernel.stop(Instant(reading.now));
    reading.settle(effects);
    let left = reading.written();
    assert!(left > written, "{left:?} after {written:?}");
    // The device starts again, the same card.
    let mut again = Reading::new(storage);
    again.open();
    assert_eq!(again.kept(), left);
}

#[test]
fn a_larger_font_keeps_the_place() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    for _ in 0..4 {
        reading.run(core_actions::NEXT_PAGE);
    }
    let before = reading.kept();
    reading.run(core_actions::NEXT_PAGE);
    let next = reading.kept();
    reading.run(core_actions::PREV_PAGE);
    reading.run(actions::FONT_LARGER);
    let after = reading.kept();
    // The page starts where a line starts, at the place read or just
    // before it.
    assert_eq!(after.0, before.0);
    assert!(
        after.1 <= before.1 && after.1 + 400 > before.1,
        "{after:?} for {before:?}"
    );
    // Laid out anew: larger letters, less on a page.
    reading.run(core_actions::NEXT_PAGE);
    let larger = reading.kept();
    assert!(
        larger.1 - after.1 < next.1 - before.1,
        "{after:?} to {larger:?}, before {before:?} to {next:?}"
    );
}

#[test]
fn the_size_slider_lays_the_book_out_anew() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    let start = reading.kept();
    reading.run(core_actions::NEXT_PAGE);
    let page = reading.kept().1 - start.1;
    reading.run(core_actions::PREV_PAGE);
    set(&mut reading, settings::FONT_SIZE.static_str(), "8");
    let small = reading.kept();
    reading.run(core_actions::NEXT_PAGE);
    assert!(
        reading.kept().1 - small.1 > page,
        "more on a page in small letters"
    );
}

#[test]
fn bookmarks_are_kept() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    reading.run(actions::TOGGLE_BOOKMARK);
    let marks = |reading: &Reading| {
        let marks = paperpaw_sdk::annotations::marks(&reading.storage, BOOK);
        let bookmarks = marks.iter().filter(|m| m.kind == Kind::Bookmark);
        bookmarks.count()
    };
    assert_eq!(marks(&reading), 1);
    // Again on the same page: taken away.
    reading.run(actions::TOGGLE_BOOKMARK);
    assert_eq!(marks(&reading), 0);
}

#[test]
fn plugins_hear_the_reading() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    reading.run(core_actions::NEXT_PAGE);
    reading.now += 10;
    let effects = reading
        .kernel
        .run_action(Instant(reading.now), core_actions::BACK);
    reading.settle(effects);
    let heard = reading.heard();
    assert_eq!(heard.first(), Some(&"opened"));
    assert_eq!(heard.iter().filter(|e| **e == "turned").count(), 2);
    let events = reading.heard.borrow();
    assert!(events.iter().all(|(_, r)| r.path == BOOK));
    let progress: Vec<f32> = events.iter().map(|(_, r)| r.progress).collect();
    assert!(progress.windows(2).all(|p| p[0] <= p[1]), "{progress:?}");
}

#[test]
fn plugins_hear_the_chapter() {
    let epub = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");
    let mut reading = Reading::with_book(Shared::default(), EPUB, epub);
    reading.open();
    // Until every page is counted.
    let counted = |reading: &Reading| {
        let heard = reading.heard.borrow();
        heard.last().is_some_and(|(_, r)| r.pages.is_some())
    };
    let mut turned = 0;
    while !counted(&reading) {
        reading.run(core_actions::NEXT_PAGE);
        turned += 1;
        assert!(turned < 400, "the pages are never counted");
    }
    // Then through the chapters: each page in one, as many pages left in
    // it as come before the next.
    let from = reading.heard.borrow().len() - 1;
    for _ in 0..60 {
        reading.run(core_actions::NEXT_PAGE);
    }
    let heard = reading.heard.borrow();
    let pages: Vec<&events::Reading> = heard[from..].iter().map(|(_, r)| r).collect();
    assert!(pages.iter().all(|r| r.path == EPUB && r.page.is_some()));
    let titles: std::collections::BTreeSet<&str> =
        pages.iter().map(|r| r.chapter.as_str()).collect();
    for chapter in ["Down the Rabbit-Hole", "The Pool of Tears"] {
        assert!(titles.iter().any(|t| t.ends_with(chapter)), "{titles:?}");
    }
    for pair in pages.windows(2) {
        let (this, next) = (pair[0], pair[1]);
        assert_eq!(next.page, this.page.map(|p| p + 1));
        // Before the first chapter (the title page): none.
        if this.chapter.is_empty() {
            assert_eq!(this.chapter_left, None);
            continue;
        }
        let left = this.chapter_left.expect("pages left once counted");
        if next.chapter == this.chapter {
            assert_eq!(next.chapter_left, Some(left - 1), "{this:?} {next:?}");
        } else {
            assert_eq!(left, 0, "{this:?} {next:?}");
        }
    }
}

#[test]
fn arrows_turn_pages() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    let second = reading.kept();
    reading.run(core_actions::NEXT_PAGE);
    let third = reading.kept();
    reading.run(core_actions::LEFT);
    assert_eq!(reading.kept(), second);
    reading.run(core_actions::RIGHT);
    assert_eq!(reading.kept(), third);
    reading.run(core_actions::UP);
    assert_eq!(reading.kept(), second);
    reading.run(core_actions::DOWN);
    assert_eq!(reading.kept(), third);
}

/// Holds the button `name` long enough for a long press.
fn hold(reading: &mut Reading, name: &'static str) {
    for (after, state) in [(10, ButtonState::Pressed), (1000, ButtonState::Released)] {
        reading.now += after;
        let effects = reading
            .kernel
            .handle(Instant(reading.now), InputEvent::Button { name, state });
        reading.settle(effects);
    }
    reading.now += 6000;
    let effects = reading.kernel.tick(Instant(reading.now));
    reading.settle(effects);
}

#[test]
fn held_page_buttons_go_a_chapter_on_and_back() {
    let epub = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");
    let mut reading = Reading::with_book(Shared::default(), EPUB, epub);
    reading.open();
    set(&mut reading, "reader.hold-chapters", "on");
    let chapter = |reading: &Reading| reading.heard.borrow().last().unwrap().1.chapter.clone();
    hold(&mut reading, "page-forward");
    hold(&mut reading, "page-forward");
    let second = chapter(&reading);
    hold(&mut reading, "page-forward");
    assert_ne!(chapter(&reading), second);
    hold(&mut reading, "page-back");
    assert_eq!(chapter(&reading), second);
}

#[test]
fn with_two_buttons_holding_forward_still_opens_the_menu() {
    let epub = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");
    let mut reading = Reading::with_preset(Shared::default(), EPUB, epub, "core.two-buttons");
    reading.open();
    hold(&mut reading, "page-forward");
    assert_ne!(
        reading.kernel.active_screen().map(Id::as_str),
        Some("reader.screen")
    );
}

/// What a status item hears of the chapter: the page in it and its pages.
type InChapter = Rc<RefCell<Option<(String, String)>>>;

/// A plugin with a status item that notes what it is told.
struct Probe(InChapter);

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("probe"), "Probe", Version::new(0, 1, 0))
            .description("Notes what status items are told.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let heard = self.0.clone();
        r.add_status_item(paperpaw_sdk::StatusItemSpec::new(
            Id::new_static("probe.chapter"),
            "probe",
            Box::new(move |_, cx| {
                let get = |key| cx.get(key).unwrap_or_default().to_string();
                *heard.borrow_mut() =
                    Some((get("reader.chapter-page"), get("reader.chapter-pages")));
                None
            }),
        ))
    }
}

/// The EPUB `epub` on a card, a status line with [`Probe`]'s item shown.
fn probed(epub: &[u8], in_chapter: InChapter) -> Reading {
    let storage = Shared::default();
    storage.create_dir("books").unwrap();
    storage.write(EPUB, epub).unwrap();
    let heard = Heard::default();
    let mut kernel = Kernel::builder(DEVICE)
        .setting_default_text("reader.status.bottom-left", "probe.chapter")
        .storage(storage.clone())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_format_epub::EpubFormat)
        .plugin(Reader)
        .plugin(Listener(heard.clone()))
        .plugin(Probe(in_chapter))
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let mut reading = Reading {
        kernel,
        storage,
        heard,
        now: 0,
        book: EPUB,
    };
    reading.settle(effects);
    reading
}

#[test]
fn status_lines_know_the_page_in_the_chapter() {
    let epub = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");
    let in_chapter = InChapter::default();
    let mut reading = probed(epub, in_chapter.clone());
    reading.open();
    let last = |reading: &Reading| reading.heard.borrow().last().unwrap().1.clone();
    let mut turned = 0;
    while last(&reading).pages.is_none() || !last(&reading).chapter.ends_with("Rabbit-Hole") {
        reading.run(core_actions::NEXT_PAGE);
        turned += 1;
        assert!(turned < 400, "the pages are never counted");
    }
    // The chapter's first page, then its second.
    for page in ["1", "2"] {
        reading.kernel.render(&mut |_, _| {});
        let (at, pages) = in_chapter.borrow().clone().expect("the line was drawn");
        let left = last(&reading).chapter_left.unwrap();
        assert_eq!(at, page);
        assert_eq!(
            pages.parse::<u32>().unwrap(),
            at.parse::<u32>().unwrap() + left
        );
        reading.run(core_actions::NEXT_PAGE);
    }
}

#[test]
fn held_select_marks_the_page() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    let bookmarks = |reading: &Reading| {
        let marks = paperpaw_sdk::annotations::marks(&reading.storage, BOOK);
        marks.iter().filter(|m| m.kind == Kind::Bookmark).count()
    };
    hold(&mut reading, "confirm");
    assert_eq!(bookmarks(&reading), 1);
    assert_eq!(
        reading.kernel.active_screen().map(Id::as_str),
        Some("reader.screen")
    );
    hold(&mut reading, "confirm");
    assert_eq!(bookmarks(&reading), 0);
}

/// Presses the button `name` briefly (let go before the kernel's timers
/// run, which would make it a long press).
fn press(reading: &mut Reading, name: &'static str) {
    let mut effects = None;
    for (after, state) in [(10, ButtonState::Pressed), (50, ButtonState::Released)] {
        reading.now += after;
        effects = Some(
            reading
                .kernel
                .handle(Instant(reading.now), InputEvent::Button { name, state }),
        );
    }
    if let Some(effects) = effects {
        reading.settle(effects);
    }
    reading.now += 6000;
    let effects = reading.kernel.tick(Instant(reading.now));
    reading.settle(effects);
}

/// Sets a setting as the settings screen would.
fn set(reading: &mut Reading, id: &str, value: &str) {
    reading.now += 10;
    let effects = reading
        .kernel
        .set_setting(Instant(reading.now), id, value)
        .unwrap();
    reading.settle(effects);
}

#[test]
fn held_page_buttons_turn_one_page_by_default() {
    let epub = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");
    let mut reading = Reading::with_book(Shared::default(), EPUB, epub);
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    let page = |reading: &mut Reading| reading.kept();
    let start = page(&mut reading);
    reading.run(core_actions::NEXT_PAGE);
    let next = page(&mut reading);
    reading.run(core_actions::PREV_PAGE);
    // Held, a page on, as pressed: no chapter.
    hold(&mut reading, "page-forward");
    assert_eq!(page(&mut reading), next);
    press(&mut reading, "page-back");
    assert_eq!(page(&mut reading), start);
    // On again: held, a chapter on.
    set(&mut reading, "reader.hold-chapters", "on");
    hold(&mut reading, "page-forward");
    hold(&mut reading, "page-forward");
    let chapter = |reading: &Reading| reading.heard.borrow().last().unwrap().1.chapter.clone();
    let second = chapter(&reading);
    hold(&mut reading, "page-forward");
    assert_ne!(chapter(&reading), second);
}

#[test]
fn swapped_page_buttons_turn_the_other_way_while_reading() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    let at = |reading: &Reading| {
        let heard = reading.heard.borrow();
        let last = &heard.last().unwrap().1;
        (last.section, last.offset)
    };
    reading.run(core_actions::NEXT_PAGE);
    reading.run(core_actions::NEXT_PAGE);
    let third = at(&reading);
    set(&mut reading, "core.swap-page-buttons", "on");
    press(&mut reading, "page-forward");
    let second = at(&reading);
    assert!(second < third, "{second:?} {third:?}");
    press(&mut reading, "page-back");
    assert_eq!(at(&reading), third);
}

/// Where the sixth page begins with the settings `sets` (pages of
/// running text: the first ones end where the book's parts do).
fn sixth_page(sets: &[(&str, &str)]) -> (u32, u32) {
    let mut reading = Reading::new(Shared::default());
    for (id, value) in sets {
        set(&mut reading, id, value);
    }
    reading.open();
    for _ in 0..5 {
        reading.run(core_actions::NEXT_PAGE);
    }
    reading.kept()
}

#[test]
fn paragraphs_keep_lines_together_as_set() {
    // Lines kept together: pages end earlier, where paragraphs break.
    let standard = sixth_page(&[]);
    let kept = sixth_page(&[("core.widows", "3"), ("core.orphans", "3")]);
    assert!(kept < standard, "{kept:?} {standard:?}");
}

#[test]
fn the_page_holds_what_its_type_and_margins_leave_room_for() {
    let standard = sixth_page(&[]);
    // Margins at the top and bottom of their own: more lines, or fewer.
    assert!(sixth_page(&[("core.margin-ends", "0")]) > standard);
    assert!(sixth_page(&[("core.margin-ends", "20")]) < standard);
    // Following the sides until set: larger sides, fewer lines too.
    assert!(sixth_page(&[("core.margin", "20")]) < sixth_page(&[("core.margin-ends", "20")]));
    // Space between paragraphs, apart from the indents.
    assert!(sixth_page(&[("core.paragraph-spacing", "on")]) < standard);
    let flat = sixth_page(&[("core.indent", "off")]);
    assert!(sixth_page(&[("core.indent", "off"), ("core.paragraph-spacing", "on")]) < flat);
    // Lines closer than the font sets them.
    assert!(
        sixth_page(&[("core.line-spacing", "80")]) > sixth_page(&[("core.line-spacing", "100")])
    );
}

#[test]
fn a_book_may_be_turned_on_its_own() {
    use paperpaw_platform::Rotation;
    let storage = Shared::default();
    let mut reading = Reading::new(storage.clone());
    reading.open();
    set(&mut reading, "core.scope-only", "on");
    set(&mut reading, "core.rotation", "core.rotation.landscape");
    assert_eq!(reading.kernel.rotation(), Rotation::Deg90);
    // Closed: the device as it was; opened again, turned again.
    reading.run(core_actions::BACK);
    assert_eq!(reading.kernel.rotation(), Rotation::Deg0);
    reading.open();
    assert_eq!(reading.kernel.rotation(), Rotation::Deg90);
}

#[test]
fn a_percentage_is_gone_to_and_back_returns() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    let before = reading.kept();
    let answer = |reading: &mut Reading, percent: &str| {
        reading.now += 10;
        let params = Params::new()
            .with(params::PATH, BOOK)
            .with(params::TEXT, percent);
        let effects = reading.kernel.send_event(
            Instant(reading.now),
            Id::new_static("reader.percent-entered"),
            params,
        );
        reading.settle(effects);
        reading.heard.borrow().last().unwrap().1.progress
    };
    let half = answer(&mut reading, "50");
    assert!((45.0..=55.0).contains(&half), "{half}");
    let end = answer(&mut reading, "100");
    assert!(end > 95.0, "{end}");
    // Back to where the jumps began.
    reading.run(core_actions::BACK);
    reading.run(core_actions::BACK);
    assert_eq!(reading.kept(), before);
}

/// Where pages 0 to `pages` of the book begin, turned to one by one, and
/// the storage with the book's position after the last of them.
fn page_starts(pages: usize) -> (Shared, Vec<(u32, u32)>) {
    let text = include_str!("../../../samples/library/books/alice-in-wonderland.txt");
    page_starts_of(text, pages)
}

/// The same for the book `text`.
fn page_starts_of(text: &str, pages: usize) -> (Shared, Vec<(u32, u32)>) {
    let storage = Shared::default();
    let mut reading = Reading::with_book(storage.clone(), BOOK, text.as_bytes());
    reading.open();
    let mut starts = vec![reading.kept()];
    for _ in 0..pages {
        reading.run(core_actions::NEXT_PAGE);
        starts.push(reading.kept());
    }
    (storage, starts)
}

/// The book opened again where it was left, its page index made anew
/// as far as one step of eight pages before the storage refuses more.
fn reopened_with_a_short_index(storage: &Shared) -> Reading {
    let dir = format!(
        ".paperpaw/plugins/reader/books/{:016x}",
        paperpaw_sdk::digest::fnv1a64(BOOK)
    );
    let mut indexes = Vec::new();
    storage
        .list(&dir, &mut |entry| {
            if entry.name.starts_with("pages-") {
                indexes.push(format!("{dir}/{}", entry.name));
            }
        })
        .unwrap();
    assert!(!indexes.is_empty());
    for index in indexes {
        storage.remove(&index).unwrap();
    }
    storage.1.set(Some(1));
    // The book is on the card already.
    let mut reading = Reading::with_book(storage.clone(), BOOK, b"");
    reading.open();
    reading
}

#[test]
fn a_page_back_stays_near_where_the_index_does_not_reach() {
    let (storage, starts) = page_starts(12);
    let mut reading = reopened_with_a_short_index(&storage);
    assert_eq!(reading.kept(), starts[12]);
    reading.run(core_actions::PREV_PAGE);
    let back = reading.kept();
    // About the page before: not the last page the index knows (the
    // eighth), nor where it was.
    assert!(
        starts[10] < back && back < starts[12],
        "{back:?} in {starts:?}"
    );
    // And on from there, past the page left.
    reading.run(core_actions::NEXT_PAGE);
    reading.run(core_actions::NEXT_PAGE);
    assert!(reading.kept() > starts[12], "{:?}", reading.kept());
}

#[test]
fn an_index_the_storage_stops_taking_is_still_used() {
    let (storage, starts) = page_starts(12);
    let mut reading = reopened_with_a_short_index(&storage);
    // The pages it holds are gone to by number.
    reading.now += 10;
    let params = Params::new()
        .with(params::PATH, BOOK)
        .with(params::TEXT, "4");
    let effects = reading.kernel.send_event(
        Instant(reading.now),
        Id::new_static("reader.page-entered"),
        params,
    );
    reading.settle(effects);
    assert_eq!(reading.kept(), starts[3]);
    reading.run(core_actions::PREV_PAGE);
    assert_eq!(reading.kept(), starts[2]);
}

#[test]
fn a_position_that_cannot_be_laid_out_opens_the_book_at_its_start() {
    let epub = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");
    let storage = Shared::default();
    let mut reading = Reading::with_book(storage.clone(), EPUB, epub);
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    let second = reading.kept();
    reading.run(core_actions::BACK);
    storage
        .write(
            &paperpaw_sdk::positions::file(EPUB),
            b"[position]\nsection = 2\noffset = 100\n",
        )
        .unwrap();
    // Its third part cannot be read (a damaged card, say).
    storage.2.set(Some("/sections/2/txt"));
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    assert_eq!(reading.kept(), second);
}

/// The page indexes kept for `book`.
fn indexes(storage: &Shared, book: &str) -> Vec<String> {
    let dir = format!(
        ".paperpaw/plugins/reader/books/{:016x}",
        paperpaw_sdk::digest::fnv1a64(book)
    );
    let mut found = Vec::new();
    let _ = storage.list(&dir, &mut |entry| {
        if entry.name.starts_with("pages-") {
            found.push(entry.name.to_string());
        }
    });
    found
}

/// The pages of the book as counted once it is opened.
fn pages_counted(reading: &mut Reading) -> u32 {
    reading.run(core_actions::NEXT_PAGE);
    let last = reading.heard.borrow().last().unwrap().1.clone();
    last.pages.expect("the pages are counted")
}

#[test]
fn another_edition_of_a_book_is_counted_anew() {
    let text = include_str!("../../../samples/library/books/alice-in-wonderland.txt");
    let storage = Shared::default();
    let mut reading = Reading::new(storage.clone());
    reading.open();
    let whole = pages_counted(&mut reading);
    // Replaced by a shorter one under the same name, and opened again.
    let half = &text[..text.len() / 2];
    let half = &half[..half.rfind('\n').unwrap()];
    storage.write(BOOK, half.as_bytes()).unwrap();
    let mut again = Reading::new(storage);
    again.open();
    let shorter = pages_counted(&mut again);
    let mut fresh = Reading::with_book(Shared::default(), BOOK, half.as_bytes());
    fresh.open();
    assert_eq!(shorter, pages_counted(&mut fresh));
    assert!(shorter < whole, "{shorter} of {whole}");
}

#[test]
fn page_indexes_of_layouts_left_do_not_pile_up() {
    let storage = Shared::default();
    let mut reading = Reading::new(storage.clone());
    reading.open();
    for size in 7..=16 {
        set(
            &mut reading,
            settings::FONT_SIZE.static_str(),
            &size.to_string(),
        );
    }
    let kept = indexes(&storage, BOOK).len();
    assert!((1..=4).contains(&kept), "{kept} indexes");
}

#[test]
fn text_chosen_is_let_go_when_the_page_turns_by_itself() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    reading.run(actions::SELECT);
    reading.run(actions::AUTO_TURN);
    let before = reading.kept();
    // The page turns by itself.
    reading.now += 120_000;
    let effects = reading.kernel.tick(Instant(reading.now));
    reading.settle(effects);
    reading.run(actions::AUTO_TURN);
    assert!(reading.kept() > before);
    // Nothing chosen on the new page: the page is marked as without.
    reading.run(actions::TOGGLE_BOOKMARK);
    let marks = paperpaw_sdk::annotations::marks(&reading.storage, BOOK);
    assert_eq!(marks.iter().filter(|m| m.kind == Kind::Bookmark).count(), 1);
}

#[test]
fn search_and_contents_read_the_book_as_the_page_shows_it() {
    let epub = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");
    let storage = Shared::default();
    let mut reading = Reading::with_book(storage.clone(), EPUB, epub);
    // Without the book's own styles: its chapters converted into a cache
    // of their own.
    set(&mut reading, "core.publisher-styles", "off");
    reading.open();
    reading.run(actions::TOC);
    reading.run(core_actions::BACK);
    reading.now += 10;
    let params = Params::new()
        .with(params::PATH, EPUB)
        .with(params::TEXT, "Rabbit");
    let effects = reading.kernel.send_event(
        Instant(reading.now),
        Id::new_static("reader.search-entered"),
        params,
    );
    reading.settle(effects);
    assert_eq!(
        reading.kernel.active_screen().map(Id::as_str),
        Some("reader.search-results")
    );
    // No chapter converted with the styles as well.
    let cache = format!(
        ".paperpaw/plugins/format-epub/{}",
        paperpaw_sdk::library::key(EPUB)
    );
    let mut styled = Vec::new();
    storage
        .list(&cache, &mut |entry| {
            let chapter = entry.name.strip_suffix(".txt");
            if chapter.is_some_and(|c| c.bytes().all(|b| b.is_ascii_digit())) {
                styled.push(entry.name.to_string());
            }
        })
        .unwrap();
    assert!(styled.is_empty(), "{styled:?}");
}

/// How often a status item was asked what it shows.
type Asked = Rc<std::cell::Cell<u32>>;

/// A plugin with a status item that counts how often it is asked.
struct Counter(Asked);

impl Plugin for Counter {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("counter"), "Counter", Version::new(0, 1, 0))
            .description("Counts what status items are asked.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let asked = self.0.clone();
        r.add_status_item(paperpaw_sdk::StatusItemSpec::new(
            Id::new_static("counter.item"),
            "counter",
            Box::new(move |_, _| {
                asked.set(asked.get() + 1);
                None
            }),
        ))
    }
}

#[test]
fn a_page_drawn_in_bands_asks_the_status_line_once() {
    let storage = Shared::default();
    storage.create_dir("books").unwrap();
    let text = include_str!("../../../samples/library/books/alice-in-wonderland.txt");
    storage.write(BOOK, text.as_bytes()).unwrap();
    let asked = Asked::default();
    // Bands of a hundred rows, as on the X4.
    let mut kernel = Kernel::builder(DEVICE)
        .frame_rows(100)
        .setting_default_text("reader.status.bottom-left", "counter.item")
        .setting_default_text("reader.status.top-lines", "")
        .storage(storage.clone())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(Reader)
        .plugin(Counter(asked.clone()))
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let mut reading = Reading {
        kernel,
        storage,
        heard: Heard::default(),
        now: 0,
        book: BOOK,
    };
    reading.settle(effects);
    reading.open();
    reading.kernel.render(&mut |_, _| {});
    reading.run(core_actions::NEXT_PAGE);
    asked.set(0);
    let mut bands = 0;
    reading.kernel.render(&mut |_, _| bands += 1);
    assert!(bands > 4, "{bands} bands");
    assert!(
        (1..=2).contains(&asked.get()),
        "asked {} times",
        asked.get()
    );
}

#[test]
fn small_devices_count_pages_with_rests_between() {
    let storage = Shared::default();
    storage.create_dir("books").unwrap();
    let text = include_str!("../../../samples/library/books/alice-in-wonderland.txt");
    storage.write(BOOK, text.as_bytes()).unwrap();
    // The X4: about half a second to lay out a page.
    let small = DeviceInfo {
        memory: MemoryClass::Small,
        ..DEVICE
    };
    let mut kernel = Kernel::builder(small)
        .storage(storage.clone())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(Reader)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let params = Params::new().with(params::PATH, BOOK);
    let mut effects = kernel.open_slot(Instant(10), slots::READER, params);
    // Two seconds of counting.
    let mut now = 10;
    while let Some(at) = effects.wake_at().filter(|at| at.0 <= 2010) {
        now = now.max(at.0);
        effects = kernel.tick(Instant(now));
    }
    let index = indexes(&storage, BOOK).pop().expect("an index");
    let dir = format!(
        ".paperpaw/plugins/reader/books/{:016x}",
        paperpaw_sdk::digest::fnv1a64(BOOK)
    );
    let pages = storage.size(&format!("{dir}/{index}")).unwrap() / 8;
    // A page and a rest as long, not a page every two milliseconds.
    assert!((1..=8).contains(&pages), "{pages} pages counted");
}

#[test]
fn where_i_was_chosen_with_the_buttons_returns() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    let before = reading.kept();
    reading.run(actions::SKIM_BOOK);
    reading.run(core_actions::NEXT_PAGE);
    assert_ne!(reading.kept(), before);
    // The buttons below the bar: Chapter back, Where I was, Chapter on,
    // Page number; down moves to the first, then the next.
    reading.run(core_actions::DOWN);
    reading.run(core_actions::DOWN);
    reading.run(core_actions::CONFIRM);
    assert_eq!(
        reading.kernel.active_screen().map(Id::as_str),
        Some("reader.screen")
    );
    assert_eq!(reading.kept(), before);
}

#[test]
fn select_in_the_skimming_bar_stays_where_it_went() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    reading.run(actions::SKIM_BOOK);
    reading.run(core_actions::NEXT_PAGE);
    let skimmed = reading.kept();
    reading.run(core_actions::CONFIRM);
    assert_eq!(
        reading.kernel.active_screen().map(Id::as_str),
        Some("reader.screen")
    );
    assert_eq!(reading.kept(), skimmed);
}

/// The entries of a menu as a plugin's action finds them.
type Entries = Rc<RefCell<Vec<String>>>;

/// A plugin whose action `looker.look` notes the reader's More menu.
struct MenuLook(Entries);

impl Plugin for MenuLook {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("looker"), "Menus", Version::new(0, 1, 0))
            .description("Looks into menus.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let entries = self.0.clone();
        r.add_action(
            paperpaw_sdk::ActionSpec::new(Id::new_static("looker.look"), "look").handler(
                move |env: &paperpaw_sdk::Env<'_>, _, _: &mut paperpaw_sdk::Commands| {
                    *entries.borrow_mut() = env
                        .menus()
                        .items(paperpaw_plugin_reader::MORE_MENU.as_str())
                        .iter()
                        .map(|item| item.id.as_str().to_string())
                        .collect();
                },
            ),
        )
    }
}

#[test]
fn status_lines_switch_from_the_menu_without_a_touch_screen() {
    let entries = Entries::default();
    let mut kernel = Kernel::builder(DEVICE)
        .storage(Shared::default())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Reader)
        .plugin(MenuLook(entries.clone()))
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let mut reading = Reading {
        kernel,
        storage: Shared::default(),
        heard: Heard::default(),
        now: 0,
        book: BOOK,
    };
    reading.settle(effects);
    reading.run(Id::new_static("looker.look"));
    let entries = entries.borrow();
    assert!(
        entries.iter().any(|e| e == "reader.next-status-line"),
        "{entries:?}"
    );
}

/// The notices shown.
type Notices = Rc<RefCell<Vec<String>>>;

/// A plugin that shows notices as nothing and keeps their texts.
struct NoticeTaker(Notices);

/// A notice, shown as nothing.
struct Notice;

impl paperpaw_sdk::Screen for Notice {
    fn draw(&mut self, _: &mut paperpaw_sdk::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> paperpaw_sdk::Handled {
        paperpaw_sdk::Handled::No
    }
}

impl Plugin for NoticeTaker {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("notices"), "Notices", Version::new(0, 1, 0))
            .description("Keeps the notices shown.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let notices = self.0.clone();
        r.add_screen(paperpaw_sdk::ScreenSpec::new(
            Id::new_static("notices.notice"),
            "notice",
            Box::new(move |_, params| {
                let text = params.get(params::TEXT).unwrap_or_default();
                notices.borrow_mut().push(text.to_string());
                Box::new(Notice)
            }),
        ))?;
        r.bind_slot(
            slots::NOTICE,
            Id::new_static("notices.notice"),
            paperpaw_sdk::Priority::DEFAULT,
        )
    }
}

/// A kernel reading the book on `storage`, keeping the notices shown.
fn with_notices(storage: Shared) -> (Reading, Notices) {
    storage.create_dir("books").unwrap();
    let text = include_str!("../../../samples/library/books/alice-in-wonderland.txt");
    storage.write(BOOK, text.as_bytes()).unwrap();
    let notices = Notices::default();
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(Reader)
        .plugin(NoticeTaker(notices.clone()))
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let mut reading = Reading {
        kernel,
        storage,
        heard: Heard::default(),
        now: 0,
        book: BOOK,
    };
    reading.settle(effects);
    reading.open();
    (reading, notices)
}

#[test]
fn a_place_that_cannot_be_kept_is_said_once() {
    let storage = Shared::default();
    *storage.3.borrow_mut() = Some(".paperpaw/positions".into());
    let (mut reading, notices) = with_notices(storage);
    for _ in 0..3 {
        reading.run(core_actions::NEXT_PAGE);
    }
    let said: Vec<String> = notices
        .borrow()
        .iter()
        .filter(|n| n.contains("could not be saved"))
        .cloned()
        .collect();
    assert_eq!(said.len(), 1, "{said:?}");
}

#[test]
fn a_bookmark_that_cannot_be_kept_is_said() {
    let storage = Shared::default();
    *storage.3.borrow_mut() = Some(".paperpaw/annotations".into());
    let (mut reading, notices) = with_notices(storage);
    reading.run(actions::TOGGLE_BOOKMARK);
    assert!(
        notices.borrow().iter().any(|n| n.contains("bookmark")),
        "{:?}",
        notices.borrow()
    );
}

#[test]
fn a_page_back_after_returning_from_a_jump_is_the_page_before() {
    let (storage, starts) = page_starts(3);
    let mut reading = Reading::new(storage);
    reading.open();
    assert_eq!(reading.kept(), starts[3]);
    reading.now += 10;
    let params = Params::new()
        .with(params::PATH, BOOK)
        .with(params::TEXT, "20");
    let effects = reading.kernel.send_event(
        Instant(reading.now),
        Id::new_static("reader.page-entered"),
        params,
    );
    reading.settle(effects);
    assert!(reading.kept() > starts[3]);
    reading.run(core_actions::BACK);
    assert_eq!(reading.kept(), starts[3]);
    reading.run(core_actions::PREV_PAGE);
    assert_eq!(reading.kept(), starts[2]);
}

/// The book opened again at `at`, with a short page index (see
/// [`reopened_with_a_short_index`]).
fn reopened_at(storage: &Shared, at: (u32, u32)) -> Reading {
    let position = format!("[position]\nsection = {}\noffset = {}\n", at.0, at.1);
    storage
        .write(&paperpaw_sdk::positions::file(BOOK), position.as_bytes())
        .unwrap();
    reopened_with_a_short_index(storage)
}

#[test]
fn a_page_back_over_looser_text_leaves_no_line_out() {
    // Short lines of talk, then long paragraphs: a page of the talk holds
    // far less text than a page of the paragraphs after it.
    let mut text = String::new();
    for i in 0..400 {
        text.push_str(&format!("\"Yes,\" said {i}.\n\n"));
    }
    let words = "word ".repeat(120);
    for _ in 0..60 {
        text.push_str(&words);
        text.push_str("\n\n");
    }
    let loose = u32::try_from(text.find("word").unwrap()).unwrap();
    let (storage, starts) = page_starts_of(&text, 200);
    // The second page of the paragraphs.
    let here = starts
        .iter()
        .position(|s| s.1 > loose)
        .map(|i| i + 1)
        .unwrap();
    let mut reading = reopened_at(&storage, starts[here]);
    reading.run(core_actions::PREV_PAGE);
    let back = reading.kept();
    assert!(back < starts[here], "{back:?}");
    // The page there reaches the page left: on from it, nothing skipped.
    reading.run(core_actions::NEXT_PAGE);
    let on = reading.kept();
    assert!(on >= starts[here], "{on:?} skips to {:?}", starts[here]);
}

/// The page words a plugin asked the reader for.
type WordsAsked = Rc<RefCell<Option<paperpaw_sdk::reading::PageWords>>>;

const WORDS_HEARD: paperpaw_sdk::event::Event<paperpaw_sdk::reading::PageWords> =
    paperpaw_sdk::event::Event::new("asker.words");

/// A plugin whose action `asker.ask` asks for the page's words.
struct Asker(WordsAsked);

impl Plugin for Asker {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("asker"), "Asker", Version::new(0, 1, 0))
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_action(
            paperpaw_sdk::ActionSpec::new(Id::new_static("asker.ask"), "ask").handler(
                |_: &paperpaw_sdk::Env<'_>, _, out: &mut paperpaw_sdk::Commands| {
                    out.push(paperpaw_sdk::reading::ask_page_words(&WORDS_HEARD));
                },
            ),
        )?;
        let asked = self.0.clone();
        r.on(WORDS_HEARD, move |_, words, _| {
            *asked.borrow_mut() = Some(words.clone());
        })
    }
}

#[test]
fn any_plugin_reads_the_words_of_the_page() {
    let storage = Shared::default();
    storage.create_dir("books").unwrap();
    let text = include_str!("../../../samples/library/books/alice-in-wonderland.txt");
    storage.write(BOOK, text.as_bytes()).unwrap();
    let asked = WordsAsked::default();
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(Reader)
        .plugin(Asker(asked.clone()))
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let mut reading = Reading {
        kernel,
        storage,
        heard: Heard::default(),
        now: 0,
        book: BOOK,
    };
    reading.settle(effects);
    // No book open: no answer.
    reading.run(Id::new_static("asker.ask"));
    assert!(asked.borrow().is_none());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    reading.run(Id::new_static("asker.ask"));
    let words = asked.borrow().clone().expect("the reader answered");
    assert_eq!(words.path, BOOK);
    assert!(words.words.len() > 20, "{}", words.words.len());
    let kept = reading.kept();
    assert_eq!(
        words.start,
        paperpaw_sdk::document::Locator::new(kept.0, kept.1)
    );
    // Each word is the book's text at its place.
    for word in &words.words[..10] {
        let from = word.start.offset as usize;
        assert!(
            text[from..].starts_with(word.text.trim_end_matches('-')),
            "{word:?}"
        );
    }
}

#[test]
fn a_place_kept_by_the_reader_before_is_read() {
    let storage = Shared::default();
    let mut first = Reading::new(storage.clone());
    first.open();
    for _ in 0..3 {
        first.run(core_actions::NEXT_PAGE);
    }
    let left = first.kept();
    // Kept where the reader kept it before positions were shared.
    storage
        .remove(&paperpaw_sdk::positions::file(BOOK))
        .unwrap();
    let dir = format!(
        ".paperpaw/plugins/reader/books/{:016x}",
        paperpaw_sdk::digest::fnv1a64(BOOK)
    );
    let old = format!("[progress]\nsection = {}\noffset = {}\n", left.0, left.1);
    storage
        .write(&format!("{dir}/progress.ini"), old.as_bytes())
        .unwrap();
    let mut again = Reading::new(storage);
    again.open();
    reading_runs_nothing(&mut again);
    assert_eq!(again.kept(), left);
}

/// Lets a moment pass without input.
fn reading_runs_nothing(reading: &mut Reading) {
    reading.now += 10;
    let effects = reading.kernel.tick(Instant(reading.now));
    reading.settle(effects);
}

#[test]
fn a_book_closed_says_where_as_an_xpointer() {
    let epub = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");
    let mut reading = Reading::with_book(Shared::default(), EPUB, epub);
    reading.open();
    for _ in 0..5 {
        reading.run(core_actions::NEXT_PAGE);
    }
    reading.run(core_actions::BACK);
    let heard = reading.heard.borrow();
    let (name, closed) = heard.last().unwrap();
    assert_eq!(*name, "closed");
    let xpointer = closed.xpointer.clone().unwrap_or_default();
    assert!(xpointer.starts_with("/body/DocFragment["), "{xpointer:?}");
}

/// The params a screen of one's own was opened with.
type OpenedWith = Rc<RefCell<Option<Params>>>;

/// A plugin whose contents take the place of the reader's.
struct OwnContents(OpenedWith);

/// A screen that shows nothing.
struct Blank;

impl paperpaw_sdk::Screen for Blank {
    fn draw(&mut self, _: &mut paperpaw_sdk::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> paperpaw_sdk::Handled {
        paperpaw_sdk::Handled::No
    }
}

impl Plugin for OwnContents {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("contents"),
            "Contents",
            Version::new(0, 1, 0),
        )
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let opened = self.0.clone();
        r.add_screen(paperpaw_sdk::ScreenSpec::new(
            Id::new_static("contents.list"),
            "contents",
            Box::new(move |_, params| {
                *opened.borrow_mut() = Some(params.clone());
                Box::new(Blank)
            }),
        ))?;
        r.bind_slot(
            slots::TOC,
            Id::new_static("contents.list"),
            paperpaw_sdk::Priority::PREFERRED,
        )
    }
}

#[test]
fn the_contents_are_a_slot_another_plugin_fills() {
    let storage = Shared::default();
    storage.create_dir("books").unwrap();
    let text = include_str!("../../../samples/library/books/alice-in-wonderland.txt");
    storage.write(BOOK, text.as_bytes()).unwrap();
    let opened = OpenedWith::default();
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(Reader)
        .plugin(OwnContents(opened.clone()))
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let mut reading = Reading {
        kernel,
        storage,
        heard: Heard::default(),
        now: 0,
        book: BOOK,
    };
    reading.settle(effects);
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    let at = reading.kept();
    reading.run(actions::TOC);
    assert_eq!(
        reading.kernel.active_screen().map(Id::as_str),
        Some("contents.list")
    );
    let params = opened.borrow().clone().expect("opened");
    assert_eq!(params.get(params::PATH), Some(BOOK));
    assert_eq!(params.get(params::OFFSET), Some(at.1.to_string().as_str()));
}

/// The books a block filter was told of.
type Told = Rc<RefCell<Vec<String>>>;

/// A plugin that leaves out every paragraph naming the Rabbit.
struct NoRabbit(Told);

impl Plugin for NoRabbit {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("norabbit"),
            "No rabbit",
            Version::new(0, 1, 0),
        )
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        use paperpaw_sdk::filters::{TextFilterSpec, Verdict};
        let told = self.0.clone();
        r.add_text_filter(TextFilterSpec::block(
            Id::new_static("norabbit.filter"),
            "no rabbit",
            Box::new(move |_, cx, block| {
                let mut told = told.borrow_mut();
                if !told.iter().any(|t| t == cx.path) {
                    told.push(cx.path.to_string());
                }
                match block.text.contains("Rabbit") {
                    true => Verdict::Drop,
                    false => Verdict::Keep,
                }
            }),
            Box::new(|_| 1),
        ))
    }
}

#[test]
fn a_block_filter_leaves_paragraphs_out() {
    let storage = Shared::default();
    storage.create_dir("books").unwrap();
    let text = include_str!("../../../samples/library/books/alice-in-wonderland.txt");
    storage.write(BOOK, text.as_bytes()).unwrap();
    let told = Told::default();
    let asked = WordsAsked::default();
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(Reader)
        .plugin(NoRabbit(told.clone()))
        .plugin(Asker(asked.clone()))
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let mut reading = Reading {
        kernel,
        storage,
        heard: Heard::default(),
        now: 0,
        book: BOOK,
    };
    reading.settle(effects);
    reading.open();
    for _ in 0..8 {
        reading.run(core_actions::NEXT_PAGE);
        reading.run(Id::new_static("asker.ask"));
        let words = asked.borrow().clone().expect("the reader answered");
        assert!(!words.text().contains("Rabbit"), "{}", words.text());
    }
    assert_eq!(*told.borrow(), [BOOK.to_string()]);
}

/// How often the coming book was asked for.
type Opened = Rc<std::cell::Cell<u32>>;

const COMING: &str = "books/coming.part";
const ARRIVED: Id = Id::new_static("coming.arrived");

/// A format of books still coming in: never ready here; and an action
/// saying the book arrived.
struct Coming(Opened);

impl Plugin for Coming {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("coming"), "Coming", Version::new(0, 1, 0))
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let asked = self.0.clone();
        r.add_format(paperpaw_sdk::FormatSpec::new(
            Id::new_static("coming.part"),
            "coming",
            vec![std::borrow::Cow::Borrowed("part")],
            Box::new(move |_, _, _| {
                asked.set(asked.get() + 1);
                Err(paperpaw_sdk::document::DocumentError::NotReady)
            }),
        ))?;
        r.add_action(
            paperpaw_sdk::ActionSpec::new(ARRIVED, "coming.arrived").handler(|_, _, out| {
                for path in ["books/other.part", COMING] {
                    let ready = events::DocumentReady::new(path.to_string());
                    out.push(events::DOCUMENT_READY.emit(ready));
                }
            }),
        )
    }
}

#[test]
fn a_book_still_coming_opens_when_it_is_there() {
    let storage = Shared::default();
    storage.create_dir("books").unwrap();
    storage.write(COMING, b"part of it").unwrap();
    let asked = Opened::default();
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Reader)
        .plugin(Coming(asked.clone()))
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let mut reading = Reading {
        kernel,
        storage,
        heard: Heard::default(),
        now: 0,
        book: COMING,
    };
    reading.settle(effects);
    reading.open();
    assert_eq!(asked.get(), 1);
    // Told that another book and then this one is there: asked again once.
    reading.run(ARRIVED);
    assert_eq!(asked.get(), 2);
}

#[path = "reading/chapters.rs"]
mod chapters;

#[path = "reading/kept.rs"]
mod kept;

#[path = "reading/placed.rs"]
mod placed;
