#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use crate::native_model::Entry;
use alloc::vec::Vec;
use paperpaw_sdk::MemoryStorage;

fn sha256(data: &[u8]) -> Digest {
    Digest::Sha256(paperpaw_sdk::digest::sha256::hex(data))
}

fn file(path: &str, body: &[u8]) -> File {
    File {
        path: path.into(),
        url: alloc::format!("https://lib.example/p/{path}"),
        size: body.len() as u64,
        digest: sha256(body),
    }
}

fn page(id: &str, files: Vec<File>) -> Page {
    let entry = Entry {
        id: id.into(),
        name: "Airy\ttheme".into(),
        ..Entry::default()
    };
    Page {
        entry,
        files,
        code: false,
        author_key: None,
        recovery_key: None,
        rotations: Vec::new(),
        more: crate::native_item::More::default(),
    }
}

/// The network of a test: requests are recorded, the answers written by hand.
struct Fake {
    storage: MemoryStorage,
    asked: Vec<(String, String, u64)>,
    cancelled: usize,
    next: u32,
    refuse: Option<Error>,
}

impl Fake {
    fn new() -> Self {
        Self {
            storage: MemoryStorage::new(),
            asked: Vec::new(),
            cancelled: 0,
            next: 1,
            refuse: None,
        }
    }
    fn answer(&mut self, body: &[u8]) -> Done {
        let (_, file, _) = self.asked.last().unwrap().clone();
        self.storage.write(&file, body).unwrap();
        Done::new(
            RequestId::new(self.next - 1).unwrap(),
            FILE_REQUEST,
            Reason::Ok,
            true,
        )
    }
}

impl Wire for Fake {
    fn storage(&self) -> &dyn Storage {
        &self.storage
    }
    fn get(
        &mut self,
        url: &str,
        file: &str,
        token: u32,
        max_bytes: u64,
    ) -> Result<RequestId, Error> {
        assert_eq!(token, FILE_REQUEST);
        if let Some(error) = self.refuse {
            return Err(error);
        }
        self.asked.push((url.into(), file.into(), max_bytes));
        self.next += 1;
        Ok(RequestId::new(self.next - 1).unwrap())
    }
    fn cancel(&mut self, token: u32) {
        assert_eq!(token, FILE_REQUEST);
        self.cancelled += 1;
    }
}

const INI: &[u8] = b"id = airy-theme\nname = Airy\n";
const DE: &[u8] = b"hello = Hallo\n";

fn two_files() -> Page {
    page(
        "airy-theme",
        alloc::vec![file("plugin.ini", INI), file("i18n/de.ini", DE)],
    )
}

#[test]
fn the_files_come_one_after_the_other_each_checked() {
    let mut wire = Fake::new();
    let mut download = Download::new();
    assert_eq!(
        download.start(&mut wire, &two_files()).unwrap(),
        Started::Downloading
    );
    assert_eq!(download.step(), Step::Downloading { done: 0, total: 2 });
    // The first goes to the staging folder, its size the most the host takes.
    assert_eq!(
        wire.asked[0],
        (
            "https://lib.example/p/plugin.ini".into(),
            "stage/airy-theme/plugin.ini".into(),
            INI.len() as u64
        )
    );
    let progress = Download::progress(&wire.storage).unwrap().unwrap();
    assert_eq!(
        (
            progress.plugin.as_str(),
            progress.name.as_str(),
            progress.done,
            progress.total
        ),
        ("airy-theme", "Airy theme", 0, 2)
    );
    let done = wire.answer(INI);
    assert_eq!(
        download.ended(&mut wire, &done).unwrap(),
        Step::Downloading { done: 1, total: 2 }
    );
    assert_eq!(wire.asked.len(), 2);
    assert_eq!(wire.asked[1].1, "stage/airy-theme/i18n/de.ini");
    let done = wire.answer(DE);
    assert_eq!(download.ended(&mut wire, &done).unwrap(), Step::Ready);
    // Handed to the install: the job is forgotten, the files stay.
    let staged = download.take_ready(&wire.storage).unwrap().unwrap();
    assert_eq!(staged.plugin, "airy-theme");
    assert_eq!(staged.folder, "stage/airy-theme");
    assert_eq!(download.step(), Step::Installing);
    assert_eq!(Download::progress(&wire.storage).unwrap(), None);
    assert!(wire.storage.exists("stage/airy-theme/i18n/de.ini"));
    assert_eq!(download.take_ready(&wire.storage).unwrap(), None);
}

