//! Hostile books: made to exhaust memory, storage or time. Each must open
//! and read within small, fixed bounds. Memory is measured with an
//! allocator that counts this thread's heap; the files of the storage are
//! not counted, as on a device they are on the card.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use paperpaw_epub_family::{EpubDocument, Fb2Document, HtmlDocument, MarkdownDocument};
use paperpaw_sdk::document::{Align, Document, Locator};
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};
use paperpaw_zip::testing::ZipBuilder;

/// The system's allocator, counting what this thread holds.
struct Counting;

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
    /// Set while the storage works: its files are not counted.
    static PAUSED: Cell<bool> = const { Cell::new(false) };
}

fn count(delta: isize) {
    if PAUSED.try_with(Cell::get).unwrap_or(true) {
        return;
    }
    let _ = LIVE.try_with(|live| {
        let now = live.get() + delta;
        live.set(now);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now)));
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let block = unsafe { System.alloc(layout) };
        if !block.is_null() {
            count(layout.size() as isize);
        }
        block
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        unsafe { System.dealloc(block, layout) };
        count(-(layout.size() as isize));
    }

    unsafe fn realloc(&self, block: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let moved = unsafe { System.realloc(block, layout, size) };
        if !moved.is_null() {
            count(size as isize - layout.size() as isize);
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The most heap `f` holds at once, beyond what was held before.
fn peak<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.with(Cell::get);
    PEAK.with(|peak| peak.set(base));
    let out = f();
    let top = PEAK.with(Cell::get);
    (out, (top - base) as usize)
}

/// Runs `f` without counting what it allocates.
fn paused<T>(f: impl FnOnce() -> T) -> T {
    PAUSED.with(|paused| paused.set(true));
    let out = f();
    PAUSED.with(|paused| paused.set(false));
    out
}

/// A card: files in memory that do not count as heap, and how often they
/// are read and written.
struct Card {
    files: MemoryStorage,
    reads: Cell<usize>,
    writes: Cell<usize>,
    /// Reads by file.
    reads_by_file: RefCell<BTreeMap<String, usize>>,
}

impl Card {
    fn new(path: &str, book: Vec<u8>) -> Card {
        Card {
            files: MemoryStorage::new().with(path, book),
            reads: Cell::new(0),
            writes: Cell::new(0),
            reads_by_file: RefCell::new(BTreeMap::new()),
        }
    }

    /// Reads so far of the files whose path ends in `end`.
    fn reads_of(&self, end: &str) -> usize {
        let reads = self.reads_by_file.borrow();
        reads
            .iter()
            .filter(|(path, _)| path.ends_with(end))
            .map(|(_, n)| n)
            .sum()
    }

    /// The bytes of all files below `dir`.
    fn bytes_below(&self, dir: &str) -> u64 {
        let mut files = Vec::new();
        list_all(&self.files, dir, &mut files);
        files.iter().map(|f| self.files.size(f).unwrap_or(0)).sum()
    }
}

fn list_all(storage: &MemoryStorage, dir: &str, out: &mut Vec<String>) {
    let mut entries = Vec::new();
    let _ = storage.list(dir, &mut |entry| {
        entries.push((format!("{dir}/{}", entry.name), entry.is_dir))
    });
    for (path, is_dir) in entries {
        if is_dir {
            list_all(storage, &path, out);
        } else {
            out.push(path);
        }
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        paused(|| {
            self.reads.set(self.reads.get() + 1);
            *self
                .reads_by_file
                .borrow_mut()
                .entry(String::from(path))
                .or_default() += 1;
            self.files.read_at(path, offset, buf)
        })
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        paused(|| self.files.size(path))
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        paused(|| {
            self.writes.set(self.writes.get() + 1);
            self.files.write(path, data)
        })
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        paused(|| {
            self.writes.set(self.writes.get() + 1);
            self.files.append(path, data)
        })
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        paused(|| self.files.remove(path))
    }

    fn exists(&self, path: &str) -> bool {
        paused(|| self.files.exists(path))
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        paused(|| self.files.rename(from, to))
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        paused(|| self.files.create_dir(path))
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        paused(|| self.files.remove_dir_all(path))
    }
}

const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#;

/// An EPUB of one chapter, `ch1.xhtml`, with `extra` files beside it;
/// those named `.css` are its stylesheets.
fn epub_with(chapter: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let stylesheets: String = extra
        .iter()
        .filter_map(|(name, _)| name.strip_prefix("OEBPS/"))
        .filter(|name| name.ends_with(".css"))
        .enumerate()
        .map(|(i, name)| format!("<item id=\"s{i}\" href=\"{name}\" media-type=\"text/css\"/>"))
        .collect();
    let opf = format!(
        "<?xml version=\"1.0\"?><package xmlns=\"http://www.idpf.org/2007/opf\" \
         version=\"3.0\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
         <dc:title>Hostile</dc:title></metadata><manifest>\
         <item id=\"c1\" href=\"ch1.xhtml\" media-type=\"application/xhtml+xml\"/>\
         {stylesheets}</manifest><spine><itemref idref=\"c1\"/></spine></package>"
    );
    let mut zip = ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated("META-INF/container.xml", CONTAINER)
        .deflated("OEBPS/content.opf", opf.as_str())
        .deflated("OEBPS/ch1.xhtml", chapter);
    for (name, data) in extra {
        zip = zip.deflated(name, data.to_vec());
    }
    zip.build()
}

fn epub(chapter: &str) -> Vec<u8> {
    epub_with(chapter, &[])
}

/// What reading a book gave: its blocks, words and last block's text
/// (the texts themselves are not kept, so as not to count them as heap).
#[derive(Debug, Default, PartialEq, Eq)]
struct Read {
    blocks: usize,
    words: usize,
    last: String,
}

/// Reads every block of every section.
fn read_all(doc: &dyn Document, storage: &dyn Storage) -> Read {
    let mut read = Read::default();
    for section in 0..doc.sections() {
        let mut at = Locator::new(section, 0);
        while let Ok(Some(block)) = doc.block_at(storage, at) {
            assert!(block.end > at.offset, "no progress at {at:?}");
            at = Locator::new(section, block.end);
            read.blocks += 1;
            read.words += block.text.split_whitespace().count();
            read.last = block.text;
        }
    }
    read
}

/// Opens and reads an EPUB on `card`; the most heap it took.
fn open_and_read(card: &Card) -> (Read, usize) {
    peak(|| {
        let doc = EpubDocument::open(card, "book.epub").expect("opens");
        read_all(&doc, card)
    })
}

/// The most heap a book may take to be opened and read. A plain book
/// takes about 55 KiB here (on 64 bits), mostly the archive's inflater;
/// the bound leaves room for what a hostile one fills up to its limits
/// (deep elements, the places of many text nodes), not for growth with
/// it.
const PLAIN_PEAK: usize = 128 * 1024;

#[test]
fn deeply_nested_elements_take_little_memory_and_storage() {
    // Unclosed elements, each with a little text, as careless markup has.
    let mut chapter = String::from("<html><body>");
    for i in 0..20_000 {
        chapter.push_str(&format!("<span class=\"c{}\">w{i} ", i % 7));
    }
    chapter.push_str("</body></html>");
    let card = Card::new("book.epub", epub(&chapter));
    let (read, peak) = open_and_read(&card);
    assert!(peak < PLAIN_PEAK, "{peak} bytes at once");
    assert!(read.last.ends_with("w19999"));
    // The places of its text nodes are listed briefly, so what is written
    // stays near what the chapter says.
    let written = card.bytes_below(".paperpaw");
    assert!(
        written < 16 * chapter.len() as u64,
        "{written} bytes written"
    );
}

#[test]
fn unclosed_paragraphs_are_ended_by_the_next() {
    let mut chapter = String::from("<html><body>");
    for i in 0..1500 {
        chapter.push_str(&format!("<p>Paragraph {i}."));
    }
    chapter.push_str("</body></html>");
    let card = Card::new("book.epub", epub(&chapter));
    let (read, peak) = open_and_read(&card);
    assert!(peak < PLAIN_PEAK, "{peak} bytes at once");
    assert_eq!(read.blocks, 1500);
    assert_eq!(read.last, "Paragraph 1499.");
}

#[test]
fn many_names_long_names_and_nested_links_take_little_memory() {
    let mut chapter = String::from("<html><body><div>");
    // Children of many names, and names as long as markup may be.
    for i in 0..20_000 {
        chapter.push_str(&format!("<x{i}>a</x{i}>"));
    }
    let long = "n".repeat(3000);
    for _ in 0..100 {
        chapter.push_str(&format!("<{long}>b"));
    }
    for _ in 0..100 {
        chapter.push_str(&format!("</{long}>"));
    }
    // Links nested in links, each with a long target.
    let target = format!("https://example.org/{}", "t".repeat(1900));
    for _ in 0..100 {
        chapter.push_str(&format!("<a href=\"{target}\">c "));
    }
    chapter.push_str("</div></body></html>");
    let card = Card::new("book.epub", epub(&chapter));
    let (read, peak) = open_and_read(&card);
    assert!(peak < PLAIN_PEAK, "{peak} bytes at once");
    assert!(read.last.ends_with('c'));
}

#[test]
fn a_block_of_many_text_nodes_is_continued() {
    // One paragraph, its words alternately bold and not.
    let mut chapter = String::from("<html><body><p>");
    for i in 0..3000 {
        chapter.push_str(&format!("<b>{i}</b> {i} "));
    }
    chapter.push_str("</p></body></html>");
    let card = Card::new("book.epub", epub(&chapter));
    let (read, peak) = open_and_read(&card);
    assert!(peak < PLAIN_PEAK, "{peak} bytes at once");
    assert!(read.blocks > 1);
    assert_eq!(read.words, 6000);
    assert!(read.last.ends_with(" 2999"));
}

#[test]
fn many_pictures_are_listed_on_the_card() {
    let mut chapter = String::from("<html><body>");
    for i in 0..5000 {
        chapter.push_str(&format!("<img src=\"p{i}.png\" alt=\"{i}\"/>"));
    }
    chapter.push_str("</body></html>");
    let card = Card::new("book.epub", epub(&chapter));
    let (read, peak) = open_and_read(&card);
    assert!(peak < PLAIN_PEAK, "{peak} bytes at once");
    // None of them is in the book: each is its alternative text.
    assert_eq!(read.blocks, 5000);
    assert_eq!(read.last, "4999");
}

#[test]
fn what_a_chapter_writes_is_bounded_by_its_size() {
    // Little markup, many places to list: text nodes deep down with long
    // names, each of one character.
    let mut chapter = String::from("<html><body>");
    for depth in 0..8 {
        chapter.push_str(&format!("<{}{depth}>", "d".repeat(30)));
    }
    for _ in 0..200_000 {
        chapter.push_str("<b>x</b>");
    }
    chapter.push_str("</body></html>");
    let card = Card::new("book.epub", epub(&chapter));
    let (read, peak) = open_and_read(&card);
    assert!(peak < PLAIN_PEAK, "{peak} bytes at once");
    assert!(read.blocks > 0);
    let written = card.bytes_below(".paperpaw");
    assert!(
        written < 16 * chapter.len() as u64 + (2 << 20),
        "{written} bytes written"
    );
    // Opened again, it is read from the cache as it is.
    let writes = card.writes.get();
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    assert_eq!(read_all(&doc, &card), read);
    assert_eq!(card.writes.get(), writes);
}

/// `zip` with the data of entry `name` damaged, so it cannot be inflated.
fn damage(mut zip: Vec<u8>, name: &str) -> Vec<u8> {
    let at = zip
        .windows(4)
        .enumerate()
        .filter(|(_, w)| *w == b"PK\x03\x04")
        .map(|(i, _)| i)
        .find(|&i| {
            let len = usize::from(u16::from_le_bytes([zip[i + 26], zip[i + 27]]));
            zip.get(i + 30..i + 30 + len) == Some(name.as_bytes())
        })
        .expect("entry");
    let name_len = usize::from(u16::from_le_bytes([zip[at + 26], zip[at + 27]]));
    let extra = usize::from(u16::from_le_bytes([zip[at + 28], zip[at + 29]]));
    let data = at + 30 + name_len + extra;
    // A deflate block of the reserved type.
    zip[data..data + 8].fill(0xff);
    zip
}

#[test]
fn a_chapter_that_cannot_be_read_is_not_converted_again_and_again() {
    let chapter = format!("<html><body><p>{}</p></body></html>", "word ".repeat(2000));
    let card = Card::new("book.epub", damage(epub(&chapter), "OEBPS/ch1.xhtml"));
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    assert!(doc.block_at(&card, Locator::START).is_err());
    let (reads, writes) = (card.reads.get(), card.writes.get());
    for _ in 0..10 {
        assert!(doc.block_at(&card, Locator::START).is_err());
    }
    assert_eq!(card.writes.get(), writes, "converted again");
    assert!(card.reads.get() - reads <= 10, "inflated again");
}

#[test]
fn a_stylesheet_in_latin_1_keeps_its_rules() {
    let css = b"/* \xa9 2010 Publisher */ .c { text-align: center }";
    let chapter = "<html><body><p class=\"c\">Middle</p></body></html>";
    let card = Card::new("book.epub", epub_with(chapter, &[("OEBPS/style.css", css)]));
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    let block = doc.block_at(&card, Locator::START).unwrap().unwrap();
    assert_eq!(block.align, Align::Center);
}

#[test]
fn a_stylesheet_of_a_bad_byte_and_much_space_takes_little_memory() {
    let mut css = b".c { text-align: center } \xff".to_vec();
    css.resize(css.len() + (1 << 20), b' ');
    let chapter = "<html><body><p class=\"c\">Middle</p></body></html>";
    let card = Card::new(
        "book.epub",
        epub_with(chapter, &[("OEBPS/style.css", &css)]),
    );
    let (block, peak) = peak(|| {
        let doc = EpubDocument::open(&card, "book.epub").unwrap();
        doc.block_at(&card, Locator::START).unwrap().unwrap()
    });
    assert!(peak < PLAIN_PEAK, "{peak} bytes at once");
    assert_eq!(block.align, Align::Center);
}

#[test]
fn positions_in_a_long_chapter_take_little_memory() {
    // Its list of places is over a megabyte.
    let mut chapter = String::from("<html><body>");
    for _ in 0..80_000 {
        chapter.push_str("<p>x</p>");
    }
    chapter.push_str("</body></html>");
    let card = Card::new("book.epub", epub(&chapter));
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    // Converted, its length is known.
    doc.block_at(&card, Locator::START).unwrap();
    let length = doc.section_len(0);
    let last = doc
        .block_at(&card, Locator::new(0, length - 1))
        .unwrap()
        .unwrap();
    let (xpointer, heap) = peak(|| doc.xpointer(&card, Locator::new(0, last.start)));
    assert!(heap < 16 * 1024, "{heap} bytes at once");
    let xpointer = xpointer.unwrap();
    assert_eq!(xpointer, "/body/DocFragment[1]/body/p[80000]/text()[1].0");
    let (at, heap) = peak(|| doc.locate_xpointer(&card, &xpointer));
    assert!(heap < 16 * 1024, "{heap} bytes at once");
    assert_eq!(at, Some(Locator::new(0, last.start)));
    let element = "/body/DocFragment[1]/body/p[79999]";
    let before = doc.locate_xpointer(&card, element).unwrap();
    assert_eq!(
        doc.block_at(&card, before).unwrap().unwrap().end,
        last.start
    );
}

#[test]
fn stale_positions_are_refused() {
    let chapter = "<html><body><p>Only <a href=\"#x\">this</a>.</p></body></html>";
    let card = Card::new("book.epub", epub(chapter));
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    // A section the book does not have, e.g. from before it changed.
    assert!(doc.block_at(&card, Locator::new(9, 0)).unwrap().is_none());
    assert!(doc.links(&card, 9, 0, u32::MAX).is_empty());
    assert_eq!(doc.xpointer(&card, Locator::new(9, 0)), None);
    assert_eq!(doc.page_label(&card, Locator::new(9, 0)), None);
    // A position far past the chapter's end.
    doc.block_at(&card, Locator::START).unwrap();
    let (xpointer, heap) = peak(|| doc.xpointer(&card, Locator::new(0, 50 << 20)));
    assert!(heap < 16 * 1024, "{heap} bytes at once");
    assert_eq!(xpointer.as_deref(), Some("/body/DocFragment[1]/body/p[1]"));
}

#[test]
fn the_contents_of_a_long_fictionbook_are_found_in_few_reads() {
    let mut book = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?><FictionBook><body>");
    for i in 0..1000 {
        book.push_str(&format!(
            "<section id=\"s{i}\"><title><p>Part {i}</p></title><p>Text {i}.</p></section>"
        ));
    }
    book.push_str("</body></FictionBook>");
    let card = Card::new("book.fb2", book.into_bytes());
    let doc = Fb2Document::open(&card, "book.fb2", true).unwrap();
    assert_eq!(doc.toc_len(&card), 1000);
    doc.block_at(&card, Locator::START).unwrap();
    // As the reader goes through them: entries, then their places.
    let before = card.reads_of("/anc");
    let mut starts = Vec::new();
    for i in 0..1000 {
        let entry = doc.toc_entry(&card, i).unwrap();
        starts.push(doc.toc_target(&card, i).unwrap());
        assert_eq!(entry.at, starts[i as usize]);
    }
    let reads = card.reads_of("/anc") - before;
    assert!(reads < 5000, "{reads} reads");
    assert!(starts.windows(2).all(|w| w[0] < w[1]));
    let last = doc.block_at(&card, starts[999]).unwrap().unwrap();
    assert_eq!(last.text, "Part 999");
}

/// An EPUB of one chapter and a navigation document listing printed
/// pages.
fn epub_with_pages(chapter: &str, nav: &str) -> Vec<u8> {
    let opf = "<?xml version=\"1.0\"?><package xmlns=\"http://www.idpf.org/2007/opf\" \
        version=\"3.0\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
        <dc:title>Pages</dc:title></metadata><manifest>\
        <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\
        <item id=\"c1\" href=\"ch1.xhtml\" media-type=\"application/xhtml+xml\"/>\
        </manifest><spine><itemref idref=\"c1\"/></spine></package>";
    ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated("META-INF/container.xml", CONTAINER)
        .deflated("OEBPS/content.opf", opf)
        .deflated("OEBPS/nav.xhtml", nav)
        .deflated("OEBPS/ch1.xhtml", chapter)
        .build()
}

#[test]
fn printed_pages_are_resolved_in_few_reads_and_little_memory() {
    let (mut chapter, mut nav) = (
        String::from("<html><body>"),
        String::from("<html><body><nav epub:type=\"page-list\"><ol>"),
    );
    for i in 0..2000 {
        chapter.push_str(&format!(
            "<p><span id=\"page{i}\"/>Page {i} begins here.</p>"
        ));
        nav.push_str(&format!(
            "<li><a href=\"ch1.xhtml#page{i}\">{}</a></li>",
            i + 1
        ));
    }
    chapter.push_str("</body></html>");
    nav.push_str("</ol></nav></body></html>");
    let card = Card::new("book.epub", epub_with_pages(&chapter, &nav));
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    doc.block_at(&card, Locator::START).unwrap();
    let before = card.reads_of("/anc") + card.reads_of("pages.txt");
    let (labels, heap) = peak(|| {
        let mut at = Locator::START;
        let mut labels = 0;
        while let Some(block) = doc.block_at(&card, at).unwrap() {
            let label = doc.page_label(&card, Locator::new(0, block.start)).unwrap();
            assert_eq!(label, (labels + 1).to_string(), "{}", block.text);
            labels += 1;
            at = Locator::new(0, block.end);
        }
        labels
    });
    assert_eq!(labels, 2000);
    assert!(heap < 64 * 1024, "{heap} bytes at once");
    let reads = card.reads_of("/anc") + card.reads_of("pages.txt") - before;
    assert!(reads < 4 * 2000, "{reads} reads");
}

#[test]
fn a_web_page_is_read_once_until_it_changes() {
    let page = format!(
        "<html lang=\"en\"><head><title>A page</title></head><body><main>{}</main></body></html>",
        "<p>Some text of the page.</p>".repeat(2000)
    );
    let card = Card::new("page.html", page.into_bytes());
    let doc = HtmlDocument::open(&card, "page.html", true).unwrap();
    doc.block_at(&card, Locator::START).unwrap();
    let before = card.reads_of("page.html");
    let again = HtmlDocument::open(&card, "page.html", true).unwrap();
    assert_eq!(again.metadata().title.as_deref(), Some("A page"));
    assert_eq!(again.metadata().language.as_deref(), Some("en"));
    let first = again.block_at(&card, Locator::START).unwrap().unwrap();
    assert_eq!(first.text, "Some text of the page.");
    // Its first bytes, to know it again.
    let reads = card.reads_of("page.html") - before;
    assert!(reads <= 2, "{reads} reads");
}

#[test]
fn markdown_is_converted_into_its_own_cache() {
    let card = Card::new("notes.md", b"# Notes\n\nSome *text*.\n".to_vec());
    let doc = MarkdownDocument::open(&card, "notes.md", true).unwrap();
    assert_eq!(
        doc.block_at(&card, Locator::START).unwrap().unwrap().text,
        "Notes"
    );
    // All of it in the folder kept for the file (by its key), none in
    // a folder of an internal path that no book has.
    let mut files = Vec::new();
    list_all(&card.files, ".paperpaw/plugins/format-epub", &mut files);
    let key = paperpaw_sdk::library::key("notes.md");
    assert!(!files.is_empty());
    for file in files {
        assert!(file.contains(&format!("/md-{key}/")), "{file}");
    }
}

/// An EPUB of package document `opf` and one chapter, `ch1.xhtml`.
fn epub_with_opf(opf: &str, chapter: &str) -> Vec<u8> {
    ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated("META-INF/container.xml", CONTAINER)
        .deflated("OEBPS/content.opf", opf)
        .deflated("OEBPS/ch1.xhtml", chapter)
        .build()
}

#[test]
fn a_package_of_endless_metadata_is_read_once_in_bounded_memory() {
    let mut opf = String::from(
        "<?xml version=\"1.0\"?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\">\
         <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>",
    );
    opf.push_str(&"Title ".repeat(50_000));
    opf.push_str("</dc:title>");
    for i in 0..20_000 {
        opf.push_str(&format!(
            "<dc:subject>Subject {i}</dc:subject><dc:creator>A{i}</dc:creator>"
        ));
    }
    opf.push_str(
        "</metadata><manifest><item id=\"c1\" href=\"ch1.xhtml\" \
         media-type=\"application/xhtml+xml\"/></manifest><spine>",
    );
    for _ in 0..20_000 {
        opf.push_str("<itemref idref=\"c1\"/>");
    }
    opf.push_str("</spine></package>");
    let chapter = "<html><body><p>Text.</p></body></html>";
    let card = Card::new("book.epub", epub_with_opf(&opf, chapter));
    let (doc, heap) = peak(|| EpubDocument::open(&card, "book.epub").unwrap());
    assert!(heap < 2 * PLAIN_PEAK, "{heap} bytes at once");
    let metadata = doc.metadata();
    assert!(metadata.title.as_ref().unwrap().len() <= 512);
    assert!(metadata.subjects.len() <= 32 && metadata.authors.len() <= 16);
    assert_eq!(doc.sections(), 4096);
    // Opened again, from what was kept of it.
    let writes = card.writes.get();
    let again = EpubDocument::open(&card, "book.epub").unwrap();
    assert_eq!(card.writes.get(), writes, "the package was read again");
    assert_eq!(again.metadata(), metadata);
}

/// A PNG whose header says its size, `width` by `height`, and whose data
/// is missing.
fn broken_png(width: u32, height: u32) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend_from_slice(&width.to_be_bytes());
    png.extend_from_slice(&height.to_be_bytes());
    png.extend_from_slice(&[8, 0, 0, 0, 0, 0, 0, 0, 0]);
    png.resize(200, 0);
    png
}

