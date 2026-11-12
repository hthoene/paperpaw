use super::*;
use alloc::string::ToString;
use alloc::vec;
use paperpaw_sdk::{Color, Rect, document::Locator};

fn word(text: &str, (section, offset): (u32, u32), (x, y): (u16, u16)) -> WordBox {
    WordBox::new(
        Rect::new(x, y, 10 * text.len() as u16, 20),
        text.to_string(),
        Locator::new(section, offset),
        Locator::new(section, offset + text.len() as u32),
    )
}

/// "The cat sat" on one line of section 0, "Dogs run" on the next of 1.
fn page() -> Vec<WordBox> {
    vec![
        word("The", (0, 0), (0, 0)),
        word("cat", (0, 4), (40, 0)),
        word("sat", (0, 8), (80, 0)),
        word("Dogs", (1, 0), (0, 30)),
        word("run", (1, 5), (50, 30)),
    ]
}

fn texts(fire: &Fire) -> Vec<(u32, u32, String)> {
    stage::input_decode(&fire.input)
        .unwrap()
        .flatten()
        .filter_map(|r| match r {
            stage::Record::DecorateRun(r) => Some((r.section, r.offset, r.text.to_string())),
            _ => None,
        })
        .collect()
}

fn marks(triples: &[(u16, u16, u16)]) -> Vec<u8> {
    triples
        .iter()
        .flat_map(|(r, f, t)| [r.to_le_bytes(), f.to_le_bytes(), t.to_le_bytes()].concat())
        .collect()
}

fn look(number: u32, role: &str, background: u32, flags: u32) -> stage::Record<'_> {
    stage::Record::DecorateLook(stage::DecorateLook::new(
        number, role, background, 0, 0, 0, 0, flags,
    ))
}

