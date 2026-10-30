//! The document model's invariants: offset maps made the way format
//! plugins make them (decoding, dropped bytes, entities) take every
//! character to its source and back; `Block::replace` keeps the text,
//! the styles and the source positions around what it replaced; styles
//! and alignments survive their compact form; reading progress grows
//! with the position and stays within 0 to 1; links with a scheme are
//! external; a document's optional parts default to nothing; a run's
//! inline language resolves within its block and leaves the other bits.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_document::{
    Align, Block, BlockKind, Document, DocumentError, Lang, Link, LinkKind, Locator, MAX_FAMILIES,
    MAX_INLINE_LANGS, Metadata, NoStorage, OffsetMap, Run, Storage, TextStyle, is_external,
};
use proptest::prelude::*;

/// A block as a format plugin makes it: every text character comes from
/// `span` source bytes (an entity, a legacy encoding, `\r\n` made one
/// space) after `dropped` bytes it leaves out. Returns the block and the
/// source offset of each character.
fn decoded(start: u32, chars: &[(char, u32, u32)]) -> (Block, Vec<(usize, u32)>) {
    let mut text = String::new();
    let mut map = OffsetMap::default();
    let mut source = start;
    let mut sources = Vec::new();
    // The last checkpoint, from where text and source go in lockstep.
    let mut lockstep = (0usize, start);
    for &(c, span, dropped) in chars {
        source += dropped;
        let index = text.len();
        if lockstep.1 + (index - lockstep.0) as u32 != source {
            map.checkpoint(index, source);
            lockstep = (index, source);
        }
        sources.push((index, source));
        text.push(c);
        source += span;
    }
    let block = Block::new(BlockKind::Paragraph, start, source, text)
        .map(map)
        .align(Align::Auto);
    (block, sources)
}

fn chars() -> impl Strategy<Value = Vec<(char, u32, u32)>> {
    let c = prop_oneof![
        6 => proptest::char::range('a', 'z'),
        1 => Just(' '),
        1 => Just('ä'),
        1 => Just('€'),
        1 => Just('𝄞'),
    ];
    proptest::collection::vec(
        (c, 1u32..6, prop_oneof![4 => Just(0u32), 1 => 1u32..3]),
        0..30,
    )
}

/// Runs over char boundaries, sorted and not overlapping.
fn runs_for(text: &str, cuts: &[(prop::sample::Index, u8)]) -> Vec<Run> {
    let bounds: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain([text.len()])
        .collect();
    let mut points: Vec<(usize, u8)> = cuts
        .iter()
        .map(|(i, style)| (bounds[i.index(bounds.len())], *style))
        .collect();
    points.sort();
    points.dedup_by_key(|p| p.0);
    points
        .chunks(2)
        .filter(|pair| pair.len() == 2 && pair[0].0 < pair[1].0)
        .map(|pair| {
            Run::new(
                pair[0].0 as u32,
                pair[1].0 as u32,
                TextStyle::from_bits(u32::from(pair[0].1 % 4 + 1)),
            )
        })
        .collect()
}

/// The style of byte `index` straight from the runs.
fn style_of(runs: &[Run], index: usize) -> TextStyle {
    runs.iter()
        .find(|r| r.start as usize <= index && index < r.end as usize)
        .map_or(TextStyle::REGULAR, |r| r.style)
}