#[test]
fn a_picture_that_cannot_be_decoded_is_tried_once() {
    let chapter = "<html><body><img src=\"bad.png\" alt=\"Broken\"/></body></html>";
    let card = Card::new(
        "book.epub",
        epub_with(chapter, &[("OEBPS/bad.png", &broken_png(300, 200))]),
    );
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    let block = doc.block_at(&card, Locator::START).unwrap().unwrap();
    let image = block.image.expect("laid out at its size");
    assert_eq!(doc.prepare_image(&card, &image), None);
    let reads = card.reads_of("book.epub");
    for _ in 0..5 {
        assert_eq!(doc.prepare_image(&card, &image), None);
    }
    assert_eq!(card.reads_of("book.epub"), reads, "decoded again");
    // Its alternative text from now on.
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    let block = doc.block_at(&card, Locator::START).unwrap().unwrap();
    assert_eq!((block.image, block.text.as_str()), (None, "Broken"));
}

#[test]
fn a_picture_of_no_size_is_not_laid_out() {
    let chapter = "<html><body><img src=\"none.png\" alt=\"None\"/></body></html>";
    let card = Card::new(
        "book.epub",
        epub_with(chapter, &[("OEBPS/none.png", &broken_png(0, 0))]),
    );
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    let block = doc.block_at(&card, Locator::START).unwrap().unwrap();
    assert_eq!((block.image, block.text.as_str()), (None, "None"));
}

