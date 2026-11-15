use super::*;
use alloc::vec::Vec;
use paperpaw_sdk::{document::TextStyle, text::Around};

#[derive(Debug, Default)]
struct Fake {
    asked: Cell<usize>,
    refuse: Cell<bool>,
}

impl Measure for &Fake {
    fn width(&self, role: Role, size: f32, text: &str) -> Result<f32, Error> {
        self.asked.set(self.asked.get() + 1);
        if self.refuse.get() {
            return Err(Error::State);
        }
        // Bold is wider; every character half a size.
        let bold = if matches!(role, Role::Bold | Role::BoldItalic) {
            1.5
        } else {
            1.0
        };
        Ok(size * 0.5 * bold * text.chars().count() as f32)
    }

    fn metrics(&self, _: Role, size: f32) -> Result<(f32, f32), Error> {
        if self.refuse.get() {
            return Err(Error::State);
        }
        Ok((size * 0.8, size * 0.25))
    }
}

fn face(style: TextStyle, heading: u8, size: u16) -> Face {
    Face::new(style, heading, size)
}

#[test]
fn faces_are_set_in_their_roles() {
    let plain = face(TextStyle::REGULAR, 0, 0);
    assert_eq!(Role::of(plain), Role::Reading);
    // A heading is bold where its style says so, not by its level.
    assert_eq!(Role::of(face(TextStyle::REGULAR, 2, 0)), Role::Reading);
    assert_eq!(Role::of(face(TextStyle::BOLD, 2, 0)), Role::Bold);
    assert_eq!(Role::of(face(TextStyle::BOLD, 0, 0)), Role::Bold);
    assert_eq!(Role::of(face(TextStyle::ITALIC, 0, 0)), Role::Italic);
    assert_eq!(
        Role::of(face(TextStyle::BOLD_ITALIC, 0, 0)),
        Role::BoldItalic
    );
    assert_eq!(Role::BoldItalic.key(), "role:reading-bold-italic");
    assert_eq!(Role::Reading.key(), "role:reading");
}

#[test]
fn sizes_follow_the_heading_or_the_books_percent() {
    let fake = Fake::default();
    let faces = Faces::new(&fake, 20.0, 1.0).unwrap();
    assert!((faces.size(face(TextStyle::REGULAR, 0, 0)) - 20.0).abs() < 1e-4);
    assert!((faces.size(face(TextStyle::REGULAR, 0, 150)) - 30.0).abs() < 1e-4);
    assert!((faces.size(face(TextStyle::REGULAR, 1, 0)) - 32.0).abs() < 1e-4);
    assert!((faces.size(face(TextStyle::REGULAR, 9, 0)) - 20.0).abs() < 1e-4);
    // The book's percent wins over a heading's scale.
    assert!((faces.size(face(TextStyle::REGULAR, 1, 50)) - 10.0).abs() < 1e-4);
    assert_eq!(Faces::new(&fake, 0.0, 1.0).unwrap_err(), Error::Invalid);
    assert_eq!(
        Faces::new(&fake, f32::NAN, 1.0).unwrap_err(),
        Error::Invalid
    );
}

#[test]
fn a_width_is_asked_of_the_host_once() {
    let fake = Fake::default();
    let faces = Faces::new(&fake, 20.0, 1.0).unwrap();
    let plain = face(TextStyle::REGULAR, 0, 0);
    assert!((faces.advance(plain, 'a') - 10.0).abs() < 1e-4);
    assert!((faces.advance(plain, 'a') - 10.0).abs() < 1e-4);
    assert_eq!(fake.asked.get(), 1);
    // Another face of the same letter is another measure.
    assert!((faces.advance(face(TextStyle::BOLD, 0, 0), 'a') - 15.0).abs() < 1e-4);
    assert_eq!(fake.asked.get(), 2);
    assert!((faces.advance(plain, 'ä') - 10.0).abs() < 1e-4);
    // The default `advance_text` sums the characters.
    let sum = faces.advance_text(plain, "aä", &paperpaw_sdk::text::Around::NONE);
    assert!((sum - 20.0).abs() < 1e-4);
}

