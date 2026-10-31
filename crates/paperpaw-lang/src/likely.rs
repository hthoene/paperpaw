//! The script a language is usually written in, for the tags that name
//! none (CLDR's "likely subtags", the script part): `ar`, `fa`, `he` do
//! not say `Arab` or `Hebr`, yet that is what text in them is written in.
//!
//! Data, sorted by language code for a binary search: one more language
//! is one more row, never a change of shape. Languages written in Latin
//! letters are not listed (nor are languages not listed): they have none.

pub(crate) const LIKELY: &[(&str, &str)] = &[
    ("am", "Ethi"),
    ("ar", "Arab"),
    ("as", "Beng"),
    ("be", "Cyrl"),
    ("bg", "Cyrl"),
    ("bn", "Beng"),
    ("bo", "Tibt"),
    ("ckb", "Arab"),
    ("doi", "Deva"),
    ("dv", "Thaa"),
    ("dz", "Tibt"),
    ("el", "Grek"),
    ("fa", "Arab"),
    ("gu", "Gujr"),
    ("he", "Hebr"),
    ("hi", "Deva"),
    ("hy", "Armn"),
    ("ja", "Jpan"),
    ("ka", "Geor"),
    ("kk", "Cyrl"),
    ("km", "Khmr"),
    ("kn", "Knda"),
    ("ko", "Kore"),
    ("kok", "Deva"),
    ("ky", "Cyrl"),
    ("lo", "Laoo"),
    ("mai", "Deva"),
    ("mk", "Cyrl"),
    ("ml", "Mlym"),
    ("mn", "Cyrl"),
    ("mr", "Deva"),
    ("my", "Mymr"),
    ("ne", "Deva"),
    ("or", "Orya"),
    ("pa", "Guru"),
    ("ps", "Arab"),
    ("ru", "Cyrl"),
    ("sa", "Deva"),
    ("sd", "Arab"),
    ("si", "Sinh"),
    ("sr", "Cyrl"),
    ("syr", "Syrc"),
    ("ta", "Taml"),
    ("te", "Telu"),
    ("tg", "Cyrl"),
    ("th", "Thai"),
    ("ti", "Ethi"),
    ("tt", "Cyrl"),
    ("ug", "Arab"),
    ("uk", "Cyrl"),
    ("ur", "Arab"),
    ("yi", "Hebr"),
    ("zh", "Hans"),
];

#[cfg(test)]
mod tests {
    use super::LIKELY;
    use crate::Lang;

    #[test]
    fn the_table_is_sorted_and_names_scripts_the_tables_know() {
        assert!(LIKELY.windows(2).all(|w| w[0].0 < w[1].0));
        for (language, script) in LIKELY {
            assert!(
                crate::scripts().any(|k| k == *script),
                "{language}: {script}"
            );
            assert_eq!(Lang::parse(language).language().as_str(), *language);
        }
    }

    #[test]
    fn languages_get_their_usual_script_and_a_tag_can_say_otherwise() {
        let script = |tag: &str| Lang::parse(tag).likely_script();
        for tag in ["ar", "fa", "fa-IR", "ur", "ps", "ckb", "ug"] {
            assert_eq!(script(tag), Some("Arab"), "{tag}");
        }
        assert_eq!(script("he"), Some("Hebr"));
        assert_eq!(script("sr"), Some("Cyrl"));
        assert_eq!(script("sr-Latn"), Some("Latn"));
        assert_eq!(script("zh-Hant"), Some("Hant"));
        assert_eq!(script("hi"), Some("Deva"));
        assert_eq!(script("de"), None);
        assert_eq!(script("en-GB"), None);
        assert_eq!(script(""), None);
        assert_eq!(script("not a tag"), None);
        assert_eq!(Lang::NONE.likely_script(), None);
    }
}
