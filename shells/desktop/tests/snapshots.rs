//! Snapshot tests: the emulator draws screens headless on several device
//! profiles, and each picture is compared with its reference in
//! `tests/snapshots`. Layout faults (text over a button, a cut line, a
//! widget out of place) then show as a failing test.
//!
//! The emulator's clock and battery are fixed, and every run starts from
//! a fresh library with the sample books and the pages of
//! `tests/fixtures` (for what the samples do not show, such as small
//! capitals), so the pictures repeat. A few
//! pixels may differ between computers where glyph edges round
//! differently; more than that fails.
//!
//! After a change meant to alter a screen, write the references anew:
//!
//! ```sh
//! PAPERPAW_BLESS=1 cargo test -p paperpaw-emulator --test snapshots
//! ```
//!
//! and look at the changed pictures before committing them. A failing
//! test leaves what it drew in `target/snapshots`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "support/pixels.rs"]
mod pixels;
use pixels::{grays, pixels, write_gray};

/// A screen to draw: its name, the device profile, settings, input steps.
struct Case {
    name: &'static str,
    profile: &'static str,
    /// The book open at start, if any.
    book: Option<&'static str>,
    sets: &'static [&'static str],
    script: &'static str,
    /// On colour profiles: the share of pixels, in parts per ten thousand,
    /// that must be in colour (ADR 0015). The reference keeps the grays.
    colour: usize,
    /// Requests answered from the emulator's fixtures (`--network
    /// fixtures`): a plugin library and the servers plugins ask, offline.
    fixtures: bool,
}

/// Cases that need native activation: Wi-Fi is a native-only plugin since
/// #244 and the desktop does not offer `uses.wifi`, so the interface-6 app has
/// no Wi-Fi page to draw. Their references stay; they run again then.
const NEEDS_NATIVE_ACTIVATION: [&str; 2] = ["x4-pro-wifi", "x4-pro-wifi-networks"];

