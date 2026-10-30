//! The headlines widget on the home screen, with the real kernel, the
//! home screen and the default fonts: the headlines fetched last, as many
//! as fit and are wanted, in every frame; without feeds, how to add them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat, Storage,
};

/// An X4: black and white, buttons.
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

/// A card with two feeds fetched and five headlines.
fn card() -> MemoryStorage {
    let storage = MemoryStorage::new();
    for feed in ["Tagesschau", "Guardian"] {
        storage
            .write(&format!("News/{feed}.html"), b"<p>news</p>")
            .unwrap();
    }
    let headlines = "Budget agreed\tNews/Tagesschau.html\n\
                     Storm warning\tNews/Tagesschau.html\n\
                     Water on the Moon\tNews/Guardian.html\n\
                     A reading garden\tNews/Guardian.html\n\
                     Strike ends\tNews/Tagesschau.html";
    storage
        .write(".paperpaw/plugins/news/headlines.txt", headlines.as_bytes())
        .unwrap();
    storage
}

/// The home screen with `page` as its first page and the settings of
/// `config` (lines of the `[news]` section): what it describes, and how
/// many pixels are black.
fn home(storage: MemoryStorage, page: &str, config: &str) -> (String, usize) {
    let (text, rows) = screen(storage, page, config);
    (text, rows.iter().sum())
}

/// [`home`], with the black pixels of each row of the screen.
fn screen(storage: MemoryStorage, page: &str, config: &str) -> (String, Vec<usize>) {
    let user = format!(
        "[core]\nlanguage = en\n[help]\nshow = false\n[home]\ncolumns = 4\npage-1 = \"{page}\"\n[news]\n{config}\n"
    );
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config(user)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_home::Home)
        .plugin(paperpaw_plugin_news::News)
        .build()
        .unwrap();
    let mut effects = kernel.start(Instant(0));
    let mut now = 0;
    for _ in 0..1000 {
        match effects.wake_at() {
            Some(at) if at.0 <= now + 1000 => {
                now = now.max(at.0);
                effects = kernel.tick(Instant(now));
            }
            _ => break,
        }
    }
    let mut rows = vec![0; usize::from(DEVICE.display.height)];
    kernel.render(&mut |top, view| {
        for y in 0..view.height {
            let black = (0..view.width)
                .filter(|&x| view.pixel(x, y).luma() < 128)
                .count();
            if let Some(row) = rows.get_mut(usize::from(top + y)) {
                *row += black;
            }
        }
    });
    let mut text = String::new();
    for part in kernel.describe() {
        text.push_str(&part.label);
        text.push('\n');
    }
    (text, rows)
}

/// Black pixels in the rows from `from` to `to` of a screen's `rows`.
fn ink(rows: &[usize], from: usize, to: usize) -> usize {
    rows[from..to].iter().sum()
}

#[test]
fn the_newest_headlines_show_as_many_as_wanted() {
    let (shown, _) = home(card(), "news.widget.headlines 0/0 4x4", "");
    assert!(shown.contains("News\n"), "{shown}");
    // Four at first.
    for title in [
        "Budget agreed",
        "Storm warning",
        "Water on the Moon",
        "A reading garden",
    ] {
        assert!(shown.contains(title), "{title}: {shown}");
    }
    assert!(!shown.contains("Strike ends"), "{shown}");
    let (shown, _) = home(
        card(),
        "news.widget.headlines 0/0 4x4",
        "widget.headlines.count = news.count.2\nwidget.headlines.source = true",
    );
    assert!(shown.contains("Storm warning"), "{shown}");
    assert!(!shown.contains("Water on the Moon"), "{shown}");
    assert!(shown.contains("Tagesschau"), "{shown}");
}

#[test]
fn without_feeds_it_says_how_to_add_them() {
    let (shown, _) = home(MemoryStorage::new(), "news.widget.headlines 0/0 4x2", "");
    assert!(shown.contains("Add news feeds"), "{shown}");
}

#[test]
fn it_draws_in_every_frame() {
    let (_, empty) = home(card(), "", "");
    for frame in [
        "core.frame.none",
        "core.frame.line",
        "core.frame.card",
        "core.frame.dark",
    ] {
        for size in ["4x2", "2x2", "4x1"] {
            for lines in ["news.lines.1", "news.lines.2"] {
                let page = format!("news.widget.headlines 0/0 {size}");
                let config =
                    format!("widget.headlines.frame = {frame}\nwidget.headlines.lines = {lines}");
                let (_, black) = home(card(), &page, &config);
                assert!(black > empty + 50, "{size} in {frame}, {lines}");
            }
        }
    }
}

/// A headline in a low cell stays inside it: the caption is left out
/// rather than the headline drawn below the cell.
#[test]
fn a_low_cell_keeps_its_headline_inside() {
    let (_, empty) = screen(card(), "", "");
    let below = ink(&empty, 140, 600);
    for size in ["1x1", "2x1", "4x1"] {
        let page = format!("news.widget.headlines 0/0 {size}");
        let (shown, rows) = screen(card(), &page, "");
        assert!(shown.contains("Bu"), "{size}: {shown}");
        assert_eq!(ink(&rows, 140, 600), below, "{size} draws below its cell");
    }
}

/// Without feeds, a cell too small for the whole sentence still says
/// where feeds are added, rather than the sentence cut short.
#[test]
fn a_small_cell_says_where_feeds_are_added() {
    let (shown, _) = home(MemoryStorage::new(), "news.widget.headlines 0/0 2x1", "");
    assert!(
        shown.contains("Add feeds under News in the tools"),
        "{shown}"
    );
}
