//! The pages counted in the background are not kept while a font pack
//! on the card cannot be read: the letters it cannot measure are 0 wide,
//! and a page index of them would send every later page to the wrong
//! place. The step is made again; after a few in a row the count is
//! given up (the pages already counted are used).

use super::*;
use paperpaw_sdk::FontFace;
use paperpaw_text::{Glyph, LineMetrics, PackBuilder};

/// Where the pack is on the card.
const PACK: &str = "fonts/flaky.ppfp";

/// A font of that pack, read in a family of its own.
struct Flaky;

impl Plugin for Flaky {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("flaky"), "Flaky", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_font_face(FontFace::new(
            Id::new_static("flaky.regular"),
            PACK,
            "Flaky",
            false,
            false,
        ))
    }
}

/// A pack of the printable ASCII characters, 5 or 6 wide at 10 pixels,
/// and of enough others with advances of their own that it reads them
/// when asked rather than keeping them all.
fn pack() -> Vec<u8> {
    let mut builder = PackBuilder::new();
    let others = '\u{100}'..='\u{9000}';
    let glyphs = (' '..='~')
        .chain(others)
        .map(|c| (c, Glyph::empty(5.0 + f32::from(c as u8 % 2))))
        .collect();
    builder.add_strike(10.0, LineMetrics::new(8.0, 2.0, 12.0), glyphs);
    builder.build()
}

/// Whether the card gives the pack up: its reads fail.
fn card(storage: &Shared, out: bool) {
    storage.2.set(out.then_some(".ppfp"));
    storage.4.set(None);
}

/// Alice read in the pack's font, opened but not yet counted: the first
/// page is laid out, the pages not.
fn opened(out: bool) -> (Reading, paperpaw_core::Effects) {
    let storage = Shared::default();
    let text = include_str!("../../../../samples/library/books/alice-in-wonderland.txt");
    storage.create_dir("books").unwrap();
    storage.write(BOOK, text.as_bytes()).unwrap();
    storage.create_dir("fonts").unwrap();
    storage.write(PACK, &pack()).unwrap();
    let heard = Heard::default();
    let kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .user_config("[core]\nreading-font = Flaky\n")
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(Reader)
        .plugin(Listener(heard.clone()))
        .plugin(Flaky)
        .build()
        .unwrap();
    let mut reading = Reading {
        kernel,
        storage,
        heard,
        now: 0,
        book: BOOK,
    };
    let effects = reading.kernel.start(Instant(0));
    reading.settle(effects);
    // The pack reads what it needs when first asked: out from here on,
    // it reads nothing.
    card(&reading.storage, out);
    reading.now += 10;
    let params = Params::new().with(params::PATH, BOOK);
    let effects = reading
        .kernel
        .open_slot(Instant(reading.now), slots::READER, params);
    (reading, effects)
}

/// One background step: the timer due is run.
fn step(reading: &mut Reading, effects: paperpaw_core::Effects) -> paperpaw_core::Effects {
    let at = effects.wake_at().expect("a step is due");
    reading.now = reading.now.max(at.0);
    reading.kernel.tick(Instant(reading.now))
}

/// The page indexes the reader keeps, by name, with their bytes.
fn kept(reading: &Reading) -> Vec<(String, Vec<u8>)> {
    let dir = format!(
        ".paperpaw/plugins/reader/books/{:016x}",
        paperpaw_sdk::digest::fnv1a64(BOOK)
    );
    let mut found: Vec<_> = indexes(&reading.storage, BOOK)
        .into_iter()
        .map(|name| {
            let bytes = reading.storage.0.get(&format!("{dir}/{name}"));
            (name, bytes.unwrap_or_default().to_vec())
        })
        .collect();
    found.sort();
    found
}

#[test]
fn a_step_made_while_the_card_was_out_is_made_again() {
    // The index of a card that never failed.
    let (mut control, effects) = opened(false);
    control.settle(effects);
    let whole = kept(&control);
    assert!(!whole.is_empty());
    // A card out for a step: nothing is kept of it, and the index that
    // follows is the same.
    let (mut reading, effects) = opened(true);
    let effects = step(&mut reading, effects);
    assert!(kept(&reading).iter().all(|(_, bytes)| bytes.is_empty()));
    card(&reading.storage, false);
    reading.settle(effects);
    assert_eq!(kept(&reading), whole);
}

#[test]
fn the_count_is_given_up_when_the_card_stays_out() {
    let (mut reading, mut effects) = opened(true);
    // Steps in a row (one may be another timer); the card is back, but
    // the count is not taken up again for this layout.
    for _ in 0..8 {
        effects = step(&mut reading, effects);
    }
    assert!(kept(&reading).iter().all(|(_, bytes)| bytes.is_empty()));
    card(&reading.storage, false);
    reading.settle(effects);
    assert!(kept(&reading).iter().all(|(_, bytes)| bytes.is_empty()));
}
