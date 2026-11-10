//! Hostile packages: what `META-INF/container.xml`, the package document
//! (metadata, manifest, spine) and the tables of contents (EPUB 3
//! navigation document, EPUB 2 NCX) may say, and what opening such a book
//! must do: fail with an error, or read what can be read; never panic,
//! never write outside the plugin's cache.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Recording, blocks, container, opf, text, words, xhtml, zip};
use paperpaw_plugin_format_epub::EpubDocument;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::document::{Document, DocumentError, Locator};

const XHTML: &str = "application/xhtml+xml";

fn bytes(text: impl Into<String>) -> Vec<u8> {
    text.into().into_bytes()
}

/// Opens `epub` on a recording storage.
fn open(epub: Vec<u8>) -> (Recording, Result<EpubDocument, DocumentError>) {
    let storage = Recording::new(MemoryStorage::new().with("book.epub", epub));
    let doc = EpubDocument::open(&storage, "book.epub");
    (storage, doc)
}

/// A book of the given package and files besides the container.
fn package_book(package: String, files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut all = vec![
        (
            "META-INF/container.xml",
            bytes(container("OEBPS/content.opf")),
        ),
        ("OEBPS/content.opf", bytes(package)),
    ];
    all.extend(files.iter().cloned());
    zip(&all, false)
}

fn malformed(result: Result<EpubDocument, DocumentError>) -> bool {
    matches!(result, Err(DocumentError::Malformed(_)))
}

#[test]
fn a_book_without_container_is_malformed() {
    let epub = zip(&[("OEBPS/c.xhtml", bytes(xhtml("<p>x</p>")))], false);
    let (storage, doc) = open(epub);
    assert!(malformed(doc));
    assert_eq!(storage.written_outside(&[]), Vec::<String>::new());
}

#[test]
fn a_container_without_rootfile_is_malformed() {
    for container in [
        "<container><rootfiles/></container>",
        "<container><rootfiles><rootfile media-type=\"x\"/></rootfiles></container>",
        "",
        "\u{feff}<<<>>>",
    ] {
        let epub = zip(&[("META-INF/container.xml", bytes(container))], false);
        let (_, doc) = open(epub);
        assert!(malformed(doc), "{container}");
    }
}

#[test]
fn a_container_pointing_outside_or_nowhere_is_malformed() {
    for target in [
        "../../content.opf",
        "/etc/passwd",
        "OEBPS/missing.opf",
        "C:\\Windows\\content.opf",
        "",
        "META-INF/container.xml/..",
    ] {
        let mut files = vec![("META-INF/container.xml", bytes(container(target)))];
        // A package exists, only not where the container says.
        files.push((
            "OEBPS/content.opf",
            bytes(opf("", &[("c", "c.xhtml", XHTML, "")], &["c"])),
        ));
        files.push(("OEBPS/c.xhtml", bytes(xhtml("<p>x</p>"))));
        let (storage, doc) = open(zip(&files, false));
        assert!(malformed(doc), "{target}");
        assert_eq!(storage.written_outside(&[]), Vec::<String>::new());
    }
}

#[test]
fn a_container_naming_itself_as_package_is_malformed() {
    let epub = zip(
        &[(
            "META-INF/container.xml",
            bytes(container("META-INF/container.xml")),
        )],
        false,
    );
    let (_, doc) = open(epub);
    // Read as a package, it has no spine.
    assert!(malformed(doc));
}

#[test]
fn hrefs_climbing_out_of_the_archive_stay_inside_it() {
    // `..` beyond the archive's root stays at the root, as in URLs.
    let package = opf(
        "",
        &[
            ("a", "../../../../top.xhtml", XHTML, ""),
            ("b", "text/../../OEBPS/./inner.xhtml", XHTML, ""),
        ],
        &["a", "b"],
    );
    let epub = package_book(
        package,
        &[
            ("top.xhtml", bytes(xhtml("<p>Top level</p>"))),
            ("OEBPS/inner.xhtml", bytes(xhtml("<p>Inner one</p>"))),
        ],
    );
    let (storage, doc) = open(epub);
    let doc = doc.unwrap();
    assert_eq!(doc.sections(), 2);
    assert_eq!(text(&doc, &storage), "Top level\nInner one");
    assert_eq!(storage.written_outside(&[]), Vec::<String>::new());
}