fn check_runs(block: &Block) {
    for run in &block.runs {
        assert!(run.start < run.end, "empty run {run:?}");
        assert!(
            run.end as usize <= block.text.len(),
            "{run:?} past {:?}",
            block.text
        );
    }
    for pair in block.runs.windows(2) {
        assert!(pair[0].end <= pair[1].start, "runs overlap: {pair:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn decoded_characters_map_to_their_source_and_back(
        start in 0u32..1000,
        chars in chars(),
        probes in proptest::collection::vec(0u32..1200, 0..20),
    ) {
        let (block, sources) = decoded(start, &chars);
        for &(index, source) in &sources {
            prop_assert_eq!(block.source_offset(index), source, "char at {}", index);
            prop_assert_eq!(block.text_index(source), index, "source {}", source);
        }
        prop_assert_eq!(block.source_offset(block.text.len()), block.end);
        // Any offset gives a character boundary.
        for offset in probes {
            let index = block.text_index(offset);
            prop_assert!(index <= block.text.len() && block.text.is_char_boundary(index));
        }
    }

    /// A position never maps past the character that follows it in the
    /// source, and later positions never map to earlier text (see the
    /// shrunk case below).
    #[test]
    fn positions_map_in_order_and_never_past_the_next_character(
        start in 0u32..1000,
        chars in chars(),
    ) {
        let (block, sources) = decoded(start, &chars);
        for &(index, source) in &sources {
            for offset in start..=source {
                prop_assert!(block.text_index(offset) <= index, "{} past {}", offset, index);
            }
        }
        let mut last = 0;
        for offset in start..=block.end {
            let index = block.text_index(offset);
            prop_assert!(index >= last, "{} went back to {}", offset, index);
            last = index;
        }
    }

    #[test]
    fn styles_are_those_of_the_runs(
        chars in chars(),
        cuts in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 0..8),
    ) {
        let (mut block, _) = decoded(0, &chars);
        block.runs = runs_for(&block.text, &cuts);
        for index in 0..block.text.len() + 2 {
            prop_assert_eq!(block.style_at(index), style_of(&block.runs, index), "{}", index);
        }
    }

    #[test]
    fn replacing_keeps_text_styles_and_positions_around_it(
        chars in chars(),
        cuts in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 0..8),
        range in (any::<prop::sample::Index>(), any::<prop::sample::Index>()),
        with in "[a-zé ]{0,6}",
    ) {
        let (mut block, sources) = decoded(10, &chars);
        block.runs = runs_for(&block.text, &cuts);
        let before = block.clone();
        let bounds: Vec<usize> = sources.iter().map(|s| s.0).chain([block.text.len()]).collect();
        let (a, b) = (range.0.index(bounds.len()), range.1.index(bounds.len()));
        let (from, to) = (bounds[a.min(b)], bounds[a.max(b)]);
        block.replace(from..to, &with);

        let mut expected = before.text.clone();
        expected.replace_range(from..to, &with);
        prop_assert_eq!(&block.text, &expected);
        // Before the replaced text, nothing changed.
        for &(index, source) in sources.iter().filter(|s| s.0 < from) {
            prop_assert_eq!(block.source_offset(index), source);
            prop_assert_eq!(block.text_index(source), index);
            prop_assert_eq!(block.style_at(index), before.style_at(index));
        }
        // After it, everything moved along and keeps its source.
        let shift = |i: usize| i + with.len() - (to - from);
        for &(index, source) in sources.iter().filter(|s| s.0 >= to) {
            prop_assert_eq!(block.source_offset(shift(index)), source, "{}", index);
            // Text inserted before a character shares its source: its
            // position may lead to either.
            if !(from == to && index == to) {
                prop_assert_eq!(block.text_index(source), shift(index), "{}", source);
            }
            prop_assert_eq!(block.style_at(shift(index)), before.style_at(index));
        }
        for run in &block.runs {
            prop_assert!(run.start < run.end && run.end as usize <= block.text.len());
        }
    }

    /// Runs stay sorted and apart when a replacement covers the end of
    /// one and the start of the next.
    #[test]
    fn replacing_keeps_runs_apart(
        chars in chars(),
        cuts in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 0..8),
        range in (any::<prop::sample::Index>(), any::<prop::sample::Index>()),
        with in "[a-z]{0,6}",
    ) {
        let (mut block, sources) = decoded(10, &chars);
        block.runs = runs_for(&block.text, &cuts);
        let bounds: Vec<usize> = sources.iter().map(|s| s.0).chain([block.text.len()]).collect();
        let (a, b) = (range.0.index(bounds.len()), range.1.index(bounds.len()));
        block.replace(bounds[a.min(b)]..bounds[a.max(b)], &with);
        check_runs(&block);
    }

    #[test]
    fn styles_survive_their_bits_and_combine_like_sets(a in 0..0x1000u32, b in 0..0x1000u32, c in 0..0x1000u32) {
        let (a, b, c) = (TextStyle::from_bits(a), TextStyle::from_bits(b), TextStyle::from_bits(c));
        prop_assert_eq!(TextStyle::from_bits(a.bits()), a);
        let ab = a.union(b);
        prop_assert_eq!(ab.bold(), a.bold() || b.bold());
        prop_assert_eq!(ab.italic(), a.italic() || b.italic());
        prop_assert_eq!(ab.small_caps(), a.small_caps() || b.small_caps());
        prop_assert_eq!(ab.superscript(), a.superscript() || b.superscript());
        prop_assert_eq!(ab.underline(), a.underline() || b.underline());
        prop_assert_eq!(ab.strike(), a.strike() || b.strike());
        prop_assert!(!a.without(TextStyle::UNDERLINE).underline());
        prop_assert_eq!(a.without(TextStyle::UNDERLINE).family(), a.family());
        prop_assert_eq!(ab.family(), if b.family() != 0 { b.family() } else { a.family() });
        prop_assert_eq!(a.union(a), a);
        prop_assert_eq!(ab.union(c), a.union(b.union(c)));
        prop_assert!(a.family() <= MAX_FAMILIES);
        for index in [0, 1, MAX_INLINE_LANGS, MAX_INLINE_LANGS + 1, u8::MAX] {
            let with = a.with_lang_index(index).with_family(3);
            prop_assert_eq!(with.lang_index(), if index > MAX_INLINE_LANGS { 0 } else { index });
            prop_assert_eq!(with.family(), 3);
            prop_assert_eq!(with.without(TextStyle::BOLD).lang_index(), with.lang_index());
            prop_assert_eq!(TextStyle::from_bits(with.bits()), with);
            prop_assert_eq!(a.union(with).lang_index(), with.lang_index());
        }
        for family in [0, 1, MAX_FAMILIES, MAX_FAMILIES + 1, u8::MAX] {
            let with = a.with_family(family);
            prop_assert_eq!(with.family(), if family > MAX_FAMILIES { 0 } else { family });
            prop_assert_eq!(
                (with.bold(), with.italic(), with.small_caps()),
                (a.bold(), a.italic(), a.small_caps())
            );
        }
    }

    #[test]
    fn progress_grows_with_the_position(
        lens in proptest::collection::vec(prop_oneof![3 => 0u32..1000, 1 => any::<u32>()], 1..6),
        positions in proptest::collection::vec((any::<prop::sample::Index>(), any::<prop::sample::Index>()), 2..10),
    ) {
        let book = Sections(lens.clone());
        let mut at: Vec<Locator> = positions
            .iter()
            .map(|(s, o)| {
                let section = s.index(lens.len());
                let offset = o.index(lens[section] as usize + 1) as u32;
                Locator::new(section as u32, offset)
            })
            .collect();
        at.sort();
        let mut last = 0.0f32;
        for locator in at {
            let p = book.progress(locator);
            prop_assert!((0.0..=1.0).contains(&p), "{}", p);
            prop_assert!(p >= last, "{:?} at {} after {}", locator, p, last);
            last = p;
        }
        let total: u64 = lens.iter().map(|&l| u64::from(l)).sum();
        if total > 0 {
            prop_assert_eq!(book.progress(Locator::START), 0.0);
            let end = Locator::new(lens.len() as u32 - 1, *lens.last().unwrap());
            prop_assert_eq!(book.progress(end), 1.0);
        }
        // Past the end and in sections that do not exist: at most 1.
        prop_assert!(book.progress(Locator::new(u32::MAX, u32::MAX)) <= 1.0);
    }

    #[test]
    fn links_with_a_scheme_are_external(
        scheme in "[a-zA-Z][a-zA-Z0-9+.-]{1,12}",
        rest in "[^\\x00]{0,20}",
        path in "[a-zA-Z0-9_./#-]{0,20}",
    ) {
        let external = format!("{scheme}:{rest}");
        prop_assert!(is_external(&external), "{}", external);
        prop_assert!(!is_external(&path), "{}", path);
        // A drive letter or a scheme after a path is no scheme.
        let drive = format!("{}:{rest}", &scheme[..1]);
        prop_assert!(!is_external(&drive), "{}", drive);
        let relative = format!("../{scheme}:{rest}");
        prop_assert!(!is_external(&relative), "{}", relative);
    }
}

