//! The picture home screen with the real kernel and the standard app: it
//! takes the place of the built-in home screen only while it is switched
//! on; the picture is made once for the screen and again when its settings
//! change; the bars show always, never or after a press, and the press
//! that shows them does nothing else.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::rc::Rc;

use common::*;

const WIDE: &[u8] = include_bytes!("data/wide.jpg");
const ON: &str = "[plugins]\npicture-home = on\n";
const DIR: &str = ".paperpaw/plugins/picture-home";
const BAKED: &str = ".paperpaw/plugins/picture-home/background.gray";
const NOTE: &str = ".paperpaw/plugins/picture-home/background.txt";

/// A page with nothing on it but the picture and the bars.
const EMPTY: &str = "page-1 = core.space";

fn storage(extra: &str) -> Shared {
    Shared::memory(
        &format!("{ON}[picture-home]\n{extra}\n"),
        &[("wallpapers/wide.jpg", WIDE)],
    )
}

fn open(device: DeviceInfo, extra: &str) -> App {
    let mut app = App::new(device, storage(extra));
    app.home();
    app
}

/// The baked picture's gray at column `x` of row `y`.
fn baked(app: &App, x: u64, y: u64) -> u8 {
    let mut head = [0u8; 12];
    app.storage.read_at(BAKED, 0, &mut head).unwrap();
    assert_eq!(&head[..4], b"PPGI");
    let width = u64::from(u32::from_le_bytes(head[4..8].try_into().unwrap()));
    let mut pixel = [0u8; 1];
    app.storage
        .read_at(BAKED, 12 + y * width + x, &mut pixel)
        .unwrap();
    pixel[0]
}

#[test]
fn the_built_in_home_screen_stays_until_the_plugin_is_on() {
    let mut app = App::new(X4, Shared::memory("", &[]));
    app.home();
    assert_eq!(app.screen(), "home.dashboard");
    let mut app = open(X4, "");
    assert_eq!(app.screen(), "picture-home.dashboard");
    assert!(app.shows("Start reading"), "{}", app.described());
}

#[test]
fn the_picture_is_made_once_for_the_screen() {
    let app = open(X4, "picture = wide.jpg\nlighten = 0");
    let names = app.storage.names(DIR);
    assert!(names.contains(&"background.gray".to_string()), "{names:?}");
    assert!(!names.contains(&"background.part".to_string()), "{names:?}");
    assert_eq!(app.storage.size(BAKED).unwrap(), 12 + 480 * 800);
    let note = app.storage.text(NOTE).unwrap();
    assert!(note.ends_with("|Fill|11|100|0|false|480x800"), "{note}");
    assert!(note.len() < 100, "{note}");
    // Shown again, restarted: the same file, nothing made anew.
    let before = app.storage.size(BAKED).unwrap();
    let mut app = app.restart();
    app.home();
    assert_eq!(app.storage.size(BAKED).unwrap(), before);
    assert_eq!(app.storage.text(NOTE).unwrap(), note);
}

#[test]
fn fill_cuts_the_sides_and_the_place_says_which() {
    // The picture runs from black on the left to white on the right.
    let left = open(
        X4,
        "picture = wide.jpg\nlighten = 0\nanchor = picture-home.anchor.left",
    );
    assert!(baked(&left, 10, 400) < 40);
    assert!(baked(&left, 479, 400) < 150);
    let right = open(
        X4,
        "picture = wide.jpg\nlighten = 0\nanchor = picture-home.anchor.right",
    );
    assert!(baked(&right, 470, 400) > 232);
    assert!(baked(&right, 0, 400) > 100);
}

#[test]
fn the_whole_picture_has_white_above_and_below() {
    let app = open(
        X4,
        "picture = wide.jpg\nlighten = 0\nfit = picture-home.fit.fit",
    );
    assert_eq!(baked(&app, 10, 5), 255);
    assert_eq!(baked(&app, 10, 794), 255);
    assert!(baked(&app, 3, 400) < 24);
    assert!(baked(&app, 470, 400) > 232);
}

#[test]
fn a_tile_repeats() {
    let app = open(
        X4,
        "picture = wide.jpg\nlighten = 0\nfit = picture-home.fit.tile\nanchor = picture-home.anchor.top-left",
    );
    // Tiles 240 wide: black at the left of each.
    assert!(baked(&app, 3, 40) < 40);
    assert!(baked(&app, 243, 40) < 40);
    assert!(baked(&app, 236, 40) > 200);
}

#[test]
fn lightening_makes_the_picture_paler() {
    let plain = open(
        X4,
        "picture = wide.jpg\nlighten = 0\nanchor = picture-home.anchor.left",
    );
    let pale = open(
        X4,
        "picture = wide.jpg\nlighten = 50\nanchor = picture-home.anchor.left",
    );
    assert!(baked(&pale, 10, 400) >= baked(&plain, 10, 400) + 100);
}

