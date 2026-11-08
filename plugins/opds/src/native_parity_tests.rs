//! The native feed reader reads what the interface-6 reader read, the same
//! way: the same feeds, hostile pieces of feeds, the same links joined, the
//! same file types. And the native settings are the interface-6 settings,
//! field by field.
#![allow(clippy::unwrap_used, reason = "fixtures")]
extern crate std;

use crate::native_model::{self as native, Source};
use alloc::{format, string::String, vec, vec::Vec};
use paperpaw_sdk::{MemoryStorage, Storage, native::Error};
use proptest::prelude::*;

const BASE: &str = "https://cat.example/opds/root.xml";

/// The bytes as a source, in the pieces a file is read in.
struct Bytes<'a>(&'a [u8]);
impl Source for Bytes<'_> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        let from = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.0.len());
        let n = buf.len().min(self.0.len() - from);
        buf[..n].copy_from_slice(&self.0[from..from + n]);
        Ok(n)
    }
}

/// Both readers' feeds as text: the types have the same names and fields.
fn both(data: &[u8], base: &str) -> (String, String) {
    let storage = MemoryStorage::new();
    storage.write("f.xml", data).unwrap();
    let old = format!("{:?}", crate::feed::read(&storage, "f.xml", base));
    let new = format!("{:?}", native::read(&mut Bytes(data), base).unwrap());
    (old, new)
}

#[test]
fn feeds_are_read_as_before() {
    let feeds = [
        r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Cat &amp; Co</title>
<link rel="next" type="application/atom+xml" href="?page=2"/>
<link rel="search" href="/s?q={searchTerms}"/>
<entry><title>  Alice
 in   Wonderland </title><id>urn:1</id><author><name>Carroll, Lewis</name></author><author><name>Tenniel</name></author>
<summary type="text">A girl falls.</summary>
<link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="/a.epub" title="EPUB  (no images)"/>
<link rel="http://opds-spec.org/acquisition" type="application/x-unknown" href="/b.MOBI"/>
<link rel="http://opds-spec.org/image/thumbnail" href="t.png"/>
<link rel="http://opds-spec.org/image" href="c.png"/></entry>
<entry><title>Shelf</title><link rel="subsection" type="application/atom+xml" href="../shelf.xml"/></entry>
<entry><title>Nothing</title></entry></feed>"#,
        "",
        "<feed></feed>",
        "not xml at all",
        "<feed><entry><title>x</title><link rel=\"http://opds-spec.org/acquisition\" href=\"data:abc\"/></entry></feed>",
        "<opensearch><link rel=\"search\" type=\"application/opensearchdescription+xml\" href=\"/os.xml\"/></opensearch>",
    ];
    for feed in feeds {
        let (old, new) = both(feed.as_bytes(), BASE);
        assert_eq!(new, old, "{feed}");
    }
    // The size limits cut the same way.
    let mut many = String::from("<feed><title>Many</title>");
    for n in 0..150 {
        many.push_str(&format!("<entry><title>{n} {}</title><link rel=\"http://opds-spec.org/acquisition\" type=\"application/epub+zip\" href=\"/{n}.epub\"/></entry>", "word ".repeat(200)));
    }
    many.push_str("</feed>");
    let (old, new) = both(many.as_bytes(), BASE);
    assert_eq!(new, old);
    assert_eq!(
        native::read(&mut Bytes(many.as_bytes()), BASE)
            .unwrap()
            .entries
            .len(),
        native::MAX_ENTRIES
    );
}

