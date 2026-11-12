use super::*;
use crate::fail_alloc::sweep;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::cell::Cell;

/// A font of Latin letters only (as the X4's firmware has): it draws what is
/// below U+0250 and nothing else. `calls` counts the questions.
fn latin<'a>(calls: &'a Cell<usize>) -> impl FnMut(&str) -> Result<usize, Error> + 'a {
    move |text: &str| {
        calls.set(calls.get() + 1);
        Ok(text
            .char_indices()
            .find(|(_, c)| u32::from(*c) >= 0x250)
            .map_or(text.len(), |(at, _)| at))
    }
}

/// A font that draws `extra` too (the pack of its letters installed).
fn latin_and(extra: &'static str) -> impl FnMut(&str) -> Result<usize, Error> {
    move |text: &str| {
        Ok(text
            .char_indices()
            .find(|(_, c)| u32::from(*c) >= 0x250 && script_of(*c) != Some(extra))
            .map_or(text.len(), |(at, _)| at))
    }
}

/// A font that draws every letter but the Hangul syllables (Hangul's
/// letters of everyday text are there, the rare syllables not).
fn no_syllables(text: &str) -> Result<usize, Error> {
    Ok(text
        .char_indices()
        .find(|(_, c)| is_syllable(*c))
        .map_or(text.len(), |(at, _)| at))
}

/// A book of sections of blocks: `read` as the hint asks it.
struct Book(Vec<Vec<&'static str>>);

impl Book {
    /// The block at `offset` of `section` (each block is ten source bytes
    /// long), as the document gives it.
    fn read(&self, section: u32, offset: u32) -> Option<(String, u32)> {
        let blocks = self.0.get(usize::try_from(section).ok()?)?;
        let at = usize::try_from(offset / 10).ok()?;
        blocks.get(at).map(|text| (text.to_string(), offset + 10))
    }

    fn sections(&self) -> u32 {
        u32::try_from(self.0.len()).unwrap()
    }

    fn lacking(&self, covers: &mut dyn FnMut(&str) -> Result<usize, Error>) -> Lacking {
        lacking(self.sections(), |s, o| self.read(s, o), covers).unwrap()
    }
}

fn scripts(found: &Lacking) -> Vec<&'static str> {
    found.scripts().collect()
}

#[test]
fn the_letters_of_each_script_are_beta_3s() {
    assert_eq!(script_of('α'), Some("Grek"));
    assert_eq!(script_of('ἀ'), Some("Grek"));
    assert_eq!(script_of('ж'), Some("Cyrl"));
    assert_eq!(script_of('ꙮ'), Some("Cyrl"));
    assert_eq!(script_of('ع'), Some("Arab"));
    assert_eq!(script_of('ﻻ'), Some("Arab"));
    assert_eq!(script_of('א'), Some("Hebr"));
    assert_eq!(script_of('한'), Some("Kore"));
    assert_eq!(script_of('ᄀ'), Some("Kore"));
    assert_eq!(script_of('a'), None);
    assert_eq!(script_of('é'), None);
    assert_eq!(script_of('中'), None);
    // Every script of the table has its letter, and its pack.
    for (script, letter, pack) in SCRIPTS {
        assert_eq!(script_of(letter), Some(script));
        assert!(is_pack(pack));
    }
    assert!(is_pack(KOREAN_EXTRA) && !is_pack("dictionary"));
}

#[test]
fn the_declared_language_names_its_script() {
    assert_eq!(declared("el"), Some("Grek"));
    assert_eq!(declared("ru-RU"), Some("Cyrl"));
    assert_eq!(declared("fa"), Some("Arab"));
    assert_eq!(declared("he"), Some("Hebr"));
    assert_eq!(declared("ko"), Some("Kore"));
    // Written in Latin letters, or in a script the table has no pack of.
    assert_eq!(declared("sr-Latn"), None);
    assert_eq!(declared("en"), None);
    assert_eq!(declared("ja"), None);
    assert_eq!(declared(""), None);
}

