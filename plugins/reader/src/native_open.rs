//! Where the reader opens a book that the `core.reader` hand-over names
//! (`docs/abi/document.md` section 1, *Opening at a place*): the params
//! `section` and `offset` (decimal `u32`, both or neither: a `doc_at` position)
//! win over `page`; anything else that is wrong or missing is not an error, the
//! book opens where it would without it. Host-free and allocation-free: the
//! screen that reads the hand-over asks, then checks the place against the
//! document (a section past the last one, an offset past the section's end).

/// The hand-over param of a position's section.
pub const SECTION: &str = "section";
/// The hand-over param of a position's offset in its section.
pub const OFFSET: &str = "offset";
/// The hand-over param of a page.
pub const PAGE: &str = "page";

/// A position of the book as the document host counts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Place {
    /// The section of the position.
    pub section: u32,
    /// The offset in the section.
    pub offset: u32,
}

/// Where the hand-over asks the book to open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Start {
    /// At a position (`section` and `offset`).
    At(Place),
    /// At a page (`page`, as the sender wrote it).
    Page(u32),
    /// Nothing usable was named: the saved place, else the start.
    Saved,
}

/// A decimal `u32`: digits only (no sign, no space, not empty).
#[must_use]
pub fn decimal(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// What the hand-over's params ask for. `get` is the hand-over's lookup of a
/// param by name.
#[must_use]
pub fn start<'a>(get: impl Fn(&str) -> Option<&'a str>) -> Start {
    if let (Some(section), Some(offset)) = (
        get(SECTION).and_then(decimal),
        get(OFFSET).and_then(decimal),
    ) {
        return Start::At(Place { section, offset });
    }
    match get(PAGE).and_then(decimal) {
        Some(page) => Start::Page(page),
        None => Start::Saved,
    }
}

/// The place to open at once the document has been asked about the one the
/// hand-over named: that place when the document has it, else what the
/// hand-over would ask without it (`page`, else the saved place).
#[must_use]
pub fn opening<'a>(
    get: impl Fn(&str) -> Option<&'a str> + Copy,
    has: impl Fn(Place) -> bool,
) -> Start {
    match start(get) {
        Start::At(place) if has(place) => Start::At(place),
        Start::At(_) => match get(PAGE).and_then(decimal) {
            Some(page) => Start::Page(page),
            None => Start::Saved,
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params<'a>(rows: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<&'a str> + Copy {
        move |name| rows.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
    }

    #[test]
    fn both_open_at_the_place_and_win_over_page() {
        let rows = [("section", "3"), ("offset", "120"), ("page", "9")];
        assert_eq!(
            start(params(&rows)),
            Start::At(Place {
                section: 3,
                offset: 120
            })
        );
    }

    #[test]
    fn neither_opens_at_the_page_or_the_saved_place() {
        assert_eq!(start(params(&[])), Start::Saved);
        assert_eq!(start(params(&[("page", "9")])), Start::Page(9));
        assert_eq!(start(params(&[("page", "x")])), Start::Saved);
    }

    #[test]
    fn only_one_is_ignored() {
        assert_eq!(start(params(&[("section", "3")])), Start::Saved);
        assert_eq!(start(params(&[("offset", "3")])), Start::Saved);
        assert_eq!(
            start(params(&[("section", "3"), ("page", "4")])),
            Start::Page(4)
        );
    }

    #[test]
    fn bad_values_are_ignored() {
        for bad in [
            "",
            "-1",
            "+1",
            " 1",
            "1 ",
            "1.0",
            "0x10",
            "four",
            "4294967296",
            "١",
        ] {
            assert_eq!(
                start(params(&[("section", bad), ("offset", "1")])),
                Start::Saved,
                "{bad:?}"
            );
            assert_eq!(
                start(params(&[("section", "1"), ("offset", bad), ("page", "2")])),
                Start::Page(2),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn the_largest_values_and_leading_zeros_are_fine() {
        let rows = [("section", "4294967295"), ("offset", "007")];
        assert_eq!(
            start(params(&rows)),
            Start::At(Place {
                section: u32::MAX,
                offset: 7
            })
        );
    }

    #[test]
    fn a_place_the_book_lacks_falls_back() {
        let rows = [("section", "99"), ("offset", "0"), ("page", "5")];
        assert_eq!(opening(params(&rows), |_| false), Start::Page(5));
        assert_eq!(opening(params(&rows[..2]), |_| false), Start::Saved);
        assert_eq!(
            opening(params(&rows), |p| p.section == 99),
            Start::At(Place {
                section: 99,
                offset: 0
            })
        );
    }
}