const CASES: &[Case] = &[
    Case {
        name: "x4-settings-search",
        profile: "xteink-x4",
        book: None,
        sets: &["slots.core.home=settings.list"],
        script: "right",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-settings-search-de",
        profile: "xteink-x4",
        book: None,
        sets: &["slots.core.home=settings.list", "core.language=de"],
        script: "right,right",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-home",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-reader-epub",
        profile: "xteink-x4",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "page-forward,page-forward",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-settings-round-strong",
        profile: "xteink-x4",
        book: None,
        sets: &[
            "slots.core.home=settings.list",
            "core.ui-corners=core.corners.round",
            "core.ui-contrast=core.contrast.strong",
        ],
        script: "right,right",
        colour: 0,
        fixtures: false,
    },
    // The reading settings with a sample of text above them. On the X4
    // (buttons alone, compact rows) the text size changed with the
    // buttons, shown there before it is kept; on the Boox in German, its
    // sliders as touched.
    Case {
        name: "x4-reading-preview",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "run:core.settings,wait:300,right,confirm,wait:300,right,confirm,right,right,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "boox-reading-de",
        profile: "boox-go-7",
        book: None,
        sets: &["core.language=de"],
        script: "swipe:600:1100:600:500,wait:300,tap-on:Einstellungen,wait:500,tap-on:Lesen,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "boox-home",
        profile: "boox-go-7",
        book: None,
        sets: &[],
        script: "",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "boox-home-arranged",
        profile: "boox-go-7",
        book: None,
        // Placed in the grid: the clock and the date side by side, the
        // tools in a corner, empty cells between.
        sets: &[
            "home.page-1=home.clock 0/0 2x1, home.date 2/0 2x1, tool:core.library 0/4 1x1, tool:core.settings 3/4 1x1",
            "home.clock.date=false",
            "home.date.align=core.align.right",
        ],
        script: "",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "boox-reader-epub",
        profile: "boox-go-7",
        book: Some("books/die-verwandlung.epub"),
        sets: &[],
        script: "",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "boox-reader-txt",
        profile: "boox-go-7",
        book: Some("books/alice-in-wonderland.txt"),
        sets: &["core.language=de"],
        script: "",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "boox-small-caps",
        profile: "boox-go-7",
        book: Some("books/small-caps.html"),
        sets: &[],
        script: "",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-pictures-shown",
        profile: "xteink-x4",
        book: Some("books/pictures.epub"),
        sets: &[],
        script: "wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-pictures-caption",
        profile: "xteink-x4",
        book: Some("books/pictures.epub"),
        sets: &["aids.pictures=aids.pictures.caption"],
        script: "wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-pictures-hidden",
        profile: "xteink-x4",
        book: Some("books/pictures.epub"),
        sets: &["aids.pictures=aids.pictures.hide"],
        script: "wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "boox-settings",
        profile: "boox-go-7",
        book: None,
        sets: &["slots.core.home=settings.list"],
        script: "",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "boox-widget-editor",
        profile: "boox-go-7",
        book: None,
        // A long press shows the grid; a tap on the clock its options.
        sets: &[],
        script: "long-tap:600:1400,wait:300,tap:600:120,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "boox-developer-settings",
        profile: "boox-go-7",
        book: None,
        // Developer options on: their group of settings.
        sets: &["settings.advanced=true", "core.developer=true"],
        script: "swipe:600:1100:600:500,wait:300,tap-on:Settings,wait:500,tap-on:Developer,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        // The Wi-Fi page (the emulator pretends to manage Wi-Fi on the X4
        // profiles), switched on: the phone setup first (no network is
        // kept), the row of the networks found.
        name: "x4-pro-wifi",
        profile: "xteink-x4-pro",
        book: None,
        sets: &[],
        script: "swipe:240:700:240:300,wait:300,tap-on:Settings,wait:500,tap-on:Wi-Fi,wait:500,tap:240:140,wait:2500",
        colour: 0,
        fixtures: false,
    },
    Case {
        // The same, the page of the networks found opened.
        name: "x4-pro-wifi-networks",
        profile: "xteink-x4-pro",
        book: None,
        sets: &[],
        script: "swipe:240:700:240:300,wait:300,tap-on:Settings,wait:500,tap-on:Wi-Fi,wait:500,tap:240:140,wait:2500,tap-on:Networks,wait:500",
        colour: 0,
        fixtures: false,
    },
    // The plugin store on the X4, its list kept from before (the library
    // asked for does not answer): search, the newest, then the groups with
    // how many plugins for the X4 each has.
    Case {
        name: "x4-store",
        profile: "xteink-x4",
        book: None,
        sets: &["store.catalog=http://127.0.0.1:9/plugins.json"],
        script: "run:store.open,wait:6000",
        colour: 0,
        fixtures: false,
    },
    // A plugin's page in the store, from the library the emulator answers
    // offline: its long summary, its facts and its rows each below the
    // other (a page that drew its rows over its summary passed before).
    Case {
        name: "x4-store-page",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "run:store.open,wait:3000,open:store.plugin:id=reading-aids,wait:3000",
        colour: 0,
        fixtures: true,
    },
    Case {
        name: "x4-pro-store-page",
        profile: "xteink-x4-pro",
        book: None,
        sets: &[],
        script: "run:store.open,wait:3000,open:store.plugin:id=reading-aids,wait:3000",
        colour: 0,
        fixtures: true,
    },
    Case {
        name: "boox-store-page",
        profile: "boox-go-7",
        book: None,
        sets: &[],
        script: "run:store.open,wait:3000,open:store.plugin:id=reading-aids,wait:3000",
        colour: 0,
        fixtures: true,
    },
    Case {
        name: "boox-all-tools",
        profile: "boox-go-7",
        book: None,
        // Up on the home screen: every tool.
        sets: &[],
        script: "swipe:600:1100:600:500,wait:300",
        colour: 0,
        fixtures: false,
    },
    // The screen lock's PIN keypad (`boox-lock-pin`) is gone with the interface-6 lock; it
    // returns with the native lock screen when the host runs native plugins (#228).
    // The screens one passes through most, on the X4 (buttons alone,
    // black and white) and on the Boox (touch): the main menu and the
    // tools, the library, the reader's menus, the contents, the
    // bookmarks, settings pages, statistics, the file manager, the
    // sleep screen, the keyboard, a book's menu. (Not the about page: it shows the
    // version, which would need a new picture at every release.)
    Case {
        name: "x4-main-menu",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "run:core.menu,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-tools",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "run:core.tools,wait:300",
        colour: 0,
        fixtures: false,
    },
    // All tools in German: names on two lines, hyphenated, and the
    // focus round the focused tool's icon and name.
    Case {
        name: "x4-drawer-de",
        profile: "xteink-x4",
        book: None,
        sets: &["core.language=de"],
        script: "open:home.drawer,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-reader-menu",
        profile: "xteink-x4",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "confirm,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-toc",
        profile: "xteink-x4",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "run:reader.show-toc,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-bookmarks",
        profile: "xteink-x4",
        book: Some("books/die-verwandlung.epub"),
        sets: &[],
        script: "page-forward,page-forward,page-forward,run:reader.toggle-bookmark,wait:300,\
                 run:reader.show-annotations,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-settings-display",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "run:core.settings,wait:300,open:settings.group:id=core.group.reading-display,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-settings-controls",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "run:core.settings,wait:300,open:settings.group:id=core.group.controls,wait:300",
        colour: 0,
        fixtures: false,
    },
    // Buttons in slots of a touch row: the footnote's side by side, the
    // skim screen's four and the transfer's one below the other.
    Case {
        name: "x4-footnote",
        profile: "xteink-x4",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "open:reader.footnote:text=A note at the foot of the page.,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-skim",
        profile: "xteink-x4",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "page-forward,open:reader.skim-screen,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-transfer",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "run:transfer.open,wait:300",
        colour: 0,
        fixtures: false,
    },
    // The flash cards before any deck: how to bring cards in (a phone,
    // CSV, TSV or Anki), in English and German.
    Case {
        name: "x4-flashcards-empty",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "run:vocabulary.flash-cards,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-flashcards-empty-de",
        profile: "xteink-x4",
        book: None,
        sets: &["core.language=de"],
        script: "run:vocabulary.flash-cards,wait:300",
        colour: 0,
        fixtures: false,
    },
    // The same files: the row that says they are new, and the result of
    // reading them as the flash cards open (a line without a front told).
    Case {
        name: "x4-flashcards-sd-todo",
        profile: "xteink-x4",
        book: None,
        sets: &["vocabulary.sd-auto=false"],
        script: "run:vocabulary.flash-cards,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-flashcards-sd-result",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "run:vocabulary.flash-cards,wait:900",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-flashcards-sd-result-de",
        profile: "xteink-x4",
        book: None,
        sets: &["core.language=de"],
        script: "run:vocabulary.flash-cards,wait:900",
        colour: 0,
        fixtures: false,
    },
    // Part 2: the way into the files (a save for each deck, the
    // switch of each linked deck), and a deck saved to its file. The decks
    // are made from the files first (German is deck 1).
    Case {
        name: "x4-flashcards-sd-menu",
        profile: "xteink-x4",
        book: None,
        sets: &["vocabulary.sd-auto=false"],
        script: "open:vocabulary.sd-card,wait:900,open:vocabulary.sd-menu,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-flashcards-sd-menu-de",
        profile: "xteink-x4",
        book: None,
        sets: &["vocabulary.sd-auto=false", "core.language=de"],
        script: "open:vocabulary.sd-card,wait:900,open:vocabulary.sd-menu,wait:300",
        colour: 0,
        fixtures: false,
    },
    // German has a line that is no card: a save would drop it, so it is
    // refused; Spanish is saved.
    Case {
        name: "x4-flashcards-sd-not-saved",
        profile: "xteink-x4",
        book: None,
        sets: &["vocabulary.sd-auto=false"],
        script: "open:vocabulary.sd-card,wait:900,back,wait:100,open:vocabulary.sd-card:save=1,wait:900",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-flashcards-sd-not-saved-de",
        profile: "xteink-x4",
        book: None,
        sets: &["vocabulary.sd-auto=false", "core.language=de"],
        script: "open:vocabulary.sd-card,wait:900,back,wait:100,open:vocabulary.sd-card:save=1,wait:900",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-flashcards-sd-saved",
        profile: "xteink-x4",
        book: None,
        sets: &["vocabulary.sd-auto=false"],
        script: "open:vocabulary.sd-card,wait:900,back,wait:100,open:vocabulary.sd-card:save=2,wait:900",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-flashcards-sd-saved-de",
        profile: "xteink-x4",
        book: None,
        sets: &["vocabulary.sd-auto=false", "core.language=de"],
        script: "open:vocabulary.sd-card,wait:900,back,wait:100,open:vocabulary.sd-card:save=2,wait:900",
        colour: 0,
        fixtures: false,
    },
    Case {
        // The power button: the sleep screen, the device asleep.
        name: "x4-sleep",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "power,wait:500",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "boox-main-menu",
        profile: "boox-go-7",
        book: None,
        sets: &[],
        script: "run:core.menu,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        // A tap in the middle of the page: the reader's menu bar, opened
        // before the pages are counted (a page turns as its button goes
        // down). The count goes on below it, and the bar shows "2 / 185"
        // once it ends (#117).
        name: "boox-reader-menu",
        profile: "boox-go-7",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "page-forward,tap:632:840,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        // The panel below the page: font, sizes, switches.
        name: "boox-reader-font",
        profile: "boox-go-7",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "page-forward,run:reader.quick-settings,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "boox-toc",
        profile: "boox-go-7",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "run:reader.show-toc,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "boox-settings-display",
        profile: "boox-go-7",
        book: None,
        sets: &[],
        script: "run:core.settings,wait:300,tap-on:Display,wait:300",
        colour: 0,
        fixtures: false,
    },
    Case {
        // The settings searched: the keyboard below the field.
        name: "boox-keyboard",
        profile: "boox-go-7",
        book: None,
        sets: &[],
        script: "run:core.settings,wait:300,tap-on:Search,wait:300",
        colour: 0,
        fixtures: false,
    },
    // The quick panel while reading, its sliders a fingertip high and
    // across the screen: on the X4 Pro while a finger moves the light's
    // colour (down far from the knob, which moves as far as the finger,
    // its value shown at once), on the Boox once it is lifted.
    Case {
        name: "x4-pro-quick-panel",
        profile: "xteink-x4-pro",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "run:core.quick-panel,wait:300,down:300:318,move:330:320,move:360:322,wait:100",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "boox-quick-panel",
        profile: "boox-go-7",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "run:core.quick-panel,wait:300,down:1000:435,move:950:437,move:800:439,up:800:439,wait:300",
        colour: 0,
        fixtures: false,
    },
    // The quick panel over the home screen on the X4: from the screen's
    // top edge, over its status line (#120), no band of it above.
    Case {
        name: "x4-quick-panel",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "run:core.quick-panel,wait:300",
        colour: 0,
        fixtures: false,
    },
    // The reader's panel on the X4, the row that opens the fonts focused:
    // its whole name and value on the focus bar, aligned with the rows
    // below.
    Case {
        name: "x4-reader-panel",
        profile: "xteink-x4",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "run:reader.quick-settings,wait:300,right,wait:200",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "kobo-colour-cover",
        profile: "kobo-libra-colour",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "",
        colour: 1_000,
        fixtures: false,
    },
    // A picture as a book: its page, the picture fit to the screen.
    Case {
        name: "x4-pictures-book",
        profile: "xteink-x4",
        book: Some("books/alice-cover.jpg"),
        sets: &[],
        script: "wait:1500",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-mobi-reading",
        profile: "xteink-x4",
        book: Some("books/lighthouse.mobi"),
        sets: &[],
        script: "wait:1500",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-mobi-picture",
        profile: "xteink-x4",
        book: Some("books/lighthouse.mobi"),
        sets: &[],
        script: "wait:1500,page-forward,wait:1500",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-mobi-locked",
        profile: "xteink-x4",
        book: Some("books/locked.azw"),
        sets: &[],
        script: "wait:1500",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-docx-reading",
        profile: "xteink-x4",
        book: Some("books/report.docx"),
        sets: &[],
        script: "wait:1500",
        colour: 0,
        fixtures: false,
    },
    Case {
        name: "x4-docx-second-page",
        profile: "xteink-x4",
        book: Some("books/report.docx"),
        sets: &[],
        script: "wait:1500,page-forward,wait:1500",
        colour: 0,
        fixtures: false,
    },
];

