//! The Word documents `core.format` provider against the interface-6 reader
//! on the same documents: the same blocks, runs, offsets, contents, links and
//! pictures, the preparation, and the refusals a hostile call or document gets.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

mod native;

use native::{Roots, SIDE, call, docx, docx_with, heading, open_call, open_ready, p, rich_body};
use native::{rows, run, state};
use paperpaw_plugin_format_docx::DocxDocument;
use paperpaw_plugin_format_docx::provider::{DocxProvider, MAX_DOCUMENTS};
use paperpaw_sdk::document::{BlockKind, Document, Locator};
use paperpaw_sdk::image::Fit;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{MemoryStorage, OpenOptions, Storage};

const CACHE_MADE: u32 = 1 << 6;
const HAS_TOC: u32 = 1 << 1;

/// A block as the provider tells it, its fragments joined.
#[derive(Debug, PartialEq)]
struct Joined {
    kind: u32,
    level: u32,
    flags: u32,
    start: u32,
    end: u32,
    align: u32,
    text: String,
    runs: Vec<(u32, u32, u32)>,
    maps: Vec<(u32, u32)>,
    image: Option<(String, u32, u32, u32, u32)>,
    last: bool,
    next: u32,
}

fn block(
    p: &mut DocxProvider,
    io: &dyn Storage,
    section: u32,
    offset: u32,
) -> Result<Joined, Error> {
    let mut joined = Joined {
        kind: 0,
        level: 0,
        flags: 0,
        start: 0,
        end: 0,
        align: 0,
        text: String::new(),
        runs: Vec::new(),
        maps: Vec::new(),
        image: None,
        last: false,
        next: 0,
    };
    let (mut cursor, mut generation) = (0, 0);
    loop {
        let reply = run(
            p,
            io,
            &call(8, 1, |c| {
                c.section = section;
                c.offset = offset;
                c.cursor = cursor;
                c.generation = generation;
            }),
        )?;
        let mut more = None;
        for record in rows(&reply) {
            match record {
                stage::Record::FmtBlock(b) => {
                    joined.kind = b.kind;
                    joined.level = b.level;
                    joined.flags = b.flags;
                    joined.start = b.start;
                    joined.end = b.end;
                    joined.align = b.align;
                }
                stage::Record::FmtTextPart(t) => joined.text.push_str(t.text),
                stage::Record::FmtRun(r) => joined.runs.push((r.start, r.end, r.style)),
                stage::Record::FmtMap(m) => joined.maps.push((m.text, m.source)),
                stage::Record::FmtImage(i) => {
                    joined.image = Some((i.path.to_string(), i.index, i.width, i.height, i.flags));
                }
                stage::Record::FmtEnd(e) => {
                    if e.flags & 2 != 0 {
                        more = Some((e.next, e.generation));
                    } else {
                        joined.last = e.flags & 1 != 0;
                        joined.next = e.next;
                    }
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        match more {
            Some((next, g)) => (cursor, generation) = (next, g),
            None => return Ok(joined),
        }
    }
}

fn old_options() -> OpenOptions {
    OpenOptions::default().image_fit(Fit::side(SIDE))
}

fn kind_of(kind: BlockKind) -> (u32, u32) {
    match kind {
        BlockKind::Paragraph => (0, 0),
        BlockKind::Heading(level) => (1, u32::from(level)),
        BlockKind::Image => (3, 0),
        other => panic!("{other:?}"),
    }
}

/// All blocks of the provider against interface 6's: how many, and the
/// pictures' indexes and flags seen.
fn compare(
    p: &mut DocxProvider,
    io: &Roots,
    old: &DocxDocument,
    old_io: &MemoryStorage,
) -> (usize, Vec<(String, u32, u32)>) {
    let (mut at, mut count, mut images) = (0u32, 0, Vec::new());
    loop {
        let wanted = old.block_at(old_io, Locator::new(0, at)).unwrap();
        let got = block(p, io, 0, at);
        let Some(wanted) = wanted else {
            assert_eq!(got, Err(Error::NotFound));
            break;
        };
        let got = got.unwrap();
        let (kind, level) = kind_of(wanted.kind);
        assert_eq!((got.kind, got.level), (kind, level), "block at {at}");
        assert_eq!((got.start, got.end), (wanted.start, wanted.end));
        assert_eq!(got.text, wanted.text);
        assert_eq!(got.align, u32::from(wanted.align.bits()));
        assert_eq!(
            got.flags,
            u32::from(wanted.continues) | u32::from(wanted.continued) << 1
        );
        let runs: Vec<_> = wanted
            .runs
            .iter()
            .map(|r| (r.start, r.end, r.style.bits()))
            .collect();
        assert_eq!(got.runs, runs, "runs of block at {at}");
        assert_eq!(got.maps, Vec::<(u32, u32)>::new());
        match (&wanted.image, &got.image) {
            (Some(image), Some((path, index, w, h, flags))) => {
                assert_eq!((*w, *h), (image.width, image.height));
                images.push((path.clone(), *index, *flags));
            }
            (None, None) => {}
            other => panic!("picture of block at {at}: {other:?}"),
        }
        assert_eq!(got.next, wanted.end);
        at = wanted.end;
        count += 1;
        assert!(count < 1000);
    }
    (count, images)
}

/// Whether `end` closes the last page of a list, after checking what `document.md`
/// section 3 asks of a list end: bit 0 the only flag, and a generation that is never 0.
fn last_page(end: &stage::FmtEnd) -> bool {
    assert_eq!(end.generation, 1, "never 0");
    assert_eq!(end.flags & !1, 0, "bit 0 is the only flag of a list");
    end.flags & 1 == 1
}

#[test]
fn a_document_is_prepared_and_reads_like_interface_6() {
    let data = docx(&rich_body());
    let io = Roots::new(data.clone());
    let mut p = DocxProvider::new();
    let (s, status, flags) = open_ready(&mut p, &io);
    assert_eq!((s, status), (0, 0));
    assert_eq!(flags, CACHE_MADE | HAS_TOC);
    let old_io = MemoryStorage::new().with("d.docx", data);
    let old = DocxDocument::open(&old_io, "d.docx", &old_options()).unwrap();
    // The blocks, from the first to the last; the pictures are not decoded yet.
    let (count, images) = compare(&mut p, &io, &old, &old_io);
    assert!(count >= 12, "{count}");
    assert_eq!(images.len(), 2, "{images:?}");
    assert!(images.iter().all(|(_, _, flags)| *flags == 1));
    // The host asks for each (op 18); the fine one is the picture
    // interface 6 decodes, pixel for pixel; the damaged one is not found.
    let wanted = old.block_at(&old_io, Locator::new(0, 0)).unwrap().unwrap();
    assert!(matches!(wanted.kind, BlockKind::Heading(1)));
    let mut decoded = 0;
    let mut at = 0;
    while let Some(b) = old.block_at(&old_io, Locator::new(0, at)).unwrap() {
        at = b.end;
        let Some(image) = &b.image else { continue };
        let name = &image.path[image.path.rfind('/').map_or(0, |at| at + 1)..];
        let (path, index, ..) = images
            .iter()
            .find(|(path, ..)| path.ends_with(name))
            .cloned()
            .unwrap();
        let old_picture = old.prepare_image(&old_io, image);
        let reply = run(
            &mut p,
            &io,
            &call(18, 1, |c| {
                c.index = index;
                c.image_side = SIDE;
            }),
        );
        match (old_picture, reply) {
            (Some(old_picture), Ok(reply)) => {
                assert!(matches!(rows(&reply)[0], stage::Record::FmtImage(i) if i.path == path));
                let (mut a, mut b) = (vec![0u8; 100_000], vec![0u8; 100_000]);
                let n = io.read_at(&path, 0, &mut a).unwrap();
                let m = old_io.read_at(&old_picture.path, 0, &mut b).unwrap();
                assert_eq!(a[..n], b[..m]);
                decoded += 1;
            }
            (None, Err(Error::NotFound)) => {}
            other => panic!("picture {index}: {other:?}"),
        }
    }
    assert_eq!(decoded, 1);
    // Read again: the picture is there now, the damaged one is its text.
    let (_, images) = compare(&mut p, &io, &old, &old_io);
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].2, 0);
    contents_links_and_metadata(&mut p, &io, &old, &old_io);
}

/// Contents, links and the metadata against interface 6.
fn contents_links_and_metadata(
    p: &mut DocxProvider,
    io: &Roots,
    old: &DocxDocument,
    old_io: &MemoryStorage,
) {
    let reply = run(p, io, &call(9, 1, |_| {})).unwrap();
    let r = rows(&reply);
    assert_eq!(r.len() as u32, old.toc_len(old_io) + 1);
    assert!(r.len() >= 4);
    for (index, row) in r.iter().take(r.len() - 1).enumerate() {
        let entry = old.toc_entry(old_io, index as u32).unwrap();
        let stage::Record::FmtToc(t) = row else {
            panic!("toc row")
        };
        assert_eq!(
            (t.title, t.level, t.section, t.offset),
            (
                entry.title.as_str(),
                u32::from(entry.level),
                0,
                entry.at.offset
            )
        );
        let reply = run(p, io, &call(10, 1, |c| c.index = index as u32)).unwrap();
        assert!(matches!(rows(&reply)[0], stage::Record::FmtAt(a) if a.offset == entry.at.offset));
    }
    assert_eq!(
        run(p, io, &call(10, 1, |c| c.index = 99)),
        Err(Error::NotFound)
    );
    // Links: an address of the document and a place in it.
    let wanted = old.links(old_io, 0, 0, u32::MAX);
    assert_eq!(wanted.len(), 2, "{wanted:?}");
    let reply = run(p, io, &call(14, 1, |c| c.index = u32::MAX)).unwrap();
    let r = rows(&reply);
    assert_eq!(r.len(), wanted.len() + 1);
    for (row, link) in r.iter().zip(&wanted) {
        let stage::Record::FmtLink(l) = row else {
            panic!("link row")
        };
        assert_eq!(
            (l.start, l.end, l.kind, l.href),
            (link.start, link.end, 0, link.href.as_str())
        );
        if link.href.starts_with('#') {
            let reply = run(
                p,
                io,
                &call(15, 1, |c| {
                    c.text = Box::leak(l.href.to_string().into_boxed_str())
                }),
            )
            .unwrap();
            let place = old.link_target(old_io, &link.href).unwrap();
            assert!(matches!(rows(&reply)[0], stage::Record::FmtAt(a) if a.offset == place.offset));
        }
    }
    assert!(
        wanted
            .iter()
            .any(|l| l.href == "https://example.org/a?b=1&c=2")
    );
    assert_eq!(
        run(p, io, &call(15, 1, |c| c.text = "#99999999")),
        Err(Error::NotFound)
    );
    assert_eq!(
        run(p, io, &call(15, 1, |c| c.text = "https://example.org/")),
        Err(Error::NotFound)
    );
    // The metadata.
    let reply = run(p, io, &call(5, 1, |_| {})).unwrap();
    assert!(matches!(
        rows(&reply)[0],
        stage::Record::FmtMeta(m) if m.title == "The Report" && m.language == "de-DE"
            && m.sections == 1 && m.authors == 1 && m.subjects == 0
    ));
    let reply = run(
        p,
        io,
        &call(6, 1, |c| {
            c.flags = 1;
            c.index = 0;
        }),
    )
    .unwrap();
    assert!(matches!(rows(&reply)[0], stage::Record::FmtText(t) if t.text == "Ann Author"));
    let reply = run(p, io, &call(7, 1, |_| {})).unwrap();
    let r = rows(&reply);
    assert!(matches!(r[0], stage::Record::FmtSection(s) if s.length == old.section_len(0)));
    assert!(
        matches!(r[1], stage::Record::FmtEnd(e) if e.total == 1 && e.flags == 1 && e.generation == 1)
    );
}

#[test]
fn preparing_is_one_call_and_other_ops_wait() {
    let io = Roots::new(docx(&rich_body()));
    let mut p = DocxProvider::new();
    let (s, status, flags, ppm) = state(&run(&mut p, &io, &open_call(1)).unwrap());
    assert_eq!((s, status, flags, ppm), (1, 0, 0, 0));
    assert_eq!(run(&mut p, &io, &call(7, 1, |_| {})), Err(Error::Busy));
    assert_eq!(block(&mut p, &io, 0, 0), Err(Error::Busy));
    let (s, status, flags, _) = state(&run(&mut p, &io, &call(3, 1, |_| {})).unwrap());
    assert_eq!((s, status), (0, 0));
    assert_eq!(flags, CACHE_MADE | HAS_TOC);
    // Prepared: a call again says ready.
    let (s, ..) = state(&run(&mut p, &io, &call(3, 1, |_| {})).unwrap());
    assert_eq!(s, 0);
}

#[test]
fn a_cache_that_is_whole_opens_ready_at_once() {
    let io = Roots::new(docx(&rich_body()));
    let mut p = DocxProvider::new();
    assert_eq!(open_ready(&mut p, &io).2 & CACHE_MADE, CACHE_MADE);
    let first = block(&mut p, &io, 0, 0).unwrap();
    run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
    assert_eq!(p.documents(), 0);
    // Opened again: nothing is made again, and the title is the document's.
    let (s, status, flags, _) = state(&run(&mut p, &io, &open_call(1)).unwrap());
    assert_eq!((s, status, flags), (0, 0, HAS_TOC));
    assert_eq!(block(&mut p, &io, 0, 0).unwrap(), first);
}

#[test]
fn long_lists_and_blocks_come_in_pages_and_fragments() {
    let mut body = String::new();
    for n in 0..150 {
        body.push_str(&heading(1, &format!("Chapter {n}")));
        body.push_str(&p(&format!("Text of chapter {n}.")));
    }
    // Many runs in one paragraph; many addresses (a page of links holds few).
    body.push_str("<w:p>");
    for n in 0..120 {
        body.push_str(&format!(
            "<w:r><w:rPr><w:b/></w:rPr><w:t>b{n}</w:t></w:r><w:r><w:t> w </w:t></w:r>"
        ));
    }
    body.push_str("</w:p><w:p>");
    let mut rels = String::new();
    for n in 0..60 {
        let url = format!("https://example.org/{n}/{}", "x".repeat(150));
        rels.push_str(&format!(
            "<Relationship Id=\"u{n}\" Type=\"http://x/hyperlink\" Target=\"{url}\" TargetMode=\"External\"/>"
        ));
        body.push_str(&format!(
            "<w:hyperlink r:id=\"u{n}\"><w:r><w:t>link{n} </w:t></w:r></w:hyperlink>"
        ));
    }
    body.push_str("</w:p>");
    let data = docx_with(&body, &rels);
    let io = Roots::new(data.clone());
    let mut p = DocxProvider::new();
    assert_eq!(open_ready(&mut p, &io).0, 0);
    let old_io = MemoryStorage::new().with("d.docx", data);
    let old = DocxDocument::open(&old_io, "d.docx", &old_options()).unwrap();
    assert_eq!(old.toc_len(&old_io), 150);
    let (mut cursor, mut seen) = (0u32, 0usize);
    loop {
        let reply = run(&mut p, &io, &call(9, 1, |c| c.cursor = cursor)).unwrap();
        let r = rows(&reply);
        let stage::Record::FmtEnd(end) = r[r.len() - 1] else {
            panic!("end")
        };
        assert_eq!(end.total, 150);
        assert!(r.len() <= 64);
        for (n, row) in r[..r.len() - 1].iter().enumerate() {
            let stage::Record::FmtToc(t) = row else {
                panic!("row")
            };
            assert_eq!(t.index as usize, seen + n);
            assert_eq!(t.title, format!("Chapter {}", seen + n));
        }
        seen += r.len() - 1;
        if last_page(&end) {
            break;
        }
        cursor = end.next;
    }
    assert_eq!(seen, 150);
    // The paragraph with 120 runs is a block of several fragments.
    let mut at = 0;
    let (wanted, got) = loop {
        let b = old.block_at(&old_io, Locator::new(0, at)).unwrap().unwrap();
        if b.runs.len() == 120 {
            break (b.clone(), block(&mut p, &io, 0, b.start).unwrap());
        }
        at = b.end;
    };
    assert_eq!(got.runs.len(), 120);
    assert_eq!(got.text, wanted.text);
    // Links come in pages that hold as many addresses as fit a reply.
    let expected = old.links(&old_io, 0, 0, u32::MAX);
    assert_eq!(expected.len(), 60);
    let (mut cursor, mut urls, mut pages) = (0u32, Vec::new(), 0);
    loop {
        let reply = run(
            &mut p,
            &io,
            &call(14, 1, |c| {
                c.cursor = cursor;
                c.index = u32::MAX;
            }),
        )
        .unwrap();
        let r = rows(&reply);
        let stage::Record::FmtEnd(end) = r[r.len() - 1] else {
            panic!("end")
        };
        assert_eq!(end.total, 60);
        for row in &r[..r.len() - 1] {
            let stage::Record::FmtLink(l) = row else {
                panic!("row")
            };
            urls.push((l.start, l.end, l.href.to_string()));
        }
        pages += 1;
        if last_page(&end) {
            break;
        }
        cursor = end.next;
    }
    assert!(pages > 2, "{pages} pages");
    let wanted: Vec<_> = expected
        .iter()
        .map(|l| (l.start, l.end, l.href.clone()))
        .collect();
    assert_eq!(urls, wanted);
}

#[test]
fn the_ops_a_word_document_has_no_answer_to() {
    let io = Roots::new(docx(&rich_body()));
    let mut p = DocxProvider::new();
    // Not a sniffer: no rows.
    assert!(rows(&run(&mut p, &io, &call(1, 0, |c| c.arg = b"PK")).unwrap()).is_empty());
    assert_eq!(open_ready(&mut p, &io).0, 0);
    for op in [11, 12, 13, 17] {
        assert_eq!(run(&mut p, &io, &call(op, 1, |_| {})), Err(Error::NotFound));
    }
    let reply = run(&mut p, &io, &call(16, 1, |_| {})).unwrap();
    assert!(
        matches!(rows(&reply)[..], [stage::Record::FmtEnd(e)] if e.total == 0 && e.flags == 1 && e.generation == 1)
    );
    // The links of a later section: an empty list, still ended as the last page.
    let reply = run(&mut p, &io, &call(14, 1, |c| c.section = 1)).unwrap();
    assert!(
        matches!(rows(&reply)[..], [stage::Record::FmtEnd(e)] if e.total == 0 && e.flags == 1 && e.generation == 1)
    );
    for op in [19, 20, 99] {
        assert_eq!(
            run(&mut p, &io, &call(op, 1, |_| {})),
            Err(Error::Unsupported)
        );
    }
    // A document that is not there; an id opened twice; too many.
    assert_eq!(run(&mut p, &io, &call(5, 9, |_| {})), Err(Error::NotFound));
    assert_eq!(run(&mut p, &io, &open_call(1)), Err(Error::Invalid));
    for id in 2..=MAX_DOCUMENTS as u32 {
        run(&mut p, &io, &open_call(id)).unwrap();
    }
    assert_eq!(run(&mut p, &io, &open_call(99)), Err(Error::Full));
    // Input that is no `fmt.call`.
    assert_eq!(p.call(&io, &[1, 2, 3], &mut [0u8; 64]), Err(Error::Invalid));
}

#[test]
fn documents_that_cannot_be_read_open_as_failed_documents() {
    let no_body = paperpaw_zip::testing::ZipBuilder::new()
        .deflated("[Content_Types].xml", "<Types/>")
        .build();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("not a document", no_body),
        ("short", b"PK\x03\x04".to_vec()),
        ("text", b"just some text, no archive at all".to_vec()),
        ("empty", Vec::new()),
    ];
    for (name, data) in cases {
        let io = Roots::new(data);
        let mut p = DocxProvider::new();
        let (s, ..) = state(&run(&mut p, &io, &open_call(1)).unwrap());
        assert_eq!(s, 1, "{name}");
        let (s, status, flags, _) = state(&run(&mut p, &io, &call(3, 1, |_| {})).unwrap());
        assert_eq!((s, flags), (2, 0), "{name}");
        assert_ne!(status, 0, "{name}");
        // The id stays until it is closed, and says what it is.
        assert_eq!(
            run(&mut p, &io, &call(5, 1, |_| {})),
            Err(Error::State),
            "{name}"
        );
        let (s, status2, ..) = state(&run(&mut p, &io, &call(3, 1, |_| {})).unwrap());
        assert_eq!((s, status2), (2, status), "{name}");
        run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
        assert_eq!(p.documents(), 0);
        // What a failed conversion made is not left in the cache.
        assert!(io.size("blocks.bin").is_err(), "{name}");
        assert!(io.size("done").is_err(), "{name}");
    }
}

