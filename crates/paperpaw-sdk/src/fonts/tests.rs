use super::*;
use crate::MemoryStorage;
use paperpaw_text::{Glyph, PackBuilder};

/// A pack holding `chars`, with empty glyphs.
fn pack(chars: &str) -> alloc::vec::Vec<u8> {
    let metrics = LineMetrics::new(8.0, 2.0, 12.0);
    let mut builder = PackBuilder::new();
    let glyphs = chars.chars().map(|c| (c, Glyph::empty(5.0))).collect();
    builder.add_strike(10.0, metrics, glyphs);
    builder.build()
}

/// Kerned pairs and ligatures: what is drawn takes the room measured,
/// which is the characters' advances and their pairs (as a layout
/// adds them up); `ff`, `ffi` and `ffl` are drawn as one only where
/// that takes their room.
#[test]
fn kerned_and_ligated_text_is_drawn_as_measured() {
    use paperpaw_graphics::Framebuffer;
    use paperpaw_platform::PixelFormat;
    let metrics = LineMetrics::new(8.0, 2.0, 12.0);
    let mut builder = PackBuilder::new();
    // f i l 5 3 3; fi narrower than f + i, fl as wide, ff wider than
    // f + f, ffl as wide as its letters, ffi not.
    let advances = "AV5fil\u{fb00}\u{fb01}\u{fb02}\u{fb03}\u{fb04}";
    let glyphs = advances
        .chars()
        .zip([6.0, 6.0, 2.0, 5.0, 3.0, 3.0, 12.0, 7.0, 8.0, 13.0, 13.0])
        .map(|(c, advance)| (c, Glyph::empty(advance)))
        .collect();
    builder.add_strike(10.0, metrics, glyphs);
    builder.kerning(&[('A', 'V', -0.25), ('V', 'A', -0.125)]);
    let mut fonts = FontSet::new(4096);
    let pack = Font::from_data(builder.build(), &[]).unwrap();
    fonts.add(Id::new_static("a.font"), pack);
    let font = fonts.get(0).unwrap();
    let mut frame = Framebuffer::new(200, 20, PixelFormat::Gray8).unwrap();
    let tone = paperpaw_text::ToneCurve::SMOOTH;
    let all = [
        font,
        font.unkerned(),
        font.ligatures(),
        font.unkerned().ligatures(),
    ];
    for handle in all {
        for text in ["AVA", "fi", "ffi", "ffl", "ff5fl", "Af"] {
            let measured = handle.measure(10.0, text);
            let mut canvas = frame.canvas();
            let black = Color::BLACK;
            let drawn = handle.draw_toned(&mut canvas, 10.0, 0.0, 10, text, black, 0, tone);
            assert!(
                (drawn - measured).abs() < 1e-4,
                "{text}: {drawn} or {measured}"
            );
            let chars: Vec<char> = text.chars().collect();
            let summed = chars.iter().map(|c| handle.advance(10.0, *c)).sum::<f32>()
                + chars
                    .windows(2)
                    .map(|w| handle.kern(10.0, w[0], w[1]))
                    .sum::<f32>();
            assert!(
                (summed - measured).abs() < 1e-4,
                "{text}: {summed} or {measured}"
            );
        }
    }
    assert_eq!(font.measure(10.0, "AV"), 9.5);
    assert_eq!(font.unkerned().measure(10.0, "AV"), 12.0);
    // fi as wide as its ligature, only with ligatures.
    assert_eq!(font.measure(10.0, "fi"), 8.0);
    assert_eq!(font.ligatures().measure(10.0, "fi"), 7.0);
    let joined = |text: &str| {
        let handle = font.ligatures();
        handle
            .ligature_at(handle.font, 10.0, text)
            .map(|j| (j.0, j.1))
    };
    assert_eq!(joined("fil"), Some(('\u{fb01}', 2)));
    assert_eq!(joined("fl"), Some(('\u{fb02}', 2)));
    assert_eq!(joined("ffl"), Some(('\u{fb04}', 3)), "13 as f, f and l");
    assert_eq!(joined("ffi"), None, "f alone, then fi: 13 is not 12");
    assert_eq!(joined("ff5"), None, "12 is not 10");
    assert_eq!(joined("f5"), None);
}

