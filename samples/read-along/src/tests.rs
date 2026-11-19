use super::*;

/// The number of records of an answer: its first byte.
fn records(draw: impl FnOnce(&mut LayerAnswer<'_>)) -> u8 {
    let mut out = [0; 128];
    let mut answer = LayerAnswer::new(&mut out).unwrap();
    draw(&mut answer);
    answer.finish().unwrap();
    out[0]
}

#[test]
fn the_words_read_are_one_stretch_of_the_line_and_words_without_a_box_are_left_out() {
    let words = [(3, 20), (30, 12), (0, 0), (60, 15), (80, 9)];
    // Words 1 to 3 are read: the stretch from word 1 to word 3.
    let found = span(words.iter().copied(), |n| (1..4).contains(&n));
    assert_eq!(found, Some((30, 75)));
    // Only a word without a box: nothing to mark.
    assert_eq!(span(words.iter().copied(), |n| n == 2), None);
    assert_eq!(span(words.iter().copied(), |_| false), None);
    assert_eq!(span(core::iter::empty(), |_| true), None);
}

#[test]
fn a_mark_is_one_record_around_or_under_the_words_and_off_is_none() {
    let mark = Mark {
        left: 30,
        right: 75,
        top: 8,
        bottom: 20,
    };
    for (style, rows) in [(Style::Frame, 1), (Style::Underline, 1), (Style::Off, 0)] {
        assert_eq!(
            records(|a| draw(a, style, mark).unwrap()),
            rows,
            "{style:?}"
        );
    }
}

#[test]
fn the_setting_names_the_style_and_each_style_has_its_own_signature() {
    assert_eq!(Style::of("read-along.style.underline"), Style::Underline);
    assert_eq!(Style::of("read-along.style.off"), Style::Off);
    assert_eq!(Style::of("read-along.style.frame"), Style::Frame);
    assert_eq!(Style::of("whatever"), Style::Frame);
    assert_ne!(Style::Frame.signature(), Style::Underline.signature());
    assert!(Style::Off.signature() != 0);
}
