//! A font plugin installed while a book is open (the store fetches
//! Literata's bold and italic in the background): the book is laid out
//! again with the faces there are now, under the page index of its new
//! key, not left with pages measured for the regular face.

use super::*;
use paperpaw_sdk::{
    ActionSpec, Command, Commands, Env, Font, FontSpec, MemoryStorage as Memory, Priority, Uses,
};
use paperpaw_text::{Glyph, LineMetrics, PackBuilder};

const INSTALL: Id = Id::new_static("installer.install");
const INSTALL_PACK: Id = Id::new_static("installer.install-pack");
const REMOVE_PACK: Id = Id::new_static("installer.remove-pack");

/// A pack of the letters a, e and t, `advance` wide.
fn pack(advance: f32) -> Vec<u8> {
    let mut builder = PackBuilder::new();
    let metrics = LineMetrics::new(16.0, 4.0, 24.0);
    let glyphs = "aet".chars().map(|c| (c, Glyph::empty(advance))).collect();
    builder.add_strike(20.0, metrics, glyphs);
    builder.build()
}

/// A pack of the letters the lean font lacks (b, c, d, ...), 14 wide.
fn letters() -> Vec<u8> {
    let mut builder = PackBuilder::new();
    let metrics = LineMetrics::new(16.0, 4.0, 24.0);
    let glyphs = "bcdfghijklmnopqrsuvwxyz "
        .chars()
        .map(|c| (c, Glyph::empty(14.0)))
        .collect();
    builder.add_strike(20.0, metrics, glyphs);
    builder.build()
}

/// What the lean firmware has of Literata: the regular face alone.
struct Lean;

impl Plugin for Lean {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("fonts-default"),
            "Fonts",
            Version::new(1, 0, 0),
        )
        .uses(Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let font = Font::from_data(pack(10.0), &[]).unwrap();
        let id = Id::new_static("fonts-default.literata");
        r.add_font(FontSpec::new(id.clone(), font).family("Literata", false, false))?;
        r.bind_font(paperpaw_sdk::builtin::fonts::READING, id, Priority::DEFAULT)
    }
}

/// The store, as far as it matters: an action that installs the plugin.
struct Installer;

impl Plugin for Installer {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("installer"),
            "Installer",
            Version::new(0, 1, 0),
        )
        .uses(Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_action(ActionSpec::new(INSTALL, "installer.install").handler(
            |_: &Env<'_>, _, out: &mut Commands| {
                out.push(Command::InstallPlugin(
                    Id::new_static("literata-styles"),
                    String::from("plugins/.styles.part"),
                ));
            },
        ))?;
        r.add_action(
            ActionSpec::new(INSTALL_PACK, "installer.install-pack").handler(
                |_: &Env<'_>, _, out: &mut Commands| {
                    out.push(Command::InstallPlugin(
                        Id::new_static("script-more"),
                        String::from("plugins/.script.part"),
                    ));
                },
            ),
        )?;
        r.add_action(
            ActionSpec::new(REMOVE_PACK, "installer.remove-pack").handler(
                |_: &Env<'_>, _, out: &mut Commands| {
                    out.push(Command::RemovePlugin(Id::new_static("script-more")));
                },
            ),
        )
    }
}

/// The data plugin: a bold face for the reading role.
fn styles() -> Memory {
    Memory::new()
        .with(
            "plugins/.styles.part/plugin.ini",
            "[plugin]\nid = literata-styles\nname = Styles\nversion = 1.0.0\nformats = 3\n\n\
             [family]\nname = Literata\nbold = bold.ppfp\n\n\
             [fonts]\ncore.font-reading-bold = literata-styles.bold\n",
        )
        .with("plugins/.styles.part/fonts/bold.ppfp", pack(12.0))
}

/// A script pack: more letters for Literata, a pack that extends it.
fn script() -> Memory {
    Memory::new()
        .with(
            "plugins/.script.part/plugin.ini",
            "[plugin]\nid = script-more\nname = More letters\nversion = 1.0.0\nformats = 3\n\n\
             [extends]\nmore.ppfp = fonts-default.literata\n",
        )
        .with("plugins/.script.part/fonts/more.ppfp", letters())
}

/// Opens the book on a lean firmware, with the pack waiting on the card.
fn lean_reading() -> Reading {
    let text = include_str!("../../../../samples/library/books/alice-in-wonderland.txt");
    lean_reading_of(BOOK, text.as_bytes())
}

/// The same for the book `bytes` at `book`.
fn lean_reading_of(book: &'static str, bytes: &[u8]) -> Reading {
    let storage = Shared::default();
    storage.create_dir("books").unwrap();
    storage.write(book, bytes).unwrap();
    let stored = styles();
    let more = script();
    for (from, path) in [
        (&stored, "plugins/.styles.part/plugin.ini"),
        (&stored, "plugins/.styles.part/fonts/bold.ppfp"),
        (&more, "plugins/.script.part/plugin.ini"),
        (&more, "plugins/.script.part/fonts/more.ppfp"),
    ] {
        let data = paperpaw_sdk::read_file(from, path, 1 << 16).unwrap();
        storage.create_dir(path.rsplit_once('/').unwrap().0).ok();
        storage.write(path, &data).unwrap();
    }
    let heard = Heard::default();
    let kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .plugin(Lean)
        .plugin(Installer)
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(formats::TestEpub)
        .plugin(Reader)
        .plugin(Listener(heard.clone()))
        .build()
        .unwrap();
    let mut reading = Reading {
        kernel,
        storage,
        heard,
        now: 0,
        book,
    };
    let effects = reading.kernel.start(Instant(0));
    reading.settle(effects);
    reading
}

