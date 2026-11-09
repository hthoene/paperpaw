//! The requests of the page, over an in-memory card.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use alloc::string::ToString;
use alloc::vec::Vec;
use paperpaw_sdk::served::{LOCKED_FOR, WRONG_KEYS};

#[derive(Default)]
struct Fake {
    entries: Vec<(String, bool, u64)>,
    free: Option<u64>,
    puts: Vec<(Option<String>, String)>,
}

impl Fake {
    fn with(entries: &[(&str, bool, u64)]) -> Self {
        Self {
            entries: entries
                .iter()
                .map(|(path, dir, size)| (path.to_string(), *dir, *size))
                .collect(),
            free: Some(1 << 30),
            puts: Vec::new(),
        }
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
        self.entries.push((path.to_string(), true, 0));
        Ok(())
    }
    fn remove(&mut self, path: &str) -> Result<(), Error> {
        let below = alloc::format!("{path}/");
        self.entries
            .retain(|(at, ..)| at != path && !at.starts_with(&below));
        Ok(())
    }
    fn free(&mut self) -> Result<Option<u64>, Error> {
        Ok(self.free)
    }
    fn put(&mut self, body: Option<&str>, path: &str) -> Result<(), Error> {
        self.puts.push((body.map(str::to_string), path.to_string()));
        self.entries.push((path.to_string(), false, 1));
        Ok(())
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
    answer(card, guard, &request)
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
    assert_eq!((page.status, page.body, done), (200, Body::Page, None));
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
    assert_eq!(answer(&mut card, &mut guard, &later).0.status, 200);
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
fn moving_and_installing_plugins_are_said_to_be_missing() {
    let mut card = Fake::with(&[("a.epub", false, 5)]);
    for (method, target) in [
        ("POST", "/api/move?k=key&path=a.epub&to="),
        ("GET", "/api/plugins?k=key"),
        ("PUT", "/api/plugin/x/y?k=key"),
        ("POST", "/api/plugin/x?k=key"),
    ] {
        assert_eq!(run(&mut card, method, target).0.status, 501, "{target}");
    }
    // The key is asked for first.
    assert_eq!(run(&mut card, "GET", "/api/plugins").0.status, 403);
    assert_eq!(run(&mut card, "GET", "/api/other?k=key").0.status, 404);
    assert_eq!(card.entries.len(), 1);
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