/// Any Tabler icon has a name; one the icon font lacks is drawn from
/// an icon pack that extends it, others stay without a drawing.
#[test]
fn icon_packs_add_icons_the_font_lacks() {
    let zoom = crate::icons::char("zoom-in").expect("every Tabler name is known");
    let star = crate::icons::char("star").unwrap();
    let other = crate::icons::char("zoom-out").unwrap();
    let mut packed = String::new();
    packed.push(zoom);
    let storage: Rc<dyn Storage> =
        Rc::new(MemoryStorage::new().with("plugins/icons-zoom/fonts/icons.ppfp", pack(&packed)));
    let mut fonts = Fonts::new(4096);
    let mut shipped = String::new();
    shipped.push(star);
    let icons = Font::from_data(pack(&shipped), &[]).unwrap();
    fonts
        .add(FontSpec::new(Id::new_static("a.icons"), icons))
        .unwrap();
    fonts.propose(roles::ICONS, Id::new_static("a.icons"), Priority::DEFAULT);
    fonts
        .add_extension(
            Id::new_static("icons-zoom.icons"),
            String::from("plugins/icons-zoom/fonts/icons.ppfp"),
            Id::new_static("a.icons"),
        )
        .unwrap();
    assert!(fonts.load(&storage).is_empty());
    let bound = fonts.bound(roles::ICONS.as_str()).unwrap();
    assert!(bound.has_char(star) && bound.has_char(zoom));
    assert!(!bound.has_char(other));
}

#[test]
fn extensions_add_characters_read_from_storage() {
    let storage: Rc<dyn Storage> =
        Rc::new(MemoryStorage::new().with("plugins/cyr/fonts/latin-cyr.ppfp", pack("жщ")));
    let mut fonts = Fonts::new(4096);
    let latin = Font::from_data(pack("ab"), &[]).unwrap();
    fonts
        .add(FontSpec::new(Id::new_static("a.latin"), latin))
        .unwrap();
    let ui = Font::from_data(pack("ab"), &[]).unwrap();
    fonts
        .add(FontSpec::new(Id::new_static("a.ui"), ui))
        .unwrap();
    fonts.propose(roles::READING, Id::new_static("a.latin"), Priority::DEFAULT);
    fonts
        .add_extension(
            Id::new_static("cyr.latin-cyr"),
            String::from("plugins/cyr/fonts/latin-cyr.ppfp"),
            Id::new_static("a.latin"),
        )
        .unwrap();
    assert!(
        !fonts.get("a.latin").unwrap().has_char('ж'),
        "not before it is loaded"
    );
    assert!(fonts.load(&storage).is_empty());
    // The font it extends, and others by way of their fallback.
    assert!(fonts.get("a.latin").unwrap().has_char('ж'));
    assert!(fonts.get("a.ui").unwrap().has_char('щ'));
    assert_eq!(fonts.get("a.ui").unwrap().measure(10.0, "aж"), 10.0);
    assert!(!fonts.get("a.ui").unwrap().has_char('ю'));
}

/// A pack at `size`, its glyphs `advance` wide.
fn pack_at(size: f32, advance: f32) -> alloc::vec::Vec<u8> {
    let metrics = LineMetrics::new(size * 0.8, size * 0.2, size * 1.2);
    let mut builder = PackBuilder::new();
    let glyphs = "ab".chars().map(|c| (c, Glyph::empty(advance))).collect();
    builder.add_strike(size, metrics, glyphs);
    builder.build()
}

