//! The stylesheet parser on hostile and arbitrary input: it never panics,
//! keeps within its bounds, reads a stylesheet the same however it is cut
//! into pieces, and its rules survive the cache (`styles.txt`).

extern crate std;

use alloc::string::String;
use alloc::vec::Vec;

use proptest::prelude::*;

use super::*;

/// `cases` cases, or as many as `PROPTEST_CASES` says, for longer runs.
fn cases(cases: u32) -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(cases);
    ProptestConfig::with_cases(cases)
}

/// Rules of `css` fed in pieces cut at `cuts` (character counts).
fn fed_in_pieces(css: &str, cuts: &[usize]) -> Rules {
    let mut rules = Rules::default();
    let mut parser = Parser::new("OEBPS/").unwrap();
    let chars: Vec<char> = css.chars().collect();
    let mut at = 0;
    for &cut in cuts {
        let end = (at + cut).min(chars.len());
        let piece: String = chars[at..end].iter().collect();
        parser.feed(&piece, &mut rules);
        at = end;
    }
    let rest: String = chars[at..].iter().collect();
    parser.feed(&rest, &mut rules);
    rules
}

fn whole(css: &str) -> Rules {
    let mut rules = Rules::default();
    Parser::new("OEBPS/").unwrap().feed(css, &mut rules);
    rules
}

/// Pieces of CSS, good and bad, to build stylesheets from.
const PIECES: &[&str] = &[
    "p",
    ".c",
    "h1.x",
    "div p",
    "a:hover",
    "*",
    ",",
    "{",
    "}",
    ";",
    ":",
    " ",
    "\n",
    "/*",
    "*/",
    "\"",
    "'",
    "@media screen",
    "@import url(a.css);",
    "@font-face",
    "@charset \"utf-8\";",
    "text-align: center",
    "text-align: right",
    "font-weight: bold",
    "font-style: italic",
    "display: none",
    "text-indent: 99999999999em",
    "text-indent: -3em",
    "margin: 1em 2em 3em",
    "margin-top: 1e30px",
    "padding-bottom: 12pt",
    "margin-left: 1e30%",
    "padding-right: -5em",
    "margin: 0 99999999em 0 inf%",
    "line-height: 1e30",
    "line-height: -2",
    "line-height: 0.0001%",
    "vertical-align: super",
    "text-decoration: underline line-through",
    "font-size: 999999%",
    "font-size: 0.001em",
    "font-size: inf%",
    "font-family: Fancy, serif",
    "font-family: \"A B\"",
    "src: url(f.ttf)",
    "src: url(../../x.woff) format('woff')",
    "!important",
];

fn stylesheet() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(PIECES), 0..80).prop_map(|p| p.concat())
}

proptest! {
    #![proptest_config(cases(256))]

    #[test]
    fn arbitrary_text_never_panics_and_stays_bounded(css in any::<String>()) {
        let rules = whole(&css);
        prop_assert!(rules.rules.len() <= MAX_RULES);
        prop_assert!(rules.families.len() <= usize::from(MAX_FAMILIES));
        let _ = Rules::decode(&rules.encode());
        let _ = Props::decode(&css);
    }

    #[test]
    fn values_stay_within_their_bounds(css in stylesheet()) {
        let rules = whole(&css);
        for (_, props) in &rules.rules {
            prop_assert!(props.indent.is_none_or(|i| (-MAX_INDENT as i16..=MAX_INDENT as i16).contains(&i)));
            for space in [props.margin_top, props.margin_bottom, props.padding_top, props.padding_bottom] {
                prop_assert!(space.is_none_or(|s| s <= MAX_SPACE as u16));
            }
            for side in [props.margin_left, props.margin_right, props.padding_left, props.padding_right] {
                prop_assert!(side.is_none_or(|s| s <= MAX_SIDE as u16));
            }
            prop_assert!(props.line_height.is_none_or(|h| (50..=400).contains(&h)));
            if let Some(size) = props.size {
                let of = size.of(300);
                prop_assert!((MIN_SIZE as u16..=MAX_SIZE as u16).contains(&of));
            }
            prop_assert!(props.family.is_none_or(|f| f <= MAX_FAMILIES));
        }
        for face in &rules.faces {
            prop_assert!(face.family >= 1 && face.family <= MAX_FAMILIES);
            prop_assert!(!face.path.split('/').any(|part| part == ".."), "{}", face.path);
        }
    }

    #[test]
    fn pieces_read_as_the_whole(
        css in stylesheet(),
        cuts in prop::collection::vec(0usize..12, 0..40),
    ) {
        let one = whole(&css);
        let pieces = fed_in_pieces(&css, &cuts);
        prop_assert_eq!(pieces.encode(), one.encode());
        prop_assert_eq!(pieces.faces, one.faces);
    }

    #[test]
    fn rules_survive_the_cache(css in stylesheet()) {
        let rules = whole(&css);
        let copy = Rules::decode(&rules.encode());
        prop_assert_eq!(&copy.rules, &rules.rules);
        prop_assert_eq!(&copy.families, &rules.families);
    }
}

