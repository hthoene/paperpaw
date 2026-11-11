//! The Markdown `core.format` provider against the interface-6 reader on the
//! same file: the same blocks (texts, runs, maps), whole or in fragments, and
//! the refusals a hostile call gets. The book is root 3 (named by the call's
//! `text`), the converted page root 6.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_epub_family::{MarkdownDocument, markdown_provider};
use paperpaw_sdk::document::{Document, Locator};
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

const BOOK: &str = "# A Title\n\nFirst *paragraph* with `code` and a [link](#two).\n\n## Two\n\n- one\n- two\n\n> quoted words\n\n```\nlet x = 1;\n```\n";

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

#[test]
fn a_markdown_file_reads_like_interface_6_block_by_block() {
    let io = Roots::new("notes.md", BOOK.as_bytes());
    let mut p = markdown_provider();
    let reply = run(&mut p, &io, &call(2, |c| c.text = "notes.md")).unwrap();
    let stage::Record::FmtState(s) = rows(&reply)[0] else {
        panic!("state")
    };
    assert_eq!((s.state, s.status), (0, 0), "{s:?}");
    let old_io = MemoryStorage::new().with("notes.md", BOOK.as_bytes().to_vec());
    let old = MarkdownDocument::open(&old_io, "notes.md", true).unwrap();
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
    let reply = run(&mut p, &io, &call(9, |_| {})).unwrap();
    assert_eq!(rows(&reply).len() as u32, old.toc_len(&old_io) + 1);
}

#[test]
fn bad_calls_get_their_refusals() {
    let io = Roots::new("n.md", BOOK.as_bytes());
    let mut p = markdown_provider();
    assert!(matches!(
        run(&mut p, &io, &call(5, |_| {})),
        Err(Error::NotFound)
    ));
    assert!(matches!(run(&mut p, &io, &[7]), Err(Error::Invalid)));
    let reply = run(&mut p, &io, &call(1, |c| c.document = 0)).unwrap();
    assert_eq!(rows(&reply).len(), 0, "markdown does not sniff");
    run(&mut p, &io, &call(2, |c| c.text = "n.md")).unwrap();
    assert!(matches!(
        run(&mut p, &io, &call(2, |c| c.text = "n.md")),
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
            c.text = "gone.md";
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
