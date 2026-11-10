use super::*;

#[test]
fn the_path_is_one_for_each_version_size_and_language() {
    assert_eq!(
        path("fredoka", "1.0", "de").unwrap(),
        "samples/fredoka-1.0-26-de.ppfp"
    );
    // Only letters, digits and dots of a version are kept; two dots in a row
    // would look like a step up and are refused.
    assert_eq!(
        path("a", "1.0/x", "en").unwrap(),
        "samples/a-1.0x-26-en.ppfp"
    );
    assert_eq!(path("a", "1.0/../x", "en"), Err(Error::Invalid));
}

#[test]
fn a_path_that_leaves_the_folder_or_is_too_long_is_refused() {
    assert_eq!(path("../x", "1", "en"), Err(Error::Invalid));
    assert_eq!(path("a/b", "1", "en"), Err(Error::Invalid));
    let long = "x".repeat(130);
    assert_eq!(path(&long, "1", "en"), Err(Error::Invalid));
}

#[test]
fn the_characters_are_each_once_in_order_with_an_ellipsis() {
    assert_eq!(characters("Aab", "ba c").unwrap(), " Aabc…");
    assert_eq!(characters("", "").unwrap(), "…");
    assert_eq!(characters("a\nb", "").unwrap(), "ab…");
}

#[test]
fn the_address_asks_for_the_size_and_the_characters() {
    assert_eq!(
        url("https://x.org/f/sample", "a b").unwrap(),
        "https://x.org/f/sample?size=26&text=a+b"
    );
    assert_eq!(
        url("https://x.org/s?v=1", "ä…").unwrap(),
        "https://x.org/s?v=1&size=26&text=%C3%A4%E2%80%A6"
    );
}

#[test]
fn a_path_over_the_limit_is_refused_before_anything_is_built() {
    let long = "x".repeat(10_000);
    assert_eq!(path(&long, "1", "en"), Err(Error::Invalid));
    assert_eq!(path("a", &long, "en"), Err(Error::Invalid));
    assert_eq!(path("a", "1", &long), Err(Error::Invalid));
}