#[test]
fn percent_encoded_hrefs_and_fragments_resolve() {
    let package = opf(
        "",
        &[
            ("nav", "nav.xhtml", XHTML, "nav"),
            ("a", "text/ch%201.xhtml", XHTML, ""),
            // A fragment on a manifest item names the file all the same.
            ("b", "text/%C3%BCber.xhtml#ignored", XHTML, ""),
        ],
        &["a", "b"],
    );
    let nav = "<html><body><nav epub:type=\"toc\"><ol>\
        <li><a href=\"text/ch%201.xhtml\">One</a></li>\
        <li><a href=\"text/%C3%BCber.xhtml#s%C3%A9c\">Two</a></li>\
        <li><a href=\"text/ch%201.xhtml#%\">Broken escape</a></li>\
        </ol></nav></body></html>";
    let epub = package_book(
        package,
        &[
            ("OEBPS/nav.xhtml", bytes(nav)),
            ("OEBPS/text/ch 1.xhtml", bytes(xhtml("<p>First</p>"))),
            (
                "OEBPS/text/über.xhtml",
                bytes(xhtml("<p>Before</p><p id=\"séc\">Section</p>")),
            ),
        ],
    );
    let (storage, doc) = open(epub);
    let doc = doc.unwrap();
    assert_eq!(text(&doc, &storage), "First\nBefore\nSection");
    assert_eq!(doc.toc_len(&storage), 3);
    let target = doc.toc_target(&storage, 1).unwrap();
    assert_eq!(target.section, 1);
    let block = doc.block_at(&storage, target).unwrap().unwrap();
    assert_eq!(block.text, "Section");
    // A broken escape is kept as written: an anchor that is not there
    // leads to the chapter's start.
    assert_eq!(doc.toc_target(&storage, 2), Some(Locator::new(0, 0)));
}

#[test]
fn spine_items_that_are_missing_are_skipped() {
    let package = opf(
        "",
        &[
            ("a", "a.xhtml", XHTML, ""),
            ("gone", "gone.xhtml", XHTML, ""),
            ("b", "b.xhtml", XHTML, ""),
        ],
        // `ghost` is in no item, `gone` in no file.
        &["ghost", "a", "gone", "b", ""],
    );
    let epub = package_book(
        package,
        &[
            ("OEBPS/a.xhtml", bytes(xhtml("<p>Alpha</p>"))),
            ("OEBPS/b.xhtml", bytes(xhtml("<p>Beta</p>"))),
        ],
    );
    let (storage, doc) = open(epub);
    let doc = doc.unwrap();
    assert_eq!(doc.sections(), 2);
    assert_eq!(text(&doc, &storage), "Alpha\nBeta");
}

#[test]
fn a_spine_of_nothing_readable_is_malformed() {
    for spine in [&[][..], &["ghost"][..], &["gone", "gone"][..]] {
        let package = opf("", &[("gone", "gone.xhtml", XHTML, "")], spine);
        let (_, doc) = open(package_book(package, &[]));
        assert!(malformed(doc), "{spine:?}");
    }
}

#[test]
fn a_chapter_listed_twice_is_read_twice() {
    // A cycle of sorts: the spine comes back to where it began.
    let package = opf(
        "",
        &[("a", "a.xhtml", XHTML, ""), ("b", "b.xhtml", XHTML, "")],
        &["a", "b", "a", "a"],
    );
    let epub = package_book(
        package,
        &[
            ("OEBPS/a.xhtml", bytes(xhtml("<p>Again</p>"))),
            ("OEBPS/b.xhtml", bytes(xhtml("<p>Once</p>"))),
        ],
    );
    let (storage, doc) = open(epub);
    let doc = doc.unwrap();
    assert_eq!(doc.sections(), 4);
    assert_eq!(words(&doc, &storage), ["Again", "Once", "Again", "Again"]);
}

