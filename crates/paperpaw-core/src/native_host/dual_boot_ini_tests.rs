#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
//! `paperpaw.ini`'s `dual-boot`: read as the firmware reads it, set in the
//! last `[firmware]` section with the rest of the file as it was.
extern crate std;
use super::*;
use alloc::{format, vec};
use paperpaw_sdk::MemoryStorage;
use std::string::String;

fn edited(text: &str, mode: Mode) -> String {
    String::from_utf8(edit(text, mode).unwrap()).unwrap()
}

#[test]
fn the_choice_is_read_as_the_firmware_reads_it() {
    assert_eq!(choice(""), Choice::Auto);
    assert_eq!(choice("[firmware]\ndual-boot = off\n"), Choice::Off);
    assert_eq!(choice("[firmware]\ndual-boot = OFF\n"), Choice::Off);
    assert_eq!(choice("[firmware]\ndual-boot = 0\n"), Choice::Off);
    assert_eq!(choice("[firmware]\ndual-boot = auto\n"), Choice::Auto);
    assert_eq!(choice("[firmware]\ndual-boot = on\n"), Choice::On);
    // A typo is on: it must not leave the other firmware open to an update.
    assert_eq!(choice("[firmware]\ndual-boot = onn\n"), Choice::On);
    // The last one wins, in any `[firmware]` section; other sections do not
    // count, nor does a comment.
    assert_eq!(
        choice("[firmware]\ndual-boot = off\n[x]\ndual-boot = on\n"),
        Choice::Off
    );
    assert_eq!(
        choice("[firmware]\ndual-boot = on\n[firmware]\ndual-boot = off\n"),
        Choice::Off
    );
    assert_eq!(choice("[firmware]\n; dual-boot = on\n"), Choice::Auto);
    assert_eq!(choice("\u{feff}[firmware]\ndual-boot = off\n"), Choice::Off);
}

#[test]
fn a_missing_section_is_added_and_the_rest_stays() {
    assert_eq!(edited("", Mode::Off), "[firmware]\ndual-boot = off\n");
    assert_eq!(
        edited("[plugins]\na = b\n", Mode::Off),
        "[plugins]\na = b\n[firmware]\ndual-boot = off\n"
    );
    // A last line without an ending gets one first.
    assert_eq!(
        edited("[plugins]\na = b", Mode::Auto),
        "[plugins]\na = b\n[firmware]\ndual-boot = auto\n"
    );
    assert_eq!(
        edited("[plugins]\r\na = b\r\n", Mode::Off),
        "[plugins]\r\na = b\r\n[firmware]\r\ndual-boot = off\r\n"
    );
}

#[test]
fn the_key_is_set_in_place_or_added_to_the_last_firmware_section() {
    assert_eq!(
        edited("[firmware]\ndual-boot = on\nx = 1\n", Mode::Off),
        "[firmware]\ndual-boot = off\nx = 1\n"
    );
    // Added at the end of the section, before the next one and its blank line.
    assert_eq!(
        edited("[firmware]\nx = 1\n\n[plugins]\na = b\n", Mode::Off),
        "[firmware]\nx = 1\ndual-boot = off\n\n[plugins]\na = b\n"
    );
    // The last section is the one set; an earlier one is left as it was.
    assert_eq!(
        edited(
            "[firmware]\ndual-boot = on\n[plugins]\n[firmware]\ny = 2\n",
            Mode::Off
        ),
        "[firmware]\ndual-boot = on\n[plugins]\n[firmware]\ny = 2\ndual-boot = off\n"
    );
    // The line's own ending stays, and an empty section gets the key.
    assert_eq!(
        edited("[firmware]\r\ndual-boot = on\r\n", Mode::Auto),
        "[firmware]\r\ndual-boot = auto\r\n"
    );
    assert_eq!(
        edited("[firmware]\n", Mode::Off),
        "[firmware]\ndual-boot = off\n"
    );
    assert_eq!(
        edited("[firmware]", Mode::Off),
        "[firmware]\ndual-boot = off\n"
    );
    // The result reads as asked.
    for mode in [Mode::Auto, Mode::Off] {
        for text in ["", "[firmware]\ndual-boot = on\n", "[a]\nb = c"] {
            let result = edited(text, mode);
            let want = if mode == Mode::Off {
                Choice::Off
            } else {
                Choice::Auto
            };
            assert_eq!(choice(&result), want, "{text:?}");
        }
    }
}