#[test]
fn a_comic_page_or_cover_that_cannot_be_decoded_is_tried_once() {
    let zip = ZipBuilder::new()
        .stored("1.jpg", b"\xff\xd8\xff\xe0 not really a picture".to_vec())
        .build();
    let card = Card::new("book.cbz", zip);
    let doc = paperpaw_epub_family::CbzDocument::open(&card, "book.cbz").unwrap();
    assert!(
        doc.block_at(&card, Locator::START)
            .unwrap()
            .unwrap()
            .image
            .is_none()
    );
    let reads = card.reads_of("book.cbz");
    for _ in 0..5 {
        assert!(
            doc.block_at(&card, Locator::START)
                .unwrap()
                .unwrap()
                .image
                .is_none()
        );
        assert!(doc.cover(&card).unwrap().is_none());
    }
    // The cover once more, as it is its own picture.
    let once = card.reads_of("book.cbz") - reads;
    assert!(doc.cover(&card).unwrap().is_none());
    assert_eq!(card.reads_of("book.cbz") - reads, once, "decoded again");

    let book = "<?xml version=\"1.0\" encoding=\"utf-8\"?><FictionBook \
        xmlns:l=\"http://www.w3.org/1999/xlink\"><description><title-info>\
        <coverpage><image l:href=\"#c\"/></coverpage></title-info></description>\
        <body><section><p>Text.</p></section></body>\
        <binary id=\"c\" content-type=\"image/png\">iVBORw0KGgo=</binary></FictionBook>";
    let card = Card::new("book.fb2", book.as_bytes().to_vec());
    let doc = Fb2Document::open(&card, "book.fb2", true).unwrap();
    assert!(doc.cover(&card).unwrap().is_none());
    let reads = card.reads_of("book.fb2");
    for _ in 0..5 {
        assert!(doc.cover(&card).unwrap().is_none());
    }
    assert_eq!(card.reads_of("book.fb2"), reads, "read through again");
}

