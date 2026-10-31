/// CLDR plural categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluralCategory {
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}

impl PluralCategory {
    /// Suffix used in message keys, e.g. `library.books.few`.
    pub const fn id(self) -> &'static str {
        match self {
            PluralCategory::Zero => "zero",
            PluralCategory::One => "one",
            PluralCategory::Two => "two",
            PluralCategory::Few => "few",
            PluralCategory::Many => "many",
            PluralCategory::Other => "other",
        }
    }
}

/// Plural rule families for integer counts, named in catalogs by
/// [`PluralRule::id`].
///
/// A new rule family is added to [`cldr_rules`] (as a
/// [`PluralRule::Cldr`] index), not as a variant: the CLDR's rules for
/// whole numbers are all there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PluralRule {
    /// No plural distinction: Chinese, Japanese, Korean, Thai, Vietnamese, …
    Other,
    /// `one` for 1: English, German, Dutch, Swedish, Italian, Spanish, …
    OneOther,
    /// `one` for 0 and 1: French, Brazilian Portuguese, …
    ZeroOneOther,
    /// `one`/`few`/`many`: Russian, Ukrainian, Belarusian.
    EastSlavic,
    /// `one`/`few`/`many`: Polish.
    Polish,
    /// `one`/`few`/`other`: Czech, Slovak.
    CzechSlovak,
    /// A rule of the CLDR's, by its place in [`cldr_rules`]: new rules
    /// are added there, not as variants.
    Cldr(u8),
}

/// The CLDR's plural rules beyond the variants of [`PluralRule`], by the
/// name a catalog's `[meta] plural` gives them; a rule's place is its
/// [`PluralRule::Cldr`] index. Rules are only added at the end. With
/// the variants, they are all the CLDR's rules for whole numbers (a
/// plugin keeps its own copy of [`PluralRule::category`], so a rule
/// added later would read as `other` in plugins built before).
const CLDR: [&str; 21] = [
    // zero 0, one 1, two 2, few 3-10, many 11-99 (of the last two
    // digits), other: Arabic.
    "arabic",
    // one 1, two 2, other: Hebrew; also Inuktitut, the Sami languages.
    "hebrew",
    // zero 0, one 1, two 2, few 3, many 6, other: Welsh.
    "welsh",
    // one 1, two 2, few 3-6, many 7-10, other: Irish.
    "irish",
    // one 1, 11; two 2, 12; few 3-10, 13-19; other: Scottish Gaelic.
    "scottish-gaelic",
    // one, two, few by the last digits, many for millions: Breton.
    "breton",
    // one ending in 1, two in 2, few 0, 20, 40, 60, 80 (of the last two
    // digits), other: Manx.
    "manx",
    // one, two, few 3-4 of the last two digits, other: Slovenian, Lower
    // and Upper Sorbian.
    "slovenian",
    // one ending in 1, few in 2-9, not 11-19, other: Lithuanian.
    "lithuanian",
    // zero ending in 0 or 11-19, one ending in 1 but not 11, other:
    // Latvian, Prussian.
    "latvian",
    // one 1, few 0 and 1-19 of the last two digits (but 1), other: Romanian,
    // Moldavian.
    "romanian",
    // one 1, two 2, few 0 and 3-10, many 11-19 (of the last two), other:
    // Maltese.
    "maltese",
    // one ending in 1 but not 11, other: Icelandic, Macedonian.
    "icelandic",
    // one ending in 1 but not 11, few 2-4 but not 12-14, other: Bosnian,
    // Croatian, Serbian.
    "south-slavic",
    // zero, one, two, few, many by the last digits: Cornish.
    "cornish",
    // one 0-1, few 2-10, other: Tachelhit.
    "tachelhit",
    // zero 0, one 1, other: Colognian, Langi, Anii.
    "zero-one",
    // other ending in 4, 6 or 9, else one: Filipino, Tagalog.
    "filipino",
    // one 0-1 and 11-99, other: Central Atlas Tamazight.
    "tamazight",
    // one 1, many for whole millions, other: Spanish, Italian, Catalan,
    // European Portuguese (`one-other` without the millions).
    "one-many-other",
    // one 0-1, many for whole millions, other: French, Brazilian
    // Portuguese (`zero-one-other` without the millions).
    "zero-one-many-other",
];

