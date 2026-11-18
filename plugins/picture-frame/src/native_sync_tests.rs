#![allow(clippy::unwrap_used, clippy::panic, reason = "test assertions")]
//! The sync against a fake WebDAV server in memory: it answers a `PROPFIND`
//! with a listing, a `GET` with the file or a status, and every request ends
//! with the event the host would send.
use super::*;
use crate::native_cache::{self as cache, MANIFEST};
use crate::test_volume::Mem;
use alloc::collections::BTreeMap;
use alloc::vec;

const BASE: &str = "https://dav.example.org/photos/";

#[derive(Default)]
struct Fake {
    volume: Mem,
    /// The server's files: name and bytes.
    files: BTreeMap<String, Vec<u8>>,
    /// What every request answers instead: a status.
    status: Option<u32>,
    /// The end the next request has instead of an answer.
    end: Option<End>,
    pending: Option<Done>,
    asked: Vec<String>,
    refuse_get: bool,
    cancelled: bool,
}

impl Fake {
    fn serve(&mut self, name: &str, size: usize) {
        self.files.insert(name.into(), vec![7u8; size]);
    }

    fn listing(&self) -> String {
        let mut xml = String::from(
            r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/photos/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>"#,
        );
        for (name, bytes) in &self.files {
            xml.push_str(&alloc::format!(
                "<d:response><d:href>/photos/{name}</d:href><d:propstat><d:prop><d:resourcetype/><d:getcontentlength>{}</d:getcontentlength></d:prop></d:propstat></d:response>",
                bytes.len()
            ));
        }
        xml.push_str("</d:multistatus>");
        xml
    }

    fn end(&mut self, token: u32, saved: bool, ok_status: u32) {
        self.pending = Some(match (self.end.take(), self.status) {
            (Some(end), _) => Done {
                token,
                end,
                status: 0,
                saved: false,
            },
            (None, Some(status)) => Done {
                token,
                end: End::Answered,
                status,
                saved: false,
            },
            (None, None) => Done {
                token,
                end: End::Answered,
                status: ok_status,
                saved,
            },
        });
    }
}

impl Link for Fake {
    fn volume(&self) -> &dyn Volume {
        &self.volume
    }

    fn propfind(&mut self, url: &str) -> Result<(), Error> {
        assert_eq!(url, BASE);
        self.asked.push("PROPFIND".into());
        let ok = self.end.is_none() && self.status.is_none();
        if ok {
            self.volume.write(LISTING, self.listing().as_bytes())?;
        }
        self.end(LIST_TOKEN, ok, 207);
        Ok(())
    }

    fn get(&mut self, url: &str, save: &str, max: u64) -> Result<(), Error> {
        if self.refuse_get {
            return Err(Error::Invalid);
        }
        let name = url.rsplit('/').next().unwrap();
        self.asked.push(alloc::format!("GET {name}"));
        let found = self.files.get(name).cloned();
        match found {
            _ if self.end.is_some() || self.status.is_some() => self.end(GET_TOKEN, false, 200),
            None => {
                self.status = None;
                self.pending = Some(Done {
                    token: GET_TOKEN,
                    end: End::Answered,
                    status: 404,
                    saved: false,
                });
            }
            Some(bytes) if bytes.len() as u64 > max => {
                self.pending = Some(Done {
                    token: GET_TOKEN,
                    end: End::Other,
                    status: 0,
                    saved: false,
                });
            }
            Some(bytes) => {
                self.volume.write(save, &bytes)?;
                self.end(GET_TOKEN, true, 200);
            }
        }
        Ok(())
    }

    fn cancel(&mut self) {
        self.cancelled = true;
    }
}

/// Runs a sync to its end.
fn run(fake: &mut Fake, cap: u64, window: usize) -> Finish {
    let mut sync = Sync::default();
    sync.start(
        fake,
        Job {
            url: BASE.into(),
            cap,
            window,
        },
    )
    .unwrap();
    assert!(sync.running());
    loop {
        let done = fake.pending.take().expect("a request is open");
        match sync.done(fake, &done).unwrap() {
            Step::Quiet => {}
            Step::Ended(finish) => {
                assert!(!sync.running());
                return finish;
            }
        }
    }
}

fn cached(fake: &Fake) -> Vec<String> {
    cache::read(&fake.volume)
        .into_iter()
        .map(|e| e.href.rsplit('/').next().unwrap().into())
        .collect()
}

