//! The requests of the page, over an in-memory card.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use alloc::string::ToString;
use alloc::vec::Vec;
use paperpaw_sdk::served::{LOCKED_FOR, WRONG_KEYS};

/// The card as the host shows it. The files of a book format are books: they
/// go through the library, which a log tells (`library.move`, `files.move`,
/// ...), and `fail` is what the host refuses a change with.
#[derive(Default)]
struct Fake {
    entries: Vec<(String, bool, u64)>,
    free: Option<u64>,
    puts: Vec<(Option<String>, String)>,
    log: Vec<String>,
    fail: Option<Error>,
}

/// The fixtures and the log are the test's, not the code's allocations.
fn kept<T>(build: impl FnOnce() -> T) -> T {
    crate::fail_alloc::exempt(build)
}

fn is_book(path: &str) -> bool {
    files::has_extension(path, "epub")
}

/// The library's folder, as the card's setting says.
const LIBRARY: &str = "Books";

impl Fake {
    fn with(entries: &[(&str, bool, u64)]) -> Self {
        Self {
            entries: entries
                .iter()
                .map(|(path, dir, size)| (path.to_string(), *dir, *size))
                .collect(),
            free: Some(1 << 30),
            ..Self::default()
        }
    }

    fn has(&self, path: &str) -> bool {
        self.entries.iter().any(|(at, ..)| at == path)
    }

    fn refuse(&self) -> Result<(), Error> {
        self.fail.map_or(Ok(()), Err)
    }
}

impl Card for Fake {
    fn stat(&mut self, path: &str) -> Result<Option<(bool, u64)>, Error> {
        // The host refuses what is not the user's.
        if path.starts_with("plugins") {
            return Err(Error::Invalid);
        }
        Ok(self
            .entries
            .iter()
            .find(|(at, ..)| at.eq_ignore_ascii_case(path))
            .map(|(_, dir, size)| (*dir, *size)))
    }
    fn list(
        &mut self,
        dir: &str,
        take: &mut dyn FnMut(&str, bool, u64) -> Result<bool, Error>,
    ) -> Result<(), Error> {
        for (path, is_dir, size) in &self.entries {
            if files::parent(path) == dir && !take(files::name(path), *is_dir, *size)? {
                break;
            }
        }
        Ok(())
    }
    fn make_dir(&mut self, path: &str) -> Result<(), Error> {
        kept(|| self.entries.push((path.to_string(), true, 0)));
        Ok(())
    }
    fn remove(&mut self, path: &str, is_dir: bool) -> Result<(), Error> {
        self.refuse()?;
        let book = !is_dir && is_book(path);
        kept(|| {
            self.log.push(alloc::format!(
                "{} {path}",
                if book {
                    "library.delete"
                } else {
                    "files.remove"
                }
            ));
            let below = alloc::format!("{path}/");
            self.entries
                .retain(|(at, ..)| at != path && !at.starts_with(&below));
        });
        Ok(())
    }
    fn move_to(&mut self, from: &str, to: &str, is_dir: bool) -> Result<(), Error> {
        self.refuse()?;
        if self.has(to) {
            return Err(Error::State);
        }
        let book = !is_dir && is_book(from);
        kept(|| {
            self.log.push(alloc::format!(
                "{} {from} -> {to}",
                if book { "library.move" } else { "files.move" }
            ));
            let below = alloc::format!("{from}/");
            for (at, ..) in &mut self.entries {
                if at == from {
                    *at = to.to_string();
                } else if let Some(rest) = at.strip_prefix(&below) {
                    *at = alloc::format!("{to}/{rest}");
                }
            }
        });
        Ok(())
    }
    fn free(&mut self) -> Result<Option<u64>, Error> {
        Ok(self.free)
    }
    fn put(&mut self, body: Option<&str>, path: &str) -> Result<String, Error> {
        self.refuse()?;
        let book = body.is_some()
            && is_book(path)
            && files::parent(path)
                .get(..LIBRARY.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(LIBRARY));
        kept(|| {
            self.log.push(alloc::format!(
                "{} {path}",
                if book { "library.import" } else { "files.put" }
            ));
            self.puts.push((body.map(str::to_string), path.to_string()));
            self.entries.push((path.to_string(), false, 1));
            Ok(path.to_string())
        })
    }
}

