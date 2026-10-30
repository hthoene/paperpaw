//! A reader on a real kernel for the integration tests: the text and EPUB
//! formats, the default fonts, a plugin that hears what the reader tells
//! others (events, slots it opens), a keyboard that is typed on by the
//! test, and helpers to press buttons, touch the page, let time pass and
//! look at what is on the screen.

#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{Io, Kernel, SpeechCall};
use paperpaw_platform::{InputEvent, PointerEvent, PointerPhase, PointerTool, Rotation};
use paperpaw_sdk::builtin::{events, params, slots};
use paperpaw_sdk::{
    Canvas, Cx, DeviceInfo, DirEntry, DisplayInfo, Env, Handled, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, Params, PixelFormat, Plugin, Point, Priority, RegisterError,
    Registrar, Screen, ScreenSpec, Storage, StorageError, Version,
};

/// A device with buttons and no touch screen, as the X4.
pub const BUTTONS: DeviceInfo = DeviceInfo::new(
    "test-buttons",
    "Test buttons",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Large,
);

/// A touch screen of the same size, without buttons.
pub const TOUCH: DeviceInfo = BUTTONS
    .with_id("test-touch")
    .with_name("Test touch")
    .with_input(InputCaps::new(false, true, false, &[]));

/// A storage the test looks into while kernels have it.
#[derive(Clone, Default)]
pub struct Shared(pub Rc<MemoryStorage>);

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
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
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

impl Shared {
    /// The text of file `path`, if there is one.
    pub fn text(&self, path: &str) -> Option<String> {
        self.0
            .get(path)
            .map(|data| String::from_utf8_lossy(&data).into_owned())
    }
}

/// What the reader told other plugins: events by name with what they
/// carry, and the slots it opened with their parameters.
#[derive(Default)]
pub struct Heard {
    pub events: Vec<(&'static str, Params)>,
    pub slots: Vec<(&'static str, Params)>,
}

/// Hears the reader: its events, and the slots it opens (the dictionary,
/// the keyboard, the translator, book information), as screens that show
/// nothing.
pub struct Recorder(pub Rc<RefCell<Heard>>);

/// The parts of the recorder's own place of status items: its item at
/// the left.
static PLACE_PARTS: [paperpaw_sdk::SettingSpec; 3] = [
    part("recorder.left", "recorder.status"),
    part("recorder.center", ""),
    part("recorder.right", ""),
];

const fn part(id: &'static str, default: &'static str) -> paperpaw_sdk::SettingSpec {
    paperpaw_sdk::SettingSpec::new(
        paperpaw_sdk::Id::new_static(id),
        "recorder",
        paperpaw_sdk::SettingKind::List,
        paperpaw_sdk::SettingValue::Text(std::borrow::Cow::Borrowed(default)),
        paperpaw_sdk::builtin::groups::OTHER,
    )
}

/// A screen that, as it shows, notes how many targets the reader's line
/// of items has in the recorder's place (`lines`, parameter `targets`).
struct LinesProbe(Rc<RefCell<Heard>>);

impl Screen for LinesProbe {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let context = Params::new();
        let area = paperpaw_sdk::Rect::new(0, 0, 480, 40);
        let frame =
            paperpaw_sdk::StatusFrame::new(area, false, 20.0, paperpaw_sdk::Color::BLACK, &context)
                .place("recorder.place");
        let targets = env
            .status_items()
            .line_targets("reader.status-line.items", &env, &frame)
            .len();
        let noted = Params::new().with("targets", targets.to_string());
        self.0.borrow_mut().slots.push(("lines", noted));
    }
}

/// A screen standing in for another plugin's: it shows nothing and closes
/// on *Back* as any screen does.
struct Stand;

impl Screen for Stand {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for Recorder {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("recorder"),
            "Recorder",
            Version::new(0, 1, 0),
        )
        .description("Hears what the reader tells others.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // As `events::BOOK_OPENED` and the others name them.
        for (event, name) in [
            ("core.book-opened", "opened"),
            ("core.page-turned", "turned"),
            ("core.book-closed", "closed"),
            ("core.book-finished", "finished"),
            ("core.word-looked-up", "looked-up"),
            ("core.text-chosen", "text-chosen"),
            ("core.annotations-changed", "annotations"),
        ] {
            let heard = self.0.clone();
            let event: paperpaw_sdk::event::Event<Params> = paperpaw_sdk::event::Event::new(event);
            r.on(event, move |_, params, _| {
                heard.borrow_mut().events.push((name, params.clone()));
            })?;
        }
        for (slot, name, screen) in [
            (slots::LOOKUP, "lookup", "recorder.lookup"),
            (slots::KEYBOARD, "keyboard", "recorder.keyboard"),
            (slots::TRANSLATE, "translate", "recorder.translate"),
            (slots::BOOK_INFO, "book-info", "recorder.book-info"),
            (slots::MENU, "menu", "recorder.menu"),
        ] {
            let screen = Id::new_static(screen);
            let heard = self.0.clone();
            r.add_screen(ScreenSpec::new(
                screen.clone(),
                "recorder",
                Box::new(move |_, params| {
                    heard.borrow_mut().slots.push((name, params.clone()));
                    Box::new(Stand)
                }),
            ))?;
            r.bind_slot(slot, screen, Priority::DEFAULT)?;
        }
        // A place of status items of its own, its item at the left, and a
        // screen that asks the reader's line of items for its targets
        // there.
        r.add_settings(&PLACE_PARTS)?;
        r.add_status_place(paperpaw_sdk::status::Place::new(
            Id::new_static("recorder.place"),
            "recorder",
            [
                Id::new_static("recorder.left"),
                Id::new_static("recorder.center"),
                Id::new_static("recorder.right"),
            ],
        ))?;
        let heard = self.0.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("recorder.lines"),
            "recorder",
            Box::new(move |_, _| Box::new(LinesProbe(heard.clone()))),
        ))?;
        // A status item that opens the book's information when tapped.
        r.add_status_item(
            paperpaw_sdk::StatusItemSpec::new(
                Id::new_static("recorder.status"),
                "recorder",
                Box::new(|_, _| Some(paperpaw_sdk::StatusValue::Text("Tapped item".into()))),
            )
            .tap(Id::new_static("recorder.info")),
        )?;
        r.add_action(
            paperpaw_sdk::ActionSpec::new(Id::new_static("recorder.info"), "recorder").handler(
                |_: &Env<'_>, _, out: &mut paperpaw_sdk::Commands| {
                    let from = Params::new().with("from", "status");
                    out.push(paperpaw_sdk::Command::OpenSlot(slots::BOOK_INFO, from));
                },
            ),
        )?;
        Ok(())
    }
}

