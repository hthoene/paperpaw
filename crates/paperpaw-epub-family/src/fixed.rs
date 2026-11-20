//! Fixed-layout EPUBs (picture books, comics): a book whose package says
//! `rendition:layout` is `pre-paginated` (for the whole book, or for the
//! spine items that ask for it) has one fixed page per spine item. A page
//! that is one picture (an `<img>` or an SVG `<image>`, no text of
//! account) is shown whole, as a page of a comic book archive is
//! ([`CbzDocument`]): the spine's order is the pages' order, `page-progression-direction`
//! their way. A book of pre-paginated pages with reading matter on them is
//! not opened here: the reflowing conversion reads it, a page a section.
//!
//! The list of pages (`pages.bin`, as a comic's) is made once by reading each
//! page, and kept; so is the answer that a book is not one of pictures
//! (`reflow`).

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::digest::fnv1a64;
use paperpaw_sdk::document::{DocumentError, Metadata, PageProgression};
use paperpaw_sdk::xml::{Attrs, Token, local_name, try_decode};
use paperpaw_sdk::zip::ZipArchive;

use crate::cbz::CbzDocument;
use crate::fallible;
use crate::package::{dir_of, error, package_path, tokenize, try_resolve};
use crate::{PLUGIN, cached};

/// Pages kept at most: the first in reading order.
const MAX_PAGES: usize = 2000;
/// Characters of text (not counting spaces) from which a page is reading
/// matter, not a picture with a caption.
const TEXT_PAGE: usize = 120;
/// Most of a metadata field kept.
const MAX_FIELD: usize = 512;
/// Authors kept at most.
const MAX_AUTHORS: usize = 8;
/// The list's first word when the book runs left to right and right to left
/// (a comic's list names its `ComicInfo.xml` there: neither is a record).
const LEFT_TO_RIGHT: u32 = u32::MAX;
const RIGHT_TO_LEFT: u32 = u32::MAX - 1;

/// An item of the spine asks for its own layout (its `properties`).
const PAGINATED: u8 = 1;
const REFLOWABLE: u8 = 2;

/// What the package document says.
#[derive(Default)]
struct Opf {
    metadata: Metadata,
    fixed: bool,
    rtl: bool,
    /// The manifest's pages' ids (hashed) with their archive records, sorted.
    items: Vec<(u64, u32)>,
    /// The spine: an id (hashed) and what its itemref asks for.
    spine: Vec<(u64, u8)>,
}

/// Which text of the package is being read.
#[derive(Clone, Copy, PartialEq)]
enum Field {
    None,
    Title,
    Author,
    Language,
    Layout,
}

/// A manifest item that is a page (the hash of its id and its record in
/// the archive) onto `items`.
fn manifest_item(
    storage: &dyn Storage,
    archive: &ZipArchive,
    base: &str,
    attrs: &Attrs<'_>,
    items: &mut Vec<(u64, u32)>,
) -> Result<(), DocumentError> {
    let get = |n: &str| attrs.try_get(n).map_err(|_| fallible::OOM);
    let media = get("media-type")?.unwrap_or_default();
    if !matches!(
        &*media,
        "application/xhtml+xml" | "text/html" | "image/svg+xml"
    ) {
        return Ok(());
    }
    if let Some(href) = get("href")?
        && let Some(id) = get("id")?
    {
        let (file, _) = try_resolve(base, &href)?;
        match archive.entry(storage, &file) {
            Ok(Some(entry)) => {
                fallible::push(items, (fnv1a64(id.as_bytes()), entry.record()))?;
            }
            Ok(None) => {}
            Err(e) => return Err(error(e)),
        }
    }
    Ok(())
}

/// A spine item: the hash of its id and the layout it asks for.
fn spine_item(attrs: &Attrs<'_>) -> Result<(u64, u8), DocumentError> {
    let get = |n: &str| attrs.try_get(n).map_err(|_| fallible::OOM);
    let id = fnv1a64(get("idref")?.unwrap_or_default().as_bytes());
    let mut asks = 0;
    for property in get("properties")?.unwrap_or_default().split_whitespace() {
        match property {
            "rendition:layout-pre-paginated" => asks = PAGINATED,
            "rendition:layout-reflowable" => asks = REFLOWABLE,
            _ => {}
        }
    }
    Ok((id, asks))
}