#[test]
fn a_spine_naming_the_package_or_a_picture_reads_without_panic() {
    // The package itself and a picture as chapters: whatever text they
    // give, the book opens and every section can be read to its end.
    let package = opf(
        "<dc:title>Self</dc:title>",
        &[
            ("self", "content.opf", "application/oebps-package+xml", ""),
            ("pic", "p.png", "image/png", ""),
            ("mime", "../mimetype", "text/plain", ""),
            ("a", "a.xhtml", XHTML, ""),
        ],
        &["self", "pic", "mime", "a"],
    );
    let epub = package_book(
        package,
        &[
            ("OEBPS/p.png", vec![0x89, b'P', b'N', b'G', 0, 1, 2, 3]),
            ("OEBPS/a.xhtml", bytes(xhtml("<p>Real text</p>"))),
        ],
    );
    let (storage, doc) = open(epub);
    let doc = doc.unwrap();
    assert_eq!(doc.sections(), 4);
    let read = blocks(&doc, &storage);
    assert_eq!(
        read.last().map(|(s, b)| (*s, b.text.as_str())),
        Some((3, "Real text"))
    );
}

#[test]
fn metadata_of_any_size_is_read_and_bounded() {
    let long = "Wort ".repeat(50_000);
    let escaped = "&lt;p&gt;Blurb &amp;amp; more&lt;/p&gt;".repeat(5_000);
    let metadata = format!(
        "<dc:title>  Title\n\t of  the   book </dc:title>\
         <dc:title>Second title</dc:title>\
         <dc:creator></dc:creator><dc:creator>A &amp; B</dc:creator>\
         <dc:language>  </dc:language><dc:language>de</dc:language>\
         <dc:subject>{long}</dc:subject>\
         <dc:description>{escaped}</dc:description>\
         <meta name=\"calibre:series\" content=\"  The   Series \"/>\
         <meta name=\"calibre:series_index\" content=\"2.5\"/>"
    );
    let package = opf(&metadata, &[("a", "a.xhtml", XHTML, "")], &["a"]);
    let epub = package_book(package, &[("OEBPS/a.xhtml", bytes(xhtml("<p>x</p>")))]);
    let (_, doc) = open(epub);
    let doc = doc.unwrap();
    let m = doc.metadata();
    assert_eq!(m.title.as_deref(), Some("Title of the book"));
    assert_eq!(m.authors, ["A & B"]);
    assert_eq!(m.language.as_deref(), Some("de"));
    assert_eq!(m.series.as_deref(), Some("The Series"));
    assert_eq!(m.series_index.as_deref(), Some("2.5"));
    // The blurb is plain text and cut.
    let description = m.description.as_deref().unwrap();
    assert!(
        description.starts_with("Blurb & more Blurb"),
        "{description:.40}"
    );
    assert!(description.len() <= 2000 + '…'.len_utf8());
    assert!(!description.contains('<'));
}

#[test]
fn a_book_with_long_metadata_reuses_its_cache() {
    // `package.ini` was read back with a limit of 16 KB; a longer one
    // (long titles, many subjects) was never found again: each open read
    // the package anew and dropped the converted chapters. What is kept of
    // a package is bounded now (32 subjects), and is found again.
    let subjects: String = (0..2_000)
        .map(|i| format!("<dc:subject>Subject number {i}</dc:subject>"))
        .collect();
    let package = opf(
        &format!("<dc:title>T</dc:title>{subjects}"),
        &[("a", "a.xhtml", XHTML, "")],
        &["a"],
    );
    let epub = package_book(package, &[("OEBPS/a.xhtml", bytes(xhtml("<p>x</p>")))]);
    let (storage, doc) = open(epub);
    assert_eq!(doc.unwrap().metadata().subjects.len(), 32);
    storage.written.borrow_mut().clear();
    let again = EpubDocument::open(&storage, "book.epub").unwrap();
    assert_eq!(again.metadata().subjects.len(), 32);
    let rewritten: Vec<String> = storage
        .written
        .borrow()
        .iter()
        .filter(|p| p.ends_with("package.ini"))
        .cloned()
        .collect();
    assert_eq!(rewritten, Vec::<String>::new(), "package read again");
}

/// A navigation document of `depth` nested lists, an entry at each.
fn nested_nav(depth: usize, href: &str) -> String {
    let mut nav = String::from(
        "<html xmlns:epub=\"http://www.idpf.org/2007/ops\"><body><nav epub:type=\"toc\">",
    );
    for level in 0..depth {
        nav.push_str(&format!("<ol><li><a href=\"{href}\">Level {level}</a>"));
    }
    for _ in 0..depth {
        nav.push_str("</li></ol>");
    }
    nav.push_str("</nav></body></html>");
    nav
}