#[test]
fn a_sync_fetches_the_photos_of_the_folder_into_the_cache() {
    let mut fake = Fake::default();
    fake.serve("a.jpg", 300);
    fake.serve("b.png", 500);
    fake.serve("readme.txt", 10);
    let finish = run(&mut fake, 1 << 20, 0);
    assert_eq!(finish.key, "picture-frame.synced");
    assert!(finish.ok);
    assert_eq!((finish.fetched, finish.cached, finish.photos), (2, 2, 2));
    assert_eq!(fake.asked, ["PROPFIND", "GET a.jpg", "GET b.png"]);
    assert_eq!(cached(&fake), ["a.jpg", "b.png"]);
    // The files are in the cache, the listing is gone.
    let list = cache::read(&fake.volume);
    for entry in &list {
        let path = alloc::format!("cache/{}", entry.name);
        assert_eq!(fake.volume.size(&path).unwrap(), entry.size);
    }
    assert!(fake.volume.get(LISTING).is_none());
}

#[test]
fn a_second_sync_fetches_only_what_is_new_and_drops_what_the_server_lost() {
    let mut fake = Fake::default();
    fake.serve("a.jpg", 300);
    fake.serve("b.jpg", 300);
    run(&mut fake, 1 << 20, 0);
    let gone = cache::read(&fake.volume)
        .into_iter()
        .find(|e| e.href.ends_with("b.jpg"))
        .unwrap();
    fake.files.remove("b.jpg");
    fake.serve("c.jpg", 100);
    fake.asked.clear();
    let finish = run(&mut fake, 1 << 20, 0);
    assert_eq!(fake.asked, ["PROPFIND", "GET c.jpg"]);
    assert_eq!((finish.fetched, finish.cached), (1, 2));
    assert_eq!(cached(&fake), ["a.jpg", "c.jpg"]);
    assert!(
        fake.volume
            .get(&alloc::format!("cache/{}", gone.name))
            .is_none()
    );
}

#[test]
fn the_cache_never_holds_more_than_the_cap_and_the_next_sync_goes_on() {
    let mut fake = Fake::default();
    for name in ["a.jpg", "b.jpg", "c.jpg", "d.jpg"] {
        fake.serve(name, 400);
    }
    let first = run(&mut fake, 1000, 0);
    assert_eq!((first.fetched, first.cached, first.photos), (2, 2, 4));
    assert_eq!(cached(&fake), ["a.jpg", "b.jpg"]);
    assert_eq!(first.window, 2);
    let second = run(&mut fake, 1000, first.window);
    assert_eq!(cached(&fake), ["c.jpg", "d.jpg"]);
    assert_eq!(second.window, 0);
    // The bytes on the card stay within the cap at the end of each sync.
    let total: u64 = cache::read(&fake.volume).iter().map(|e| e.size).sum();
    assert!(total <= 1000);
    let dropped = fake
        .volume
        .files
        .borrow()
        .keys()
        .filter(|k| k.starts_with("cache/"))
        .count();
    assert_eq!(dropped, 2);
}

#[test]
fn a_server_that_refuses_the_account_or_has_no_folder_says_so_and_changes_nothing() {
    let mut fake = Fake::default();
    fake.serve("a.jpg", 10);
    run(&mut fake, 1 << 20, 0);
    for (status, key) in [
        (401, "picture-frame.denied"),
        (403, "picture-frame.denied"),
        (404, "picture-frame.not-found"),
        (500, "picture-frame.failed"),
    ] {
        fake.status = Some(status);
        let finish = run(&mut fake, 1 << 20, 0);
        assert_eq!(finish.key, key, "{status}");
        assert!(!finish.ok);
        assert_eq!(cached(&fake), ["a.jpg"], "{status}");
    }
}