#[test]
fn the_contents_of_a_fictionbook_are_kept_on_the_card() {
    let mut book = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?><FictionBook><body>");
    let title = "A long title of a section, as some books have them. ".repeat(4);
    for i in 0..5000 {
        book.push_str(&format!(
            "<section><title><p>{i} {title}</p></title><p>T.</p></section>"
        ));
    }
    book.push_str("</body></FictionBook>");
    let card = Card::new("book.fb2", book.into_bytes());
    let (doc, heap) = peak(|| Fb2Document::open(&card, "book.fb2", true).unwrap());
    assert!(heap < 64 * 1024, "{heap} bytes at once");
    assert_eq!(doc.toc_len(&card), 5000);
    let (again, heap) = peak(|| Fb2Document::open(&card, "book.fb2", true).unwrap());
    assert!(heap < 16 * 1024, "{heap} bytes at once");
    let entry = again.toc_entry(&card, 4999).unwrap();
    assert!(entry.title.starts_with("4999 A long title"));
    let block = again.block_at(&card, entry.at).unwrap().unwrap();
    assert!(block.text.starts_with("4999 A long title"));
}

#[test]
fn markdown_of_one_long_line_or_endless_brackets_takes_little() {
    let mut md = "word ".repeat(40_000);
    md.push_str(&"[".repeat(50_000));
    md.push_str("](x)");
    let card = Card::new("notes.md", md.into_bytes());
    let (read, heap) = peak(|| {
        let doc = MarkdownDocument::open(&card, "notes.md", true).unwrap();
        read_all(&doc, &card)
    });
    assert!(heap < PLAIN_PEAK, "{heap} bytes at once");
    assert!(read.words >= 40_000);
}

