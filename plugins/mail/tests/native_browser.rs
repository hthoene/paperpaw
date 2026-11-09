//! The lines of the list screens, from what the card keeps: the folders with
//! their unseen mails, a folder's mails newest first, the tasks of a check.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests unwrap what they just made"
)]

use paperpaw_plugin_mail::native_model::account::Values;
use paperpaw_plugin_mail::native_model::browser::{
    Line, Mode, Target, When, Words, boxes, folder, folder_title, grouped, special_name, tasks,
    when_of,
};
use paperpaw_plugin_mail::native_model::mem::{Error, Secret};
use paperpaw_plugin_mail::native_model::session::Task;
use paperpaw_plugin_mail::native_model::store::{self, Folder, Row};
use paperpaw_sdk::MemoryStorage;
use std::collections::BTreeMap;

/// Words that show their key, and dates as `when:<seconds>`.
struct Said;

impl Words for Said {
    fn tr(&mut self, key: &str) -> Result<String, Error> {
        Ok(format!("<{key}>"))
    }
    fn when(&mut self, date: i64) -> Result<String, Error> {
        Ok(if date <= 0 {
            String::new()
        } else {
            format!("when:{date}")
        })
    }
}

struct Set(BTreeMap<String, String>);

impl Values for Set {
    fn text(&self, id: &str) -> Result<String, Error> {
        Ok(self.0.get(id).cloned().unwrap_or_default())
    }
    fn int(&self, id: &str) -> Result<i64, Error> {
        Ok(self.0.get(id).and_then(|v| v.parse().ok()).unwrap_or(0))
    }
    fn secret(&self, _: &str) -> Result<Option<Secret>, Error> {
        Ok(None)
    }
}

fn accounts(numbers: &[u8]) -> Vec<paperpaw_plugin_mail::native_model::account::Account> {
    let mut set = BTreeMap::new();
    for n in numbers {
        for (k, v) in [
            ("address", format!("a{n}@example.org")),
            ("imap-server", "imap.example.org".into()),
            ("imap-port", "993".into()),
            ("smtp-server", "smtp.example.org".into()),
            ("smtp-port", "465".into()),
        ] {
            set.insert(format!("mail.{n}.{k}"), v);
        }
    }
    struct Secrets;
    impl Values for Secrets {
        fn text(&self, _: &str) -> Result<String, Error> {
            Ok(String::new())
        }
        fn int(&self, _: &str) -> Result<i64, Error> {
            Ok(0)
        }
        fn secret(&self, _: &str) -> Result<Option<Secret>, Error> {
            let mut s = Secret::with_capacity(1)?;
            s.push_str("x")?;
            Ok(Some(s))
        }
    }
    struct Both(Set, Secrets);
    impl Values for Both {
        fn text(&self, id: &str) -> Result<String, Error> {
            self.0.text(id)
        }
        fn int(&self, id: &str) -> Result<i64, Error> {
            self.0.int(id)
        }
        fn secret(&self, id: &str) -> Result<Option<Secret>, Error> {
            self.1.secret(id)
        }
    }
    let both = Both(Set(set), Secrets);
    numbers
        .iter()
        .filter_map(|n| {
            paperpaw_plugin_mail::native_model::account::Account::read(&both, *n).unwrap()
        })
        .collect()
}

fn row(uid: u32, flags: &str, date: i64, from: &str, subject: &str) -> Row {
    Row::parse(&format!(
        "{uid}\t{flags}\t{date}\t100\t{from}\t{from}@x\t{subject}\t<m{uid}>"
    ))
    .unwrap()
    .unwrap()
}

fn titles(lines: &[Line]) -> Vec<&str> {
    lines.iter().map(|l| l.title.as_str()).collect()
}

#[test]
fn no_account_says_to_set_one_up() {
    let storage = MemoryStorage::new();
    let lines = boxes(&storage, &mut Said, &[]).unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].title, "<mail.set-up>");
    assert_eq!(lines[0].note, "<mail.set-up.help>");
    assert_eq!(lines[0].target, Target::Settings);
}

#[test]
fn an_account_with_no_folders_yet_shows_its_inbox() {
    let storage = MemoryStorage::new();
    let lines = boxes(&storage, &mut Said, &accounts(&[1])).unwrap();
    assert_eq!(
        titles(&lines),
        [
            "<mail.check>",
            "<mail.inbox>",
            "<mail.write>",
            "<mail.settings>"
        ]
    );
    assert_eq!(
        lines[1].target,
        Target::Folder {
            account: 1,
            raw: "INBOX".into()
        }
    );
    // One account: no address under the folder.
    assert_eq!(lines[1].note, "");
}

#[test]
fn folders_show_unseen_counts_and_the_account_when_there_are_several() {
    let storage = MemoryStorage::new();
    let folders = vec![
        Folder::new("INBOX", vec![]).unwrap(),
        Folder::new("Sent Items", vec!["\\sent".into()]).unwrap(),
        Folder::new("Hidden", vec!["\\noselect".into()]).unwrap(),
        Folder::new("Receipts", vec![]).unwrap(),
    ];
    store::write_folders(&storage, &store::folders_file(2).unwrap(), &folders).unwrap();
    let counts =
        store::Counts::read(&MemoryStorage::new().with("s", b"10\t3\t7\t0\n"), "s").unwrap();
    store::write_atomic(
        &storage,
        &store::status_file(2, "INBOX").unwrap(),
        counts.line().unwrap().as_bytes(),
    )
    .unwrap();
    let lines = boxes(&storage, &mut Said, &accounts(&[1, 2])).unwrap();
    assert_eq!(
        titles(&lines),
        [
            "<mail.check>",
            "<mail.inbox>", // account 1, nothing synced yet
            "<mail.inbox>", // account 2
            "<mail.folder.sent>",
            "Receipts",
            "<mail.write>",
            "<mail.settings>"
        ]
    );
    assert_eq!(lines[1].note, "a1@example.org");
    assert_eq!(lines[2].note, "a2@example.org");
    assert_eq!(lines[1].value, "");
    assert_eq!(lines[2].value, "3");
    assert_eq!(lines[3].value, "");
}

