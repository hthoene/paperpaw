//! The note pinned to the sleep screen: a small file in the plugin's data
//! folder, so that the screen is made without asking the library (a large
//! library takes longer to pass than the screen may take). Held on a note in
//! *All notes* pins it (held again, it is let go); the file keeps the time it
//! was pinned (0 where the clock is not known), the book's title, the note's
//! text and what the note's row said below its text, one to a line.
use alloc::string::String;
use paperpaw_sdk::native::Error;

/// The file, in the plugin's data folder.
pub const FILE: &str = "pinned.txt";
/// The most bytes the file has (one write of the storage).
pub const MOST: usize = 3000;

/// A pinned note.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pinned {
    /// When it was pinned, in seconds since 1970; 0 if the clock was not set.
    pub at: i64,
    pub book: String,
    pub text: String,
    pub detail: String,
}

/// `text` on one line: breaks and tabs become spaces, cut at `max` bytes at a
/// letter's end.
fn line(text: &str, max: usize) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(text.len().min(max))
        .map_err(|_| Error::NoMemory)?;
    for c in text.chars() {
        let c = if c.is_control() { ' ' } else { c };
        if out.len() + c.len_utf8() > max {
            break;
        }
        out.push(c);
    }
    Ok(out)
}

impl Pinned {
    /// The note with its texts cut to what the file holds.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(at: i64, book: &str, text: &str, detail: &str) -> Result<Self, Error> {
        Ok(Self {
            at,
            book: line(book, 200)?,
            text: line(text, 1800)?,
            detail: line(detail, 300)?,
        })
    }

    /// The file's text.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn encode(&self) -> Result<String, Error> {
        let mut out = String::new();
        out.try_reserve(self.book.len() + self.text.len() + self.detail.len() + 32)
            .map_err(|_| Error::NoMemory)?;
        use core::fmt::Write;
        write!(
            out,
            "{}\n{}\n{}\n{}\n",
            self.at, self.book, self.text, self.detail
        )
        .map_err(|_| Error::NoMemory)?;
        Ok(out)
    }

    /// The note a file's text holds; `None` for a text that is not one.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn decode(text: &str) -> Result<Option<Self>, Error> {
        let mut lines = text.split('\n');
        let (Some(at), Some(book), Some(body), Some(detail)) =
            (lines.next(), lines.next(), lines.next(), lines.next())
        else {
            return Ok(None);
        };
        let Ok(at) = at.parse() else {
            return Ok(None);
        };
        if body.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self::new(at, book, body, detail)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pinned_note_comes_back_as_it_was_written() {
        let note = Pinned::new(
            1_795_000_000,
            "Frankl",
            "“Wer ein Warum hat”",
            "Highlight · 12 %",
        )
        .unwrap();
        let text = note.encode().unwrap();
        assert_eq!(Pinned::decode(&text).unwrap(), Some(note));
    }

    #[test]
    fn breaks_in_a_text_become_spaces_and_a_long_one_is_cut() {
        let note = Pinned::new(0, "A\nB", &"é".repeat(2000), "").unwrap();
        assert_eq!(note.book, "A B");
        assert!(note.text.len() <= 1800 && note.text.chars().all(|c| c == 'é'));
        assert!(note.encode().unwrap().len() <= MOST);
    }

    #[test]
    fn a_file_that_is_not_a_note_is_none() {
        assert_eq!(Pinned::decode("").unwrap(), None);
        assert_eq!(Pinned::decode("x\nb\nt\nd\n").unwrap(), None);
        assert_eq!(Pinned::decode("5\nb\n\nd\n").unwrap(), None);
    }
}