/// The shrunk case of `rules_survive_the_cache`: an unterminated string
/// in `font-family` took the line break after it into the family's name,
/// and the cache (a line per family) split it. A line break now ends a
/// string, and white space in a name is one space.
#[test]
fn family_names_with_line_breaks_survive_the_cache() {
    let rules = whole("{font-family: \"A B\"\n*/}");
    assert!(!rules.families.iter().any(|f| f.contains('\n')));
    let copy = Rules::decode(&rules.encode());
    assert_eq!(copy.families, rules.families);
    // An escaped line break stays in the string, not in the name.
    let rules = whole(".x { font-family: \"A\\\nB\" }");
    assert!(!rules.families.iter().any(|f| f.contains('\n')));
    let copy = Rules::decode(&rules.encode());
    assert_eq!(copy.families, rules.families);
    // What follows the break is read back as a line of its own, here a
    // rule the stylesheet does not have: every paragraph hidden.
    let rules = whole(".x { font-family: \"a\np\th1\" }");
    let copy = Rules::decode(&rules.encode());
    assert_eq!(copy.props("p", ""), rules.props("p", ""));
}

#[test]
fn small_capitals_survive_the_cache() {
    let rules = whole(".sc { font-variant: small-caps }");
    assert_eq!(rules.rules[0].1.small_caps, Some(true));
    let copy = Rules::decode(&rules.encode());
    assert_eq!(copy.rules, rules.rules);
}

#[test]
fn nested_at_rules_are_skipped_whole() {
    let css = alloc::format!(
        "{}.inner {{ text-align: right }}{} .after {{ text-align: center }}",
        "@media a {".repeat(5000),
        "}".repeat(5000)
    );
    let rules = whole(&css);
    assert_eq!(rules.props("p", "inner").align, None);
    assert_eq!(rules.props("p", "after").align, Some(Align::Center));
}

#[test]
fn an_unterminated_comment_swallows_the_rest() {
    let rules = whole(".a { text-align: right } /* .b { text-align: center }");
    assert_eq!(rules.props("p", "a").align, Some(Align::Right));
    assert_eq!(rules.props("p", "b").align, None);
}

#[test]
fn rules_past_the_limit_are_dropped() {
    let css: String = (0..MAX_RULES + 100)
        .map(|i| alloc::format!(".r{i} {{ text-align: center }}"))
        .collect();
    let rules = whole(&css);
    assert_eq!(rules.rules.len(), MAX_RULES);
    assert_eq!(rules.props("p", "r0").align, Some(Align::Center));
    assert_eq!(
        rules
            .props("p", &alloc::format!("r{}", MAX_RULES + 50))
            .align,
        None
    );
}

#[test]
fn huge_numbers_are_clamped_not_overflowed() {
    assert_eq!(
        length("99999999999999999999999999999999999999em"),
        Some(i32::MAX)
    );
    assert_eq!(
        length("-99999999999999999999999999999999999999em"),
        Some(i32::MIN)
    );
    assert_eq!(length("NaNem"), None);
    assert_eq!(length("--1px"), None);
    assert_eq!(space("1e30px"), None);
    assert_eq!(space("99999999999px"), Some(MAX_SPACE as u16));
    assert_eq!(FontSize::Relative(u16::MAX).of(u16::MAX), MAX_SIZE as u16);
    assert_eq!(
        font_size("99999999%").map(|s| s.of(100)),
        Some(MAX_SIZE as u16)
    );
}