#[test]
fn a_folder_lists_its_mails_newest_first_with_marks() {
    let storage = MemoryStorage::new();
    let rows = vec![
        row(3, "S", 300, "Ann", "Hello"),
        row(9, "F", 900, "Bob", ""),
        row(5, "", 0, "Cy", "News"),
    ];
    store::write_index(&storage, &store::index_file(1, "INBOX").unwrap(), &rows).unwrap();
    let lines = folder(&storage, &mut Said, 1, "INBOX").unwrap();
    assert_eq!(
        titles(&lines),
        [
            "<mail.check>",
            "\u{2022} * Bob",
            "\u{2022} Cy",
            "Ann",
            "<mail.write>"
        ]
    );
    assert_eq!(lines[1].note, "<mail.no-subject>");
    assert_eq!(lines[1].value, "when:900");
    assert_eq!(lines[2].value, "");
    assert_eq!(lines[3].target, Target::Message { uid: 3 });
    assert_eq!(lines[3].note, "Hello");
}

#[test]
fn an_unreadable_index_is_an_empty_folder() {
    let storage = MemoryStorage::new().with(
        &store::index_file(1, "INBOX").unwrap(),
        b"garbage\nmore garbage\n",
    );
    let lines = folder(&storage, &mut Said, 1, "INBOX").unwrap();
    assert_eq!(titles(&lines), ["<mail.check>", "<mail.write>"]);
}

#[test]
fn purposes_are_named_by_flag_or_by_the_usual_name() {
    let f = |name: &str, flags: &[&str]| {
        Folder::new(name, flags.iter().map(|f| (*f).into()).collect()).unwrap()
    };
    assert_eq!(special_name(&f("inbox", &[])), Some("mail.inbox"));
    assert_eq!(
        special_name(&f("Gesendet", &["\\sent"])),
        Some("mail.folder.sent")
    );
    assert_eq!(
        special_name(&f("Archive", &[])),
        Some("mail.folder.archive")
    );
    assert_eq!(special_name(&f("Receipts", &[])), None);
    assert_eq!(
        special_name(&f("Archive", &["\\trash"])),
        Some("mail.folder.trash")
    );
}

#[test]
fn a_folders_title_is_its_purpose_or_its_name() {
    let storage = MemoryStorage::new();
    let folders = vec![
        Folder::new("Sent Items", vec!["\\sent".into()]).unwrap(),
        Folder::new("Receipts", vec![]).unwrap(),
    ];
    store::write_folders(&storage, &store::folders_file(1).unwrap(), &folders).unwrap();
    let title = |raw: &str| folder_title(&storage, &mut Said, 1, raw).unwrap();
    assert_eq!(title("Sent Items"), "<mail.folder.sent>");
    assert_eq!(title("Receipts"), "Receipts");
    assert_eq!(title("inbox"), "<mail.inbox>");
    assert_eq!(title("Unknown"), "Unknown");
}

#[test]
fn a_check_does_what_the_list_shows() {
    let boxes_tasks = tasks(&Mode::Boxes, 2, 20, 55).unwrap();
    assert_eq!(boxes_tasks.len(), 2);
    assert!(matches!(&boxes_tasks[0], Task::Folders { out } if out == "a2/folders.txt"));
    assert!(matches!(
        &boxes_tasks[1],
        Task::Sync { folder, count: 20, now: 55, .. } if folder == "INBOX"
    ));
    let mode = Mode::Folder {
        account: 1,
        raw: "Sent".into(),
    };
    assert_eq!(tasks(&mode, 1, 5, 0).unwrap().len(), 1);
    assert!(tasks(&mode, 2, 5, 0).unwrap().is_empty());
}

#[test]
fn a_fingerprint_is_shown_in_two_lines_of_groups() {
    let hex = "0123456789abcdef".repeat(4);
    let shown = grouped(&hex).unwrap();
    let (first, second) = shown.split_once('\n').unwrap();
    assert_eq!(first.split(' ').count(), 4);
    assert_eq!(second.split(' ').count(), 4);
    assert!(first.starts_with("01234567 89abcdef"));
}

#[test]
fn a_mail_is_shown_by_time_today_and_by_date_before() {
    // 2026-11-09 12:00:00 UTC, in a zone one hour ahead.
    let now = 1_794_225_600;
    let hour = 3600;
    assert_eq!(when_of(0, Some(now), 0), When::Never);
    assert_eq!(
        when_of(now - 2 * hour, Some(now), hour),
        When::Today {
            hour: 11,
            minute: 0
        }
    );
    assert_eq!(
        when_of(now - 36 * hour, Some(now), hour),
        When::Day { month: 11, day: 8 }
    );
    // Midnight in the zone is the day's edge, not UTC's.
    let midnight_local = now - 13 * hour;
    assert_eq!(
        when_of(midnight_local, Some(now), hour),
        When::Today { hour: 0, minute: 0 }
    );
    assert_eq!(
        when_of(midnight_local - 1, Some(now), hour),
        When::Day { month: 11, day: 8 }
    );
    // Without a clock there is no today.
    assert_eq!(when_of(now, None, 0), When::Day { month: 11, day: 9 });
}