fn read_opf(storage: &dyn Storage, archive: &ZipArchive, path: &str) -> Result<Opf, DocumentError> {
    let base = fallible::string(dir_of(path))?;
    let mut opf = Opf::default();
    let (mut field, mut text) = (Field::None, String::new());
    let mut failed = None;
    tokenize(storage, archive, path, &mut |token| {
        if failed.is_some() {
            return;
        }
        let result = (|| -> Result<(), DocumentError> {
            match token {
                Token::Start { name, attrs, .. } => {
                    let get = |n: &str| attrs.try_get(n).map_err(|_| fallible::OOM);
                    text.clear();
                    field = Field::None;
                    match local_name(name) {
                        "title" if opf.metadata.title.is_none() => field = Field::Title,
                        "creator" if opf.metadata.authors.len() < MAX_AUTHORS => {
                            field = Field::Author;
                        }
                        "language" if opf.metadata.language.is_none() => field = Field::Language,
                        "meta" => {
                            let property = get("property")?;
                            if property.as_deref() == Some("rendition:layout")
                                && get("refines")?.is_none()
                            {
                                field = Field::Layout;
                            } else if get("name")?.as_deref() == Some("fixed-layout")
                                && get("content")?.as_deref() == Some("true")
                            {
                                opf.fixed = true;
                            }
                        }
                        "spine" => {
                            opf.rtl = get("page-progression-direction")?.as_deref() == Some("rtl");
                        }
                        "item" if opf.items.len() < 2 * MAX_PAGES => {
                            manifest_item(storage, archive, &base, &attrs, &mut opf.items)?;
                        }
                        "itemref" if opf.spine.len() < 2 * MAX_PAGES => {
                            let item = spine_item(&attrs)?;
                            fallible::push(&mut opf.spine, item)?;
                        }
                        _ => {}
                    }
                }
                Token::Text(t) if field != Field::None => {
                    if text.len() < MAX_FIELD {
                        let t = try_decode(&t).map_err(|_| fallible::OOM)?;
                        fallible::push_str(&mut text, t.trim())?;
                    }
                }
                Token::End { name } if field != Field::None => {
                    let value = core::mem::take(&mut text);
                    let value = value.trim();
                    if !value.is_empty() && local_name(name) != "meta" || field == Field::Layout {
                        match field {
                            Field::Title => opf.metadata.title = Some(fallible::string(value)?),
                            Field::Author => {
                                fallible::push(&mut opf.metadata.authors, fallible::string(value)?)?
                            }
                            Field::Language => {
                                opf.metadata.language = Some(fallible::string(value)?);
                            }
                            Field::Layout => opf.fixed |= value == "pre-paginated",
                            Field::None => {}
                        }
                    }
                    field = Field::None;
                }
                _ => {}
            }
            Ok(())
        })();
        if let Err(e) = result {
            failed = Some(e);
        }
    })?;
    if let Some(e) = failed {
        return Err(e);
    }
    opf.items.sort_unstable();
    Ok(opf)
}

/// What a page holds: its first picture (a path in the archive) and how much
/// text it has.
struct Page {
    picture: Option<String>,
    text: usize,
}

