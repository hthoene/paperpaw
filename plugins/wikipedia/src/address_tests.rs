//! The address of a summary for any word and language: always on a
//! wiki of letters at `wikipedia.org`, the word one encoded path segment.

extern crate std;

use proptest::prelude::*;

use crate::native_model::{article_url, summary_url, wiki};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn any_word_stays_one_segment_on_a_wiki(language in ".{0,20}", word in ".{0,40}") {
        let url = summary_url(&language, &word).unwrap();
        let rest = url.strip_prefix("https://").unwrap();
        let (host, path) = rest.split_once('/').unwrap();
        let code = host.strip_suffix(".wikipedia.org").unwrap();
        prop_assert!(!code.is_empty());
        prop_assert!(code.chars().all(|c| c.is_ascii_lowercase()), "{}", code);
        prop_assert_eq!(code, wiki(&language).unwrap());
        let title = path.strip_prefix("api/rest_v1/page/summary/").unwrap();
        prop_assert!(!title.contains(['/', '?', '#', ' ', '\n', '\t']), "{}", title);
        prop_assert!(title.is_ascii(), "{}", title);
    }
}

#[test]
fn an_article_is_named_by_its_address_below_it() {
    assert_eq!(
        article_url("de-DE", "Hund").unwrap(),
        "de.wikipedia.org/wiki/Hund"
    );
    assert_eq!(
        article_url("en", "White Rabbit").unwrap(),
        "en.wikipedia.org/wiki/White_Rabbit"
    );
}
