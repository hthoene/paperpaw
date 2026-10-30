//! Reading apps besides the shipped reader: books open with the one
//! chosen for the book, else for its folder, else for its format, else
//! the default one, else the shipped reader; a reader is offered only
//! for the formats it reads; the choices outlast a restart; chosen with
//! the buttons of an X4 or by touch. One reading app is open at a time;
//! one that cannot open a book passes it on; a choice of a reader that
//! is gone is passed over silently. *Open with …* in the book menu
//! opens a book this time with the app chosen, or another way plugins
//! add, and keeps one only when asked to.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat,
    PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_sdk::readers::{self, Choice, ReaderSpec};
use paperpaw_sdk::{
    ActionSpec, Canvas, Commands, Cx, Env, Handled, Id, Manifest, MemoryStorage, Plugin,
    RegisterError, Registrar, SDK_VERSION, Screen, ScreenSpec,
};

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

/// A touch screen, as a Boox.
const BOOX: DeviceInfo = DeviceInfo::new(
    "test-boox",
    "Test Boox",
    DisplayInfo::new(1264, 1680, 300, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(false, true, false, &[]),
    MemoryClass::Large,
);

const TEXT: &str = "books/notes.txt";
const IN_FOLDER: &str = "books/plain/story.txt";
const EPUB: &str = "books/alice.epub";
const PLAIN: Id = Id::new_static("plain.app");
const PLAIN_SCREEN: Id = Id::new_static("plain.screen");
const SHIPPED: &str = "reader.screen";
/// The plain plugin's other way to open a book.
const SEND: Id = Id::new_static("plain.send");

thread_local! {
    /// The books sent, by their paths.
    static SENT: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A reader of plain text only, and actions that open books as the
/// library does.
struct Plain;

struct Page;

impl Screen for Page {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for Plain {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("plain"), "Plain", SDK_VERSION)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            PLAIN_SCREEN,
            "plain.screen",
            Box::new(|_, _| Box::new(Page)),
        ))?;
        r.add_reader(
            ReaderSpec::new(PLAIN, PLAIN_SCREEN).formats(&[Id::new_static("format-txt.txt")]),
        )?;
        r.add_translations(
            "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\nplain.app = Plain text reader\nplain.send = Send to a computer\n",
        )?;
        // Another way to open a book, in the list of *Open with …*.
        r.add_action(ActionSpec::new(SEND, "plain.send").handler(|_, _, _| {}))?;
        r.add_menu_entry(paperpaw_sdk::MenuEntry::new(
            paperpaw_sdk::builtin::menus::OPEN_WITH,
            SEND,
        ))?;
        r.on(
            paperpaw_sdk::builtin::events::BOOK_CHOSEN,
            |_, chosen, _| {
                if chosen.action == SEND {
                    SENT.with(|sent| sent.borrow_mut().push(chosen.path.clone()));
                }
            },
        )?;
        r.add_action(
            ActionSpec::new(Id::new_static("plain.ask-book"), "plain.ask-book").handler(
                |_: &Env<'_>, _, out: &mut Commands| out.push(readers::ask(Choice::Book(TEXT))),
            ),
        )?;
        r.add_action(
            ActionSpec::new(Id::new_static("plain.ask-folder"), "plain.ask-folder").handler(
                |_: &Env<'_>, _, out: &mut Commands| {
                    out.push(readers::ask(Choice::Folder("books/plain")))
                },
            ),
        )?;
        for (action, path) in [
            ("plain.text", TEXT),
            ("plain.folder", IN_FOLDER),
            ("plain.epub", EPUB),
        ] {
            r.add_action(
                ActionSpec::new(Id::new_static(action), action).handler(
                    move |_: &Env<'_>, _, out: &mut Commands| out.push(readers::open(path)),
                ),
            )?;
        }
        Ok(())
    }
}

/// A reader of plain text that cannot open any after all.
struct Picky;

const PICKY: Id = Id::new_static("picky.app");
const PICKY_SCREEN: Id = Id::new_static("picky.screen");

/// Says at once that it cannot open the book it was opened with.
struct Refusing(paperpaw_sdk::Params);

impl Screen for Refusing {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        cx.command(readers::cannot_open(&self.0, &PICKY));
    }
}

impl Plugin for Picky {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("picky"), "Picky", SDK_VERSION)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            PICKY_SCREEN,
            "picky.screen",
            Box::new(|_, params| Box::new(Refusing(params.clone()))),
        ))?;
        r.add_reader(
            ReaderSpec::new(PICKY, PICKY_SCREEN).formats(&[Id::new_static("format-txt.txt")]),
        )
    }
}