#[test]
fn a_network_that_goes_ends_the_sync_and_keeps_what_was_fetched() {
    let mut fake = Fake::default();
    fake.serve("a.jpg", 10);
    fake.serve("b.jpg", 10);
    let mut sync = Sync::default();
    let job = Job {
        url: BASE.into(),
        cap: 1 << 20,
        window: 0,
    };
    sync.start(&mut fake, job).unwrap();
    let done = fake.pending.take().unwrap();
    assert!(matches!(sync.done(&mut fake, &done).unwrap(), Step::Quiet));
    // The first photo came; the network goes before the second.
    let done = fake.pending.take().unwrap();
    assert!(matches!(sync.done(&mut fake, &done).unwrap(), Step::Quiet));
    fake.end = Some(End::Network);
    fake.end(GET_TOKEN, false, 0);
    let done = fake.pending.take().unwrap();
    let Step::Ended(finish) = sync.done(&mut fake, &done).unwrap() else {
        panic!("over");
    };
    assert_eq!(finish.key, "picture-frame.no-network");
    assert!(!finish.ok);
    assert_eq!(finish.fetched, 1);
    assert_eq!(cached(&fake), ["a.jpg"]);
    assert!(!sync.running());
}

#[test]
fn a_photo_the_server_does_not_have_or_that_is_too_large_is_left_out() {
    let mut fake = Fake::default();
    fake.serve("a.jpg", 10);
    fake.serve("big.jpg", 600);
    // The listing says 600 B; the cap leaves 500: it is not even asked for.
    let finish = run(&mut fake, 500, 0);
    assert_eq!(fake.asked, ["PROPFIND", "GET a.jpg"]);
    assert_eq!(finish.fetched, 1);
    assert!(finish.ok);
    // A photo that disappears between the listing and the fetch.
    let mut fake = Fake::default();
    fake.serve("a.jpg", 10);
    fake.serve("b.jpg", 10);
    let mut sync = Sync::default();
    let job = Job {
        url: BASE.into(),
        cap: 1 << 20,
        window: 0,
    };
    sync.start(&mut fake, job).unwrap();
    let done = fake.pending.take().unwrap();
    assert!(matches!(sync.done(&mut fake, &done).unwrap(), Step::Quiet));
    fake.files.remove("b.jpg");
    let finish = loop {
        let done = fake.pending.take().unwrap();
        if let Step::Ended(finish) = sync.done(&mut fake, &done).unwrap() {
            break finish;
        }
    };
    assert_eq!(finish.key, "picture-frame.synced");
    assert_eq!(finish.fetched, 1);
    assert_eq!(cached(&fake), ["a.jpg"]);
}

#[test]
fn an_empty_server_folder_empties_the_cache() {
    let mut fake = Fake::default();
    fake.serve("a.jpg", 10);
    run(&mut fake, 1 << 20, 0);
    fake.files.clear();
    let finish = run(&mut fake, 1 << 20, 0);
    assert_eq!(finish.key, "picture-frame.empty-server");
    assert!(finish.ok);
    assert!(cached(&fake).is_empty());
    assert!(fake.volume.get(MANIFEST).is_some_and(|m| m.is_empty()));
}

#[test]
fn a_refused_request_ends_a_start_and_leaves_a_photo_out() {
    struct Refuses(Fake);
    impl Link for Refuses {
        fn volume(&self) -> &dyn Volume {
            self.0.volume()
        }
        fn propfind(&mut self, _: &str) -> Result<(), Error> {
            Err(Error::Busy)
        }
        fn get(&mut self, _: &str, _: &str, _: u64) -> Result<(), Error> {
            Err(Error::Busy)
        }
        fn cancel(&mut self) {}
    }
    let mut sync = Sync::default();
    let job = Job {
        url: BASE.into(),
        cap: 1 << 20,
        window: 0,
    };
    assert_eq!(
        sync.start(&mut Refuses(Fake::default()), job),
        Err(Error::Busy)
    );
    assert!(!sync.running());
    // A listing that came, and every GET refused: the sync ends, nothing fetched.
    let mut fake = Fake::default();
    fake.serve("a.jpg", 10);
    fake.refuse_get = true;
    let finish = run(&mut fake, 1 << 20, 0);
    assert_eq!(finish.key, "picture-frame.failed");
    assert!(!finish.ok);
}

#[test]
fn cancelling_stops_the_requests_and_the_sync() {
    let mut fake = Fake::default();
    fake.serve("a.jpg", 10);
    let mut sync = Sync::default();
    sync.start(
        &mut fake,
        Job {
            url: BASE.into(),
            cap: 1 << 20,
            window: 0,
        },
    )
    .unwrap();
    sync.cancel(&mut fake);
    assert!(fake.cancelled);
    assert!(!sync.running());
    // An end that comes late is ignored.
    let done = fake.pending.take().unwrap();
    assert!(matches!(sync.done(&mut fake, &done).unwrap(), Step::Quiet));
}
