use super::*;
use alloc::vec;

const FILE: &str = include_str!("../example/alice-in-wonderland.txt");

fn index_of(file: &str, part: usize) -> Index {
    let mut index = Index::default();
    for chunk in file.as_bytes().chunks(part) {
        index.read(chunk).unwrap();
    }
    index.finish().unwrap();
    index
}

#[test]
fn a_side_file_is_found_by_the_first_letters_of_its_sentences_in_any_parts() {
    for part in [7, 1024] {
        let index = index_of(FILE, part);
        assert_eq!(index.len(), 2, "comments left out");
        // The book's quotes, spaces and case need not be the file's.
        let at = index
            .find(key(
                "so  SHE was considering in her own mind (as well as she",
            ))
            .unwrap();
        let found = translation(&FILE.as_bytes()[at as usize..]);
        assert!(found.starts_with("So überlegte sie"), "{found}");
        assert!(found.ends_with("vorbeilief."));
    }
    assert_eq!(index_of(FILE, 64).find(key("Down the rabbit hole")), None);
    assert_eq!(key("“…”"), 0);
}

#[test]
fn lines_too_long_or_without_a_translation_are_left_out() {
    let long = "a".repeat(LONGEST_LINE + 1);
    let file = alloc::format!("{long}\tx\nno tab here\nshort\t \nlast\tthe end");
    let index = index_of(&file, 100);
    assert_eq!(index.len(), 1);
    let at = index.find(key("last")).unwrap() as usize;
    assert_eq!(translation(&file.as_bytes()[at..]), "the end");
}

#[test]
fn a_text_is_cut_into_sentences_with_what_closes_them() {
    let text = "“Oh dear! Oh dear! I shall be late!” (when she thought it over) she said... Mr. ok";
    let parts: Vec<&str> = sentences(text)
        .into_iter()
        .map(|r| &text[r.start as usize..r.end as usize])
        .collect();
    assert_eq!(
        parts,
        [
            "“Oh dear!",
            "Oh dear!",
            "I shall be late!”",
            "(when she thought it over) she said...",
            "Mr.",
            "ok"
        ]
    );
    assert!(sentences("   ").is_empty());
}

#[test]
fn a_block_is_answered_under_its_sentences_or_after_it() {
    let text = "One. Two! Three?";
    let found = |s: &str| (s != "Two!").then(|| String::from("x ") + s);
    for (place, rows) in [(Place::Under, 2), (Place::After, 1)] {
        let mut out = vec![0; 512];
        let mut answer = TextAnswer::new(&mut out).unwrap();
        super::answer(&mut answer, text, place, found).unwrap();
        let len = answer.finish().unwrap();
        // The count of the batch's records comes first.
        assert_eq!(out[0], rows, "{place:?}, {len}");
    }
    assert_eq!(
        side_file("books/alice-in-wonderland.epub").as_deref(),
        Some("alice-in-wonderland.txt")
    );
    assert_eq!(side_file("a/b.c/plain").as_deref(), Some("plain.txt"));
    assert_eq!(side_file("books/.epub"), None);
}