/// Pixels whose gray differs by more than this count as different.
const LEVELS: u8 = 48;
/// The share of pixels that may differ, in parts per ten thousand.
const ALLOWED: usize = 20;

/// The repository: two folders above this crate.
fn root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .unwrap_or(manifest)
        .to_path_buf()
}

/// A library with only the sample books (and the book to reopen).
fn library(case: &Case) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "paperpaw-snapshot-{}-{}",
        case.name,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let books = dir.join("books");
    fs::create_dir_all(&books).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut sources = vec![root().join("samples/library/books"), fixtures.clone()];
    if case.name.contains("endbook") {
        sources.push(fixtures.with_file_name("fixtures-endbook"));
    }
    if case.name.contains("docx") {
        sources.push(fixtures.with_file_name("fixtures-docx"));
    }
    if case.name.contains("pictures") {
        sources.push(fixtures.with_file_name("fixtures-pictures"));
    }
    if case.name.contains("mobi") {
        sources.push(fixtures.with_file_name("fixtures-mobi"));
    }
    if case.name == "x4-pictures-book" {
        sources.push(root().join("crates/paperpaw-image/tests/data"));
    }
    for dir in sources {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                fs::copy(entry.path(), books.join(entry.file_name())).unwrap();
            }
        }
    }
    // The plugin store's index of the list, as if kept from a visit
    // before.
    if case.name.contains("store") {
        let store = dir.join(".paperpaw/plugins/store");
        fs::create_dir_all(&store).unwrap();
        let kept = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/store");
        for entry in fs::read_dir(kept).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), store.join(entry.file_name())).unwrap();
        }
    }
    // Decks as files on the card: one with a line that is no card.
    if case.name.contains("flashcards-sd") {
        let folder = dir.join("Flashcards");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("German.tsv"),
            "front\tback\texample\nKatze\tcat\tDie Katze schläft.\nHund\tdog\n\tno front\nHaus\thouse\n",
        )
        .unwrap();
        fs::write(folder.join("Spanish.csv"), "casa;house\ngato;cat\n").unwrap();
    }
    // Hyphenation is data: the German names of the drawer break only where
    // the German pack is installed (built from the sources, as shipped).
    if case.name == "x4-drawer-de" {
        let german = paperpaw_hyphenation_packs::PACKAGES
            .iter()
            .find(|package| package.id == "hyphenation-de")
            .unwrap();
        let plugins = dir.join("plugins");
        fs::create_dir_all(&plugins).unwrap();
        paperpaw_hyphenation_packs::build(&root(), &plugins, german).unwrap();
    }
    // The first-run setup is done (it is not what any case shows).
    let setup = dir.join(".paperpaw/plugins/setup");
    fs::create_dir_all(&setup).unwrap();
    fs::write(setup.join("done"), "").unwrap();
    if let Some(book) = case.book {
        let state = dir.join(".paperpaw/plugins/reader");
        fs::create_dir_all(&state).unwrap();
        fs::write(
            state.join("state.ini"),
            format!("[reader]\nlast = {book}\n"),
        )
        .unwrap();
    }
    dir
}