#[test]
fn the_scripts_the_font_lacks_are_found_in_the_text_in_their_order() {
    let book = Book(alloc::vec![
        alloc::vec!["Chapter one", "Он сказал: «Привет»."],
        alloc::vec!["Ἐν ἀρχῇ ἦν ὁ λόγος", "שלום and Привет"],
    ]);
    let calls = Cell::new(0);
    let found = book.lacking(&mut latin(&calls));
    assert_eq!(scripts(&found), ["Cyrl", "Grek", "Hebr"]);
    assert!(!found.syllable());
    // A font with the Greek letters lacks the others only.
    let found = book.lacking(&mut latin_and("Grek"));
    assert_eq!(scripts(&found), ["Cyrl", "Hebr"]);
    // A book in Latin letters lacks nothing, and asks nothing.
    let calls = Cell::new(0);
    let plain = Book(alloc::vec![alloc::vec!["Plain text, café."]]);
    assert_eq!(scripts(&plain.lacking(&mut latin(&calls))), [] as [&str; 0]);
    assert_eq!(calls.get(), 0);
}

#[test]
fn a_hangul_syllable_the_font_lacks_is_told_apart_from_its_other_letters() {
    // The letters (jamo) alone: Korean lacks, no syllable.
    let jamo = Book(alloc::vec![alloc::vec!["ᄀᄂᄃ"]]);
    let calls = Cell::new(0);
    let found = jamo.lacking(&mut latin(&calls));
    assert_eq!(scripts(&found), ["Kore"]);
    assert!(!found.syllable());
    // Syllables the font lacks.
    let text = Book(alloc::vec![alloc::vec!["ᄀ 한국어"]]);
    let found = text.lacking(&mut latin(&calls));
    assert_eq!(scripts(&found), ["Kore"]);
    assert!(found.syllable());
    // A font with the letters and without the syllables.
    let found = text.lacking(&mut no_syllables);
    assert!(found.syllable());
    let found = jamo.lacking(&mut no_syllables);
    assert_eq!(scripts(&found), [] as [&str; 0]);
}

#[test]
fn only_the_first_blocks_of_the_first_sections_are_read() {
    // Greek in the fourth section is past the sections looked at.
    let mut sections = alloc::vec![alloc::vec!["Latin"]; 3];
    sections.push(alloc::vec!["αβγ"]);
    let calls = Cell::new(0);
    assert_eq!(
        scripts(&Book(sections).lacking(&mut latin(&calls))),
        [] as [&str; 0]
    );
    // Greek after the eightieth block is not read.
    let mut blocks = alloc::vec!["Latin"; BLOCKS];
    blocks.push("αβγ");
    assert_eq!(
        scripts(&Book(alloc::vec![blocks.clone()]).lacking(&mut latin(&calls))),
        [] as [&str; 0]
    );
    blocks.insert(BLOCKS - 1, "αβγ");
    assert_eq!(
        scripts(&Book(alloc::vec![blocks]).lacking(&mut latin(&calls))),
        ["Grek"]
    );
    // Nor after the letters a few pages hold.
    let long: &'static str = "a".repeat(LETTERS).leak();
    let book = Book(alloc::vec![alloc::vec![long, "αβγ"]]);
    assert_eq!(scripts(&book.lacking(&mut latin(&calls))), [] as [&str; 0]);
    // The blocks of every section are read, not only the first one's.
    let book = Book(alloc::vec![
        alloc::vec!["a"],
        alloc::vec!["b"],
        alloc::vec!["c", "ж"]
    ]);
    assert_eq!(scripts(&book.lacking(&mut latin(&calls))), ["Cyrl"]);
}