/// Reads the spine item `record`: the first picture it shows and its text.
fn read_page(
    storage: &dyn Storage,
    archive: &ZipArchive,
    record: u32,
) -> Result<Page, DocumentError> {
    let entry = archive.entry_by_record(storage, record).map_err(error)?;
    let base = fallible::string(dir_of(&entry.name))?;
    let mut page = Page {
        picture: None,
        text: 0,
    };
    // Text of `<title>`, `<style>` and `<script>` is not on the page.
    let mut hidden = 0u32;
    let mut failed = None;
    let mut tokenizer = paperpaw_sdk::xml::Tokenizer::new();
    let mut out_of_memory = false;
    let mut handle = |token: Token<'_>| match token {
        Token::Start {
            name,
            attrs,
            self_closing,
        } => {
            let name = local_name(name);
            if matches!(name, "title" | "style" | "script") && !self_closing {
                hidden += 1;
            }
            if page.picture.is_none() && matches!(name, "img" | "image") {
                let href = attrs
                    .iter()
                    .find(|(n, _)| matches!(local_name(n), "src" | "href"))
                    .map(|(_, value)| value);
                if let Some(href) = href.filter(|h| !h.starts_with("data:")) {
                    let found = try_decode(href)
                        .map_err(|_| fallible::OOM)
                        .and_then(|href| try_resolve(&base, &href))
                        .map(|(path, _)| path);
                    match found {
                        Ok(path) => page.picture = Some(path),
                        Err(e) => failed = Some(e),
                    }
                }
            }
        }
        Token::End { name } => {
            if matches!(local_name(name), "title" | "style" | "script") {
                hidden = hidden.saturating_sub(1);
            }
        }
        Token::Text(t) if hidden == 0 => {
            page.text += t.chars().filter(|c| !c.is_whitespace()).count();
        }
        Token::CData(t) if hidden == 0 => {
            page.text += t.chars().filter(|c| !c.is_whitespace()).count();
        }
        _ => {}
    };
    archive
        .stream(storage, &entry, &mut |chunk| {
            out_of_memory = tokenizer.try_feed(chunk, &mut handle).is_err();
            !out_of_memory
        })
        .map_err(error)?;
    if out_of_memory {
        return Err(DocumentError::OutOfMemory);
    }
    tokenizer.finish(&mut handle);
    match failed {
        Some(e) => Err(e),
        None => Ok(page),
    }
}

/// Whether `path` names a picture PaperPaw decodes.
fn is_picture(path: &str) -> bool {
    let (_, extension) = path.rsplit_once('.').unwrap_or(("", ""));
    ["jpg", "jpeg", "png", "gif"]
        .iter()
        .any(|e| extension.eq_ignore_ascii_case(e))
}

/// Makes the list of pages of `opf` in `list` (the first word the way the book
/// runs, then each page's picture, as a record): how many pages, 0 for a book
/// that is not one of pictures.
fn list_pages(
    storage: &dyn Storage,
    archive: &ZipArchive,
    opf: &Opf,
    list: &str,
) -> Result<u32, DocumentError> {
    let mut records: Vec<u32> = Vec::new();
    let mut others = 0usize;
    for &(id, asks) in opf.spine.iter().take(2 * MAX_PAGES) {
        let at = opf.items.partition_point(|&(h, _)| h < id);
        let Some(&(_, record)) = opf.items.get(at).filter(|&&(h, _)| h == id) else {
            continue;
        };
        let paginated = asks == PAGINATED || (opf.fixed && asks != REFLOWABLE);
        let page = if paginated {
            read_page(storage, archive, record)?
        } else {
            Page {
                picture: None,
                text: TEXT_PAGE,
            }
        };
        let picture = page
            .picture
            .filter(|p| page.text < TEXT_PAGE && is_picture(p))
            .and_then(|p| archive.entry(storage, &p).ok().flatten());
        match picture {
            Some(entry) if records.len() < MAX_PAGES => {
                fallible::push(&mut records, entry.record())?
            }
            Some(_) => {}
            None => others += 1,
        }
    }
    // A book that is mostly text is read as text.
    if records.is_empty() || others * 4 > records.len() + others {
        return Ok(0);
    }
    let mut data = Vec::new();
    fallible::reserve_exact(&mut data, 4 * (1 + records.len()))?;
    let way = if opf.rtl {
        RIGHT_TO_LEFT
    } else {
        LEFT_TO_RIGHT
    };
    data.extend_from_slice(&way.to_le_bytes());
    for record in &records {
        data.extend_from_slice(&record.to_le_bytes());
    }
    storage.write(list, &data)?;
    Ok(records.len() as u32)
}