/// Tries readers whose screens are not its own, or not there.
struct Borrower;

impl Plugin for Borrower {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("borrower"), "Borrower", SDK_VERSION)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let other = r.add_reader(ReaderSpec::new(
            Id::new_static("borrower.app"),
            PLAIN_SCREEN,
        ));
        assert!(matches!(other, Err(RegisterError::Namespace { .. })));
        let missing = r.add_reader(ReaderSpec::new(
            Id::new_static("borrower.app"),
            Id::new_static("borrower.screen"),
        ));
        assert!(matches!(missing, Err(RegisterError::Invalid { .. })));
        Ok(())
    }
}

struct App {
    kernel: Kernel,
    now: u64,
}

impl App {
    fn new(storage: MemoryStorage, config: &str) -> Self {
        Self::with(storage, config, true)
    }

    /// The same, with the plain reader or without (as shipped).
    fn with(storage: MemoryStorage, config: &str, plain: bool) -> Self {
        Self::on(DEVICE, storage, config, plain)
    }

    /// The same on `device`.
    fn on(device: DeviceInfo, storage: MemoryStorage, config: &str, plain: bool) -> Self {
        Self::of(device, storage, config, |b| {
            if plain { b.plugin(Plain) } else { b }
        })
    }

    /// The same, with the plugins `more` adds.
    fn of(
        device: DeviceInfo,
        storage: MemoryStorage,
        config: &str,
        more: impl FnOnce(paperpaw_core::KernelBuilder) -> paperpaw_core::KernelBuilder,
    ) -> Self {
        let storage = storage.with(".paperpaw/plugins/setup/done", "");
        let builder = Kernel::builder(device)
            .storage(storage)
            // On touch screens without the hints over it all.
            .user_config(format!(
                "[help]\nshow = false\n[core]\nlanguage = en\n{config}"
            ));
        let builder = paperpaw_app::install(builder, paperpaw_app::Fonts::Packs);
        let mut kernel = more(builder).build().unwrap();
        let effects = kernel.start(Instant(0));
        let mut app = Self { kernel, now: 0 };
        app.settle(effects);
        app
    }

    fn settle(&mut self, mut effects: Effects) {
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

    /// A tap on the middle of the row or button that starts with `label`,
    /// the topmost.
    fn tap(&mut self, label: &str) -> String {
        self.press_on(label, 60)
    }

    /// A finger held a second on what starts with `label`.
    fn long_press(&mut self, label: &str) -> String {
        self.press_on(label, 1000)
    }

    /// A finger on what starts with `label`, `ms` long.
    fn press_on(&mut self, label: &str, ms: u64) -> String {
        self.kernel.render(&mut |_, _| {});
        let parts = self.kernel.describe();
        let part = parts
            .iter()
            .rev()
            .find(|p| p.label.starts_with(label))
            .unwrap_or_else(|| panic!("{label}: {parts:?}"));
        let (x, y) = (
            i32::from(part.area.x) + i32::from(part.area.width) / 2,
            i32::from(part.area.y) + i32::from(part.area.height) / 2,
        );
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            let event = InputEvent::Pointer(PointerEvent::new(
                0,
                phase,
                paperpaw_sdk::Point::new(x, y),
                PointerTool::Finger,
                1000,
            ));
            self.now += 20;
            let effects = self.kernel.handle(Instant(self.now), event);
            if phase == PointerPhase::Up {
                self.settle(effects);
            } else {
                // Held: what a long press does happens meanwhile (no
                // waiting past it, which would hold a tap too long).
                self.now += ms;
                self.kernel.tick(Instant(self.now));
            }
        }
        self.kernel
            .active_screen()
            .map(|id| id.to_string())
            .unwrap_or_default()
    }

    /// The screen the book at `path` opens in, as a shell opens it.
    fn book(&mut self, path: &str) -> String {
        self.now += 100;
        let effects = self.kernel.open_book(Instant(self.now), path);
        self.settle(effects);
        self.kernel
            .active_screen()
            .map(|id| id.to_string())
            .unwrap_or_default()
    }