/// The names of the CLDR rules a catalog can name beyond the variants of
/// [`PluralRule`], in the order of their [`PluralRule::Cldr`] index.
pub fn cldr_rules() -> impl Iterator<Item = &'static str> {
    CLDR.into_iter()
}

impl PluralRule {
    pub const fn id(self) -> &'static str {
        match self {
            PluralRule::Other => "other",
            PluralRule::OneOther => "one-other",
            PluralRule::ZeroOneOther => "zero-one-other",
            PluralRule::EastSlavic => "east-slavic",
            PluralRule::Polish => "polish",
            PluralRule::CzechSlovak => "czech-slovak",
            PluralRule::Cldr(i) if (i as usize) < CLDR.len() => CLDR[i as usize],
            PluralRule::Cldr(_) => "other",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        if let Some(i) = CLDR.iter().position(|rule| *rule == id) {
            return u8::try_from(i).ok().map(PluralRule::Cldr);
        }
        [
            PluralRule::Other,
            PluralRule::OneOther,
            PluralRule::ZeroOneOther,
            PluralRule::EastSlavic,
            PluralRule::Polish,
            PluralRule::CzechSlovak,
        ]
        .into_iter()
        .find(|rule| rule.id() == id)
    }

    /// The category for an integer `count`.
    pub fn category(self, count: i64) -> PluralCategory {
        use PluralCategory::{Few, Many, One, Other};
        let n = count.unsigned_abs();
        // Only the rules that need them divide: a division of 64 bits is
        // a call on small devices.
        let few = || {
            let (mod10, mod100) = (n % 10, n % 100);
            (2..=4).contains(&mod10) && !(12..=14).contains(&mod100)
        };
        match self {
            PluralRule::Other => Other,
            PluralRule::OneOther => {
                if n == 1 {
                    One
                } else {
                    Other
                }
            }
            PluralRule::ZeroOneOther => {
                if n <= 1 {
                    One
                } else {
                    Other
                }
            }
            PluralRule::EastSlavic => {
                if n % 10 == 1 && n % 100 != 11 {
                    One
                } else if few() {
                    Few
                } else {
                    Many
                }
            }
            PluralRule::Polish => {
                if n == 1 {
                    One
                } else if few() {
                    Few
                } else {
                    Many
                }
            }
            PluralRule::CzechSlovak => match n {
                1 => One,
                2..=4 => Few,
                _ => Other,
            },
            // Arabic.
            PluralRule::Cldr(0) => match (n, n % 100) {
                (0, _) => PluralCategory::Zero,
                (1, _) => One,
                (2, _) => PluralCategory::Two,
                (_, 3..=10) => Few,
                (_, 11..=99) => Many,
                _ => Other,
            },
            // Hebrew.
            PluralRule::Cldr(1) => match n {
                1 => One,
                2 => PluralCategory::Two,
                _ => Other,
            },
            PluralRule::Cldr(index) => cldr(index, n),
        }
    }
}

/// The category of `n` by the CLDR rule at `index` (from Welsh on); `other`
/// for a rule this release does not know: a newer catalog's.
fn cldr(index: u8, n: u64) -> PluralCategory {
    use PluralCategory::{Few, Many, One, Other, Two, Zero};
    let (mod10, mod100) = (n % 10, n % 100);
    match index {
        // Welsh.
        2 => match n {
            0 => Zero,
            1 => One,
            2 => Two,
            3 => Few,
            6 => Many,
            _ => Other,
        },
        // Irish.
        3 => match n {
            1 => One,
            2 => Two,
            3..=6 => Few,
            7..=10 => Many,
            _ => Other,
        },
        // Scottish Gaelic.
        4 => match n {
            1 | 11 => One,
            2 | 12 => Two,
            3..=10 | 13..=19 => Few,
            _ => Other,
        },
        // Breton.
        5 => {
            if mod10 == 1 && !matches!(mod100, 11 | 71 | 91) {
                One
            } else if mod10 == 2 && !matches!(mod100, 12 | 72 | 92) {
                Two
            } else if matches!(mod10, 3 | 4 | 9) && !matches!(mod100, 10..=19 | 70..=79 | 90..=99) {
                Few
            } else if n != 0 && n.is_multiple_of(1_000_000) {
                Many
            } else {
                Other
            }
        }
        // Manx.
        6 => match (mod10, mod100) {
            (1, _) => One,
            (2, _) => Two,
            (_, 0 | 20 | 40 | 60 | 80) => Few,
            _ => Other,
        },
        // Slovenian.
        7 => match mod100 {
            1 => One,
            2 => Two,
            3 | 4 => Few,
            _ => Other,
        },
        // Lithuanian.
        8 => match (mod10, mod100) {
            (_, 11..=19) => Other,
            (1, _) => One,
            (2..=9, _) => Few,
            _ => Other,
        },
        // Latvian.
        9 => match (mod10, mod100) {
            (0, _) | (_, 11..=19) => Zero,
            (1, _) => One,
            _ => Other,
        },
        // Romanian.
        10 => match (n, mod100) {
            (1, _) => One,
            (0, _) | (_, 1..=19) => Few,
            _ => Other,
        },
        // Maltese.
        11 => match (n, mod100) {
            (1, _) => One,
            (2, _) => Two,
            (0, _) | (_, 3..=10) => Few,
            (_, 11..=19) => Many,
            _ => Other,
        },
        _ => cldr_from_icelandic(index, n),
    }
}

