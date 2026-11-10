//! What the start does before plugins are found: replay, set aside, sweep.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::plugin_swap::{BAD, Removal, begin_removal};
use paperpaw_platform::firmware::Text;
use paperpaw_sdk::MemoryStorage;
use std::vec::Vec;

fn lines(card: &MemoryStorage) -> Vec<String> {
    let mut said = Vec::new();
    replay_at_start(card, &mut |line| said.push(line));
    said
}

#[test]
fn leftovers_no_journal_owns_are_swept_and_a_stage_or_an_old_folder_never() {
    let card = MemoryStorage::new();
    for path in [
        "plugins/dials/plugin.ini",
        "plugins/.cut.part/plugin.ini",
        "plugins/.removing-gone/plugin.ini",
        ".paperpaw/plugins/.removing-gone/notes.txt",
        ".paperpaw/plugins/gone/notes.txt",
        "plugins/.old-dials/plugin.ini",
        "plugins/.hidden/plugin.ini",
        "plugins/.part/plugin.ini",
    ] {
        card.write(path, b"x").unwrap();
    }
    sweep(&card);
    assert!(card.exists("plugins/dials/plugin.ini"));
    assert!(
        card.exists("plugins/.cut.part/plugin.ini"),
        "a stage may be the shell's, waiting to be installed"
    );
    assert!(!card.exists("plugins/.removing-gone/plugin.ini"));
    assert!(!card.exists(".paperpaw/plugins/.removing-gone/notes.txt"));
    assert!(
        card.exists(".paperpaw/plugins/gone/notes.txt"),
        "data stays"
    );
    assert!(
        card.exists("plugins/.old-dials/plugin.ini"),
        "maybe the only copy"
    );
    assert!(card.exists("plugins/.hidden/plugin.ini"));
    assert!(
        card.exists("plugins/.part/plugin.ini"),
        "no id, not a stage"
    );
}

#[test]
fn nothing_is_swept_while_a_journal_is_there_and_at_most_eight_a_start() {
    let card = MemoryStorage::new();
    for n in 0..12 {
        card.write(&alloc::format!("plugins/.removing-p{n}/plugin.ini"), b"x")
            .unwrap();
    }
    card.write("plugins/.journal", b"journal").unwrap();
    sweep(&card);
    assert!(card.exists("plugins/.removing-p0/plugin.ini"));
    card.remove("plugins/.journal").unwrap();
    sweep(&card);
    let left = (0..12)
        .filter(|n| card.exists(&alloc::format!("plugins/.removing-p{n}/plugin.ini")))
        .count();
    assert_eq!(left, 12 - SWEEP_MAX);
    sweep(&card);
    assert!((0..12).all(|n| !card.exists(&alloc::format!("plugins/.removing-p{n}/plugin.ini"))));
}

#[test]
fn a_journalled_removal_is_finished_at_start_and_says_nothing() {
    let card = MemoryStorage::new();
    card.write("plugins/dials/plugin.ini", b"x").unwrap();
    card.write(".paperpaw/plugins/dials/notes.txt", b"x")
        .unwrap();
    begin_removal(
        &card,
        &Removal {
            id: Text::new("dials"),
            data: true,
        },
    )
    .unwrap();
    // With the data the removal stops where the host must forget the plugin's
    // secrets and grants: the journal stays, said by no line, and the start
    // reports that the first pass has to finish it.
    let mut said = Vec::new();
    assert!(replay_at_start(&card, &mut |line| said.push(line)));
    assert!(said.is_empty(), "{said:?}");
    assert!(!card.exists("plugins/dials/plugin.ini"));
    assert!(!card.exists(".paperpaw/plugins/dials/notes.txt"));
    assert!(card.size("plugins/.journal").is_ok());
    // Nothing is swept while the journal is there.
    sweep(&card);
    assert!(card.exists(".paperpaw/plugins/.removing-dials/notes.txt"));
    let mut asked = Vec::new();
    let done = crate::native_host::plugin_swap::resume(&card, &mut |id| {
        asked.push(String::from(id));
        true
    });
    assert!(matches!(
        done,
        crate::native_host::plugin_swap::Replayed::Removed { data: true, .. }
    ));
    assert_eq!(asked, ["dials"]);
    assert!(card.size("plugins/.journal").is_err());
    sweep(&card);
    assert!(!card.exists(".paperpaw/plugins/.removing-dials/notes.txt"));
}

#[test]
fn a_removal_that_keeps_the_data_is_finished_at_start_with_no_host() {
    let card = MemoryStorage::new();
    card.write("plugins/dials/plugin.ini", b"x").unwrap();
    card.write(".paperpaw/plugins/dials/notes.txt", b"x")
        .unwrap();
    begin_removal(
        &card,
        &Removal {
            id: Text::new("dials"),
            data: false,
        },
    )
    .unwrap();
    let mut said = Vec::new();
    assert!(!replay_at_start(&card, &mut |line| said.push(line)));
    assert!(said.is_empty(), "{said:?}");
    assert!(!card.exists("plugins/dials/plugin.ini"));
    assert!(card.exists(".paperpaw/plugins/dials/notes.txt"));
    assert!(card.size("plugins/.journal").is_err());
}

#[test]
fn a_journal_that_cannot_be_read_is_set_aside_with_a_line_in_the_log() {
    let card = MemoryStorage::new();
    card.write("plugins/.journal", b"not a record").unwrap();
    let said = lines(&card);
    assert_eq!(said.len(), 1);
    assert!(said[0].contains("set aside"), "{}", said[0]);
    assert!(card.size("plugins/.journal").is_err());
    assert!(card.size(BAD).is_ok());
}

#[test]
fn a_journal_the_card_could_not_finish_stays_and_is_said() {
    let card = MemoryStorage::new();
    card.write("plugins/dials/plugin.ini", b"x").unwrap();
    begin_removal(
        &card,
        &Removal {
            id: Text::new("dials"),
            data: false,
        },
    )
    .unwrap();
    let cut = crate::native_host::plugin_swap::tests::Cut::after(&card, 0);
    let mut said = Vec::new();
    replay_at_start(&cut, &mut |line| said.push(line));
    assert_eq!(said.len(), 1);
    assert!(said[0].contains("next start"), "{}", said[0]);
    assert!(
        card.size("plugins/.journal").is_ok(),
        "kept for the next start"
    );
    assert!(card.exists("plugins/dials/plugin.ini"));
}