#[test]
fn a_block_that_does_not_advance_ends_its_section() {
    let mut reads = 0;
    let found = lacking(
        2,
        |section, _| {
            reads += 1;
            (reads < 100).then(|| ((if section == 0 { "a" } else { "ж" }).to_string(), 0))
        },
        &mut latin(&Cell::new(0)),
    )
    .unwrap();
    // One read of the first section (its end is not after its start), then
    // the second one's.
    assert_eq!(scripts(&found), ["Cyrl"]);
    assert_eq!(reads, 2);
}

#[test]
fn the_font_is_asked_a_few_times_not_once_a_letter() {
    // A Greek book whose font has its letters: thousands of letters, asked
    // about in runs.
    let block: &'static str = "λόγος ".repeat(200).leak();
    let book = Book(alloc::vec![alloc::vec![block; 10]]);
    let calls = Cell::new(0);
    let mut font = |text: &str| {
        calls.set(calls.get() + 1);
        assert!(text.len() <= ASK);
        Ok(text.len())
    };
    assert_eq!(scripts(&book.lacking(&mut font)), [] as [&str; 0]);
    assert!(calls.get() < 100, "{} questions", calls.get());
    // A font without them: once known, the script is not asked about again.
    let calls = Cell::new(0);
    assert_eq!(scripts(&book.lacking(&mut latin(&calls))), ["Grek"]);
    assert_eq!(calls.get(), 1);
}

#[test]
fn a_font_that_cannot_tell_asks_for_nothing() {
    let book = Book(alloc::vec![alloc::vec!["αβγ"]]);
    let mut refusing = |_: &str| Err(Error::Unsupported);
    assert_eq!(
        lacking(book.sections(), |s, o| book.read(s, o), &mut refusing),
        Err(Error::Unsupported)
    );
    // An answer inside a letter is not one.
    let mut odd = |_: &str| Ok(1);
    assert_eq!(
        lacking(book.sections(), |s, o| book.read(s, o), &mut odd),
        Err(Error::Invalid)
    );
}

fn found(text: &'static str) -> Lacking {
    Book(alloc::vec![alloc::vec![text]]).lacking(&mut latin(&Cell::new(0)))
}

fn wanted(
    found: Lacking,
    script: Option<&'static str>,
    korean: bool,
    kept: &str,
) -> Vec<&'static str> {
    let mut installed = || korean;
    needed(
        found,
        script,
        &mut latin(&Cell::new(0)),
        &mut installed,
        kept,
    )
    .unwrap()
    .ids()
    .to_vec()
}

#[test]
fn the_packs_are_those_of_the_scripts_lacking_and_not_asked_before() {
    let text = found("ж α");
    assert_eq!(
        wanted(text, None, false, ""),
        ["script-cyrillic", "script-greek"]
    );
    // Asked before for this book: not again.
    assert_eq!(
        wanted(text, None, false, "script-greek\n"),
        ["script-cyrillic"]
    );
    assert_eq!(
        wanted(text, None, false, "script-cyrillic\nscript-greek"),
        [] as [&str; 0]
    );
    // The declared language adds its script where the font lacks its letter.
    let latin_text = found("Plain");
    assert_eq!(
        wanted(latin_text, Some("Hebr"), false, ""),
        ["script-hebrew"]
    );
    assert_eq!(
        wanted(text, Some("Grek"), false, ""),
        ["script-cyrillic", "script-greek"]
    );
    let mut covered = latin_and("Hebr");
    let mut korean = || false;
    assert!(
        needed(latin_text, Some("Hebr"), &mut covered, &mut korean, "")
            .unwrap()
            .is_empty()
    );
    // At most four at once.
    let all = found("ж α ع א 한");
    assert_eq!(wanted(all, None, false, "").len(), MOST);
}

