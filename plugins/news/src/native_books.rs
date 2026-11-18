//! Which book of the library is a feed's: the library names the book it took
//! (`Library::import` answers its path), and the plugin keeps that path next
//! to the mark of the feed's address in `books.txt` (`mark<TAB>path` lines),
//! so that a feed is opened from the list without looking for its book.
use alloc::string::String;
use paperpaw_sdk::native::Error;

/// The file in the data folder.
pub const BOOKS_FILE: &str = "books.txt";
/// The largest file read: a list of this size holds some hundreds of feeds.
pub const BOOKS_CAP: usize = 64 * 1024;
/// Books kept at most (the feeds list holds few; a damaged file does not grow).
pub const BOOKS_KEPT: usize = 300;

/// The path kept for the feed marked `mark` in `text` (the file as read).
#[must_use]
pub fn find<'a>(text: &'a str, mark: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (kept, path) = line.split_once('\t')?;
        (kept == mark && !path.is_empty()).then_some(path)
    })
}

/// The file once the feed marked `mark` has its book at `path`: the line of
/// that feed is replaced, the others stay (at most [`BOOKS_KEPT`] in all).
/// `old` is the file as it was, if it could be read.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn with(old: Option<&str>, mark: &str, path: &str) -> Result<String, Error> {
    let mut out = String::new();
    let room = old.map_or(0, str::len) + mark.len() + path.len() + 2;
    out.try_reserve_exact(room).map_err(|_| Error::NoMemory)?;
    out.push_str(mark);
    out.push('\t');
    out.extend(path.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')));
    out.push('\n');
    let mut kept = 1;
    for line in old.unwrap_or_default().lines() {
        let other = line
            .split_once('\t')
            .is_some_and(|(kept, path)| kept != mark && !path.is_empty());
        if other && kept < BOOKS_KEPT {
            out.push_str(line);
            out.push('\n');
            kept += 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feed_finds_its_book_by_its_mark() {
        let text = "3fa9c2\tNews/Tagesschau (3fa9c2).html\n0a0b0c\tNews/Site (0a0b0c).html\n";
        assert_eq!(find(text, "0a0b0c"), Some("News/Site (0a0b0c).html"));
        assert_eq!(find(text, "ffffff"), None);
        assert_eq!(find("3fa9c2\t\n", "3fa9c2"), None);
        assert_eq!(find("", "3fa9c2"), None);
    }

    #[test]
    fn a_new_book_replaces_the_feeds_line_and_comes_first() {
        let old = "3fa9c2\tNews/A.html\n0a0b0c\tNews/B.html\n";
        assert_eq!(
            with(Some(old), "0a0b0c", "News/B (2).html").unwrap(),
            "0a0b0c\tNews/B (2).html\n3fa9c2\tNews/A.html\n"
        );
        assert_eq!(
            with(None, "3fa9c2", "News/A.html").unwrap(),
            "3fa9c2\tNews/A.html\n"
        );
    }

    #[test]
    fn damaged_lines_are_dropped_and_the_file_does_not_grow_without_end() {
        let mut old = String::from("no-tab-here\nempty\t\n");
        for i in 0..400 {
            old.push_str(&alloc::format!("{i:06x}\tNews/{i}.html\n"));
        }
        let text = with(Some(&old), "ffffff", "News/N.html").unwrap();
        assert_eq!(text.lines().count(), BOOKS_KEPT);
        assert!(text.starts_with("ffffff\tNews/N.html\n"));
        assert!(!text.contains("no-tab-here"));
        // A path cannot carry a line break into the file.
        let text = with(None, "ffffff", "a\nb\tc").unwrap();
        assert_eq!(text, "ffffff\tabc\n");
    }
}