#[test]
fn body_lines_keep_the_readers_spacing_and_headings_are_set_by_their_size() {
    let fake = Fake::default();
    // Natural height of 20 px text: 16 + 5 = 21; spacing 1.5 gives 30.
    let faces = Faces::new(&fake, 20.0, 1.5).unwrap();
    let (ascent, height) = faces.line(face(TextStyle::REGULAR, 0, 0));
    assert!((height - 30.0).abs() < 1e-4);
    assert!((ascent - (16.0 + (30.0 - 21.0) / 2.0)).abs() < 1e-4);
    // The book's percent scales the reader's values.
    let (ascent, height) = faces.line(face(TextStyle::REGULAR, 0, 50));
    assert!((height - 15.0).abs() < 1e-4 && (ascent - 10.25).abs() < 1e-4);
    // Spacing below 1: that much of the font's own distance.
    let tight = Faces::new(&fake, 20.0, 0.5).unwrap();
    assert!((tight.line(face(TextStyle::REGULAR, 0, 0)).1 - 10.5).abs() < 1e-4);
    // A heading: at least 1.2 of its size, the rest shared above and below.
    let (ascent, height) = faces.line(face(TextStyle::REGULAR, 1, 0));
    // Size 32: natural 33.6 is more than 38.4 / 1.2 = 32: 38.4 wins.
    assert!((height - 38.4).abs() < 1e-3);
    assert!((ascent - (25.6 + (38.4 - 33.6) / 2.0)).abs() < 1e-3);
    assert!(!faces.failed());
    assert!((faces.line_height() - 30.0).abs() < 1e-4);
    // At spacing 1.0 the font's own distance (21) beats the size (20).
    let natural = Faces::new(&fake, 20.0, 1.0).unwrap();
    assert!((natural.line_height() - 21.0).abs() < 1e-4);
}

#[test]
fn a_refusal_is_noted_and_the_layout_goes_on() {
    let fake = Fake::default();
    let faces = Faces::new(&fake, 20.0, 1.0).unwrap();
    fake.refuse.set(true);
    let plain = face(TextStyle::REGULAR, 0, 0);
    assert!(faces.advance(plain, 'q').abs() < 1e-6);
    assert!(faces.failed());
    let (ascent, height) = faces.line(face(TextStyle::REGULAR, 2, 0));
    assert!(ascent > 0.0 && height > ascent);
    assert!(faces.take_failed());
    assert!(!faces.failed());
    // A refused width is not kept: the next ask goes to the host again.
    fake.refuse.set(false);
    assert!((faces.advance(plain, 'q') - 10.0).abs() < 1e-4);
}

#[test]
fn the_tables_find_what_they_keep_and_nothing_else() {
    let tables = Tables::new(Letters::DEFAULT).unwrap();
    for c in ['a', 'Z', ' ', '\u{e4}', '\u{2019}'] {
        assert_eq!(tables.width(Role::Reading, 20.0, c), None);
        tables.keep_width(Role::Reading, 20.0, c, 7.25);
        assert_eq!(tables.width(Role::Reading, 20.0, c), Some(7.25));
        // Another size or role is another context.
        assert_eq!(tables.width(Role::Reading, 21.0, c), None);
        assert_eq!(tables.width(Role::Bold, 20.0, c), None);
    }
    let av = ('A', 'V');
    assert_eq!(tables.pair(Role::Italic, 20.0, av), None);
    tables.keep_pair(Role::Italic, 20.0, av, -1.5);
    assert_eq!(tables.pair(Role::Italic, 20.0, av), Some(-1.5));
    // A pair of no kerning is known as well, and another pair is not.
    tables.keep_pair(Role::Italic, 20.0, ('V', 'A'), 0.0);
    assert_eq!(tables.pair(Role::Italic, 20.0, ('V', 'A')), Some(0.0));
    assert_eq!(tables.pair(Role::Italic, 20.0, ('A', 'A')), None);
    assert_eq!(tables.pair(Role::Italic, 21.0, av), None);
    // A pair found to have none after it had some is none.
    tables.keep_pair(Role::Italic, 20.0, av, 0.0);
    assert_eq!(tables.pair(Role::Italic, 20.0, av), Some(0.0));
}

