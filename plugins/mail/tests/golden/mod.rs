//! Answers pinned from the interface-6 code, which is gone: a golden file is
//! lines of `tag<TAB>content`, content being the `Debug` text of what the
//! interface-6 code answered for one input. A test builds the same lines from
//! the native code and compares them with the lines of its tag.
#![allow(dead_code, reason = "each test uses some of these")]

/// The lines of `tag`, the tag and its tab taken off.
pub fn lines<'a>(golden: &'a str, tag: &str) -> Vec<&'a str> {
    golden
        .lines()
        .filter_map(|line| {
            let (found, rest) = line.split_once('\t')?;
            (found == tag).then_some(rest)
        })
        .collect()
}

/// Says where `actual` differs from the pinned lines of `tag`.
pub fn check(golden: &str, tag: &str, actual: &[String]) {
    let pinned = lines(golden, tag);
    assert!(!pinned.is_empty(), "no golden lines for {tag:?}");
    for (at, (want, got)) in pinned.iter().zip(actual).enumerate() {
        assert_eq!(got, want, "{tag}: line {}", at + 1);
    }
    assert_eq!(
        actual.len(),
        pinned.len(),
        "{tag}: the number of lines differs"
    );
}