/// [`cldr`] from Icelandic on.
fn cldr_from_icelandic(index: u8, n: u64) -> PluralCategory {
    use PluralCategory::{Few, Many, One, Other, Two, Zero};
    let (mod10, mod100) = (n % 10, n % 100);
    match index {
        // Icelandic.
        12 => {
            if mod10 == 1 && mod100 != 11 {
                One
            } else {
                Other
            }
        }
        // Bosnian, Croatian, Serbian.
        13 => match (mod10, mod100) {
            (1, m) if m != 11 => One,
            (2..=4, m) if !(12..=14).contains(&m) => Few,
            _ => Other,
        },
        // Cornish.
        14 => {
            let thousands = n % 100_000;
            if n == 0 {
                Zero
            } else if n == 1 {
                One
            } else if matches!(mod100, 2 | 22 | 42 | 62 | 82)
                || n.is_multiple_of(1000)
                    && (matches!(thousands, 1000..=20_000)
                        || matches!(thousands, 40_000 | 60_000 | 80_000))
                || n % 1_000_000 == 100_000
            {
                Two
            } else if matches!(mod100, 3 | 23 | 43 | 63 | 83) {
                Few
            } else if matches!(mod100, 1 | 21 | 41 | 61 | 81) {
                Many
            } else {
                Other
            }
        }
        // Tachelhit.
        15 => match n {
            0 | 1 => One,
            2..=10 => Few,
            _ => Other,
        },
        // Colognian, Langi, Anii.
        16 => match n {
            0 => Zero,
            1 => One,
            _ => Other,
        },
        // Filipino.
        17 => {
            if matches!(mod10, 4 | 6 | 9) {
                Other
            } else {
                One
            }
        }
        // Central Atlas Tamazight.
        18 => match n {
            0 | 1 | 11..=99 => One,
            _ => Other,
        },
        // Spanish, Italian, Catalan, European Portuguese.
        19 => match n {
            1 => One,
            _ if n != 0 && n.is_multiple_of(1_000_000) => Many,
            _ => Other,
        },
        // French, Brazilian Portuguese.
        20 => match n {
            0 | 1 => One,
            _ if n.is_multiple_of(1_000_000) => Many,
            _ => Other,
        },
        _ => Other,
    }
}

#[cfg(test)]
mod tests {
    use super::PluralCategory::*;
    use super::*;

    #[test]
    fn east_slavic() {
        let rule = PluralRule::EastSlavic;
        let got: [PluralCategory; 7] = [1, 2, 5, 11, 21, 22, 112].map(|n| rule.category(n));
        assert_eq!(got, [One, Few, Many, Many, One, Few, Many]);
    }

    #[test]
    fn polish_and_czech() {
        assert_eq!(PluralRule::Polish.category(22), Few);
        assert_eq!(PluralRule::Polish.category(21), Many);
        assert_eq!(PluralRule::CzechSlovak.category(3), Few);
        assert_eq!(PluralRule::CzechSlovak.category(22), Other);
    }

