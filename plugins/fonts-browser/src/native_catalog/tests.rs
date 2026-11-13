use super::*;
use crate::fail_alloc::sweep;
use alloc::{format, string::ToString, vec};
use paperpaw_sdk::MemoryStorage;

const BASE: &str = "https://fonts.example/fonts.json";
const SHA1: &str = "0123456789abcdef0123456789abcdef01234567";

fn file(id: &str, path: &str, n: usize) -> String {
    format!(
        r#"{{"path": "{path}", "url": "/{id}/{n}", "size": {}, "sha1": "{SHA1}"}}"#,
        n + 1
    )
}

fn font(id: &str, files: usize, outline: bool) -> String {
    let list = |kind: &str| {
        (0..files)
            .map(|n| match n {
                0 => file(id, "plugin.ini", n),
                _ => file(id, &format!("fonts/{kind}{n}"), n),
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let outline = if outline {
        format!(r#", "outline": [{}]"#, list("outline"))
    } else {
        String::new()
    };
    format!(
        r#"{{"id": "{id}", "name": "Font {id}", "category": "sans", "author": "A", "licence": "OFL-1.1",
  "version": "1.0", "styles": 2, "sample": "/{id}/sample", "packs": [{}]{outline}}}"#,
        list("pack")
    )
}

fn catalog(fonts: &[String]) -> Vec<u8> {
    format!(r#"{{"format": 1, "fonts": [{}]}}"#, fonts.join(",\n")).into_bytes()
}

fn read(json: &[u8], outlines: bool) -> Vec<Font> {
    Catalog::read(BASE, outlines, json).unwrap().unwrap()
}

/// What the interface-6 reader makes of `json`, as comparable text.
fn legacy(json: &[u8], outlines: bool) -> Vec<String> {
    let storage = MemoryStorage::new().with("catalog.json", json.to_vec());
    crate::catalog::read(&storage, "catalog.json", BASE, outlines)
        .unwrap_or_default()
        .iter()
        .map(|f| format!("{}|{}", f.to_param(), f.size()))
        .collect()
}

fn ported(fonts: &[Font]) -> Vec<String> {
    fonts
        .iter()
        .map(|f| format!("{}|{}", f.to_param().unwrap(), f.size()))
        .collect()
}

#[test]
fn the_catalog_reads_as_the_interface_6_reader_does() {
    let mut fonts = vec![font("a000", MAX_FILES + 3, false)];
    fonts.extend((1..MAX_FONTS + 5).map(|n| font(&format!("a{n:03}"), 3, false)));
    let many = catalog(&fonts);
    let broken = font("c", 2, true).replace(
        r#""outline": [{"path": "plugin.ini""#,
        r#""outline": [{"path": "theme.ini""#,
    );
    let mixed = catalog(&[
        font("b", 2, false),
        font("A", 2, true),
        font("a", 2, true),
        broken,
        font("../x", 2, false),
        font("d", 0, false),
    ]);
    for json in [&many, &mixed] {
        for outlines in [false, true] {
            let got = ported(&read(json, outlines));
            assert!(!got.is_empty());
            assert_eq!(got, legacy(json, outlines), "outlines {outlines}");
        }
    }
    assert_eq!(read(&many, false).len(), MAX_FONTS);
    assert_eq!(read(&many, false)[0].files.len(), MAX_FILES);
}

#[test]
fn a_catalog_cut_short_keeps_the_font_it_ends_in() {
    let json = catalog(&[font("a", 2, false), font("b", 2, false)]);
    let cut = &json[..json.len() - 12];
    assert_eq!(ported(&read(cut, false)), legacy(cut, false));
    assert_eq!(read(cut, false).len(), 1);
}

#[test]
fn what_is_no_catalog_is_none() {
    for text in [
        &b""[..],
        b"{}",
        b"[1,2,3]",
        br#"{"format": 2, "fonts": []}"#,
        br#"{"fonts": []}"#,
        b"\x00\xff\xfe not json at all",
        &[b'['; 5000],
        &[b'{'; 70],
    ] {
        assert_eq!(
            Catalog::read(BASE, false, text).unwrap(),
            None,
            "{text:.20?}"
        );
    }
    let empty = Catalog::read(BASE, false, br#"{"format": 1, "fonts": []}"#);
    assert_eq!(empty.unwrap(), Some(Vec::new()));
}

#[test]
fn hostile_values_leave_fonts_unfit_not_broken() {
    let long = "x".repeat(10_000);
    let json = format!(
        r#"{{"format": 1, "fonts": [
 {{"id": "{long}", "name": "{long}", "version": "{long}", "packs": [{{"path": "{long}"}}]}},
 {{"id": "ok", "name": "Ok", "styles": 99999999999999999999, "packs": [
   {{"path": "plugin.ini", "url": "https://x.example/p", "size": 99999999999999999999, "sha1": "{SHA1}"}},
   {{"path": "../a", "url": "u", "sha1": "{SHA1}"}}]}},
 {{"id": "ok2", "name": "Ok2", "packs": [{{"path": "plugin.ini", "url": "a\tb", "sha1": "{SHA1}"}}]}}
]}}"#
    );
    let got = read(json.as_bytes(), false);
    // The first has no files, the second a file that leaves the folder,
    // the third an address with a control character.
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn a_font_goes_to_its_page_whole() {
    let mut font = read(&catalog(&[font("a", 3, false)]), false).remove(0);
    assert_eq!(
        Font::from_param(&font.to_param().unwrap())
            .unwrap()
            .as_ref(),
        Some(&font)
    );
    font.name = String::from("Two\tWords\n");
    let back = Font::from_param(&font.to_param().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(back.name, "Two Words ");
    assert_eq!(back.files, font.files);
    for text in [
        "",
        "\n\n",
        "../a\tA\t\t\t\t1.0\t1\t",
        "a\tA\t\t\t\t1.0\tx\t",
        "a\tA\t\t\t\t1.0\t1\t\nonly\tthree\tfields",
        "a\tA\t\t\t\t1.0\t1\t\np\tNaN\ts\tu",
        "a\tA\t\t\t\t1.0\t1\t",
    ] {
        assert_eq!(Font::from_param(text).unwrap(), None, "{text:?}");
    }
    let many = "a\tA\t\t\t\t1.0\t1\t".to_string() + &"\nplugin.ini\t1\ts\tu".repeat(100);
    assert_eq!(
        Font::from_param(&many).unwrap().unwrap().files.len(),
        MAX_FILES
    );
}

#[test]
fn a_newer_version_is_told_from_the_installed_one() {
    let font = Font {
        version: "1.2.0".to_string(),
        ..Font::default()
    };
    let older = Version::parse("1.1.0");
    assert!(font.newer(older));
    assert!(!font.newer(Version::parse("1.2.0")));
    assert!(!font.newer(None));
    assert!(!Font::default().newer(older));
}

#[test]
fn addresses_are_resolved_as_the_sdk_does() {
    let bases = [
        "https://fonts.example/fonts.json",
        "https://fonts.example/a/b/fonts.json?x=1#top",
        "http://h:8080",
        "https://h/a/b/",
        "no-scheme",
        "",
    ];
    let hrefs = [
        "",
        ".",
        "..",
        "./",
        "../",
        "a",
        "a/b",
        "../a",
        "../../../a",
        "./a/./b/../c",
        "/a",
        "/a/../..",
        "/a/b?q#f",
        "?q",
        "#f",
        "//other/x",
        "https://x/y",
        "mailto:a@b",
        "C:/x",
        "a//b",
        "/",
    ];
    for base in bases {
        for href in hrefs {
            assert_eq!(
                join_url(base, href).unwrap(),
                paperpaw_sdk::web::join_url(base, href),
                "{base:?} + {href:?}"
            );
        }
    }
}

#[test]
fn refused_allocations_end_in_no_memory_and_recover() {
    let json = catalog(&[font("a", 3, true), font("b", 2, false), font("c", 4, true)]);
    // The parser allocates fallibly too (`try_feed`/`try_finish`): the whole
    // read, parser included, is swept.
    for outlines in [false, true] {
        sweep(|| Catalog::read(BASE, outlines, &json).map(|fonts| fonts.map(|f| f.len())));
    }
    let fonts = read(&json, true);
    sweep(|| fonts[0].try_clone().map(|f| f.files.len()));
    sweep(|| fonts[0].plugin().map(|p| p.len()));
    let param = fonts[0].to_param().unwrap();
    sweep(|| fonts[0].to_param().map(|p| p.len()));
    sweep(|| Font::from_param(&param).map(|f| f.map(|f| f.files.len())));
    for (base, href) in [
        ("https://h/a/b", "../c?d#e"),
        ("https://h", "//x/y"),
        ("x", "y"),
    ] {
        sweep(|| join_url(base, href).map(|u| u.len()));
    }
}

#[test]
fn fonts_over_the_caps_are_not_offered() {
    use crate::native_plan::{MAX_OUTLINE, MAX_PACK};
    let with_size = |id: &str, size: u64| {
        font(id, 2, true).replace("\"size\": 2,", &format!("\"size\": {size},"))
    };
    let json = catalog(&[
        font("ok", 2, true),
        with_size("packbig", MAX_PACK + 1),
        with_size("zero", 0),
        with_size("outbig", MAX_OUTLINE + 1),
    ]);
    // The size is replaced in the packs and in the font files alike: a font
    // over the packs' cap is still fine as font files.
    let ids =
        |outlines| -> Vec<String> { read(&json, outlines).into_iter().map(|f| f.id).collect() };
    assert_eq!(ids(false), ["ok"]);
    assert_eq!(ids(true), ["ok", "packbig"]);
    // A font file may be larger than a pack.
    let json = catalog(&[font("big", 2, true).replacen(
        "\"outline\": [{\"path\": \"plugin.ini\", \"url\": \"/big/0\", \"size\": 1,",
        "\"outline\": [{\"path\": \"plugin.ini\", \"url\": \"/big/0\", \"size\": 900000,",
        1,
    )]);
    assert_eq!(read(&json, true).len(), 1);
    assert_eq!(read(&json, false).len(), 1);
}

#[test]
fn the_librarys_packs_over_half_a_mebibyte_are_offered_as_on_interface_6() {
    // The sizes of the largest packs of paperpaw.org's fonts (EB Garamond,
    // Nunito, Atkinson Hyperlegible Next's bold italic).
    for size in [558_198, 624_799, 666_854] {
        let json =
            catalog(&[font("big", 2, true).replace("\"size\": 2,", &format!("\"size\": {size},"))]);
        assert_eq!(read(&json, false).len(), 1, "{size}");
        assert_eq!(read(&json, true).len(), 1, "{size}");
    }
}

#[test]
fn a_catalog_in_a_file_is_read_as_the_text_is() {
    let fonts: Vec<String> = (0..40).map(|n| font(&format!("f{n}"), 2, true)).collect();
    let json = catalog(&fonts);
    assert!(json.len() > 4096, "more than one piece");
    let storage = MemoryStorage::new().with("catalog.json", json.clone());
    for outlines in [false, true] {
        let from_file = Catalog::read_file(BASE, outlines, &storage, "catalog.json")
            .unwrap()
            .unwrap();
        assert_eq!(ported(&from_file), ported(&read(&json, outlines)));
    }
    // No file is no catalog; a file that is no catalog is none either.
    assert_eq!(
        Catalog::read_file(BASE, false, &storage, "none.json").unwrap(),
        None
    );
    let storage = MemoryStorage::new().with("catalog.json", "not json");
    assert_eq!(
        Catalog::read_file(BASE, false, &storage, "catalog.json").unwrap(),
        None
    );
}

#[test]
fn reading_the_catalog_file_may_be_refused_at_every_allocation() {
    let json = catalog(&[font("a", 2, false), font("b", 2, false)]);
    let storage = MemoryStorage::new().with("catalog.json", json);
    sweep(|| Catalog::read_file(BASE, false, &storage, "catalog.json").map(|f| f.map(|f| f.len())));
}
