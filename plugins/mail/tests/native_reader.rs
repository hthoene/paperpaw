//! The reader of a mail, host-free: what it shows (the headers and the text),
//! what fetching does, the stars and the read marks in the list, where the
//! parts go, and how the text and the buttons move.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests unwrap what they just made"
)]

use paperpaw_plugin_mail::native_model::browser::Words;
use paperpaw_plugin_mail::native_model::mem::Error;
use paperpaw_plugin_mail::native_model::reader::{
    Step, change_row, fetch_tasks, flag_task, focus_after, layout, scrolled, shown, starred,
    with_flag,
};
use paperpaw_plugin_mail::native_model::session::Task;
use paperpaw_plugin_mail::native_model::store::{self, Letter, Row};
use paperpaw_sdk::{MemoryStorage, Rect};

struct Said;

impl Words for Said {
    fn tr(&mut self, key: &str) -> Result<String, Error> {
        Ok(format!("<{key}>"))
    }
    fn when(&mut self, date: i64) -> Result<String, Error> {
        Ok(if date <= 0 {
            String::new()
        } else {
            format!("t{date}")
        })
    }
}

fn letter(text: &str) -> Letter {
    let file = format!(
        "From: Ann <ann@x>\nTo: bob@x\nCc: cy@x\nSubject: Hi\nDate: 500\nMessage-ID: <1@x>\n\n{text}"
    );
    let storage = MemoryStorage::new().with("l", file.as_bytes());
    store::read_letter(&storage, "l").unwrap().unwrap()
}

fn row(uid: u32, flags: &str) -> Row {
    Row::parse(&format!("{uid}\t{flags}\t1\t2\ta\ta@x\ts\t<m>"))
        .unwrap()
        .unwrap()
}

#[test]
fn the_headers_come_before_the_text() {
    let mut l = letter("Hello.\nBye.");
    l.attachments = vec!["a.pdf (application/pdf)".into(), "b.png (image/png)".into()];
    let text = shown(&mut Said, &l).unwrap();
    assert!(text.starts_with("<mail.from>: "), "{text}");
    assert!(text.contains("<mail.cc>: cy@x\n"));
    assert!(text.contains("<mail.date-label>: t500\n"));
    assert!(text.contains("<mail.attachments>: a.pdf (application/pdf), b.png (image/png)\n"));
    assert!(text.ends_with("\n\nHello.\nBye."), "{text:?}");
    assert!(!text.contains("<mail.cut>"));
}

#[test]
fn a_header_with_nothing_to_say_is_left_out_and_a_cut_is_said() {
    let mut l = letter("x");
    l.cc = String::new();
    l.date = 0;
    l.truncated = true;
    let text = shown(&mut Said, &l).unwrap();
    assert!(!text.contains("<mail.cc>"));
    assert!(!text.contains("<mail.date-label>"));
    assert!(!text.contains("<mail.attachments>"));
    assert!(text.ends_with("\n\n<mail.cut>"));
}

#[test]
fn fetching_gets_the_body_and_marks_it_read_where_the_settings_say_so() {
    let tasks = fetch_tasks(2, "INBOX", 9, true).unwrap();
    assert_eq!(tasks.len(), 2);
    assert!(
        matches!(&tasks[0], Task::Body { folder, uid: 9, out } if folder == "INBOX" && out.ends_with("m9.eml"))
    );
    assert!(matches!(
        &tasks[1],
        Task::Flag {
            flag: "\\Seen",
            on: true,
            uid: 9,
            ..
        }
    ));
    assert_eq!(fetch_tasks(2, "INBOX", 9, false).unwrap().len(), 1);
    assert!(matches!(
        flag_task("INBOX", 3, "\\Flagged", false).unwrap(),
        Task::Flag {
            flag: "\\Flagged",
            on: false,
            uid: 3,
            ..
        }
    ));
}

