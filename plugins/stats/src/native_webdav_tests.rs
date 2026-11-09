//! The sync engine over a memory storage: what each answer of the server
//! leads to, the files kept, and the words in both languages.
use crate::native_webdav::{
    Both, DIR, Downloaded, Engine, FILE, Facts, INCOMING, Precondition, Say, Step, basic_auth,
    folder,
};
use crate::screens_tests::words;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use paperpaw_sdk::{MemoryStorage, Storage};

fn facts<'a>(folder: &'a str, user: &'a str) -> Facts<'a> {
    Facts {
        network: true,
        folder,
        user,
    }
}

fn names(storage: &dyn Storage) -> Vec<String> {
    let mut names = Vec::new();
    // A folder that was never written to is empty.
    storage
        .list(DIR, &mut |entry| {
            // The sync's own files (a merge writes the statistics' too).
            if entry.name.starts_with(FILE) || entry.name.starts_with("incoming") {
                names.push(entry.name.to_string());
            }
        })
        .unwrap_or(());
    names.sort();
    names
}

/// A first sync: nothing there yet (404), the statistics of this device up.
fn first_sync(storage: &MemoryStorage, engine: &mut Engine) -> Step {
    assert_eq!(
        engine
            .start(storage, facts("cloud.example/dav", "me"))
            .unwrap(),
        Step::Download
    );
    engine
        .downloaded(
            storage,
            Downloaded {
                status: 404,
                etag: None,
            },
        )
        .unwrap()
        .unwrap()
}

#[test]
fn a_sync_needs_a_network_and_a_folder() {
    let storage = MemoryStorage::new();
    let mut engine = Engine::default();
    let off = Facts {
        network: false,
        ..facts("a", "")
    };
    assert_eq!(
        engine.start(&storage, off).unwrap(),
        Step::Say(Say::NoNetwork)
    );
    assert_eq!(
        engine.start(&storage, facts("  ", "")).unwrap(),
        Step::Say(Say::NoFolder)
    );
    assert!(!engine.running());
}

#[test]
fn the_download_is_of_the_file_in_the_folder() {
    let storage = MemoryStorage::new();
    let mut engine = Engine::default();
    assert_eq!(
        engine
            .start(&storage, facts("cloud.example/dav/", "me"))
            .unwrap(),
        Step::Download
    );
    assert_eq!(engine.url(), "https://cloud.example/dav/statistics.sqlite3");
    assert_eq!(engine.user(), "me");
    assert!(engine.running());
    assert_eq!(folder("http://x/y/").unwrap(), "http://x/y");
}

#[test]
fn a_first_sync_uploads_only_if_there_is_still_no_file() {
    let storage = MemoryStorage::new();
    let mut engine = Engine::default();
    assert_eq!(
        first_sync(&storage, &mut engine),
        Step::Upload(Precondition::NoFile)
    );
    assert!(storage.size(&format!("{DIR}/{FILE}")).is_ok());
    assert!(storage.size(&format!("{DIR}/{INCOMING}")).is_err());
    // Up: the upload is what the next sync compares with.
    let done = engine
        .uploaded(&storage, facts("cloud.example/dav", "me"), 201)
        .unwrap();
    assert!(matches!(
        done,
        Some(Step::Say(Say::Done { kept: false, .. }))
    ));
    assert!(!engine.running());
    let kept = names(&storage);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(kept[0].starts_with("statistics.sqlite3.") && kept[0].ends_with(".sync"));
}

#[test]
fn a_file_gone_from_where_this_device_uploaded_is_not_replaced_at_once() {
    let storage = MemoryStorage::new();
    let mut engine = Engine::default();
    first_sync(&storage, &mut engine);
    engine
        .uploaded(&storage, facts("cloud.example/dav", "me"), 200)
        .unwrap();
    assert_eq!(
        engine
            .start(&storage, facts("cloud.example/dav", "me"))
            .unwrap(),
        Step::Download
    );
    let step = engine
        .downloaded(
            &storage,
            Downloaded {
                status: 404,
                etag: None,
            },
        )
        .unwrap();
    assert_eq!(step, Some(Step::Say(Say::Missing)));
    // The last upload goes: the next sync uploads.
    assert!(names(&storage).is_empty(), "{:?}", names(&storage));
    assert_eq!(
        first_sync(&storage, &mut engine),
        Step::Upload(Precondition::NoFile)
    );
}