/// A pack that extends a font with larger sizes (the firmware leaves
/// the largest out): text asked larger than the font's largest is
/// measured and set in it, smaller text in the font, and the font's
/// largest size is its.
#[test]
fn extensions_add_larger_sizes() {
    let storage: Rc<dyn Storage> =
        Rc::new(MemoryStorage::new().with("plugins/big/fonts/big.ppfp", pack_at(36.0, 20.0)));
    let mut fonts = Fonts::new(4096);
    let base = Font::from_data(pack_at(32.0, 16.0), &[]).unwrap();
    fonts
        .add(FontSpec::new(Id::new_static("a.serif"), base))
        .unwrap();
    fonts
        .add_extension(
            Id::new_static("big.big"),
            String::from("plugins/big/fonts/big.ppfp"),
            Id::new_static("a.serif"),
        )
        .unwrap();
    assert_eq!(fonts.get("a.serif").unwrap().largest_size(), Some(32.0));
    assert!(fonts.load(&storage).is_empty());
    let serif = fonts.get("a.serif").unwrap();
    assert_eq!(serif.largest_size(), Some(36.0));
    assert_eq!(serif.measure(36.7, "ab"), 40.0, "by the larger pack");
    assert_eq!(serif.measure(33.0, "ab"), 32.0, "by the font itself");
    // Its metrics, to a pack's 64th of a pixel.
    assert!((serif.line_metrics(36.0).line_height - 36.0 * 1.2).abs() < 0.1);
}

/// A pack at `sizes` of `chars`, its glyphs `advance` wide.
fn pack_of(chars: &str, sizes: &[f32], advance: f32) -> alloc::vec::Vec<u8> {
    let mut builder = PackBuilder::new();
    for &size in sizes {
        let metrics = LineMetrics::new(size * 0.8, size * 0.2, size * 1.2);
        let glyphs = chars.chars().map(|c| (c, Glyph::empty(advance))).collect();
        builder.add_strike(size, metrics, glyphs);
    }
    builder.build()
}

/// A pack of another script's letters in more sizes than the font has
/// (Arabic in Literata's seven, for the firmware's Literata up to 32 px)
/// adds those letters, not sizes: text larger than the font's largest is
/// not set in it, and the font's largest size stays its own. With a pack
/// of the font's larger sizes beside it, that one sets them, the other
/// script's letters still from their pack.
#[test]
fn packs_of_other_letters_add_no_sizes() {
    let storage: Rc<dyn Storage> = Rc::new(
        MemoryStorage::new()
            .with(
                "plugins/ar/fonts/ar.ppfp",
                pack_of("\u{628}\u{62a}", &[20.0, 32.0, 48.0], 9.0),
            )
            .with("plugins/big/fonts/big.ppfp", pack_of("ab", &[48.0], 20.0)),
    );
    let mut fonts = Fonts::new(4096);
    let base = Font::from_data(pack_of("ab", &[20.0, 32.0], 16.0), &[]).unwrap();
    fonts
        .add(FontSpec::new(Id::new_static("a.serif"), base))
        .unwrap();
    fonts
        .add_extension(
            Id::new_static("ar.ar"),
            String::from("plugins/ar/fonts/ar.ppfp"),
            Id::new_static("a.serif"),
        )
        .unwrap();
    assert!(fonts.load(&storage).is_empty());
    let serif = fonts.get("a.serif").unwrap();
    assert_eq!(serif.largest_size(), Some(32.0));
    assert_eq!(serif.measure(48.0, "ab"), 32.0, "by the font itself");
    assert_eq!(serif.measure(48.0, "\u{628}"), 9.0, "by the Arabic pack");
    fonts
        .add_extension(
            Id::new_static("big.big"),
            String::from("plugins/big/fonts/big.ppfp"),
            Id::new_static("a.serif"),
        )
        .unwrap();
    assert!(fonts.load(&storage).is_empty());
    let serif = fonts.get("a.serif").unwrap();
    assert_eq!(serif.largest_size(), Some(48.0));
    assert_eq!(serif.measure(48.0, "ab"), 40.0, "by the larger pack");
    assert_eq!(serif.measure(48.0, "\u{628}"), 9.0, "by the Arabic pack");
}

