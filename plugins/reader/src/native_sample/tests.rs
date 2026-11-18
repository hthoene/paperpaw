use super::*;
use crate::native_draw::Ink;
use crate::native_typeset::Role;
use alloc::string::ToString;
use alloc::vec::Vec;

/// Every character is half the size wide; a line is the size high.
struct Fixed;

impl Measure for Fixed {
    fn width(&self, _: Role, size: f32, text: &str) -> Result<f32, Error> {
        Ok(size * 0.5 * text.chars().count() as f32)
    }
    fn metrics(&self, _: Role, size: f32) -> Result<(f32, f32), Error> {
        Ok((size * 0.8, size * 0.2))
    }
}

#[derive(Default)]
struct Recorder {
    words: Vec<(f32, i32, alloc::string::String)>,
}

impl Ink for Recorder {
    fn run(&mut self, _: Role, size: f32, x: f32, baseline: i32, text: &str) -> Result<f32, Error> {
        self.words.push((x, baseline, text.to_string()));
        Ok(size * 0.5 * text.chars().count() as f32)
    }
    fn fill(&mut self, _: i32, _: i32, _: u32, _: u32) -> Result<(), Error> {
        Ok(())
    }
    fn image(&mut self, _: u32, _: i32, _: i32, _: u32, _: u32) -> Result<(), Error> {
        Ok(())
    }
}

const ONE: &str = "Alice was beginning to get very tired of sitting by her sister on the bank.";
const TWO: &str = "And what is the use of a book?";

#[test]
fn the_sample_is_a_fifth_of_the_display_high() {
    assert_eq!(height(800), 160);
    assert_eq!(height(1872), 374);
    assert_eq!(height(4), 0);
}

#[test]
fn the_text_has_the_side_margins_of_a_page_and_two_millimetres_above() {
    // 4 mm at 254 dpi are 40 pixels; 2 mm are 20.
    let reading = Reading::new(12, 120, 100, 4, 4, true).with_status(90, 90);
    let room = room((480, 160), (480, 800, 254), &reading).unwrap();
    assert_eq!(
        (room.x, room.y, room.width, room.height),
        (40, 20, 400, 120)
    );
    // A frame narrower than the display: the text stays inside it.
    let narrow = room_of((300, 160), &reading);
    assert_eq!((narrow.x, narrow.width), (40, 220));
    // No room for a line: nothing is drawn.
    assert!(super::room((480, 40), (480, 800, 254), &reading).is_none());
    assert!(super::room((90, 160), (480, 800, 254), &reading).is_none());
    // Margins of none, as a page has them.
    let none = room_of((480, 160), &Reading::new(12, 120, 100, 0, 0, false));
    assert_eq!((none.x, none.width), (0, 480));
}

fn room_of(frame: (u32, u32), reading: &Reading) -> native_pages::Box {
    room(frame, (480, 800, 254), reading).unwrap()
}

#[test]
fn the_paragraphs_are_blocks_one_after_the_other() {
    let text = SampleText::of(&["One two.", "Three."]).unwrap();
    assert_eq!(text.paragraph(3), Some((0, 9, "One two.")));
    assert_eq!(text.paragraph(9), Some((9, 15, "Three.")));
    assert_eq!(text.paragraph(15), None);
    let block = text
        .block_at(&NoStorage, Locator::new(0, 0))
        .unwrap()
        .unwrap();
    assert_eq!(
        (block.text.as_str(), block.start, block.end),
        ("One two.", 0, 9)
    );
    assert!(
        text.block_at(&NoStorage, Locator::new(1, 0))
            .unwrap()
            .is_none()
    );
    assert_eq!(text.section_len(0), 15);
    // Inside a letter: no block.
    let text = SampleText::of(&["é"]).unwrap();
    assert_eq!(text.paragraph(1), None);
}

#[test]
fn the_sample_is_laid_out_and_drawn_inside_its_room() {
    let text = SampleText::of(&[ONE, TWO]).unwrap();
    let faces = Faces::new(Fixed, 20.0, 1.0).unwrap();
    let room = native_pages::Box {
        x: 40,
        y: 20,
        width: 400,
        height: 120,
    };
    let page = lay_out(&faces, &text, room, (20.0, true)).unwrap();
    // Six lines of 20 pixels fit: both paragraphs, at most 40 letters a line.
    assert!(
        page.lines.len() >= 3 && page.lines.len() <= 6,
        "{}",
        page.lines.len()
    );
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (40.0, 70.0)).unwrap();
    let said: Vec<&str> = ink.words.iter().map(|(_, _, w)| w.as_str()).collect();
    assert_eq!(said.first(), Some(&"Alice"));
    assert!(said.contains(&"book?"), "{said:?}");
    for (x, baseline, word) in &ink.words {
        let right = x + 10.0 * word.chars().count() as f32;
        assert!(*x >= 40.0 && right <= 440.5, "{word} at {x}");
        assert!(*baseline > 70 && *baseline <= 190, "{word} on {baseline}");
    }
}

#[test]
fn a_larger_size_shows_less_of_the_sample() {
    let text = SampleText::of(&[ONE, TWO]).unwrap();
    let room = native_pages::Box {
        x: 0,
        y: 0,
        width: 400,
        height: 120,
    };
    let words = |base: f32| {
        let faces = Faces::new(Fixed, base, 1.0).unwrap();
        let page = lay_out(&faces, &text, room, (base, false)).unwrap();
        page.lines
            .iter()
            .map(|line| line.text.split(' ').count())
            .sum::<usize>()
    };
    assert!(words(40.0) < words(20.0));
}

#[test]
fn every_allocation_of_the_text_and_its_blocks_may_be_refused() {
    crate::fail_alloc::sweep(|| SampleText::of(&[ONE, TWO]).map(|_| ()));
    let text = SampleText::of(&[ONE, TWO]).unwrap();
    crate::fail_alloc::sweep(|| {
        text.block_at(&NoStorage, Locator::new(0, 0))
            .map(|_| ())
            .map_err(|error| match error {
                DocumentError::OutOfMemory => Error::NoMemory,
                _ => Error::Invalid,
            })
    });
}
