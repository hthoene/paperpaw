//! Page layers and frames on the shipped reader: a plugin reserves room
//! beside the text (at the right, left or top) and draws in it beside
//! every line, hears taps on it (by touch and with the buttons, not where
//! the bookmark corner is), draws behind the text and over it while
//! switched on, and sets pages in two columns or in one narrower one.

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
use paperpaw_sdk::builtin::{actions, events, params, slots};
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::layers::{PageFrameSpec, PageLayerSpec, Side};
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::{
    Color, Id, Manifest, MemoryStorage, Params, Plugin, Point, Rect, RegisterError, Registrar,
    SDK_VERSION, SettingSpec,
};

/// Buttons and a touch screen.
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
const BAR: Id = Id::new_static("margin.bar");
const TWO: Id = Id::new_static("margin.two");
const INSET: Id = Id::new_static("margin.inset");
/// Switches the layers that wipe out lines 3 (behind the text) and 4
/// (over it).
const ERASE: Setting<bool> = Setting::new("margin.erase");
static SETTINGS: [SettingSpec; 1] = [SettingSpec::switch(
    ERASE.static_str(),
    "margin.erase",
    false,
    paperpaw_sdk::builtin::groups::READING,
)];

/// What the layer was shown last, and the taps it heard.
#[derive(Debug, Default)]
struct Seen {
    room: Option<Rect>,
    columns: Vec<Rect>,
    lines: Vec<(Rect, Locator)>,
    tapped: Vec<(Id, Option<Locator>, i32)>,
}

/// A bar beside every line, in a room at a side; layers that wipe out a
/// line while switched on; two columns, and one narrower.
struct Margin(Rc<RefCell<Seen>>, Side);

impl Plugin for Margin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("margin"), "Margin", SDK_VERSION)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let (seen, side) = (self.0.clone(), self.1);
        r.add_page_layer(
            PageLayerSpec::new(BAR, "margin.bar", move |canvas, _, page| {
                let mut seen = seen.borrow_mut();
                seen.room = page.room;
                seen.columns = page.columns.to_vec();
                seen.lines = page.lines.iter().map(|l| (l.area, l.start)).collect();
                if let Some(room) = page.room {
                    for line in page.lines {
                        let (y, height) = (line.area.y, line.area.height);
                        canvas.fill_rect(
                            i32::from(room.x),
                            i32::from(y),
                            4,
                            u32::from(height),
                            Color::BLACK,
                        );
                    }
                }
            })
            .reserve(move |env| Some((side, env.metrics().mm(8) as u16)))
            .taps(),
        )?;
        // Line `n` of the page wiped out in white, across its column.
        let wipe = |n: usize| {
            move |canvas: &mut paperpaw_sdk::Canvas<'_>,
                  _: &paperpaw_sdk::Env<'_>,
                  page: &paperpaw_sdk::layers::PageView<'_>| {
                if let Some(line) = page.lines.get(n) {
                    let a = line.area;
                    canvas.fill_rect(
                        i32::from(a.x),
                        i32::from(a.y),
                        u32::from(a.width),
                        u32::from(a.height),
                        Color::WHITE,
                    );
                }
            }
        };
        let behind = Id::new_static("margin.behind");
        r.add_page_layer(
            PageLayerSpec::new(behind, "margin.behind", wipe(2))
                .order(-1)
                .switch(&ERASE),
        )?;
        let over = Id::new_static("margin.over");
        r.add_page_layer(
            PageLayerSpec::new(over, "margin.over", wipe(3))
                .order(1)
                .switch(&ERASE),
        )?;
        r.add_settings(&SETTINGS)?;
        r.add_page_frame(PageFrameSpec::new(INSET, |_, a| {
            vec![Rect::new(a.x + 30, a.y, a.width - 60, a.height)]
        }))?;
        let seen = self.0.clone();
        r.on(events::PAGE_LAYER_TAPPED, move |_, tapped, _| {
            let heard = (tapped.layer.clone(), tapped.line(), tapped.y);
            seen.borrow_mut().tapped.push(heard);
        })?;
        r.add_page_frame(PageFrameSpec::columns(TWO, 2, 24))
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
    /// The book open in the reader, with `config` besides English; the
    /// room at the right.
    fn new(config: &str) -> Self {
        Self::with(config, Side::Right)
    }

    /// The same with the room at `side`.
    fn with(config: &str, side: Side) -> Self {
        let storage = MemoryStorage::new()
            .with(
                BOOK,
                include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
            )
            .with(".paperpaw/plugins/setup/done", "");
        let builder = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config(format!(
                "[help]\nshow = false\n[core]\nlanguage = en\n{config}"
            ));
        let seen = Rc::new(RefCell::new(Seen::default()));
        let mut kernel = paperpaw_app::install(builder, fonts())
            .plugin(Margin(seen.clone(), side))
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

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
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
            // Not held long: no time passes before it is lifted.
            if phase == PointerPhase::Up {
                self.settle(effects);
            }
        }
    }

    /// The screen, by rows of luma.
    fn image(&mut self) -> Vec<Vec<u8>> {
        let display = DEVICE.display;
        let mut image = vec![vec![255u8; usize::from(display.width)]; usize::from(display.height)];
        self.kernel.render(&mut |y, view| {
            for row in 0..view.height {
                if let Some(line) = image.get_mut(usize::from(y + row)) {
                    for x in 0..view.width {
                        line[usize::from(x)] = view.pixel(x, row).luma();
                    }
                }
            }
        });
        image
    }
}