fn reply(records: &[stage::Record<'_>]) -> Vec<u8> {
    let mut out = vec![0; stage::MAX_REPLY];
    let n = stage::reply_encode(records, &mut out).unwrap();
    out.truncate(n);
    out
}

fn asked(words: &[WordBox]) -> (Decorations, Fire) {
    let d = Decorations::new(words).unwrap();
    let fire = d.fire(words, ("Books/a.epub", "en"), 0).unwrap().unwrap();
    (d, fire)
}

#[test]
fn a_page_is_asked_as_one_run_a_section_with_its_place_in_the_book() {
    let words = page();
    let (d, fire) = asked(&words);
    assert_eq!(
        texts(&fire),
        [
            (0, 0, "The cat sat".to_string()),
            (1, 0, "Dogs run".to_string())
        ]
    );
    let mut records = stage::input_decode(&fire.input).unwrap();
    let Some(Ok(stage::Record::DecoratePage(head))) = records.next() else {
        panic!("no head");
    };
    assert_eq!(
        (head.path, head.language, head.runs),
        ("Books/a.epub", "en", 2)
    );
    assert!(
        d.fire(&words, ("Books/a.epub", "en"), fire.first_run + fire.runs)
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_word_broken_over_two_lines_is_asked_once_whole() {
    let words = vec![
        word("an", (0, 0), (0, 0)),
        word("exam-", (0, 3), (30, 0)),
        word("ple", (0, 3), (0, 30)),
        word("here", (0, 11), (40, 30)),
    ];
    let (mut d, fire) = asked(&words);
    assert_eq!(texts(&fire)[0].2, "an example here");
    // "example" is bytes 3..10: both its boxes are marked.
    d.absorb(
        &fire,
        "vocabulary",
        &reply(&[
            look(0, "vocabulary.known", 1, 0),
            stage::Record::DecorateMarks(stage::DecorateMarks::new(0, &marks(&[(0, 3, 10)]))),
        ]),
    );
    let (_, covered) = d.marked(&words).next().unwrap();
    let boxes: Vec<_> = covered.words().map(|w| w.text.as_str()).collect();
    assert_eq!(boxes, ["exam-", "ple"]);
    assert_eq!(
        covered.tapped_text().unwrap(),
        ("example".to_string(), (0, 3), (0, 6))
    );
}

#[test]
fn a_long_section_is_split_into_runs_at_a_word_and_fires_hold_whole_runs() {
    let words: Vec<_> = (0..1_200_u32)
        .map(|n| word("wordy", (0, n * 6), (0, (n % 30) as u16 * 20)))
        .collect();
    let d = Decorations::new(&words).unwrap();
    assert!(d.runs.len() >= 3);
    assert!(
        d.runs
            .iter()
            .all(|r| usize::from(r.len) <= stage::DECORATE_RUN_TEXT_MAX_BYTES)
    );
    let mut from = 0;
    let mut sent = Vec::new();
    while let Some(fire) = d.fire(&words, ("a.txt", "de"), from).unwrap() {
        assert!(fire.input.len() <= stage::MAX_INPUT && fire.runs <= RUNS_PER_FIRE);
        sent.extend(texts(&fire));
        from = fire.first_run + fire.runs;
    }
    assert_eq!(sent.len(), d.runs.len());
    // Each run starts at the place of its first word, all in section 0.
    assert_eq!(sent[1].1, d.runs[1].offset);
    assert!(sent.iter().all(|(s, _, t)| *s == 0 && !t.starts_with(' ')));
}

#[test]
fn marks_are_checked_alone_and_a_bad_row_or_look_drops_its_row() {
    let words = page();
    let (mut d, fire) = asked(&words);
    d.absorb(
        &fire,
        "vocabulary",
        &reply(&[
            look(0, "vocabulary.new", 6, 0),
            // Another plugin's role: dropped with its marks.
            look(1, "dictionary.new", 7, 0),
            // Good, run past the runs sent, empty, past its run's text: one stays.
            stage::Record::DecorateMarks(stage::DecorateMarks::new(
                0,
                &marks(&[(0, 4, 7), (2, 0, 1), (0, 5, 5), (1, 0, 99)]),
            )),
            stage::Record::DecorateMarks(stage::DecorateMarks::new(1, &marks(&[(0, 0, 3)]))),
            // A look never defined, and a length not of whole marks.
            stage::Record::DecorateMarks(stage::DecorateMarks::new(9, &marks(&[(0, 0, 3)]))),
            stage::Record::DecorateMarks(stage::DecorateMarks::new(0, &[0, 0, 0, 0, 3])),
        ]),
    );
    let kept: Vec<_> = d
        .marked(&words)
        .map(|(look, c)| {
            (
                look.role.as_str(),
                c.words().map(|w| w.text.clone()).collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(kept, [("vocabulary.new", vec!["cat".to_string()])]);
    assert_eq!(d.dropped, 3);
    // Only the look a mark uses is kept.
    assert_eq!(d.looks.len(), 1);
}

#[test]
fn a_bound_inside_a_character_drops_the_mark() {
    let words = vec![word("Größe", (0, 0), (0, 0))];
    let (mut d, fire) = asked(&words);
    d.absorb(
        &fire,
        "v",
        &reply(&[
            look(0, "v.x", 1, 0),
            stage::Record::DecorateMarks(stage::DecorateMarks::new(
                0,
                &marks(&[(0, 0, 3), (0, 0, 4)]),
            )),
        ]),
    );
    assert_eq!((d.marks.len(), d.dropped), (1, 1));
}

#[test]
fn a_reply_the_codec_refuses_gives_no_marks() {
    let words = page();
    let (mut d, fire) = asked(&words);
    d.absorb(&fire, "vocabulary", &[3, 1, 2]);
    assert!(d.marks.is_empty());
}

#[test]
fn a_page_keeps_at_most_256_marks_over_all_subscribers() {
    let words: Vec<_> = (0..300_u32)
        .map(|n| word("ab", (0, n * 3), (0, 0)))
        .collect();
    let (mut d, fire) = asked(&words);
    let triples: Vec<_> = (0..200_u16).map(|n| (0, n * 3, n * 3 + 2)).collect();
    for subscriber in ["a", "b"] {
        let role = alloc::format!("{subscriber}.x");
        d.absorb(
            &fire,
            subscriber,
            &reply(&[
                look(0, &role, 1, 0),
                stage::Record::DecorateMarks(stage::DecorateMarks::new(0, &marks(&triples))),
            ]),
        );
    }
    assert_eq!(
        (d.marks.len(), d.dropped),
        (MAX_MARKS, 400 - MAX_MARKS as u32)
    );
}

#[test]
fn a_tap_on_a_mark_that_takes_taps_is_its_plugins() {
    let words = page();
    let (mut d, fire) = asked(&words);
    d.absorb(
        &fire,
        "vocabulary",
        &reply(&[
            look(0, "vocabulary.new", 6, codes::TAPS),
            look(1, "vocabulary.plain", 6, 0),
            stage::Record::DecorateMarks(stage::DecorateMarks::new(0, &marks(&[(1, 0, 4)]))),
            stage::Record::DecorateMarks(stage::DecorateMarks::new(1, &marks(&[(0, 0, 3)]))),
        ]),
    );
    // "The" takes no taps; "Dogs" does.
    assert!(d.tapped(&words, 5, 5).is_none());
    let (look, covered) = d.tapped(&words, 5, 35).unwrap();
    let input = tap_input(look, &covered, "Books/a.epub").unwrap();
    let Some(Ok(stage::Record::DecorateTapped(t))) = stage::input_decode(&input).unwrap().next()
    else {
        panic!("no tap");
    };
    assert_eq!(
        (
            t.role,
            t.text,
            t.path,
            t.section,
            t.offset,
            t.end_section,
            t.end_offset
        ),
        ("vocabulary.new", "Dogs", "Books/a.epub", 1, 0, 1, 4)
    );
    let yes = reply(&[stage::Record::DecorateTaken(stage::DecorateTaken::new(1))]);
    assert!(taken("vocabulary.new", "vocabulary", &yes));
    // Only the role's plugin takes it.
    assert!(!taken("vocabulary.new", "dictionary", &yes));
    let no = reply(&[stage::Record::DecorateTaken(stage::DecorateTaken::new(0))]);
    assert!(!taken("vocabulary.new", "vocabulary", &no));
}

#[derive(Default)]
struct Fills(
    Vec<(i32, i32, u32, u32, Color)>,
    Vec<((i32, i32, u32, u32), u32)>,
);

impl crate::native_marks::Pen for Fills {
    fn fill(&mut self, x: i32, y: i32, w: u32, h: u32, c: Color) -> Result<(), Error> {
        self.0.push((x, y, w, h, c));
        Ok(())
    }

    fn stroke(&mut self, rect: (i32, i32, u32, u32), t: u32, _: Color) -> Result<(), Error> {
        self.1.push((rect, t));
        Ok(())
    }
}

/// 254 dpi: a tenth of a millimetre is a pixel; many grays.
fn gauge() -> crate::native_marks::Gauge {
    crate::native_marks::Gauge::of(254, None, true)
}

#[test]
fn backgrounds_join_a_lines_words_and_the_first_subscriber_is_drawn_last() {
    let words = page();
    let (mut d, fire) = asked(&words);
    d.absorb(
        &fire,
        "a",
        &reply(&[
            look(0, "a.x", 6, 0),
            stage::Record::DecorateMarks(stage::DecorateMarks::new(0, &marks(&[(0, 4, 11)]))),
        ]),
    );
    d.absorb(
        &fire,
        "b",
        &reply(&[
            look(0, "b.x", 1, 0),
            stage::Record::DecorateMarks(stage::DecorateMarks::new(0, &marks(&[(0, 0, 3)]))),
        ]),
    );
    let mut fills = Fills::default();
    draw_behind(&mut fills, gauge(), &d, &words, (100, 200)).unwrap();
    let yellow = codes::tone(6).unwrap().color();
    let light = codes::tone(1).unwrap().color();
    // Half a millimetre (5 px) round each: "cat sat" one box from 40 to 110,
    // drawn after b's "The".
    assert_eq!(
        fills.0,
        [(95, 200, 40, 20, light), (135, 200, 80, 20, yellow)]
    );
}

#[test]
fn few_grays_shade_a_background_in_rows() {
    let words = page();
    let (mut d, fire) = asked(&words);
    d.absorb(
        &fire,
        "a",
        &reply(&[
            look(0, "a.x", 2, 0),
            stage::Record::DecorateMarks(stage::DecorateMarks::new(0, &marks(&[(0, 0, 3)]))),
        ]),
    );
    let mut fills = Fills::default();
    let few = crate::native_marks::Gauge::of(254, Some(4), false);
    draw_behind(&mut fills, few, &d, &words, (0, 0)).unwrap();
    assert!(!fills.0.is_empty() && fills.0.iter().all(|f| f.3 == 1 && f.4 == Color::BLACK));
}

#[test]
fn lines_and_frames_go_over_the_text_four_tenths_thick() {
    let words = page();
    let (mut d, fire) = asked(&words);
    d.absorb(
        &fire,
        "a",
        &reply(&[
            stage::Record::DecorateLook(stage::DecorateLook::new(
                0,
                "a.x",
                0,
                0,
                1,
                0,
                0,
                codes::FRAME | codes::STRUCK,
            )),
            stage::Record::DecorateMarks(stage::DecorateMarks::new(0, &marks(&[(1, 0, 4)]))),
        ]),
    );
    let mut fills = Fills::default();
    draw_over(&mut fills, gauge(), &d, &words, (0, 0)).unwrap();
    // The frame: half a millimetre round "Dogs", 4 px thick.
    assert_eq!(fills.1, [((-5, 30, 50, 20), 4)]);
    // The line through at 3/5 of the height, the line under at the bottom.
    assert!(fills.0.contains(&(0, 30 + 12 - 2, 40, 4, Color::BLACK)));
    assert!(fills.0.iter().any(|f| f.1 + f.3 as i32 == 50));
    let mut nothing = Fills::default();
    draw_behind(&mut nothing, gauge(), &d, &words, (0, 0)).unwrap();
    assert!(nothing.0.is_empty());
}

#[test]
fn a_look_is_the_sdks_decoration_by_the_fixed_codes() {
    let look = Look {
        role: "v.x".to_string(),
        background: 6,
        ink: 0,
        line: 4,
        line_tone: 11,
        thickness: 3,
        flags: (codes::FRAME | codes::TAPS) as u8,
    };
    let deco = decoration(&look);
    use paperpaw_sdk::decorations::{LineKind, Tone};
    assert_eq!(
        (
            deco.background,
            deco.ink,
            deco.line,
            deco.line_tone,
            deco.thickness,
            deco.frame,
            deco.struck
        ),
        (
            Some(Tone::Yellow),
            None,
            Some(LineKind::Wavy),
            Some(Tone::Red),
            3,
            true,
            false
        )
    );
}