fn nav_book(nav: String) -> Vec<u8> {
    let package = opf(
        "",
        &[
            ("nav", "nav.xhtml", XHTML, "nav"),
            ("a", "a.xhtml", XHTML, ""),
        ],
        &["a"],
    );
    package_book(
        package,
        &[
            ("OEBPS/nav.xhtml", bytes(nav)),
            ("OEBPS/a.xhtml", bytes(xhtml("<p id=\"x\">Text</p>"))),
        ],
    )
}

#[test]
fn deeply_nested_contents_keep_every_entry() {
    let (storage, doc) = open(nav_book(nested_nav(200, "a.xhtml#x")));
    let doc = doc.unwrap();
    assert_eq!(doc.toc_len(&storage), 200);
    for i in [0, 1, 100, 199] {
        let entry = doc.toc_entry(&storage, i).unwrap();
        assert_eq!(entry.title, format!("Level {i}"));
        assert_eq!(usize::from(entry.level), i as usize);
    }
}

#[test]
fn contents_nested_deeper_than_255_levels_do_not_panic() {
    // `ol_depth += 1` in `read_nav` overflows at the 256th list.
    let result = std::panic::catch_unwind(|| {
        let (storage, doc) = open(nav_book(nested_nav(300, "a.xhtml")));
        doc.unwrap().toc_len(&storage)
    });
    assert_eq!(result.ok(), Some(300));
}

#[test]
fn deeply_nested_ncx_keeps_every_entry() {
    let mut map = String::new();
    for level in 0..400 {
        map.push_str(&format!(
            "<navPoint id=\"n{level}\"><navLabel><text>Point {level}</text></navLabel>\
             <content src=\"a.xhtml\"/>"
        ));
    }
    map.push_str(&"</navPoint>".repeat(400));
    let ncx = format!("<ncx><navMap>{map}</navMap></ncx>");
    let package = opf(
        "",
        &[
            ("ncx", "toc.ncx", "application/x-dtbncx+xml", ""),
            ("a", "a.xhtml", XHTML, ""),
        ],
        &["a"],
    )
    .replace("<spine>", "<spine toc=\"ncx\">");
    let epub = package_book(
        package,
        &[
            ("OEBPS/toc.ncx", bytes(ncx)),
            ("OEBPS/a.xhtml", bytes(xhtml("<p>Text</p>"))),
        ],
    );
    let (storage, doc) = open(epub);
    let doc = doc.unwrap();
    assert_eq!(doc.toc_len(&storage), 400);
    let last = doc.toc_entry(&storage, 399).unwrap();
    assert_eq!(last.title, "Point 399");
    // Levels deeper than a byte holds stay at the deepest.
    assert!(last.level >= 254);
}

#[test]
fn contents_entries_leading_nowhere_are_dropped_or_go_to_the_chapter() {
    let nav = "<html><body><nav epub:type=\"toc\"><ol>\
        <li><a href=\"missing.xhtml\">Missing file</a></li>\
        <li><a href=\"https://example.com/a.xhtml\">Web</a></li>\
        <li><a href=\"a.xhtml#nowhere\">Missing anchor</a></li>\
        <li><a>No link</a></li>\
        <li><a href=\"\">Empty link</a></li>\
        <li><a href=\"a.xhtml\">   </a></li>\
        <li><a href=\"../../../../OEBPS/a.xhtml#x\">Climbing</a></li>\
        <li><a href=\"a.xhtml#x\">Fine</a></li>\
        </ol></nav></body></html>";
    let (storage, doc) = open(nav_book(nav.into()));
    let doc = doc.unwrap();
    let titles: Vec<String> = (0..doc.toc_len(&storage))
        .map(|i| doc.toc_entry(&storage, i).unwrap().title)
        .collect();
    assert_eq!(titles, ["Missing anchor", "Climbing", "Fine"]);
    assert_eq!(doc.toc_target(&storage, 0), Some(Locator::new(0, 0)));
    let fine = doc.toc_target(&storage, 2).unwrap();
    assert_eq!(doc.toc_target(&storage, 1), Some(fine));
    assert_eq!(doc.block_at(&storage, fine).unwrap().unwrap().text, "Text");
    // Past the end: none.
    assert_eq!(doc.toc_entry(&storage, 3), None);
    assert_eq!(doc.toc_target(&storage, u32::MAX), None);
}

