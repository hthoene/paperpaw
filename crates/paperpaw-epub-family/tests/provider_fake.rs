//! The provider over a small made-up document: what a real parser does not
//! produce on demand (a long language, a picture in the cache), and the
//! provider's refusals under hostile calls and a refused allocation.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_epub_family::provider::FormatProvider;
use paperpaw_sdk::document::{
    Annotation, AnnotationKind, Block, BlockKind, Document, DocumentError, ImageRef, Locator,
    Metadata,
};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{Env, MemoryStorage, OpenOptions, Storage};

struct Fake {
    meta: Metadata,
}

fn cache() -> String {
    Env::plugin_dir("format-epub")
}

impl Document for Fake {
    fn metadata(&self) -> &Metadata {
        &self.meta
    }
    fn sections(&self) -> u32 {
        1
    }
    fn section_len(&self, _: u32) -> u32 {
        40
    }
    fn block_at(&self, _: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        Ok(match at.offset {
            0..=9 => {
                let mut b = Block::new(BlockKind::Paragraph, 0, 10, "some words".into());
                b.lang = paperpaw_sdk::Lang::parse("de");
                b.inline_langs = Some(Box::new([
                    paperpaw_sdk::Lang::parse("fr"),
                    paperpaw_sdk::Lang::parse("pt-BR"),
                ]));
                for (i, kind) in [
                    AnnotationKind::Ruby,
                    AnnotationKind::Phonetic,
                    AnnotationKind::Gloss,
                    AnnotationKind::Other(2),
                ]
                .into_iter()
                .enumerate()
                {
                    b.annotations
                        .push(Annotation::new(0, 4, format!("note{i}"), kind));
                }
                Some(b)
            }
            10..=19 => {
                let mut b = Block::new(BlockKind::Image, 10, 20, String::new());
                b.image = Some(ImageRef {
                    path: format!("{}/pic.gray", cache()),
                    width: 30,
                    height: 40,
                });
                Some(b)
            }
            // A picture the document decodes when a page shows it: its file
            // is not there yet (`prepare_image`).
            20..=29 => {
                let mut b = Block::new(BlockKind::Image, 20, 30, String::new());
                b.image = Some(ImageRef {
                    path: format!("{}/lazy.gray", cache()),
                    width: 30,
                    height: 40,
                });
                Some(b)
            }
            // A picture that cannot be served (not a file of the cache), with
            // its alt text.
            30..=39 => {
                let mut b = Block::new(BlockKind::Image, 30, 40, "a cat on a mat".into());
                b.image = Some(ImageRef {
                    path: "elsewhere/pic.gray".into(),
                    width: 30,
                    height: 40,
                });
                Some(b)
            }
            _ => None,
        })
    }
    fn prepare_image(&self, _: &dyn Storage, image: &ImageRef) -> Option<ImageRef> {
        image.path.ends_with("/lazy.gray").then(|| ImageRef {
            path: format!("{}/decoded.gray", cache()),
            width: 30,
            height: 40,
        })
    }
}

fn open(_: &dyn Storage, _: &str, _: &OpenOptions) -> Result<Box<dyn Document>, DocumentError> {
    let mut meta = Metadata::default();
    meta.title = Some("T".into());
    // 40 bytes: longer than a call carries (35), as interface 6 opened it.
    meta.language = Some("x-a-very-long-private-use-language-tag-1".into());
    Ok(Box::new(Fake { meta }))
}

fn provider() -> FormatProvider {
    FormatProvider::new("fake.fake", open, cache(), |_| false)
}