/// Pieces of Atom feeds, put together at random.
fn fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::from("<feed xmlns=\"http://www.w3.org/2005/Atom\">")),
        Just(String::from("</feed>")),
        Just(String::from("<entry>")),
        Just(String::from("</entry>")),
        Just(String::from("<title>")),
        Just(String::from("</title>")),
        Just(String::from("<id>")),
        Just(String::from("</id>")),
        Just(String::from("<author><name>")),
        Just(String::from("</name></author>")),
        Just(String::from("<content type=\"text\">")),
        Just(String::from("<summary>")),
        Just(String::from("</content>")),
        Just(String::from("<![CDATA[x]]>")),
        Just(String::from("&amp;&#0;&#xFFFFFFFF;&bogus;&")),
        Just(String::from(
            "<link rel=\"subsection\" type=\"application/atom+xml\" href=\"../../x\"/>"
        )),
        Just(String::from(
            "<link rel=\"http://opds-spec.org/acquisition\" type=\"application/epub+zip\" href=\"javascript:alert(1)\"/>"
        )),
        Just(String::from(
            "<link rel=\"http://opds-spec.org/acquisition/open-access\" type=\"text/plain\" href=\"/b.epub\" title=\"T\"/>"
        )),
        Just(String::from(
            "<link rel=\"http://opds-spec.org/image/thumbnail\" href=\"/t.png\"/>"
        )),
        Just(String::from("<link rel=\"next\" href=\"?page=2\"/>")),
        Just(String::from(
            "<link rel=\"search\" href=\"/s?q={searchTerms}\"/>"
        )),
        Just(String::from("<link href=\"\"/>")),
        Just(String::from("<!DOCTYPE x [<!ENTITY a \"aaaa\">]>")),
        "[a-z <>/\"=&;]{0,20}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(300)))]

    #[test]
    fn pieces_of_feeds_are_read_as_before(parts in prop::collection::vec(fragment(), 0..80)) {
        let text = parts.concat();
        let (old, new) = both(text.as_bytes(), BASE);
        prop_assert_eq!(new, old);
    }

    #[test]
    fn any_bytes_are_read_as_before_and_without_panic(data in prop::collection::vec(any::<u8>(), 0..3000)) {
        let (old, new) = both(&data, BASE);
        prop_assert_eq!(new, old);
    }

    #[test]
    fn links_are_joined_as_before(
        base in prop_oneof![
            Just(String::from("https://cat.example/opds/root.xml?x=1#f")),
            Just(String::from("http://h/")), Just(String::from("https://h")),
            Just(String::new()), "[a-z:/?#.]{0,20}",
        ],
        href in prop_oneof![
            Just(String::from("/a")), Just(String::from("../b")), Just(String::from("./c/../d")),
            Just(String::from("?q")), Just(String::from("#f")), Just(String::from("//other/e")),
            Just(String::from("mailto:x")), Just(String::from("..")), Just(String::from("a/./b/..")),
            "[a-z:/?#.]{0,20}",
        ],
    ) {
        prop_assert_eq!(native::join_url(&base, &href).unwrap(), paperpaw_sdk::web::join_url(&base, &href));
    }

    #[test]
    fn texts_are_cleaned_as_before(text in "(\\PC|\\s){0,700}") {
        prop_assert_eq!(native::clean(&text).unwrap(), crate::feed::clean(&text));
    }
}

#[test]
fn file_types_are_as_before() {
    for (mime, url) in [
        ("application/epub+zip", "/x"),
        ("application/epub+zip; charset=x", "/x"),
        ("application/octet-stream", "/dir/Book.EPUB?dl=1"),
        ("", "/a/b.htm#x"),
        ("", "/a/b.html"),
        ("", "/a/epub"),
        ("", "/a/.epub"),
        ("x/y", "/é.txt"),
        ("application/fb2+zip", ""),
        ("", ""),
        ("", "/ünï.cbz"),
    ] {
        let old = crate::feed::Book {
            mime: mime.into(),
            url: url.into(),
            title: String::new(),
        };
        let new = native::Book {
            mime: mime.into(),
            url: url.into(),
            title: String::new(),
        };
        assert_eq!(new.extension(), old.extension(), "{mime:?} {url:?}");
    }
}

/// The batches `declarations::settings` submits, as records.
/// One declared setting, field by field.
type Spec = (
    String,
    String,
    String,
    String,
    u32,
    u32,
    Vec<u8>,
    i32,
    i32,
    String,
    i32,
);

