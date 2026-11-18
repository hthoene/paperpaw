//! An attachment as a book: which attachments of a message are offered (a
//! base64 one of a kind of file the reader opens), by the number IMAP gives
//! the part, kept beside the letter; the name made safe for the card; the part
//! decoded from one file into another in pieces.
#![allow(clippy::unwrap_used, reason = "test assertions")]

use paperpaw_plugin_mail::native_model::attachment::decode_file;
use paperpaw_plugin_mail::native_model::codec::encode_base64_into;
use paperpaw_plugin_mail::native_model::render::{
    MAX_SAVES, Save, is_book_name, render_with_saves, safe_name,
};
use paperpaw_plugin_mail::native_model::store;
use paperpaw_sdk::{MemoryStorage, Storage};

fn base64_lines(bytes: &[u8]) -> String {
    let mut out = String::new();
    encode_base64_into(bytes, &mut out, true).unwrap();
    out
}

fn saves_of(message: &str) -> Vec<(String, String)> {
    let storage = MemoryStorage::new();
    storage.write("raw", message.as_bytes()).unwrap();
    let (_, saves) = render_with_saves(&storage, "raw", "out", 24 * 1024).unwrap();
    saves.into_iter().map(|s| (s.part, s.name)).collect()
}

fn head(kind: &str) -> String {
    format!(
        "From: a@example.org\r\nTo: b@example.org\r\nSubject: Book\r\nMIME-Version: 1.0\r\nContent-Type: {kind}\r\n\r\n"
    )
}

fn text_part(boundary: &str) -> String {
    format!("--{boundary}\r\nContent-Type: text/plain\r\n\r\nHere it is.\r\n")
}

fn file_part(boundary: &str, kind: &str, name: &str, encoding: &str, body: &str) -> String {
    format!(
        "--{boundary}\r\nContent-Type: {kind}; name=\"{name}\"\r\nContent-Disposition: attachment; filename=\"{name}\"\r\nContent-Transfer-Encoding: {encoding}\r\n\r\n{body}\r\n"
    )
}

#[test]
fn a_book_attached_to_a_mixed_message_is_offered_as_part_two() {
    let message = format!(
        "{}{}{}--b--\r\n",
        head("multipart/mixed; boundary=b"),
        text_part("b"),
        file_part("b", "application/epub+zip", "tiny-book.epub", "base64", &base64_lines(b"PK\x03\x04 a book"))
    );
    assert_eq!(saves_of(&message), [("2".to_string(), "tiny-book.epub".to_string())]);
}

#[test]
fn parts_are_numbered_as_imap_numbers_them_also_inside_other_parts() {
    // mixed [ alternative [ plain, html ], related [ html, pdf ], epub ]
    let message = format!(
        "{}--m\r\nContent-Type: multipart/alternative; boundary=a\r\n\r\n--a\r\nContent-Type: text/plain\r\n\r\nplain\r\n--a\r\nContent-Type: text/html\r\n\r\n<p>html</p>\r\n--a--\r\n\
         --m\r\nContent-Type: multipart/related; boundary=r\r\n\r\n--r\r\nContent-Type: text/html\r\n\r\n<p>x</p>\r\n{}--r--\r\n{}--m--\r\n",
        head("multipart/mixed; boundary=m"),
        file_part("r", "application/pdf", "inner.pdf", "base64", "UEsDBA=="),
        file_part("m", "application/epub+zip", "outer.epub", "base64", "UEsDBA==")
    );
    assert_eq!(
        saves_of(&message),
        [
            ("2.2".to_string(), "inner.pdf".to_string()),
            ("3".to_string(), "outer.epub".to_string())
        ]
    );
}

#[test]
fn only_base64_books_are_offered_and_at_most_four() {
    let mut message = format!(
        "{}{}{}{}",
        head("multipart/mixed; boundary=b"),
        text_part("b"),
        // Not a book, not base64: named in the attachments but not offered.
        file_part("b", "image/png", "pic.png", "base64", "UEsDBA=="),
        file_part("b", "application/epub+zip", "qp.epub", "quoted-printable", "PK=03=04"),
    );
    for n in 0..6 {
        message.push_str(&file_part("b", "application/pdf", &format!("b{n}.PDF"), "base64", "UEsDBA=="));
    }
    message.push_str("--b--\r\n");
    let saves = saves_of(&message);
    assert_eq!(saves.len(), MAX_SAVES);
    assert_eq!(saves[0], ("4".to_string(), "b0.PDF".to_string()));
    // A message that is no multipart has no attachment to offer.
    assert!(saves_of(&format!("{}hello\r\n", head("text/plain"))).is_empty());
}