/// Draws `case` and returns its picture in grays, and how many of its
/// pixels are in colour.
fn draw(case: &Case) -> (u32, u32, Vec<u8>, usize) {
    let library = library(case);
    let shot = library.join("shot.png");
    let mut command = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"));
    command
        .args(["--profile", case.profile, "--headless", "--library"])
        .arg(&library)
        .args(["--screenshot"])
        .arg(&shot);
    if !case.script.is_empty() {
        command.args(["--script", case.script]);
    }
    // The hint on gestures would cover every first home screen.
    // A card of the same free space on every computer.
    command.args(["--free-space", "7400000000"]);
    for set in case.sets {
        command.args(["--set", set]);
    }
    if case.book.is_none() {
        command.args(["--set", "reader.open-last=false"]);
    }
    if case.fixtures {
        command.args(["--network", "fixtures"]);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}: {}",
        case.name,
        String::from_utf8_lossy(&output.stderr)
    );
    let picture = pixels(&shot);
    let _ = fs::remove_dir_all(&library);
    picture
}

#[test]
fn screens_look_as_before() {
    let bless = std::env::var_os("PAPERPAW_BLESS").is_some();
    let references = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let failed_dir = root().join("target/snapshots");
    let mut failures = Vec::new();
    for case in CASES {
        if NEEDS_NATIVE_ACTIVATION.contains(&case.name) {
            continue;
        }
        let (width, height, gray, colourful) = draw(case);
        if colourful < gray.len() * case.colour / 10_000 {
            failures.push(format!(
                "{}: {colourful} pixels in colour, too few",
                case.name
            ));
        }
        let reference = references.join(format!("{}.png", case.name));
        if bless || !reference.exists() {
            write_gray(&reference, width, height, &gray);
            continue;
        }
        let (w, h, expected) = grays(&reference);
        let differing = if (w, h) == (width, height) {
            gray.iter()
                .zip(&expected)
                .filter(|(a, b)| a.abs_diff(**b) > LEVELS)
                .count()
        } else {
            usize::MAX
        };
        if differing > gray.len() * ALLOWED / 10_000 {
            let drawn = failed_dir.join(format!("{}.png", case.name));
            write_gray(&drawn, width, height, &gray);
            failures.push(format!(
                "{}: {} pixels differ (drawn: {})",
                case.name,
                if differing == usize::MAX {
                    format!("size {width}x{height} instead of {w}x{h}, all")
                } else {
                    differing.to_string()
                },
                drawn.display()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "screens changed; if meant, run with PAPERPAW_BLESS=1:\n{}",
        failures.join("\n")
    );
}

#[test]
fn screens_say_what_they_show() {
    let case = Case {
        name: "describe",
        profile: "boox-go-7",
        book: None,
        sets: &["slots.core.home=settings.list"],
        script: "",
        colour: 0,
        fixtures: false,
    };
    let library = library(&case);
    let output = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"))
        .args(["--profile", case.profile, "--headless", "--library"])
        .arg(&library)
        .args(["--set", "reader.open-last=false"])
        .args(["--set", case.sets[0]])
        .args([
            "--script",
            "tap-on:Reading,wait:300,tap-on:Display,wait:300,describe",
        ])
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&library);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The pages tapped by their names, the rows with their text and state.
    assert!(text.contains("Heading \"Display\""), "{text}");
    assert!(
        text.contains("Row \"Night mode, White text on black.\" (off)"),
        "{text}"
    );
}

/// The titles of headers stand in their middle by their capitals: the
/// first letter of each (a capital) leaves as much room above it as below
/// (a pixel apart at most; two for a round capital, which overshoots the
/// flat ones by a pixel), whatever the letters after it (a *g* or a *y*
/// hangs below), not as their lines of text would, with more room above
/// the letters. On the X4, the X4 Pro and the Boox, on screens with a back
/// arrow and page arrows. (The library's lists, which also had a note and an
/// empty list, are the native `files` plugin's: the emulator does not register
/// native plugins yet.)
#[test]
fn headers_are_centred_by_their_capitals() {
    let screens = [
        "run:core.settings,wait:300",
        "run:core.settings,wait:300,run:core.confirm,wait:300",
    ];
    let mut failures = Vec::new();
    for profile in ["xteink-x4", "xteink-x4-pro", "boox-go-7"] {
        for (n, script) in screens.iter().enumerate() {
            let case = Case {
                name: "header",
                profile,
                book: None,
                sets: &[],
                script: "",
                colour: 0,
                fixtures: false,
            };
            let library = library(&case);
            let shot = library.join("shot.png");
            let output = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"))
                .args(["--profile", profile, "--headless", "--library"])
                .arg(&library)
                .args(["--screenshot"])
                .arg(&shot)
                .args(["--set", "reader.open-last=false"])
                .args(["--script", &format!("{script},describe")])
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            let (width, _, gray) = grays(&shot);
            let _ = fs::remove_dir_all(&library);
            // `Heading "…" at x,y wxh`
            let at = text
                .lines()
                .find(|l| l.starts_with("Heading "))
                .and_then(|l| l.rsplit_once(" at "))
                .map(|(_, at)| at.to_string())
                .unwrap_or_else(|| panic!("{profile} {script}: no header in {text}"));
            let numbers: Vec<u32> = at
                .split([',', 'x', ' '])
                .map(|n| n.trim().parse().unwrap())
                .collect();
            let [x, y, w, h] = numbers[..] else {
                panic!("{at}");
            };
            // Each row's dark pixels; a row dark across the header is its
            // line below, the header's end.
            let dark = |row: u32| {
                (x..x + w)
                    .filter(|col| gray[(row * width + col) as usize] < 128)
                    .count() as u32
            };
            let end = (y..y + h)
                .find(|row| dark(*row) >= w * 9 / 10)
                .unwrap_or(y + h);
            // The title's first letter: the first columns with ink up to
            // one without, after the back arrow where touch shows one.
            let inked = |col: u32| (y..end).any(|row| gray[(row * width + col) as usize] < 128);
            let run = |from: u32| {
                let start = (from..x + w).find(|col| inked(*col))?;
                Some((
                    start,
                    (start..x + w).find(|col| !inked(*col)).unwrap_or(x + w),
                ))
            };
            let first_run = run(x);
            let letter = if profile == "xteink-x4" {
                first_run
            } else {
                first_run.and_then(|(_, stop)| run(stop))
            };
            let Some((start, stop)) = letter else {
                failures.push(format!("{profile} screen {n}: no ink in the title"));
                continue;
            };
            // Where the next letter touches the capital (a fitted *B* and
            // a *y* do), the run is more than the capital: its own
            // columns are those inked along its top row, from the first
            // inked one. The next letter's descender is not in them.
            let ink_at = |row: u32, col: u32| gray[(row * width + col) as usize] < 128;
            let top = (y..end).find(|row| (start..stop).any(|col| ink_at(*row, col)));
            let (start, stop) = top
                .and_then(|top| {
                    let from = (start..stop).find(|col| ink_at(top, *col))?;
                    let to = (from..stop).find(|col| !ink_at(top, *col)).unwrap_or(stop);
                    Some((from, to))
                })
                .unwrap_or((start, stop));
            let rows: Vec<u32> = (y..end)
                .filter(|row| (start..stop).any(|col| ink_at(*row, col)))
                .collect();
            let (first, last) = (rows[0], rows[rows.len() - 1]);
            let above = first - y;
            let below = end - last - 1;
            // A round capital (an *S*) overshoots the height of the flat
            // ones by a pixel, and the baseline's pixel may take that from
            // one side only: one more pixel apart.
            let round = text
                .lines()
                .find_map(|l| l.strip_prefix("Heading \""))
                .is_some_and(|t| t.starts_with(['C', 'G', 'J', 'O', 'Q', 'S', 'U']));
            if above.abs_diff(below) > 1 + u32::from(round) {
                failures.push(format!(
                    "{profile} screen {n} ({script}): {above} px above the capital, {below} below"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