    /// The screen a book opens in, opened by `action`.
    fn opens(&mut self, action: &'static str) -> String {
        self.now += 100;
        let effects = self
            .kernel
            .run_action(Instant(self.now), Id::new_static(action));
        self.settle(effects);
        self.kernel
            .active_screen()
            .map(|id| id.to_string())
            .unwrap_or_default()
    }
}

fn books() -> MemoryStorage {
    MemoryStorage::new()
        .with(TEXT, "Some notes.")
        .with(IN_FOLDER, "A story.")
        .with(
            EPUB,
            &include_bytes!("../../../samples/library/books/alice-in-wonderland.epub")[..],
        )
}

#[test]
fn books_open_in_the_shipped_reader_until_another_is_chosen() {
    let mut app = App::new(books(), "");
    assert_eq!(app.opens("plain.text"), SHIPPED);
    assert_eq!(app.opens("plain.epub"), SHIPPED);
}

#[test]
fn a_format_opens_with_its_reading_app() {
    let mut app = App::new(books(), "reading-app.format-txt.txt = plain.app\n");
    assert_eq!(app.opens("plain.text"), "plain.screen");
    // EPUB has its own: as the default.
    assert_eq!(app.opens("plain.epub"), SHIPPED);
}

#[test]
fn the_default_reading_app_opens_what_it_reads() {
    let mut app = App::new(books(), "reading-app = plain.app\n");
    assert_eq!(app.opens("plain.text"), "plain.screen");
    // It does not read EPUB: the shipped reader does.
    assert_eq!(app.opens("plain.epub"), SHIPPED);
}

#[test]
fn a_book_and_a_folder_keep_their_reading_app() {
    let storage = books();
    readers::remember(&storage, Choice::Folder("books/plain"), Some(&PLAIN)).unwrap();
    let mut app = App::new(storage, "");
    assert_eq!(app.opens("plain.folder"), "plain.screen");
    assert_eq!(app.opens("plain.text"), SHIPPED);
    // The book's own goes before its folder's.
    let shipped = Id::new_static("reader.app");
    readers::remember(
        app.kernel.storage(),
        Choice::Book(IN_FOLDER),
        Some(&shipped),
    )
    .unwrap();
    assert_eq!(app.opens("plain.folder"), SHIPPED);
    readers::remember(app.kernel.storage(), Choice::Book(TEXT), Some(&PLAIN)).unwrap();
    assert_eq!(app.opens("plain.text"), "plain.screen");
}

#[test]
fn the_settings_offer_the_readers_of_each_format() {
    let app = App::new(books(), "");
    let settings = app.kernel.settings();
    let choices = |id: &str| {
        let spec = settings.spec(id).unwrap_or_else(|| panic!("no {id}"));
        match &spec.kind {
            paperpaw_sdk::SettingKind::Choice(choices) => choices.len(),
            _ => 0,
        }
    };
    // Text: the default, the shipped reader, the plain one.
    assert_eq!(choices("core.reading-app.format-txt.txt"), 3);
    // EPUB: only the shipped reader reads it, nothing to choose: no
    // setting at all.
    assert!(settings.spec("core.reading-app.format-epub.epub").is_none());
    assert_eq!(choices("core.reading-app"), 3);
}

/// The options of the list of reading apps shown, and the focused one.
fn listed(app: &mut App) -> (Vec<String>, Option<String>) {
    app.kernel.render(&mut |_, _| {});
    let parts = app.kernel.describe();
    let rows = parts
        .iter()
        .filter(|p| p.role == paperpaw_sdk::semantics::Role::Row)
        .map(|p| p.label.clone())
        .collect();
    let focused = parts.iter().find(|p| p.focused).map(|p| p.label.clone());
    (rows, focused)
}

#[test]
fn open_with_lists_the_readers_of_the_book_and_keeps_the_one_chosen() {
    let mut app = App::new(books(), "");
    assert_eq!(app.opens("plain.ask-book"), "core.choice-dialog");
    let (rows, _) = listed(&mut app);
    assert_eq!(
        rows,
        ["PaperPaw reader", "Plain text reader", "Standard"],
        "{rows:?}"
    );
    // With the buttons: the second, chosen.
    assert_eq!(app.opens("core.down"), "core.choice-dialog");
    assert_eq!(app.opens("core.confirm"), "plain.screen");
    // Kept: it opens there again.
    assert_eq!(app.opens("plain.text"), "plain.screen");
}

