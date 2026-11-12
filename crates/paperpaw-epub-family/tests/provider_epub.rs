//! The EPUB `core.format` provider against the interface-6 reader on the
//! same file: the same blocks (texts, runs, maps), whole or in fragments, and
//! the refusals a hostile call gets. The book is root 3 (named by the call's
//! `text`), the converted page root 6.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::book;
use paperpaw_epub_family::{EpubDocument, epub_provider};
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
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.mem.list(&self.map(dir), visit)
    }
    fn exists(&self, path: &str) -> bool {
        self.mem.exists(&self.map(path))
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        if !path.starts_with(&self.prefix) {
            return Err(StorageError::Unsupported);
        }
        self.mem.create_dir(&self.map(path))
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        if !path.starts_with(&self.prefix) {
            return Err(StorageError::Unsupported);
        }
        self.mem.remove_dir_all(&self.map(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        if !from.starts_with(&self.prefix) || !to.starts_with(&self.prefix) {
            return Err(StorageError::Unsupported);
        }
        self.mem.rename(&self.map(from), &self.map(to))
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

fn book_bytes() -> Vec<u8> {
    book(
        &[
            "<h1>A Title</h1><p>First <em>paragraph</em> with <code>code</code> and a <a href=\"c1.xhtml#two\">link</a>.</p><ul><li>one</li><li>two</li></ul><blockquote>quoted words</blockquote>".to_string(),
            "<h2 id=\"two\">Two</h2><p>Second chapter text.</p><pre>let x = 1;</pre>".to_string(),
        ],
        &[],
    )
}

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
    section: u32,
    offset: u32,
) -> Result<Joined, Error> {
    let (mut text, mut runs, mut maps, mut span) = (String::new(), Vec::new(), Vec::new(), (0, 0));
    let (mut cursor, mut generation) = (0, 0);
    loop {
        let reply = run(
            p,
            io,
            &call(8, |c| {
                c.section = section;
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
    section: u32,
    offset: u32,
) -> [i64; 13] {
    let reply = run(
        p,
        io,
        &call(8, |c| {
            c.section = section;
            c.offset = offset;
        }),
    )
    .unwrap();
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
fn an_epub_book_reads_like_interface_6_block_by_block() {
    let io = Roots::new("notes.epub", &book_bytes());
    let mut p = epub_provider();
    let reply = run(&mut p, &io, &call(2, |c| c.text = "notes.epub")).unwrap();
    let stage::Record::FmtState(s) = rows(&reply)[0] else {
        panic!("state")
    };
    assert_eq!((s.state, s.status), (0, 0), "{s:?}");
    let old_io = MemoryStorage::new().with("notes.epub", book_bytes());
    let old = EpubDocument::open(&old_io, "notes.epub").unwrap();
    let mut count = 0;
    for section in 0..old.sections() {
        let mut at = Locator::new(section, 0);
        while let Some(expected) = old.block_at(&old_io, at).unwrap() {
            let (text, runs, maps, span, _) = block(&mut p, &io, section, at.offset).unwrap();
            assert_eq!(text, expected.text, "block {count}");
            assert_eq!(span, (expected.start, expected.end));
            let want: Vec<_> = expected
                .runs
                .iter()
                .map(|r| (r.start, r.end, r.style.bits()))
                .collect();
            assert_eq!(runs, want);
            assert_eq!(maps, expected.map.checkpoints().collect::<Vec<_>>());
            let h = header(&mut p, &io, section, at.offset);
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
            at = Locator::new(section, expected.end);
            count += 1;
        }
        assert!(matches!(
            block(&mut p, &io, section, at.offset),
            Err(Error::NotFound)
        ));
    }
    assert!(count >= 5, "{count} blocks");
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
    let io = Roots::new("n.epub", &book_bytes());
    let mut p = epub_provider();
    assert!(matches!(
        run(&mut p, &io, &call(5, |_| {})),
        Err(Error::NotFound)
    ));
    assert!(matches!(run(&mut p, &io, &[7]), Err(Error::Invalid)));
    let reply = run(&mut p, &io, &call(1, |c| c.document = 0)).unwrap();
    assert_eq!(rows(&reply).len(), 0, "no data is no epub");
    run(&mut p, &io, &call(2, |c| c.text = "n.epub")).unwrap();
    assert!(matches!(
        run(&mut p, &io, &call(2, |c| c.text = "n.epub")),
        Err(Error::Invalid)
    ));
    for op in [13, 15] {
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
            c.text = "gone.epub";
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

#[test]
fn the_first_bytes_tell_an_epub_from_another_zip() {
    let io = Roots::new("n.epub", &book_bytes());
    let mut p = epub_provider();
    let head: &'static [u8] = Box::leak(book_bytes()[..512].to_vec().into_boxed_slice());
    let reply = run(&mut p, &io, &call(1, |c| c.arg = head)).unwrap();
    assert!(matches!(rows(&reply)[0], stage::Record::FmtYes(_)));
    let other: &'static [u8] = Box::leak(vec![b'P', b'K', 3, 4, 0, 0, 0, 0].into_boxed_slice());
    let reply = run(&mut p, &io, &call(1, |c| c.arg = other)).unwrap();
    assert_eq!(rows(&reply).len(), 0);
}

#[test]
#[ignore = "waits for Core todo 208 (fallible shared parser): a refused allocation aborts the process while the parser allocates infallibly"]
fn every_allocation_of_a_read_may_be_refused() {
    panic!(
        "acceptance case: refuse each allocation of open, meta, sections and block; NoMemory, never a panic"
    );
}

const TTF: &[u8] = include_bytes!("data/atkinson-subset.ttf");

/// A book with one embedded font and a chapter with a language of its own.
fn font_book() -> Vec<u8> {
    font_book_with("a.ttf", TTF.to_vec(), "")
}

/// As [`font_book`], the font file `name` holding `data`, `encryption` the
/// content of `META-INF/encryption.xml` (empty: none).
fn font_book_with(name: &str, data: Vec<u8>, encryption: &str) -> Vec<u8> {
    font_book_full("urn:test", &[(name, data)], encryption)
}

/// A book with the fonts `fonts` (file name, data) as families `F0`, `F1`, …,
/// the unique identifier `identifier`, and `encryption` the content of
/// `META-INF/encryption.xml` (empty: none).
fn font_book_full(identifier: &str, fonts: &[(&str, Vec<u8>)], encryption: &str) -> Vec<u8> {
    let package = common::opf(
        &format!(
            "<dc:identifier id=\"uid\">{identifier}</dc:identifier><dc:language>de</dc:language>"
        ),
        &[
            ("c", "c.xhtml", "application/xhtml+xml", ""),
            ("css", "style.css", "text/css", ""),
        ],
        &["c"],
    );
    let mut css = String::new();
    let mut body = String::new();
    for (i, (name, _)) in fonts.iter().enumerate() {
        css.push_str(&format!(
            "@font-face {{ font-family: F{i}; src: url(fonts/{name}) }} .f{i} {{ font-family: F{i} }}\n"
        ));
        body.push_str(&format!(
            "<p class=\"f{i}\">Font text {i}, <span lang=\"fr\">bonjour</span> again</p>"
        ));
    }
    let paths: Vec<String> = fonts
        .iter()
        .map(|(n, _)| format!("OEBPS/fonts/{n}"))
        .collect();
    let mut files: Vec<(&str, Vec<u8>)> = vec![
        (
            "META-INF/container.xml",
            common::container("OEBPS/content.opf").into_bytes(),
        ),
        ("OEBPS/content.opf", package.into_bytes()),
        ("OEBPS/c.xhtml", common::xhtml(&body).into_bytes()),
        ("OEBPS/style.css", css.into_bytes()),
    ];
    for (path, (_, data)) in paths.iter().zip(fonts) {
        files.push((path.as_str(), data.clone()));
    }
    if !encryption.is_empty() {
        files.push(("META-INF/encryption.xml", encryption.as_bytes().to_vec()));
    }
    common::zip(&files, false)
}

#[test]
fn an_embedded_font_is_written_to_the_cache_and_listed() {
    let io = Roots::new("f.epub", &font_book());
    let mut p = epub_provider();
    run(&mut p, &io, &call(2, |c| c.text = "f.epub")).unwrap();
    let reply = run(&mut p, &io, &call(16, |_| {})).unwrap();
    let faces: Vec<_> = rows(&reply)
        .into_iter()
        .filter_map(|r| match r {
            stage::Record::FmtFont(f) => Some((f.index, f.family, f.flags, f.bytes)),
            _ => None,
        })
        .collect();
    assert_eq!(faces.len(), 1, "{faces:?}");
    assert_eq!((faces[0].0, faces[0].2), (0, 0));
    assert!(faces[0].1 >= 1, "a family");
    assert_eq!(faces[0].3 as usize, TTF.len());
    // The file the host reads: `fonts/0` of the document cache (root 6).
    let path = format!("{}/fonts/0", Env::plugin_dir("format-epub"));
    assert_eq!(io.size(&path).unwrap(), TTF.len() as u64);
    // Asked again, it is found there whole and listed the same.
    let again = run(&mut p, &io, &call(16, |_| {})).unwrap();
    assert_eq!(rows(&again).len(), rows(&reply).len());
    // No half-written file stays beside it.
    assert!(!io.exists(&format!("{path}.tmp")));
}

#[test]
fn a_run_in_another_language_names_it_by_its_index() {
    let io = Roots::new("f.epub", &font_book());
    let mut p = epub_provider();
    run(&mut p, &io, &call(2, |c| c.text = "f.epub")).unwrap();
    let reply = run(&mut p, &io, &call(8, |_| {})).unwrap();
    let mut block_lang = String::new();
    let mut langs = Vec::new();
    let mut indexes = Vec::new();
    for row in rows(&reply) {
        match row {
            stage::Record::FmtBlock(b) => block_lang = b.lang.to_string(),
            stage::Record::FmtInlineLang(l) => langs.push(l.lang.to_string()),
            stage::Record::FmtRun(r) => {
                indexes.push(paperpaw_sdk::document::TextStyle::from_bits(r.style).lang_index())
            }
            _ => {}
        }
    }
    assert_eq!(block_lang, "de", "the book's language");
    assert_eq!(langs, ["fr"]);
    assert!(indexes.contains(&1u8), "a run names the first: {indexes:?}");
}

/// Whether the provider's sniff (op 1) takes `head` for an EPUB.
fn sniffs(head: &[u8]) -> bool {
    let io = Roots::new("n.epub", &book_bytes());
    let mut p = epub_provider();
    let head: &'static [u8] = Box::leak(head.to_vec().into_boxed_slice());
    let reply = run(&mut p, &io, &call(1, |c| c.arg = head)).unwrap();
    !rows(&reply).is_empty()
}

#[test]
fn a_zip_that_is_not_quite_an_epub_is_not_sniffed() {
    use paperpaw_zip::testing::ZipBuilder;
    let epub = |name: &str, mime: &str| {
        ZipBuilder::new()
            .stored(name, mime.as_bytes().to_vec())
            .deflated("META-INF/container.xml", b"<container/>".to_vec())
            .build()
    };
    assert!(sniffs(&epub("mimetype", "application/epub+zip")));
    // The wrong case, in the entry's name or in what it says.
    assert!(!sniffs(&epub("MIMETYPE", "application/epub+zip")));
    assert!(!sniffs(&epub("Mimetype", "application/epub+zip")));
    assert!(!sniffs(&epub("mimetype", "Application/EPUB+zip")));
    // The mimetype deflated (the format asks for it stored).
    let packed = ZipBuilder::new()
        .deflated("mimetype", b"application/epub+zip".to_vec())
        .build();
    assert!(!sniffs(&packed));
    // The mimetype not the first entry.
    let later = ZipBuilder::new()
        .stored("a.txt", b"a".to_vec())
        .stored("mimetype", b"application/epub+zip".to_vec())
        .build();
    assert!(!sniffs(&later));
    // A head too short for the name and the extra field its header counts,
    // or for the text after them: never a read past it.
    let whole = epub("mimetype", "application/epub+zip");
    for cut in [0, 3, 8, 29, 30, 34, 37, 38, 45, 57] {
        assert!(!sniffs(&whole[..cut.min(whole.len())]), "cut at {cut}");
    }
    let mut padded = whole.clone();
    padded[28..30].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(!sniffs(&padded), "an extra field longer than the head");
    let mut named = whole;
    named[26..28].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(!sniffs(&named), "a name longer than the head");
}

/// The root-6 file of face 0 after the faces were asked for.
fn face_file(book: Vec<u8>) -> (Vec<u8>, u32) {
    let io = Roots::new("f.epub", &book);
    let mut p = epub_provider();
    run(&mut p, &io, &call(2, |c| c.text = "f.epub")).unwrap();
    let reply = run(&mut p, &io, &call(16, |_| {})).unwrap();
    let bytes = rows(&reply)
        .into_iter()
        .find_map(|r| match r {
            stage::Record::FmtFont(f) => Some(f.bytes),
            _ => None,
        })
        .expect("a face");
    let path = format!("{}/fonts/0", Env::plugin_dir("format-epub"));
    assert!(
        !io.exists(&format!("{path}.tmp")),
        "no temporary file stays"
    );
    let mut data = vec![0u8; bytes as usize];
    if bytes > 0 {
        assert_eq!(io.read_at(&path, 0, &mut data).unwrap(), data.len());
    }
    (data, bytes)
}

#[test]
fn an_obfuscated_font_is_revealed_as_it_is_copied() {
    // IDPF obfuscation: the first 1,040 bytes XORed with the SHA-1 of the
    // book's identifier (here `urn:test`).
    let mut sha = paperpaw_sdk::digest::sha1::Sha1::new();
    sha.update(b"urn:test");
    let key = sha.finish();
    let mut hidden = TTF.to_vec();
    for (i, byte) in hidden.iter_mut().take(1040).enumerate() {
        *byte ^= key[i % key.len()];
    }
    let encryption = "<encryption xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><EncryptedData xmlns=\"http://www.w3.org/2001/04/xmlenc#\"><EncryptionMethod Algorithm=\"http://www.idpf.org/2008/embedding\"/><CipherData><CipherReference URI=\"OEBPS/fonts/a.ttf\"/></CipherData></EncryptedData></encryption>";
    let (data, bytes) = face_file(font_book_with("a.ttf", hidden, encryption));
    assert_eq!(bytes as usize, TTF.len());
    assert_eq!(data, TTF, "the font as the book's maker made it");
}

#[test]
fn a_woff_font_is_converted_whole_and_still_listed() {
    let woff = include_bytes!("data/atkinson-subset.woff").to_vec();
    let (data, bytes) = face_file(font_book_with("a.woff", woff, ""));
    assert!(bytes > 0, "the converted font is listed");
    assert_eq!(&data[..4], &[0, 1, 0, 0], "a TrueType file");
}

#[test]
fn a_font_that_is_not_in_the_book_is_listed_with_no_bytes() {
    let book = font_book_with("a.ttf", Vec::new(), "");
    let (_, bytes) = face_file(book);
    assert_eq!(bytes, 0);
}

#[test]
fn a_plain_font_is_copied_in_chunks_and_a_woff_is_left_to_the_whole_path() {
    let copy = |name: &str, data: Vec<u8>| {
        let storage = MemoryStorage::new().with("f.epub", font_book_with(name, data, ""));
        let doc = EpubDocument::open(&storage, "f.epub").unwrap();
        let mut params = 0u32.to_le_bytes().to_vec();
        params.extend_from_slice(b"copy/0");
        let answer = doc
            .extended(&storage, "format-epub.font-copy", &params)
            .unwrap();
        (answer, storage)
    };
    let (answer, storage) = copy("a.ttf", TTF.to_vec());
    assert_eq!(answer[0], 0, "written");
    assert_eq!(
        u32::from_le_bytes(answer[1..5].try_into().unwrap()) as usize,
        TTF.len()
    );
    assert_eq!(storage.size("copy/0").unwrap() as usize, TTF.len());
    let (answer, storage) = copy(
        "a.woff",
        include_bytes!("data/atkinson-subset.woff").to_vec(),
    );
    assert_eq!(answer[0], 1, "not streamed");
    assert!(!storage.exists("copy/0") && !storage.exists("copy/0.tmp"));
    // A key the document does not know is not answered.
    let storage = MemoryStorage::new().with("f.epub", font_book());
    let doc = EpubDocument::open(&storage, "f.epub").unwrap();
    assert!(doc.extended(&storage, "other", &[]).is_none());
}

/// The faces of a book as the provider lists them: (index, bytes).
fn faces_of(
    p: &mut paperpaw_epub_family::provider::FormatProvider,
    io: &dyn Storage,
) -> Vec<(u32, u32)> {
    let reply = run(p, io, &call(16, |_| {})).unwrap();
    rows(&reply)
        .into_iter()
        .filter_map(|r| match r {
            stage::Record::FmtFont(f) => Some((f.index, f.bytes)),
            _ => None,
        })
        .collect()
}

/// `size` bytes of filler: it deflates to almost nothing.
fn filler(size: usize) -> Vec<u8> {
    vec![0x41; size]
}

#[test]
fn a_font_over_the_size_limit_is_its_own_fault_and_the_others_are_listed() {
    let big = filler(17 * 1024 * 1024);
    let book = font_book_full("urn:test", &[("big.ttf", big), ("a.ttf", TTF.to_vec())], "");
    let io = Roots::new("f.epub", &book);
    let mut p = epub_provider();
    run(&mut p, &io, &call(2, |c| c.text = "f.epub")).unwrap();
    let faces = faces_of(&mut p, &io);
    assert_eq!(faces.len(), 2, "{faces:?}");
    assert_eq!(faces[0], (0, 0), "the large one: no bytes, no error");
    assert_eq!(faces[1], (1, TTF.len() as u32), "the other is listed");
    assert!(!io.exists(&format!("{}/fonts/0.tmp", Env::plugin_dir("format-epub"))));
}

#[test]
fn the_copied_faces_of_a_book_stop_at_the_total_cap() {
    // Three of 12 MiB: the third would make 36 MiB, over the 32 MiB cap.
    let fonts: Vec<(&str, Vec<u8>)> = ["a.ttf", "b.ttf", "c.ttf"]
        .iter()
        .map(|n| (*n, filler(12 * 1024 * 1024)))
        .collect();
    let io = Roots::new("f.epub", &font_book_full("urn:test", &fonts, ""));
    let mut p = epub_provider();
    run(&mut p, &io, &call(2, |c| c.text = "f.epub")).unwrap();
    let faces = faces_of(&mut p, &io);
    let size = 12 * 1024 * 1024;
    assert_eq!(faces, [(0, size), (1, size), (2, 0)]);
    let third = format!("{}/fonts/2", Env::plugin_dir("format-epub"));
    assert!(!io.exists(&third), "the face over the cap is not kept");
}

/// A storage that counts reads of the book and can refuse appends.
struct Spy {
    inner: Roots,
    book_reads: std::cell::Cell<u32>,
    fail_appends: std::cell::Cell<bool>,
}

impl Storage for Spy {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if !path.starts_with(&self.inner.prefix) {
            self.book_reads.set(self.book_reads.get() + 1);
        }
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if self.fail_appends.get() {
            return Err(StorageError::Io);
        }
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.inner.exists(path)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.inner.create_dir(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove_dir_all(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.rename(from, to)
    }
}

#[test]
fn a_face_that_fails_to_copy_leaves_no_temporary_file_and_is_not_tried_again() {
    let io = Spy {
        inner: Roots::new("f.epub", &font_book()),
        book_reads: std::cell::Cell::new(0),
        fail_appends: std::cell::Cell::new(false),
    };
    let mut p = epub_provider();
    run(&mut p, &io, &call(2, |c| c.text = "f.epub")).unwrap();
    // The book is converted; now the card refuses appends (full, failing).
    io.fail_appends.set(true);
    assert_eq!(faces_of(&mut p, &io), [(0, 0)], "listed with no bytes");
    let path = format!("{}/fonts/0", Env::plugin_dir("format-epub"));
    assert!(!io.exists(&path) && !io.exists(&format!("{path}.tmp")));
    // Remembered for the open book: the next call does not read the book
    // again, even where appends work now.
    io.fail_appends.set(false);
    let reads = io.book_reads.get();
    assert_eq!(faces_of(&mut p, &io), [(0, 0)]);
    assert_eq!(io.book_reads.get(), reads, "the face was inflated again");
}

#[test]
fn an_adobe_obfuscated_font_is_revealed_as_it_is_copied() {
    // Adobe obfuscation: the first 1,024 bytes XORed with the 16 bytes of
    // the book's UUID identifier.
    let uuid = "urn:uuid:0123456789abcdef0123456789abcdef";
    let key: Vec<u8> = (0..16)
        .map(|i| u8::from_str_radix(&uuid[9 + 2 * i..11 + 2 * i], 16).unwrap())
        .collect();
    let mut hidden = TTF.to_vec();
    for (i, byte) in hidden.iter_mut().take(1024).enumerate() {
        *byte ^= key[i % 16];
    }
    let encryption = "<encryption xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><EncryptedData xmlns=\"http://www.w3.org/2001/04/xmlenc#\"><EncryptionMethod Algorithm=\"http://ns.adobe.com/pdf/enc#RC\"/><CipherData><CipherReference URI=\"OEBPS/fonts/a.ttf\"/></CipherData></EncryptedData></encryption>";
    let book = font_book_full(uuid, &[("a.ttf", hidden)], encryption);
    let (data, bytes) = face_file(book);
    assert_eq!(bytes as usize, TTF.len());
    assert_eq!(data, TTF);
}

#[test]
fn every_paged_list_ends_with_the_last_page_bit_and_a_generation_that_is_never_0() {
    let io = Roots::new("notes.epub", &book_bytes());
    let mut p = epub_provider();
    run(&mut p, &io, &call(2, |c| c.text = "notes.epub")).unwrap();
    // Sections (7), contents (9), links (14) and fonts (16): `document.md` section 3,
    // paged replies: bit 0 the last page and no other flag, `generation` the format's own.
    for op in [7, 9, 14, 16] {
        let (mut cursor, mut generation) = (0, 0);
        loop {
            let reply = run(
                &mut p,
                &io,
                &call(op, |c| {
                    c.cursor = cursor;
                    c.generation = generation;
                }),
            )
            .unwrap();
            let all = rows(&reply);
            let Some(stage::Record::FmtEnd(end)) = all.last().copied() else {
                panic!("op {op}: no end")
            };
            assert_ne!(end.generation, 0, "op {op}");
            assert_eq!(end.flags & !1, 0, "op {op}: only bit 0");
            if end.flags & 1 == 1 {
                break;
            }
            assert!(
                end.next > cursor,
                "op {op}: a page that is not the last goes on"
            );
            (cursor, generation) = (end.next, end.generation);
        }
    }
}

/// Every file below `dir` of `io`'s cache, as paths.
fn cached_files(io: &Roots, dir: &str, into: &mut Vec<String>) {
    let mut entries = Vec::new();
    let _ = io
        .mem
        .list(dir, &mut |e| entries.push((e.name.to_string(), e.is_dir)));
    for (name, is_dir) in entries {
        let path = format!("{dir}/{name}");
        if is_dir {
            cached_files(io, &path, into);
        } else {
            into.push(path);
        }
    }
}

/// The state flags of an open of `notes.epub` with the open flags `flags`.
fn open_flags(
    p: &mut paperpaw_epub_family::provider::FormatProvider,
    io: &Roots,
    flags: u32,
) -> u32 {
    let reply = run(
        p,
        io,
        &call(2, |c| {
            c.text = "notes.epub";
            c.flags = flags;
        }),
    )
    .unwrap();
    let stage::Record::FmtState(s) = rows(&reply)[0] else {
        panic!("state")
    };
    assert_eq!((s.state, s.status), (0, 0), "{s:?}");
    s.flags
}

#[test]
fn a_background_open_builds_nothing_and_a_normal_one_after_it_finds_everything_out() {
    const XPOINTERS: u32 = 1 << 2;
    let io = Roots::new("notes.epub", &book_bytes());
    let mut p = epub_provider();
    // A background open (the library's cover, an index build): no contents,
    // positions or labels, and nothing written but what a cover needs.
    assert_eq!(open_flags(&mut p, &io, 4) & XPOINTERS, 0);
    let mut files = Vec::new();
    cached_files(&io, "cache", &mut files);
    assert!(files.is_empty(), "{files:?}");
    run(&mut p, &io, &call(4, |_| {})).unwrap();
    // A normal open of the same book finds the flags out, as ever.
    assert_eq!(open_flags(&mut p, &io, 0) & XPOINTERS, XPOINTERS);
    let mut files = Vec::new();
    cached_files(&io, "cache", &mut files);
    assert!(
        files.iter().any(|f| f.ends_with("package.ini")),
        "{files:?}"
    );
}