#[test]
fn a_file_that_is_not_what_the_library_lists_stops_it_and_goes_away() {
    // Another size, another digest of the same size, no 2xx answer, a
    // connection that went, a card that is full.
    for (body, reason, saved, want) in [
        (&b"short"[..], Reason::Ok, true, Failure::Damaged),
        (
            &b"id = other-thing\nname = Xiry\n"[..],
            Reason::Ok,
            true,
            Failure::Damaged,
        ),
        (INI, Reason::Ok, false, Failure::Damaged),
        (INI, Reason::NoNetwork, false, Failure::Network),
        (INI, Reason::Timeout, false, Failure::Network),
        (INI, Reason::Full, false, Failure::Card),
    ] {
        let mut wire = Fake::new();
        let mut download = Download::new();
        download.start(&mut wire, &two_files()).unwrap();
        let mut done = wire.answer(body);
        done.reason = reason;
        done.saved = saved;
        assert_eq!(
            download.ended(&mut wire, &done).unwrap(),
            Step::Failed(want)
        );
        // What was downloaded is gone, and nothing more is asked for.
        assert!(!wire.storage.exists("stage/airy-theme/plugin.ini"));
        assert_eq!(Download::progress(&wire.storage).unwrap(), None);
        assert_eq!(wire.asked.len(), 1);
    }
}

#[test]
fn an_answer_to_another_request_changes_nothing() {
    let mut wire = Fake::new();
    let mut download = Download::new();
    download.start(&mut wire, &two_files()).unwrap();
    let mut done = wire.answer(INI);
    // One of a download stopped before, and one of another plugin's token.
    let other = Done::new(RequestId::new(99).unwrap(), FILE_REQUEST, Reason::Ok, true);
    assert_eq!(
        download.ended(&mut wire, &other).unwrap(),
        Step::Downloading { done: 0, total: 2 }
    );
    done.token = FILE_REQUEST + 1;
    assert_eq!(
        download.ended(&mut wire, &done).unwrap(),
        Step::Downloading { done: 0, total: 2 }
    );
    assert_eq!(wire.asked.len(), 1);
}

#[test]
fn a_start_stops_the_one_before_and_a_cancel_forgets_it() {
    let mut wire = Fake::new();
    let mut download = Download::new();
    download.start(&mut wire, &two_files()).unwrap();
    let old = wire.answer(INI);
    let other = page("other", alloc::vec![file("plugin.ini", INI)]);
    download.start(&mut wire, &other).unwrap();
    assert_eq!(wire.cancelled, 1);
    assert!(!wire.storage.exists("stage/airy-theme/plugin.ini"));
    // The answer to the file of the first download is passed over.
    assert_eq!(
        download.ended(&mut wire, &old).unwrap(),
        Step::Downloading { done: 0, total: 1 }
    );
    download.cancel(&mut wire);
    assert_eq!(download.step(), Step::Idle);
    assert_eq!(Download::progress(&wire.storage).unwrap(), None);
    assert!(!wire.storage.exists("stage/other/plugin.ini"));
}

