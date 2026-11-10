//! When a word is looked up the reader runs the stage
//! `reader.word-looked-up` (`stages/reader.word-looked-up@1.def`), so a
//! vocabulary list takes it: the interface-6 event `core.word-looked-up` as a
//! stage. Subscribers answer with `word.done`; the reader ignores the answers
//! and every refusal (the lookup is the user's, whatever a subscriber does).
use crate::stages::reader_word_looked_up_v1 as stage;
use alloc::vec::Vec;
use paperpaw_sdk::native::{Error, Host, stages::Stage};

/// The word's place in the book and what it was found in, as the stage carries
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Looked<'a> {
    /// The word looked up (cut at 1,024 bytes).
    pub word: &'a str,
    /// The sentence it stood in (cut at 512 bytes).
    pub sentence: &'a str,
    /// The book's language, where known (dropped over 35 bytes).
    pub language: &'a str,
    /// The book's file (not cut: a cut path names another file).
    pub path: &'a str,
    /// The book's title, where known (cut at 256 bytes).
    pub title: &'a str,
    /// The section the word is in.
    pub section: u32,
    /// The word's offset in that section.
    pub offset: u32,
}

fn cut(text: &str, most: usize) -> &str {
    let mut end = text.len().min(most);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}

/// The input of one run.
///
/// # Errors
/// [`Error::NoSpace`] for a path over the schema's 512 bytes; [`Error::NoMemory`].
pub fn input(looked: &Looked<'_>) -> Result<Vec<u8>, Error> {
    if looked.path.len() > stage::WORD_LOOKED_PATH_MAX_BYTES {
        return Err(Error::NoSpace);
    }
    let language = if looked.language.len() > stage::WORD_LOOKED_LANGUAGE_MAX_BYTES {
        ""
    } else {
        looked.language
    };
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(stage::MAX_INPUT)
        .map_err(|_| Error::NoMemory)?;
    bytes.resize(stage::MAX_INPUT, 0);
    let record = stage::Record::WordLooked(stage::WordLooked::new(
        cut(looked.word, stage::WORD_LOOKED_WORD_MAX_BYTES),
        cut(looked.sentence, stage::WORD_LOOKED_SENTENCE_MAX_BYTES),
        looked.path,
        looked.section,
        looked.offset,
        language,
        cut(looked.title, stage::WORD_LOOKED_TITLE_MAX_BYTES),
    ));
    let length = stage::input_encode(&[record], &mut bytes).map_err(|_| Error::Invalid)?;
    bytes.truncate(length);
    Ok(bytes)
}

/// Announces `looked` to `ask`, once, with the stage's input: the one place a
/// lookup becomes a stage run. `ask` does the run (the host's stage, or a
/// recorder in a test). A path the stage cannot carry is refused before `ask`
/// is called.
///
/// # Errors
/// What [`input`] refuses, and what `ask` returns.
pub fn announce(
    looked: &Looked<'_>,
    ask: impl FnOnce(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let input = input(looked)?;
    ask(&input)
}

/// Runs the stage for `looked`: every subscriber is asked once, in stage
/// order; what they answer or fail with is not read. The native lookup flow
/// calls this when the user looks a word up (the interface-6 reader still
/// fires its event: see `PORTING-7.md`).
///
/// # Errors
/// What the input or the host refuses; none of it concerns the lookup, which
/// goes on whatever this returns.
pub fn run(host: &Host<'_>, looked: &Looked<'_>) -> Result<(), Error> {
    announce(looked, |input| {
        let mut out = Vec::new();
        let room = Stage::out_len(stage::MAX_REPLY as u32);
        out.try_reserve_exact(room).map_err(|_| Error::NoMemory)?;
        out.resize(room, 0);
        let stage = Stage::new(host, stage::KEY)?;
        let mut run = stage.run(input, &mut out);
        while run.next()?.is_some() {}
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn looked<'a>(word: &'a str, path: &'a str) -> Looked<'a> {
        Looked {
            word,
            sentence: "A sentence.",
            language: "en",
            path,
            title: "Title",
            section: 3,
            offset: 40,
        }
    }

    #[test]
    fn the_input_carries_everything_a_list_needs_to_go_back_to_the_word() {
        let bytes = input(&looked("serendipity", "Books/a.epub")).unwrap();
        let mut records = stage::input_decode(&bytes).unwrap();
        let Some(Ok(stage::Record::WordLooked(r))) = records.next() else {
            panic!("no record");
        };
        assert_eq!(
            (r.word, r.sentence, r.language, r.path, r.title),
            ("serendipity", "A sentence.", "en", "Books/a.epub", "Title")
        );
        assert_eq!((r.section, r.offset), (3, 40));
    }

    #[test]
    fn long_texts_are_cut_at_a_character_and_a_long_path_is_refused() {
        let long = "é".repeat(1_000);
        let bytes = input(&looked(&long, "a.epub")).unwrap();
        let mut records = stage::input_decode(&bytes).unwrap();
        let Some(Ok(stage::Record::WordLooked(r))) = records.next() else {
            panic!("no record");
        };
        assert!(r.word.len() <= 1024 && r.word.chars().all(|c| c == 'é'));
        let path = "p".repeat(513);
        assert_eq!(input(&looked("w", &path)), Err(Error::NoSpace));
        let long_language = "l".repeat(36);
        let mut odd = looked("w", "a.epub");
        odd.language = &long_language;
        let bytes = input(&odd).unwrap();
        let mut records = stage::input_decode(&bytes).unwrap();
        let Some(Ok(stage::Record::WordLooked(r))) = records.next() else {
            panic!("no record");
        };
        assert_eq!(r.language, "");
    }

    #[test]
    fn a_lookup_is_one_stage_run_with_the_payload_of_the_word() {
        use core::cell::RefCell;
        let runs = RefCell::new(Vec::<Vec<u8>>::new());
        announce(&looked("serendipity", "Books/a.epub"), |input| {
            runs.borrow_mut().push(input.to_vec());
            Ok(())
        })
        .unwrap();
        let runs = runs.into_inner();
        assert_eq!(runs.len(), 1);
        assert_eq!(
            runs[0],
            input(&looked("serendipity", "Books/a.epub")).unwrap()
        );
        let mut records = stage::input_decode(&runs[0]).unwrap();
        assert_eq!(records.len(), 1);
        let Some(Ok(stage::Record::WordLooked(r))) = records.next() else {
            panic!("no record");
        };
        assert_eq!(
            (
                r.word, r.sentence, r.path, r.section, r.offset, r.language, r.title
            ),
            (
                "serendipity",
                "A sentence.",
                "Books/a.epub",
                3,
                40,
                "en",
                "Title"
            )
        );
    }

    #[test]
    fn a_path_over_the_limit_is_refused_and_no_run_happens() {
        let path = "p".repeat(513);
        let mut asked = false;
        let result = announce(&looked("w", &path), |_| {
            asked = true;
            Ok(())
        });
        assert_eq!(result, Err(Error::NoSpace));
        assert!(!asked, "the stage must not run with a cut path");
        // The longest the stage carries goes through whole.
        let path = "p".repeat(512);
        assert!(announce(&looked("w", &path), |_| Ok(())).is_ok());
    }

    #[test]
    fn what_the_run_fails_with_is_given_back_for_the_caller_to_ignore() {
        assert_eq!(
            announce(&looked("w", "a.epub"), |_| Err(Error::State)),
            Err(Error::State)
        );
    }
}