#[test]
fn what_makes_a_book_and_a_name_the_card_takes() {
    for yes in ["a.epub", "A.EPUB", "x.y.pdf", "b.mobi", "c.fb2", "d.txt", "e.docx", "f.cbz", "g.azw3"] {
        assert!(is_book_name(yes), "{yes}");
    }
    for no in ["", "epub", ".epub", "a.exe", "a.epub.exe", "a.png", "a."] {
        assert!(!is_book_name(no), "{no}");
    }
    assert_eq!(safe_name("tiny-book.epub").unwrap(), "tiny-book.epub");
    // No folder, no dots in front, nothing FAT refuses, no control characters.
    assert_eq!(safe_name("../../etc/passwd.epub").unwrap(), "passwd.epub");
    assert_eq!(safe_name("C:\\Users\\a\\b.epub").unwrap(), "b.epub");
    assert_eq!(safe_name("..hidden.epub").unwrap(), "hidden.epub");
    assert_eq!(safe_name("a<b>:c|d?.epub").unwrap(), "a_b__c_d_.epub");
    assert_eq!(safe_name("a\u{0}b\n.epub").unwrap(), "a_b_.epub");
    assert_eq!(safe_name("..").unwrap(), "attachment");
    assert_eq!(safe_name("").unwrap(), "attachment");
    assert!(safe_name(&"x".repeat(500)).unwrap().len() <= 80);
    assert_eq!(safe_name("Über.epub").unwrap(), "Über.epub");
}

#[test]
fn the_saves_are_kept_beside_the_letter_and_read_back() {
    let storage = MemoryStorage::new();
    let message = format!(
        "{}{}{}--b--\r\n",
        head("multipart/mixed; boundary=b"),
        text_part("b"),
        file_part("b", "application/epub+zip", "../Tiny Book.epub", "base64", "UEsDBA==")
    );
    storage.write("m1.eml", message.as_bytes()).unwrap();
    let letter = store::make_letter(&storage, "m1.eml", "m1.txt").unwrap();
    assert_eq!(letter.attachments.len(), 1);
    let saves = store::read_saves(&storage, "m1.txt").unwrap();
    assert_eq!(
        saves,
        [Save::of("2", "Tiny Book.epub")]
    );
    // A letter made again without one takes the old file away.
    storage.write("m1.eml", format!("{}hello\r\n", head("text/plain")).as_bytes()).unwrap();
    store::make_letter(&storage, "m1.eml", "m1.txt").unwrap();
    assert!(store::read_saves(&storage, "m1.txt").unwrap().is_empty());
    assert!(storage.size("m1.att").is_err());
    // A file that is not as it was written reads as none.
    storage.write("m2.att", b"x\ty\n..\tz\nno tab\n1.2\t\n").unwrap();
    assert!(store::read_saves(&storage, "m2.txt").unwrap().is_empty());
}

#[test]
fn a_part_is_decoded_from_one_file_into_another_in_pieces() {
    let storage = MemoryStorage::new();
    // 10,000 bytes (not a multiple of the piece, nor of three).
    let bytes: Vec<u8> = (0..10_000u32).map(|n| (n * 7 % 251) as u8).collect();
    storage.write("part", base64_lines(&bytes).as_bytes()).unwrap();
    let size = decode_file(&storage, "part", "book").unwrap();
    assert_eq!(size, bytes.len() as u64);
    let mut got = vec![0u8; 11_000];
    let n = storage.read_at("book", 0, &mut got).unwrap();
    assert_eq!(&got[..n], &bytes[..]);
    // The padding and the line breaks of one or two bytes at the end.
    for len in [0usize, 1, 2, 3, 4] {
        storage.write("part", base64_lines(&bytes[..len]).as_bytes()).unwrap();
        assert_eq!(decode_file(&storage, "part", "book").unwrap(), len as u64);
    }
}
