//! After the reader wrote a book's notes as Markdown it runs the stage
//! `reader.notes-exported` (`stages/reader.notes-exported@1.def`), so a notes
//! app or a sync takes the file further: the interface-6 event
//! `core.notes-exported` as a stage. Subscribers answer with `notes.done`;
//! the reader ignores the answers and every refusal (the export is done and
//! said so to the user whatever they do).
use crate::stages::reader_notes_exported_v1 as stage;
use alloc::vec::Vec;
use paperpaw_sdk::native::{Error, Host, stages::Stage};

/// The root of the user's files on the card: where the notes file is.
pub const ROOT_FILES: u32 = 1;

/// The longest title the stage carries; a longer one is cut at a character.
pub const MAX_TITLE: usize = stage::NOTES_EXPORTED_TITLE_MAX_BYTES;

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
/// [`Error::NoSpace`] for a path over the schema's 512 bytes (it is not cut: a
/// cut path names another file); [`Error::NoMemory`].
pub fn input(root: u32, path: &str, title: &str) -> Result<Vec<u8>, Error> {
    if path.len() > stage::NOTES_EXPORTED_PATH_MAX_BYTES {
        return Err(Error::NoSpace);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(stage::MAX_INPUT)
        .map_err(|_| Error::NoMemory)?;
    bytes.resize(stage::MAX_INPUT, 0);
    let record =
        stage::Record::NotesExported(stage::NotesExported::new(root, path, cut(title, MAX_TITLE)));
    let length = stage::input_encode(&[record], &mut bytes).map_err(|_| Error::Invalid)?;
    bytes.truncate(length);
    Ok(bytes)
}

/// Runs the stage for the notes file at `path` (in `root`): every subscriber
/// is asked once, in stage order; what they answer or fail with is not read.
///
/// # Errors
/// What the input or the host refuses; none of it concerns the export.
pub fn run(host: &Host<'_>, root: u32, path: &str, title: &str) -> Result<(), Error> {
    let input = input(root, path, title)?;
    let mut out = Vec::new();
    let room = Stage::out_len(stage::MAX_REPLY as u32);
    out.try_reserve_exact(room).map_err(|_| Error::NoMemory)?;
    out.resize(room, 0);
    let stage = Stage::new(host, stage::KEY)?;
    let mut run = stage.run(&input, &mut out);
    while run.next()?.is_some() {}
    Ok(())
}

/// What the export does once the file is (or is not) written: the stage runs
/// after a successful write only, and a failure of the run changes nothing.
/// Returns whether the notes were written.
pub fn after_write<W, E>(written: Result<(), W>, announce: impl FnOnce() -> Result<(), E>) -> bool {
    if written.is_err() {
        return false;
    }
    // Whatever the run does is not the export's: the notes are written.
    announce().ok();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    #[test]
    fn the_stage_runs_after_a_write_and_not_after_a_failure() {
        let runs = Cell::new(0);
        let count = || {
            runs.set(runs.get() + 1);
            Ok::<(), ()>(())
        };
        assert!(after_write(Ok::<(), ()>(()), count));
        assert_eq!(runs.get(), 1);
        assert!(!after_write(Err::<(), ()>(()), count));
        assert_eq!(runs.get(), 1, "no run when the file was not written");
    }

    #[test]
    fn a_refused_run_does_not_undo_the_export() {
        assert!(after_write(Ok::<(), ()>(()), || Err::<(), _>(
            Error::Denied
        )));
    }

    #[test]
    fn the_input_is_the_record_the_schema_says() {
        let bytes = input(ROOT_FILES, "Notes/A - B.md", "B").unwrap();
        let mut records = stage::input_decode(&bytes).unwrap();
        let Some(Ok(stage::Record::NotesExported(row))) = records.next() else {
            panic!("one notes.exported record");
        };
        assert_eq!((row.root, row.path, row.title), (1, "Notes/A - B.md", "B"));
        assert!(records.next().is_none());
    }

    #[test]
    fn a_long_title_is_cut_at_a_character_and_a_long_path_is_refused() {
        let title = "é".repeat(200);
        let bytes = input(ROOT_FILES, "Notes/a.md", &title).unwrap();
        let mut records = stage::input_decode(&bytes).unwrap();
        let Some(Ok(stage::Record::NotesExported(row))) = records.next() else {
            panic!("one record");
        };
        assert!(row.title.len() <= MAX_TITLE);
        assert!(row.title.chars().all(|c| c == 'é'));
        assert_eq!(
            input(ROOT_FILES, &"a".repeat(513), "t").err(),
            Some(Error::NoSpace)
        );
        assert!(input(ROOT_FILES, &"a".repeat(512), "t").is_ok());
    }

    #[test]
    fn building_the_input_survives_every_refused_allocation() {
        crate::fail_alloc::sweep(|| input(ROOT_FILES, "Notes/a.md", "t").map(|v| v.len()));
    }
}