#[test]
fn a_picture_that_cannot_be_decoded_is_not_found_and_not_tried_twice() {
    let body = format!("{}{}", p("Before"), native::drawing("rId5", "Broken"));
    let io = Roots::new(docx(&body));
    let mut p = DocxProvider::new();
    assert_eq!(open_ready(&mut p, &io).0, 0);
    for _ in 0..2 {
        assert_eq!(
            run(
                &mut p,
                &io,
                &call(18, 1, |c| {
                    c.index = 0;
                    c.image_side = SIDE;
                })
            ),
            Err(Error::NotFound)
        );
    }
    assert!(io.size("img-0.fail").is_ok());
    assert_eq!(
        run(&mut p, &io, &call(18, 1, |c| c.index = 7)),
        Err(Error::NotFound)
    );
}

#[test]
fn a_larger_side_is_a_zoom_in_a_file_of_its_own() {
    let io = Roots::new(docx(&native::drawing("rId1", "Gradient")));
    let mut p = DocxProvider::new();
    assert_eq!(open_ready(&mut p, &io).0, 0);
    let ask = |p: &mut DocxProvider, side: u32| {
        let reply = run(
            p,
            &io,
            &call(18, 1, |c| {
                c.index = 0;
                c.image_side = side;
            }),
        )
        .unwrap();
        let stage::Record::FmtImage(i) = rows(&reply)[0] else {
            panic!("image")
        };
        (i.path.to_string(), i.width, i.height)
    };
    let page = ask(&mut p, SIDE);
    assert!(page.0.starts_with("img-0."));
    let zoom = ask(&mut p, SIDE + 200);
    assert!(zoom.0.starts_with("zoom-0-1000."), "{zoom:?}");
    assert!(zoom.1 >= page.1 && zoom.2 >= page.2);
    // Asked again: the same files.
    assert_eq!(ask(&mut p, SIDE), page);
}

