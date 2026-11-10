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

    fn line(&self, _: Role, size: f32) -> Result<(f32, f32), Error> {
        if self.refuse.get() {
            return Err(Error::State);
        }
        Ok((size * 0.8, size * 1.25))
    }
}

fn face(style: TextStyle, heading: u8, size: u16) -> Face {
    Face::new(style, heading, size)
}

#[test]
fn faces_are_set_in_their_roles() {
    let plain = face(TextStyle::REGULAR, 0, 0);
    assert_eq!(Role::of(plain), Role::Reading);
    assert_eq!(Role::of(face(TextStyle::REGULAR, 2, 0)), Role::Bold);
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
    let faces = Faces::new(&fake, 20.0).unwrap();
    assert!((faces.size(face(TextStyle::REGULAR, 0, 0)) - 20.0).abs() < 1e-4);
    assert!((faces.size(face(TextStyle::REGULAR, 0, 150)) - 30.0).abs() < 1e-4);
    assert!((faces.size(face(TextStyle::REGULAR, 1, 0)) - 32.0).abs() < 1e-4);
    assert!((faces.size(face(TextStyle::REGULAR, 9, 0)) - 20.0).abs() < 1e-4);
    assert_eq!(Faces::new(&fake, 0.0).unwrap_err(), Error::Invalid);
    assert_eq!(Faces::new(&fake, f32::NAN).unwrap_err(), Error::Invalid);
}

#[test]
fn a_width_is_asked_of_the_host_once() {
    let fake = Fake::default();
    let faces = Faces::new(&fake, 20.0).unwrap();
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
fn lines_are_measured_in_their_face() {
    let fake = Fake::default();
    let faces = Faces::new(&fake, 20.0).unwrap();
    let (ascent, height) = faces.line(face(TextStyle::REGULAR, 0, 0));
    assert!((ascent - 16.0).abs() < 1e-4 && (height - 25.0).abs() < 1e-4);
    assert!(!faces.failed());
}

#[test]
fn a_refusal_is_noted_and_the_layout_goes_on() {
    let fake = Fake::default();
    let faces = Faces::new(&fake, 20.0).unwrap();
    fake.refuse.set(true);
    let plain = face(TextStyle::REGULAR, 0, 0);
    assert!(faces.advance(plain, 'q').abs() < 1e-6);
    assert!(faces.failed());
    let (ascent, height) = faces.line(plain);
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