#[test]
fn a_broken_navigation_document_falls_back_to_the_ncx() {
    let ncx = "<ncx><navMap><navPoint><navLabel><text>From NCX</text></navLabel>\
               <content src=\"a.xhtml\"/></navPoint></navMap></ncx>";
    let package = opf(
        "",
        &[
            ("nav", "nav.xhtml", XHTML, "nav"),
            ("ncx", "toc.ncx", "application/x-dtbncx+xml", ""),
            ("a", "a.xhtml", XHTML, ""),
        ],
        &["a"],
    );
    let epub = package_book(
        package,
        &[
            // No `toc` nav at all, and unclosed.
            (
                "OEBPS/nav.xhtml",
                bytes("<html><body><nav><ol><li><a href="),
            ),
            ("OEBPS/toc.ncx", bytes(ncx)),
            ("OEBPS/a.xhtml", bytes(xhtml("<p>Text</p>"))),
        ],
    );
    let (storage, doc) = open(epub);
    let doc = doc.unwrap();
    assert_eq!(doc.toc_len(&storage), 1);
    assert_eq!(doc.toc_entry(&storage, 0).unwrap().title, "From NCX");
}

#[test]
fn encrypted_chapters_read_as_whatever_they_hold() {
    // Real DRM: `encryption.xml` names a chapter whose data is ciphertext.
    // It is not readable, but the book opens and the other chapters are.
    let encryption = "<encryption><EncryptedData><EncryptionMethod \
        Algorithm=\"http://www.w3.org/2001/04/xmlenc#aes128-cbc\"/>\
        <CipherData><CipherReference URI=\"OEBPS/secret.xhtml\"/></CipherData>\
        </EncryptedData></encryption>";
    let cipher: Vec<u8> = (0..5000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let package = opf(
        "",
        &[
            ("s", "secret.xhtml", XHTML, ""),
            ("a", "a.xhtml", XHTML, ""),
        ],
        &["s", "a"],
    );
    let epub = package_book(
        package,
        &[
            ("META-INF/encryption.xml", bytes(encryption)),
            ("OEBPS/secret.xhtml", cipher),
            ("OEBPS/a.xhtml", bytes(xhtml("<p>Plain chapter</p>"))),
        ],
    );
    let (storage, doc) = open(epub);
    let doc = doc.unwrap();
    let read = blocks(&doc, &storage);
    assert_eq!(read.last().unwrap().1.text, "Plain chapter");
}

#[test]
fn paths_written_stay_in_the_cache_whatever_the_book_names() {
    // Covers, stylesheets and pictures named with paths that climb out,
    // are absolute or use backslashes.
    let package = opf(
        "<meta name=\"cover\" content=\"cov\"/>",
        &[
            (
                "cov",
                "../../../../../cover.png",
                "image/png",
                "cover-image",
            ),
            ("css", "/etc/style.css", "text/css", ""),
            ("a", "..\\..\\a.xhtml", XHTML, ""),
            ("b", "b.xhtml", XHTML, ""),
        ],
        &["a", "b"],
    );
    let chapter = xhtml(
        "<p>Pictures:</p><img src=\"../../../../../../tmp/x.png\" alt=\"one\"/>\
         <img src=\"/abs.png\" alt=\"two\"/><img src=\"C:\\x.png\" alt=\"three\"/>",
    );
    let epub = package_book(
        package,
        &[
            ("cover.png", vec![1, 2, 3]),
            ("OEBPS/b.xhtml", bytes(chapter)),
        ],
    );
    let (storage, doc) = open(epub);
    let doc = doc.unwrap();
    let _ = doc.cover(&storage);
    assert_eq!(words(&doc, &storage), ["Pictures:", "one", "two", "three"]);
    assert_eq!(storage.written_outside(&[]), Vec::<String>::new());
}
