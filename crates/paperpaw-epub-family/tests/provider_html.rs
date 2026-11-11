//! The HTML `core.format` provider against the interface-6 reader on the
//! same file: the same blocks (texts, runs, maps), whole or in fragments, and
//! the refusals a hostile call gets. The book is root 3 (named by the call's
//! `text`), the converted page root 6.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_epub_family::{HtmlDocument, html_provider};
use paperpaw_sdk::document::{BlockKind, Document, Locator};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{DirEntry, Env, MemoryStorage, Storage, StorageError};

/// Root 3 (the folder of the book) and root 6 (below the plugin's folder).
struct Roots {
    mem: MemoryStorage,
    prefix: String,
}

impl Roots {
    fn new(name: &str, book: &[u8]) -> Self {
        Self {
            mem: MemoryStorage::new().with(&format!("book/{name}"), book.to_vec()),
            prefix: Env::plugin_dir("format-epub"),
        }
    }
    fn map(&self, path: &str) -> String {
        match path.strip_prefix(&self.prefix) {
            Some(rest) => format!("cache{rest}"),
            None => format!("book/{path}"),
        }
    }
}

impl Storage for Roots {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.mem.read_at(&self.map(path), offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.mem.size(&self.map(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if !path.starts_with(&self.prefix) {
            return Err(StorageError::Unsupported);
        }
        self.mem.write(&self.map(path), data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if !path.starts_with(&self.prefix) {
            return Err(StorageError::Unsupported);
        }
        self.mem.append(&self.map(path), data)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.mem.remove(&self.map(path))
    }
}

fn call(op: u32, f: impl FnOnce(&mut stage::FmtCall<'static>)) -> Vec<u8> {
    let mut c = stage::FmtCall::new(op, 1, 0, 0, 0, 0, 0, 1, 0, 0, 7, "", &[]);
    f(&mut c);
    let mut buf = vec![0u8; 4096];
    let n = stage::input_encode(&[stage::Record::FmtCall(c)], &mut buf).unwrap();
    buf.truncate(n);
    buf
}

fn run(
    p: &mut paperpaw_epub_family::provider::FormatProvider,
    io: &dyn Storage,
    input: &[u8],
) -> Result<Vec<u8>, Error> {
    let mut out = vec![0u8; 3840];
    let n = p.call(io, input, &mut out)?;
    out.truncate(n);
    Ok(out)
}

fn rows(reply: &[u8]) -> Vec<stage::Record<'_>> {
    stage::reply_decode(reply)
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

const BOOK: &str = "<html><head><title>A Title</title></head><body><h1>A Title</h1><p>First <em>paragraph</em> with <code>code</code> and a <a href=\"#two\">link</a>.</p><h2 id=\"two\">Two</h2><ul><li>one</li><li>two</li></ul><blockquote>quoted words</blockquote><pre>let x = 1;</pre></body></html>";

/// Text, runs (start, end, style), maps, span and flags of a block.
type Joined = (
    String,
    Vec<(u32, u32, u32)>,
    Vec<(u32, u32)>,
    (u32, u32),
    u32,
);

/// One block of the provider, joined from its fragments: text, runs, maps.
fn block(
    p: &mut paperpaw_epub_family::provider::FormatProvider,
    io: &dyn Storage,
    offset: u32,
) -> Result<Joined, Error> {
    let (mut text, mut runs, mut maps, mut span) = (String::new(), Vec::new(), Vec::new(), (0, 0));
    let (mut cursor, mut generation) = (0, 0);
    loop {
        let reply = run(
            p,
            io,
            &call(8, |c| {
                c.offset = offset;
                c.cursor = cursor;
                c.generation = generation;
            }),
        )?;
        let mut flags_end = (0, 0, 0);
        for row in rows(&reply) {
            match row {
                stage::Record::FmtBlock(b) => span = (b.start, b.end),
                stage::Record::FmtTextPart(t) => text.push_str(t.text),
                stage::Record::FmtRun(r) => runs.push((r.start, r.end, r.style)),
                stage::Record::FmtMap(m) => maps.push((m.text, m.source)),
                stage::Record::FmtEnd(e) => flags_end = (e.flags, e.generation, e.next),
                _ => {}
            }
        }
        if flags_end.0 & 2 == 0 {
            return Ok((text, runs, maps, span, flags_end.0));
        }
        (cursor, generation) = (flags_end.2, flags_end.1);
    }
}

/// The header row of a block's first fragment: kind, level, flags, align,
/// indent, space, size, margins, line height, direction, writing mode, drop cap.
fn header(
    p: &mut paperpaw_epub_family::provider::FormatProvider,
    io: &dyn Storage,
    offset: u32,
) -> [i64; 13] {
    let reply = run(p, io, &call(8, |c| c.offset = offset)).unwrap();
    for row in rows(&reply) {
        if let stage::Record::FmtBlock(b) = row {
            return [
                b.kind.into(),
                b.level.into(),
                b.flags.into(),
                b.align.into(),
                b.indent.into(),
                b.space.into(),
                b.size.into(),
                b.margin_start.into(),
                b.margin_end.into(),
                b.line_height.into(),
                b.dir.into(),
                b.writing_mode.into(),
                b.drop_cap.into(),
            ];
        }
    }
    panic!("no block row");
}

#[test]
fn an_html_file_reads_like_interface_6_block_by_block() {
    let io = Roots::new("notes.html", BOOK.as_bytes());
    let mut p = html_provider();
    let reply = run(&mut p, &io, &call(2, |c| c.text = "notes.html")).unwrap();
    let stage::Record::FmtState(s) = rows(&reply)[0] else {
        panic!("state")
    };
    assert_eq!((s.state, s.status), (0, 0), "{s:?}");
    let old_io = MemoryStorage::new().with("notes.html", BOOK.as_bytes().to_vec());
    let old = HtmlDocument::open(&old_io, "notes.html", true).unwrap();
    let mut at = Locator::START;
    let mut count = 0;
    while let Some(expected) = old.block_at(&old_io, at).unwrap() {
        let (text, runs, maps, span, _) = block(&mut p, &io, at.offset).unwrap();
        assert_eq!(text, expected.text, "block {count}");
        assert_eq!(span, (expected.start, expected.end));
        let want: Vec<_> = expected
            .runs
            .iter()
            .map(|r| (r.start, r.end, r.style.bits()))
            .collect();
        assert_eq!(runs, want);
        assert_eq!(maps, expected.map.checkpoints().collect::<Vec<_>>());
        let h = header(&mut p, &io, at.offset);
        assert_eq!(
            h[1],
            i64::from(matches!(expected.kind, BlockKind::Heading(_)) as u8)
                * i64::from(match expected.kind {
                    BlockKind::Heading(l) => l,
                    _ => 0,
                })
        );
        assert_eq!(h[3], i64::from(expected.align.bits()), "align {count}");
        assert_eq!(h[4], expected.indent.map_or(i64::from(i32::MIN), i64::from));
        assert_eq!(h[5], expected.space.map_or(i64::from(u32::MAX), i64::from));
        assert_eq!(h[6], expected.size.map_or(i64::from(u32::MAX), i64::from));
        assert_eq!(h[7], i64::from(expected.margins.0));
        assert_eq!(h[8], i64::from(expected.margins.1));
        assert_eq!(h[12], i64::from(expected.drop_cap));
        at = Locator::new(0, expected.end);
        count += 1;
    }
    assert!(count >= 6, "{count} blocks");
    assert!(matches!(
        block(&mut p, &io, at.offset),
        Err(Error::NotFound)
    ));
    // The title, sections and contents.
    let reply = run(&mut p, &io, &call(5, |_| {})).unwrap();
    match rows(&reply)[0] {
        stage::Record::FmtMeta(m) => {
            assert_eq!(Some(m.title), old.metadata().title.as_deref());
            assert_eq!(m.sections, old.sections());
        }
        other => panic!("not meta: {other:?}"),
    }
    // The contents, row by row.
    let reply = run(&mut p, &io, &call(9, |_| {})).unwrap();
    let toc: Vec<_> = rows(&reply)
        .into_iter()
        .filter_map(|r| match r {
            stage::Record::FmtToc(t) => Some((t.level, t.title.to_string())),
            _ => None,
        })
        .collect();
    assert_eq!(toc.len() as u32, old.toc_len(&old_io));
    for (i, (level, title)) in toc.iter().enumerate() {
        let e = old.toc_entry(&old_io, i as u32).unwrap();
        assert_eq!(
            (*level, title.as_str()),
            (u32::from(e.level), e.title.as_str())
        );
    }
    // The links of the whole section, row by row.
    let reply = run(&mut p, &io, &call(14, |c| c.index = u32::MAX)).unwrap();
    let links: Vec<_> = rows(&reply)
        .into_iter()
        .filter_map(|r| match r {
            stage::Record::FmtLink(l) => Some((l.start, l.end, l.href.to_string())),
            _ => None,
        })
        .collect();
    let want: Vec<_> = old
        .links(&old_io, 0, 0, u32::MAX)
        .into_iter()
        .map(|l| (l.start, l.end, l.href))
        .collect();
    assert_eq!(links, want);
    assert!(!links.is_empty());
}

#[test]
fn bad_calls_get_their_refusals() {
    let io = Roots::new("n.html", BOOK.as_bytes());
    let mut p = html_provider();
    assert!(matches!(
        run(&mut p, &io, &call(5, |_| {})),
        Err(Error::NotFound)
    ));
    assert!(matches!(run(&mut p, &io, &[7]), Err(Error::Invalid)));
    let reply = run(&mut p, &io, &call(1, |c| c.document = 0)).unwrap();
    assert_eq!(rows(&reply).len(), 0, "html does not sniff");
    run(&mut p, &io, &call(2, |c| c.text = "n.html")).unwrap();
    assert!(matches!(
        run(&mut p, &io, &call(2, |c| c.text = "n.html")),
        Err(Error::Invalid)
    ));
    for op in [12, 13, 15] {
        assert!(
            matches!(run(&mut p, &io, &call(op, |_| {})), Err(Error::NotFound)),
            "op {op}"
        );
    }
    assert!(matches!(
        run(&mut p, &io, &call(99, |_| {})),
        Err(Error::Unsupported)
    ));
    // A name that is no file fails the document, not the provider.
    let reply = run(
        &mut p,
        &io,
        &call(2, |c| {
            c.document = 2;
            c.text = "gone.html";
        }),
    )
    .unwrap();
    let stage::Record::FmtState(s) = rows(&reply)[0] else {
        panic!("state")
    };
    assert_eq!(s.state, 2);
    assert!(matches!(
        run(&mut p, &io, &call(8, |c| c.document = 2)),
        Err(Error::State)
    ));
    run(&mut p, &io, &call(4, |_| {})).unwrap();
    run(&mut p, &io, &call(4, |_| {})).unwrap();
}
