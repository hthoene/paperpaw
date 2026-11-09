//! The fetch of the feeds (`native_fetch`) over a volume in memory, a network
//! that records what it was asked and a shelf that records what it was given.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use crate::native_book::Failure;
use crate::native_convert::Converted;
use crate::native_convert_tests::day_text;
use crate::native_fetch::{
    Done, Ended, FEED_REQUEST, Fetcher, PAGE_FILE, Shelf, Started, Wire, download, load,
};
use crate::native_news::Feed;
use crate::test_volume::Mem;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error,
    net::{Reason, RequestId},
};
use paperpaw_ui_kit::files::Volume;

const RSS: &str = "<rss><channel><title>Daily News - all of it</title><item><title>One</title><description>Text</description></item></channel></rss>";
const SIGN_IN: &str = "<html><body>Sign in to the Wi-Fi</body></html>";

struct World {
    volume: Mem,
    requests: Vec<(String, String)>,
    cancelled: usize,
    no_network: bool,
    next: u32,
}

impl World {
    fn new(feeds: &str) -> Self {
        let volume = Mem::default();
        volume.write("feeds.txt", feeds.as_bytes()).unwrap();
        Self {
            volume,
            requests: Vec::new(),
            cancelled: 0,
            no_network: false,
            next: 0,
        }
    }

    /// What the host does when a request ends well: the answer is in its file.
    fn answer(&self, url: &str, body: &str) {
        self.volume
            .write(&download(url).unwrap(), body.as_bytes())
            .unwrap();
    }

    fn id(&self) -> RequestId {
        RequestId::new(self.next).unwrap()
    }
}

impl Wire for World {
    fn volume(&self) -> &dyn Volume {
        &self.volume
    }

    fn get(&mut self, url: &str, file: &str) -> Result<RequestId, Error> {
        if self.no_network {
            return Err(Error::Unsupported);
        }
        self.requests.push((url.to_string(), file.to_string()));
        self.next += 1;
        Ok(self.id())
    }

    fn cancel(&mut self) {
        self.cancelled += 1;
    }