/// A line of text on the screen: its rows (top, bottom) and its words'
/// columns (left, right).
pub type Line = ((usize, usize), Vec<(usize, usize)>);

/// A reader on a kernel, with a book.
pub struct Reading {
    pub kernel: Kernel,
    pub storage: Shared,
    pub heard: Rc<RefCell<Heard>>,
    pub now: u64,
    /// The book read.
    pub book: String,
    /// The screen as last rendered, by rows of luma.
    pub image: Vec<Vec<u8>>,
}

/// The settings every reading starts with: English, and no status line
/// at the top.
const CONFIG: &str = "[core]\nlanguage = en\n";

impl Reading {
    /// A kernel on `device` over `storage`, with the book `data` at `path`
    /// (unless one is there already), and the settings of `config`.
    pub fn new(device: DeviceInfo, storage: Shared, path: &str, data: &[u8], config: &str) -> Self {
        if !storage.exists(path) {
            if let Some((dir, _)) = path.rsplit_once('/') {
                storage.create_dir(dir).unwrap();
            }
            storage.write(path, data).unwrap();
        }
        Self::on(device, storage, path, config, false)
    }

    /// The same, where the device can speak.
    pub fn speaking(device: DeviceInfo, storage: Shared, path: &str, data: &[u8]) -> Self {
        if !storage.exists(path) {
            storage.create_dir("books").unwrap();
            storage.write(path, data).unwrap();
        }
        Self::on(device, storage, path, "", true)
    }