fn ask(card: &mut Fake, guard: &mut Guard, method: &str, target: &str) -> Outcome {
    let request = Request {
        method,
        target,
        body: Some(".received/1/7"),
        body_len: 10,
        at: Instant(0),
    };
    reply(card, guard, &request)
}

/// Answers with no plugins to install (the card's requests only).
fn reply(card: &mut Fake, guard: &mut Guard, request: &Request<'_>) -> Outcome {
    let mut plugins = crate::native_install::tests::Fake::default();
    let mut installs = Installs::default();
    let mut parts = Parts {
        card,
        plugins: &mut plugins,
        installs: &mut installs,
    };
    answer(&mut parts, guard, request)
}

fn run(card: &mut Fake, method: &str, target: &str) -> Outcome {
    ask(card, &mut Guard::new("key".to_string()), method, target)
}

fn json(answer: &Answer) -> &str {
    match &answer.body {
        Body::Owned(json) => json,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_page_is_served_with_the_key_and_not_without() {
    let mut card = Fake::default();
    let (page, done) = run(&mut card, "GET", "/?k=key");
    assert_eq!(
        (page.status, page.body, done),
        (200, Body::Data(PAGE_FILE), None)
    );
    let (refused, _) = run(&mut card, "GET", "/");
    assert_eq!(refused.status, 403);
    let (refused, _) = run(&mut card, "GET", "/?k=wrong");
    assert_eq!(refused.status, 403);
    // The icon is not a guess at the key.
    let (icon, _) = run(&mut card, "GET", "/favicon.ico");
    assert_eq!(icon.status, 404);
}

#[test]
fn a_few_wrong_keys_lock_even_the_right_one_for_a_while() {
    let mut card = Fake::default();
    let mut guard = Guard::new("key".to_string());
    for _ in 0..WRONG_KEYS {
        assert_eq!(ask(&mut card, &mut guard, "GET", "/?k=no").0.status, 403);
    }
    assert_eq!(ask(&mut card, &mut guard, "GET", "/?k=key").0.status, 429);
    let later = Request {
        method: "GET",
        target: "/?k=key",
        body: None,
        body_len: 0,
        at: Instant(LOCKED_FOR + 1),
    };
    assert_eq!(reply(&mut card, &mut guard, &later).0.status, 200);
}

#[test]
fn the_listing_leaves_out_hidden_names_and_goes_on_in_parts() {
    let mut card = Fake::with(&[
        ("a.epub", false, 5),
        (".hidden", false, 1),
        ("Books", true, 0),
    ]);
    let (list, done) = run(&mut card, "GET", "/api/list?k=key");
    assert_eq!(
        json(&list),
        "{\"entries\":[{\"name\":\"a.epub\",\"dir\":false,\"size\":5},{\"name\":\"Books\",\"dir\":true,\"size\":0}]}"
    );
    assert_eq!(done, None);
    // A folder of many: every answer fits an inline body, and `next` goes on.
    let many: Vec<(String, bool, u64)> = (0..200)
        .map(|i| {
            (
                alloc::format!("a book with a long name {i:03}.epub"),
                false,
                9,
            )
        })
        .collect();
    let mut card = Fake {
        entries: many,
        ..Fake::default()
    };
    let (mut from, mut seen) = (0, 0);
    loop {
        let target = alloc::format!("/api/list?k=key&from={from}");
        let (list, _) = run(&mut card, "GET", &target);
        let json = json(&list);
        assert!(json.len() <= INLINE, "{}", json.len());
        seen += json.matches("\"name\"").count();
        match json.split_once("\"next\":") {
            Some((_, next)) => from = next.trim_end_matches('}').parse().unwrap(),
            None => break,
        }
    }
    assert_eq!(seen, 200);
}

#[test]
fn a_missing_folder_and_a_file_are_not_a_listing() {
    let mut card = Fake::with(&[("a.epub", false, 5)]);
    assert_eq!(
        run(&mut card, "GET", "/api/list?k=key&path=nope").0.status,
        404
    );
    assert_eq!(
        run(&mut card, "GET", "/api/list?k=key&path=a.epub")
            .0
            .status,
        404
    );
}

#[test]
fn a_file_is_sent_whole_from_the_card() {
    let mut card = Fake::with(&[("Books", true, 0), ("Books/a b.epub", false, 5)]);
    let (file, done) = run(&mut card, "GET", "/api/file?k=key&path=Books%2Fa%20b.epub");
    assert_eq!(file.status, 200);
    assert_eq!(file.body, Body::File("Books/a b.epub".to_string()));
    assert_eq!(done, Some((Done::Sent, "a b.epub".to_string())));
    assert_eq!(
        run(&mut card, "GET", "/api/file?k=key&path=Books").0.status,
        404
    );
}

#[test]
fn paths_that_are_not_plain_are_refused() {
    let mut card = Fake::with(&[("a.epub", false, 5)]);
    for path in [
        "..%2Fx",
        ".paperpaw",
        "Books%2F..%2Fa.epub",
        "a.epub.",
        "a%00b",
        "a%5Cb",
    ] {
        let target = alloc::format!("/api/file?k=key&path={path}");
        assert_eq!(run(&mut card, "GET", &target).0.status, 400, "{path}");
    }
    // `plugins` is the host's: it refuses it by identity.
    assert_eq!(
        run(&mut card, "GET", "/api/list?k=key&path=plugins")
            .0
            .status,
        400
    );
    // Bytes that are not UTF-8.
    assert_eq!(
        run(&mut card, "GET", "/api/list?k=key&path=%ff").0.status,
        400
    );
}

#[test]
fn an_upload_goes_to_a_free_name_and_is_told() {
    let mut card = Fake::with(&[("Books", true, 0), ("Books/a.epub", false, 5)]);
    let (done_ok, done) = run(&mut card, "PUT", "/api/file?k=key&path=Books%2Fa.epub");
    assert_eq!(done_ok.status, 200);
    assert_eq!(done, Some((Done::Received, "a (2).epub".to_string())));
    assert_eq!(
        card.puts,
        [(
            Some(".received/1/7".to_string()),
            "Books/a (2).epub".to_string()
        )]
    );
}

#[test]
fn an_upload_that_does_not_fit_or_has_no_folder_is_refused_and_told() {
    let mut card = Fake::with(&[]);
    card.free = Some(9);
    let (refused, done) = run(&mut card, "PUT", "/api/file?k=key&path=big.epub");
    assert_eq!(refused.status, 507);
    assert_eq!(done, Some((Done::NotReceived, "big.epub".to_string())));
    card.free = None;
    // Free room not known: the card says when it is full.
    assert_eq!(
        run(&mut card, "PUT", "/api/file?k=key&path=ok.epub")
            .0
            .status,
        200
    );
    let (refused, done) = run(&mut card, "PUT", "/api/file?k=key&path=none%2Fa.epub");
    assert_eq!(refused.status, 400);
    assert_eq!(done, Some((Done::NotReceived, "a.epub".to_string())));
    // Nothing is put at the top.
    assert_eq!(run(&mut card, "PUT", "/api/file?k=key").0.status, 400);
}

/// A card from Beta 3 can hold plain secrets in the file until its first load
/// moves them out: it is not served, in any case, whatever else is.
#[test]
fn the_settings_are_not_served() {
    let mut card = Fake::with(&[("paperpaw.ini", false, 5), ("a.epub", false, 5)]);
    for path in ["paperpaw.ini", "PaperPaw.INI"] {
        let (refused, done) = run(&mut card, "GET", &["/api/file?k=key&path=", path].concat());
        assert_eq!((refused.status, done), (403, None), "{path}");
    }
    let (ok, _) = run(&mut card, "GET", "/api/file?k=key&path=a.epub");
    assert_eq!(ok.status, 200);
}

#[test]
fn the_settings_cannot_be_put_made_or_deleted_over() {
    let mut card = Fake::with(&[("paperpaw.ini", false, 5)]);
    let (refused, done) = run(&mut card, "POST", "/api/delete?k=key&path=paperpaw.ini");
    assert_eq!((refused.status, done), (403, None));
    // Taken, so a new file gets another name, and the settings stay.
    let (ok, done) = run(&mut card, "PUT", "/api/file?k=key&path=paperpaw.ini");
    assert_eq!(ok.status, 200);
    assert_eq!(done, Some((Done::Received, "paperpaw (2).ini".to_string())));
    // Any case, as the card takes it.
    let mut card = Fake::with(&[]);
    let (refused, done) = run(&mut card, "PUT", "/api/file?k=key&path=PAPERPAW.INI");
    assert_eq!(
        (refused.status, done.map(|(d, _)| d)),
        (403, Some(Done::NotReceived))
    );
    assert_eq!(
        run(&mut card, "POST", "/api/mkdir?k=key&path=PaperPaw.ini")
            .0
            .status,
        403
    );
}

#[test]
fn a_folder_is_made_in_a_free_place_and_deleting_takes_what_is_in_it() {
    let mut card = Fake::with(&[("Books", true, 0), ("Books/a.epub", false, 5)]);
    let (ok, done) = run(&mut card, "POST", "/api/mkdir?k=key&path=Books");
    assert_eq!(ok.status, 200);
    assert_eq!(done, Some((Done::Folder, "Books (2)".to_string())));
    let (ok, done) = run(&mut card, "POST", "/api/delete?k=key&path=Books");
    assert_eq!(
        (ok.status, done),
        (200, Some((Done::Deleted, "Books".to_string())))
    );
    assert!(
        card.entries
            .iter()
            .all(|(at, ..)| !at.starts_with("Books/"))
    );
    // Not everything at once, and not what is not there.
    assert_eq!(run(&mut card, "POST", "/api/delete?k=key").0.status, 404);
    assert_eq!(
        run(&mut card, "POST", "/api/delete?k=key&path=nope")
            .0
            .status,
        404
    );
}

#[test]
fn the_key_is_asked_for_first_and_unknown_requests_are_not_found() {
    let mut card = Fake::with(&[("a.epub", false, 5)]);
    assert_eq!(
        run(&mut card, "POST", "/api/move?path=a.epub&to=").0.status,
        403
    );
    assert_eq!(run(&mut card, "GET", "/api/plugins").0.status, 403);
    assert_eq!(run(&mut card, "GET", "/api/other?k=key").0.status, 404);
    assert_eq!(card.entries.len(), 1);
}

fn mv(card: &mut Fake, target: &str) -> Outcome {
    run(card, "POST", &alloc::format!("/api/move?k=key&{target}"))
}

fn moved(card: &mut Fake, target: &str, name: &str) {
    let (answer, done) = mv(card, target);
    assert_eq!(answer.status, 200, "{target}");
    assert_eq!(done, Some((Done::Moved, name.to_string())), "{target}");
}

#[test]
fn a_file_is_moved_to_a_folder_and_renamed() {
    let mut card = Fake::with(&[
        ("Notes", true, 0),
        ("Notes/old.txt", false, 5),
        ("a.txt", false, 3),
    ]);
    moved(&mut card, "path=a.txt&to=Notes", "a.txt");
    assert!(card.has("Notes/a.txt") && !card.has("a.txt"));
    // Back to the top, under a new name: a rename.
    moved(
        &mut card,
        "path=Notes%2Fa.txt&to=&name=b%20c.txt",
        "b c.txt",
    );
    assert!(card.has("b c.txt") && !card.has("Notes/a.txt"));
    // Renamed where it is.
    moved(&mut card, "path=b%20c.txt&to=&name=d.txt", "d.txt");
    // Not a book: the card's move, which the host's library follows.
    assert_eq!(
        card.log,
        [
            "files.move a.txt -> Notes/a.txt",
            "files.move Notes/a.txt -> b c.txt",
            "files.move b c.txt -> d.txt"
        ]
    );
}

#[test]
fn a_book_is_moved_by_the_library_with_all_that_is_kept_for_it() {
    let mut card = Fake::with(&[("Books", true, 0), ("Books/a.epub", false, 5)]);
    moved(&mut card, "path=Books%2Fa.epub&to=&name=b.epub", "b.epub");
    moved(&mut card, "path=b.epub&to=Books", "b.epub");
    assert_eq!(
        card.log,
        [
            "library.move Books/a.epub -> b.epub",
            "library.move b.epub -> Books/b.epub"
        ]
    );
}

#[test]
fn a_folder_is_moved_whole_by_the_card_whatever_is_in_it() {
    let mut card = Fake::with(&[
        ("Books", true, 0),
        ("Books/Old", true, 0),
        ("Books/Old/a.epub", false, 5),
        ("Shelf", true, 0),
    ]);
    moved(&mut card, "path=Books%2FOld&to=Shelf", "Old");
    assert!(card.has("Shelf/Old/a.epub") && !card.has("Books/Old"));
    // The host's library follows a folder; no book is asked about.
    assert_eq!(card.log, ["files.move Books/Old -> Shelf/Old"]);
}

#[test]
fn nothing_is_moved_over_what_is_there() {
    let mut card = Fake::with(&[
        ("a.epub", false, 5),
        ("Books", true, 0),
        ("Books/a.epub", false, 6),
        ("Books/a (2).epub", false, 7),
        ("Dir", true, 0),
        ("Books/Dir", true, 0),
    ]);
    moved(&mut card, "path=a.epub&to=Books", "a (3).epub");
    assert!(card.has("Books/a (3).epub") && card.has("Books/a.epub"));
    // A folder over a folder; a rename to a taken name.
    moved(&mut card, "path=Dir&to=Books", "Dir (2)");
    moved(
        &mut card,
        "path=Books%2Fa.epub&to=Books&name=A%20%282%29.EPUB",
        "A (2) (2).EPUB",
    );
    assert!(card.has("Books/A (2) (2).EPUB") && card.has("Books/a (3).epub"));
}

#[test]
fn the_same_place_is_no_change() {
    let mut card = Fake::with(&[("Books", true, 0), ("Books/a.epub", false, 5)]);
    for target in [
        "path=Books%2Fa.epub&to=Books",
        "path=Books%2Fa.epub&to=books&name=A.EPUB",
        "path=Books%2Fa.epub&to=Books%2F&name=a.epub",
        "path=Books&to=",
    ] {
        let (answer, done) = mv(&mut card, target);
        assert_eq!((answer.status, done), (200, None), "{target}");
    }
    assert!(card.log.is_empty());
}

#[test]
fn a_folder_never_goes_into_itself() {
    let mut card = Fake::with(&[
        ("A", true, 0),
        ("A/B", true, 0),
        ("A/B/C", true, 0),
        ("AB", true, 0),
    ]);
    for target in [
        "path=A&to=A%2FB%2FC",
        "path=A&to=A%2FB",
        "path=A%2FB&to=a%2Fb%2FC",
    ] {
        assert_eq!(mv(&mut card, target).0.status, 400, "{target}");
    }
    // Another folder that starts the same is another folder.
    moved(&mut card, "path=A&to=AB", "A");
    assert!(card.has("AB/A/B/C"));
    assert_eq!(card.log.len(), 1);
}

#[test]
fn the_settings_are_neither_moved_nor_a_target() {
    let mut card = Fake::with(&[
        ("paperpaw.ini", false, 5),
        ("Books", true, 0),
        ("Books/a.txt", false, 1),
    ]);
    for target in [
        "path=paperpaw.ini&to=Books",
        "path=PaperPaw.INI&to=&name=x.ini",
        "path=PaperPaw.INI&to=Books&name=x.ini",
    ] {
        let (answer, done) = mv(&mut card, target);
        assert_eq!((answer.status, done), (403, None), "{target}");
    }
    // Taken: the free name is not the settings.
    moved(
        &mut card,
        "path=Books%2Fa.txt&to=&name=PAPERPAW.INI",
        "PAPERPAW (2).INI",
    );
    assert!(card.has("paperpaw.ini"));
    // Not there (a card without it): nothing is moved to be it.
    let mut card = Fake::with(&[("Books", true, 0), ("Books/a.txt", false, 1)]);
    let (answer, done) = mv(&mut card, "path=Books%2Fa.txt&to=&name=PAPERPAW.INI");
    assert_eq!((answer.status, done), (403, None));
    assert!(card.has("Books/a.txt") && card.log.is_empty());
}

#[test]
fn the_target_must_be_a_folder_that_is_there_and_the_source_something_there() {
    let mut card = Fake::with(&[("a.txt", false, 1), ("b.txt", false, 1), ("Books", true, 0)]);
    for (target, status) in [
        ("path=a.txt&to=none", 404),
        ("path=a.txt&to=b.txt", 404),
        ("path=none&to=Books", 404),
        // Not everything at once.
        ("to=Books", 404),
        ("path=&to=Books", 404),
        // The folder is not made on the way.
        ("path=a.txt&to=Books%2Fnew", 404),
    ] {
        assert_eq!(mv(&mut card, target).0.status, status, "{target}");
    }
    assert!(card.log.is_empty());
}

#[test]
fn hostile_names_and_paths_are_refused_before_the_card_is_asked() {
    let mut card = Fake::with(&[("a.txt", false, 1), ("Books", true, 0)]);
    for target in [
        "path=a.txt&to=&name=..",
        "path=a.txt&to=&name=..%2Fx",
        "path=a.txt&to=&name=x%2Fy",
        "path=a.txt&to=&name=x%5Cy",
        "path=a.txt&to=&name=.hidden",
        "path=a.txt&to=&name=x.",
        "path=a.txt&to=&name=%20x",
        "path=a.txt&to=&name=a%00b",
        "path=a.txt&to=&name=a%3Ab",
        "path=a.txt&to=&name=",
        "path=a.txt&to=&name=%ff",
        "path=..%2Fa.txt&to=",
        "path=a.txt&to=..",
        "path=a.txt&to=Books%2F..%2F..",
        "path=a.txt&to=.paperpaw",
        "path=.paperpaw&to=Books",
        "path=a.txt&to=plugins",
        "path=plugins&to=Books",
    ] {
        assert_eq!(mv(&mut card, target).0.status, 400, "{target}");
    }
    assert!(card.log.is_empty());
    assert!(card.has("a.txt"));
}

#[test]
fn what_the_host_refuses_a_change_with_is_said() {
    for (error, status) in [
        (Error::Busy, 409),
        (Error::State, 409),
        (Error::Denied, 403),
        (Error::Full, 507),
        (Error::NoMemory, 503),
        (Error::Io, 500),
    ] {
        let mut card = Fake::with(&[("a.epub", false, 1), ("b.txt", false, 1), ("D", true, 0)]);
        card.fail = Some(error);
        for target in [
            "/api/move?k=key&path=a.epub&to=D",
            "/api/move?k=key&path=b.txt&to=D",
            "/api/move?k=key&path=D&to=&name=E",
            "/api/delete?k=key&path=a.epub",
            "/api/delete?k=key&path=D",
        ] {
            let (answer, done) = run(&mut card, "POST", target);
            assert_eq!((answer.status, done), (status, None), "{error:?} {target}");
        }
        // The host's own answers are said as plain words.
        let (answer, done) = run(&mut card, "PUT", "/api/file?k=key&path=x.epub");
        assert_eq!(answer.status, status, "{error:?}");
        assert_eq!(done, Some((Done::NotReceived, "x.epub".to_string())));
        assert_eq!(card.entries.len(), 3);
    }
    // A move onto a name taken meanwhile is the host's `State`.
    let mut card = Fake::with(&[("a.txt", false, 1), ("D", true, 0), ("D/a.txt", false, 1)]);
    assert_eq!(card.move_to("a.txt", "D/a.txt", false), Err(Error::State));
    assert_eq!(Fail::from(Error::Busy).1, "busy, try again");
}

#[test]
fn deleting_a_book_goes_through_the_library_and_a_folder_through_the_card() {
    let mut card = Fake::with(&[
        ("Books", true, 0),
        ("Books/a.epub", false, 5),
        ("Books/b.txt", false, 5),
        ("Books/Old", true, 0),
        ("Books/Old/c.epub", false, 5),
    ]);
    for (path, name) in [
        ("Books%2Fa.epub", "a.epub"),
        ("Books%2Fb.txt", "b.txt"),
        ("Books%2FOld", "Old"),
    ] {
        let (answer, done) = run(
            &mut card,
            "POST",
            &alloc::format!("/api/delete?k=key&path={path}"),
        );
        assert_eq!(
            (answer.status, done),
            (200, Some((Done::Deleted, name.to_string())))
        );
    }
    assert_eq!(
        card.log,
        [
            "library.delete Books/a.epub",
            "files.remove Books/b.txt",
            "files.remove Books/Old"
        ]
    );
    assert_eq!(card.entries.len(), 1);
}

#[test]
fn a_book_sent_into_the_library_folder_is_imported_and_others_are_put() {
    let mut card = Fake::with(&[
        ("Books", true, 0),
        ("Books/Sub", true, 0),
        ("Other", true, 0),
    ]);
    for (path, name) in [
        ("Books%2Fa.epub", "a.epub"),
        ("Books%2FSub%2Fb.epub", "b.epub"),
        ("Other%2Fc.epub", "c.epub"),
        ("Books%2Fd.txt", "d.txt"),
    ] {
        let (answer, done) = run(
            &mut card,
            "PUT",
            &alloc::format!("/api/file?k=key&path={path}"),
        );
        assert_eq!(
            (answer.status, done),
            (200, Some((Done::Received, name.to_string())))
        );
    }
    assert_eq!(
        card.log,
        [
            "library.import Books/a.epub",
            "library.import Books/Sub/b.epub",
            "files.put Other/c.epub",
            "files.put Books/d.txt"
        ]
    );
}

/// Every refusal of memory is an answer, never an abort, and a change is
/// whole or not made.
#[test]
fn memory_that_runs_short_is_refused_not_aborted() {
    let requests = [
        ("GET", "/api/list?k=key&path=Books"),
        (
            "POST",
            "/api/move?k=key&path=Books%2Fa.epub&to=Shelf&name=b.epub",
        ),
        ("POST", "/api/move?k=key&path=Books%2Fa.txt&to=Shelf"),
        ("POST", "/api/move?k=key&path=Shelf&to=Books"),
        ("POST", "/api/move?k=key&path=Books%2Fa.epub&to=Books"),
        ("POST", "/api/delete?k=key&path=Books%2Fa.epub"),
        ("POST", "/api/delete?k=key&path=Shelf"),
        ("POST", "/api/mkdir?k=key&path=Shelf"),
        ("PUT", "/api/file?k=key&path=Books%2Fa.epub"),
    ];
    for (method, target) in requests {
        crate::fail_alloc::sweep(|| {
            let (mut card, mut guard) = kept(|| {
                let card = Fake::with(&[
                    ("Books", true, 0),
                    ("Books/a.epub", false, 5),
                    ("Books/a.txt", false, 5),
                    ("Shelf", true, 0),
                ]);
                (card, Guard::new("key".to_string()))
            });
            let before = card.entries.len();
            let (answer, _) = ask(&mut card, &mut guard, method, target);
            match answer.status {
                200 => Ok(()),
                503 => {
                    // Nothing half done: a refused change is no change.
                    assert!(card.log.is_empty() && card.entries.len() == before);
                    Err(Error::NoMemory)
                }
                other => panic!("{method} {target}: {other}"),
            }
        });
    }
}

#[test]
fn json_text_is_safe_inside_a_script() {
    let mut out = String::new();
    push_json(&mut out, "a\"b\\c</script>\n\u{e9}").unwrap();
    assert_eq!(out, "\"a\\\"b\\\\c\\u003c/script>\\u000a\u{e9}\"");
}

#[test]
fn the_query_is_read_as_the_sdk_reads_it() {
    for (query, name) in [
        ("a=1&b=x%20y+z", "b"),
        ("a=%41%4", "a"),
        ("k=abc&path=%C3%A9", "path"),
        ("flag&x=1", "flag"),
        ("x=1", "none"),
    ] {
        assert_eq!(
            param(query, name).unwrap(),
            paperpaw_sdk::served::param(query, name),
            "{query}"
        );
    }
}
