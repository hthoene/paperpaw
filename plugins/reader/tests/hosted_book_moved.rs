//! `event.book_moved` through the kernel's native host: the reader keeps the book it
//! read last in `state.ini`; when that book moves the path follows it, and a move of
//! another book leaves the file alone.
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
#[path = "../../test-support/hosted.rs"]
mod hosted;

use paperpaw_platform::Instant;
use paperpaw_sdk::{EventData, builtin::events};

const STATE: &str = ".paperpaw/plugins/reader/state.ini";

/// The reader started with `last` as the book it read last; the file it keeps then.
fn after_move(last: &str, from: &str, to: &str) -> String {
    let setup = hosted::Setup::default()
        .with_kernel(|builder| builder.storage(paperpaw_sdk::MemoryStorage::default()));
    let mut reader = hosted::install_in(
        &setup,
        paperpaw_plugin_reader::NATIVE_ENTRY.entry(),
        "reader",
        &[
            "core.device",
            "core.document",
            "core.files",
            "core.library",
            "core.registry",
            "core.ui",
            "core.work",
        ],
    )
    .expect("the host admits the plugin");
    let text = format!("[reader]\nlast = \"{last}\"\n");
    for dir in [".paperpaw", ".paperpaw/plugins", ".paperpaw/plugins/reader"] {
        reader.kernel().storage().create_dir(dir).ok();
    }
    reader
        .kernel()
        .storage()
        .write(STATE, text.as_bytes())
        .unwrap();
    drop(reader.kernel().start(Instant(0)));
    let moved = events::BookMoved::new(from.into(), to.into()).to_params();
    let effects = reader
        .kernel()
        .send_event(Instant(0), events::BOOK_MOVED.id().clone(), moved);
    let mut effects = effects;
    for step in 1..50u64 {
        let Some(at) = effects.wake_at() else { break };
        effects = reader.kernel().tick(Instant(at.0.max(step)));
    }
    let mut buf = vec![0u8; 4096];
    let n = reader
        .kernel()
        .storage()
        .read_at(STATE, 0, &mut buf)
        .unwrap();
    assert!(reader.dropped().is_empty(), "{:?}", reader.dropped());
    String::from_utf8(buf[..n].to_vec()).unwrap()
}

#[test]
#[ignore = "needs book_moved delivery (Services, #243)"]
fn the_last_book_follows_a_move() {
    let state = after_move("books/a.epub", "books/a.epub", "books/new/a.epub");
    assert!(state.contains("books/new/a.epub"), "{state}");
    assert!(!state.contains("\"books/a.epub\""), "{state}");
}

#[test]
#[ignore = "needs book_moved delivery (Services, #243)"]
fn the_move_of_another_book_changes_nothing() {
    let state = after_move("books/a.epub", "books/b.epub", "books/new/b.epub");
    assert!(state.contains("books/a.epub"), "{state}");
    assert!(!state.contains("books/new/b.epub"), "{state}");
}