#[test]
fn a_book_open_when_the_faces_arrive_is_laid_out_again() {
    let mut reading = lean_reading();
    reading.open();
    assert!(
        reading
            .kernel
            .fonts()
            .bound("core.font-reading-bold")
            .is_none()
    );
    let before = indexes(&reading.storage, BOOK);
    assert_eq!(before.len(), 1, "{before:?}");
    // The pack arrives while the book is on the screen.
    reading.now += 100;
    let effects = reading.kernel.run_action(Instant(reading.now), INSTALL);
    reading.settle(effects);
    assert!(
        reading
            .kernel
            .fonts()
            .bound("core.font-reading-bold")
            .is_some()
    );
    assert_eq!(
        reading.kernel.active_screen().map(Id::as_str),
        Some("reader.screen")
    );
    // Its pages are counted anew, under the key of the new faces.
    let after = indexes(&reading.storage, BOOK);
    assert_eq!(after.len(), 2, "{after:?}");
    assert!(after.iter().any(|name| !before.contains(name)));
}

/// A pack of letters for a font, installed after the book was opened: the
/// letters it adds measure otherwise, so the pages are counted anew, not
/// taken from the count made without it (the count said 2 pages for a
/// book of one: the second never came).
#[test]
fn a_script_pack_that_arrives_makes_the_pages_be_counted_anew() {
    let mut reading = lean_reading();
    reading.open();
    let before = indexes(&reading.storage, BOOK);
    assert_eq!(before.len(), 1, "{before:?}");
    reading.now += 100;
    let effects = reading
        .kernel
        .run_action(Instant(reading.now), INSTALL_PACK);
    reading.settle(effects);
    assert!(reading.kernel.fonts().contains("script-more.more"));
    assert_eq!(
        reading.kernel.active_screen().map(Id::as_str),
        Some("reader.screen")
    );
    let after = indexes(&reading.storage, BOOK);
    assert_eq!(after.len(), 2, "{after:?}");
    assert!(after.iter().any(|name| !before.contains(name)));
}

/// Where the page shown starts, as the reader last said.
fn here(reading: &Reading) -> (u32, u32) {
    let heard = reading.heard.borrow();
    let (_, r) = heard
        .iter()
        .rev()
        .find(|(n, _)| *n == "opened" || *n == "turned")
        .unwrap();
    (r.section, r.offset)
}

/// Turns to the first page, then forward to the last: the start of every
/// page on the way, and the pages the reader says the book has by then.
fn whole_book(reading: &mut Reading) -> (Vec<(u32, u32)>, Option<u32>) {
    for _ in 0..2000 {
        let at = here(reading);
        reading.run(core_actions::PREV_PAGE);
        if here(reading) == at {
            break;
        }
    }
    let mut starts = vec![here(reading)];
    loop {
        reading.run(core_actions::NEXT_PAGE);
        let at = here(reading);
        if starts.last() == Some(&at) {
            break;
        }
        starts.push(at);
        assert!(starts.len() < 2000, "no end to the book");
    }
    // Some more moments for the count to end.
    for _ in 0..40 {
        reading.run(core_actions::NEXT_PAGE);
    }
    let pages = reading.heard.borrow().last().unwrap().1.pages;
    (starts, pages)
}

/// A long book of chapters, a pack that comes and goes: the pages of each
/// layout are counted right (each page after the one before, as many as
/// the count says) and the place read stays.
#[test]
fn a_script_pack_that_comes_and_goes_keeps_a_long_book_whole() {
    let epub = include_bytes!("../../../../samples/library/books/alice-in-wonderland.epub");
    let mut reading = lean_reading_of("books/alice.epub", epub);
    reading.open();
    let (without, counted) = whole_book(&mut reading);
    assert!(without.windows(2).all(|p| p[0] < p[1]), "{without:?}");
    assert_eq!(counted, Some(without.len() as u32));
    // Back to the middle of it.
    for _ in 0..without.len() / 2 {
        reading.run(core_actions::PREV_PAGE);
    }
    let place = here(&reading);
    let kept = reading.kept();
    // The pack arrives: the letters measure otherwise.
    reading.now += 100;
    let effects = reading
        .kernel
        .run_action(Instant(reading.now), INSTALL_PACK);
    reading.settle(effects);
    assert!(reading.kernel.fonts().contains("script-more.more"));
    assert_eq!(reading.kept(), kept, "the place read moved");
    let (with, counted) = whole_book(&mut reading);
    assert!(with.windows(2).all(|p| p[0] < p[1]), "{with:?}");
    assert_eq!(counted, Some(with.len() as u32));
    assert_ne!(with.len(), without.len(), "the pack changes no page");
    // The pack goes again: the count as it was.
    reading.now += 100;
    let effects = reading.kernel.run_action(Instant(reading.now), REMOVE_PACK);
    reading.settle(effects);
    assert!(!reading.kernel.fonts().contains("script-more.more"));
    let (again, counted) = whole_book(&mut reading);
    assert_eq!(again, without);
    assert_eq!(counted, Some(without.len() as u32));
    // Where one was, the page of it: a page that holds the place.
    assert!(without.iter().any(|start| *start <= place));
}