/// A family of packs on the storage: its faces read a glyph at a time
/// while it is chosen, freed when another is, forgotten with their
/// plugin.
#[test]
fn families_of_packs_are_chosen_and_removed() {
    let storage: Rc<dyn Storage> = Rc::new(
        MemoryStorage::new()
            .with("plugins/round/fonts/round.ppfp", pack("ab"))
            .with("plugins/round/fonts/round-bold.ppfp", pack("abc")),
    );
    let mut fonts = Fonts::new(4096);
    let builtin = Font::from_data(pack("a"), &[]).unwrap();
    fonts
        .add(FontSpec::new(Id::new_static("a.serif"), builtin).family("Serif", false, false))
        .unwrap();
    fonts.propose(roles::READING, Id::new_static("a.serif"), Priority::DEFAULT);
    for (name, bold) in [("round", false), ("round-bold", true)] {
        let face = FontFace::new(
            Id::parse(&alloc::format!("round.{name}")).unwrap(),
            alloc::format!("plugins/round/fonts/{name}.ppfp"),
            "Round",
            bold,
            false,
        );
        assert!(face.is_pack());
        fonts.add_face(face).unwrap();
    }
    assert_eq!(fonts.families(), ["Serif", "Round"]);
    assert!(
        fonts.get("round.round").is_none(),
        "not before it is chosen"
    );

    fonts.set_family(Some("Round"));
    assert!(fonts.load(&storage).is_empty());
    let reading = fonts.role(roles::READING.as_str()).unwrap();
    assert_eq!(reading.id().as_str(), "round.round");
    assert!(reading.has_char('b') && !reading.has_char('c'));
    let bold = fonts.bound(roles::READING_BOLD.as_str()).unwrap();
    assert!(bold.has_char('c'));
    // No italic: the regular face stands in, not another family's.
    assert!(fonts.bound(roles::READING_ITALIC.as_str()).is_none());

    fonts.set_family(None);
    assert!(fonts.load(&storage).is_empty());
    assert!(fonts.get("round.round").is_none(), "freed");

    fonts.set_family(Some("Round"));
    assert!(fonts.load(&storage).is_empty());
    assert_eq!(fonts.remove_plugin("round"), 2);
    assert_eq!(fonts.families(), ["Serif"]);
    assert_eq!(
        fonts.role(roles::READING.as_str()).unwrap().id().as_str(),
        "a.serif"
    );
}

/// Text that joins or runs right to left is set by the text crate's
/// shaping, measured as drawn; Latin as before.
#[test]
fn right_to_left_text_is_drawn_as_measured() {
    use paperpaw_graphics::Framebuffer;
    use paperpaw_platform::PixelFormat;
    use paperpaw_text::Around;
    // Beh and its forms, 3 wide where it joins on both sides.
    let metrics = LineMetrics::new(8.0, 2.0, 12.0);
    let mut builder = PackBuilder::new();
    let glyphs = [
        ('A', 5.0),
        ('V', 5.0),
        ('\u{628}', 2.0),
        ('\u{fe91}', 2.0),
        ('\u{fe92}', 3.0),
        ('\u{fe90}', 2.0),
    ];
    builder.add_strike(
        10.0,
        metrics,
        glyphs.iter().map(|&(c, a)| (c, Glyph::empty(a))).collect(),
    );
    builder.kerning(&[('A', 'V', -0.25)]);
    let mut fonts = FontSet::new(4096);
    fonts.add(
        Id::new_static("a.font"),
        Font::from_data(builder.build(), &[]).unwrap(),
    );
    let font = fonts.get(0).unwrap();
    let rtl = Around::new(crate::lang::Direction::Rtl, crate::lang::Lang::NONE);
    let beh = "\u{628}\u{628}\u{628}";
    assert_eq!(font.measure_in(10.0, beh, &rtl), 7.0);
    // Joined to a letter before it: the first takes its middle form.
    let after = rtl.with_context(Some('\u{628}'), None);
    assert_eq!(font.measure_in(10.0, beh, &after), 8.0);
    assert_eq!(
        font.measure_in(10.0, "AV", &Around::NONE),
        font.measure(10.0, "AV")
    );
    let mut frame = Framebuffer::new(60, 20, PixelFormat::Gray8).unwrap();
    let tone = paperpaw_text::ToneCurve::SMOOTH;
    let mut canvas = frame.canvas();
    let drawn = font.draw_in(
        &mut canvas,
        10.0,
        0.0,
        10,
        beh,
        Color::BLACK,
        (0, tone),
        &after,
    );
    assert_eq!(drawn, 8.0);
}

