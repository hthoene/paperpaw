//! The Mobipocket `core.format` provider against the interface-6 reader on
//! the same books: the same blocks, runs, offsets, contents, links and
//! pictures, the preparation in steps, and the refusals a hostile call or book
//! gets.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::io::{COVER, Roots, SIDE, call, open_call, open_ready, rows, run, state};
use common::{Builder, png};
use paperpaw_plugin_format_mobi::MobiDocument;
use paperpaw_plugin_format_mobi::provider::{MAX_DOCUMENTS, MobiProvider};
use paperpaw_sdk::document::{BlockKind, Document, Locator};
use paperpaw_sdk::image::Fit;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{MemoryStorage, OpenOptions, Storage};

const CACHE_MADE: u32 = 1 << 6;
const HAS_TOC: u32 = 1 << 1;

const HTML: &str = "<html><head><title>Ignored</title></head><body>\
    <h1>The Title</h1>\
    <p>Hello   <b>bold</b> and <i>it</i>.\n  Next &amp; last &#233;&eacute;</p>\
    <p align=\"center\">Middle <a filepos=0000000000>link</a> here</p>\
    <p>One<br/>Two</p>\
    <h2>Second</h2><ul><li>first</li><li>second</li></ul>\
    <p><img recindex=\"00001\" alt=\"A  grey\n gradient\"/></p>\
    <p>After the picture.</p></body></html>";

fn builder() -> Builder {
    let target = HTML.find("Middle").unwrap();
    let mut builder = Builder::new(&HTML.replace("0000000000", &format!("{target:010}")));
    builder.authors = vec!["Ann Author".into(), "Bo Writer".into()];
    builder.subjects = vec!["Fiction".into()];
    builder.pictures = vec![png(40, 30), png(20, 20)];
    builder.cover = Some(1);
    builder
}

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
    p: &mut MobiProvider,
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
    OpenOptions::default()
        .image_fit(Fit::side(SIDE))
        .cover_fit(Fit::side(COVER))
}

fn kind_of(kind: BlockKind) -> (u32, u32) {
    match kind {
        BlockKind::Paragraph => (0, 0),
        BlockKind::Heading(level) => (1, u32::from(level)),
        BlockKind::Image => (3, 0),
        other => panic!("{other:?}"),
    }
}

/// Whether `end` closes the last page of a list, after checking what `document.md`
/// section 3 asks of a list end: bit 0 the only flag, and a generation that is never 0.
fn last_page(end: &stage::FmtEnd) -> bool {
    assert_eq!(end.generation, 1, "never 0");
    assert_eq!(end.flags & !1, 0, "bit 0 is the only flag of a list");
    end.flags & 1 == 1
}

#[test]
fn a_book_is_prepared_in_steps_and_reads_like_interface_6() {
    let data = builder().build();
    let io = Roots::new(data.clone());
    let mut p = MobiProvider::new();
    let (s, status, flags) = open_ready(&mut p, &io);
    assert_eq!((s, status), (0, 0));
    assert_eq!(flags, CACHE_MADE | HAS_TOC);
    let old_io = MemoryStorage::new().with("b.mobi", data);
    let old = MobiDocument::open(&old_io, "b.mobi", &old_options()).unwrap();
    // The blocks, from the first to the last.
    let (mut at, mut count, mut saw_image) = (0u32, 0, false);
    loop {
        let wanted = old.block_at(&old_io, Locator::new(0, at)).unwrap();
        let got = block(&mut p, &io, 0, at);
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
        assert_eq!(got.runs, runs);
        let maps: Vec<_> = wanted.map.checkpoints().collect();
        assert_eq!(got.maps, maps);
        if let Some(image) = &wanted.image {
            saw_image = true;
            let (path, index, w, h, flags) = got.image.clone().unwrap();
            assert_eq!((index, w, h), (1, image.width, image.height));
            // Not decoded yet: the host asks (op 18), and gets the picture.
            assert_eq!(flags, 1);
            let reply = run(
                &mut p,
                &io,
                &call(18, 1, |c| {
                    c.index = 1;
                    c.image_side = SIDE;
                }),
            )
            .unwrap();
            assert!(matches!(rows(&reply)[0], stage::Record::FmtImage(i) if i.path == path));
            let old_picture = old.prepare_image(&old_io, image).unwrap();
            let (mut a, mut b) = (vec![0u8; 100_000], vec![0u8; 100_000]);
            let n = io.read_at(&path, 0, &mut a).unwrap();
            let m = old_io.read_at(&old_picture.path, 0, &mut b).unwrap();
            assert_eq!(a[..n], b[..m]);
            // Now it is there.
            assert_eq!(block(&mut p, &io, 0, at).unwrap().image.unwrap().4, 0);
        } else {
            assert_eq!(got.image, None);
        }
        assert_eq!(got.next, wanted.end);
        at = wanted.end;
        count += 1;
        assert!(count < 1000);
    }
    assert!(count >= 9 && saw_image);
    contents_links_cover_and_metadata(&mut p, &io, &old, &old_io);
}

