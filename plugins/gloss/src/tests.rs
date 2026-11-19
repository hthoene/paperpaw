use crate::glossary::*;
use paperpaw_plugin_reader::stages::reader_text_v1 as text;
use paperpaw_sdk::native::page_layers::TextAnswer;
use std::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

fn list(file: &str, part: usize) -> Glossary {
    let mut glossary = Glossary::default();
    for chunk in file.as_bytes().chunks(part) {
        glossary.read(chunk).unwrap();
    }
    glossary.finish().unwrap();
    glossary
}

/// The glosses of the answer for `text`: the word they sit over, their small
/// text, kind and size.
fn placed(text_of: &str, glossary: &Glossary, size: u16) -> Vec<(String, String, u32, u32)> {
    let mut out = vec![0; 3840];
    let mut answer = TextAnswer::new(&mut out).unwrap();
    super::glossary::answer(&mut answer, text_of, glossary, size).unwrap();
    let len = answer.finish().unwrap();
    text::reply_decode(&out[..len])
        .unwrap()
        .map(|row| match row.unwrap() {
            text::Record::TextGloss(g) => (
                text_of[g.from as usize..g.to as usize].to_string(),
                g.text.to_string(),
                g.kind,
                g.size,
            ),
            other => panic!("{other:?}"),
        })
        .collect()
}

const LIST: &str = "# a list\nHaus\thouse\nMaus\tmouse\nwell-known\tbekannt\nDon't\tnicht\nHund\t\nkein tab\n\
                    Haus\tsecond text\n";

#[test]
fn the_small_text_is_placed_over_the_right_word_in_any_parts_of_the_file() {
    for part in [3, 1024] {
        let glossary = list(LIST, part);
        assert_eq!(
            glossary.len(),
            5,
            "comments, empty and tab-less lines left out"
        );
        let got = placed(
            "Das Haus, die Maus und der Hund: a well-known house; don’t go. Hausmaus!",
            &glossary,
            40,
        );
        // Whole words only (not Hausmaus), any case, a word listed twice keeps
        // its first text, ’ is '; ruby (0) at the size asked.
        let rows: Vec<(&str, &str)> = got.iter().map(|g| (g.0.as_str(), g.1.as_str())).collect();
        assert_eq!(
            rows,
            [
                ("Haus", "house"),
                ("Maus", "mouse"),
                ("well-known", "bekannt"),
                ("don’t", "nicht")
            ],
            "part {part}"
        );
        assert!(got.iter().all(|g| g.2 == 0 && g.3 == 40));
    }
}

#[test]
fn a_script_without_spaces_gets_the_longest_phrase_of_the_list() {
    let glossary = list("日本\tにほん\n日本語\tにほんご\n語\tご\n私\tわたし\n", 64);
    let got = placed("私は日本語を話す。日本", &glossary, 50);
    let rows: Vec<(&str, &str)> = got.iter().map(|g| (g.0.as_str(), g.1.as_str())).collect();
    assert_eq!(
        rows,
        [("私", "わたし"), ("日本語", "にほんご"), ("日本", "にほん")]
    );
}

#[test]
fn a_long_small_text_is_cut_at_a_character_and_a_long_line_is_left_out() {
    let long = "é".repeat(LONGEST_GLOSS);
    let file = format!("a\t{long}\n{}\tx\nb\tfine", "z".repeat(LONGEST_LINE + 1));
    let glossary = list(&file, 100);
    assert_eq!(glossary.len(), 2);
    let small = glossary.find(key("a")).unwrap();
    assert_eq!(small.len(), LONGEST_GLOSS);
    assert!(small.chars().all(|c| c == 'é'));
    assert_eq!(glossary.find(key("b")), Some("fine"));
    assert_eq!(key("“…”"), 0);
}

#[test]
fn the_files_are_the_books_then_its_languages() {
    assert_eq!(book_file("books/Faust.epub").as_deref(), Some("Faust.txt"));
    assert_eq!(book_file("a/b.c/plain").as_deref(), Some("plain.txt"));
    assert_eq!(book_file("books/.epub"), None);
    assert_eq!(language_file("de-AT").as_deref(), Some("de.txt"));
    assert_eq!(language_file("JA").as_deref(), Some("ja.txt"));
    assert_eq!(language_file(""), None);
    assert_eq!(language_file("../x"), None);
}

#[test]
fn the_file_changes_the_hash_for_the_layouts_key() {
    assert_ne!(list("a\tb\n", 8).hash(), list("a\tc\n", 8).hash());
    assert_eq!(list("a\tb\n", 1).hash(), list("a\tb\n", 8).hash());
}