#[test]
fn a_download_that_is_not_a_statistics_file_ends_the_sync_unchanged() {
    let storage = MemoryStorage::new();
    let mut engine = Engine::default();
    engine
        .start(&storage, facts("cloud.example/dav", ""))
        .unwrap();
    storage
        .write(&format!("{DIR}/{INCOMING}"), b"<html>login</html>")
        .unwrap();
    let step = engine
        .downloaded(
            &storage,
            Downloaded {
                status: 200,
                etag: Some("\"abc\""),
            },
        )
        .unwrap();
    assert!(
        matches!(step, Some(Step::Say(Say::Unreadable(_)))),
        "{step:?}"
    );
    assert!(names(&storage).is_empty(), "{:?}", names(&storage));
    assert!(!engine.running());
}

#[test]
fn answers_that_are_none_end_the_sync_with_their_status() {
    for status in [0, 401, 403, 404, 409, 500, 503] {
        let storage = MemoryStorage::new();
        let mut engine = Engine::default();
        engine
            .start(&storage, facts("cloud.example/dav", ""))
            .unwrap();
        let step = engine
            .downloaded(&storage, Downloaded { status, etag: None })
            .unwrap();
        // A 404 with no upload before is the first sync.
        if status == 404 {
            assert_eq!(step, Some(Step::Upload(Precondition::NoFile)));
        } else {
            assert_eq!(step, Some(Step::Say(Say::Status(status))));
        }
    }
}

#[test]
fn an_upload_that_found_the_file_changed_starts_again_three_times() {
    let storage = MemoryStorage::new();
    let mut engine = Engine::default();
    let f = facts("cloud.example/dav", "me");
    let down = Downloaded {
        status: 404,
        etag: None,
    };
    engine.start(&storage, f).unwrap();
    for _ in 0..2 {
        engine.downloaded(&storage, down).unwrap();
        assert_eq!(
            engine.uploaded(&storage, f, 412).unwrap(),
            Some(Step::Download)
        );
    }
    engine.downloaded(&storage, down).unwrap();
    assert_eq!(
        engine.uploaded(&storage, f, 412).unwrap(),
        Some(Step::Say(Say::Status(412)))
    );
    assert!(!engine.running());
    // Nothing is left behind.
    assert!(names(&storage).is_empty(), "{:?}", names(&storage));
}

#[test]
fn a_request_that_got_no_answer_ends_the_sync() {
    let storage = MemoryStorage::new();
    let mut engine = Engine::default();
    assert_eq!(engine.lost(&storage), None);
    engine
        .start(&storage, facts("cloud.example/dav", ""))
        .unwrap();
    assert_eq!(engine.lost(&storage), Some(Step::Say(Say::Status(0))));
    assert!(!engine.running());
    // A late answer to an ended sync is ignored.
    let late = engine
        .downloaded(
            &storage,
            Downloaded {
                status: 200,
                etag: None,
            },
        )
        .unwrap();
    assert_eq!(late, None);
}

#[test]
fn other_uploads_are_forgotten_after_one() {
    let storage = MemoryStorage::new();
    storage
        .write(&format!("{DIR}/statistics.sqlite3.sync"), b"old")
        .unwrap();
    storage
        .write(
            &format!("{DIR}/statistics.sqlite3.0123456789abcdef.sync"),
            b"other",
        )
        .unwrap();
    storage.write(&format!("{DIR}/pages.bin"), b"keep").unwrap();
    let mut engine = Engine::default();
    first_sync(&storage, &mut engine);
    engine
        .uploaded(&storage, facts("cloud.example/dav", "me"), 200)
        .unwrap();
    let left = names(&storage);
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(storage.size(&format!("{DIR}/pages.bin")).is_ok());
}

#[test]
fn the_authorization_is_what_interface_6_sent() {
    for (user, password) in [
        ("me", "pw"),
        ("a", ""),
        ("user name", "pä:ss wörd"),
        ("", "x"),
    ] {
        assert_eq!(
            basic_auth(user, password).unwrap(),
            paperpaw_sdk::web::basic_auth(user, password)
        );
    }
}