    fn declare_work(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

/// A shelf that keeps what it was given, and refuses what it is told to.
#[derive(Default)]
struct Kept {
    books: Vec<(String, String)>,
    refuse: Option<Failure>,
}

impl Shelf for Kept {
    fn put(&mut self, feed: &Feed, name: &str, page: &str, _: &Converted) -> Result<(), Failure> {
        if let Some(failure) = self.refuse {
            return Err(failure);
        }
        assert_eq!(page, PAGE_FILE);
        self.books.push((feed.url.clone(), name.to_string()));
        Ok(())
    }
}

fn ok(world: &World) -> Done {
    Done::new(world.id(), FEED_REQUEST, Reason::Ok, true)
}

fn end(f: &mut Fetcher, w: &mut World, shelf: &mut Kept, done: &Done) -> Ended {
    f.ended(w, shelf, done, (10, 8 * 1024), &day_text).unwrap()
}

#[test]
fn a_start_says_why_it_cannot() {
    let mut f = Fetcher::new();
    let mut w = World::new("");
    assert_eq!(f.start(&mut w, 0, true).unwrap(), Started::None);
    let mut w = World::new("https://a.example/feed\tA\n");
    assert_eq!(f.start(&mut w, 1, false).unwrap(), Started::None);
    w.no_network = true;
    assert_eq!(f.start(&mut w, 0, false).unwrap(), Started::NoNetwork);
    assert!(!f.busy());
    let mut w = World::new("");
    w.volume.write("feeds.txt", &[0xff, 0xfe, 0x00]).unwrap();
    assert_eq!(f.start(&mut w, 0, true).unwrap(), Started::Unreadable);
    // What it could not read is left as it is.
    assert_eq!(w.volume.get("feeds.txt").unwrap(), [0xff, 0xfe, 0x00]);
}

#[test]
fn a_feed_becomes_its_book_with_the_name_it_brought() {
    let url = "https://a.example/feed";
    let mut f = Fetcher::new();
    let mut w = World::new(&format!("{url}\n"));
    let mut shelf = Kept::default();
    assert_eq!(f.start(&mut w, 0, false).unwrap(), Started::Fetching);
    assert_eq!(
        w.requests,
        [(url.to_string(), download(url).unwrap())],
        "asked for with its own file"
    );
    assert!(f.busy());
    w.answer(url, RSS);
    let done = ok(&w);
    assert_eq!(
        end(&mut f, &mut w, &mut shelf, &done),
        Ended::Finished { done: 1, failed: 0 }
    );
    assert_eq!(shelf.books, [(url.to_string(), "Daily News".to_string())]);
    // The name is kept with the feed; no file is left behind.
    assert_eq!(load(w.volume()).unwrap()[0].name, "Daily News");
    assert!(w.volume.get(&download(url).unwrap()).is_none());
    assert!(w.volume.get(PAGE_FILE).is_none());
    assert!(!f.busy());
}

#[test]
fn all_feeds_one_after_the_other_and_a_failed_one_does_not_stop_them() {
    let (a, b, c, d) = (
        "https://a.example/1",
        "https://b.example/2",
        "https://c.example/3",
        "https://d.example/4",
    );
    let mut f = Fetcher::new();
    let mut w = World::new(&format!("{a}\n{b}\tB\n{c}\n{d}\n"));
    let mut shelf = Kept::default();
    f.start(&mut w, 0, true).unwrap();
    // A: fine.
    w.answer(a, RSS);
    let done = ok(&w);
    assert_eq!(end(&mut f, &mut w, &mut shelf, &done), Ended::Next);
    assert_eq!(w.requests.last().unwrap().0, b);
    // B: no answer came (not saved).
    let done = Done::new(w.id(), FEED_REQUEST, Reason::Ok, false);
    assert_eq!(end(&mut f, &mut w, &mut shelf, &done), Ended::Next);
    assert_eq!(w.requests.last().unwrap().0, c);
    // C: a sign-in page, no article: the book there stays.
    w.answer(c, SIGN_IN);
    let done = ok(&w);
    assert_eq!(end(&mut f, &mut w, &mut shelf, &done), Ended::Next);
    assert_eq!(w.requests.last().unwrap().0, d);
    // D: fine.
    w.answer(d, RSS);
    let done = ok(&w);
    assert_eq!(
        end(&mut f, &mut w, &mut shelf, &done),
        Ended::Finished { done: 2, failed: 2 }
    );
    assert_eq!(shelf.books.len(), 2);
    assert_eq!(w.requests.len(), 4);
}

#[test]
fn a_feed_removed_meanwhile_is_left_out_and_the_next_is_asked_for() {
    let (a, b) = ("https://a.example/1", "https://b.example/2");
    let mut f = Fetcher::new();
    let mut w = World::new(&format!("{a}\n{b}\n"));
    let mut shelf = Kept::default();
    f.start(&mut w, 0, true).unwrap();
    // The reader removes the feed while it is fetched.
    w.volume
        .write("feeds.txt", format!("{b}\n").as_bytes())
        .unwrap();
    w.answer(a, RSS);
    let done = ok(&w);
    assert_eq!(end(&mut f, &mut w, &mut shelf, &done), Ended::Next);
    assert!(shelf.books.is_empty());
    assert_eq!(w.requests.last().unwrap().0, b);
    w.answer(b, RSS);
    let done = ok(&w);
    assert_eq!(
        end(&mut f, &mut w, &mut shelf, &done),
        Ended::Finished { done: 1, failed: 0 }
    );
}

#[test]
fn an_end_that_is_not_the_one_under_way_is_ignored() {
    let url = "https://a.example/feed";
    let mut f = Fetcher::new();
    let mut w = World::new(&format!("{url}\n"));
    let mut shelf = Kept::default();
    // Nothing under way.
    let done = Done::new(RequestId::new(1).unwrap(), FEED_REQUEST, Reason::Ok, true);
    assert_eq!(end(&mut f, &mut w, &mut shelf, &done), Ended::Ignored);
    f.start(&mut w, 0, false).unwrap();
    let other = Done::new(RequestId::new(9).unwrap(), FEED_REQUEST, Reason::Ok, true);
    assert_eq!(end(&mut f, &mut w, &mut shelf, &other), Ended::Ignored);
    let token = Done::new(w.id(), 7, Reason::Ok, true);
    assert_eq!(end(&mut f, &mut w, &mut shelf, &token), Ended::Ignored);
    assert!(f.busy());
    // A cancelled one ends as ignored.
    f.cancel(&mut w);
    assert!(!f.busy());
    assert_eq!(w.cancelled, 1);
    f.start(&mut w, 0, false).unwrap();
    let cancelled = Done::new(w.id(), FEED_REQUEST, Reason::Cancelled, false);
    assert_eq!(end(&mut f, &mut w, &mut shelf, &cancelled), Ended::Ignored);
}

#[test]
fn a_shelf_that_refuses_counts_as_a_failed_feed_and_no_memory_stops() {
    let url = "https://a.example/feed";
    let mut f = Fetcher::new();
    let mut w = World::new(&format!("{url}\n"));
    let mut shelf = Kept {
        refuse: Some(Failure::Failed),
        ..Kept::default()
    };
    f.start(&mut w, 0, false).unwrap();
    w.answer(url, RSS);
    let done = ok(&w);
    assert_eq!(
        end(&mut f, &mut w, &mut shelf, &done),
        Ended::Finished { done: 0, failed: 1 }
    );
    // The name did not come with a book that was not made.
    assert_eq!(load(w.volume()).unwrap()[0].name, "");
    shelf.refuse = Some(Failure::Memory);
    f.start(&mut w, 0, false).unwrap();
    w.answer(url, RSS);
    let done = ok(&w);
    assert_eq!(end(&mut f, &mut w, &mut shelf, &done), Ended::Memory);
    assert!(w.volume.get(&download(url).unwrap()).is_none());
}
