//! EPUB 2 and 3 support for PaperPaw.
//!
//! - Metadata (title, authors, language) from the package document.
//! - Reading order from the spine; table of contents from the EPUB 3
//!   navigation document or the EPUB 2 NCX.
//! - Chapters are converted on first use, streamed from the ZIP archive
//!   through an XHTML tokenizer, into a compact block format stored in the
//!   plugin's cache. Reading afterwards needs only the block at hand, so
//!   books of any size work on small devices.
//!
//! Supported markup: paragraphs, headings, lists, block quotes, line
//! breaks, preformatted text, anchors and links, styles and images.
//!
//! HTML files (`.html`, `.htm`, `.xhtml`), e.g. saved web pages, open
//! the same way, showing only a page's reading matter (see
//! [`HtmlDocument`]), Markdown (`.md`, see [`MarkdownDocument`]),
//! FictionBooks (`.fb2`, see [`Fb2Document`]) and comic book archives
//! (`.cbz`, see [`CbzDocument`]): each a plugin of its own, so that it can
//! be switched off or replaced alone ([`HtmlFormat`], [`MarkdownFormat`],
//! [`Fb2Format`], [`CbzFormat`]).

#![no_std]

extern crate alloc;

mod cache_paths;
mod cached;
mod cbz;
mod convert;
mod css;
mod document;
mod fb2;
mod html;
mod markdown;
mod obfuscation;
mod package;
mod pictures;
mod woff;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec;

use paperpaw_sdk::document::Document;
use paperpaw_sdk::{FormatSpec, Id, Manifest, Plugin, RegisterError, Registrar};

pub use cbz::CbzDocument;
pub use document::EpubDocument;
pub use fb2::Fb2Document;
pub use html::HtmlDocument;
pub use markdown::MarkdownDocument;

pub const PLUGIN: Id = Id::new_static("format-epub");
pub const FORMAT: Id = Id::new_static("format-epub.epub");

/// HTML files, e.g. web pages: the plugin and its format.
pub const HTML_PLUGIN: Id = Id::new_static("format-html");
pub const HTML: Id = Id::new_static("format-html.html");
/// FictionBook 2.
pub const FB2_PLUGIN: Id = Id::new_static("format-fb2");
pub const FB2: Id = Id::new_static("format-fb2.fb2");
/// Comic book archives.
pub const CBZ_PLUGIN: Id = Id::new_static("format-cbz");
pub const CBZ: Id = Id::new_static("format-cbz.cbz");
/// Markdown.
pub const MARKDOWN_PLUGIN: Id = Id::new_static("format-markdown");
pub const MARKDOWN: Id = Id::new_static("format-markdown.markdown");

/// A plugin's manifest: its id, its name and what it does.
fn manifest(
    id: Id,
    name: &'static str,
    description: &'static str,
    how: &'static [(&'static str, &'static str)],
) -> Manifest {
    Manifest::new(id, name, paperpaw_sdk::package_version!())
        .description(description)
        .how(how)
        .uses(paperpaw_sdk::Uses::NOTHING)
}

/// How each works, by language (see `Manifest::how`).
static EPUB_HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/format-epub/en.txt")),
    ("de", include_str!("../how/format-epub/de.txt")),
];
static HTML_HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/format-html/en.txt")),
    ("de", include_str!("../how/format-html/de.txt")),
];
static FB2_HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/format-fb2/en.txt")),
    ("de", include_str!("../how/format-fb2/de.txt")),
];
static MARKDOWN_HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/format-markdown/en.txt")),
    ("de", include_str!("../how/format-markdown/de.txt")),
];
static CBZ_HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/format-cbz/en.txt")),
    ("de", include_str!("../how/format-cbz/de.txt")),
];

/// EPUB books. HTML, FictionBook, Markdown and comic books are plugins of
/// their own ([`HtmlFormat`], [`Fb2Format`], [`MarkdownFormat`],
/// [`CbzFormat`]), so each can be switched off or replaced alone; they
/// share the conversion of this crate.
#[derive(Debug, Default)]
pub struct EpubFormat;

impl Plugin for EpubFormat {
    fn manifest(&self) -> Manifest {
        manifest(
            PLUGIN,
            "EPUB",
            "Opens EPUB books with their styles, table of contents and images.",
            &EPUB_HOW,
        )
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // What is converted of each book, moved and removed with it.
        r.add_book_cache(paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str()), "")?;
        // One's own stylesheets as found to tell whether a book opens at
        // once, for opening it.
        let kept = Rc::new(document::PreparedCss::default());
        let prepared = Rc::clone(&kept);
        r.add_format(
            FormatSpec::new(
                FORMAT,
                "format-epub.title",
                vec![Cow::Borrowed("epub")],
                Box::new(move |storage, path, options| {
                    EpubDocument::open_prepared(storage, path, options, &kept)
                        .map(|d| Box::new(d) as Box<dyn Document>)
                }),
            )
            .prepared(Box::new(move |storage, path, options| {
                EpubDocument::prepared_keeping(storage, path, options, &prepared)
            }))
            .sniff(is_epub),
        )?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// HTML files (`.html`, `.htm`, `.xhtml`), e.g. saved web pages: only a
/// page's reading matter (see [`HtmlDocument`]).
#[derive(Debug, Default)]
pub struct HtmlFormat;

impl Plugin for HtmlFormat {
    fn manifest(&self) -> Manifest {
        manifest(
            HTML_PLUGIN,
            "HTML",
            "Opens web pages saved as files, showing only what is to be read.",
            &HTML_HOW,
        )
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // What is converted of each book, moved and removed with it.
        r.add_book_cache(paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str()), "html-")?;
        r.add_format(FormatSpec::new(
            HTML,
            "format-html.title",
            vec![
                Cow::Borrowed("html"),
                Cow::Borrowed("htm"),
                Cow::Borrowed("xhtml"),
            ],
            Box::new(|storage, path, options| {
                HtmlDocument::open(storage, path, options.publisher_styles)
                    .map(|d| Box::new(d) as Box<dyn Document>)
            }),
        ))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/html/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/html/de.ini")))?;
        Ok(())
    }
}

/// FictionBooks (`.fb2`, `.fb2.zip`, see [`Fb2Document`]).
#[derive(Debug, Default)]
pub struct Fb2Format;