/// Whether any pixel from column `from` to before `to` is inked, in rows
/// `rows`.
fn inked(image: &[Vec<u8>], rows: std::ops::Range<usize>, from: usize, to: usize) -> bool {
    rows.into_iter()
        .any(|y| image[y][from..to].iter().any(|&luma| luma < 128))
}

#[test]
fn a_layer_draws_in_the_room_it_reserves() {
    let mut app = App::new("");
    let image = app.image();
    let seen = app.seen.borrow();
    let room = seen.room.expect("a room");
    // At the right of the text, 8 mm wide, inside the page's margin.
    assert!(room.right() < u32::from(DEVICE.display.width), "{room:?}");
    assert!(room.width > 60 && room.width < 75, "{room:?}");
    assert_eq!(seen.columns.len(), 1);
    let column = seen.columns[0];
    assert_eq!(
        column.right(),
        u32::from(room.x),
        "the text ends where the room begins"
    );
    assert!(seen.lines.len() > 8, "{} lines", seen.lines.len());
    // A bar beside each line, and no text in the room.
    let (area, _) = seen.lines[3];
    let rows = usize::from(area.y)..area.bottom() as usize;
    let x = usize::from(room.x);
    assert!(inked(&image, rows.clone(), x, x + 4));
    let text = usize::from(seen.lines[0].0.y)..seen.lines.last().unwrap().0.bottom() as usize;
    assert!(!inked(&image, text, x + 5, room.right() as usize));
}

#[test]
fn a_layers_room_hears_taps_and_the_buttons() {
    let mut app = App::new("");
    app.image();
    let (room, line) = {
        let seen = app.seen.borrow();
        (seen.room.expect("a room"), seen.lines[5])
    };
    // Beside the sixth line: told so.
    let y = i32::from(line.0.y) + i32::from(line.0.height) / 2;
    app.tap(Point::new(i32::from(room.x) + 10, y));
    assert_eq!(app.seen.borrow().tapped, [(BAR, Some(line.1), y)]);
    // The book's text has no links: choosing with the buttons comes to the
    // room first, and confirming tells the plugin too.
    app.image();
    app.run(Id::new_static("reader.links"));
    app.run(actions::CONFIRM);
    let seen = app.seen.borrow();
    assert_eq!(seen.tapped.len(), 2, "{:?}", seen.tapped);
    assert_eq!(seen.tapped[1].0, BAR);
}

#[test]
fn a_frame_sets_the_page_in_two_columns() {
    let mut app = App::new("page-frame = margin.two\n");
    let image = app.image();
    let seen = app.seen.borrow();
    let room = seen.room.expect("a room");
    let [left, right] = seen.columns[..] else {
        panic!("columns {:?}", seen.columns);
    };
    // Side by side, 24 pixels apart, up to the room.
    assert_eq!(left.right() + 24, u32::from(right.x));
    assert_eq!(right.right(), u32::from(room.x));
    // Lines in both, the right one going on where the left one ends.
    let in_right: Vec<_> = seen.lines.iter().filter(|(a, _)| a.x == right.x).collect();
    assert!(!in_right.is_empty() && in_right.len() < seen.lines.len());
    assert!(seen.lines.windows(2).all(|w| w[0].1 < w[1].1));
    // Ink on both sides of a gap without any.
    let text = usize::from(seen.lines[0].0.y)..seen.lines.last().unwrap().0.bottom() as usize;
    let gap = (left.right() as usize + 2)..(usize::from(right.x) - 2);
    assert!(!inked(&image, text.clone(), gap.start, gap.end));
    assert!(inked(&image, text.clone(), usize::from(left.x), gap.start));
    assert!(inked(&image, text, gap.end, usize::from(room.x)));
    drop(seen);
    // The next page goes on after the second column.
    let last = app.seen.borrow().lines.last().unwrap().1;
    app.run(actions::NEXT_PAGE);
    app.image();
    let first = app.seen.borrow().lines[0].1;
    assert!(first > last, "{first:?} after {last:?}");
}

