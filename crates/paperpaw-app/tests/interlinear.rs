//! An interlinear reading on the shipped reader: a text filter asks for
//! room above every line (`Block::leading_above`), the page layout gives
//! it (each line lower by it, the page ending sooner, each line's
//! `leading` saying how much), and a layer that asks for the page's words
//! gets them with their boxes, the words the reader has for the page,
//! with the room free above them. Where no layer asks, none are measured.
//! A filter inserts a block after each paragraph (a parallel text): it
//! shows, and the book's words, pages and places stay the book's. A tap
//! on an item the layer drew, or Select on it chosen with the buttons,
//! comes to the layer's plugin; other taps stay the reader's.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat,
    PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_sdk::builtin::{events, params, slots};
use paperpaw_sdk::document::{Block, BlockKind, Locator};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::filters::TextFilterSpec;
use paperpaw_sdk::layers::{LayerItem, PageLayerSpec};
use paperpaw_sdk::reading::PageWords;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::{
    ActionSpec, Commands, Env, Id, Manifest, MemoryStorage, Params, Plugin, Point, Rect,
    RegisterError, Registrar, SDK_VERSION, SettingSpec,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-both",
    "Test both",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        true,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

const BOOK: &str = "books/alice-in-wonderland.txt";
/// Whether the room above the lines is asked for.
const ON: Setting<bool> = Setting::new("interlinear.on");
/// Whether a translation follows each paragraph.
const PARALLEL: Setting<bool> = Setting::new("interlinear.parallel");
static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::switch(
        ON.static_str(),
        "interlinear.on",
        false,
        paperpaw_sdk::builtin::groups::READING,
    ),
    SettingSpec::switch(
        PARALLEL.static_str(),
        "interlinear.parallel",
        false,
        paperpaw_sdk::builtin::groups::READING,
    ),
];
/// What the inserted translations begin with, which no word of the book
/// has.
const SIGN: &str = "§";
/// The room asked for: 70 hundredths of an em.
const LEADING: u16 = 70;
const ASK: Id = Id::new_static("interlinear.ask");
const HEARD: Event<PageWords> = Event::new("interlinear.words");

/// What the layers were shown last, and the page's words the reader told.
#[derive(Debug, Default)]
struct Seen {
    start: Locator,
    end: Locator,
    /// Each line's box and its room above.
    lines: Vec<(Rect, u16)>,
    /// The words the layer got.
    words: Vec<(Rect, String, Locator, Locator)>,
    told: Option<PageWords>,
    /// The items tapped or chosen, with the line beside each.
    tapped: Vec<(u32, Option<Locator>)>,
}

/// Room above every line while switched on, and a layer that draws by
/// the page's words (it notes them), if it `asks` for them.
struct Interlinear {
    seen: Rc<RefCell<Seen>>,
    asks: bool,
}

impl Plugin for Interlinear {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("interlinear"), "Interlinear", SDK_VERSION)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_text_filter(TextFilterSpec::new(
            Id::new_static("interlinear.room"),
            "interlinear.room",
            Box::new(|env: &Env<'_>, block| {
                if env.settings().read(&ON) {
                    block.leading_above = Some(LEADING);
                }
                if env.settings().read(&PARALLEL) && block.kind == BlockKind::Paragraph {
                    let reversed: String = block.text.chars().rev().collect();
                    let text = format!("{SIGN} {reversed}");
                    let len = text.len() as u32;
                    block.insert_after(Block::new(BlockKind::Paragraph, 0, len, text));
                }
            }),
            Box::new(|env: &Env<'_>| {
                u64::from(env.settings().read(&ON)) + 2 * u64::from(env.settings().read(&PARALLEL))
            }),
        ))?;
        let seen = self.seen.clone();
        let layer = PageLayerSpec::new(
            Id::new_static("interlinear.words"),
            "words",
            move |_, _, page| {
                let mut seen = seen.borrow_mut();
                seen.start = page.start;
                seen.end = page.end;
                seen.lines = page.lines.iter().map(|l| (l.area, l.leading)).collect();
                seen.words = page
                    .words
                    .iter()
                    .map(|w| (w.area, w.text.clone(), w.start, w.end))
                    .collect();
            },
        );
        // The room above each line an item: tapped, it would open the
        // line's translation.
        let layer = layer.items(|_, page| {
            page.lines
                .iter()
                .enumerate()
                .filter(|(_, l)| l.leading > 0)
                .map(|(i, l)| {
                    let a = l.area;
                    let room = Rect::new(a.x, a.y - l.leading, a.width, l.leading);
                    LayerItem::new(i as u32, room).label("Translation")
                })
                .collect()
        });
        r.add_page_layer(if self.asks { layer.words() } else { layer })?;
        let seen = self.seen.clone();
        r.on(events::PAGE_LAYER_TAPPED, move |_, tapped, _| {
            if let Some(item) = tapped.item {
                seen.borrow_mut().tapped.push((item, tapped.line()));
            }
        })?;
        r.add_action(
            ActionSpec::new(ASK, "ask").handler(|_: &Env<'_>, _, out: &mut Commands| {
                out.push(paperpaw_sdk::reading::ask_page_words(&HEARD));
            }),
        )?;
        let seen = self.seen.clone();
        r.on(HEARD, move |_, words, _| {
            seen.borrow_mut().told = Some(words.clone());
        })
    }
}

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