#[test]
fn a_file_over_the_size_or_unreadable_is_refused_and_nothing_is_written() {
    let card = MemoryStorage::new();
    // Over the size.
    card.write(INI_PATH, &std::vec![b'a'; INI_MAX + 1]).unwrap();
    assert_eq!(set(&card, Mode::Off), Err(Failure::Unreadable));
    assert_eq!(card.size(INI_PATH).unwrap(), INI_MAX as u64 + 1);
    // Not UTF-8.
    card.write(INI_PATH, &[0xff, 0xfe, 0x00]).unwrap();
    assert_eq!(set(&card, Mode::Off), Err(Failure::Unreadable));
    assert_eq!(read(&card).unwrap(), [0xff, 0xfe, 0x00]);
    // The reader skips what is no UTF-8 (as the firmware does): it chooses
    // nothing here, but an edit of such a file is refused.
    assert_eq!(card_choice(&card), Ok(Choice::Auto));
    assert_eq!(
        edit(&"a".repeat(INI_MAX + 1), Mode::Off),
        Err(Failure::Unreadable)
    );
}

#[test]
fn a_missing_file_is_an_empty_one_and_the_write_replaces_the_file_whole() {
    let card = MemoryStorage::new();
    assert_eq!(card_choice(&card), Ok(Choice::Auto));
    set(&card, Mode::Off).unwrap();
    assert_eq!(read(&card).unwrap(), b"[firmware]\ndual-boot = off\n");
    assert_eq!(card_choice(&card), Ok(Choice::Off));
    set(&card, Mode::Auto).unwrap();
    assert_eq!(card_choice(&card), Ok(Choice::Auto));
    assert_eq!(Mode::of(0), Some(Mode::Auto));
    assert_eq!(Mode::of(1), Some(Mode::Off));
    assert_eq!(Mode::of(2), None);
}

#[test]
fn quoted_and_escaped_values_are_read_as_the_firmware_reads_them() {
    // The X4's reader is `paperpaw_config::parse` per line: quotes count.
    assert_eq!(choice("[firmware]\ndual-boot = \"off\"\n"), Choice::Off);
    assert_eq!(choice("[firmware]\ndual-boot = \"auto\"\n"), Choice::Auto);
    assert_eq!(choice("[firmware]\ndual-boot = \"on\"\n"), Choice::On);
    // Unquoted, an inline `#` is part of the value: it is no `off`, so on.
    assert_eq!(choice("[firmware]\ndual-boot = off # not on\n"), Choice::On);
    // The editor sets the line the firmware reads as the key, not a copy.
    for (text, mode) in [
        ("[firmware]\ndual-boot = \"off\"\n", Mode::Auto),
        ("[firmware]\ndual-boot = \"on\"\n", Mode::Off),
    ] {
        let out = edited(text, mode);
        assert_eq!(
            choice(&out),
            if mode == Mode::Off {
                Choice::Off
            } else {
                Choice::Auto
            },
            "{text:?}"
        );
        assert_eq!(out.matches("dual-boot").count(), 1, "{out:?}");
    }
    // Text after a closing quote makes the entry an error to the reader (the
    // line is skipped, so it chooses nothing): the edit adds its own line and
    // keeps the CRLF endings.
    let out = edited("[firmware]\r\n  dual-boot=\"off\" # mine\r\n", Mode::Off);
    assert_eq!(choice(&out), Choice::Off, "{out:?}");
    assert!(out.ends_with("dual-boot = off\r\n"), "{out:?}");
}