#[test]
fn tables_with_no_room_keep_nothing_and_every_ask_goes_to_the_host() {
    let none = Tables::default();
    none.keep_width(Role::Reading, 20.0, 'a', 3.0);
    assert_eq!(none.width(Role::Reading, 20.0, 'a'), None);
    none.keep_pair(Role::Reading, 20.0, ('A', 'V'), 2.0);
    assert_eq!(none.pair(Role::Reading, 20.0, ('A', 'V')), None);
    let fake = Fake::default();
    let faces = Faces::with_tables(&fake, 20.0, 1.0, none).unwrap();
    let plain = face(TextStyle::REGULAR, 0, 0);
    let before = fake.asked.get();
    assert!((faces.advance(plain, 'a') - 10.0).abs() < 1e-4);
    assert!((faces.advance(plain, 'a') - 10.0).abs() < 1e-4);
    assert_eq!(fake.asked.get(), before + 2);
}

#[test]
fn a_context_is_emptied_for_a_fifth_one_and_a_kerned_pair_pushed_out_is_asked_again() {
    let tables = Tables::new(Letters::DEFAULT).unwrap();
    for size in [10.0, 11.0, 12.0, 13.0] {
        tables.keep_width(Role::Reading, size, 'a', size);
    }
    // The least used (10) goes for a fifth.
    assert_eq!(tables.width(Role::Reading, 11.0, 'a'), Some(11.0));
    tables.keep_width(Role::Reading, 14.0, 'a', 14.0);
    assert_eq!(tables.width(Role::Reading, 10.0, 'a'), None);
    assert_eq!(tables.width(Role::Reading, 14.0, 'a'), Some(14.0));
    assert_eq!(tables.width(Role::Reading, 11.0, 'a'), Some(11.0));
    // Many kerned pairs of one context: what is found is always what was kept.
    let letters: Vec<char> = ('a'..='z').chain('A'..='Z').collect();
    for (n, &l) in letters.iter().enumerate() {
        for &r in &letters {
            tables.keep_pair(Role::Reading, 14.0, (l, r), (n as f32 + 1.0) / 8.0);
        }
    }
    for (n, &l) in letters.iter().enumerate() {
        for &r in &letters {
            if let Some(kern) = tables.pair(Role::Reading, 14.0, (l, r)) {
                assert!(
                    (kern - (n as f32 + 1.0) / 8.0).abs() < 1e-3,
                    "{l}{r}: {kern}"
                );
            }
        }
    }
}

#[test]
fn what_is_not_drawn_takes_no_room_and_spaces_follow_the_word_spacing() {
    let fake = Fake::default();
    let faces = Faces::new(&fake, 20.0, 1.0).unwrap().word_spacing(1.5);
    let plain = face(TextStyle::REGULAR, 0, 0);
    assert!(faces.advance(plain, '\u{200b}').abs() < 1e-6);
    assert!(faces.advance(plain, '\u{feff}').abs() < 1e-6);
    assert!((faces.advance(plain, ' ') - 15.0).abs() < 1e-4);
    assert!((faces.advance(plain, 'a') - 10.0).abs() < 1e-4);
    // Out of range or not a number: kept to what the setting allows.
    let wide = Faces::new(&fake, 20.0, 1.0).unwrap().word_spacing(9.0);
    assert!((wide.advance(plain, ' ') - 20.0).abs() < 1e-4);
    let nan = Faces::new(&fake, 20.0, 1.0).unwrap().word_spacing(f32::NAN);
    assert!((nan.advance(plain, ' ') - 10.0).abs() < 1e-4);
}

#[test]
fn a_small_capital_is_measured_as_a_smaller_capital() {
    let fake = Fake::default();
    let faces = Faces::new(&fake, 20.0, 1.0).unwrap();
    let small = face(TextStyle::SMALL_CAPS, 0, 0);
    // Half a size a letter: a capital 10, a small one 0.78 of that.
    assert!((faces.advance(small, 'a') - 7.8).abs() < 1e-4);
    assert!((faces.advance(small, 'A') - 10.0).abs() < 1e-4);
    assert!((faces.advance(face(TextStyle::REGULAR, 0, 0), 'a') - 10.0).abs() < 1e-4);
}

/// Every letter half a size, `AV` a tenth of the size closer, `ﬁ` 0.6 of it.
#[derive(Debug, Default)]
struct Kerned;