struct App {
    kernel: Kernel,
    now: u64,
    seen: Rc<RefCell<Seen>>,
}

impl App {
    /// The book open in the reader, the room asked for if `on`; its page
    /// drawn and its words told.
    fn new(on: bool) -> Self {
        Self::with(on, true)
    }

    /// The same, the layer asking for the words if `asks`.
    fn with(on: bool, asks: bool) -> Self {
        Self::configured(on, false, asks)
    }

    /// The same, translations following each paragraph if `parallel`.
    fn configured(on: bool, parallel: bool, asks: bool) -> Self {
        let storage = MemoryStorage::new()
            .with(
                BOOK,
                include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
            )
            .with(".paperpaw/plugins/setup/done", "");
        let switch = |on: bool| if on { "on" } else { "off" };
        let config = format!(
            "[help]\nshow = false\n[core]\nlanguage = en\n[interlinear]\non = {}\nparallel = {}\n",
            switch(on),
            switch(parallel)
        );
        let builder = Kernel::builder(DEVICE).storage(storage).user_config(config);
        let seen = Rc::new(RefCell::new(Seen::default()));
        let mut kernel = paperpaw_app::install(builder, fonts())
            .plugin(Interlinear {
                seen: seen.clone(),
                asks,
            })
            .build()
            .unwrap();
        let effects = kernel.start(Instant(0));
        let mut app = Self {
            kernel,
            now: 0,
            seen,
        };
        app.settle(effects);
        app.now += 100;
        let open = Params::new().with(params::PATH, BOOK);
        let effects = app.kernel.open_slot(Instant(app.now), slots::READER, open);
        app.settle(effects);
        app.show();
        app
    }

    /// The page drawn and its words told.
    fn show(&mut self) {
        self.kernel.render(&mut |_, _| {});
        self.run(ASK);
    }

    /// `pages` pages on, drawn and told.
    fn turn(&mut self, pages: usize) {
        for _ in 0..pages {
            self.run(paperpaw_sdk::builtin::actions::NEXT_PAGE);
        }
        self.show();
    }

