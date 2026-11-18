//! The sync against a fake card and a fake WebDAV server: every rule of the
//! plan, the trash, the guards, a cut at every request (the host writes a
//! fetched file whole or not at all; a sent one may or may not have arrived)
//! and the resume after it.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use crate::test_volume::Mem;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::ToString;
use paperpaw_ui_kit::dav::{listing::etag_hash, url::segment};
use paperpaw_ui_kit::files::Volume;

const BASE: &str = "http://h/dav/";

#[derive(Debug, Clone)]
enum Req {
    Propfind(String),
    Get(String, String),
    Put(String, String),
    Delete(String),
    Mkcol(String),
}

/// A card and a server in memory.
pub(crate) struct Fake {
    data: Mem,
    /// The card's files: path to content and time.
    card: BTreeMap<String, (String, u64)>,
    card_dirs: BTreeSet<String>,
    /// The server's files below `/dav/`: path to content and a version.
    server: BTreeMap<String, (String, u64)>,
    server_dirs: BTreeSet<String>,
    clock: u64,
    req: Option<Req>,
    /// Requests made, by method and path.
    log: Vec<String>,
    /// Whether the server tells an `ETag`.
    etags: bool,
    /// Where the server answers `500` to a request for this path.
    broken: BTreeSet<String>,
    /// The card refuses a rename (a card that went).
    no_rename: bool,
}

impl Fake {
    fn new() -> Self {
        let mut fake = Self {
            data: Mem::default(),
            card: BTreeMap::new(),
            card_dirs: BTreeSet::new(),
            server: BTreeMap::new(),
            server_dirs: BTreeSet::new(),
            clock: 1000,
            req: None,
            log: Vec::new(),
            etags: true,
            broken: BTreeSet::new(),
            no_rename: false,
        };
        fake.server_dirs.insert(String::new());
        fake.card_dirs.insert("Notes".into());
        fake
    }

    fn tick(&mut self) -> u64 {
        self.clock += 7;
        self.clock
    }

