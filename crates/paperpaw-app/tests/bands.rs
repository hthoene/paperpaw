//! A frame the kernel draws in bands is the frame it draws at once.
//! The X4 has no memory for a whole frame: the kernel lends each screen a
//! band at a time (`frame_rows`), on the X4 96 rows of its panel, which lies
//! on its side (the screen's columns). A screen that lays itself out in the
//! band it is lent, not in its own room, shows up broken on the device only.
//! The screens of the standard app are drawn here both ways, on the X4's
//! panel, and compared pixel by pixel.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod native_activation;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::{Id, MemoryStorage, Params};

/// The X4's panel as its firmware has it: on its side, two bits of gray.
const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(800, 480, 220, PixelFormat::Gray2, Rotation::Deg90, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

const BOOK: &str = "books/a-walk.txt";

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

struct App {
    kernel: Kernel,
    now: u64,
}

impl App {
    /// The app on a card with a book, drawn in bands of `rows` rows or at once.
    fn new(rows: Option<u16>) -> Self {
        let text = "A walk along the river, and back by the mill.\n\n".repeat(200);
        let card = native_activation::SharedCard::new(
            MemoryStorage::new()
                .with(".paperpaw/plugins/setup/done", Vec::new())
                .with(BOOK, text.into_bytes()),
        );
        let mut builder = Kernel::builder(DEVICE)
            .storage(card.clone())
            .user_config(native_activation::QUIET);
        if let Some(rows) = rows {
            builder = builder.frame_rows(rows).row_align(8);
        }
        let builder = paperpaw_app::install(builder, fonts());
        let mut kernel = native_activation::build_on(builder, card.handle());
        let effects = kernel.start(Instant(0));
        let mut app = Self { kernel, now: 0 };
        app.settle(effects);
        app
    }

    /// Lets timers run until nothing is due within a second, drawing what
    /// changes as a device would.
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            self.kernel.render(&mut |_, _| {});
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

    fn open(&mut self, slot: Id, params: Params) {
        self.now += 100;
        let effects = self.kernel.open_slot(Instant(self.now), slot, params);
        self.settle(effects);
    }

    /// The whole screen drawn anew: every pixel's gray, row by row, and how
    /// many bands it came in.
    fn frame(&mut self) -> (Vec<u8>, usize) {
        let display = self.kernel.device().display;
        let width = usize::from(display.width);
        let mut frame = vec![0xAB; width * usize::from(display.height)];
        let mut bands = 0;
        self.now += 100;
        self.kernel.draw_again(Instant(self.now));
        self.kernel.render(&mut |top, rows| {
            bands += 1;
            for y in 0..rows.height {
                for x in 0..rows.width {
                    let at = usize::from(top + y) * width + usize::from(x);
                    if let Some(pixel) = frame.get_mut(at) {
                        *pixel = rows.pixel(x, y).g;
                    }
                }
            }
        });
        (frame, bands)
    }
}

/// The screens looked at: the home screen, the settings, a book in the reader.
fn screens(rows: Option<u16>) -> Vec<(&'static str, Vec<u8>, usize)> {
    let mut app = App::new(rows);
    let mut out = Vec::new();
    let (frame, bands) = app.frame();
    out.push(("home", frame, bands));
    app.run(actions::SETTINGS);
    let (frame, bands) = app.frame();
    out.push(("settings", frame, bands));
    app.open(slots::READER, Params::new().with(params::PATH, BOOK));
    let (frame, bands) = app.frame();
    out.push(("reader", frame, bands));
    out
}

/// Where two frames differ: how many pixels, and the first rows that do.
fn difference(whole: &[u8], banded: &[u8], width: usize) -> Option<String> {
    let differing: Vec<usize> = whole
        .iter()
        .zip(banded)
        .enumerate()
        .filter_map(|(at, (a, b))| (a != b).then_some(at))
        .collect();
    let first = *differing.first()?;
    let last = *differing.last()?;
    Some(format!(
        "{} pixels differ, rows {} to {}, first at x {}",
        differing.len(),
        first / width,
        last / width,
        first % width
    ))
}

#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native Settings, home and reader (`native-linked-reading`): the interface-6 ones are removed (#244)"
)]
fn a_screen_drawn_in_bands_is_the_screen_drawn_at_once() {
    let width = usize::from(DEVICE.display.width);
    let whole = screens(None);
    // Something is drawn, and the screens are not one and the same.
    for (name, frame, bands) in &whole {
        assert_eq!(*bands, 1, "{name}: drawn at once");
        assert!(
            frame.iter().any(|g| *g < 0x80),
            "{name}: something is drawn"
        );
        assert!(!frame.contains(&0xAB), "{name}: every pixel is drawn");
    }
    assert_ne!(whole[0].1, whole[1].1, "home and settings differ");
    assert_ne!(whole[1].1, whole[2].1, "settings and reader differ");
    // The X4's band (96 rows of the panel), and one that does not divide it.
    for rows in [96, 56] {
        let banded = screens(Some(rows));
        let mut faults = Vec::new();
        for ((name, expected, _), (_, found, bands)) in whole.iter().zip(&banded) {
            assert!(*bands > 1, "{name}: drawn in bands of {rows} rows");
            if let Some(what) = difference(expected, found, width) {
                faults.push(format!("{name} in bands of {rows} rows: {what}"));
            }
        }
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }
}