#[test]
fn choosing_a_frame_lays_the_page_out_anew_where_it_was() {
    let mut app = App::new("");
    app.run(actions::NEXT_PAGE);
    app.image();
    let (columns, first) = {
        let seen = app.seen.borrow();
        (seen.columns.len(), seen.lines[0].1)
    };
    assert_eq!(columns, 1);
    app.now += 100;
    let effects = app
        .kernel
        .set_setting(Instant(app.now), "core.page-frame", "margin.two")
        .unwrap();
    app.settle(effects);
    app.image();
    let seen = app.seen.borrow();
    assert_eq!(seen.columns.len(), 2);
    // From the line that holds where the page was, in the narrower column.
    let (start, next) = (seen.lines[0].1, seen.lines.get(1).map(|line| line.1));
    assert!(start <= first, "{start:?} after {first:?}");
    assert!(
        next.is_none_or(|next| next > first),
        "{first:?} not on the first line"
    );
}

/// Whether the rows of `area` have ink in its columns.
fn inked_in(image: &[Vec<u8>], area: Rect) -> bool {
    inked(
        image,
        usize::from(area.y)..area.bottom() as usize,
        usize::from(area.x),
        area.right() as usize,
    )
}

#[test]
fn layers_below_order_0_are_behind_the_text_and_the_others_over_it() {
    let mut app = App::new("[margin]\nerase = on\n");
    let image = app.image();
    let seen = app.seen.borrow();
    // Wiped out behind the text: the text is there all the same.
    assert!(inked_in(&image, seen.lines[2].0));
    // Wiped out over it: gone.
    assert!(!inked_in(&image, seen.lines[3].0));
}

#[test]
fn a_layer_switched_on_draws_at_once() {
    let mut app = App::new("");
    let image = app.image();
    let line = app.seen.borrow().lines[3].0;
    assert!(inked_in(&image, line));
    app.now += 100;
    let effects = app
        .kernel
        .set_setting(Instant(app.now), "margin.erase", "on")
        .unwrap();
    app.settle(effects);
    let image = app.image();
    assert!(!inked_in(&image, line));
}

#[test]
fn rooms_at_the_left_and_at_the_top() {
    let mut app = App::with("", Side::Left);
    app.image();
    let (room, column) = {
        let seen = app.seen.borrow();
        (seen.room.expect("a room"), seen.columns[0])
    };
    // Left of the text, as high.
    assert_eq!(room.right(), u32::from(column.x));
    assert_eq!((room.y, room.height), (column.y, column.height));

    let mut app = App::with("", Side::Top);
    app.image();
    let seen = app.seen.borrow();
    let (room, column) = (seen.room.expect("a room"), seen.columns[0]);
    // Above it, as wide; the text starts below it.
    assert_eq!(room.bottom(), u32::from(column.y));
    assert_eq!((room.x, room.width), (column.x, column.width));
    assert!(seen.lines[0].0.y >= column.y);
}

#[test]
fn a_frame_of_one_narrower_column_sets_the_text_in_it() {
    let mut plain = App::new("");
    plain.image();
    let text = plain.seen.borrow().columns[0];
    let mut app = App::new("page-frame = margin.inset\n");
    let image = app.image();
    let seen = app.seen.borrow();
    let [column] = seen.columns[..] else {
        panic!("columns {:?}", seen.columns);
    };
    assert_eq!(
        column,
        Rect::new(text.x + 30, text.y, text.width - 60, text.height)
    );
    assert!(
        seen.lines
            .iter()
            .all(|(a, _)| a.x == column.x && a.width == column.width)
    );
    // No ink between the text's edge and the column's.
    let rows = usize::from(seen.lines[0].0.y)..seen.lines.last().unwrap().0.bottom() as usize;
    assert!(!inked(
        &image,
        rows,
        usize::from(text.x),
        usize::from(column.x)
    ));
}

#[test]
fn the_bookmark_corner_goes_before_a_room_at_the_top() {
    let mut app = App::with("", Side::Top);
    app.image();
    let room = app.seen.borrow().room.expect("a room");
    // In the room and in the corner (12 mm at the top right): a bookmark,
    // not a tap on the room.
    let at = Point::new(room.right() as i32 - 3, i32::from(room.y) + 2);
    assert!(at.x > i32::from(DEVICE.display.width) - 100, "{room:?}");
    app.tap(at);
    assert!(
        app.seen.borrow().tapped.is_empty(),
        "{:?}",
        app.seen.borrow().tapped
    );
    // Lower in the room, out of the corner: the layer's.
    app.tap(Point::new(i32::from(room.x) + 3, i32::from(room.y) + 2));
    assert_eq!(app.seen.borrow().tapped.len(), 1);
}
