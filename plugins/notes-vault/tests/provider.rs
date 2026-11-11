//! The Markdown `core.format` provider against the interface-6 reader on the
//! same note of a vault: the same blocks (texts, runs, maps), whole or in
//! fragments, with the vault's pictures read from the card by the card path the
//! call's `text` names, and the refusals a hostile call gets. The book is root 3
//! (`path ""`), the conversion the document's cache (root 6).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_plugin_notes_vault::MarkdownDocument;
use paperpaw_plugin_notes_vault::native_format::{VaultIo, call, markdown_provider};
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{DirEntry, Env, MemoryStorage, Storage, StorageError};

/// Root 3 (the one book, `path ""`) and root 6 (below the plugin's folder).
struct Roots {
    mem: MemoryStorage,
    prefix: String,
}

impl Roots {
    fn new(book: &[u8]) -> Self {
        Self {
            mem: MemoryStorage::new().with("book", book.to_vec()),
            prefix: Env::plugin_dir("notes-vault"),
        }
    }
    fn map(&self, path: &str) -> String {
        match path.strip_prefix(&self.prefix) {
            Some(rest) => format!("cache{rest}"),
            None => "book".to_string(),
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

fn frame<'a>(op: u32, f: impl FnOnce(&mut stage::FmtCall<'a>)) -> Vec<u8> {
    let mut c = stage::FmtCall::new(op, 1, 0, 0, 0, 0, 0, 1, 0, 0, 7, "", &[]);
    f(&mut c);
    let mut buf = vec![0u8; 4096];
    let n = stage::input_encode(&[stage::Record::FmtCall(c)], &mut buf).unwrap();
    buf.truncate(n);
    buf
}

const NOTE_PATH: &str = "v/Notes/n.md";

const NOTE: &str = "---\ntags: [a, b]\n---\n# A Title\n\nSome *words* and [[Other|a label]], \
[[Other#Big Part]] and `code`.\n\n![[Cat.png]] ![[gone.png]] ![[Other]]\n\n\
> [!warning] Careful now\n> The body.\n\n| a | b |\n|---|---|\n| 1 | [[n|x]] |\n\n\
## Big Part\n\n- one\n- two\n";

/// The vault on the card: the note, another, a picture below.
fn card() -> MemoryStorage {
    MemoryStorage::new()
        .with("v/.obsidian/app.json", b"{}".to_vec())
        .with(NOTE_PATH, NOTE.as_bytes().to_vec())
        .with("v/Notes/Other.md", b"# Other\n".to_vec())
        .with("v/Files/deep/Cat.PNG", b"not a picture".to_vec())
}

fn run(
    p: &mut paperpaw_epub_family::provider::FormatProvider,
    base: &dyn Storage,
    card: &dyn Storage,
    input: &[u8],
) -> Result<Vec<u8>, Error> {
    let mut out = vec![0u8; 3840];
    let n = call(p, base, card, input, &mut out)?;
    out.truncate(n);
    Ok(out)
}

fn rows(reply: &[u8]) -> Vec<stage::Record<'_>> {
    stage::reply_decode(reply)
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// Text, runs (start, end, style), maps and span of a block, joined from its
/// fragments.
type Joined = (String, Vec<(u32, u32, u32)>, Vec<(u32, u32)>, (u32, u32));

fn block(
    p: &mut paperpaw_epub_family::provider::FormatProvider,
    base: &dyn Storage,
    card: &dyn Storage,
    offset: u32,
) -> Result<Joined, Error> {
    let (mut text, mut runs, mut maps, mut span) = (String::new(), Vec::new(), Vec::new(), (0, 0));
    let (mut cursor, mut generation) = (0, 0);
    loop {
        let reply = run(
            p,
            base,
            card,
            &frame(8, |c| {
                c.offset = offset;
                c.cursor = cursor;
                c.generation = generation;
            }),
        )?;
        let mut end = (0, 0, 0);
        for row in rows(&reply) {
            match row {
                stage::Record::FmtBlock(b) => span = (b.start, b.end),
                stage::Record::FmtTextPart(t) => text.push_str(t.text),
                stage::Record::FmtRun(r) => runs.push((r.start, r.end, r.style)),
                stage::Record::FmtMap(m) => maps.push((m.text, m.source)),
                stage::Record::FmtEnd(e) => end = (e.flags, e.generation, e.next),
                _ => {}
            }
        }
        if end.0 & 2 == 0 {
            return Ok((text, runs, maps, span));
        }
        (cursor, generation) = (end.2, end.1);
    }
}

/// The provider's blocks of a note, each against interface 6's.
fn reads_like_interface_6(open_text: &'static str) {
    let base = Roots::new(NOTE.as_bytes());
    let card = card();
    let mut p = markdown_provider().unwrap();
    let reply = run(&mut p, &base, &card, &frame(2, |c| c.text = open_text)).unwrap();
    let stage::Record::FmtState(s) = rows(&reply)[0] else {
        panic!("state")
    };
    assert_eq!((s.state, s.status), (0, 0), "{s:?}");
    let old_io = self::card();
    let old = MarkdownDocument::open_in(&old_io, NOTE_PATH, true, !open_text.is_empty()).unwrap();
    let mut at = Locator::START;
    let mut count = 0;
    while let Some(expected) = old.block_at(&old_io, at).unwrap() {
        let (text, runs, maps, span) = block(&mut p, &base, &card, at.offset).unwrap();
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
    assert!(count >= 8, "{count} blocks");
    assert!(matches!(
        block(&mut p, &base, &card, at.offset),
        Err(Error::NotFound)
    ));
    let reply = run(&mut p, &base, &card, &frame(5, |_| {})).unwrap();
    match rows(&reply)[0] {
        stage::Record::FmtMeta(m) => {
            if open_text.is_empty() {
                // No name to take the title from (root 3 has none): the host's
                // library names an untitled book by its file.
                assert_eq!(m.title, "");
            } else {
                assert_eq!(Some(m.title), old.metadata().title.as_deref());
            }
            assert_eq!(m.sections, old.sections());
        }
        other => panic!("not meta: {other:?}"),
    }
}

#[test]
fn a_note_of_a_vault_reads_like_interface_6_block_by_block() {
    reads_like_interface_6(NOTE_PATH);
}

#[test]
fn a_note_without_a_card_path_reads_without_a_vault() {
    // No name in the text: the note is root 3 alone and nothing is looked for
    // on the card; the same text as the old reader gives a note outside a vault.
    reads_like_interface_6("");
}

#[test]
fn the_card_is_read_only_and_the_cache_is_not() {
    let base = Roots::new(b"x");
    let card = card();
    let prefix = Env::plugin_dir("notes-vault");
    let io = VaultIo::new(&base, &card, &prefix, NOTE_PATH);
    // The note is the book, other card paths are the card.
    let mut buf = [0u8; 8];
    assert_eq!(io.read_at(NOTE_PATH, 0, &mut buf).unwrap(), 1);
    assert_eq!(io.size("v/Notes/Other.md").unwrap(), 8);
    assert!(io.write("v/x.md", b"x").is_err());
    assert!(io.append("v/x.md", b"x").is_err());
    assert!(io.remove("v/Notes/Other.md").is_err());
    let cache = format!("{prefix}/a/b");
    io.write(&cache, b"kept").unwrap();
    assert_eq!(io.size(&cache).unwrap(), 4);
    // Only the card lists; the book's folder is not listed.
    let mut names = Vec::new();
    io.list("v/Notes", &mut |e| names.push(e.name.to_string()))
        .unwrap();
    names.sort();
    assert_eq!(names, ["Other.md", "n.md"]);
    assert!(io.list(&prefix, &mut |_| {}).is_err());
}

#[test]
fn bad_calls_get_their_refusals() {
    let base = Roots::new(NOTE.as_bytes());
    let card = card();
    let mut p = markdown_provider().unwrap();
    assert!(matches!(
        run(&mut p, &base, &card, &frame(5, |_| {})),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        run(&mut p, &base, &card, &[7]),
        Err(Error::Invalid)
    ));
    let reply = run(&mut p, &base, &card, &frame(1, |c| c.document = 0)).unwrap();
    assert_eq!(rows(&reply).len(), 0, "markdown does not sniff");
    run(&mut p, &base, &card, &frame(2, |c| c.text = NOTE_PATH)).unwrap();
    assert!(matches!(
        run(&mut p, &base, &card, &frame(2, |c| c.text = NOTE_PATH)),
        Err(Error::Invalid)
    ));
    assert!(matches!(
        run(&mut p, &base, &card, &frame(99, |_| {})),
        Err(Error::Unsupported)
    ));
    // A note that is no file on the card fails the document, not the provider.
    let missing = Roots {
        mem: MemoryStorage::new(),
        prefix: Env::plugin_dir("notes-vault"),
    };
    let reply = run(
        &mut p,
        &missing,
        &card,
        &frame(2, |c| {
            c.document = 2;
            c.text = "v/gone.md";
        }),
    )
    .unwrap();
    let stage::Record::FmtState(s) = rows(&reply)[0] else {
        panic!("state")
    };
    assert_eq!(s.state, 2);
    run(&mut p, &base, &card, &frame(4, |_| {})).unwrap();
    run(&mut p, &base, &card, &frame(4, |_| {})).unwrap();
}

#[test]
fn hostile_texts_and_paths_make_no_panic() {
    let base = Roots::new(b"[[a]]]]![[b]] | x |\n|-|\n> [!");
    let card = card();
    let long = "a/".repeat(500);
    for text in [
        "",
        "/",
        "../../etc",
        "v/Notes/n.md/x",
        long.as_str(),
        "v/Notes/\u{0}",
    ] {
        let mut p = markdown_provider().unwrap();
        let _ = run(&mut p, &base, &card, &frame(2, |c| c.text = text));
        let _ = run(&mut p, &base, &card, &frame(8, |_| {}));
    }
}