    fn write_card(&mut self, path: &str, text: &str) {
        let t = self.tick();
        self.card.insert(format!("Notes/{path}"), (text.into(), t));
        let mut dir = String::from("Notes");
        for part in path
            .split('/')
            .rev()
            .skip(1)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            dir = format!("{dir}/{part}");
            self.card_dirs.insert(dir.clone());
        }
    }

    fn write_server(&mut self, path: &str, text: &str) {
        let t = self.tick();
        self.server.insert(path.into(), (text.into(), t));
        let mut dir = String::new();
        for part in path
            .split('/')
            .rev()
            .skip(1)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            dir = if dir.is_empty() {
                part.into()
            } else {
                format!("{dir}/{part}")
            };
            self.server_dirs.insert(dir.clone());
        }
    }

    /// The files in the trash, by their names in the data folder.
    fn trashed(&self) -> Vec<String> {
        let prefix = format!("{TRASH_DIR}/");
        let mut found: Vec<String> = self
            .data
            .files
            .borrow()
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        found.sort();
        found
    }

    fn card(&self, path: &str) -> Option<&str> {
        self.card
            .get(&format!("Notes/{path}"))
            .map(|(t, _)| t.as_str())
    }

    fn server(&self, path: &str) -> Option<&str> {
        self.server.get(path).map(|(t, _)| t.as_str())
    }

    /// Every text that is anywhere: the card, the server, the trash.
    fn texts(&self) -> BTreeSet<String> {
        self.card
            .values()
            .chain(self.server.values())
            .map(|(t, _)| t.clone())
            .collect()
    }

    fn rel(url: &str) -> String {
        let rest = url.strip_prefix(BASE).unwrap_or("");
        paperpaw_sdk::web::percent_decode(rest.trim_end_matches('/'))
    }

    fn done(status: u32) -> Done {
        Done {
            end: End::Answered,
            status,
            saved: false,
            version: 0,
        }
    }

    /// Answers the request under way.
    fn execute(&mut self) -> Done {
        let req = self.req.take().unwrap();
        match req {
            Req::Propfind(url) => {
                self.log.push(format!("PROPFIND {}", Self::rel(&url)));
                self.propfind(&url)
            }
            Req::Get(url, path) => {
                let rel = Self::rel(&url);
                self.log.push(format!("GET {rel}"));
                if self.broken.contains(&rel) {
                    return Self::done(500);
                }
                let Some((text, _)) = self.server.get(&rel).cloned() else {
                    return Self::done(404);
                };
                let t = self.tick();
                self.card.insert(path, (text, t));
                Done {
                    saved: true,
                    ..Self::done(200)
                }
            }
            Req::Put(url, path) => {
                let rel = Self::rel(&url);
                self.log.push(format!("PUT {rel}"));
                if self.broken.contains(&rel) {
                    return Self::done(500);
                }
                let parent = rel.rsplit_once('/').map_or("", |(p, _)| p);
                if !self.server_dirs.contains(parent) {
                    return Self::done(409);
                }
                let (text, _) = self.card.get(&path).cloned().unwrap();
                let t = self.tick();
                self.server.insert(rel, (text, t));
                Done {
                    version: if self.etags {
                        etag_hash(&format!("\"{t}\""))
                    } else {
                        0
                    },
                    ..Self::done(201)
                }
            }
            Req::Delete(url) => {
                let rel = Self::rel(&url);
                self.log.push(format!("DELETE {rel}"));
                Self::done(if self.server.remove(&rel).is_some() {
                    204
                } else {
                    404
                })
            }
            Req::Mkcol(url) => {
                let rel = Self::rel(&url);
                self.log.push(format!("MKCOL {rel}"));
                let parent = rel.rsplit_once('/').map_or("", |(p, _)| p);
                if self.server_dirs.contains(&rel) {
                    Self::done(405)
                } else if !self.server_dirs.contains(parent) {
                    Self::done(409)
                } else {
                    self.server_dirs.insert(rel);
                    Self::done(201)
                }
            }
        }
    }

    fn propfind(&mut self, url: &str) -> Done {
        let rel = Self::rel(url);
        if !self.server_dirs.contains(&rel) {
            return Self::done(404);
        }
        let own = if rel.is_empty() {
            "/dav/".to_string()
        } else {
            format!(
                "/dav/{}/",
                rel.split('/')
                    .map(|p| segment(p).unwrap())
                    .collect::<Vec<_>>()
                    .join("/")
            )
        };
        let mut xml = format!(
            "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\"><d:response><d:href>{own}</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>"
        );
        let prefix = if rel.is_empty() {
            String::new()
        } else {
            format!("{rel}/")
        };
        for dir in &self.server_dirs {
            if let Some(name) = dir.strip_prefix(&prefix)
                && !name.is_empty()
                && !name.contains('/')
            {
                xml.push_str(&format!("<d:response><d:href>{own}{}/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>", segment(name).unwrap()));
            }
        }
        for (path, (text, version)) in &self.server {
            if let Some(name) = path.strip_prefix(&prefix)
                && !name.contains('/')
            {
                let etag = if self.etags {
                    format!("<d:getetag>\"{version}\"</d:getetag>")
                } else {
                    String::new()
                };
                xml.push_str(&format!("<d:response><d:href>{own}{}</d:href><d:propstat><d:prop><d:resourcetype/><d:getcontentlength>{}</d:getcontentlength>{etag}</d:prop></d:propstat></d:response>", segment(name).unwrap(), text.len()));
            }
        }
        xml.push_str("</d:multistatus>");
        self.data.write("listing.xml", xml.as_bytes()).unwrap();
        Done {
            saved: true,
            ..Self::done(207)
        }
    }
}

