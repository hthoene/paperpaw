//! Highlights and notes of the page on show: what the marking built the bar's
//! items with, the highlights read again after a change, and the note typed
//! for a highlight (`native_marks::edit`). The plugin asks for the note (it
//! holds the `core.ui` handle and the answer comes to it) and gives it back
//! with [`View::note_entered`].
use super::{Body, Error, Host, Library, Open, View, native_marks, native_place};
use crate::native_marking::act::NoteAsk;
use crate::native_marks::{Span, edit};

impl Open {
    /// The highlights read again from the library (a change was made). Where
    /// the library cannot say, the ones held stay.
    pub(super) fn reload_marks(
        &mut self,
        host: &Host<'_>,
        library: Option<&Library>,
    ) -> Result<(), Error> {
        let Some(library) = library else {
            return Ok(());
        };
        match native_marks::load(host, library, &self.path, self.conversion) {
            Ok(spans) => self.marks.replace(spans),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {}
        }
        Ok(())
    }
}

impl View {
    /// The note *Note* started on the last input: where it goes and the note it
    /// has, to be asked for (taken: once).
    pub fn take_note(&mut self) -> Option<NoteAsk> {
        match &mut self.body {
            Body::Open(open) => open.marking.take_note(),
            _ => None,
        }
    }

    /// The note typed for the highlight at `target`: set on it (an empty note
    /// takes it away), the highlights read again, the page drawn again. A change
    /// the library refuses is said.
    ///
    /// # Errors
    /// `NoMemory`, and the host's refusal to redraw.
    pub fn note_entered(
        &mut self,
        host: &Host<'_>,
        library: Option<&Library>,
        target: Span,
        note: &str,
    ) -> Result<(), Error> {
        let (Body::Open(open), Some(library)) = (&mut self.body, library) else {
            return Ok(());
        };
        let done = edit::set_note(
            host,
            library,
            &open.path,
            (target, open.conversion, native_place::wall_seconds(host)),
            note,
        );
        match done {
            Ok(()) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => crate::native_bookmark::say_not_saved(host),
        }
        open.reload_marks(host, Some(library))?;
        super::redraw(host, super::RedrawMode::Partial)
    }
}
