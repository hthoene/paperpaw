//! The first walk of new books on the card as the X4 has it: what it
//! reads and writes for each (#98). A book opened for its details
//! writes next to nothing of its format's cache; what is written is the
//! library's own (each book's details, the catalog).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use super::*;
use paperpaw_zip::testing::ZipBuilder;

const BOOKS: usize = 60;

/// A small EPUB without a cover, as the X4 test's books are.
fn epub(n: usize) -> Vec<u8> {
    let opf = format!(
        r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Book {n}</dc:title><dc:creator>Author {}</dc:creator><dc:language>en</dc:language>
  </metadata>
  <manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest>
  <spine><itemref idref="c1"/></spine>
</package>"#,
        n % 7
    );
    ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated(
            "META-INF/container.xml",
            r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        )
        .deflated("content.opf", opf)
        .deflated("c1.xhtml", "<html><body><p>Text.</p></body></html>")
        .build()
}

/// What the walk took on the card, per kind of path.
static TAKEN: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

struct Walker;

impl Plugin for Walker {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("walker"), "Walker", Version::new(1, 0, 0))
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.on(events::STARTED, |env: &Env<'_>, _: &(), _| {
            let _ = tallied();
            let (read, written) = (READ.load(Relaxed), WRITTEN.load(Relaxed));
            let mut walk = paperpaw_sdk::library::catalog::Walk::start(env, Vec::new()).unwrap();
            while !walk.step(env).unwrap() {}
            let (read, written) = (READ.load(Relaxed) - read, WRITTEN.load(Relaxed) - written);
            *TAKEN.lock().unwrap() = format!("{read} r {written} w; {}", tallied());
        })
    }
}

/// Blocks written to paths of `kind` in `taken`, as [`tallied`] says.
fn written_to(taken: &str, kind: &str) -> u64 {
    taken
        .split("; ")
        .find_map(|part| part.strip_prefix(&format!("{kind}: ")))
        .and_then(|rest| rest.split(' ').nth(4))
        .map_or(0, |w| w.parse().unwrap())
}

/// Calls on paths of `kind` in `taken`, as [`tallied`] says.
fn calls_to(taken: &str, kind: &str) -> u64 {
    taken
        .split("; ")
        .find_map(|part| part.strip_prefix(&format!("{kind}: ")))
        .and_then(|rest| rest.split(' ').next())
        .map_or(0, |c| c.parse().unwrap())
}

/// Blocks read from paths of `kind` in `taken`, as [`tallied`] says.
fn read_from(taken: &str, kind: &str) -> u64 {
    taken
        .split("; ")
        .find_map(|part| part.strip_prefix(&format!("{kind}: ")))
        .and_then(|rest| rest.split(' ').nth(2))
        .map_or(0, |r| r.parse().unwrap())
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn the_first_walk_writes_next_to_nothing_of_the_books_formats() {
    // FAT32 with clusters of 32 KiB, as in the bench.
    let card = Card::new(32 << 30).unwrap();
    for n in 0..BOOKS {
        card.write(&format!("books/f{}/b{n}.epub", n % 6), &epub(n))
            .unwrap();
    }
    let mut kernel = Kernel::builder(DEVICE)
        .storage(card)
        .setting_default(&settings::LIBRARY_FOLDER, "books".into())
        .plugin(Walker)
        .build()
        .unwrap();
    kernel.start(paperpaw_platform::Instant(0));
    let taken = TAKEN.lock().unwrap().clone();
    println!("{BOOKS} books: {taken}");
    let found = paperpaw_sdk::library::catalog::count(&card, &|_| true);
    assert_eq!(found, BOOKS, "{taken}");
    // Before #98 the format's cache took about 130 blocks a book. Now a
    // block a book at most: a new book's folder is removed in case a cut
    // removal left one (the card writes back what it kept, #385 S2).
    assert!(
        written_to(&taken, ".paperpaw/plugins") <= BOOKS as u64,
        "{taken}"
    );
    // All else written is the library's own: the books' details (a
    // folder of 32 KiB made for each of their 256 groups first), the
    // catalog, the listings of the folders walked; and the above.
    let written: u64 = taken.split(' ').nth(2).unwrap().parse().unwrap();
    let own: u64 = ["details", "library files", "books", ".paperpaw/plugins"]
        .iter()
        .map(|kind| written_to(&taken, kind))
        .sum();
    assert_eq!(written, own, "{taken}");
    // Walked again without the catalog (another start, the catalog
    // lost): its books are taken from their details, read from the book
    // once (their covers not, #398): no book is opened, and nothing is
    // written of them.
    card.remove(&format!("{}/catalog.txt", paperpaw_sdk::library::DIR))
        .unwrap();
    let mut kernel = Kernel::builder(DEVICE)
        .storage(card)
        .setting_default(&settings::LIBRARY_FOLDER, "books".into())
        .plugin(Walker)
        .build()
        .unwrap();
    kernel.start(paperpaw_platform::Instant(0));
    let again = TAKEN.lock().unwrap().clone();
    println!("again: {again}");
    assert_eq!(written_to(&again, "details"), 0, "{again}");
    assert!(read_from(&again, "details") > 0, "{again}");
    // The books' folder and its six listed, no book read.
    assert_eq!(calls_to(&again, "books"), 7, "{again}");
    assert_eq!(written_to(&again, ".paperpaw/plugins"), 0, "{again}");
}