/// The book at `path` as a document of whole pages, if it is a fixed-layout
/// book of pictures; `None` for any other, which the reflowing conversion
/// reads.
pub(crate) fn open(
    storage: &dyn Storage,
    path: &str,
    options: &paperpaw_sdk::OpenOptions,
) -> Result<Option<CbzDocument>, DocumentError> {
    let archive = ZipArchive::open(storage, path).map_err(error)?;
    let package = package_path(storage, &archive)?;
    let cache = fallible::kept_path(&fallible::plugin_dir(PLUGIN.as_str())?, "fxd-", path)?;
    let stamp = fallible::try_format!("{}\n", fallible::fingerprint(storage, path)?)?;
    let stamp_path = fallible::try_format!("{cache}/source")?;
    let list = fallible::try_format!("{cache}/pages.bin")?;
    let fresh = paperpaw_sdk::read_file(storage, &stamp_path, 64)
        .ok()
        .as_deref()
        == Some(stamp.as_bytes());
    let opf = if fresh && storage.size(&list).is_ok_and(|size| size >= 8) {
        None
    } else if fresh
        && storage
            .size(&fallible::try_format!("{cache}/reflow")?)
            .is_ok()
    {
        return Ok(None);
    } else {
        let opf = read_opf(storage, &archive, &package)?;
        if !opf.fixed && !opf.spine.iter().any(|&(_, asks)| asks == PAGINATED) {
            // Most books: nothing is kept for them.
            return Ok(None);
        }
        let _ = storage.remove_dir_all(&cache);
        let _ = storage.create_dir(&cache);
        if list_pages(storage, &archive, &opf, &list)? == 0 {
            storage.write(&fallible::try_format!("{cache}/reflow")?, b"")?;
            storage.write(&stamp_path, stamp.as_bytes())?;
            return Ok(None);
        }
        storage.write(&stamp_path, stamp.as_bytes())?;
        Some(opf)
    };
    let size = storage.size(&list)?;
    let pages = (size / 4 - 1) as u32;
    let rtl = cached::u32_at(storage, &list, 0).ok() == Some(RIGHT_TO_LEFT);
    let mut metadata = match opf {
        Some(opf) => opf.metadata,
        // Kept apart from the pages: read again from the package.
        None => read_opf(storage, &archive, &package)?.metadata,
    };
    metadata.direction = if rtl {
        PageProgression::RightToLeft
    } else {
        PageProgression::LeftToRight
    };
    if metadata.title.is_none() {
        let name = path.rsplit('/').next().unwrap_or(path);
        metadata.title = Some(fallible::string(
            name.rsplit_once('.').map_or(name, |(s, _)| s),
        )?);
    }
    Ok(Some(CbzDocument::from_pages(
        archive, pages, cache, metadata, options, rtl,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;
    use paperpaw_sdk::document::{BlockKind, Document, Locator};
    use paperpaw_zip::testing::ZipBuilder;

    const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;

    fn opf(layout: &str, ppd: &str, refs: &str) -> String {
        alloc::format!(
            r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
 <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
  <dc:title>Picture Book</dc:title><dc:creator>Ann</dc:creator><dc:language>en</dc:language>
  {layout}
 </metadata>
 <manifest>
  <item id="p1" href="p1.xhtml" media-type="application/xhtml+xml"/>
  <item id="p2" href="p2.xhtml" media-type="application/xhtml+xml"/>
  <item id="p3" href="p3.xhtml" media-type="application/xhtml+xml"/>
 </manifest>
 <spine {ppd}>{refs}</spine>
</package>"#
        )
    }

    const REFS: &str = r#"<itemref idref="p1"/><itemref idref="p2"/><itemref idref="p3"/>"#;

    fn page(body: &str) -> String {
        alloc::format!(
            r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:xlink="http://www.w3.org/1999/xlink"><head><title>A page</title></head><body>{body}</body></html>"#
        )
    }

    fn book(layout: &str, ppd: &str, bodies: [&str; 3]) -> MemoryStorage {
        let mut zip = ZipBuilder::new()
            .stored("mimetype", b"application/epub+zip".to_vec())
            .deflated("META-INF/container.xml", CONTAINER.as_bytes())
            .deflated("OEBPS/content.opf", opf(layout, ppd, REFS).as_bytes());
        for (n, body) in bodies.iter().enumerate() {
            zip = zip.deflated(
                &alloc::format!("OEBPS/p{}.xhtml", n + 1),
                page(body).as_bytes(),
            );
        }
        let zip = zip
            .stored("OEBPS/img/a.jpg", b"not a picture".to_vec())
            .stored("OEBPS/img/b.png", b"not one either".to_vec())
            .stored("OEBPS/img/c.gif", b"nor this".to_vec())
            .build();
        MemoryStorage::new().with("b/pic.epub", zip)
    }

    const LAYOUT: &str = r#"<meta property="rendition:layout">pre-paginated</meta>"#;
    const PICTURES: [&str; 3] = [
        r#"<img src="img/a.jpg" alt="one"/>"#,
        r#"<svg><image xlink:href="img/b.png" width="10" height="10"/></svg>"#,
        r#"<div><img src="img/c.gif"/></div>"#,
    ];

    fn names(doc: &CbzDocument, storage: &MemoryStorage) -> Vec<String> {
        (0..doc.sections())
            .map(|s| {
                let block = doc.block_at(storage, Locator::new(s, 0)).unwrap().unwrap();
                // Pictures that cannot be decoded are named.
                assert_eq!(block.kind, BlockKind::Paragraph);
                block.text
            })
            .collect()
    }

    #[test]
    fn pictures_of_a_pre_paginated_book_are_its_pages() {
        let storage = book(LAYOUT, r#"page-progression-direction="rtl""#, PICTURES);
        let doc = open(&storage, "b/pic.epub", &Default::default())
            .unwrap()
            .expect("a fixed-layout book of pictures");
        assert!(doc.whole_pages());
        assert_eq!(doc.sections(), 3);
        assert_eq!(
            names(&doc, &storage),
            ["OEBPS/img/a.jpg", "OEBPS/img/b.png", "OEBPS/img/c.gif"]
        );
        let m = doc.metadata();
        assert_eq!(m.title.as_deref(), Some("Picture Book"));
        assert_eq!(m.authors, ["Ann"]);
        assert_eq!(m.direction, PageProgression::RightToLeft);
        // The second open reads the kept list.
        let again = open(&storage, "b/pic.epub", &Default::default())
            .unwrap()
            .unwrap();
        assert_eq!(again.sections(), 3);
        assert_eq!(again.metadata().direction, PageProgression::RightToLeft);
    }

    #[test]
    fn a_book_without_a_layout_is_not_fixed() {
        let storage = book("", "", PICTURES);
        // No layout at all: not a fixed-layout book.
        assert!(
            open(&storage, "b/pic.epub", &Default::default())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_pre_paginated_book_with_text_is_read_as_text() {
        let text = "<p>Once upon a time there was a long story that goes on and on and on, \
            so long that the page is a page of reading matter and not a picture with a caption.</p>";
        let storage = book(LAYOUT, "", [text, text, PICTURES[0]]);
        assert!(
            open(&storage, "b/pic.epub", &Default::default())
                .unwrap()
                .is_none()
        );
        // The answer is kept.
        assert!(
            open(&storage, "b/pic.epub", &Default::default())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_caption_does_not_make_a_page_text() {
        let captioned = r#"<img src="img/a.jpg"/><p>Page one</p>"#;
        let storage = book(LAYOUT, "", [captioned, PICTURES[1], PICTURES[2]]);
        let doc = open(&storage, "b/pic.epub", &Default::default())
            .unwrap()
            .unwrap();
        assert_eq!(doc.sections(), 3);
    }
}
