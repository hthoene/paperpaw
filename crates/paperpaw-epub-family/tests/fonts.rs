//! Embedded fonts from hostile books: obfuscated with the wrong key or an
//! unknown method, WOFF files with damaged tables, fonts too large or
//! missing, more families than a style can name.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{container, opf, xhtml, zip};
use paperpaw_plugin_format_epub::EpubDocument;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::document::{Document, DocumentError, Locator, MAX_FAMILIES};

const TTF: &[u8] = include_bytes!("data/atkinson-subset.ttf");
const WOFF: &[u8] = include_bytes!("data/atkinson-subset.woff");
const IDPF: &str = "http://www.idpf.org/2008/embedding";
const ADOBE: &str = "http://ns.adobe.com/pdf/enc#RC";

/// A book whose stylesheet embeds `fonts` (file name, data) as families
/// `F0`, `F1`, …, obfuscated as `encryption` says (method, file name),
/// with the unique identifier `identifier`.
fn font_book(
    identifier: &str,
    fonts: &[(&str, Vec<u8>)],
    encryption: &[(&str, &str)],
) -> (MemoryStorage, EpubDocument) {
    let mut css = String::new();
    for (i, (name, _)) in fonts.iter().enumerate() {
        css.push_str(&format!(
            "@font-face {{ font-family: F{i}; src: url(fonts/{name}) }} .f{i} {{ font-family: F{i} }}\n"
        ));
    }
    let body: String = (0..fonts.len())
        .map(|i| format!("<p class=\"f{i}\">Font {i}</p>"))
        .collect();
    let package = opf(
        &format!("<dc:identifier id=\"uid\">{identifier}</dc:identifier>"),
        &[
            ("c", "c.xhtml", "application/xhtml+xml", ""),
            ("css", "style.css", "text/css", ""),
        ],
        &["c"],
    );
    let mut xml = String::from("<encryption>");
    for (method, name) in encryption {
        xml.push_str(&format!(
            "<EncryptedData><EncryptionMethod Algorithm=\"{method}\"/><CipherData>\
             <CipherReference URI=\"OEBPS/fonts/{name}\"/></CipherData></EncryptedData>"
        ));
    }
    xml.push_str("</encryption>");
    let mut files = vec![
        (
            "META-INF/container.xml".to_string(),
            container("OEBPS/content.opf").into_bytes(),
        ),
        ("META-INF/encryption.xml".to_string(), xml.into_bytes()),
        ("OEBPS/content.opf".to_string(), package.into_bytes()),
        ("OEBPS/c.xhtml".to_string(), xhtml(&body).into_bytes()),
        ("OEBPS/style.css".to_string(), css.into_bytes()),
    ];
    for (name, data) in fonts {
        files.push((format!("OEBPS/fonts/{name}"), data.clone()));
    }
    let files: Vec<(&str, Vec<u8>)> = files.iter().map(|(n, d)| (n.as_str(), d.clone())).collect();
    let storage = MemoryStorage::new().with("book.epub", zip(&files, false));
    let doc = EpubDocument::open(&storage, "book.epub").unwrap();
    (storage, doc)
}

/// `data` obfuscated as IDPF says, with key `identifier`.
fn idpf(data: &[u8], identifier: &str) -> Vec<u8> {
    let id: String = identifier.chars().filter(|c| !c.is_whitespace()).collect();
    let mut sha = paperpaw_sdk::digest::sha1::Sha1::new();
    sha.update(id.as_bytes());
    let key = sha.finish();
    data.iter()
        .enumerate()
        .map(|(i, b)| if i < 1040 { b ^ key[i % key.len()] } else { *b })
        .collect()
}

#[test]
fn obfuscated_fonts_are_revealed_with_the_books_identifier() {
    let id = "urn:uuid:12345678-9abc-def0-1234-56789abcdef0";
    let (storage, doc) = font_book(id, &[("a.ttf", idpf(TTF, id))], &[(IDPF, "a.ttf")]);
    assert_eq!(doc.font_data(&storage, 0).unwrap(), TTF);
}