/// The shrunk case of `replacing_keeps_runs_apart`: "BB" bold and "DD"
/// italic, "BccD" replaced by "X": only the bold run may cover "X".
#[test]
fn replacing_the_end_of_one_run_and_the_start_of_the_next() {
    let (mut block, _) = decoded(0, &[]);
    block.text = String::from("aaBBccDDee");
    block.end = 10;
    block.runs = vec![
        Run::new(2, 4, TextStyle::BOLD),
        Run::new(6, 8, TextStyle::ITALIC),
    ];
    block.replace(3..7, "X");
    assert_eq!(block.text, "aaBXDee");
    check_runs(&block);
}

#[test]
fn replacing_outside_the_text_or_inside_a_character_changes_nothing() {
    let (block, _) = decoded(0, &[('a', 1, 0), ('ä', 1, 0), ('b', 1, 0)]);
    #[allow(
        clippy::reversed_empty_ranges,
        reason = "a range the wrong way round, on purpose"
    )]
    for range in [2..3, 1..2, 9..12, 3..1] {
        let mut changed = block.clone();
        changed.replace(range.clone(), "x");
        assert_eq!(changed, block, "{range:?}");
    }
    // An end past the text is the text's end.
    let mut changed = block.clone();
    changed.replace(1..99, "!");
    assert_eq!(changed.text, "a!");
    assert_eq!(changed.source_offset(1), 1);
}

#[test]
fn alignments_survive_their_bits() {
    for align in [
        Align::Auto,
        Align::Start,
        Align::Center,
        Align::End,
        Align::Justify,
    ] {
        assert_eq!(Align::from_bits(align.bits()), align);
    }
    for unknown in [5, 9, u8::MAX] {
        assert_eq!(Align::from_bits(unknown), Align::Auto);
    }
}