#[test]
fn a_changed_setting_makes_the_picture_anew() {
    let mut app = open(X4, "picture = wide.jpg\nlighten = 0");
    let note = app.storage.text(NOTE).unwrap();
    app.set("picture-home.zoom", "200");
    app.home();
    let again = app.storage.text(NOTE).unwrap();
    assert_ne!(note, again);
    assert!(again.contains("|200|"), "{again}");
}

#[test]
fn taking_the_picture_away_takes_the_made_one_away() {
    let mut app = open(X4, "picture = wide.jpg");
    assert!(app.storage.exists(BAKED));
    app.set("picture-home.picture", "");
    app.home();
    assert!(!app.storage.exists(BAKED));
    assert!(!app.storage.exists(NOTE));
}

#[test]
fn a_picture_that_is_none_leaves_the_page_plain_and_is_not_tried_again() {
    let mut app = open(X4, "picture = missing.jpg");
    assert_eq!(app.screen(), "picture-home.dashboard");
    assert!(!app.storage.exists(BAKED));
    assert!(app.shows("Start reading"), "{}", app.described());
}

/// The bar's look in the tests: a black plate, which shows where the
/// device has no clock or battery to put on it.
const PLATE: &str = "bar.look = picture-home.look.inverted";

/// Whether anything is drawn in the top strip, where the bar goes.
fn ink_at_top(app: &mut App) -> bool {
    (0..480).step_by(2).any(|x| (4..44).any(|y| app.dark(x, y)))
}

#[test]
fn the_bar_shows_always_never_or_after_a_press() {
    let mut on = open(X4, &format!("{EMPTY}\n{PLATE}\nbar = picture-home.bar.on"));
    assert!(ink_at_top(&mut on));
    let mut off = open(X4, &format!("{EMPTY}\n{PLATE}\nbar = picture-home.bar.off"));
    assert!(!ink_at_top(&mut off));
    let mut auto = open(
        X4,
        &format!("{EMPTY}\n{PLATE}\nbar = picture-home.bar.auto"),
    );
    assert!(!ink_at_top(&mut auto));
    // The first press only shows it: the focus does not move.
    auto.press("right");
    assert!(ink_at_top(&mut auto));
    // A few seconds later it is gone.
    auto.now += 5000;
    let effects = auto.kernel.tick(Instant(auto.now));
    auto.settle(effects);
    assert!(!ink_at_top(&mut auto));
}

#[test]
fn the_first_press_that_shows_the_bars_does_nothing_else() {
    let mut app = open(X4, "bar = picture-home.bar.auto");
    assert_eq!(app.focused(), None);
    app.press("right");
    assert_eq!(app.focused(), None, "{}", app.described());
    app.press("right");
    assert!(app.focused().is_some(), "{}", app.described());
}

#[test]
fn a_tap_on_nothing_shows_the_bar_on_touch() {
    let mut app = open(
        TOUCH,
        &format!("{EMPTY}\n{PLATE}\nbar = picture-home.bar.auto"),
    );
    assert!(!ink_at_top(&mut app));
    app.tap(Point::new(240, 400));
    assert!(ink_at_top(&mut app));
}

#[test]
fn the_picture_shows_behind_the_page_and_not_while_it_is_arranged() {
    let mut app = open(
        X4,
        &format!(
            "{EMPTY}\nbar = picture-home.bar.off\npicture = wide.jpg\nlighten = 0\nanchor = picture-home.anchor.left"
        ),
    );
    // Black at the picture's left edge.
    assert!(app.dark(10, 300));
    app.run(Id::new_static("home.edit-pages"));
    assert!(!app.dark(10, 300));
}

#[test]
fn a_widget_is_added_to_the_page() {
    let mut app = open(X4, "");
    app.open_screen(
        Id::new_static("picture-home.add"),
        Params::new()
            .with("page", "0")
            .with("kind", "core.widgets.time"),
    );
    assert_eq!(app.screen(), "picture-home.add");
    for _ in 0..30 {
        if app.focused().is_some_and(|f| f.starts_with("Date")) {
            break;
        }
        app.run(actions::DOWN);
    }
    app.run(actions::CONFIRM);
    assert_eq!(app.screen(), "picture-home.dashboard");
    let page = app
        .kernel
        .settings()
        .text("picture-home.page-1")
        .to_string();
    assert!(page.contains("home.date"), "{page}");
}

#[test]
fn the_picture_is_chosen_from_the_folder() {
    let mut app = open(X4, "");
    app.open_screen(Id::new_static("picture-home.pick"), Params::new());
    assert!(app.shows("No picture"), "{}", app.described());
    assert!(app.shows("wide.jpg"), "{}", app.described());
    app.run(actions::DOWN);
    app.run(actions::CONFIRM);
    assert_eq!(
        app.kernel.settings().text("picture-home.picture"),
        "wide.jpg"
    );
}