/// Fonts for text in more than one font: Latin with punctuation and
/// digits 5 wide, and a pack extending it with Hebrew letters 2 wide and
/// Arabic ones (beh 1 alone, 4 final, 3 in the middle, 2 initial).
fn latin_and_letters(also: &str) -> (Fonts, Rc<dyn Storage>) {
    let metrics = LineMetrics::new(8.0, 2.0, 12.0);
    let mut letters = PackBuilder::new();
    let mut glyphs: alloc::vec::Vec<_> = "שלוםבראתהספרמחירעמודלפינכבגוי"
        .chars()
        .map(|c| (c, Glyph::empty(2.0)))
        .collect();
    glyphs.extend(
        [
            ('\u{628}', 1.0),
            ('\u{fe90}', 4.0),
            ('\u{fe92}', 3.0),
            ('\u{fe91}', 2.0),
        ]
        .map(|(c, a)| (c, Glyph::empty(a))),
    );
    glyphs.extend(also.chars().map(|c| (c, Glyph::empty(2.0))));
    glyphs.sort_by_key(|(c, _)| *c);
    glyphs.dedup_by_key(|(c, _)| *c);
    letters.add_strike(10.0, metrics, glyphs);
    let storage: Rc<dyn Storage> = Rc::new(
        MemoryStorage::new().with("plugins/letters/fonts/latin-letters.ppfp", letters.build()),
    );
    let mut latin = PackBuilder::new();
    let glyphs = "abcdefghijklmnopqrstuvwxyz ()[]:.,-'\"«»?0123456789"
        .chars()
        .map(|c| (c, Glyph::empty(5.0)))
        .collect();
    latin.add_strike(10.0, metrics, glyphs);
    let mut fonts = Fonts::new(4096);
    let font = Font::from_data(latin.build(), &[]).unwrap();
    fonts
        .add(FontSpec::new(Id::new_static("a.latin"), font))
        .unwrap();
    fonts
        .add_extension(
            Id::new_static("letters.latin-letters"),
            String::from("plugins/letters/fonts/latin-letters.ppfp"),
            Id::new_static("a.latin"),
        )
        .unwrap();
    assert!(fonts.load(&storage).is_empty());
    (fonts, storage)
}

/// The parts `text` is set in, from the left: whether the Latin font
/// draws each, and its text.
fn parts_of(
    font: &FontHandle<'_>,
    text: &str,
    around: &paperpaw_text::Around,
) -> alloc::vec::Vec<(bool, String)> {
    let mut parts = alloc::vec::Vec::new();
    let latin = font.pick('a').1;
    font.set_in_parts(text, around, |_, index, part, _| {
        parts.push((index == latin, String::from(part)));
    });
    parts
}

/// A word in quotes or brackets, a colon or a full stop after a Hebrew
/// or Arabic word, digits beside letters: each character in a font that
/// has it, shown in the order the text's direction gives (#33, the
/// sample books showed "???" and lost punctuation).
#[test]
fn text_in_more_than_one_font_is_set_in_parts() {
    use paperpaw_text::Around;
    let (fonts, _storage) = latin_and_letters("");
    let font = fonts.get("a.latin").unwrap();
    let rtl = Around::new(crate::lang::Direction::Rtl, crate::lang::Lang::NONE);
    let ltr = Around::new(crate::lang::Direction::Ltr, crate::lang::Lang::NONE);
    let part = |latin: bool, text: &str| (latin, String::from(text));
    // Brackets mirror inside their part; the parts go right to left.
    assert_eq!(
        parts_of(&font, "(שלום)", &rtl),
        [part(true, ")"), part(false, "שלום"), part(true, "(")]
    );
    assert_eq!(
        parts_of(&font, "'שלום',", &rtl),
        [part(true, "',"), part(false, "שלום"), part(true, "'")]
    );
    // Digits after a hyphen: their own part, left to right (12-ב).
    assert_eq!(
        parts_of(&font, "ב-12", &rtl),
        [part(true, "12"), part(true, "-"), part(false, "ב")]
    );
    assert_eq!(
        parts_of(&font, "a(שלום)", &ltr),
        [part(true, "a("), part(false, "שלום"), part(true, ")")]
    );
    // One font for all: set as before, in one go.
    assert!(parts_of(&font, "שלום", &rtl).is_empty());
    assert!(parts_of(&font, "a(b)", &ltr).is_empty());
    // A character no font has stays where it is: no part of its own.
    assert!(parts_of(&font, "שלום₪", &rtl).is_empty());
    // Widths add up, measured as drawn: 5 + 4 × 2 + 5.
    assert_eq!(font.measure_in(10.0, "(שלום)", &rtl), 18.0);
}