#[test]
fn by_touch_a_reading_app_is_tapped_in_the_list_and_in_the_settings() {
    let mut app = App::on(BOOX, books(), "", true);
    assert_eq!(app.opens("plain.ask-book"), "core.choice-dialog");
    assert_eq!(app.tap("Plain text reader"), "plain.screen");
    assert_eq!(app.opens("plain.text"), "plain.screen");
    // The default one, in the reading settings.
    let mut app = App::on(BOOX, books(), "", true);
    app.now += 100;
    let params = paperpaw_sdk::Params::new().with(
        paperpaw_sdk::builtin::params::ID,
        paperpaw_sdk::builtin::groups::READING_BEHAVIOUR.as_str(),
    );
    let effects = app.kernel.open_slot(
        Instant(app.now),
        paperpaw_sdk::builtin::slots::SETTINGS,
        params,
    );
    app.settle(effects);
    for _ in 0..10 {
        app.kernel.render(&mut |_, _| {});
        if app
            .kernel
            .describe()
            .iter()
            .any(|p| p.label.starts_with("Reading app"))
        {
            break;
        }
        app.opens("core.next-page");
    }
    app.tap("Reading app");
    app.tap("Plain text reader");
    assert_eq!(app.opens("plain.text"), "plain.screen");
}

#[test]
fn a_folders_reading_app_is_chosen_from_every_reader() {
    let mut app = App::new(books(), "");
    app.opens("plain.ask-folder");
    let (rows, _) = listed(&mut app);
    assert_eq!(rows.len(), 3, "{rows:?}");
    app.opens("core.down");
    app.opens("core.confirm");
    assert_eq!(app.opens("plain.folder"), "plain.screen");
    // Standard forgets it again.
    app.opens("plain.ask-folder");
    app.opens("core.down");
    app.opens("core.down");
    app.opens("core.confirm");
    assert_eq!(app.opens("plain.folder"), SHIPPED);
}

/// The screen as a picture (PGM, gray), for a look before and after.
fn shot(app: &mut App, dir: &std::path::Path, name: &str) {
    let display = app.kernel.device().display;
    let (w, h) = (usize::from(display.width), usize::from(display.height));
    let mut gray = vec![255u8; w * h];
    // All of it, also what was drawn before (once what waits is drawn).
    app.kernel.render(&mut |_, _| {});
    app.kernel.rerender(&mut |y, view| {
        for row in 0..view.height {
            for x in 0..view.width {
                let at = usize::from(y + row) * w + usize::from(x);
                if let Some(pixel) = gray.get_mut(at) {
                    *pixel = view.pixel(x, row).luma();
                }
            }
        }
    });
    let mut pgm = format!("P5\n{w} {h}\n255\n").into_bytes();
    pgm.extend_from_slice(&gray);
    std::fs::write(dir.join(format!("{name}.pgm")), pgm).unwrap();
}

/// Focuses the row that starts with `name`, down from where it is.
fn focus_on(app: &mut App, name: &str) {
    for _ in 0..40 {
        app.kernel.render(&mut |_, _| {});
        // The topmost focus: an overlay's, not the screen's below it.
        let focused = app.kernel.describe().into_iter().rfind(|p| p.focused);
        if focused.is_some_and(|p| p.label.starts_with(name)) {
            return;
        }
        app.opens("core.down");
    }
    panic!("no row {name}");
}