/// Contents, links, the cover and the metadata against interface 6.
fn contents_links_cover_and_metadata(
    p: &mut MobiProvider,
    io: &Roots,
    old: &MobiDocument,
    old_io: &MemoryStorage,
) {
    // Contents and where they go.
    let reply = run(p, io, &call(9, 1, |_| {})).unwrap();
    let r = rows(&reply);
    assert_eq!(r.len() as u32, old.toc_len(old_io) + 1);
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
    // Links, and where they lead.
    let wanted = old.links(old_io, 0, 0, u32::MAX);
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
    assert_eq!(
        run(p, io, &call(15, 1, |c| c.text = "#99999999")),
        Err(Error::NotFound)
    );
    // The cover is the second picture, at the cover's size.
    let reply = run(p, io, &call(17, 1, |c| c.cover_side = COVER)).unwrap();
    let stage::Record::FmtImage(cover) = rows(&reply)[0] else {
        panic!("cover")
    };
    let old_cover = old.cover(old_io).unwrap().unwrap();
    assert_eq!(
        (cover.index, cover.width, cover.height),
        (u32::MAX, old_cover.width, old_cover.height)
    );
    // The metadata.
    let reply = run(p, io, &call(5, 1, |_| {})).unwrap();
    assert!(matches!(
        rows(&reply)[0],
        stage::Record::FmtMeta(m) if m.title == "A Test Book" && m.language == "en"
            && m.sections == 1 && m.authors == 2 && m.subjects == 1
    ));
    let reply = run(
        p,
        io,
        &call(6, 1, |c| {
            c.flags = 1;
            c.index = 1;
        }),
    )
    .unwrap();
    assert!(matches!(rows(&reply)[0], stage::Record::FmtText(t) if t.text == "Bo Writer"));
    let reply = run(p, io, &call(7, 1, |_| {})).unwrap();
    let r = rows(&reply);
    assert!(matches!(r[0], stage::Record::FmtSection(s) if s.length == old.section_len(0)));
    assert!(
        matches!(r[1], stage::Record::FmtEnd(e) if e.total == 1 && e.flags == 1 && e.generation == 1)
    );
}

#[test]
fn preparing_reports_progress_and_other_ops_wait() {
    let text = "<p>Some words to fill a record of the book. </p>".repeat(40);
    let mut b = Builder::new(&text);
    b.record_size = 256;
    let io = Roots::new(b.build());
    let mut p = MobiProvider::new();
    let (s, status, _, ppm) = state(&run(&mut p, &io, &open_call(1)).unwrap());
    assert_eq!((s, status, ppm), (1, 0, 0));
    assert_eq!(run(&mut p, &io, &call(7, 1, |_| {})), Err(Error::Busy));
    assert_eq!(block(&mut p, &io, 0, 0), Err(Error::Busy));
    let (mut last, mut steps) = (0, 0);
    loop {
        let (s, status, flags, ppm) = state(&run(&mut p, &io, &call(3, 1, |_| {})).unwrap());
        assert_eq!(status, 0);
        steps += 1;
        if s == 0 {
            assert_eq!(flags & CACHE_MADE, CACHE_MADE);
            break;
        }
        assert!(ppm >= last && ppm <= 1_000_000);
        last = ppm;
    }
    assert!(steps > 5 && last > 0, "{steps} steps");
    // Prepared: a call again says ready.
    let (s, ..) = state(&run(&mut p, &io, &call(3, 1, |_| {})).unwrap());
    assert_eq!(s, 0);
}

#[test]
fn a_cache_that_is_whole_opens_ready_at_once() {
    let io = Roots::new(builder().build());
    let mut p = MobiProvider::new();
    assert_eq!(open_ready(&mut p, &io).2 & CACHE_MADE, CACHE_MADE);
    let first = block(&mut p, &io, 0, 0).unwrap();
    run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
    assert_eq!(p.documents(), 0);
    // Opened again: nothing is made again.
    let (s, status, flags, _) = state(&run(&mut p, &io, &open_call(1)).unwrap());
    assert_eq!((s, status, flags), (0, 0, HAS_TOC));
    assert_eq!(block(&mut p, &io, 0, 0).unwrap(), first);
}

