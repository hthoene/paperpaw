//! Going to a page or a percentage of the book (`reader.go-to-page`,
//! `reader.go-to-percent`): the number is asked on the keyboard, and the
//! answer names a place of the book as Beta 3's did (`native_view/goto.rs`
//! finds it).

/// What the keyboard was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    /// The text to search for.
    Search,
    /// A page number.
    Page,
    /// A percentage of the book.
    Percent,
}

impl Asked {
    /// The question of the action `action`, where it asks for a number.
    #[must_use]
    pub fn of(action: &str) -> Option<Self> {
        match action {
            "reader.go-to-page" => Some(Self::Page),
            "reader.go-to-percent" => Some(Self::Percent),
            _ => None,
        }
    }
}

/// Where a typed number goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jump {
    /// To the start of this page (from 0).
    Page(u32),
    /// To this place of the book, in thousandths (at most 999: 100 % is the
    /// last page, not past it).
    Permille(u32),
}

/// The jump `typed` asks for, as Beta 3 read it: a page from 1 to `pages`
/// (those counted so far; a larger number is the last of them, a smaller the
/// first), or a percentage. None where no number could be read, or a page is
/// asked for and none is counted yet.
#[must_use]
pub fn jump(asked: Asked, typed: &str, pages: u32) -> Option<Jump> {
    let number: u32 = typed.trim().parse().ok()?;
    match asked {
        Asked::Page if pages > 0 => Some(Jump::Page(number.clamp(1, pages) - 1)),
        Asked::Percent => Some(Jump::Permille(number.saturating_mul(10).min(999))),
        Asked::Page | Asked::Search => None,
    }
}

/// The page (from 0) of `pages` that is `permille` thousandths into the
/// book, as Beta 3 found it on a counted book.
#[must_use]
pub fn page_at(permille: u32, pages: u32) -> u32 {
    let page = u64::from(permille) * u64::from(pages) / 1000;
    u32::try_from(page)
        .unwrap_or(u32::MAX)
        .min(pages.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_two_actions_ask_for_a_number() {
        assert_eq!(Asked::of("reader.go-to-page"), Some(Asked::Page));
        assert_eq!(Asked::of("reader.go-to-percent"), Some(Asked::Percent));
        assert_eq!(Asked::of("reader.search"), None);
    }

    #[test]
    fn a_page_is_kept_within_the_pages_counted() {
        assert_eq!(jump(Asked::Page, " 12 ", 300), Some(Jump::Page(11)));
        assert_eq!(jump(Asked::Page, "1", 300), Some(Jump::Page(0)));
        assert_eq!(jump(Asked::Page, "0", 300), Some(Jump::Page(0)));
        assert_eq!(jump(Asked::Page, "999", 300), Some(Jump::Page(299)));
        // None counted yet, or no number.
        assert_eq!(jump(Asked::Page, "5", 0), None);
        assert_eq!(jump(Asked::Page, "five", 300), None);
        assert_eq!(jump(Asked::Page, "", 300), None);
        assert_eq!(jump(Asked::Page, "-3", 300), None);
        assert_eq!(jump(Asked::Search, "3", 300), None);
    }

    #[test]
    fn a_percentage_is_a_place_before_the_end() {
        assert_eq!(jump(Asked::Percent, "0", 0), Some(Jump::Permille(0)));
        assert_eq!(jump(Asked::Percent, "50", 0), Some(Jump::Permille(500)));
        assert_eq!(jump(Asked::Percent, "100", 0), Some(Jump::Permille(999)));
        assert_eq!(
            jump(Asked::Percent, "4000000000", 0),
            Some(Jump::Permille(999))
        );
        assert_eq!(jump(Asked::Percent, "x", 0), None);
    }

    #[test]
    fn a_percentage_of_a_counted_book_is_one_of_its_pages() {
        assert_eq!(page_at(0, 300), 0);
        assert_eq!(page_at(500, 300), 150);
        assert_eq!(page_at(999, 300), 299);
        assert_eq!(page_at(999, 1), 0);
        assert_eq!(page_at(500, 0), 0);
    }
}