#[test]
fn the_first_pages_of_a_long_comic_are_kept_and_not_held_in_memory() {
    // More pictures than are kept, the last in reading order first.
    let mut zip = ZipBuilder::new();
    for i in (0..6000).rev() {
        zip = zip.stored(&format!("p{i:05}.jpg"), b"not a picture".to_vec());
    }
    let card = Card::new("book.cbz", zip.build());
    let doc = paperpaw_epub_family::CbzDocument::open(&card, "book.cbz").unwrap();
    assert_eq!(doc.sections(), 5000);
    // Pages that cannot be shown are named.
    let first = doc.block_at(&card, Locator::START).unwrap().unwrap();
    assert_eq!(first.text, "p00000.jpg");
    let last = doc.block_at(&card, Locator::new(4999, 0)).unwrap().unwrap();
    assert_eq!(last.text, "p04999.jpg");
    // Opened again, the order is read from the cache, not made again.
    let (again, heap) =
        peak(|| paperpaw_epub_family::CbzDocument::open(&card, "book.cbz").unwrap());
    assert!(heap < 64 * 1024, "{heap} bytes at once");
    assert_eq!(again.sections(), 5000);
}

/// The converted text of the book `book.epub`'s first chapter.
fn chapter_text() -> String {
    format!(
        "{}/sections/0/txt",
        paperpaw_sdk::library::kept_path(".paperpaw/plugins/format-epub", "", "book.epub")
    )
}

#[test]
fn a_cut_cache_is_converted_again() {
    let chapter = format!("<html><body><p>{}</p></body></html>", "Word ".repeat(500));
    let card = Card::new("book.epub", epub(&chapter));
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    let whole = read_all(&doc, &card);
    // Cut short, as by a power cut after its end was marked.
    let text = card.files.get(&chapter_text()).unwrap();
    card.files
        .write(&chapter_text(), &text[..text.len() / 2])
        .unwrap();
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    assert_eq!(read_all(&doc, &card), whole);
}

#[test]
fn a_damaged_cache_record_takes_little_memory() {
    let card = Card::new("book.epub", epub("<html><body><p>Text.</p></body></html>"));
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    doc.block_at(&card, Locator::START).unwrap();
    // The record's lengths at their most, the file as long as before.
    let mut text = card.files.get(&chapter_text()).unwrap();
    text[4..8].fill(0xff);
    card.files.write(&chapter_text(), &text).unwrap();
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    let (block, heap) = peak(|| doc.block_at(&card, Locator::START));
    assert!(heap < 32 * 1024, "{heap} bytes at once");
    assert!(block.is_err());
}