fn declared() -> Vec<Spec> {
    use paperpaw_core::native_host::registrations::{Record, records_decode};
    let mut out: Vec<Vec<u8>> = Vec::new();
    {
        let mut d = paperpaw_sdk::native::declare::Declarations::new(|batch: &[u8]| {
            out.push(batch.to_vec());
            Ok(())
        });
        crate::declarations::settings(&mut d).unwrap();
    }
    let mut specs: Vec<Spec> = Vec::new();
    for batch in out {
        let records = records_decode(&batch).unwrap();
        for record in records.iter() {
            match record {
                Record::SettingSpec(s) => specs.push((
                    String::from(s.name),
                    String::from(s.group),
                    String::from(s.title),
                    String::from(s.help),
                    s.kind,
                    s.flags,
                    s.default.to_vec(),
                    s.min,
                    s.max,
                    String::new(),
                    0,
                )),
                Record::SettingExtra(e) => {
                    let spec = specs.iter_mut().find(|s| s.0 == e.name).unwrap();
                    spec.9 = String::from(e.section);
                    spec.10 = e.step;
                }
                other => panic!("{other:?}"),
            }
        }
    }
    specs
}

/// The default bytes the host would hold for `kind`.
fn default_of(kind: paperpaw_sdk::native::declare::Kind<'_>) -> Vec<u8> {
    use paperpaw_core::native_host::registrations::{Record, records_decode};
    use paperpaw_sdk::native::declare::{Declarations, SpecDecl};
    let mut out: Vec<Vec<u8>> = Vec::new();
    {
        let mut d = Declarations::new(|batch: &[u8]| {
            out.push(batch.to_vec());
            Ok(())
        });
        d.spec(SpecDecl::new("x", kind, "g", "t")).unwrap();
        d.submit().unwrap();
    }
    let records = records_decode(&out[0]).unwrap();
    match records.iter().next().unwrap() {
        Record::SettingSpec(spec) => spec.default.to_vec(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_interface_6_settings_are_declared_with_the_same_fields() {
    use paperpaw_sdk::{SettingKind, SettingValue, native::declare::Kind};
    let specs = declared();
    assert_eq!(specs.len(), crate::SETTINGS.len());
    for old in &crate::SETTINGS {
        let name = old.id.as_str().strip_prefix("opds.").unwrap();
        let (_, group, title, help, kind, flags, default, min, max, section, step) = specs
            .iter()
            .find(|s| s.0 == name)
            .unwrap_or_else(|| panic!("{name} is not declared"));
        assert_eq!(group.as_str(), old.group.as_str(), "{name}");
        assert_eq!(title.as_str(), &*old.title, "{name}");
        assert_eq!(Some(help.as_str()), old.description.as_deref(), "{name}");
        match (&old.kind, &old.default) {
            (SettingKind::Bool, SettingValue::Bool(on)) => {
                assert_eq!(
                    (*kind, default.clone()),
                    (0, default_of(Kind::Bool(*on))),
                    "{name}"
                );
            }
            (SettingKind::Int { min: lo, max: hi }, SettingValue::Int(value)) => {
                assert_eq!(
                    (*kind, default.clone(), i64::from(*min), i64::from(*max)),
                    (
                        1,
                        default_of(Kind::Int {
                            default: *value,
                            min: *lo as i32,
                            max: *hi as i32
                        }),
                        *lo,
                        *hi
                    ),
                    "{name}"
                );
            }
            other => panic!("{name}: {other:?}"),
        }
        // Bit 2 kept on reset, bit 4 advanced; nothing else is set.
        let advanced = old.level == paperpaw_sdk::Level::Advanced;
        assert_eq!(
            *flags,
            u32::from(old.is_kept_on_reset()) << 2 | u32::from(advanced) << 4,
            "{name}"
        );
        assert!(!old.scoped && !old.guarded, "{name}");
        // The part of the settings page and the step of a number.
        let extra = old.extra();
        assert_eq!(
            section.as_str(),
            extra.section.as_deref().unwrap_or(""),
            "{name}"
        );
        assert_eq!(
            i64::from(*step),
            if extra.step <= 1 { 0 } else { extra.step },
            "{name}"
        );
    }
}

fn acquisition(url: &str, mime: &str) -> String {
    format!(
        "<link rel=\"http://opds-spec.org/acquisition\" type=\"{mime}\" href=\"{url}\" title=\"T\"/>"
    )
}

#[test]
fn a_long_address_or_type_is_not_kept() {
    let long_url = format!("/{}.epub", "a".repeat(native::MAX_URL));
    let long_mime = format!("application/{}", "x".repeat(native::MAX_MIME));
    let feed = format!(
        "<feed><entry><title>T</title>{}{}{}{}</entry></feed>",
        acquisition(&long_url, "application/epub+zip"),
        acquisition("/b.epub", &long_mime),
        acquisition("/c.epub", "application/epub+zip"),
        format_args!("<link rel=\"next\" href=\"{long_url}\"/>"),
    );
    let read = native::read(&mut Bytes(feed.as_bytes()), BASE).unwrap();
    let urls: Vec<&str> = read.entries[0]
        .books
        .iter()
        .map(|b| b.url.as_str())
        .collect();
    assert_eq!(urls, ["https://cat.example/c.epub"]);
    assert_eq!(read.next, None);
}

#[test]
fn a_page_of_full_entries_is_cut_at_the_page_budget() {
    // 100 entries of 16 files, each file with the longest address and type.
    let url = format!("/{}", "u".repeat(native::MAX_URL - 60));
    let mime = format!("application/{}", "m".repeat(native::MAX_MIME - 16));
    let mut feed = String::from("<feed>");
    for n in 0..native::MAX_ENTRIES {
        feed.push_str(&format!("<entry><title>{n}</title>"));
        for f in 0..native::MAX_BOOKS {
            feed.push_str(&acquisition(&format!("{url}{f}"), &mime));
        }
        feed.push_str("</entry>");
    }
    feed.push_str("</feed>");
    let read = native::read(&mut Bytes(feed.as_bytes()), BASE).unwrap();
    assert!(read.memory, "the cut is said");
    assert!(!read.entries.is_empty() && read.entries.len() < native::MAX_ENTRIES);
    let held: usize = read
        .entries
        .iter()
        .flat_map(|e| &e.books)
        .map(|b| b.mime.len() + b.url.len() + b.title.len())
        .sum();
    assert!(held <= native::MAX_PAGE, "{held} bytes");
}

/// A source that fails after `good` bytes.
struct Failing<'a> {
    data: &'a [u8],
    good: usize,
}
impl Source for Failing<'_> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        let from = usize::try_from(offset).unwrap_or(usize::MAX);
        if from >= self.good {
            return Err(Error::Io);
        }
        let end = (from + buf.len()).min(self.good).min(self.data.len());
        let n = end.saturating_sub(from);
        buf[..n].copy_from_slice(&self.data[from..from + n]);
        Ok(n)
    }
}

#[test]
fn a_failed_read_is_an_error_not_a_short_feed() {
    let feed = format!(
        "<feed><title>Cat</title><entry><title>A</title>{}</entry></feed>",
        acquisition("/a.epub", "application/epub+zip")
    );
    for good in [0, 10, native::CHUNK, feed.len() - 1] {
        let good = good.min(feed.len() - 1);
        let mut source = Failing {
            data: feed.as_bytes(),
            good,
        };
        // Past `good` bytes the source fails; it does not give 0 (the end).
        assert_eq!(
            native::read(&mut source, BASE),
            Err(Error::Io),
            "failing after {good}"
        );
    }
}

proptest! {
    /// The text the XML decoder makes of a token is never longer than the
    /// token (the bound `PORTING-7.md` relies on for its infallible
    /// allocation), and the tokenizer's tokens are bounded.
    #[test]
    fn decoding_an_entity_never_grows_the_text(text in "[a-z&#;0-9xX]{0,200}") {
        prop_assert!(paperpaw_sdk::xml::decode(&text).len() <= text.len());
    }
}