#[test]
fn stars_and_read_marks_change_the_row_of_the_list() {
    let storage = MemoryStorage::new();
    let file = store::index_file(1, "INBOX").unwrap();
    store::write_index(&storage, &file, &[row(1, ""), row(2, "S")]).unwrap();
    assert!(!starred(&storage, 1, "INBOX", 2).unwrap());
    let changed = change_row(&storage, 1, "INBOX", 2, |r| {
        r.flags = with_flag(&r.flags, 'F', true)?;
        Ok(())
    })
    .unwrap();
    assert!(changed);
    assert!(starred(&storage, 1, "INBOX", 2).unwrap());
    // The other row is as it was; a mail not in the list changes nothing.
    let rows = store::read_index(&storage, &file).unwrap();
    assert_eq!(rows.iter().find(|r| r.uid == 1).unwrap().flags, "");
    assert!(!change_row(&storage, 1, "INBOX", 7, |_| Ok(())).unwrap());
}

#[test]
fn a_flag_is_set_once_and_cleared_cleanly() {
    assert_eq!(with_flag("", 'S', true).unwrap(), "S");
    assert_eq!(with_flag("S", 'S', true).unwrap(), "S");
    assert_eq!(with_flag("SF", 'S', true).unwrap(), "SF");
    assert_eq!(with_flag("SF", 'F', false).unwrap(), "S");
    assert_eq!(with_flag("S", 'F', false).unwrap(), "S");
}

#[test]
fn the_parts_fill_the_screen_without_overlap() {
    let l = layout(480, 800, 60, 8, 16, 48, 20);
    assert_eq!(l.area.x, 8);
    assert_eq!(l.area.width, 464);
    assert!(l.area.y >= 60 + 8);
    // The text ends above the status line, which ends above the buttons.
    assert!(l.area.y + l.area.height <= l.note.y);
    assert!(l.note.y + l.note.height <= l.buttons[0].y);
    for (i, b) in l.buttons.iter().enumerate() {
        assert_eq!(b.height, 48);
        assert!(b.x + b.width <= 480 - 16 + 1);
        if i > 0 {
            let before: Rect = l.buttons[i - 1];
            assert_eq!(b.x, before.x + before.width + 8);
        }
    }
    assert!(l.buttons[2].y + 48 <= 800);
}

#[test]
fn a_tiny_screen_does_not_overflow() {
    let l = layout(10, 10, 60, 8, 16, 48, 20);
    assert_eq!(l.area.height, 0);
    assert_eq!(l.buttons[0].width, 0);
}

#[test]
fn the_text_scrolls_by_a_few_lines_or_a_page_and_stops_at_its_ends() {
    // 10 rows of 100 lines: a few is a third of a page.
    assert_eq!(scrolled(0, 10, 100, Step::Down), 3);
    assert_eq!(scrolled(3, 10, 100, Step::Up), 0);
    assert_eq!(scrolled(0, 10, 100, Step::Up), 0);
    assert_eq!(scrolled(0, 10, 100, Step::PageDown), 10);
    assert_eq!(scrolled(95, 10, 100, Step::PageDown), 90);
    assert_eq!(scrolled(5, 10, 100, Step::PageUp), 0);
    // A text that fits does not move; one row still moves by one line.
    assert_eq!(scrolled(0, 10, 4, Step::PageDown), 0);
    assert_eq!(scrolled(0, 1, 5, Step::Down), 1);
    assert_eq!(Step::of_action("core.down"), Some(Step::Down));
    assert_eq!(Step::of_action("core.next-page"), Some(Step::PageDown));
    assert_eq!(Step::of_action("core.previous"), Some(Step::PageUp));
    assert_eq!(Step::of_action("core.confirm"), None);
}

#[test]
fn the_focus_goes_round_the_buttons() {
    assert_eq!(focus_after(None, true), 0);
    assert_eq!(focus_after(None, false), 2);
    assert_eq!(focus_after(Some(2), true), 0);
    assert_eq!(focus_after(Some(0), false), 2);
    assert_eq!(focus_after(Some(1), true), 2);
}
