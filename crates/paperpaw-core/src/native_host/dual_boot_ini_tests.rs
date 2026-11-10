#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
//! `paperpaw.ini`'s `dual-boot`: read as the firmware reads it, set in the
//! last `[firmware]` section with the rest of the file as it was.
extern crate std;
use super::*;
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
    assert_eq!(card_choice(&card), Err(Failure::Unreadable));
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