impl Link for Fake {
    fn data(&self) -> &dyn Volume {
        &self.data
    }
    fn list(&mut self, folder: &str, out: &mut Vec<Entry>, most: usize) -> Result<(), Error> {
        if !self.card_dirs.contains(folder) {
            return Err(Error::NotFound);
        }
        let prefix = format!("{folder}/");
        let mut found = Vec::new();
        for (path, (text, t)) in &self.card {
            if let Some(name) = path.strip_prefix(&prefix)
                && !name.contains('/')
            {
                found.push(Entry {
                    name: name.into(),
                    folder: false,
                    side: Side {
                        size: text.len() as u64,
                        stamp: *t,
                    },
                });
            }
        }
        for dir in &self.card_dirs {
            if let Some(name) = dir.strip_prefix(&prefix)
                && !name.contains('/')
            {
                found.push(Entry {
                    name: name.into(),
                    folder: true,
                    side: Side::default(),
                });
            }
        }
        found.truncate(most);
        out.extend(found);
        Ok(())
    }
    fn stat(&mut self, path: &str) -> Result<Option<Side>, Error> {
        Ok(self.card.get(path).map(|(t, m)| Side {
            size: t.len() as u64,
            stamp: *m,
        }))
    }
    fn make_dir(&mut self, path: &str) -> Result<(), Error> {
        let mut dir = String::new();
        for part in path.split('/') {
            dir = if dir.is_empty() {
                part.into()
            } else {
                format!("{dir}/{part}")
            };
            self.card_dirs.insert(dir.clone());
        }
        Ok(())
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<(), Error> {
        if self.no_rename {
            return Err(Error::Io);
        }
        if self.card.contains_key(to) {
            return Err(Error::State);
        }
        let file = self.card.remove(from).ok_or(Error::NotFound)?;
        self.card.insert(to.into(), file);
        Ok(())
    }
    fn trash(&mut self, from: &str, to: &str) -> Result<(), Error> {
        let (text, _) = self.card.remove(from).ok_or(Error::NotFound)?;
        self.data.write(to, text.as_bytes())
    }
    fn remove(&mut self, path: &str) -> Result<(), Error> {
        self.card.remove(path).map(|_| ()).ok_or(Error::NotFound)
    }
    fn propfind(&mut self, url: &str) -> Result<(), Error> {
        self.req = Some(Req::Propfind(url.into()));
        Ok(())
    }
    fn get(&mut self, url: &str, path: &str, _max: u64) -> Result<(), Error> {
        self.req = Some(Req::Get(url.into(), path.into()));
        Ok(())
    }
    fn put(&mut self, url: &str, path: &str) -> Result<(), Error> {
        self.req = Some(Req::Put(url.into(), path.into()));
        Ok(())
    }
    fn delete(&mut self, url: &str) -> Result<(), Error> {
        self.req = Some(Req::Delete(url.into()));
        Ok(())
    }
    fn mkcol(&mut self, url: &str) -> Result<(), Error> {
        self.req = Some(Req::Mkcol(url.into()));
        Ok(())
    }
    fn cancel(&mut self) {
        self.req = None;
    }
}

fn job(mode: Mode) -> Job {
    Job {
        pair: 1,
        root: "Notes".into(),
        url: BASE.into(),
        mode,
        device: "x4".into(),
        date: "2026-11-18".into(),
    }
}

/// How a run ended: with the sync's finish, or cut at a request (`lost`: the
/// request was made, its answer never came).
enum Run {
    Finished(Finish),
    Cut,
}

/// Runs a sync; `cut` is the number of requests answered before the power
/// goes, `arrives` whether the request then under way reached the server.
fn run_with(fake: &mut Fake, mode: Mode, cut: Option<usize>, arrives: bool) -> Run {
    let mut sync = Sync::default();
    let mut step = sync.start(fake, job(mode)).unwrap();
    let mut answered = 0;
    loop {
        match step {
            Step::Ended(finish) => return Run::Finished(finish),
            Step::Quiet => {
                if cut == Some(answered) {
                    if arrives {
                        fake.execute();
                    }
                    return Run::Cut;
                }
                let done = fake.execute();
                answered += 1;
                step = sync.done(fake, &done).unwrap();
            }
        }
    }
}

fn sync(fake: &mut Fake, mode: Mode) -> Finish {
    match run_with(fake, mode, None, false) {
        Run::Finished(finish) => finish,
        Run::Cut => unreachable!(),
    }
}

fn two() -> Fake {
    let mut fake = Fake::new();
    for (name, text) in [
        ("a.md", "A"),
        ("b.md", "B"),
        ("c.md", "C"),
        ("sub/d.md", "D"),
    ] {
        fake.write_card(name, text);
    }
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.problem, "");
    assert_eq!(finish.totals.up, 4);
    assert_eq!(fake.server("sub/d.md"), Some("D"));
    fake
}

#[test]
fn a_folder_is_sent_whole_and_made_on_the_server_and_a_second_sync_asks_nothing_more() {
    let mut fake = two();
    for name in ["a.md", "b.md", "c.md", "sub/d.md"] {
        assert_eq!(fake.server(name), fake.card(name));
    }
    assert!(fake.log.iter().any(|l| l == "MKCOL sub"));
    fake.log.clear();
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.totals, Totals::default());
    assert!(
        fake.log.iter().all(|l| l.starts_with("PROPFIND")),
        "{:?}",
        fake.log
    );
}

