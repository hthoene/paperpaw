//! What each table hyphenates, as the pattern authors expect (moved here from
//! the interface-6 plugins' tests): known words break where the dictionaries
//! break them, the minimums are kept, and case does not matter.

use std::fs;

use paperpaw_hyphen::Patterns;

use super::*;
use crate::tests::built;

/// The table a package provides for its first language tag.
fn table(package: &Package) -> Vec<u8> {
    let (_, stem) = package.languages[0];
    fs::read(
        built()
            .join(package.id)
            .join("hyphenation")
            .join(format!("{stem}.pphp")),
    )
    .unwrap()
}

fn breaks(table: &[u8], word: &str) -> Vec<usize> {
    let patterns = Patterns::new(table).unwrap();
    let mut found = Vec::new();
    patterns.hyphenate(word, &mut |at| found.push(at));
    found
}

fn hyphenated(table: &[u8], word: &str) -> String {
    let found = breaks(table, word);
    let mut out = String::new();
    for (i, c) in word.char_indices() {
        if found.contains(&i) {
            out.push('-');
        }
        out.push(c);
    }
    out
}

/// A package id, the words as hyphenated, and the words whose breaks do not
/// depend on case.
const WORDS: [(&str, &[&str], &[&str]); 13] = [
    (
        "hyphenation",
        &[
            // Not "com-put-er": three letters at least after a hyphen.
            "com-puter",
            "al-go-rithm",
            "beau-ti-ful",
            "in-for-ma-tion",
            "pos-si-bil-ity",
            // From the exception list.
            "as-so-ciate",
            "project",
            "present",
            "the",
            "an",
        ],
        &["Computer", "information"],
    ),
    (
        "hyphenation-da",
        &[
            "uni-ver-si-tet",
            "dej-lig",
            "men-ne-sker",
            "fod-bold",
            "ho-ved-stad",
        ],
        &["universitet", "dejlig"],
    ),
    (
        "hyphenation-de",
        &[
            "Schiff-fahrt",
            "Zu-cker",
            "Was-ser",
            "Fens-ter",
            "Kar-tof-fel",
            "El-tern",
            "Stra-ße",
            "Uni-ver-si-tät",
            "Bä-cke-rei",
            "Mäd-chen",
            "Freund-schaft",
            "Ofen",
            "Abend",
            "Ver-wand-lung",
            "Un-ge-zie-fer",
            "Rü-cken",
        ],
        &["Wasser", "kartoffel"],
    ),
    (
        "hyphenation-es",
        &[
            "bi-blio-te-ca",
            "uni-ver-si-dad",
            "or-de-na-dor",
            "her-mo-so",
            "pa-la-bra",
            "cons-truc-ción",
        ],
        &["biblioteca", "universidad"],
    ),
    (
        "hyphenation-fr",
        &[
            "uni-ver-si-té",
            "or-di-na-teur",
            "cho-co-lat",
            "ima-gi-na-tion",
            "ma-gni-fique",
            "bi-blio-thèque",
        ],
        &["université", "ordinateur"],
    ),
    (
        "hyphenation-it",
        &[
            "bi-blio-te-ca",
            "uni-ver-si-tà",
            "me-ra-vi-glio-so",
            "au-to-mo-bi-le",
            "sta-zio-ne",
            "ra-gaz-zo",
        ],
        &["biblioteca", "università"],
    ),
    (
        "hyphenation-nb",
        &[
            "uni-ver-si-tet",
            "fan-tas-tisk",
            "men-nes-ker",
            "fot-ball",
            "ho-ved-stad",
        ],
        &["universitet", "fantastisk"],
    ),
    (
        "hyphenation-nl",
        &[
            "uni-ver-si-teit",
            "fiet-sen-ma-ker",
            "won-der-lijk",
            "aard-ap-pel",
            "boe-ken-kast",
            "com-pu-ter",
        ],
        &["universiteit", "fietsenmaker"],
    ),
    (
        "hyphenation-pl",
        &[
            "uni-wer-sy-tet",
            "wspa-nia-ły",
            "czło-wiek",
            "przy-ja-ciel",
            "kom-pu-ter",
        ],
        &["uniwersytet", "wspaniały"],
    ),
    (
        "hyphenation-pt",
        &[
            "uni-ver-si-dade",
            "com-pu-ta-dor",
            "pa-la-vra",
            "ex-tra-or-di-ná-rio",
            "ma-ra-vi-lhoso",
        ],
        &["universidade", "computador"],
    ),
    (
        "hyphenation-ru",
        &["уни-вер-си-тет", "пре-крас-ный", "ком-пью-тер", "че-ло-век"],
        &["университет", "прекрасный"],
    ),
    (
        "hyphenation-sv",
        &[
            "uni-ver-si-tet",
            "un-der-bar",
            "män-ni-ska",
            "fot-boll",
            "kun-skap",
        ],
        &["universitet", "underbar"],
    ),
    (
        "hyphenation-uk",
        &["уні-вер-си-тет", "чу-до-вий", "лю-ди-на", "кра-ї-на"],
        &["університет", "чудовий"],
    ),
];

fn words_of(id: &str) -> (&'static [&'static str], &'static [&'static str]) {
    let (_, words, case) = WORDS.iter().find(|(own, ..)| *own == id).unwrap();
    (words, case)
}

#[test]
fn words_as_the_authors_expect() {
    for package in &PACKAGES {
        let table = table(package);
        for expected in words_of(package.id).0 {
            let word: String = expected.chars().filter(|&c| c != '-').collect();
            assert_eq!(hyphenated(&table, &word), *expected, "{}", package.id);
        }
    }
}

#[test]
fn the_minimums_are_kept() {
    for package in &PACKAGES {
        let table = table(package);
        let (left, right) = (
            usize::from(package.minimums.0),
            usize::from(package.minimums.1),
        );
        assert_eq!(
            Patterns::new(&table).unwrap().minimums(),
            (left, right),
            "{}",
            package.id
        );
        // Words too short to break at all.
        for len in 0..left + right {
            let word: String = std::iter::repeat_n('a', len).collect();
            assert!(breaks(&table, &word).is_empty(), "{}: {word}", package.id);
        }
    }
}

#[test]
fn case_does_not_matter() {
    for package in &PACKAGES {
        let table = table(package);
        for word in words_of(package.id).1 {
            let lower = breaks(&table, &word.to_lowercase());
            assert!(!lower.is_empty(), "{}: {word}", package.id);
            assert_eq!(
                breaks(&table, &word.to_uppercase()),
                lower,
                "{}: {word}",
                package.id
            );
            assert_eq!(breaks(&table, word), lower, "{}: {word}", package.id);
        }
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256),
        failure_persistence: None,
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// Any word, with digits, apostrophes, capitals and letters of other
    /// scripts: breaks are inside it, ascending, on character boundaries,
    /// with the minimums before and after.
    #[test]
    fn breaks_are_always_inside_the_word(
        word in "[a-zäöüßA-ZÄÖÜ0-9'’çéñøИя-]{0,60}",
    ) {
        let english = PACKAGES.iter().find(|p| p.id == "hyphenation").unwrap();
        let table = table(english);
        let (left, right) = (usize::from(english.minimums.0), usize::from(english.minimums.1));
        let found = breaks(&table, &word);
        for (k, &at) in found.iter().enumerate() {
            proptest::prop_assert!(k == 0 || found[k - 1] < at);
            proptest::prop_assert!(0 < at && at < word.len() && word.is_char_boundary(at));
            proptest::prop_assert!(word[..at].chars().count() >= left);
            proptest::prop_assert!(word[at..].chars().count() >= right);
        }
    }
}