#[test]
fn a_later_checkpoint_at_the_same_index_wins() {
    let mut map = OffsetMap::default();
    map.checkpoint(1, 1);
    map.checkpoint(2, 5);
    map.checkpoint(2, 9);
    let (mut block, _) = decoded(0, &[('a', 1, 0), ('b', 1, 0), ('c', 1, 0)]);
    block.map = map;
    block.end = 10;
    assert_eq!(block.source_offset(2), 9);
    assert_eq!(block.text_index(9), 2);
    assert_eq!(block.text_index(6), 2, "in the gap: the checkpoint");
}

/// "Inside a gap (e.g. a dropped `\r`) map to the checkpoint", the map
/// says, also for a gap before its first checkpoint: in
/// `a<!-- a comment -->b` (text "ab") a position in the comment maps to
/// "b", not past it.
#[test]
fn a_position_in_a_gap_before_the_first_checkpoint_maps_to_the_next_character() {
    let (block, sources) = decoded(0, &[('a', 1, 0), ('b', 1, 19)]);
    assert_eq!(sources, [(0, 0), (1, 20)]);
    assert_eq!(block.text_index(20), 1);
    assert_eq!(block.text_index(10), 1, "in the comment: \"b\"");
}

/// Sections of the given lengths and nothing else.
struct Sections(Vec<u32>);

impl Document for Sections {
    fn metadata(&self) -> &Metadata {
        static METADATA: std::sync::OnceLock<Metadata> = std::sync::OnceLock::new();
        METADATA.get_or_init(Metadata::default)
    }

    fn sections(&self) -> u32 {
        self.0.len() as u32
    }

    fn section_len(&self, section: u32) -> u32 {
        self.0.get(section as usize).copied().unwrap_or(0)
    }

    fn block_at(&self, _: &dyn Storage, _: Locator) -> Result<Option<Block>, DocumentError> {
        Ok(None)
    }
}

#[test]
fn optional_parts_default_to_nothing() {
    let book = Sections(vec![10, 0]);
    let storage = NoStorage;
    let at = Locator::new(0, 3);
    assert_eq!(book.toc_len(&storage), 0);
    assert_eq!(book.toc_entry(&storage, 0), None);
    assert_eq!(book.toc_target(&storage, 0), None);
    assert_eq!(book.page_label(&storage, at), None);
    assert!(!book.whole_pages());
    assert_eq!(book.cover(&storage), Ok(None));
    assert_eq!(book.xpointer(&storage, at), None);
    assert_eq!(book.locate_xpointer(&storage, "/body/p[1]"), None);
    assert!(book.fonts(&storage).is_empty());
    assert!(matches!(
        book.font_data(&storage, 0),
        Err(DocumentError::Malformed(_))
    ));
    assert!(book.links(&storage, 0, 0, 10).is_empty());
    assert_eq!(book.link_target(&storage, "#note"), None);
    // An empty book is at its start everywhere.
    assert_eq!(Sections(vec![0, 0]).progress(Locator::new(1, 5)), 0.0);
    assert_eq!(book.progress(Locator::new(1, 0)), 1.0);
}

#[test]
fn a_run_is_in_its_inline_language_or_the_block_s() {
    let (ar, fa) = (Lang::parse("ar"), Lang::parse("fa"));
    let persian = TextStyle::ITALIC.with_lang_index(1);
    let block = Block::new(BlockKind::Paragraph, 0, 9, String::from("abc def g"))
        .lang(ar)
        .inline_langs(&[fa])
        .runs(vec![
            Run::new(4, 7, persian),
            Run::new(8, 9, TextStyle::REGULAR.with_lang_index(9)),
        ]);
    assert_eq!(block.lang_at(0), ar);
    assert_eq!(block.lang_at(5), fa);
    // An index past the block's list is the block's own language.
    assert_eq!(block.lang_at(8), ar);
    assert_eq!(block.lang_of(persian), fa);
    // At most MAX_INLINE_LANGS languages; none is None.
    let many = [fa; 20];
    let capped = block.clone().inline_langs(&many);
    assert_eq!(
        capped.inline_langs.as_deref().map(<[Lang]>::len),
        Some(usize::from(MAX_INLINE_LANGS))
    );
    assert_eq!(block.inline_langs(&[]).inline_langs, None);
}

#[test]
fn a_link_is_plain_unless_it_says() {
    let link = Link::new(1, 4, "#n1");
    assert_eq!(link.kind, LinkKind::Plain);
    assert_eq!(link.kind(LinkKind::NoteRef).kind, LinkKind::NoteRef);
    let block = Block::new(BlockKind::Paragraph, 0, 0, String::new());
    assert_eq!(block.lang, Lang::NONE);
    assert_eq!(block.drop_cap, 0);
}