#[test]
fn a_page_that_is_no_package_is_not_started() {
    let ini = file("plugin.ini", INI);
    let mut doubled = alloc::vec![ini.clone(), file("a.ini", DE)];
    doubled.push(file("a.ini", DE));
    let many: Vec<File> = (0..65)
        .map(|n| file(&alloc::format!("f{n}.txt"), DE))
        .collect();
    let mut bad_digest = ini.clone();
    bad_digest.digest = Digest::Sha1("xyz".into());
    let mut bad_url = ini.clone();
    bad_url.url = "https://x/a\tb".into();
    for (id, files) in [
        ("airy-theme", alloc::vec![file("theme.ini", INI)]),
        ("airy-theme", doubled),
        ("airy-theme", many),
        ("airy-theme", alloc::vec![bad_digest]),
        ("airy-theme", alloc::vec![bad_url]),
        ("airy-theme", alloc::vec![file("../plugin.ini", INI)]),
        ("airy-theme", alloc::vec![file(".hidden/plugin.ini", INI)]),
        ("airy-theme", Vec::new()),
        ("core", alloc::vec![ini.clone()]),
        ("Airy", alloc::vec![ini.clone()]),
    ] {
        let mut wire = Fake::new();
        let mut download = Download::new();
        assert_eq!(
            download.start(&mut wire, &page(id, files)).unwrap(),
            Started::Invalid
        );
        assert!(wire.asked.is_empty());
        assert_eq!(download.step(), Step::Idle);
    }
    assert!(is_package("airy-theme", &[ini]));
}

#[test]
fn a_refused_first_request_leaves_nothing_behind() {
    let mut wire = Fake::new();
    wire.refuse = Some(Error::Unsupported);
    let mut download = Download::new();
    assert_eq!(
        download.start(&mut wire, &two_files()).unwrap(),
        Started::NoNetwork
    );
    assert_eq!(Download::progress(&wire.storage).unwrap(), None);
    assert_eq!(download.step(), Step::Idle);
    // Memory refused by the host's side is not a missing network.
    wire.refuse = Some(Error::NoMemory);
    assert_eq!(
        download.start(&mut wire, &two_files()),
        Err(Error::NoMemory)
    );
    assert_eq!(Download::progress(&wire.storage).unwrap(), None);
}

#[test]
fn the_digests_of_both_kinds_are_checked_in_pieces() {
    let storage = MemoryStorage::new().with("f", "abc");
    let sha1 = Digest::Sha1("A9993E364706816ABA3E25717850C26C9CD0D89D".into());
    let sha256 =
        Digest::Sha256("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into());
    assert!(holds(&storage, "f", &sha1) && holds(&storage, "f", &sha256));
    assert!(!holds(&storage, "f", &Digest::Sha1("0".repeat(40))));
    assert!(!holds(&storage, "missing", &sha256));
    // A file longer than a piece.
    let long = "x".repeat(1500);
    let storage = MemoryStorage::new().with("g", long.clone());
    assert!(holds(&storage, "g", &sha256_of(long.as_bytes())));
    assert_eq!(
        digest_text(&sha1).unwrap(),
        "sha1:a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    assert_eq!(digest_parse("md5:00").unwrap(), None);
}

fn sha256_of(data: &[u8]) -> Digest {
    sha256(data)
}

#[test]
fn lines_are_read_in_pieces_and_a_long_one_is_none() {
    let long = "x".repeat(700);
    let storage = MemoryStorage::new().with("f.txt", alloc::format!("first\n{long}\nthird\nlast"));
    assert_eq!(
        nth_line(&storage, "f.txt", 0).unwrap().as_deref(),
        Some("first")
    );
    assert_eq!(nth_line(&storage, "f.txt", 1).unwrap(), Some(long));
    assert_eq!(
        nth_line(&storage, "f.txt", 3).unwrap().as_deref(),
        Some("last")
    );
    assert_eq!(nth_line(&storage, "f.txt", 4).unwrap(), None);
    assert_eq!(nth_line(&storage, "missing", 0).unwrap(), None);
    let storage = MemoryStorage::new().with("l", "y".repeat(3000));
    assert_eq!(nth_line(&storage, "l", 0).unwrap(), None);
}
