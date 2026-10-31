//! Going from chapter to chapter, on and back. A table of contents may
//! name whole chapters: in a converted EPUB, where such an entry points
//! (a section's start) comes before the first text and so before the
//! page that shows it.

use super::*;
use paperpaw_zip::testing::ZipBuilder;

const CHAPTERS: u32 = 4;

/// An EPUB of [`CHAPTERS`] chapters of several pages each, with a table
/// of contents that names whole chapters, no place in them.
fn whole_chapters_epub() -> Vec<u8> {
    let container = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
    let (mut items, mut spine, mut toc) = (String::new(), String::new(), String::new());
    let mut book = ZipBuilder::new().stored("mimetype", "application/epub+zip");
    for chapter in 0..CHAPTERS {
        items.push_str(&format!(
            r#"<item id="c{chapter}" href="c{chapter}.xhtml" media-type="application/xhtml+xml"/>"#
        ));
        spine.push_str(&format!(r#"<itemref idref="c{chapter}"/>"#));
        toc.push_str(&format!(
            r#"<li><a href="c{chapter}.xhtml">Chapter {chapter}</a></li>"#
        ));
        let body = "<p>Some words of a paragraph that takes a few lines on any page of \
                    this book, so that a chapter of them takes several pages.</p>"
            .repeat(40);
        book = book.deflated(
            &format!("OEBPS/c{chapter}.xhtml"),
            format!(
                r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>c</title></head><body>{body}</body></html>"#
            ),
        );
    }
    let opf = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Chapters</dc:title></metadata>
  <manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>{items}</manifest>
  <spine>{spine}</spine>
</package>"#
    );
    let nav = format!(
        r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body><nav epub:type="toc"><ol>{toc}</ol></nav></body></html>"#
    );
    book.deflated("META-INF/container.xml", container)
        .deflated("OEBPS/content.opf", opf)
        .deflated("OEBPS/nav.xhtml", nav)
        .build()
}

/// What the reader said last.
fn last(reading: &Reading) -> events::Reading {
    reading.heard.borrow().last().unwrap().1.clone()
}

#[test]
fn chapters_are_jumped_to_and_turned_back_from() {
    let epub = include_bytes!("../../../../samples/library/books/alice-in-wonderland.epub");
    let mut reading = Reading::with_book(Shared::default(), EPUB, epub);
    reading.open();
    // Until every page is counted, into the first chapter (a section
    // of its own, as the chapters after it).
    let mut turned = 0;
    while last(&reading).pages.is_none() || !last(&reading).chapter.ends_with("Rabbit-Hole") {
        reading.run(core_actions::NEXT_PAGE);
        turned += 1;
        assert!(turned < 400, "the pages are never counted");
    }
    reading.run(core_actions::NEXT_PAGE);
    let before = last(&reading);
    let left = before.chapter_left.unwrap();
    // On to the next chapter's first page.
    reading.run(actions::NEXT_CHAPTER);
    let next = last(&reading);
    assert_ne!(next.chapter, before.chapter, "{before:?} {next:?}");
    assert_eq!(next.page, before.page.map(|p| p + left + 1));
    // A page back is the last of the chapter before, not the page left.
    reading.run(core_actions::PREV_PAGE);
    let end = last(&reading);
    assert_eq!(end.page, next.page.map(|p| p - 1));
    assert_eq!(end.chapter, before.chapter);
    // Back to this chapter's start, then to the start of the one before.
    reading.run(actions::PREV_CHAPTER);
    let start = last(&reading);
    assert_eq!(start.chapter, before.chapter);
    assert!(start.page < before.page, "{start:?} {before:?}");
    // From a chapter's first page, to the page the one before begins on.
    reading.run(actions::PREV_CHAPTER);
    let earlier = last(&reading);
    assert!(earlier.page < start.page, "{earlier:?} {start:?}");
}

#[test]
fn whole_chapters_begin_on_their_own_pages() {
    let in_chapter = InChapter::default();
    let mut reading = probed(&whole_chapters_epub(), in_chapter.clone());
    reading.open();
    let mut turned = 0;
    while last(&reading).pages.is_none() {
        reading.run(core_actions::NEXT_PAGE);
        turned += 1;
        assert!(turned < 400, "the pages are never counted");
    }
    reading.run(actions::PREV_CHAPTER);

    // On: each chapter from its first page, the last one's left behind.
    let mut first_pages = Vec::new();
    for chapter in 0..CHAPTERS {
        let at = last(&reading);
        assert_eq!(at.chapter, format!("Chapter {chapter}"), "{at:?}");
        assert_eq!(at.section, chapter, "{at:?}");
        first_pages.push(at.page.unwrap());
        // Status lines count from the same page.
        reading.kernel.render(&mut |_, _| {});
        let (page, _) = in_chapter.borrow().clone().expect("the line was drawn");
        assert_eq!(page, "1", "{at:?}");
        if chapter + 1 < CHAPTERS {
            let left = at.chapter_left.unwrap();
            assert!(left > 0, "a chapter of several pages: {at:?}");
            reading.run(actions::NEXT_CHAPTER);
            assert_eq!(last(&reading).page, Some(at.page.unwrap() + left + 1));
        }
    }

    // Back from inside a chapter: to its start.
    reading.run(core_actions::NEXT_PAGE);
    reading.run(actions::PREV_CHAPTER);
    assert_eq!(last(&reading).page, first_pages.last().copied());

    // Back from a chapter's first page: to the chapter before, each time.
    for chapter in (0..CHAPTERS - 1).rev() {
        reading.run(actions::PREV_CHAPTER);
        let at = last(&reading);
        assert_eq!(at.chapter, format!("Chapter {chapter}"), "{at:?}");
        assert_eq!(at.page, Some(first_pages[chapter as usize]), "{at:?}");
    }
}

/// Past the pages counted yet, where the index says nothing about which
/// page anything is on.
#[test]
fn chapters_work_where_the_index_does_not_reach() {
    let storage = Shared::default();
    // One step of eight pages, not as far as the last chapter.
    storage.1.set(Some(1));
    let mut reading = Reading::with_book(storage, EPUB, &whole_chapters_epub());
    reading.open();
    for _ in 1..CHAPTERS {
        reading.run(actions::NEXT_CHAPTER);
    }
    let start = last(&reading);
    let chapter = format!("Chapter {}", CHAPTERS - 1);
    assert_eq!(start.chapter, chapter, "{start:?}");
    assert_eq!(start.pages, None, "{start:?}");

    // Back from inside the chapter: to its start, not past it.
    reading.run(core_actions::NEXT_PAGE);
    reading.run(core_actions::NEXT_PAGE);
    assert_ne!(last(&reading).offset, start.offset);
    reading.run(actions::PREV_CHAPTER);
    let back = last(&reading);
    assert_eq!(back.chapter, chapter, "{back:?}");
    assert_eq!(
        (back.section, back.offset),
        (start.section, start.offset),
        "{back:?}"
    );

    // Back from its first page: to the chapter before.
    reading.run(actions::PREV_CHAPTER);
    let before = last(&reading);
    assert_eq!(before.section, CHAPTERS - 2, "{before:?}");
}
