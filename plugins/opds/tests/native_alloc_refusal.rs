//! Every allocation of the feed reader that this crate makes may be refused:
//! the reading ends with what was read (the feed marked `memory`) or succeeds,
//! never aborts. (The XML decoder's owned text for an entity or an escape in
//! an attribute has no refusal path, so a refusal sweep cannot include a feed
//! with entities or escapes in links: GATE-ENTITY in `PORTING-7.md`. Its
//! bound and the reader's reading of such feeds are tested without refusals,
//! in `src/native_parity_tests.rs`.)
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_opds::native_model::{self as model, Source};
use paperpaw_sdk::native::Error;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

const FEED: &[u8] = b"<feed><title>Cat</title>\
<link rel=\"next\" type=\"application/atom+xml\" href=\"?page=2\"/>\
<entry><title>One two</title><id>urn:1</id><author><name>A</name></author><author><name>B</name></author>\
<summary>About</summary>\
<link rel=\"http://opds-spec.org/acquisition\" type=\"application/epub+zip\" href=\"/a.epub\" title=\"EPUB\"/>\
<link rel=\"http://opds-spec.org/image/thumbnail\" href=\"t.png\"/></entry>\
<entry><title>Shelf</title><link rel=\"subsection\" type=\"application/atom+xml\" href=\"../s.xml\"/></entry></feed>";

struct Bytes;
impl Source for Bytes {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        let from = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(FEED.len());
        let n = buf.len().min(FEED.len() - from);
        buf[..n].copy_from_slice(&FEED[from..from + n]);
        Ok(n)
    }
}

#[test]
fn reading_a_feed_survives_refused_allocations() {
    fail_alloc::sweep(|| {
        let feed = model::read(
            &mut Bytes,
            "https://cat.example/opds/root.xml",
            model::Limits::default(),
        )?;
        // A refused allocation ends the reading short and says so.
        if feed.memory {
            return Err(Error::NoMemory);
        }
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