#[test]
fn fonts_obfuscated_with_another_key_stay_scrambled_only_at_their_start() {
    let (storage, doc) = font_book(
        "urn:isbn:111",
        &[("a.ttf", idpf(TTF, "urn:isbn:222"))],
        &[(IDPF, "a.ttf")],
    );
    let data = doc.font_data(&storage, 0).unwrap();
    assert_eq!(data.len(), TTF.len());
    assert_ne!(&data[..1040], &TTF[..1040]);
    assert_eq!(&data[1040..], &TTF[1040..]);
}

#[test]
fn adobe_obfuscation_without_a_uuid_leaves_the_font_as_stored() {
    let stored = idpf(TTF, "something");
    let (storage, doc) = font_book(
        "not-a-uuid",
        &[("a.ttf", stored.clone())],
        &[(ADOBE, "a.ttf")],
    );
    assert_eq!(doc.font_data(&storage, 0).unwrap(), stored);
}

#[test]
fn unknown_encryption_methods_leave_the_font_as_stored() {
    let (storage, doc) = font_book(
        "urn:x",
        &[("a.ttf", TTF.to_vec())],
        &[("http://www.w3.org/2001/04/xmlenc#aes256-cbc", "a.ttf")],
    );
    assert_eq!(doc.font_data(&storage, 0).unwrap(), TTF);
}

#[test]
fn obfuscated_woff_fonts_are_revealed_then_unpacked() {
    let id = "urn:isbn:9780000000000";
    let (storage, doc) = font_book(id, &[("w.woff", idpf(WOFF, id))], &[(IDPF, "w.woff")]);
    let font = doc.font_data(&storage, 0).unwrap();
    assert_eq!(&font[..4], &TTF[..4]);
    // With the wrong key the WOFF is not recognised: the bytes as they are.
    let (storage, doc) = font_book(
        "urn:other",
        &[("w.woff", idpf(WOFF, id))],
        &[(IDPF, "w.woff")],
    );
    let font = doc.font_data(&storage, 0).unwrap();
    assert_ne!(&font[..4], b"wOFF");
    assert_ne!(&font[..4], &TTF[..4]);
}

/// The WOFF with `change` applied to its bytes.
fn damaged_woff(change: impl Fn(&mut Vec<u8>)) -> Vec<u8> {
    let mut woff = WOFF.to_vec();
    change(&mut woff);
    woff
}

fn put_u32(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

#[test]
fn damaged_woff_fonts_are_refused() {
    let tables = usize::from(u16::from_be_bytes([WOFF[12], WOFF[13]]));
    assert!(tables > 2);
    let entry = |n: usize| 44 + n * 20;
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "no tables",
            damaged_woff(|w| w[12..14].copy_from_slice(&[0, 0])),
        ),
        (
            "more tables than entries",
            damaged_woff(|w| w[12..14].copy_from_slice(&[0xff, 0xff])),
        ),
        ("font too large", damaged_woff(|w| put_u32(w, 16, u32::MAX))),
        (
            "table past the end",
            damaged_woff(|w| put_u32(w, entry(1) + 4, u32::MAX - 2)),
        ),
        (
            "table length past the end",
            damaged_woff(|w| put_u32(w, entry(1) + 8, u32::MAX)),
        ),
        (
            "stored longer than the table",
            damaged_woff(|w| {
                let length =
                    u32::from_be_bytes(w[entry(0) + 12..entry(0) + 16].try_into().unwrap());
                put_u32(w, entry(0) + 8, length + 1);
            }),
        ),
        (
            "table larger than unpacked",
            damaged_woff(|w| {
                let length =
                    u32::from_be_bytes(w[entry(2) + 12..entry(2) + 16].try_into().unwrap());
                put_u32(w, entry(2) + 12, length * 50);
            }),
        ),
        (
            "damaged compressed data",
            damaged_woff(|w| {
                let field = |w: &Vec<u8>, at: usize| {
                    u32::from_be_bytes(w[at..at + 4].try_into().unwrap()) as usize
                };
                // The first table stored compressed (smaller than it is).
                let n = (0..tables)
                    .find(|&n| field(w, entry(n) + 8) < field(w, entry(n) + 12))
                    .unwrap();
                let offset = field(w, entry(n) + 4);
                for b in &mut w[offset..offset + 8] {
                    *b ^= 0x5a;
                }
            }),
        ),
        ("cut short", WOFF[..WOFF.len() / 2].to_vec()),
        ("header only", WOFF[..44].to_vec()),
    ];
    for (what, woff) in cases {
        let (storage, doc) = font_book("urn:x", &[("w.woff", woff)], &[]);
        match doc.font_data(&storage, 0) {
            Err(DocumentError::Malformed(_)) => {}
            other => panic!("{what}: {:?}", other.map(|f| f.len())),
        }
    }
}