impl Plugin for Fb2Format {
    fn manifest(&self) -> Manifest {
        manifest(
            FB2_PLUGIN,
            "FictionBook",
            "Opens FictionBook books (.fb2, also zipped).",
            &FB2_HOW,
        )
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // What is converted of each book, moved and removed with it.
        r.add_book_cache(paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str()), "fb2-")?;
        r.add_format(FormatSpec::new(
            FB2,
            "format-fb2.title",
            vec![Cow::Borrowed("fb2"), Cow::Borrowed("fb2.zip")],
            Box::new(|storage, path, options| {
                Fb2Document::open_options(storage, path, options)
                    .map(|d| Box::new(d) as Box<dyn Document>)
            }),
        ))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/fb2/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/fb2/de.ini")))?;
        Ok(())
    }
}

/// Markdown (`.md`, `.markdown`, see [`MarkdownDocument`]).
#[derive(Debug, Default)]
pub struct MarkdownFormat;

impl Plugin for MarkdownFormat {
    fn manifest(&self) -> Manifest {
        manifest(
            MARKDOWN_PLUGIN,
            "Markdown",
            "Opens Markdown files (.md), e.g. notes.",
            &MARKDOWN_HOW,
        )
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // What is converted of each book, moved and removed with it.
        r.add_book_cache(paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str()), "md-")?;
        r.add_format(FormatSpec::new(
            MARKDOWN,
            "format-markdown.title",
            vec![Cow::Borrowed("md"), Cow::Borrowed("markdown")],
            Box::new(|storage, path, options| {
                MarkdownDocument::open(storage, path, options.publisher_styles)
                    .map(|d| Box::new(d) as Box<dyn Document>)
            }),
        ))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/markdown/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/markdown/de.ini")))?;
        Ok(())
    }
}

/// Comic book archives (`.cbz`, see [`CbzDocument`]).
#[derive(Debug, Default)]
pub struct CbzFormat;

impl Plugin for CbzFormat {
    fn manifest(&self) -> Manifest {
        manifest(
            CBZ_PLUGIN,
            "Comic books",
            "Opens comic book archives (.cbz), a picture a page.",
            &CBZ_HOW,
        )
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // What is converted of each book, moved and removed with it.
        r.add_book_cache(paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str()), "cbz-")?;
        r.add_format(FormatSpec::new(
            CBZ,
            "format-cbz.title",
            vec![Cow::Borrowed("cbz")],
            Box::new(|storage, path, options| {
                CbzDocument::open_options(storage, path, options)
                    .map(|d| Box::new(d) as Box<dyn Document>)
            }),
        ))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/cbz/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/cbz/de.ini")))?;
        Ok(())
    }
}

/// Bytes at each end of a file that tell it from another of its size.
const ENDS: usize = 4096;

/// What tells the file at `path` from another of its name: its size and a
/// hash of its first and last 4 KB, where an archive keeps its directory
/// with the checksums of all it holds (storages tell no time a file was
/// changed). On the heap: books are opened deep in the stack.
pub(crate) fn fingerprint(
    storage: &dyn paperpaw_sdk::Storage,
    path: &str,
) -> Result<alloc::string::String, paperpaw_sdk::StorageError> {
    let size = storage.size(path)?;
    let mut ends = vec![0u8; 2 * ENDS];
    let (head, tail) = ends.split_at_mut(ENDS);
    let head = read_full(storage, path, 0, head)?;
    let tail = read_full(storage, path, size.saturating_sub(ENDS as u64), tail)?;
    ends.copy_within(ENDS..ENDS + tail, head);
    let hash = paperpaw_sdk::digest::fnv1a64(&ends[..head + tail]);
    Ok(alloc::format!("{size} {hash:016x}"))
}