#[test]
fn hostile_documents_never_panic_the_provider() {
    let good = docx(&rich_body());
    let mut seed = 0x9e37_79b9_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for _ in 0..150 {
        let mut data = good.clone();
        for _ in 0..(next() % 12 + 1) {
            let at = next() as usize % data.len();
            data[at] = next() as u8;
        }
        if next() % 4 == 0 {
            data.truncate(next() as usize % data.len());
        }
        let io = Roots::new(data);
        let mut p = DocxProvider::new();
        let Ok(reply) = run(&mut p, &io, &open_call(1)) else {
            continue;
        };
        let (mut s, ..) = state(&reply);
        let mut guard = 0;
        while s == 1 && guard < 10 {
            s = state(&run(&mut p, &io, &call(3, 1, |_| {})).unwrap()).0;
            guard += 1;
        }
        if s == 0 {
            let mut at = 0;
            for _ in 0..200 {
                let Ok(b) = block(&mut p, &io, 0, at) else {
                    break;
                };
                if let Some((_, index, ..)) = b.image {
                    let _ = run(
                        &mut p,
                        &io,
                        &call(18, 1, |c| {
                            c.index = index;
                            c.image_side = SIDE;
                        }),
                    );
                }
                if b.last || b.next <= at {
                    break;
                }
                at = b.next;
            }
            for op in [5, 7, 9, 14] {
                let _ = run(&mut p, &io, &call(op, 1, |c| c.index = u32::MAX));
            }
        }
        run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
    }
}