/// The file manager's options for a book and a folder, and the list of
/// reading apps, as pictures in `PAPERPAW_SHOTS` (a folder): without a
/// second reading app (as shipped) and with one.
#[test]
#[ignore = "pictures for a look, run by hand"]
fn shots() {
    let Some(dir) = std::env::var_os("PAPERPAW_SHOTS") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (plain, when) in [(false, "before"), (true, "after")] {
        let mut app = App::with(books(), "", plain);
        app.opens("file-manager.open");
        focus_on(&mut app, "books");
        app.opens("core.confirm");
        focus_on(&mut app, "notes.txt");
        app.opens("core.menu");
        shot(&mut app, &dir, &format!("x4-book-options-{when}"));
        app.opens("core.back");
        focus_on(&mut app, "plain");
        app.opens("core.menu");
        shot(&mut app, &dir, &format!("x4-folder-options-{when}"));
        if plain {
            app.opens("core.back");
            focus_on(&mut app, "notes.txt");
            app.opens("core.menu");
            focus_on(&mut app, "Open with");
            app.opens("core.confirm");
            shot(&mut app, &dir, "x4-open-with-after");
        }
        // The reading settings at their part for reading apps.
        let mut app = App::with(books(), "", plain);
        app.now += 100;
        let params = paperpaw_sdk::Params::new().with(
            paperpaw_sdk::builtin::params::ID,
            paperpaw_sdk::builtin::groups::READING_BEHAVIOUR.as_str(),
        );
        let effects = app.kernel.open_slot(
            Instant(app.now),
            paperpaw_sdk::builtin::slots::SETTINGS,
            params,
        );
        app.settle(effects);
        // With two reading apps, their part; before, the last page.
        match plain {
            true => focus_on(&mut app, "Reading app"),
            false => focus_on(&mut app, "Reset to defaults"),
        }
        shot(&mut app, &dir, &format!("x4-reading-settings-{when}"));

        // The same by touch: rows held for their options, tapped.
        let mut app = App::on(BOOX, books(), "", plain);
        app.opens("file-manager.open");
        app.tap("books");
        app.long_press("notes.txt");
        shot(&mut app, &dir, &format!("boox-book-options-{when}"));
        app.opens("core.back");
        app.long_press("plain");
        shot(&mut app, &dir, &format!("boox-folder-options-{when}"));
        if plain {
            app.opens("core.back");
            app.long_press("notes.txt");
            app.tap("Open with");
            shot(&mut app, &dir, "boox-open-with-after");
        }
        let mut app = App::on(BOOX, books(), "", plain);
        app.now += 100;
        let params = paperpaw_sdk::Params::new().with(
            paperpaw_sdk::builtin::params::ID,
            paperpaw_sdk::builtin::groups::READING_BEHAVIOUR.as_str(),
        );
        let effects = app.kernel.open_slot(
            Instant(app.now),
            paperpaw_sdk::builtin::slots::SETTINGS,
            params,
        );
        app.settle(effects);
        let wanted = if plain {
            "Reading app"
        } else {
            "Reset to defaults"
        };
        for _ in 0..10 {
            app.kernel.render(&mut |_, _| {});
            if app
                .kernel
                .describe()
                .iter()
                .any(|p| p.label.starts_with(wanted))
            {
                break;
            }
            app.opens("core.next-page");
        }
        shot(&mut app, &dir, &format!("boox-reading-settings-{when}"));
    }
}

/// The screens open in the reader's slot (one of the reading apps').
fn reading(app: &App) -> Vec<String> {
    app.kernel
        .screens()
        .map(ToString::to_string)
        .filter(|s| s == SHIPPED || s == "plain.screen" || s == "picky.screen")
        .collect()
}

#[test]
fn one_reading_app_is_open_at_a_time() {
    let mut app = App::new(books(), "");
    app.opens("plain.ask-book");
    app.opens("core.down");
    assert_eq!(app.opens("core.confirm"), "plain.screen");
    // Another book, in the shipped reader: the plain one closes.
    assert_eq!(app.opens("plain.epub"), SHIPPED);
    assert_eq!(reading(&app), [SHIPPED]);
    assert_eq!(app.opens("plain.text"), "plain.screen");
    assert_eq!(reading(&app), ["plain.screen"]);
}

#[test]
fn a_reader_that_cannot_open_a_book_passes_it_on() {
    let config = "reading-app.format-txt.txt = picky.app\n";
    let mut app = App::of(DEVICE, books(), config, |b| b.plugin(Plain).plugin(Picky));
    // Picky first, which passes it on: by the order the next is the
    // default (none), then the shipped reader.
    assert_eq!(app.opens("plain.text"), SHIPPED);
    assert_eq!(reading(&app), [SHIPPED]);
}

#[test]
fn a_reading_app_that_is_gone_is_passed_over_silently() {
    let config = "reading-app = gone.app\nreading-app.format-txt.txt = gone.app\n";
    let mut app = App::with(books(), config, false);
    let warnings = app.kernel.take_warnings();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(app.book(TEXT), SHIPPED);
    // With the plain reader: the per-format choice of the gone one, kept,
    // is passed over too.
    let mut app = App::with(books(), config, true);
    assert!(app.kernel.take_warnings().is_empty());
    assert_eq!(app.opens("plain.text"), SHIPPED);
}

#[test]
fn a_book_is_the_same_with_slashes_at_its_ends() {
    let storage = books();
    readers::remember(&storage, Choice::Book("/books/notes.txt"), Some(&PLAIN)).unwrap();
    let mut app = App::new(storage, "");
    assert_eq!(app.opens("plain.text"), "plain.screen");
}