    #[test]
    fn arabic_and_hebrew() {
        let arabic = PluralRule::from_id("arabic").unwrap();
        let got: [PluralCategory; 9] =
            [0, 1, 2, 3, 10, 11, 99, 100, 103].map(|n| arabic.category(n));
        assert_eq!(got, [Zero, One, Two, Few, Few, Many, Many, Other, Few]);
        let hebrew = PluralRule::Cldr(1);
        assert_eq!(
            [1, 2, 3, 20].map(|n| hebrew.category(n)),
            [One, Two, Other, Other]
        );
        assert_eq!(PluralRule::Cldr(200).category(1), Other);
    }

    /// The CLDR's own examples of each rule (plurals.xml, integers).
    const CASES: [(&str, &[(i64, PluralCategory)]); 19] = [
        (
            "welsh",
            &[
                (0, Zero),
                (1, One),
                (2, Two),
                (3, Few),
                (6, Many),
                (4, Other),
                (7, Other),
            ],
        ),
        (
            "irish",
            &[
                (1, One),
                (2, Two),
                (5, Few),
                (9, Many),
                (11, Other),
                (0, Other),
            ],
        ),
        (
            "scottish-gaelic",
            &[(11, One), (12, Two), (19, Few), (20, Other), (0, Other)],
        ),
        (
            "breton",
            &[
                (21, One),
                (11, Other),
                (22, Two),
                (72, Other),
                (9, Few),
                (79, Other),
                (1_000_000, Many),
                (5, Other),
            ],
        ),
        (
            "manx",
            &[(11, One), (12, Two), (20, Few), (100, Few), (3, Other)],
        ),
        (
            "slovenian",
            &[(101, One), (102, Two), (104, Few), (111, Other)],
        ),
        (
            "lithuanian",
            &[(21, One), (11, Other), (9, Few), (15, Other), (10, Other)],
        ),
        (
            "latvian",
            &[(10, Zero), (13, Zero), (21, One), (11, Zero), (2, Other)],
        ),
        (
            "romanian",
            &[(1, One), (0, Few), (119, Few), (20, Other), (101, Few)],
        ),
        (
            "maltese",
            &[
                (1, One),
                (2, Two),
                (0, Few),
                (110, Few),
                (111, Many),
                (20, Other),
            ],
        ),
        ("icelandic", &[(21, One), (11, Other), (2, Other)]),
        (
            "south-slavic",
            &[(21, One), (11, Other), (22, Few), (12, Other), (5, Other)],
        ),
        (
            "cornish",
            &[
                (0, Zero),
                (1, One),
                (22, Two),
                (2000, Two),
                (100_000, Two),
                (23, Few),
                (21, Many),
                (4, Other),
            ],
        ),
        ("tachelhit", &[(0, One), (1, One), (10, Few), (11, Other)]),
        ("zero-one", &[(0, Zero), (1, One), (2, Other)]),
        (
            "filipino",
            &[(1, One), (3, One), (4, Other), (16, Other), (10, One)],
        ),
        (
            "tamazight",
            &[(0, One), (11, One), (99, One), (2, Other), (100, Other)],
        ),
        (
            "one-many-other",
            &[
                (1, One),
                (0, Other),
                (2, Other),
                (1_000_000, Many),
                (3_000_000, Many),
                (1_000_001, Other),
            ],
        ),
        (
            "zero-one-many-other",
            &[
                (0, One),
                (1, One),
                (2, Other),
                (1_000_000, Many),
                (2_500_000, Other),
            ],
        ),
    ];

    /// Each rule by the CLDR's own examples (plurals.xml, integers).
    #[test]
    fn the_cldr_rules_for_whole_numbers() {
        let rule = |id: &str| PluralRule::from_id(id).unwrap();
        for (id, examples) in CASES {
            for &(n, want) in examples {
                assert_eq!(rule(id).category(n), want, "{id} {n}");
            }
            assert_eq!(rule(id).id(), id);
        }
        assert_eq!(PluralRule::Cldr(200).id(), "other");
    }

    #[test]
    fn ids_round_trip() {
        for id in [
            "other",
            "one-other",
            "zero-one-other",
            "east-slavic",
            "polish",
            "czech-slovak",
            "arabic",
            "hebrew",
        ] {
            assert_eq!(PluralRule::from_id(id).map(PluralRule::id), Some(id));
        }
    }
}
