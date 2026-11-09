//! Every allocation the plugin's host-free code makes may be refused: the
//! readers of feeds (with entities in their texts and links, now that the XML
//! decoder has its fallible forms), the search addresses, the catalog list
//! and its text, the `Authorization` header and the names of books end in
//! `Error::NoMemory` (or, for a feed, in what was read, flagged as `memory`)
//! or succeed, never abort. The refusals of the plugin's host calls are swept
//! in `native_browse_flow.rs` and `native_sync_flow.rs`, through scripted services.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_opds::native_catalogs as catalogs;
use paperpaw_plugin_opds::native_dav as dav;
use paperpaw_plugin_opds::native_download as download;
use paperpaw_plugin_opds::native_files::Files;
use paperpaw_plugin_opds::native_json as json;
use paperpaw_plugin_opds::native_keep::{Secret, name_of};
use paperpaw_plugin_opds::native_model::{self as model, Entry, Limits, Source};
use paperpaw_plugin_opds::native_search as search;
use paperpaw_sdk::native::Error;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

const FEED: &[u8] = b"<feed><title>Cat &amp; Co</title>\
<link rel=\"next\" type=\"application/atom+xml\" href=\"?page=2&amp;x=1\"/>\
<entry><title>One &lt;two&gt; &#169;</title><id>urn:1</id><author><name>A</name></author><author><name>B</name></author>\
<summary>About &amp;amp; more</summary>\
<link rel=\"http://opds-spec.org/acquisition\" type=\"application/epub+zip\" href=\"/a.epub?x=1&amp;y=2\" title=\"EPUB &amp; more\"/>\
<link rel=\"http://opds-spec.org/image/thumbnail\" href=\"t.png\"/></entry>\
<entry><title>Shelf</title><link rel=\"subsection\" type=\"application/atom+xml\" href=\"../s.xml\"/></entry></feed>";

const JSON_FEED: &[u8] = br#"{"metadata": {"title": "Kom"},
"links": [{"rel": "next", "href": "?page=2"}, {"rel": "search", "href": "/s{?query}", "templated": true}],
"navigation": [{"href": "/series", "title": "Series", "type": "application/opds+json"}],
"publications": [{"metadata": {"title": "Alice", "identifier": "u:1", "author": [{"name": "L C"}, "S E"],
 "description": "<p>Down the &amp; <b>hole</b></p>"},
 "links": [{"rel": "http://opds-spec.org/acquisition", "href": "/b/1", "type": "application/epub+zip", "title": "EPUB"}],
 "images": [{"href": "/b/1/t"}]}]}"#;

const LISTING: &[u8] = br#"<d:multistatus xmlns:d="DAV:">
<d:response><d:href>/dav/Books/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
<d:response><d:href>/dav/Books/Die%20Verwandlung.epub</d:href><d:propstat><d:prop><d:displayname>Die &amp; V.epub</d:displayname><d:getcontenttype>application/octet-stream</d:getcontenttype></d:prop></d:propstat></d:response>
<d:response><d:href>/dav/Books/Sub/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
</d:multistatus>"#;

const DESCRIPTION: &[u8] = br#"<OpenSearchDescription>
<Url type="application/atom+xml" template="/s?q={searchTerms}&amp;f=1"/></OpenSearchDescription>"#;

struct Bytes(&'static [u8]);

impl Source for Bytes {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        let from = usize::try_from(offset).unwrap().min(self.0.len());
        let n = buf.len().min(self.0.len() - from);
        buf[..n].copy_from_slice(&self.0[from..from + n]);
        Ok(n)
    }
}

/// A feed read short for want of memory is said so: the sweep takes that as
/// the refusal it is.
fn said(feed: &model::Feed) -> Result<(), Error> {
    if feed.memory {
        return Err(Error::NoMemory);
    }
    Ok(())
}

#[test]
fn reading_a_feed_with_entities_survives_refused_allocations() {
    fail_alloc::sweep(|| {
        said(&model::read(
            &mut Bytes(FEED),
            "https://cat.example/opds/root.xml",
            Limits::default(),
        )?)
    });
}

#[test]
fn reading_an_opds_2_feed_survives_refused_allocations() {
    fail_alloc::sweep(|| {
        let feed = json::read(
            &mut Bytes(JSON_FEED),
            "https://k.example/opds",
            Limits::of_class(true),
        )?;
        said(&feed)
    });
}

#[test]
fn reading_a_webdav_listing_survives_refused_allocations() {
    fail_alloc::sweep(|| {
        let feed = dav::read(
            &mut Bytes(LISTING),
            "https://c.example/dav/Books/",
            Limits::default(),
        )?;
        said(&feed)
    });
}

#[test]
fn searching_survives_refused_allocations() {
    fail_alloc::sweep(|| {
        let template = search::template(&mut Bytes(DESCRIPTION), "https://c.example/osd.xml")?;
        search::url(
            template.as_deref().unwrap_or("/s?q={searchTerms}"),
            "Jane é Austen&x",
        )?;
        Ok(())
    });
}

#[test]
fn joining_and_cleaning_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        for href in [
            "/a",
            "../b",
            "./c/../d",
            "?q",
            "#f",
            "//other/e",
            "mailto:x",
        ] {
            model::join_url("https://cat.example/opds/root.xml?x=1", href)?;
        }
        model::clean("  Alice \n in   Wonderland ")?;
        Ok(())
    });
}

#[test]
fn the_catalog_list_survives_refused_allocations() {
    let text = include_str!("../testdata/catalogs.txt.v1");
    fail_alloc::sweep(|| {
        let mut list = catalogs::parse(text, 100)?;
        let text = catalogs::encode(&list)?;
        assert_eq!(text.lines().count(), 3);
        for catalog in &list {
            catalog.sync_folder("Books")?;
            catalog.shown_folder("Books")?;
        }
        list.push(catalogs::Catalog::new("https://www.example.org/opds")?);
        Ok(())
    });
}

#[test]
fn the_password_and_the_names_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        let password = Secret::copy("s3cret")?;
        let header = Secret::basic("me", &password)?;
        assert!(header.as_str().starts_with("Basic "));
        name_of("https://a.example")?;
        Ok(())
    });
    let entry = Entry {
        title: "A title".into(),
        authors: "Austen, Jane".into(),
        ..Entry::default()
    };
    fail_alloc::sweep(|| {
        download::book_name(&entry, "https://a.example/x", "epub")?;
        download::book_name(&Entry::default(), "https://a.example/get/x", "epub")?;
        Ok(())
    });
}

/// A file with a fixed head, for the check of a download.
struct Head(&'static [u8]);

impl Files for Head {
    fn size(&self, _: &str) -> Result<u64, Error> {
        Ok(self.0.len() as u64)
    }
    fn read_at(&self, _: &str, offset: u64, out: &mut [u8]) -> Result<usize, Error> {
        let from = usize::try_from(offset).unwrap().min(self.0.len());
        let n = out.len().min(self.0.len() - from);
        out[..n].copy_from_slice(&self.0[from..from + n]);
        Ok(n)
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    fn remove(&self, _: &str) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
}

#[test]
fn checking_a_download_survives_refused_allocations() {
    fail_alloc::sweep(|| {
        assert!(download::may_be_book(&Head(b"PK\x03\x04"), "f", "a.epub")?);
        assert!(!download::may_be_book(&Head(b"<html>"), "f", "a.txt")?);
        Ok(())
    });
}