#[test]
fn the_card_is_read_only_and_the_data_folder_is_written() {
    let data = MemoryStorage::new();
    let card = MemoryStorage::new();
    card.write("koreader/settings/statistics.sqlite3", b"x")
        .unwrap();
    let both = Both {
        data: &data,
        card: &card,
    };
    assert_eq!(
        both.size("koreader/settings/statistics.sqlite3").unwrap(),
        1
    );
    assert!(both.write("koreader/settings/new", b"y").is_err());
    both.write(&format!("{DIR}/a"), b"z").unwrap();
    assert_eq!(data.size(&format!("{DIR}/a")).unwrap(), 1);
    assert!(card.size(&format!("{DIR}/a")).is_err());
    // A look-alike folder is the card's.
    assert!(both.write(&format!("{DIR}x/a"), b"z").is_err());
}

#[test]
fn nothing_there_to_take_over_is_said() {
    let data = MemoryStorage::new();
    let card = MemoryStorage::new();
    let both = Both {
        data: &data,
        card: &card,
    };
    assert_eq!(
        crate::native_webdav::import(&both, &card).unwrap(),
        Say::NoKoreader
    );
    card.write("koreader/settings/statistics.sqlite3", b"junk")
        .unwrap();
    card.write("koreader/settings/statistics.sqlite3-wal", b"log")
        .unwrap();
    assert_eq!(
        crate::native_webdav::import(&both, &card).unwrap(),
        Say::KoreaderBusy
    );
}

#[test]
fn every_message_has_words_in_both_languages() {
    let says = [
        Say::NoNetwork,
        Say::NoFolder,
        Say::Running,
        Say::Missing,
        Say::Unreadable("cut".to_string()),
        Say::Failed("full".to_string()),
        Say::Asked,
        Say::Done {
            books: 3,
            kept: false,
        },
        Say::Done {
            books: 1,
            kept: true,
        },
        Say::Status(0),
        Say::Status(401),
        Say::Status(409),
        Say::Status(503),
        Say::NoKoreader,
        Say::KoreaderBusy,
        Say::Imported {
            books: 2,
            stays: 40,
        },
        Say::ImportUnreadable("bad".to_string()),
    ];
    for language in ["en", "de"] {
        let mut w = words(language);
        for say in &says {
            let text = say.text(&mut w).unwrap();
            assert!(
                !text.is_empty() && !text.contains('{'),
                "{language} {say:?}: {text}"
            );
        }
    }
    let mut w = words("en");
    assert_eq!(
        Say::Done {
            books: 3,
            kept: false
        }
        .text(&mut w)
        .unwrap(),
        "Synced: 3 books."
    );
    assert_eq!(
        Say::Status(503).text(&mut w).unwrap(),
        "The server reported an error (503)."
    );
}

/// Every allocation the engine's start and the authorization make is
/// fallible: a refused one is `NoMemory`, and no sync is left running.
#[test]
fn a_heap_that_refuses_ends_the_start_and_the_authorization_cleanly() {
    use crate::testing::heap::{armed, fixture};
    use paperpaw_sdk::native::Error;
    let storage = fixture(MemoryStorage::new);
    let run = || {
        let mut engine = Engine::default();
        let started = engine.start(&storage, facts("cloud.example/dav/", "me"));
        (started, engine.running())
    };
    let ((whole, running), count) = armed(usize::MAX, run);
    assert_eq!(whole.unwrap(), Step::Download);
    assert!(running && count > 0);
    for at in 1..=count {
        let ((result, running), _) = armed(at, run);
        match result {
            Ok(step) => assert_eq!(step, Step::Download, "at {at}"),
            Err(error) => {
                assert_eq!(error, Error::NoMemory, "at {at}");
                assert!(!running, "at {at}");
            }
        }
    }
    let (whole, count) = armed(usize::MAX, || basic_auth("me", "pässword"));
    assert!(whole.is_ok());
    for at in 1..=count {
        let (result, _) = armed(at, || basic_auth("me", "pässword"));
        assert!(matches!(result, Ok(_) | Err(Error::NoMemory)), "at {at}");
    }
}
