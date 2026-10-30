//! Cleaning up what is kept for books no longer on the storage
//! (`core.clean-up`).

use super::*;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::library::key;

/// What is kept for a file still on the storage stays, also where no
/// plugin opens its format now (switched off); what is kept for a file
/// gone goes.
#[test]
fn cleaning_up_keeps_what_belongs_to_files_still_there() {
    let annotations = |path: &str| alloc::format!(".paperpaw/annotations/{}", key(path));
    let storage = MemoryStorage::new()
        .with("Books/a.txt", "a book no plugin reads now")
        .with(&annotations("Books/a.txt"), "a note")
        .with(&annotations("Books/gone.txt"), "a note");
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(NoticePlugin { log: log() })
        .storage(storage)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.run_action(Instant(10), actions::CLEAN_UP);
    // The notes of a book not found go once the reader agrees.
    press(&mut kernel, 20, "left");
    press(&mut kernel, 30, "confirm");
    let storage = kernel.storage();
    assert!(storage.exists(&annotations("Books/a.txt")), "kept");
    assert!(!storage.exists(&annotations("Books/gone.txt")), "left over");
}

/// Hears the answers to the question whether the notes of books not
/// found go: the question asked.
struct Heard(Log);

impl Plugin for Heard {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("heard"), "Heard", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.0.clone();
        let answered: paperpaw_sdk::event::Event<paperpaw_sdk::replies::Confirmed> =
            paperpaw_sdk::event::Event::new("core.clean-up-answered");
        r.on(answered, move |_, answer, _| {
            let asked = answer
                .context
                .get(paperpaw_sdk::builtin::params::TITLE)
                .unwrap_or_default();
            log.borrow_mut()
                .push(alloc::format!("{asked} {}", answer.yes));
        })
    }
}

/// What the reader keeps for a book not found (renamed on a computer, on
/// a card not inserted) stays when the storage is cleaned up: asked
/// first, with how many books lose theirs, it goes on yes only. A cover
/// made small goes at once, made again when needed.
#[test]
fn cleaning_up_asks_before_the_notes_of_books_not_found_go() {
    let gone = |folder: &str, path: &str| alloc::format!(".paperpaw/{folder}/{}", key(path));
    let cover = alloc::format!(".paperpaw/library/covers/{}.thumb", key("Books/a.txt"));
    let storage = MemoryStorage::new()
        .with(&gone("annotations", "Books/a.txt"), "a note")
        .with(&gone("positions", "Books/a.txt"), "page 30")
        .with(&gone("annotations", "Books/b.txt"), "a note")
        .with(&cover, "a cover");
    let heard = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(NoticePlugin { log: log() })
        .plugin(Heard(heard.clone()))
        .user_config("[core]\nlanguage = en\n")
        .storage(storage)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.run_action(Instant(10), actions::CLEAN_UP);
    assert!(!kernel.storage().exists(&cover), "a cache");
    assert_eq!(
        kernel.active_screen().map(Id::as_str),
        Some("core.confirm-dialog")
    );
    // No, where the focus starts.
    press(&mut kernel, 20, "confirm");
    for kept in [
        gone("annotations", "Books/a.txt"),
        gone("positions", "Books/a.txt"),
        gone("annotations", "Books/b.txt"),
    ] {
        assert!(kernel.storage().exists(&kept), "{kept}");
    }
    // Yes.
    kernel.run_action(Instant(100), actions::CLEAN_UP);
    press(&mut kernel, 200, "left");
    press(&mut kernel, 300, "confirm");
    assert!(!kernel.storage().exists(&gone("annotations", "Books/a.txt")));
    assert!(!kernel.storage().exists(&gone("positions", "Books/a.txt")));
    assert!(!kernel.storage().exists(&gone("annotations", "Books/b.txt")));
    let question = "Remove the notes of 2 books not found?";
    assert_eq!(
        *heard.borrow(),
        [
            alloc::format!("{question} false"),
            alloc::format!("{question} true")
        ]
    );
}

/// On a memory card, which ignores case, what is kept for a book stays
/// whether the library folder is spelt as its setting says (`books`) or
/// as the card has it (`Books`).
#[test]
fn cleaning_up_keeps_books_by_every_spelling_of_their_folder() {
    let annotations = |path: &str| alloc::format!(".paperpaw/annotations/{}", key(path));
    let storage = MemoryStorage::new()
        .with("Books/a.txt", "a book")
        .with(&annotations("books/a.txt"), "a note")
        .with(&annotations("Books/a.txt"), "a note");
    // The kernel's library folder, `books` by default.
    let mut kernel = Kernel::builder(DEVICE)
        .storage(super::storage::Fat(storage))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.run_action(Instant(10), actions::CLEAN_UP);
    let storage = kernel.storage();
    assert!(
        storage.exists(&annotations("books/a.txt")),
        "as the setting"
    );
    assert!(storage.exists(&annotations("Books/a.txt")), "as the card");
}

/// With a clock, the caches of books not found are cleaned up by
/// themselves while charging, a day after it was first seen and then
/// weekly; not before. The reader's notes stay.
#[test]
fn cleaning_up_runs_by_itself_while_charging_now_and_then() {
    use paperpaw_platform::{Battery, SystemStatus, WallClock};
    let annotations = |path: &str| alloc::format!(".paperpaw/annotations/{}", key(path));
    let cover = alloc::format!(".paperpaw/library/covers/{}.thumb", key("Books/gone.txt"));
    let storage = MemoryStorage::new()
        .with(&annotations("Books/gone.txt"), "a note")
        .with(&cover, "a cover");
    let mut kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    let status = |charging: bool| {
        SystemStatus::new(
            Some(WallClock::new(Instant(0), 1_800_000_000_000, None)),
            Some(Battery::new(50, charging, None)),
        )
    };
    kernel.set_status(Instant(0), status(false));
    kernel.start(Instant(0));
    let gone = cover.as_str();
    // Charging at once: too early.
    kernel.set_status(Instant(1000), status(true));
    kernel.tick(Instant(2000));
    assert!(kernel.storage().exists(gone));
    // A day later, charging: cleaned up, and the time noted.
    let day = 24 * 3600 * 1000;
    kernel.set_status(Instant(day + 1000), status(false));
    kernel.set_status(Instant(day + 2000), status(true));
    for at in [day + 3000, day + 4000, day + 5000] {
        kernel.tick(Instant(at));
    }
    assert!(!kernel.storage().exists(gone));
    // The reader's notes of a book not found stay: never by itself.
    assert!(kernel.storage().exists(&annotations("Books/gone.txt")));
    let noted = paperpaw_sdk::read_file(kernel.storage(), ".paperpaw/cleaned-up.txt", 32).unwrap();
    // At the first tick after charging began.
    assert_eq!(noted, b"1800086403\n");
}