#[test]
fn a_change_on_one_side_goes_to_the_other() {
    let mut fake = two();
    fake.write_card("a.md", "A on the card");
    fake.write_server("c.md", "C on the server");
    fake.write_server("sub/new.md", "from the server");
    fake.write_card("fresh.md", "from the card");
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.totals.up, 2);
    assert_eq!(finish.totals.down, 2);
    assert_eq!(fake.server("a.md"), Some("A on the card"));
    assert_eq!(fake.card("c.md"), Some("C on the server"));
    assert_eq!(fake.card("sub/new.md"), Some("from the server"));
    assert_eq!(fake.server("fresh.md"), Some("from the card"));
    // Nothing is left to do.
    assert_eq!(sync(&mut fake, Mode::TwoWay).totals, Totals::default());
}

#[test]
fn a_file_changed_on_both_sides_is_kept_twice() {
    let mut fake = two();
    fake.write_card("b.md", "B on the card");
    fake.write_server("b.md", "B on the server");
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.totals.conflicts, 1);
    assert_eq!(fake.card("b.md"), Some("B on the server"));
    let copy = "b (conflict x4 2026-11-18).md";
    assert_eq!(fake.card(copy), Some("B on the card"));
    // The copy goes to the server at once, so both are there on both sides.
    assert_eq!(fake.server(copy), Some("B on the card"));
    assert_eq!(fake.server("b.md"), Some("B on the server"));
    assert_eq!(sync(&mut fake, Mode::TwoWay).totals, Totals::default());
}

#[test]
fn a_second_conflict_of_a_day_gets_a_number() {
    let mut fake = two();
    for round in 0..2 {
        fake.write_card("b.md", &format!("card {round}"));
        fake.write_server("b.md", &format!("server {round}"));
        assert_eq!(sync(&mut fake, Mode::TwoWay).totals.conflicts, 1);
    }
    assert_eq!(fake.card("b (conflict x4 2026-11-18).md"), Some("card 0"));
    assert_eq!(fake.card("b (conflict x4 2026-11-18 2).md"), Some("card 1"));
}

#[test]
fn a_file_never_synced_with_another_size_on_each_side_is_kept_twice_and_one_of_one_size_is_taken_as_the_same()
 {
    let mut fake = Fake::new();
    fake.write_card("same.md", "12345");
    fake.write_server("same.md", "abcde");
    fake.write_card("other.md", "card");
    fake.write_server("other.md", "the server's");
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.totals.conflicts, 1);
    assert_eq!(fake.card("same.md"), Some("12345"));
    assert_eq!(fake.server("same.md"), Some("abcde"));
    assert_eq!(fake.card("other.md"), Some("the server's"));
    assert_eq!(fake.card("other (conflict x4 2026-11-18).md"), Some("card"));
    assert_eq!(
        fake.server("other (conflict x4 2026-11-18).md"),
        Some("card")
    );
}

#[test]
fn deleted_on_one_side_and_unchanged_on_the_other_is_deleted_there_too() {
    let mut fake = two();
    fake.server.remove("a.md");
    fake.card.remove("Notes/b.md");
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.totals.deleted, 2);
    assert_eq!(fake.card("a.md"), None);
    assert_eq!(fake.server("b.md"), None);
    // The card's copy is in the trash, and the trash is listed.
    let trashed = fake.trashed();
    assert_eq!(trashed.len(), 1, "{trashed:?}");
    assert_eq!(fake.data.get(&trashed[0]).unwrap(), b"A");
    assert_eq!(state::trash(&fake.data).unwrap(), trashed);
}

#[test]
fn deleted_on_one_side_and_changed_on_the_other_is_kept() {
    let mut fake = two();
    // Deleted on the server, changed on the card: goes up again.
    fake.server.remove("a.md");
    fake.write_card("a.md", "A changed");
    // Deleted on the card, changed on the server: comes back.
    fake.card.remove("Notes/b.md");
    fake.write_server("b.md", "B changed");
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.totals.deleted, 0);
    assert_eq!(fake.server("a.md"), Some("A changed"));
    assert_eq!(fake.card("b.md"), Some("B changed"));
}

