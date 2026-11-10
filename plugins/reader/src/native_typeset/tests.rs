use super::*;
use paperpaw_sdk::document::TextStyle;

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
fn the_table_has_a_slot_for_every_key() {
    for key in [1u64 << 63, u64::MAX, 0x41 | (1 << 63), 0x5_0000_0041] {
        assert!(slot(key) < CACHE_ENTRIES);
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