    fn tap(&mut self, point: Point) {
        for phase in [PointerPhase::Down, PointerPhase::Up] {
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
        self.show();
    }

    /// The starts of the words of `pages` pages from the first, as the
    /// reader tells them.
    fn word_starts(&mut self, pages: usize) -> Vec<Locator> {
        let mut starts = Vec::new();
        for page in 0..pages {
            if page > 0 {
                self.turn(1);
            }
            let told = self.seen.borrow().told.clone().unwrap();
            assert!(
                told.words.iter().all(|w| !w.text.contains(SIGN)),
                "{told:?}"
            );
            starts.extend(told.words.iter().map(|w| w.start));
        }
        starts
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
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
}

#[test]
fn room_asked_above_the_lines_lowers_them_and_ends_the_page_sooner() {
    let plain = App::new(false);
    let spaced = App::new(true);
    let (plain, spaced) = (plain.seen.borrow(), spaced.seen.borrow());
    assert_eq!(plain.start, spaced.start);
    assert!(plain.lines.len() > 5, "{:?}", plain.lines);
    assert!(plain.lines.iter().all(|&(_, leading)| leading == 0));
    // The same room above every line of the paragraphs.
    let leading = spaced.lines[0].1;
    assert!(leading > 5, "{leading} px for 70 % of an em");
    assert!(
        spaced.lines.iter().all(|&(_, l)| l == leading),
        "{:?}",
        spaced.lines
    );
    // Each line lower by it than the one before, as without it.
    let step = |lines: &[(Rect, u16)]| i32::from(lines[1].0.y) - i32::from(lines[0].0.y);
    let (plain_step, spaced_step) = (step(&plain.lines), step(&spaced.lines));
    assert!(
        (spaced_step - plain_step - i32::from(leading)).abs() <= 1,
        "{plain_step} + {leading} = {spaced_step}"
    );
    // Fewer lines fit: the page ends sooner, and the next starts there.
    assert!(spaced.lines.len() < plain.lines.len());
    assert!(spaced.end < plain.end, "{:?} {:?}", spaced.end, plain.end);
}

#[test]
fn a_layer_that_asks_gets_the_pages_words_with_the_room_free_above_them() {
    // A page of the story, past the title.
    let mut app = App::new(true);
    app.turn(3);
    let seen = app.seen.borrow();
    assert!(seen.words.len() > 20, "{:?}", seen.words);
    // The words the reader has for the page, in reading order, each in a
    // line and below its room.
    let told = seen.told.as_ref().expect("the page's words told");
    assert_eq!(told.start, seen.start);
    let starts: Vec<Locator> = seen.words.iter().map(|w| w.2).collect();
    let told_starts: Vec<Locator> = told.words.iter().map(|w| w.start).collect();
    assert_eq!(starts, told_starts);
    for (area, text, start, end) in &seen.words {
        assert!(!text.is_empty() && start < end, "{text}");
        assert!(seen.start <= *start && *end <= seen.end, "{text}");
        let (line, leading) = seen
            .lines
            .iter()
            .find(|(l, _)| l.y <= area.y && u32::from(area.y) < l.bottom())
            .unwrap_or_else(|| panic!("{text} {area:?} in no line"));
        let room_top = i32::from(line.y) - i32::from(*leading);
        assert!(room_top >= 0, "{text}: its room on the page");
        // No other line's box reaches into the room above the word.
        for (other, _) in &seen.lines {
            if other != line {
                assert!(
                    other.bottom() as i32 <= room_top + 1
                        || i32::from(other.y) >= line.bottom() as i32,
                    "{text}: {other:?} in the room above {line:?}"
                );
            }
        }
    }
}

#[test]
fn without_a_layer_that_asks_no_words_are_given() {
    let app = App::with(true, false);
    let seen = app.seen.borrow();
    assert!(!seen.lines.is_empty());
    assert!(seen.words.is_empty());
}

#[test]
fn switching_the_room_on_lays_the_page_out_anew() {
    let mut app = App::new(false);
    let before = app.seen.borrow().end;
    assert!(app.seen.borrow().lines.iter().all(|&(_, l)| l == 0));
    app.now += 100;
    let effects = app
        .kernel
        .set_setting(Instant(app.now), "interlinear.on", "on")
        .unwrap();
    app.settle(effects);
    app.show();
    let seen = app.seen.borrow();
    assert!(seen.lines.iter().all(|&(_, l)| l > 0), "{:?}", seen.lines);
    assert!(seen.end < before, "{:?} {before:?}", seen.end);
}

#[test]
fn blocks_a_filter_inserts_show_and_the_books_places_stay_its_own() {
    let pages = 6;
    let plain = App::configured(false, false, true).word_starts(pages);
    let mut parallel = App::configured(false, true, true);
    let first_end = parallel.seen.borrow().end;
    let starts = parallel.word_starts(pages);
    // Every word once, in order, none of what was inserted: the book's
    // words as without, fewer on as many pages (the translations take
    // room).
    assert!(
        starts.windows(2).all(|w| w[0] < w[1]),
        "in order, none twice"
    );
    assert!(
        starts.len() < plain.len(),
        "{} {}",
        starts.len(),
        plain.len()
    );
    assert_eq!(starts[..], plain[..starts.len()], "no word lost");
    // The first page ends at a place of the book.
    assert!(first_end.offset > 0);
}

#[test]
fn a_tap_on_a_layers_item_is_its_plugins_and_other_taps_the_readers() {
    let mut app = App::new(true);
    app.turn(3);
    let (start, line) = {
        let seen = app.seen.borrow();
        (seen.start, seen.lines[1])
    };
    // The room above the second line: the layer's item 1.
    let (area, leading) = line;
    let room = Point::new(
        i32::from(area.x) + i32::from(area.width) / 2,
        i32::from(area.y) - i32::from(leading) / 2,
    );
    app.tap(room);
    {
        let seen = app.seen.borrow();
        assert_eq!(seen.tapped.len(), 1, "{:?}", seen.tapped);
        assert_eq!(seen.tapped[0].0, 1);
        assert!(seen.tapped[0].1.is_some(), "the line beside it");
        assert_eq!(seen.start, start, "the page stays");
    }
    // At the right edge, past the text: the reader turns the page.
    app.tap(Point::new(475, 400));
    let seen = app.seen.borrow();
    assert_eq!(seen.tapped.len(), 1, "not the layer's");
    assert!(seen.start > start, "{:?} {start:?}", seen.start);
}

#[test]
fn a_layers_item_is_chosen_with_the_buttons_as_links_are() {
    let mut app = App::new(true);
    app.turn(3);
    let start = app.seen.borrow().start;
    app.run(paperpaw_reader_links());
    app.run(paperpaw_sdk::builtin::actions::DOWN);
    app.run(paperpaw_sdk::builtin::actions::CONFIRM);
    app.show();
    let seen = app.seen.borrow();
    assert_eq!(seen.tapped.iter().map(|t| t.0).collect::<Vec<_>>(), [1]);
    assert_eq!(seen.start, start);
}

/// The reader's action that starts choosing links with the buttons.
fn paperpaw_reader_links() -> Id {
    Id::new_static("reader.links")
}