#[test]
fn download_only_never_changes_the_server() {
    let mut fake = two();
    fake.write_card("a.md", "A on the card");
    fake.write_card("local-only.md", "mine");
    fake.write_server("c.md", "C on the server");
    fake.server.remove("sub/d.md");
    fake.log.clear();
    let finish = sync(&mut fake, Mode::DownloadOnly);
    assert_eq!(finish.totals.up, 0);
    assert!(
        !fake
            .log
            .iter()
            .any(|l| l.starts_with("PUT") || l.starts_with("DELETE") || l.starts_with("MKCOL"))
    );
    assert_eq!(fake.server("a.md"), Some("A"));
    assert_eq!(fake.card("a.md"), Some("A on the card"));
    assert_eq!(fake.card("c.md"), Some("C on the server"));
    assert_eq!(fake.card("local-only.md"), Some("mine"));
    // Deleted on the server and unchanged on the card: gone from the card (to the trash).
    assert_eq!(fake.card("sub/d.md"), None);
    // What was deleted on the card comes again.
    fake.card.remove("Notes/b.md");
    sync(&mut fake, Mode::DownloadOnly);
    assert_eq!(fake.card("b.md"), Some("B"));
}

#[test]
fn a_server_that_answers_with_an_empty_folder_deletes_nothing() {
    let mut fake = Fake::new();
    for n in 0..8 {
        fake.write_card(&format!("n{n}.md"), &format!("note {n}"));
    }
    sync(&mut fake, Mode::TwoWay);
    fake.server.clear();
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.totals.deleted, 0);
    assert_eq!(finish.totals.held, 8);
    for n in 0..8 {
        // Kept on the card and sent again.
        assert_eq!(
            fake.card(&format!("n{n}.md")),
            Some(format!("note {n}").as_str())
        );
        assert_eq!(
            fake.server(&format!("n{n}.md")),
            Some(format!("note {n}").as_str())
        );
    }
    // A card that lost its files gets them back, nothing is deleted on the server.
    fake.card.clear();
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.totals.deleted, 0);
    assert_eq!(fake.card("n3.md"), Some("note 3"));
}

#[test]
fn the_trash_keeps_the_last_hundred() {
    let mut fake = Fake::new();
    // More files than a folder takes: a few folders.
    for d in 0..4 {
        for n in 0..30 {
            fake.write_card(&format!("d{d}/n{n:02}.md"), "x");
            fake.write_card(&format!("d{d}/keep{n:02}.md"), "keep");
        }
    }
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.totals.up, 240);
    // Four folders, each lost 30 of its 60 files on the server: more than half
    // is not (it is half), so they are deleted, a few at a time through the guard.
    for d in 0..4 {
        for n in 0..30 {
            fake.server.remove(&format!("d{d}/n{n:02}.md"));
        }
    }
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.totals.deleted, 120);
    let trash = state::trash(&fake.data).unwrap();
    assert_eq!(trash.len(), TRASH_MAX);
    assert_eq!(fake.trashed().len(), TRASH_MAX);
}

#[test]
fn a_cut_at_every_request_loses_nothing_and_the_next_sync_goes_on() {
    // The scenario: every kind of change at once.
    let make = || {
        let mut fake = two();
        fake.write_card("a.md", "A on the card");
        fake.write_card("b.md", "B on the card");
        fake.write_server("b.md", "B on the server");
        fake.write_server("c.md", "C on the server");
        fake.card.remove("Notes/sub/d.md");
        fake.write_card("e.md", "E new on the card");
        fake.write_server("sub/f.md", "F new on the server");
        fake
    };
    // What was written since the last sync, on either side, must not be lost.
    let must = [
        "A on the card",
        "B on the card",
        "B on the server",
        "C on the server",
        "E new on the card",
        "F new on the server",
    ];
    // How many requests an uninterrupted sync makes.
    let mut full = make();
    let mut count = 0;
    while let Run::Cut = run_with(&mut full, Mode::TwoWay, Some(count), false) {
        count += 1;
        full = make();
        assert!(count < 100);
    }
    assert!(count >= 8, "{count}");
    for cut in 0..count {
        for arrives in [false, true] {
            let mut fake = make();
            // A stray file the host left of a download that was cut.
            fake.write_card("c.md.pp-staged", "half");
            assert!(matches!(
                run_with(&mut fake, Mode::TwoWay, Some(cut), arrives),
                Run::Cut
            ));
            // The next sync, and one more: nothing is left to do.
            let finish = sync(&mut fake, Mode::TwoWay);
            assert_eq!(finish.problem, "", "cut {cut} arrives {arrives}");
            let again = sync(&mut fake, Mode::TwoWay);
            assert_eq!(
                again.totals,
                Totals::default(),
                "cut {cut} arrives {arrives}: {:?}",
                fake.log
            );
            // Everything that was anywhere is still somewhere.
            let after = fake.texts();
            for text in must {
                assert!(
                    after.contains(text),
                    "cut {cut} arrives {arrives}: lost {text:?}"
                );
            }
            // The stray file was no file of the pair.
            assert_eq!(fake.server("c.md.pp-staged"), None);
            // Both sides agree on every file.
            for (path, (text, _)) in &fake.server {
                assert_eq!(fake.card(path), Some(text.as_str()), "cut {cut}: {path}");
            }
        }
    }
}