#[test]
fn long_lists_and_blocks_come_in_pages_and_fragments() {
    let mut html = String::new();
    for n in 0..150 {
        html.push_str(&format!("<h1>Chapter {n}</h1><p>Text of chapter {n}.</p>"));
    }
    html.push_str("<p>");
    for n in 0..120 {
        html.push_str(&format!("<b>b{n}</b> w "));
    }
    html.push_str("</p>");
    let data = Builder::new(&html).build();
    let io = Roots::new(data.clone());
    let mut p = MobiProvider::new();
    assert_eq!(open_ready(&mut p, &io).0, 0);
    let old_io = MemoryStorage::new().with("b.mobi", data);
    let old = MobiDocument::open(&old_io, "b.mobi", &old_options()).unwrap();
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
    let at = html_offset(&html, "<b>b0</b>");
    let got = block(&mut p, &io, 0, at).unwrap();
    let wanted = old.block_at(&old_io, Locator::new(0, at)).unwrap().unwrap();
    assert_eq!(got.runs.len(), 120);
    assert_eq!(got.runs.len(), wanted.runs.len());
    assert_eq!(got.text, wanted.text);
    assert!(got.last);
}

fn html_offset(html: &str, what: &str) -> u32 {
    html.find(what).unwrap() as u32
}

#[test]
fn sniffing_and_the_ops_a_mobipocket_book_has_no_answer_to() {
    let data = builder().build();
    let io = Roots::new(data.clone());
    let mut p = MobiProvider::new();
    let sniff = |p: &mut MobiProvider, head: &'static [u8]| {
        run(p, &io, &call(1, 0, |c| c.arg = head)).unwrap()
    };
    let head: &'static [u8] = Box::leak(data[..128].to_vec().into_boxed_slice());
    assert!(matches!(
        rows(&sniff(&mut p, head))[..],
        [stage::Record::FmtYes(_)]
    ));
    assert!(rows(&sniff(&mut p, b"not a mobi book at all")).is_empty());
    assert_eq!(open_ready(&mut p, &io).0, 0);
    for op in [11, 12, 13] {
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
fn books_that_cannot_be_read_open_as_failed_documents() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("drm", {
            let mut b = Builder::new("<p>x</p>");
            b.encryption = 2;
            b.build()
        }),
        ("huffman", {
            let mut b = Builder::new("<p>x</p>");
            b.compression = Some(17480);
            b.build()
        }),
        ("kf8", {
            let mut b = Builder::new("<p>x</p>");
            b.version = 8;
            b.build()
        }),
        ("short", b"BOOKMOBI".to_vec()),
        ("empty", Vec::new()),
    ];
    for (name, data) in cases {
        let io = Roots::new(data);
        let mut p = MobiProvider::new();
        let (s, status, flags, _) = state(&run(&mut p, &io, &open_call(1)).unwrap());
        assert_eq!((s, flags), (2, 0), "{name}");
        assert_eq!(status, i32::from(Error::Invalid), "{name}");
        // The id stays until it is closed, and says what it is.
        assert_eq!(
            run(&mut p, &io, &call(5, 1, |_| {})),
            Err(Error::State),
            "{name}"
        );
        let (s, status, ..) = state(&run(&mut p, &io, &call(3, 1, |_| {})).unwrap());
        assert_eq!((s, status), (2, i32::from(Error::Invalid)), "{name}");
        run(&mut p, &io, &call(4, 1, |_| {})).unwrap();
        assert_eq!(p.documents(), 0);
    }
}

#[test]
fn a_picture_that_cannot_be_decoded_is_not_found_and_not_tried_twice() {
    let mut b = Builder::new("<p>Before <img recindex=1 /> after</p>");
    let mut broken = png(40, 30);
    let at = broken.len() - 20;
    broken[at..].fill(0xff);
    b.pictures = vec![broken];
    let io = Roots::new(b.build());
    let mut p = MobiProvider::new();
    assert_eq!(open_ready(&mut p, &io).0, 0);
    for _ in 0..2 {
        let result = run(
            &mut p,
            &io,
            &call(18, 1, |c| {
                c.index = 1;
                c.image_side = SIDE;
            }),
        );
        assert!(matches!(result, Err(Error::NotFound)) || result.is_ok());
    }
    assert_eq!(
        run(&mut p, &io, &call(18, 1, |c| c.index = 7)),
        Err(Error::NotFound)
    );
    assert_eq!(run(&mut p, &io, &call(17, 1, |_| {})), Err(Error::NotFound));
}

#[test]
fn hostile_books_never_panic_the_provider() {
    let good = builder().build();
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
        let mut p = MobiProvider::new();
        let Ok(reply) = run(&mut p, &io, &open_call(1)) else {
            continue;
        };
        let (mut s, ..) = state(&reply);
        let mut guard = 0;
        while s == 1 && guard < 2000 {
            s = state(&run(&mut p, &io, &call(3, 1, |_| {})).unwrap()).0;
            guard += 1;
        }
        if s == 0 {
            let mut at = 0;
            for _ in 0..200 {
                let Ok(b) = block(&mut p, &io, 0, at) else {
                    break;
                };
                if b.last || b.next <= at {
                    break;
                }
                at = b.next;
            }
            for op in [5, 7, 9, 14, 17] {
                let _ = run(&mut p, &io, &call(op, 1, |c| c.index = u32::MAX));
            }
        }
    }
}