    /// A kernel over `storage` as it is (the book may be missing).
    pub fn on(device: DeviceInfo, storage: Shared, path: &str, config: &str, speech: bool) -> Self {
        let heard = Rc::new(RefCell::new(Heard::default()));
        let kernel = Kernel::builder(device)
            .storage(storage.clone())
            .user_config(format!("{CONFIG}{config}"))
            .speech(speech)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_format_txt::TxtFormat)
            .plugin(paperpaw_plugin_format_epub::EpubFormat)
            .plugin(paperpaw_plugin_reader::Reader)
            .plugin(Recorder(heard.clone()))
            .build()
            .unwrap();
        let mut reading = Self {
            kernel,
            storage,
            heard,
            now: 0,
            book: path.to_string(),
            image: Vec::new(),
        };
        let effects = reading.kernel.start(Instant(0));
        reading.settle(effects);
        reading
    }

    /// Lets the kernel's timers run (the page index, a first opening)
    /// until nothing is due within a second.
    pub fn settle(&mut self, mut effects: paperpaw_core::Effects) {
        for _ in 0..20_000 {
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

    /// Opens the book in the reader.
    pub fn open(&mut self) {
        self.now += 10;
        let params = Params::new().with(params::PATH, self.book.clone());
        let effects = self
            .kernel
            .open_slot(Instant(self.now), slots::READER, params);
        self.settle(effects);
        assert_eq!(self.screen(), "reader.screen");
    }

    /// The screen shown.
    pub fn screen(&self) -> String {
        self.kernel
            .active_screen()
            .map(|id| id.as_str().to_string())
            .unwrap_or_default()
    }

    /// Runs `action` as a button bound to it would, after the time
    /// positions are written at most this often.
    pub fn run(&mut self, action: Id) {
        self.now += 6000;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    /// The same, a moment after the last input.
    pub fn press(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    /// Sends `event` with `params`, as a plugin would.
    pub fn send(&mut self, event: &Id, params: Params) {
        self.now += 100;
        let effects = self
            .kernel
            .send_event(Instant(self.now), event.clone(), params);
        self.settle(effects);
    }

    /// Advances the clock to `at` (milliseconds), ticking the kernel at
    /// every time it asked for on the way.
    pub fn pass_until(&mut self, at: u64) {
        for _ in 0..100_000 {
            let effects = self.kernel.tick(Instant(self.now));
            match effects.wake_at() {
                Some(next) if next.0 <= at && next.0 > self.now => self.now = next.0,
                Some(next) if next.0 <= self.now => self.now += 1,
                _ => {
                    self.now = at;
                    self.kernel.tick(Instant(self.now));
                    return;
                }
            }
        }
    }

    /// The position the reader kept: section and offset; the start
    /// while it kept none.
    pub fn kept(&mut self) -> (u32, u32) {
        self.now += 10;
        let effects = self.kernel.suspend(Instant(self.now));
        self.settle(effects);
        let Some(text) = self.storage.text(&self.progress_path()) else {
            return (0, 0);
        };
        let value = |key: &str| -> u32 {
            let line = text.lines().find(|l| l.starts_with(key)).unwrap();
            line.rsplit(' ').next().unwrap().parse().unwrap()
        };
        (value("section"), value("offset"))
    }

    /// Where the position of the book is kept.
    pub fn progress_path(&self) -> String {
        paperpaw_sdk::positions::file(&self.book)
    }

    /// Where the reader said the book is, last: opened or turned to.
    pub fn shown(&self) -> events::Reading {
        let heard = self.heard.borrow();
        let found = heard
            .events
            .iter()
            .rev()
            .find(|(n, _)| *n == "opened" || *n == "turned");
        let (_, params) = found.expect("the reader said nothing yet");
        reading_of(params)
    }

    /// The pages turned so far.
    pub fn turned(&self) -> usize {
        self.events().iter().filter(|e| **e == "turned").count()
    }

    /// The names of the events heard, in order.
    pub fn events(&self) -> Vec<&'static str> {
        self.heard.borrow().events.iter().map(|(n, _)| *n).collect()
    }

    /// What the last event `name` carried.
    pub fn last_event(&self, name: &str) -> Option<Params> {
        let heard = self.heard.borrow();
        let found = heard.events.iter().rev().find(|(n, _)| *n == name);
        found.map(|(_, p)| p.clone())
    }

    /// The parameters slot `name` was opened with last.
    pub fn last_slot(&self, name: &str) -> Option<Params> {
        let heard = self.heard.borrow();
        let found = heard.slots.iter().rev().find(|(n, _)| *n == name);
        found.map(|(_, p)| p.clone())
    }

    /// Types `text` on the keyboard asked for last and confirms it: the
    /// keyboard closes and its question's reply is sent.
    pub fn type_text(&mut self, text: &str) {
        let asked = self
            .last_slot("keyboard")
            .expect("no keyboard was asked for");
        assert_eq!(self.screen(), "recorder.keyboard");
        let reply = asked.get(params::REPLY).unwrap().to_string();
        let mut answer = Params::new();
        for (key, value) in asked.iter() {
            if ![params::TITLE, params::REPLY, params::TEXT, params::MODE].contains(&key) {
                answer.set(key.to_string(), value);
            }
        }
        answer.set(params::TEXT, text);
        self.press(paperpaw_sdk::builtin::actions::BACK);
        self.send(&Id::parse(&reply).unwrap(), answer);
    }

    /// Renders what changed and keeps the whole screen in [`Reading::image`].
    pub fn render(&mut self) {
        let display = self.kernel.device().display;
        if self.image.is_empty() {
            self.image = vec![vec![255; usize::from(display.width)]; usize::from(display.height)];
        }
        let image = &mut self.image;
        self.kernel.render(&mut |y, view| {
            for row in 0..view.height {
                let Some(line) = image.get_mut(usize::from(y + row)) else {
                    continue;
                };
                for x in 0..view.width {
                    line[usize::from(x)] = view.pixel(x, row).luma();
                }
            }
        });
    }

    /// The whole screen drawn anew (not only what changed).
    pub fn snapshot(&mut self) -> Vec<Vec<u8>> {
        // Drawn in full once more, as after opening a screen.
        self.render();
        self.image.clone()
    }

    /// The text lines of the page: rows with ink between the top and the
    /// bottom margin, as `(top, bottom)` pixel rows, and in each the
    /// words as `(left, right)` columns (ink apart by more than `gap`).
    pub fn lines(&mut self, gap: usize) -> Vec<Line> {
        self.render();
        let image = &self.image;
        let height = image.len();
        // The status line at the bottom is left out: below 92 % of the
        // screen.
        let bottom = height * 92 / 100;
        let inked = |y: usize| image[y].iter().any(|&l| l < 128);
        let mut lines = Vec::new();
        let mut y = 0;
        while y < bottom {
            if !inked(y) {
                y += 1;
                continue;
            }
            let top = y;
            while y < bottom && inked(y) {
                y += 1;
            }
            let rows = top..y;
            let width = image[0].len();
            let column = |x: usize| rows.clone().any(|r| image[r][x] < 128);
            let mut words = Vec::new();
            let mut x = 0;
            while x < width {
                if !column(x) {
                    x += 1;
                    continue;
                }
                let left = x;
                let mut right = x;
                while x < width {
                    if column(x) {
                        right = x;
                        x += 1;
                    } else if (x..(x + gap + 1).min(width)).any(&column) {
                        x += 1;
                    } else {
                        break;
                    }
                }
                words.push((left, right));
            }
            lines.push(((top, y), words));
        }
        lines
    }

    /// The middle of each word on the page, in reading order (see
    /// [`Reading::lines`]).
    pub fn words(&mut self) -> Vec<Point> {
        let mut words = Vec::new();
        for ((top, bottom), line) in self.lines(4) {
            for (left, right) in line {
                words.push(Point::new(
                    ((left + right) / 2) as i32,
                    ((top + bottom) / 2) as i32,
                ));
            }
        }
        words
    }

    /// The part of the screen that says `label`, if any.
    pub fn part(&mut self, label: &str) -> Option<paperpaw_sdk::semantics::Part> {
        self.render();
        let parts = self.kernel.describe();
        paperpaw_sdk::semantics::find(&parts, label).cloned()
    }

    /// A tap on the part of the screen that says `label`.
    pub fn tap_on(&mut self, label: &str) {
        let area = self
            .part(label)
            .unwrap_or_else(|| panic!("nothing says {label}:\n{}", self.described()))
            .area;
        self.tap(Point::new(
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        ));
    }

    /// A finger at `point`, coming down, moving or lifted.
    pub fn finger(&mut self, phase: PointerPhase, point: Point) {
        self.now += 30;
        let event = InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            point,
            PointerTool::Finger,
            1000,
        ));
        let effects = self.kernel.handle(Instant(self.now), event);
        if phase == PointerPhase::Up {
            self.settle(effects);
        }
    }

    pub fn tap(&mut self, point: Point) {
        self.now += 1000;
        self.finger(PointerPhase::Down, point);
        self.finger(PointerPhase::Up, point);
    }

    /// A finger held at `point` until a long press is recognized, then
    /// lifted.
    pub fn long_press(&mut self, point: Point) {
        self.now += 1000;
        self.finger(PointerPhase::Down, point);
        self.now += 800;
        let effects = self.kernel.tick(Instant(self.now));
        let _ = effects;
        self.finger(PointerPhase::Up, point);
    }

    /// The speech the kernel asks the shell for: texts to say (with their
    /// ids), and whether it asked to stop.
    pub fn speech(&mut self) -> (Vec<(u32, String)>, bool) {
        let mut said = Vec::new();
        let mut stop = false;
        for io in self.kernel.take_io() {
            match io {
                Io::Speech(SpeechCall::Say { id, text, .. }) => said.push((id, text)),
                Io::Speech(SpeechCall::Stop) => stop = true,
                _ => {}
            }
        }
        (said, stop)
    }

    /// Text copied to the clipboard since asked last.
    pub fn copied(&mut self) -> Vec<String> {
        let mut copied = Vec::new();
        for io in self.kernel.take_io() {
            if let Io::Copy(text) = io {
                copied.push(text);
            }
        }
        copied
    }

    /// The shell says text `id` was spoken.
    pub fn spoken(&mut self, id: u32) {
        self.now += 100;
        let effects = self.kernel.speech_done(Instant(self.now), id, true);
        self.settle(effects);
    }

    /// What the screens shown describe: a line per part, its label.
    pub fn described(&mut self) -> String {
        self.render();
        let mut text = String::new();
        for part in self.kernel.describe() {
            text.push_str(&part.label);
            text.push('\n');
        }
        text
    }
}