#[test]
fn a_server_that_tells_no_etag_is_followed_by_size() {
    let mut fake = Fake::new();
    fake.etags = false;
    fake.write_card("a.md", "A");
    sync(&mut fake, Mode::TwoWay);
    assert_eq!(sync(&mut fake, Mode::TwoWay).totals, Totals::default());
    fake.write_server("a.md", "A longer");
    assert_eq!(sync(&mut fake, Mode::TwoWay).totals.down, 1);
    assert_eq!(fake.card("a.md"), Some("A longer"));
}

#[test]
fn one_file_that_fails_is_counted_and_the_rest_goes_on() {
    let mut fake = Fake::new();
    fake.write_card("a.md", "A");
    fake.write_card("b.md", "B");
    fake.broken.insert("a.md".into());
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.totals.errors, 1);
    assert_eq!(finish.totals.up, 1);
    assert_eq!(finish.problem, "file-sync.failed");
    assert_eq!(fake.server("b.md"), Some("B"));
    // The failed one is tried again next time.
    fake.broken.clear();
    assert_eq!(sync(&mut fake, Mode::TwoWay).totals.up, 1);
}

#[test]
fn a_card_that_refuses_the_move_keeps_both_files_where_they_are() {
    let mut fake = two();
    fake.write_card("b.md", "B on the card");
    fake.write_server("b.md", "B on the server");
    fake.no_rename = true;
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.totals.errors, 1);
    assert_eq!(fake.card("b.md"), Some("B on the card"));
    assert_eq!(fake.server("b.md"), Some("B on the server"));
}

#[test]
fn a_folder_with_more_files_than_a_sync_takes_is_left_alone() {
    let mut fake = Fake::new();
    for n in 0..=MOST {
        fake.write_card(&format!("n{n:03}.md"), "x");
    }
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.problem, "file-sync.too-many");
    assert!(fake.server.is_empty());
}

#[test]
fn a_missing_server_folder_that_was_synced_before_is_no_reason_to_empty_the_card() {
    let mut fake = two();
    fake.server.clear();
    fake.server_dirs.clear();
    fake.server_dirs.insert("x".into());
    let finish = sync(&mut fake, Mode::TwoWay);
    assert_eq!(finish.problem, "file-sync.not-found");
    assert_eq!(fake.card("a.md"), Some("A"));
}

#[test]
fn the_network_going_ends_the_sync_and_the_next_one_goes_on() {
    let mut fake = Fake::new();
    fake.write_card("a.md", "A");
    let mut sync_ = Sync::default();
    sync_.start(&mut fake, job(Mode::TwoWay)).unwrap();
    // The listing is answered; the first send is lost with the network.
    let listed = fake.execute();
    assert!(matches!(
        sync_.done(&mut fake, &listed).unwrap(),
        Step::Quiet
    ));
    let lost = Done {
        end: End::Network,
        status: 0,
        saved: false,
        version: 0,
    };
    match sync_.done(&mut fake, &lost).unwrap() {
        Step::Ended(finish) => {
            assert!(finish.stopped);
            assert_eq!(finish.problem, "file-sync.no-network");
        }
        Step::Quiet => panic!("not ended"),
    }
    assert!(!sync_.running());
    assert_eq!(sync(&mut fake, Mode::TwoWay).totals.up, 1);
}

#[test]
fn the_look_counts_what_changed_on_the_card_since_the_last_sync() {
    let mut fake = two();
    assert_eq!(
        crate::native_look::pending(&mut fake, 1, "Notes").unwrap(),
        0
    );
    fake.write_card("a.md", "changed");
    fake.write_card("new.md", "new");
    fake.card.remove("Notes/c.md");
    fake.write_card("sub/e.md", "new in a folder");
    assert_eq!(
        crate::native_look::pending(&mut fake, 1, "Notes").unwrap(),
        4
    );
    sync(&mut fake, Mode::TwoWay);
    assert_eq!(
        crate::native_look::pending(&mut fake, 1, "Notes").unwrap(),
        0
    );
}