/// Arabic letters in a part join what lies beside the whole text and
/// the parts beside them: beh after a beh is final (4 wide), before a
/// colon too.
#[test]
fn parts_join_what_lies_around_them() {
    use paperpaw_graphics::Framebuffer;
    use paperpaw_platform::PixelFormat;
    use paperpaw_text::Around;
    let (fonts, _storage) = latin_and_letters("");
    let font = fonts.get("a.latin").unwrap();
    let rtl = Around::new(crate::lang::Direction::Rtl, crate::lang::Lang::NONE);
    assert_eq!(font.measure_in(10.0, "\u{628}:", &rtl), 1.0 + 5.0);
    let after = rtl.with_context(Some('\u{628}'), None);
    assert_eq!(font.measure_in(10.0, "\u{628}:", &after), 4.0 + 5.0);
    let mut frame = Framebuffer::new(60, 20, PixelFormat::Gray8).unwrap();
    let tone = paperpaw_text::ToneCurve::SMOOTH;
    let mut canvas = frame.canvas();
    let drawn = font.draw_in(
        &mut canvas,
        10.0,
        0.0,
        10,
        "\u{628}:",
        Color::BLACK,
        (0, tone),
        &after,
    );
    assert_eq!(drawn, 9.0);
}

/// The sample books' lines (scratch EPUBs of #33): every part in a font
/// that draws its characters, drawn as wide as measured; long text and
/// more parts than are kept do no harm.
#[test]
fn sample_lines_are_drawn_in_fonts_that_have_their_letters() {
    use paperpaw_graphics::Framebuffer;
    use paperpaw_platform::PixelFormat;
    use paperpaw_text::Around;
    let lines = [
        "قال: «هذا هو الكتاب (الجزء الأول)»؟",
        "הספר נכתב בשנת 1893 (לפי הלוח הגרגוריאני).",
        "קראתי את Alice in Wonderland ב-12 ימים.",
        "המחיר: 3.50 ₪ [ראו עמוד 7].",
        "באזנינו את 'השמימית' העליונים",
    ];
    let arabic: String = lines[0]
        .chars()
        .filter(|c| !c.is_ascii() && *c != '«' && *c != '»')
        .collect();
    let (fonts, _storage) = latin_and_letters(&arabic);
    let font = fonts.get("a.latin").unwrap();
    let rtl = Around::new(crate::lang::Direction::Rtl, crate::lang::Lang::NONE);
    // Quotes and colons 5 wide, Hebrew letters 2: none lost or missing.
    assert_eq!(
        font.measure_in(10.0, "'השמימית'", &rtl),
        5.0 + 7.0 * 2.0 + 5.0
    );
    assert_eq!(font.measure_in(10.0, "המחיר:", &rtl), 5.0 * 2.0 + 5.0);
    let long = "(ש) ".repeat(40);
    let mut frame = Framebuffer::new(800, 20, PixelFormat::Gray8).unwrap();
    let tone = paperpaw_text::ToneCurve::SMOOTH;
    for (line, base) in lines
        .iter()
        .map(|l| (*l, crate::lang::Direction::Rtl))
        .chain([(long.as_str(), crate::lang::Direction::Rtl)])
        .chain([("The word ספר (sefer), 3.", crate::lang::Direction::Ltr)])
    {
        let around = Around::new(base, crate::lang::Lang::NONE);
        for word in line.split(' ').filter(|w| !w.is_empty()) {
            font.set_in_parts(word, &around, |part_font, _, part, _| {
                for c in part.chars() {
                    assert!(
                        part_font.has_char(c) || !font.has_char(c),
                        "{c:?} of {word:?} in a font without it"
                    );
                }
            });
        }
        let measured = font.measure_in(10.0, line, &around);
        let mut canvas = frame.canvas();
        let drawn = font.draw_in(
            &mut canvas,
            10.0,
            0.0,
            10,
            line,
            Color::BLACK,
            (0, tone),
            &around,
        );
        assert_eq!(drawn, measured, "{line}");
    }
}
