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
/// To support a new language family, add a variant here and document which
/// languages use it.
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
/// [`PluralRule::Cldr`] index. Rules are only added at the end.
const CLDR: [&str; 2] = [
    // zero 0, one 1, two 2, few 3-10, many 11-99 (of the last two
    // digits), other: Arabic.
    "arabic", // one 1, two 2, other: Hebrew.
    "hebrew",
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
            // A rule this release does not know: a newer catalog's.
            PluralRule::Cldr(_) => Other,
        }
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