fn call(op: u32, f: impl FnOnce(&mut stage::FmtCall<'static>)) -> Vec<u8> {
    let mut c = stage::FmtCall::new(op, 1, 0, 0, 0, 0, 0, 1, 0, 0, 7, "", &[]);
    f(&mut c);
    let mut buf = vec![0u8; 4096];
    let n = stage::input_encode(&[stage::Record::FmtCall(c)], &mut buf).unwrap();
    buf.truncate(n);
    buf
}

fn run(p: &mut FormatProvider, io: &dyn Storage, input: &[u8]) -> Result<Vec<u8>, Error> {
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

fn opened() -> (FormatProvider, MemoryStorage) {
    let io = MemoryStorage::new();
    let mut p = provider();
    let reply = run(&mut p, &io, &call(2, |c| c.text = "b.fake")).unwrap();
    let stage::Record::FmtState(s) = rows(&reply)[0] else {
        panic!("state")
    };
    assert_eq!(s.state, 0, "{s:?}");
    (p, io)
}

#[test]
fn a_language_longer_than_a_call_carries_is_cut_not_refused() {
    let (mut p, io) = opened();
    let reply = run(&mut p, &io, &call(5, |_| {})).unwrap();
    let stage::Record::FmtMeta(m) = rows(&reply)[0] else {
        panic!("meta")
    };
    assert_eq!(m.language.len(), 35);
    assert!("x-a-very-long-private-use-language-tag-1".starts_with(m.language));
}

#[test]
fn the_picture_of_a_block_is_served_by_its_start() {
    let (mut p, io) = opened();
    let reply = run(&mut p, &io, &call(18, |c| c.index = 10)).unwrap();
    match rows(&reply)[0] {
        stage::Record::FmtImage(i) => {
            assert_eq!(
                (i.path, i.index, i.width, i.height),
                ("pic.gray", 10, 30, 40)
            );
        }
        other => panic!("not an image: {other:?}"),
    }
    // A text block, a place inside a block, and one past the end have none.
    for index in [0, 12, 99] {
        assert!(
            matches!(
                run(&mut p, &io, &call(18, |c| c.index = index)),
                Err(Error::NotFound)
            ),
            "{index}"
        );
    }
}

#[test]
fn hostile_calls_are_refused_and_the_provider_reads_on() {
    let (mut p, io) = opened();
    // Garbage, an empty frame, an unknown op, and ops on a closed document.
    for input in [&[][..], &[0xff; 9][..], &[7][..]] {
        assert!(run(&mut p, &io, input).is_err());
    }
    assert!(matches!(
        run(&mut p, &io, &call(99, |_| {})),
        Err(Error::Unsupported)
    ));
    for document in [0, 2, 9, u32::MAX] {
        assert!(run(&mut p, &io, &call(8, |c| c.document = document)).is_err());
    }
    // A cursor without its generation is a stale fragment.
    assert!(matches!(
        run(&mut p, &io, &call(8, |c| c.cursor = 5)),
        Err(Error::State)
    ));
    // Offsets past everything.
    for offset in [40, u32::MAX] {
        assert!(matches!(
            run(&mut p, &io, &call(8, |c| c.offset = offset)),
            Err(Error::NotFound)
        ));
    }
    // And it still serves the first block.
    let reply = run(&mut p, &io, &call(8, |_| {})).unwrap();
    assert!(
        rows(&reply)
            .iter()
            .any(|r| matches!(r, stage::Record::FmtTextPart(t) if t.text == "some words"))
    );
}

#[test]
fn inline_languages_and_ruby_kinds_are_sent() {
    let (mut p, io) = opened();
    let reply = run(&mut p, &io, &call(8, |_| {})).unwrap();
    let mut langs = Vec::new();
    let mut kinds = Vec::new();
    let mut block_lang = "";
    for row in rows(&reply) {
        match row {
            stage::Record::FmtBlock(b) => block_lang = b.lang,
            stage::Record::FmtInlineLang(l) => langs.push(l.lang),
            stage::Record::FmtRuby(r) => kinds.push((r.kind, r.text)),
            _ => {}
        }
    }
    assert_eq!(block_lang, "de");
    assert_eq!(langs, ["fr", "pt-BR"]);
    assert_eq!(
        kinds,
        [(0, "note0"), (1, "note1"), (2, "note2"), (5, "note3")]
    );
}

/// The order the host checks (document.md section 4): header, text, runs,
/// maps, picture, columns, cells, rubies, inline languages. A block with the
/// languages before its text is refused, and with it the book.
#[test]
fn a_blocks_records_come_in_the_order_the_host_checks() {
    fn rank(r: &stage::Record<'_>) -> u8 {
        match r {
            stage::Record::FmtBlock(_) => 0,
            stage::Record::FmtTextPart(_) => 1,
            stage::Record::FmtRun(_) => 2,
            stage::Record::FmtMap(_) => 3,
            stage::Record::FmtImage(_) => 4,
            stage::Record::FmtColumns(_) => 5,
            stage::Record::FmtCell(_) => 6,
            stage::Record::FmtRuby(_) => 7,
            stage::Record::FmtInlineLang(_) => 8,
            _ => 9,
        }
    }
    let (mut p, io) = opened();
    for offset in [0, 10, 20, 40] {
        let Ok(reply) = run(&mut p, &io, &call(8, |c| c.offset = offset)) else {
            continue;
        };
        let ranks: Vec<u8> = rows(&reply).iter().map(rank).filter(|r| *r != 9).collect();
        assert!(
            ranks.windows(2).all(|w| w[0] <= w[1]),
            "offset {offset}: {ranks:?}"
        );
    }
    let reply = block_at(&mut p, &io, 0);
    assert!(
        rows(&reply)
            .iter()
            .any(|r| matches!(r, stage::Record::FmtInlineLang(_)))
    );
}

fn block_at(p: &mut FormatProvider, io: &dyn Storage, offset: u32) -> Vec<u8> {
    run(p, io, &call(8, |c| c.offset = offset)).unwrap()
}

/// What the host checks of a block stream: a picture block (kind 3) has
/// exactly one picture record, and no other has one.
fn pictures_match_kind(reply: &[u8]) {
    let rows = rows(reply);
    let stage::Record::FmtBlock(block) = rows[0] else {
        panic!("block header first")
    };
    let images = rows
        .iter()
        .filter(|r| matches!(r, stage::Record::FmtImage(_)))
        .count();
    assert_eq!(
        images == 1,
        block.kind == 3,
        "kind {}, {images} pictures",
        block.kind
    );
    assert!(images <= 1);
}

#[test]
fn a_picture_the_document_decodes_late_is_decoded_before_its_block_goes_out() {
    let (mut p, io) = opened();
    let reply = block_at(&mut p, &io, 20);
    pictures_match_kind(&reply);
    let rows = rows(&reply);
    let stage::Record::FmtBlock(block) = rows[0] else {
        panic!("block")
    };
    assert_eq!(block.kind, 3);
    assert!(rows.iter().any(|r| matches!(
        r,
        stage::Record::FmtImage(i) if i.path == "decoded.gray" && (i.width, i.height) == (30, 40)
    )));
}

#[test]
fn a_picture_that_cannot_be_served_costs_the_picture_only_never_the_book() {
    let (mut p, io) = opened();
    let reply = block_at(&mut p, &io, 30);
    pictures_match_kind(&reply);
    let rows = rows(&reply);
    // A paragraph with the block's own text, no picture record.
    let stage::Record::FmtBlock(block) = rows[0] else {
        panic!("block")
    };
    assert_eq!(block.kind, 0);
    assert_eq!(block.text_bytes, "a cat on a mat".len() as u32);
    assert!(rows.iter().any(|r| matches!(
        r,
        stage::Record::FmtTextPart(t) if t.text == "a cat on a mat"
    )));
    // The ones with their pictures in the cache are unchanged.
    pictures_match_kind(&block_at(&mut p, &io, 10));
    // The same picture asked for by number is not found, as before.
    assert!(matches!(
        run(&mut p, &io, &call(18, |c| c.index = 30)),
        Err(Error::NotFound)
    ));
}