#[test]
fn fonts_too_large_are_refused_before_reading() {
    let huge = vec![0u8; 16 * 1024 * 1024 + 1];
    let (storage, doc) = font_book("urn:x", &[("big.ttf", huge)], &[]);
    assert_eq!(doc.font_data(&storage, 0), Err(DocumentError::OutOfMemory));
}

#[test]
fn missing_fonts_and_indexes_past_the_list_are_errors() {
    let (storage, doc) = font_book("urn:x", &[("a.ttf", TTF.to_vec())], &[]);
    assert!(doc.font_data(&storage, 1).is_err());
    assert!(doc.font_data(&storage, usize::MAX).is_err());
    // A face whose file is not in the book.
    let package = opf(
        "",
        &[
            ("c", "c.xhtml", "application/xhtml+xml", ""),
            ("css", "s.css", "text/css", ""),
        ],
        &["c"],
    );
    let epub = zip(
        &[
            ("META-INF/container.xml", container("content.opf").into_bytes()),
            ("content.opf", package.into_bytes()),
            ("c.xhtml", xhtml("<p class=\"g\">Ghost</p>").into_bytes()),
            (
                "s.css",
                b"@font-face { font-family: Ghost; src: url(../../ghost.ttf) } .g { font-family: Ghost }"
                    .to_vec(),
            ),
        ],
        false,
    );
    let storage = MemoryStorage::new().with("b.epub", epub);
    let doc = EpubDocument::open(&storage, "b.epub").unwrap();
    assert_eq!(doc.fonts(&storage).len(), 1);
    assert!(matches!(
        doc.font_data(&storage, 0),
        Err(DocumentError::Malformed(_))
    ));
}

#[test]
fn more_families_than_styles_can_name_fall_back_to_the_readers_font() {
    let count = usize::from(MAX_FAMILIES) + 9;
    let fonts: Vec<(String, Vec<u8>)> = (0..count)
        .map(|i| (format!("f{i}.ttf"), TTF.to_vec()))
        .collect();
    let fonts: Vec<(&str, Vec<u8>)> = fonts.iter().map(|(n, d)| (n.as_str(), d.clone())).collect();
    let (storage, doc) = font_book("urn:x", &fonts, &[]);
    let listed = doc.fonts(&storage);
    assert_eq!(listed.len(), usize::from(MAX_FAMILIES));
    assert!(
        listed
            .iter()
            .all(|f| f.family >= 1 && f.family <= MAX_FAMILIES)
    );
    // The paragraphs of families past the last have the reader's font.
    let mut at = Locator::new(0, 0);
    let mut families = Vec::new();
    while let Some(block) = doc.block_at(&storage, at).unwrap() {
        families.push(block.style_at(0).family());
        at = Locator::new(0, block.end);
    }
    assert_eq!(families.len(), count);
    assert!(families[..usize::from(MAX_FAMILIES)].iter().all(|&f| f > 0));
    assert!(
        families[usize::from(MAX_FAMILIES)..]
            .iter()
            .all(|&f| f == 0)
    );
}