/// What a reading event carries.
pub fn reading_of(params: &Params) -> events::Reading {
    <events::Reading as paperpaw_sdk::event::EventData>::from_params(params)
        .expect("a reading event")
}

/// The data folder of the book at `path`, as the reader names it.
pub fn paperpaw_plugin_reader_dir(path: &str) -> String {
    format!(
        ".paperpaw/plugins/reader/books/{:016x}",
        paperpaw_sdk::digest::fnv1a64(path)
    )
}

/// An EPUB with `chapters` (file name and the body's HTML) in this
/// order, and a table of contents of `toc` (title and link).
pub fn epub(chapters: &[(&str, String)], toc: &[(&str, &str)]) -> Vec<u8> {
    epub_with(chapters, toc, "")
}

/// The same, with `spine` the spine's attributes (e.g. its pages going
/// right to left).
pub fn epub_with(chapters: &[(&str, String)], toc: &[(&str, &str)], spine_attrs: &str) -> Vec<u8> {
    use paperpaw_zip::testing::ZipBuilder;
    let container = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
    let mut manifest = String::from(
        r#"<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#,
    );
    let mut spine = String::new();
    for (i, (name, _)) in chapters.iter().enumerate() {
        manifest.push_str(&format!(
            r#"<item id="c{i}" href="{name}" media-type="application/xhtml+xml"/>"#
        ));
        spine.push_str(&format!(r#"<itemref idref="c{i}"/>"#));
    }
    let opf = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title><dc:creator>A. Writer</dc:creator><dc:language>en</dc:language>
  </metadata>
  <manifest>{manifest}</manifest>
  <spine{spine_attrs}>{spine}</spine>
</package>"#
    );
    let entries: String = toc
        .iter()
        .map(|(title, href)| format!(r#"<li><a href="{href}">{title}</a></li>"#))
        .collect();
    let nav = format!(
        r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body><nav epub:type="toc"><ol>{entries}</ol></nav></body></html>"#
    );
    let mut zip = ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated("META-INF/container.xml", container)
        .deflated("OEBPS/content.opf", opf)
        .deflated("OEBPS/nav.xhtml", nav);
    for (name, body) in chapters {
        let page = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>{name}</title></head><body>{body}</body></html>"#
        );
        zip = zip.deflated(&format!("OEBPS/{name}"), page);
    }
    zip.build()
}

/// Paragraphs of HTML enough for `pages` pages or so.
pub fn filler(tag: &str, pages: usize) -> String {
    let mut html = String::new();
    for n in 0..pages * 6 {
        html.push_str(&format!(
            "<p>{tag} paragraph {n} goes on for a while with words to fill a line or two.</p>"
        ));
    }
    html
}

/// A plain text book of `paragraphs` paragraphs of numbered words: the
/// first word of paragraph `n` is `p{n}`, then `word{n}x{i}`.
pub fn numbered_book(paragraphs: usize, words: usize) -> String {
    let mut text = String::new();
    for n in 0..paragraphs {
        text.push_str(&format!("p{n}"));
        for i in 1..words {
            text.push_str(&format!(" word{n}x{i}"));
        }
        text.push_str(".\n\n");
    }
    text
}
