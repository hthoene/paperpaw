//! Tests of the conversion of chapters.

use super::*;
use paperpaw_sdk::xml::{Token, Tokenizer};

fn is_furniture(tag: &str) -> bool {
    let mut found = None;
    Tokenizer::parse(tag.as_bytes(), &mut |token| {
        if let Token::Start { name, attrs, .. } = token {
            found = Some(furniture(name, &attrs, true));
        }
    });
    found.unwrap()
}

/// A storage that notes the largest piece appended to a file.
struct Spy {
    inner: paperpaw_sdk::MemoryStorage,
    watched: String,
    largest: core::cell::Cell<usize>,
}

impl Storage for Spy {
    fn read_at(
        &self,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, paperpaw_sdk::StorageError> {
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, paperpaw_sdk::StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_sdk::StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_sdk::StorageError> {
        if path == self.watched {
            self.largest.set(self.largest.get().max(data.len()));
        }
        self.inner.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_sdk::DirEntry<'_>),
    ) -> Result<(), paperpaw_sdk::StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), paperpaw_sdk::StorageError> {
        self.inner.remove(path)
    }
}

/// A block of many text nodes (here a list of links in one
/// paragraph) writes where each one starts in pieces of about
/// FLUSH_SMALL, not all at once: memory stays small however long the
/// block is.
#[test]
fn origins_are_written_as_they_come() {
    let mut page = String::from("<html><body><p>");
    for i in 0..400 {
        page.push_str(&alloc::format!("<a href=\"#n{i}\">{i}</a> "));
    }
    page.push_str("</p></body></html>");
    let paths = CachePaths::new("c", "0");
    let storage = Spy {
        inner: paperpaw_sdk::MemoryStorage::new().with("page.html", page.as_str()),
        watched: paths.xpointers.clone(),
        largest: core::cell::Cell::new(0),
    };
    let source = Source::Html {
        path: "page.html",
        focus: None,
        base: None,
        pictures: false,
    };
    convert(
        &storage,
        &source,
        &paths,
        css::Rules::default(),
        false,
        false,
        Fit::side(300),
        BookText::default(),
    )
    .unwrap();
    let written = storage.inner.size(&paths.xpointers).unwrap() as usize;
    assert!(written > 4 * FLUSH_SMALL, "{written} bytes of origins");
    let largest = storage.largest.get();
    assert!(largest < 2 * FLUSH_SMALL, "{largest} bytes at once");
}

#[test]
fn page_furniture_is_recognized() {
    for tag in [
        "<nav>",
        "<div role=\"navigation\">",
        "<div class=\"vector-dropdown mw-portlet\">",
        "<span class=\"mw-editsection\">",
        "<div id=\"site_sidebar\">",
        "<p aria-hidden=\"true\">",
        "<div class=\"NoPrint\">",
    ] {
        assert!(is_furniture(tag), "{tag}");
    }
    for tag in [
        "<p>",
        "<header>",
        "<div class=\"unavailable canvas\">",
        "<div class=\"article-header\">",
        "<div id=\"mw-content-text\">",
        "<div class=\"vector-feature-main-menu-pinned-disabled\">",
        "<body class=\"menu-open\">",
    ] {
        assert!(!is_furniture(tag), "{tag}");
    }
}