/// Fills `buf` from `offset` of `path` as far as the file goes; how much.
fn read_full(
    storage: &dyn paperpaw_sdk::Storage,
    path: &str,
    offset: u64,
    buf: &mut [u8],
) -> Result<usize, paperpaw_sdk::StorageError> {
    let mut filled = 0;
    while filled < buf.len() {
        match storage.read_at(path, offset + filled as u64, &mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// The first bytes of `path`, at most 4 KB: to know a file again, or what
/// it is. On the heap: books are opened deep in the stack.
pub(crate) fn head(
    storage: &dyn paperpaw_sdk::Storage,
    path: &str,
) -> Result<alloc::vec::Vec<u8>, paperpaw_sdk::StorageError> {
    let mut head = vec![0u8; 4096];
    let n = storage.read_at(path, 0, &mut head)?;
    head.truncate(n);
    Ok(head)
}

/// Whether a file's first bytes are an EPUB's: a ZIP whose first entry
/// is `mimetype`, stored, saying `application/epub+zip` (as the format
/// asks), not any ZIP.
fn is_epub(head: &[u8]) -> bool {
    const MIME: &[u8] = b"application/epub+zip";
    let field = |at: usize| {
        head.get(at..at + 2)
            .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
    };
    let (Some(method), Some(name), Some(extra)) = (field(8), field(26), field(28)) else {
        return false;
    };
    let data = 30 + name + extra;
    head.starts_with(b"PK\x03\x04")
        && method == 0
        && head.get(30..30 + name) == Some(&b"mimetype"[..])
        && head.get(data..data + MIME.len()) == Some(MIME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::vec::Vec;
    use paperpaw_i18n::{Catalog, check};
    use paperpaw_sdk::MemoryStorage;
    use paperpaw_sdk::document::{Align, Block, BlockKind, Locator, TextStyle};
    use paperpaw_sdk::lang::{Direction, Lang};
    use paperpaw_zip::testing::ZipBuilder;

    #[test]
    fn translations_are_complete() {
        for (en, de) in [
            (
                include_str!("../i18n/en.ini"),
                include_str!("../i18n/de.ini"),
            ),
            (
                include_str!("../i18n/html/en.ini"),
                include_str!("../i18n/html/de.ini"),
            ),
            (
                include_str!("../i18n/fb2/en.ini"),
                include_str!("../i18n/fb2/de.ini"),
            ),
            (
                include_str!("../i18n/markdown/en.ini"),
                include_str!("../i18n/markdown/de.ini"),
            ),
            (
                include_str!("../i18n/cbz/en.ini"),
                include_str!("../i18n/cbz/de.ini"),
            ),
        ] {
            let (en, de) = (Catalog::parse(en).unwrap(), Catalog::parse(de).unwrap());
            let issues = check::compare(&en, &de);
            assert!(issues.is_empty(), "{issues:?}");
        }
    }

    const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;

    const OPF: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Die   Verwandlung</dc:title>
    <dc:creator>Franz Kafka</dc:creator>
    <dc:language>de</dc:language>
    <dc:subject>Novella</dc:subject>
    <dc:subject>Classics</dc:subject>
    <dc:description>&lt;p&gt;Gregor wakes up &amp;amp; finds&lt;/p&gt;&lt;p&gt;himself changed.&lt;/p&gt;</dc:description>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="text/ch%202.xhtml" media-type="application/xhtml+xml"/>
    <item id="css" href="style.css" media-type="text/css"/>
  </manifest>
  <spine><itemref idref="c1"/><itemref idref="c2"/></spine>
</package>"#;

    const NAV: &str = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body><nav epub:type="toc"><ol>
  <li><a href="text/ch1.xhtml">Erstes Kapitel</a>
    <ol><li><a href="text/ch1.xhtml#later">Später</a></li></ol></li>
  <li><a href="text/ch%202.xhtml">Zweites   Kapitel</a></li>
</ol></nav>
<nav epub:type="page-list"><ol><li><a href="text/ch1.xhtml#later">7</a></li></ol></nav>
</body></html>"#;

    const CH1: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Ignored</title>
<style>p { color: red }</style></head>
<body>
  <h1>I.</h1>
  <p>Als Gregor Samsa eines Morgens aus <em>unruhigen</em>
     Tr&#228;umen erwachte &#8212; fand er sich&nbsp;verwandelt.</p>
  <p>Zeile eins<br/>Zeile zwei</p>
  <ul><li>Punkt A</li><li>Punkt B</li></ul>
  <p id="later">Später.</p>
</body></html>"#;

    fn epub(ch2: String) -> MemoryStorage {
        epub_in(OPF, ch2)
    }

    /// The test book with package `opf`.
    fn epub_in(opf: &str, ch2: String) -> MemoryStorage {
        let zip = ZipBuilder::new()
            .stored("mimetype", "application/epub+zip")
            .deflated("META-INF/container.xml", CONTAINER)
            .deflated("OEBPS/content.opf", opf)
            .deflated("OEBPS/nav.xhtml", NAV)
            .deflated("OEBPS/text/ch1.xhtml", CH1)
            .deflated("OEBPS/text/ch 2.xhtml", ch2)
            .stored(
                "OEBPS/images/dot.png",
                &include_bytes!("../../../crates/paperpaw-image/tests/data/gray.png")[..],
            )
            .deflated(
                "OEBPS/style.css",
                "/* book styles */ .c { text-align: center } .hide { display: none }",
            )
            .build();
        MemoryStorage::new().with("books/test.epub", zip)
    }

    #[test]
    fn positions_as_koreader_writes_them() {
        // Text nodes and their characters as crengine keeps them: white
        // space folded, white space alone at the start of a block dropped.
        let ch2 = String::from(
            "<html><body><div><p>Eins</p><p>Zwei <b>fett</b> und  mehr</p></div>\n\
             <div><h2>Titel</h2><p>Drei &amp; vier\n   f\u{fc}nf</p><p>\n  Hallo</p>\
             <p>\n<em>x</em> y</p></div></body></html>",
        );
        let storage = epub(ch2);
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        let blocks: Vec<u32> = {
            let mut at = Locator::new(1, 0);
            let mut starts = Vec::new();
            while let Some(block) = doc.block_at(&storage, at).unwrap() {
                starts.push(block.start);
                at = Locator::new(1, block.end);
            }
            starts
        };
        let xp = |offset| {
            let x = doc.xpointer(&storage, Locator::new(1, offset)).unwrap();
            String::from(x.strip_prefix("/body/DocFragment[2]/body/").unwrap())
        };
        assert_eq!(xp(blocks[0]), "div[1]/p[1]/text()[1].0");
        // "Zwei fett und mehr"
        assert_eq!(xp(blocks[1] + 2), "div[1]/p[2]/text()[1].2");
        assert_eq!(xp(blocks[1] + 5), "div[1]/p[2]/b[1]/text()[1].0");
        assert_eq!(xp(blocks[1] + 9), "div[1]/p[2]/text()[2].0");
        assert_eq!(xp(blocks[1] + 14), "div[1]/p[2]/text()[2].5");
        assert_eq!(xp(blocks[2]), "div[2]/h2[1]/text()[1].0");
        // "Drei & vier fünf": characters, not bytes.
        assert_eq!(xp(blocks[3] + 12), "div[2]/p[1]/text()[1].12");
        assert_eq!(xp(blocks[3] + 15), "div[2]/p[1]/text()[1].14");
        // The folded space before "Hallo" is in the node.
        assert_eq!(xp(blocks[4]), "div[2]/p[2]/text()[1].1");
        // The line break alone before <em> is no node.
        assert_eq!(xp(blocks[5]), "div[2]/p[3]/em[1]/text()[1].0");
        assert_eq!(xp(blocks[5] + 2), "div[2]/p[3]/text()[1].1");

        let at = |x: &str| {
            doc.locate_xpointer(&storage, &alloc::format!("/body/DocFragment[2]/body/{x}"))
                .map(|l| l.offset)
        };
        // Back again, also as KOReader writes them without [1].
        for (i, offset) in [
            (0, 0),
            (1, 2),
            (1, 5),
            (1, 9),
            (1, 14),
            (3, 15),
            (4, 0),
            (5, 2),
        ] {
            let x = xp(blocks[i] + offset);
            assert_eq!(at(&x), Some(blocks[i] + offset), "{x}");
        }
        assert_eq!(at("div/p[2]/text()[2].5"), Some(blocks[1] + 14));
        assert_eq!(at("div[1]/p[2]/b/text().1"), Some(blocks[1] + 6));
        // Elements, and text PaperPaw does not show: the block.
        assert_eq!(at("div[2]"), Some(blocks[2]));
        assert_eq!(at("div[2]/p[2]/text().0"), Some(blocks[4]));
        assert_eq!(at("div[1]/p[2]/b"), Some(blocks[1]));
        assert_eq!(
            doc.locate_xpointer(&storage, "/body/DocFragment[9]/body/p"),
            None
        );
        assert_eq!(doc.locate_xpointer(&storage, "nonsense"), None);
    }

    #[test]
    fn positions_go_on_in_split_paragraphs() {
        let words = "wort ".repeat(1500);
        let ch2 = alloc::format!("<html><body><p>{words}</p></body></html>");
        let storage = epub(ch2);
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        let first = doc.block_at(&storage, Locator::new(1, 0)).unwrap().unwrap();
        let second = doc
            .block_at(&storage, Locator::new(1, first.end))
            .unwrap()
            .unwrap();
        // The second block goes on after the first's text and the space
        // it was split at.
        let char = first.text.chars().count() + 1;
        let x = doc
            .xpointer(&storage, Locator::new(1, second.start + 3))
            .unwrap();
        assert_eq!(
            x,
            alloc::format!("/body/DocFragment[2]/body/p[1]/text()[1].{}", char + 3)
        );
        assert_eq!(
            doc.locate_xpointer(&storage, &x),
            Some(Locator::new(1, second.start + 3))
        );
    }

    #[test]
    fn embedded_fonts_are_listed_used_and_revealed() {
        let opf = OPF
            .replace(
                "<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\">",
                "<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"uid\">",
            )
            .replace(
                "<dc:language>de</dc:language>",
                "<dc:language>de</dc:language><dc:identifier id=\"uid\">urn:isbn:123</dc:identifier>",
            );
        let font: Vec<u8> = (0..1500).map(|i| (i % 253) as u8).collect();
        let mut hidden = font.clone();
        obfuscation::reveal(&mut hidden, obfuscation::Method::Idpf, "urn:isbn:123");
        let encryption = r#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container"
            xmlns:enc="http://www.w3.org/2001/04/xmlenc#"><enc:EncryptedData>
            <enc:EncryptionMethod Algorithm="http://www.idpf.org/2008/embedding"/>
            <enc:CipherData><enc:CipherReference URI="OEBPS/fonts/Fancy.ttf"/></enc:CipherData>
            </enc:EncryptedData></encryption>"#;
        let zip = ZipBuilder::new()
            .stored("mimetype", "application/epub+zip")
            .deflated("META-INF/container.xml", CONTAINER)
            .deflated("META-INF/encryption.xml", encryption)
            .deflated("OEBPS/content.opf", opf.as_str())
            .deflated("OEBPS/nav.xhtml", NAV)
            .deflated("OEBPS/text/ch1.xhtml", CH1)
            .deflated(
                "OEBPS/text/ch 2.xhtml",
                "<html><body><p class=\"fancy\">Schmuck <b>fett</b></p><p>Normal</p></body></html>",
            )
            .stored("OEBPS/fonts/Fancy.ttf", &hidden[..])
            .deflated(
                "OEBPS/style.css",
                "@font-face { font-family: Fancy; src: url(fonts/Fancy.ttf) } .fancy { font-family: Fancy }",
            )
            .build();
        let storage = MemoryStorage::new().with("books/test.epub", zip);
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        let fonts = doc.fonts(&storage);
        assert_eq!(fonts.len(), 1);
        assert_eq!(
            (fonts[0].family, fonts[0].bold, fonts[0].italic),
            (1, false, false)
        );
        assert_eq!(doc.font_data(&storage, 0).unwrap(), font);
        let fancy = doc.block_at(&storage, Locator::new(1, 0)).unwrap().unwrap();
        assert_eq!(fancy.style_at(0).family(), 1);
        // Bold inside keeps the family.
        let bold = fancy.text.find("fett").unwrap();
        assert_eq!(fancy.style_at(bold), TextStyle::BOLD.with_family(1));
        let plain = doc
            .block_at(&storage, Locator::new(1, fancy.end))
            .unwrap()
            .unwrap();
        assert_eq!(plain.style_at(0).family(), 0);
        // Without the book's styles there are no embedded fonts either.
        let plain_doc = EpubDocument::open_with(&storage, "books/test.epub", false).unwrap();
        assert!(plain_doc.fonts(&storage).is_empty());
    }

    #[test]
    fn woff_fonts_come_out_as_their_font() {
        let woff = include_bytes!("../tests/data/atkinson-subset.woff");
        let zip = ZipBuilder::new()
            .stored("mimetype", "application/epub+zip")
            .deflated("META-INF/container.xml", CONTAINER)
            .deflated("OEBPS/content.opf", OPF)
            .deflated("OEBPS/nav.xhtml", NAV)
            .deflated("OEBPS/text/ch1.xhtml", CH1)
            .deflated(
                "OEBPS/text/ch 2.xhtml",
                "<html><body><p class=\"web\">Web</p></body></html>",
            )
            .stored("OEBPS/fonts/Web.woff", &woff[..])
            .deflated(
                "OEBPS/style.css",
                "@font-face { font-family: Web; src: url(fonts/Web.woff) format('woff') } \
                 .web { font-family: Web }",
            )
            .build();
        let storage = MemoryStorage::new().with("books/web.epub", zip);
        let doc = EpubDocument::open(&storage, "books/web.epub").unwrap();
        assert_eq!(doc.fonts(&storage).len(), 1);
        let font = doc.font_data(&storage, 0).unwrap();
        // A TrueType font again, as the reader's fonts read it.
        assert_eq!(&font[..4], &[0, 1, 0, 0]);
        let ttf = include_bytes!("../tests/data/atkinson-subset.ttf");
        assert!(font.len() >= ttf.len() - 64 && font.len() <= ttf.len() + 64);
    }

    fn blocks(
        storage: &MemoryStorage,
        doc: &EpubDocument,
        section: u32,
    ) -> Vec<(BlockKind, String)> {
        let mut at = Locator::new(section, 0);
        let mut out = Vec::new();
        while let Some(block) = doc.block_at(storage, at).unwrap() {
            assert!(block.end > at.offset || at.offset < block.start);
            at = Locator::new(section, block.end);
            out.push((block.kind, block.text));
        }
        out
    }

    #[test]
    fn links_lead_within_and_out_of_the_book() {
        let ch2 = String::from(
            "<html><body><p>Siehe <a href=\"ch1.xhtml#later\">später<sup>1</sup></a> \
             und <a href=\"#end\">unten</a>.</p>\
             <p>Mehr <a href=\"https://example.org/x?a=1&amp;b=2\">im Netz</a>.</p>\
             <p id=\"end\">Ende <a>ohne Ziel</a>.</p></body></html>",
        );
        let storage = epub(ch2);
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        let links = doc.links(&storage, 1, 0, u32::MAX);
        let hrefs: Vec<&str> = links.iter().map(|l| l.href.as_str()).collect();
        assert_eq!(
            hrefs,
            [
                "OEBPS/text/ch1.xhtml#later",
                "OEBPS/text/ch 2.xhtml#end",
                "https://example.org/x?a=1&b=2"
            ]
        );
        // The ranges cover the link texts.
        let first = doc.block_at(&storage, Locator::new(1, 0)).unwrap().unwrap();
        let text_of = |link: &paperpaw_sdk::document::Link| {
            let from = first.text_index(link.start);
            let to = first.text_index(link.end);
            String::from(&first.text[from..to])
        };
        assert_eq!(text_of(&links[0]), "später1");
        assert_eq!(text_of(&links[1]), "unten");
        // Only those overlapping a range.
        let tail = doc.links(&storage, 1, links[1].start, u32::MAX);
        assert_eq!(tail.len(), 2);
        // Into another chapter, at the anchor.
        let later = doc.link_target(&storage, &links[0].href).unwrap();
        assert_eq!(later.section, 0);
        let block = doc.block_at(&storage, later).unwrap().unwrap();
        assert_eq!(block.text, "Später.");
        let end = doc.link_target(&storage, &links[1].href).unwrap();
        let block = doc.block_at(&storage, end).unwrap().unwrap();
        assert_eq!(block.text, "Ende ohne Ziel.");
        assert_eq!(doc.link_target(&storage, &links[2].href), None);
    }

    #[test]
    fn reads_metadata_chapters_and_toc() {
        let ch2 = String::from("<html><body><p>Zweites.</p></body></html>");
        let storage = epub(ch2);
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        let metadata = &doc.metadata().clone();
        assert_eq!(metadata.title.as_deref(), Some("Die Verwandlung"));
        assert_eq!(metadata.authors, ["Franz Kafka"]);
        assert_eq!(metadata.language.as_deref(), Some("de"));
        assert_eq!(metadata.subjects, ["Novella", "Classics"]);
        // The printed edition's pages: from the anchor on, also in the
        // chapters after it.
        let later = doc.toc_target(&storage, 1).unwrap();
        assert_eq!(doc.page_label(&storage, Locator::new(0, 0)), None);
        assert_eq!(doc.page_label(&storage, later).as_deref(), Some("7"));
        assert_eq!(
            doc.page_label(&storage, Locator::new(1, 0)).as_deref(),
            Some("7")
        );
        assert_eq!(doc.sections(), 2);
        assert_eq!(
            metadata.description.as_deref(),
            Some("Gregor wakes up & finds himself changed.")
        );
        // Also when read from the cache.
        let again = EpubDocument::open(&storage, "books/test.epub").unwrap();
        assert_eq!(again.metadata().subjects, ["Novella", "Classics"]);
        assert_eq!(again.metadata().description, metadata.description);

        let ch1 = blocks(&storage, &doc, 0);
        let texts: Vec<&str> = ch1.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(
            texts,
            [
                "I.",
                "Als Gregor Samsa eines Morgens aus unruhigen Träumen erwachte — fand er sich\u{a0}verwandelt.",
                "Zeile eins\nZeile zwei",
                "• Punkt A",
                "• Punkt B",
                "Später."
            ]
        );
        assert_eq!(ch1[0].0, BlockKind::Heading(1));
        assert_eq!(blocks(&storage, &doc, 1)[0].1, "Zweites.");

        assert_toc(&storage, &doc);

        // A second open reads the package from the cache.
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        assert_eq!(doc.metadata(), metadata);
        assert_toc(&storage, &doc);
    }

    fn assert_toc(storage: &MemoryStorage, doc: &EpubDocument) {
        let titles: Vec<(String, u8)> = (0..doc.toc_len(storage))
            .map(|i| doc.toc_entry(storage, i).unwrap())
            .map(|e| (e.title, e.level))
            .collect();
        assert_eq!(
            titles,
            [
                (String::from("Erstes Kapitel"), 0),
                (String::from("Später"), 1),
                (String::from("Zweites Kapitel"), 0)
            ]
        );
        assert_eq!(doc.toc_target(storage, 2), Some(Locator::new(1, 0)));
        // The anchor points at the block with id="later".
        let later = doc.toc_target(storage, 1).unwrap();
        assert_eq!(later.section, 0);
        let block = doc.block_at(storage, later).unwrap().unwrap();
        assert_eq!(block.text, "Später.");
        assert!(doc.progress(Locator::new(1, 0)) > 0.5);
    }

    /// The blocks of chapter 2.
    fn chapter_blocks(storage: &MemoryStorage) -> Vec<Block> {
        let doc = EpubDocument::open(storage, "books/test.epub").unwrap();
        let mut blocks = Vec::new();
        let mut at = Locator::new(1, 0);
        while let Some(block) = doc.block_at(storage, at).unwrap() {
            at = Locator::new(1, block.end);
            blocks.push(block);
        }
        blocks
    }

    #[test]
    fn blocks_know_their_direction_and_language() {
        let ch2 = String::from(
            "<html lang=\"ar\" dir=\"rtl\"><body>\
             <p>\u{645}\u{631}\u{62d}\u{628}\u{627} <span lang=\"en\">hello</span></p>\
             <p style=\"text-align: right\">\u{646}\u{635}</p>\
             <p align=\"left\">\u{646}\u{635}</p>\
             <p dir=\"ltr\" lang=\"de\">Links <span dir=\"rtl\">bleibt</span></p>\
             <p dir=\"auto\">abc</p></body></html>",
        );
        let blocks = chapter_blocks(&epub(ch2));
        let ar = Lang::parse("ar");
        let found: Vec<(Direction, Lang, Align)> =
            blocks.iter().map(|b| (b.dir, b.lang, b.align)).collect();
        assert_eq!(
            found,
            [
                (Direction::Rtl, ar, Align::Auto),
                // `right` is where right-to-left lines start, `left` where
                // they end.
                (Direction::Rtl, ar, Align::Start),
                (Direction::Rtl, ar, Align::End),
                // An inline `dir` sets no paragraph's.
                (Direction::Ltr, Lang::parse("de"), Align::Auto),
                // `auto`: from the text, kept for its pieces.
                (Direction::Ltr, ar, Align::Auto),
            ]
        );
        // English inside Arabic: its run says so.
        let first = &blocks[0];
        let hello = first.text.find("hello").unwrap();
        assert_eq!(first.lang_of(first.style_at(hello)), Lang::parse("en"));
        assert_eq!(first.lang_of(first.style_at(0)), ar);
        assert_eq!(
            first.inline_langs.as_deref(),
            Some(&[Lang::parse("en")][..])
        );
    }

    #[test]
    fn inline_text_is_set_apart_by_dir_bdi_and_bdo() {
        let ch2 = String::from(
            "<html><body>\
             <p>\u{5d0} <span dir=\"ltr\">C++!</span> <bdi>x</bdi> <bdo dir=\"rtl\">ab</bdo></p>\
             <p><span dir=\"rtl\"> </span></p>\
             <p>\u{5d1}<em dir=\"ltr\">a <span dir=\"rtl\">\u{5d2}</span></em></p>\
             </body></html>",
        );
        let blocks = chapter_blocks(&epub(ch2));
        let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                // The spaces between stay outside.
                "\u{5d0} \u{2066}C++!\u{2069} \u{2068}x\u{2069} \u{202e}ab",
                // Nested, the innermost closed first; the block's end
                // drops the closing controls as it drops spaces, and a
                // block of them alone is none.
                "\u{5d1}\u{2066}a \u{2067}\u{5d2}",
            ]
        );
    }

    #[test]
    fn a_book_in_a_right_to_left_language_runs_right_to_left() {
        let opf = OPF.replace(
            "<dc:language>de</dc:language>",
            "<dc:language>he</dc:language>",
        );
        let ch2 = String::from(
            "<html><body><p>\u{5e9}\u{5dc}\u{5d5}\u{5dd}</p><p>123</p>\
             <p style=\"direction: ltr; text-align: left\">abc</p></body></html>",
        );
        let blocks = chapter_blocks(&epub_in(&opf, ch2));
        let found: Vec<(Direction, Align)> = blocks.iter().map(|b| (b.dir, b.align)).collect();
        assert_eq!(
            found,
            [
                (Direction::Rtl, Align::Auto),
                // Without a letter, the book's direction.
                (Direction::Rtl, Align::Auto),
                (Direction::Ltr, Align::Start),
            ]
        );
        assert!(blocks.iter().take(2).all(|b| b.lang == Lang::parse("he")));
    }

    #[test]
    fn keeps_styles_and_alignment() {
        let ch2 = String::from(
            "<html><body><center><p>Mitte <b>fett <i>beides</i></b>              <span style=\"font-style: italic\">kursiv</span></p></center>             <p>Normal <em>betont <span style=\"font-style:normal\">aufrecht</span></em></p>             <p align=\"right\">Rechts</p></body></html>",
        );
        let storage = epub(ch2);
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        let block = doc.block_at(&storage, Locator::new(1, 0)).unwrap().unwrap();
        assert_eq!(block.text, "Mitte fett beides kursiv");
        assert_eq!(block.align, Align::Center);
        let styled: Vec<(&str, TextStyle)> = block
            .runs
            .iter()
            .map(|r| (&block.text[r.start as usize..r.end as usize], r.style))
            .collect();
        assert_eq!(
            styled,
            [
                ("fett ", TextStyle::BOLD),
                ("beides", TextStyle::BOLD_ITALIC),
                ("kursiv", TextStyle::ITALIC)
            ]
        );
        let second = doc
            .block_at(&storage, Locator::new(1, block.end))
            .unwrap()
            .unwrap();
        assert_eq!(second.align, Align::Auto);
        assert_eq!(second.style_at(7), TextStyle::ITALIC);
        assert_eq!(second.style_at(second.text.len() - 1), TextStyle::REGULAR);
        let third = doc
            .block_at(&storage, Locator::new(1, second.end))
            .unwrap()
            .unwrap();
        assert_eq!((third.text.as_str(), third.align), ("Rechts", Align::End));
    }

    #[test]
    fn pictures_decoded_when_their_page_shows() {
        let ch2 = String::from(
            "<html><body><p>Vorher</p><img src=\"../images/dot.png\" alt=\"Punkt\"/></body></html>",
        );
        let storage = epub(ch2);
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        let first = doc.block_at(&storage, Locator::new(1, 0)).unwrap().unwrap();
        let block = doc
            .block_at(&storage, Locator::new(1, first.end))
            .unwrap()
            .unwrap();
        // Laid out at its size, not decoded yet.
        assert_eq!(block.kind, BlockKind::Image);
        let image = block.image.unwrap();
        assert_eq!((image.width, image.height), (37, 23));
        assert!(
            paperpaw_sdk::Storage::size(&storage, &image.path).is_err(),
            "not decoded with its chapter"
        );
        // Decoded when asked, before its page is drawn; then nothing to do.
        let decoded = doc.prepare_image(&storage, &image).unwrap();
        assert_eq!((decoded.width, decoded.height), (37, 23));
        assert!(paperpaw_sdk::Storage::size(&storage, &decoded.path).is_ok());
        assert_eq!(doc.prepare_image(&storage, &decoded), None);
    }

    /// An alternative text of three-byte characters longer than a record
    /// holds, so the record's limit falls inside a character.
    #[test]
    fn long_alternative_texts_are_cut_between_characters() {
        let alt = "日".repeat(1320);
        let ch2 = alloc::format!(
            "<html><body><img src=\"../images/missing.png\" alt=\"{alt}\"/>\
             <img src=\"../images/dot.png\" alt=\"{alt}\"/><p>Danach</p></body></html>"
        );
        let storage = epub(ch2);
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        let found = blocks(&storage, &doc, 1);
        let kinds: Vec<BlockKind> = found.iter().map(|(kind, _)| *kind).collect();
        assert_eq!(
            kinds,
            [BlockKind::Paragraph, BlockKind::Image, BlockKind::Paragraph]
        );
        // The missing picture's alternative text, cut where a character
        // ends, as is the shown one's.
        for (_, text) in &found[..2] {
            assert!(text.len() > 3000 && text.len() < 4000, "{}", text.len());
            assert!(text.chars().all(|c| c == '日'));
        }
        assert_eq!(found[2].1, "Danach");
    }

    #[test]
    fn decodes_images_into_the_cache() {
        let ch2 = String::from(
            "<html><body><p>Vorher</p><svg><image xlink:href=\"../images/dot.png\"/></svg>             <img src=\"../images/missing.png\" alt=\"Ein  Bild\"/><p>Nachher</p></body></html>",
        );
        let storage = epub(ch2);
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        let mut at = Locator::new(1, 0);
        let mut found = Vec::new();
        while let Some(block) = doc.block_at(&storage, at).unwrap() {
            at = Locator::new(1, block.end);
            let size = block.image.as_ref().map(|i| (i.width, i.height));
            found.push((block.kind, block.text.clone(), size));
        }
        assert_eq!(
            found,
            [
                (BlockKind::Paragraph, String::from("Vorher"), None),
                (BlockKind::Image, String::new(), Some((37, 23))),
                // A missing image becomes its alternative text.
                (BlockKind::Paragraph, String::from("Ein Bild"), None),
                (BlockKind::Paragraph, String::from("Nachher"), None),
            ]
        );
    }

    #[test]
    fn applies_stylesheets() {
        let ch2 = String::from(
            "<html><head><style>p.b { font-weight: bold }</style></head><body>             <p class=\"c\">Mitte</p><p class=\"x hide\">weg <b>auch weg</b></p>             <p class=\"b\">Fett</p><p>Normal</p></body></html>",
        );
        let storage = epub(ch2);
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        let mut at = Locator::new(1, 0);
        let mut found = Vec::new();
        while let Some(block) = doc.block_at(&storage, at).unwrap() {
            at = Locator::new(1, block.end);
            found.push((block.text.clone(), block.align, block.style_at(0)));
        }
        assert_eq!(
            found,
            [
                (String::from("Mitte"), Align::Center, TextStyle::REGULAR),
                (String::from("Fett"), Align::Auto, TextStyle::BOLD),
                (String::from("Normal"), Align::Auto, TextStyle::REGULAR),
            ]
        );
    }

    #[test]
    fn keeps_the_books_indents_and_spaces() {
        let ch2 = String::from(
            "<html><head><style>.head { padding-bottom: 1em }              .first { text-indent: 0; margin-top: 1.2em } p { margin: 0 }              div.box { text-indent: 2em; margin-bottom: 3em }</style></head><body>             <p class=\"head\">Title</p><p class=\"first\">One</p><p>Two</p>             <div class=\"box\"><p>Three</p></div><p>Four</p></body></html>",
        );
        let storage = epub(ch2);
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        let mut at = Locator::new(1, 0);
        let mut found = Vec::new();
        while let Some(block) = doc.block_at(&storage, at).unwrap() {
            at = Locator::new(1, block.end);
            found.push((block.text.clone(), block.indent, block.space));
        }
        let row = |t: &str, i, s| (String::from(t), i, s);
        assert_eq!(
            found,
            [
                row("Title", None, Some(0)),
                // The heading's padding plus the larger of the margins.
                row("One", Some(0), Some(220)),
                row("Two", None, Some(0)),
                // Indents are inherited.
                row("Three", Some(200), Some(0)),
                row("Four", None, Some(300)),
            ]
        );
        let plain = EpubDocument::open_with(&storage, "books/test.epub", false).unwrap();
        let block = plain
            .block_at(&storage, Locator::new(1, 0))
            .unwrap()
            .unwrap();
        assert_eq!((block.indent, block.space), (None, None));
    }

    #[test]
    fn keeps_the_books_text_sizes() {
        let ch2 = String::from(
            "<html><head><style>.title { font-size: 1.3em } .note { font-size: 80% } \
             div.small { font-size: small } h2 { font-size: 150% }</style></head><body>\
             <p class=\"title\">Title</p><p>Body</p><div class=\"small\">\
             <p class=\"note\">Note</p></div><h2>Part</h2></body></html>",
        );
        let storage = epub(ch2);
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        let mut at = Locator::new(1, 0);
        let mut found = Vec::new();
        while let Some(block) = doc.block_at(&storage, at).unwrap() {
            at = Locator::new(1, block.end);
            found.push((block.text.clone(), block.size));
        }
        let row = |t: &str, s| (String::from(t), s);
        assert_eq!(
            found,
            [
                row("Title", Some(130)),
                row("Body", None),
                // 80 % of `small` (89 %).
                row("Note", Some(71)),
                row("Part", Some(150)),
            ]
        );
    }

    #[test]
    fn ignores_stylesheets_without_publisher_styles() {
        let ch2 = String::from(
            "<html><head><style>p.b { font-weight: bold }</style></head><body>\
             <p class=\"c\">Mitte</p><p class=\"b\">Fett?</p>\
             <p style=\"font-style: italic\">Kursiv?</p><p><b>Fett</b></p></body></html>",
        );
        let storage = epub(ch2);
        // With styles first, so both caches exist side by side.
        let styled = EpubDocument::open(&storage, "books/test.epub").unwrap();
        assert_eq!(
            styled
                .block_at(&storage, Locator::new(1, 0))
                .unwrap()
                .unwrap()
                .align,
            Align::Center
        );
        let doc = EpubDocument::open_with(&storage, "books/test.epub", false).unwrap();
        let mut at = Locator::new(1, 0);
        let mut found = Vec::new();
        while let Some(block) = doc.block_at(&storage, at).unwrap() {
            at = Locator::new(1, block.end);
            found.push((block.text.clone(), block.align, block.style_at(0)));
        }
        assert_eq!(
            found,
            [
                (String::from("Mitte"), Align::Auto, TextStyle::REGULAR),
                (String::from("Fett?"), Align::Auto, TextStyle::REGULAR),
                (String::from("Kursiv?"), Align::Auto, TextStyle::REGULAR),
                // Markup stays.
                (String::from("Fett"), Align::Auto, TextStyle::BOLD),
            ]
        );
    }

    #[test]
    fn own_stylesheets_apply_after_the_books() {
        let ch2 = String::from(
            "<html><head><style>p.c { text-align: center }</style></head><body>\
             <p class=\"c\">Mitte</p></body></html>",
        );
        let storage = epub(ch2);
        use paperpaw_sdk::Storage;
        let align = |storage: &MemoryStorage| {
            let doc = EpubDocument::open(storage, "books/test.epub").unwrap();
            doc.block_at(storage, Locator::new(1, 0))
                .unwrap()
                .unwrap()
                .align
        };
        assert_eq!(align(&storage), Align::Center);
        // For all books of the folder, then this one: the last wins.
        storage
            .write("books/styles.css", b"p.c { text-align: left }")
            .unwrap();
        assert_eq!(align(&storage), Align::Start);
        storage
            .write("books/test.css", b"p.c { text-align: right }")
            .unwrap();
        assert_eq!(align(&storage), Align::End);
        // Without them, the book's own again.
        storage.remove("books/styles.css").unwrap();
        storage.remove("books/test.css").unwrap();
        assert_eq!(align(&storage), Align::Center);
    }

    #[test]
    fn plugins_stylesheets_apply_before_ones_own() {
        let ch2 = String::from(
            "<html><head><style>p.c { text-align: center }</style></head><body>\
             <p class=\"c\">Mitte</p></body></html>",
        );
        let storage = epub(ch2);
        use paperpaw_sdk::{OpenOptions, Storage};
        let align = |storage: &MemoryStorage, options: &OpenOptions| {
            let doc = EpubDocument::open_options(storage, "books/test.epub", options).unwrap();
            doc.block_at(storage, Locator::new(1, 0))
                .unwrap()
                .unwrap()
                .align
        };
        let themed = OpenOptions::default().css(String::from("p.c { text-align: right }"));
        assert_eq!(align(&storage, &themed), Align::End);
        // One's own stylesheet comes after the plugins' and wins.
        storage
            .write("books/styles.css", b"p.c { text-align: left }")
            .unwrap();
        assert_eq!(align(&storage, &themed), Align::Start);
        storage.remove("books/styles.css").unwrap();
        // Without the publisher's styles, none of them.
        let plain = themed.clone().publisher_styles(false);
        assert_eq!(align(&storage, &plain), Align::Auto);
        assert_eq!(align(&storage, &OpenOptions::default()), Align::Center);
    }

    #[test]
    fn stylesheets_found_when_prepared_serve_one_opening() {
        let ch2 = String::from(
            "<html><head><style>p.c { text-align: center }</style></head><body>\
             <p class=\"c\">Mitte</p></body></html>",
        );
        let storage = epub(ch2);
        use crate::document::PreparedCss;
        use paperpaw_sdk::{OpenOptions, Storage};
        let kept = PreparedCss::default();
        let align = |storage: &MemoryStorage, options: &OpenOptions| {
            let doc =
                EpubDocument::open_prepared(storage, "books/test.epub", options, &kept).unwrap();
            // The first chapter converted, the book opens at once.
            doc.block_at(storage, Locator::new(0, 0)).unwrap();
            doc.block_at(storage, Locator::new(1, 0))
                .unwrap()
                .unwrap()
                .align
        };
        let options = OpenOptions::default();
        storage
            .write("books/styles.css", b"p.c { text-align: left }")
            .unwrap();
        // Not converted yet: nothing is kept.
        assert!(!EpubDocument::prepared_keeping(
            &storage,
            "books/test.epub",
            &options,
            &kept
        ));
        assert_eq!(align(&storage, &options), Align::Start);
        assert!(EpubDocument::prepared_keeping(
            &storage,
            "books/test.epub",
            &options,
            &kept
        ));
        assert_eq!(align(&storage, &options), Align::Start);
        // Each opening after that looks again.
        storage.remove("books/styles.css").unwrap();
        assert_eq!(align(&storage, &options), Align::Center);
        // Kept for other options, they are not used.
        storage
            .write("books/styles.css", b"p.c { text-align: left }")
            .unwrap();
        assert!(EpubDocument::prepared_keeping(
            &storage,
            "books/test.epub",
            &options,
            &kept
        ));
        storage
            .write("books/styles.css", b"p.c { text-align: right }")
            .unwrap();
        let themed = OpenOptions::default().css(String::from("p { font-weight: bold }"));
        assert_eq!(align(&storage, &themed), Align::End);
    }

    #[test]
    fn splits_long_paragraphs_and_reuses_the_cache() {
        let words: Vec<String> = (0..3000).map(|i| alloc::format!("w{i}")).collect();
        let ch2 = alloc::format!("<html><body><p>{}</p></body></html>", words.join(" "));
        let storage = epub(ch2);
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        let ch2 = blocks(&storage, &doc, 1);
        assert!(ch2.len() > 3);
        let joined: Vec<&str> = ch2.iter().flat_map(|(_, t)| t.split_whitespace()).collect();
        assert_eq!(joined, words.iter().map(String::as_str).collect::<Vec<_>>());

        // A second open uses the cached conversion.
        let doc = EpubDocument::open(&storage, "books/test.epub").unwrap();
        assert_eq!(blocks(&storage, &doc, 1).len(), ch2.len());
    }

    #[test]
    fn an_epub_is_told_by_its_mimetype_entry_not_any_zip() {
        let epub = ZipBuilder::new()
            .stored("mimetype", "application/epub+zip")
            .deflated("META-INF/container.xml", "<container/>")
            .build();
        assert!(super::is_epub(&epub[..epub.len().min(512)]));
        // A ZIP of something else, or the mimetype compressed or later.
        let other = ZipBuilder::new().stored("page.png", "png").build();
        let packed = ZipBuilder::new()
            .deflated("mimetype", "application/epub+zip")
            .build();
        let later = ZipBuilder::new()
            .stored("a.txt", "a")
            .stored("mimetype", "application/epub+zip")
            .build();
        for zip in [other, packed, later] {
            assert!(!super::is_epub(&zip));
        }
        assert!(!super::is_epub(b"PK"));
        assert!(!super::is_epub(b""));
    }
}