#[test]
fn a_line_over_the_firmwares_limit_counts_by_its_start_only() {
    let long = std::format!("# {}", "x".repeat(2 * MAX_LINE));
    // A long comment changes nothing; a long entry guards (on); a long
    // header leaves the section.
    assert_eq!(
        choice(&std::format!("[firmware]\ndual-boot = off\n{long}\n")),
        Choice::Off
    );
    let entry = std::format!("[firmware]\ndual-boot = off {}\n", "w".repeat(MAX_LINE));
    assert_eq!(choice(&entry), Choice::On);
    let header = std::format!("[firmware]\ndual-boot = off\n[{}]\n", "z".repeat(MAX_LINE));
    assert_eq!(choice(&header), Choice::Off);
    // The editor replaces that guarding entry and reads back what it asked.
    assert_eq!(choice(&edited(&entry, Mode::Off)), Choice::Off);
    assert_eq!(choice(&edited(&entry, Mode::Auto)), Choice::Auto);
}

#[test]
fn every_carriage_return_is_dropped_as_the_firmware_does() {
    // A stray CR inside a line is no part of it: `dual-boot = o\rff` is `off`.
    assert_eq!(choice("[firmware]\ndual-boot = o\rff\n"), Choice::Off);
    assert_eq!(choice("[firmware]\r\r\ndual-\rboot = off\r\n"), Choice::Off);
    // Only the bytes kept count toward the line limit: 256 of them plus
    // CRs is one line, one byte more is too long (a long entry guards).
    let fits = std::format!("[firmware]\ndual-boot = off{}\n", "\r".repeat(40));
    assert_eq!(choice(&fits), Choice::Off);
    let pad = MAX_LINE - "dual-boot = off".len();
    let at = std::format!("[firmware]\ndual-boot = off{}\r\n", " ".repeat(pad));
    assert_eq!(choice(&at), Choice::Off);
    let over = std::format!("[firmware]\ndual-boot = off{}x\r\n", " ".repeat(pad));
    assert_eq!(choice(&over), Choice::On);
    // The key cut by the limit is no key (the reader's buffer ends first).
    let cut = std::format!(
        "[firmware]\n{}dual-boot = off{}\n",
        " ".repeat(MAX_LINE - 4),
        "x".repeat(10)
    );
    assert_eq!(choice(&cut), Choice::Auto);
}

#[test]
fn a_line_that_is_no_utf8_is_skipped_by_the_reader_and_refuses_the_edit() {
    let card = MemoryStorage::new();
    card.write(INI_PATH, b"[firmware]\n\xff\xfe\ndual-boot = off\n")
        .unwrap();
    assert_eq!(card_choice(&card), Ok(Choice::Off));
    assert_eq!(set(&card, Mode::Auto), Err(Failure::Unreadable));
}

proptest::proptest! {
    /// Whatever the file, an edit that is written reads back as asked, and
    /// the rest of the text is not lost.
    #[test]
    fn an_edit_always_reads_back_as_the_mode_asked(
        lines in proptest::collection::vec(
            proptest::prop_oneof![
                proptest::strategy::Just(String::from("[firmware]")),
                proptest::strategy::Just(String::from("[core]")),
                proptest::strategy::Just(String::from("dual-boot = off")),
                proptest::strategy::Just(String::from("dual-boot = \"off\"")),
                proptest::strategy::Just(String::from("dual-boot = on")),
                proptest::strategy::Just(String::from("  DUAL-BOOT = auto # x")),
                proptest::strategy::Just(String::from("# dual-boot = on")),
                proptest::strategy::Just(String::from("x = \"[firmware]\"")),
                proptest::strategy::Just(format!("dual-boot = off {}", "w".repeat(300))),
                proptest::strategy::Just(format!("[{}]", "z".repeat(300))),
                "[ -~]{0,40}",
            ],
            0..12,
        ),
        crlf in proptest::bool::ANY,
        last_newline in proptest::bool::ANY,
        off in proptest::bool::ANY,
    ) {
        let mut text = lines.join(if crlf { "\r\n" } else { "\n" });
        if last_newline && !text.is_empty() {
            text.push('\n');
        }
        let (mode, want) = if off {
            (Mode::Off, Choice::Off)
        } else {
            (Mode::Auto, Choice::Auto)
        };
        match edit(&text, mode) {
            Ok(out) => proptest::prop_assert_eq!(choice_of(&out), want),
            // A refusal is allowed (nothing is written), a wrong file is not.
            Err(failure) => proptest::prop_assert_eq!(failure, Failure::Unexpected),
        }
    }
}