impl Measure for Kerned {
    fn width(&self, _: Role, size: f32, text: &str) -> Result<f32, Error> {
        let mut width = 0.0;
        let mut before = None;
        for c in text.chars() {
            width += if c == '\u{fb01}' {
                size * 0.6
            } else {
                size * 0.5
            };
            if before == Some('A') && c == 'V' {
                width -= size * 0.1;
            }
            before = Some(c);
        }
        Ok(width)
    }

    fn metrics(&self, _: Role, size: f32) -> Result<(f32, f32), Error> {
        Ok((size * 0.8, size * 0.25))
    }

    fn covers(&self, _: Role, c: char) -> bool {
        c == '\u{fb01}'
    }
}

fn kerned(letters: Letters) -> Faces<Kerned> {
    Faces::new(Kerned, 20.0, 1.0).unwrap().with_letters(letters)
}

#[test]
fn a_pairs_kerning_is_its_width_against_its_letters() {
    let plain = face(TextStyle::REGULAR, 0, 0);
    let on = kerned(Letters::new(0, true, true));
    assert!((on.kern(plain, 'A', 'V') + 2.0).abs() < 1e-4);
    assert!(on.kern(plain, 'V', 'A').abs() < 1e-4);
    // No kerning in a space, nor where it is off.
    assert!(on.kern(plain, 'A', ' ').abs() < 1e-4);
    assert!(
        kerned(Letters::new(0, false, true))
            .kern(plain, 'A', 'V')
            .abs()
            < 1e-4
    );
    // A word's width is its letters and its pairs.
    let word = on.advance_text(plain, "AVA", &Around::NONE);
    assert!((word - 28.0).abs() < 1e-4, "{word}");
}

#[test]
fn letter_spacing_follows_each_letter_but_not_a_space() {
    let plain = face(TextStyle::REGULAR, 0, 0);
    let spaced = kerned(Letters::new(10, true, true));
    // 10 percent of the 20 px text, after a 10 px letter.
    assert!((spaced.advance(plain, 'a') - 12.0).abs() < 1e-4);
    assert!((spaced.advance(plain, ' ') - 10.0).abs() < 1e-4);
    let tight = kerned(Letters::new(-5, true, true));
    assert!((tight.advance(plain, 'a') - 9.0).abs() < 1e-4);
    // Kept to what the setting allows.
    assert_eq!(Letters::new(99, true, true).spacing, 20);
    assert_eq!(Letters::new(-99, true, true).spacing, -5);
    // The book's own spacing (a fortieth of the size a step) is added.
    let book = face(TextStyle::REGULAR.with_letter_spacing(4), 0, 0);
    assert!((kerned(Letters::new(0, true, true)).advance(book, 'a') - 12.0).abs() < 1e-4);
}

#[test]
fn a_ligature_is_the_room_its_letters_save_where_the_font_has_it() {
    let plain = face(TextStyle::REGULAR, 0, 0);
    // ﬁ 12 wide against f and i of 10 each.
    assert!((kerned(Letters::new(0, true, true)).kern(plain, 'f', 'i') + 8.0).abs() < 1e-4);
    // Off, spaced (drawn one by one), or a letter the font lacks (fl): none.
    assert!(
        kerned(Letters::new(0, true, false))
            .kern(plain, 'f', 'i')
            .abs()
            < 1e-4
    );
    assert!(
        kerned(Letters::new(5, true, true))
            .kern(plain, 'f', 'i')
            .abs()
            < 1e-4
    );
    assert!(
        kerned(Letters::new(0, true, true))
            .kern(plain, 'f', 'l')
            .abs()
            < 1e-4
    );
    let book = face(TextStyle::REGULAR.with_letter_spacing(2), 0, 0);
    assert!(
        kerned(Letters::new(0, true, true))
            .kern(book, 'f', 'i')
            .abs()
            < 1e-4
    );
}

#[test]
fn a_host_that_refuses_a_pair_is_a_refused_measure() {
    let fake = Fake::default();
    let faces = Faces::new(&fake, 20.0, 1.0)
        .unwrap()
        .with_letters(Letters::DEFAULT);
    fake.refuse.set(true);
    assert!(faces.kern(face(TextStyle::REGULAR, 0, 0), 'a', 'b').abs() < 1e-4);
    assert!(faces.take_failed());
}