/// How many different grays the baked picture has along row `y`.
fn grays_in_row(app: &App, y: u64) -> usize {
    let mut seen = std::collections::BTreeSet::new();
    for x in 0..480 {
        seen.insert(baked(app, x, y));
    }
    seen.len()
}

#[test]
fn the_picture_is_made_in_dots_where_it_is_to_be_dithered() {
    let automatic = open(X4, "picture = wide.jpg\nlighten = 0");
    assert!(grays_in_row(&automatic, 400) > 2);
    let grays = open(
        X4,
        "picture = wide.jpg\nlighten = 0\nrender = picture-home.render.grays",
    );
    assert!(grays_in_row(&grays, 400) > 2);
    let dots = open(
        X4,
        "picture = wide.jpg\nlighten = 0\nrender = picture-home.render.dither",
    );
    for y in [10, 200, 400, 600] {
        assert!(grays_in_row(&dots, y) <= 2, "row {y}");
    }
}

#[test]
fn a_picture_with_a_very_long_path_is_made_once_and_shown() {
    let folder = "w".repeat(501);
    let mut app = App::new(
        X4,
        Shared::memory(
            &format!("{ON}[picture-home]\nfolder = {folder}\npicture = wide.jpg\nlighten = 0\n"),
            &[(&format!("{folder}/wide.jpg"), WIDE)],
        ),
    );
    app.home();
    assert_eq!(app.screen(), "picture-home.dashboard");
    assert!(app.storage.read_at(BAKED, 0, &mut [0u8; 4]).is_ok());
    assert!(app.storage.read_at(NOTE, 0, &mut [0u8; 4]).is_ok());
    // Another draw finds it made.
    app.home();
    assert!(app.storage.read_at(NOTE, 0, &mut [0u8; 4]).is_ok());
}

#[test]
fn a_jpeg_with_a_large_block_of_metadata_is_made_as_one_without() {
    let mut with = vec![0xff, 0xd8, 0xff, 0xe1];
    with.extend((40_000u16).to_be_bytes());
    with.extend(std::iter::repeat_n(0u8, 40_000 - 2));
    with.extend_from_slice(&WIDE[2..]);
    let plain = open(X4, "picture = wide.jpg\nlighten = 0");
    let mut app = App::new(
        X4,
        Shared::memory(
            &format!("{ON}[picture-home]\npicture = wide.jpg\nlighten = 0\n"),
            &[("wallpapers/wide.jpg", &with)],
        ),
    );
    app.home();
    for (x, y) in [(10, 400), (240, 400), (470, 100), (240, 700)] {
        assert_eq!(baked(&app, x, y), baked(&plain, x, y), "{x},{y}");
    }
}

/// `jpeg` with an EXIF block that notes `orientation`, as a camera does.
fn with_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
    let mut exif = Vec::new();
    exif.extend(b"Exif\0\0II");
    exif.extend(42u16.to_le_bytes());
    exif.extend(8u32.to_le_bytes());
    exif.extend(1u16.to_le_bytes());
    exif.extend(0x0112u16.to_le_bytes());
    exif.extend(3u16.to_le_bytes());
    exif.extend(1u32.to_le_bytes());
    exif.extend(orientation.to_le_bytes());
    exif.extend([0u8; 2]);
    exif.extend(0u32.to_le_bytes());
    let mut bytes = vec![0xff, 0xd8, 0xff, 0xe1];
    bytes.extend((exif.len() as u16 + 2).to_be_bytes());
    bytes.extend(exif);
    bytes.extend_from_slice(&jpeg[2..]);
    bytes
}

fn stretched(file: &[u8]) -> App {
    let mut app = App::new(
        X4,
        Shared::memory(
            &format!(
                "{ON}[picture-home]\npicture = p.jpg\nlighten = 0\nfit = picture-home.fit.stretch\n"
            ),
            &[("wallpapers/p.jpg", file)],
        ),
    );
    app.home();
    app
}

#[test]
fn a_picture_noted_as_held_upright_is_turned_upright() {
    let plain = stretched(WIDE);
    let turned = stretched(&with_orientation(WIDE, 6));
    // Turned a quarter clockwise: the picture's left edge is at the top.
    for (x, y) in [
        (40u64, 100u64),
        (400, 100),
        (40, 700),
        (400, 700),
        (240, 400),
    ] {
        let (px, py) = (y * 480 / 800, 799 - x * 800 / 480);
        let (a, b) = (baked(&turned, x, y), baked(&plain, px, py));
        assert!(a.abs_diff(b) < 48, "{x},{y}: {a} against {b}");
    }
    // Not the picture as it is stored.
    let differs = [(40u64, 100u64), (400, 100), (40, 700), (400, 700)]
        .iter()
        .any(|&(x, y)| baked(&turned, x, y).abs_diff(baked(&plain, x, y)) > 48);
    assert!(differs);
    // Upside down, and mirrored, without the sides swapped.
    let upside_down = stretched(&with_orientation(WIDE, 3));
    for (x, y) in [(40u64, 100u64), (400, 700), (240, 300)] {
        let (a, b) = (baked(&upside_down, x, y), baked(&plain, 479 - x, 799 - y));
        assert!(a.abs_diff(b) < 48, "{x},{y}: {a} against {b}");
    }
}

