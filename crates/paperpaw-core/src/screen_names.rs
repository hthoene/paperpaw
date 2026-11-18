//! Every screen plugins added, by name: the kernel's list (#300, was the
//! SDK's `ScreenList`).

use alloc::borrow::Cow;

use paperpaw_sdk::Id;

/// Every screen plugins added, with the translation key of its name
/// ([`Place::screens`](crate::Place::screens)): interface-6 plugins', interface-7
/// plugins' ([`Titled`](paperpaw_sdk::Titled)) and those kept in the program.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScreenNames<'a> {
    added: &'a [(Id, Cow<'static, str>)],
    native: &'a [paperpaw_sdk::Titled],
    fixed: &'a [&'static paperpaw_sdk::TitleEntry],
}

impl<'a> ScreenNames<'a> {
    /// The screens of `added`, `native` and `fixed`, each sorted by id.
    pub(crate) fn new(
        added: &'a [(Id, Cow<'static, str>)],
        native: &'a [paperpaw_sdk::Titled],
        fixed: &'a [&'static paperpaw_sdk::TitleEntry],
    ) -> Self {
        Self {
            added,
            native,
            fixed,
        }
    }

    /// Each screen's id and the translation key of its name: those plugins
    /// added in the order of their ids, then those kept in the program.
    pub fn iter(self) -> impl Iterator<Item = (&'a Id, &'a str)> + 'a {
        let mut added = self
            .added
            .iter()
            .map(|(id, title)| (id, title.as_ref()))
            .peekable();
        let mut native = self
            .native
            .iter()
            .map(|titled| (titled.id(), titled.title()))
            .peekable();
        let merged = core::iter::from_fn(move || match (added.peek(), native.peek()) {
            (Some(a), Some(n)) if n.0 < a.0 => native.next(),
            (Some(_), _) => added.next(),
            (None, _) => native.next(),
        });
        let fixed = self.fixed.iter().map(|f| (&f.id, f.title));
        merged.chain(fixed)
    }

    pub fn len(&self) -> usize {
        self.added.len() + self.native.len() + self.fixed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