#[test]
fn readers_read_in_screens_of_their_own() {
    // Borrower's tries are refused (it asserts so).
    let mut app = App::of(DEVICE, books(), "", |b| b.plugin(Borrower));
    assert_eq!(app.book(TEXT), SHIPPED);
}

/// The library's sheet of the book `notes.txt`: held on touch screens,
/// the menu button with buttons only.
fn book_sheet(app: &mut App) {
    app.opens("core.library");
    if app.kernel.device().input.touch {
        app.long_press("notes");
    } else {
        focus_on(app, "notes");
        app.opens("core.menu");
    }
}

/// The book menu (the library's sheet of a book) as pictures in
/// `PAPERPAW_SHOTS`, with a second reading app installed: `when` names
/// them (before or after a change).
#[test]
#[ignore = "pictures for a look, run by hand"]
fn book_menu_shots() {
    let Some(dir) = std::env::var_os("PAPERPAW_SHOTS") else {
        return;
    };
    let when = std::env::var("PAPERPAW_WHEN").unwrap_or_else(|_| String::from("now"));
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (device, name) in [(DEVICE, "x4"), (BOOX, "boox")] {
        let mut app = App::on(device, books(), "", true);
        book_sheet(&mut app);
        shot(&mut app, &dir, &format!("{name}-book-menu-{when}"));
        // The list of *Open with …*, where there is one.
        if rows_shown(&mut app)
            .iter()
            .any(|r| r.starts_with("Open with"))
        {
            pick(&mut app, "Open with");
            shot(&mut app, &dir, &format!("{name}-open-with-{when}"));
        }
    }
}

/// Chooses the row or entry `label` of the list or sheet shown: with the
/// buttons, down to it and confirm; by touch, a tap on it.
fn pick(app: &mut App, label: &str) -> String {
    if app.kernel.device().input.touch {
        return app.tap(label);
    }
    focus_on(app, label);
    app.opens("core.confirm")
}

/// The rows of the list or sheet on top.
fn rows_shown(app: &mut App) -> Vec<String> {
    app.kernel.render(&mut |_, _| {});
    app.kernel
        .describe()
        .into_iter()
        .filter(|p| p.role == paperpaw_sdk::semantics::Role::Row)
        .map(|p| p.label)
        .collect()
}

#[test]
fn the_book_menu_offers_open_with_where_there_is_a_choice() {
    for device in [DEVICE, BOOX] {
        let mut app = App::on(device, books(), "", true);
        book_sheet(&mut app);
        assert!(
            rows_shown(&mut app)
                .iter()
                .any(|r| r.starts_with("Open with")),
            "{:?}",
            rows_shown(&mut app)
        );
        // As shipped: one reading app, no other way: nothing to choose.
        let mut app = App::on(device, books(), "", false);
        book_sheet(&mut app);
        assert!(
            !rows_shown(&mut app)
                .iter()
                .any(|r| r.starts_with("Open with"))
        );
    }
}

#[test]
fn open_with_opens_the_book_this_time_and_keeps_only_when_asked() {
    for device in [DEVICE, BOOX] {
        let mut app = App::on(device, books(), "", true);
        book_sheet(&mut app);
        assert_eq!(pick(&mut app, "Open with"), "core.choice-dialog");
        let rows = rows_shown(&mut app);
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        assert_eq!(
            &rows[rows.len() - 4..],
            [
                "PaperPaw reader",
                "Plain text reader",
                "Send to a computer",
                "Always open it with …"
            ],
            "{rows:?}"
        );
        assert_eq!(pick(&mut app, "Plain text reader"), "plain.screen");
        // Not kept: the next time as before.
        assert_eq!(app.opens("plain.text"), SHIPPED);
        // Kept when asked to.
        book_sheet(&mut app);
        pick(&mut app, "Open with");
        assert_eq!(pick(&mut app, "Always open it with"), "core.choice-dialog");
        assert_eq!(pick(&mut app, "Plain text reader"), "plain.screen");
        assert_eq!(app.opens("plain.text"), "plain.screen");
    }
}

#[test]
fn other_ways_to_open_a_book_get_the_book() {
    SENT.with(|sent| sent.borrow_mut().clear());
    let mut app = App::new(books(), "");
    book_sheet(&mut app);
    pick(&mut app, "Open with");
    pick(&mut app, "Send to a computer");
    assert_eq!(SENT.with(|sent| sent.borrow().clone()), [TEXT]);
}