#[test]
fn a_gray_file_cut_off_is_made_anew() {
    let app = open(X4, "picture = wide.jpg\nlighten = 0");
    let whole = app.storage.size(BAKED).unwrap();
    let mut half = vec![0u8; whole as usize / 2];
    app.storage.read_at(BAKED, 0, &mut half).unwrap();
    app.storage.write(BAKED, &half).unwrap();
    let mut app = app.restart();
    app.home();
    assert_eq!(app.storage.size(BAKED).unwrap(), whole);
    assert!(baked(&app, 240, 400) > 0);
}

#[test]
fn a_picture_that_cannot_be_read_is_given_up_once() {
    let mut app = App::new(
        X4,
        Shared::memory(
            &format!("{ON}[picture-home]\npicture = bad.jpg\n"),
            &[(
                "wallpapers/bad.jpg",
                b"this is not a picture, and not a short one either",
            )],
        ),
    );
    app.home();
    assert_eq!(app.screen(), "picture-home.dashboard");
    assert!(app.storage.text(NOTE).is_none());
    assert!(app.storage.size(BAKED).is_err());
    assert!(
        app.storage
            .names(DIR)
            .iter()
            .all(|n| !n.starts_with("background.")),
        "{:?}",
        app.storage.names(DIR)
    );
    // It still shows the page, and the widgets.
    assert!(app.shows("Start reading"), "{}", app.described());
}

/// A card that says when a file was changed, and may refuse to keep the
/// note beside the baked picture.
struct Card {
    files: MemoryStorage,
    stamp: std::cell::Cell<i64>,
    refuse_note: std::cell::Cell<bool>,
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.files.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if self.refuse_note.get() && path == NOTE {
            return Err(StorageError::Other(7));
        }
        self.files.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.remove(path)
    }
    fn modified(&self, path: &str) -> Option<i64> {
        (path == "wallpapers/wide.jpg").then(|| self.stamp.get())
    }
}

fn card(config: &str) -> (Rc<Card>, Shared) {
    let card = Rc::new(Card {
        files: MemoryStorage::new().with("wallpapers/wide.jpg", WIDE),
        stamp: std::cell::Cell::new(1),
        refuse_note: std::cell::Cell::new(false),
    });
    let shared = Shared(card.clone());
    shared.set_config(&format!("{ON}[picture-home]\n{config}\n"));
    shared.write(SETUP_DONE, b"").unwrap();
    (card, shared)
}

#[test]
fn a_picture_changed_in_place_is_made_anew_where_the_card_says_when() {
    let (card, shared) = card("picture = wide.jpg\nlighten = 0");
    let mut app = App::new(X4, shared.clone());
    app.home();
    let note = shared.text(NOTE).unwrap();
    card.stamp.set(2);
    let mut app = app.restart();
    app.home();
    assert_ne!(shared.text(NOTE).unwrap(), note);
    assert!(shared.size(BAKED).is_ok());
}

#[test]
fn a_note_that_cannot_be_written_leaves_no_picture_and_no_loop() {
    let (card, shared) = card("picture = wide.jpg\nlighten = 0");
    card.refuse_note.set(true);
    let mut app = App::new(X4, shared.clone());
    app.home();
    assert_eq!(app.screen(), "picture-home.dashboard");
    assert!(
        shared.size(BAKED).is_err(),
        "no picture kept without its note"
    );
    assert!(shared.text(NOTE).is_none());
    assert!(app.shows("Start reading"), "{}", app.described());
}

#[test]
fn what_a_stopped_bake_left_goes_when_the_picture_is_taken_away() {
    let app = open(X4, "picture = wide.jpg");
    for name in ["background.part", "background.turn"] {
        app.storage
            .write(&format!("{DIR}/{name}"), b"left over")
            .unwrap();
    }
    app.storage.remove(BAKED).unwrap();
    app.storage.remove(NOTE).unwrap();
    // The picture is taken away: nothing is kept.
    app.storage
        .set_config(&format!("{ON}[picture-home]\npicture =\n"));
    let mut app = app.restart();
    app.home();
    let names = app.storage.names(DIR);
    assert!(
        names.iter().all(|n| !n.starts_with("background.")),
        "{names:?}"
    );
}
