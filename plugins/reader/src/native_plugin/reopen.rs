//! Giving the documents of reading screens back, and opening their books again.
use super::*;

/// Gives back the documents of the reading screens other than `instance`: the
/// host lets a plugin keep few open (one on the X4). Each keeps its book in
/// `resting` and opens it again when it is on top again.
pub(super) fn rest(
    host: &Host<'_>,
    (docs, library): (&Documents, Option<&Library>),
    (screens, resting): (
        &mut Screens<View>,
        &mut Vec<(Instance, String, &'static str)>,
    ),
    instance: Instance,
) -> Result<(), Error> {
    for (other, view) in screens.all_with_instance() {
        if other == instance {
            continue;
        }
        let Some((path, root)) = view.book() else {
            continue;
        };
        let path = text::copy(path)?;
        mem::push(resting, (other, path, root))?;
        view.close(host, (docs, library));
    }
    Ok(())
}

impl NativeReader {
    /// The publisher's styles were turned on or off: a book is converted with
    /// them or without (Beta 3 opened it again at its place), so every screen
    /// gives its document back, which keeps the place, and opens the book anew.
    #[inline(never)]
    pub(super) fn reopen_books(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(docs) = &self.docs else {
            return Ok(());
        };
        let library = self.library.as_ref();
        let mut again = Vec::new();
        for (instance, view) in self.screens.all_with_instance() {
            let Some((path, root)) = view.book() else {
                continue;
            };
            mem::push(&mut self.resting, (instance, text::copy(path)?, root))?;
            view.close(host, (docs, library));
            mem::push(&mut again, instance)?;
        }
        for instance in again {
            self.wake(host, instance)?;
        }
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch)
            .queue_redraw(RedrawMode::Full)
            .ok();
        Ok(())
    }
}