#[test]
fn korean_asks_for_the_rare_syllables_only_once_its_letters_are_there() {
    let syllables = found("한국어");
    let jamo = found("ᄀᄂ");
    // Not there: the pack of the letters of everyday text.
    assert_eq!(wanted(syllables, None, false, ""), [KOREAN]);
    assert_eq!(wanted(jamo, None, false, ""), [KOREAN]);
    // There, and syllables are still lacking: the rare ones.
    assert_eq!(wanted(syllables, None, true, ""), [KOREAN_EXTRA]);
    assert_eq!(wanted(syllables, None, true, KOREAN_EXTRA), [] as [&str; 0]);
    // There, and only other letters lacking: nothing more.
    assert_eq!(wanted(jamo, None, true, ""), [] as [&str; 0]);
    // The registry is asked only where Korean is lacking.
    let mut asked = false;
    let mut korean = || {
        asked = true;
        false
    };
    needed(found("ж"), None, &mut latin(&Cell::new(0)), &mut korean, "").unwrap();
    assert!(!asked);
}

#[test]
fn what_was_asked_is_kept_a_few_lines_long() {
    assert_eq!(
        with_asked("", &["script-greek", "script-cyrillic"]).unwrap(),
        "script-greek\nscript-cyrillic"
    );
    // Added after what was there, never twice.
    assert_eq!(
        with_asked("script-greek\n", &["script-greek", "script-hebrew"]).unwrap(),
        "script-greek\nscript-hebrew"
    );
    // The last four are kept.
    assert_eq!(with_asked("a\nb\nc", &["d", "e"]).unwrap(), "b\nc\nd\ne");
    assert!(was_asked("script-greek\nscript-hebrew", "script-hebrew"));
    assert!(!was_asked("script-greek-x", "script-greek"));
}

#[test]
fn each_pack_has_its_own_bit_so_an_answer_reaches_only_the_book_that_asked() {
    let all: Vec<Asked> = SCRIPTS
        .iter()
        .map(|(.., pack)| bit(pack))
        .chain([bit(KOREAN_EXTRA)])
        .collect();
    assert_eq!(all.iter().fold(0, |set, b| set | b).count_ones(), 6);
    assert!(all.iter().all(|b| b.count_ones() == 1));
    assert_eq!(bit("dictionary"), 0);
    assert!(!is_pack("dictionary") && is_pack(KOREAN_EXTRA));
    let asked = bits(&["script-greek", "script-hebrew"]);
    assert_ne!(asked & bit("script-greek"), 0);
    assert_eq!(asked & bit("script-cyrillic"), 0);
}

#[test]
fn an_answer_that_could_not_be_had_is_forgotten() {
    assert_eq!(effect(PluginOutcome::Failed), Effect::Forget);
    assert_eq!(effect(PluginOutcome::Installed), Effect::Relayout);
    assert_eq!(effect(PluginOutcome::Declined), Effect::Keep);
    assert_eq!(effect(PluginOutcome::NotFound), Effect::Keep);
    assert_eq!(
        without("script-greek\nscript-hebrew", "script-greek").unwrap(),
        "script-hebrew"
    );
    // Nothing left: the file goes.
    assert_eq!(without("script-greek\n", "script-greek").unwrap(), "");
    // Forgotten, it is asked for again at the next opening.
    let kept = without("script-cyrillic\nscript-greek", "script-greek").unwrap();
    assert_eq!(wanted(found("ж α"), None, false, &kept), ["script-greek"]);
}

#[test]
fn the_file_is_in_the_books_folder_as_beta_3_named_it() {
    let path = kept_path("books/lang.epub").unwrap();
    assert_eq!(
        path,
        alloc::format!("books/{:016x}/packs-asked.txt", fnv1a64("books/lang.epub"))
    );
    assert_eq!(path.len(), 23 + FILE.len());
}

#[test]
fn every_allocation_may_be_refused() {
    sweep(|| {
        kept_path("books/a.epub")?;
        with_asked("script-greek", &["script-hebrew"])?;
        without("script-greek\nscript-hebrew", "script-greek")?;
        let mut font = |text: &str| Ok(text.len());
        lacking(1, |_, _| None, &mut font)?;
        Ok(())
    });
}
