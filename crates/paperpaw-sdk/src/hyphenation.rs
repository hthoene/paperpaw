//! Hyphenation tables by language: compiled into a plugin
//! ([`HyphenationSpec`]), or files on the card that data plugins name in
//! their `[hyphenation]` section, read in place a few blocks at a time.

use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_hyphen::Patterns;

use crate::id::Id;
use crate::plugin::RegisterError;

mod card;

#[cfg(feature = "kernel")]
pub use card::CardTable;
use card::CardTables;

/// Hyphenation patterns for a language, compiled with
/// [`hyphen::compile`](crate::hyphen::compile).
#[non_exhaustive]
pub struct HyphenationSpec {
    pub id: Id,
    /// Language tag, e.g. `de` or `en-US`.
    pub language: Cow<'static, str>,
    pub patterns: &'static [u8],
}

impl HyphenationSpec {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(id: Id, language: impl Into<Cow<'static, str>>, patterns: &'static [u8]) -> Self {
        Self {
            id,
            language: language.into(),
            patterns,
        }
    }
}

impl fmt::Debug for HyphenationSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HyphenationSpec")
            .field("id", &self.id)
            .field("language", &self.language)
            .finish_non_exhaustive()
    }
}

/// The hyphenation tables there are: those compiled into plugins and
/// those of data plugins on the card.
#[derive(Debug, Default)]
pub struct Hyphenation {
    languages: Vec<(Id, Cow<'static, str>, Patterns<'static>)>,
    card: CardTables,
}

impl Hyphenation {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, spec: HyphenationSpec) -> Result<(), RegisterError> {
        if self.languages.iter().any(|(id, ..)| *id == spec.id) {
            return Err(RegisterError::Duplicate { id: spec.id });
        }
        let patterns = Patterns::new(spec.patterns).map_err(|e| RegisterError::Invalid {
            message: Cow::Owned(alloc::format!("{}: {e}", spec.id)),
        })?;
        self.languages.push((spec.id, spec.language, patterns));
        Ok(())
    }

    pub fn contains(&self, id: &str) -> bool {
        self.languages.iter().any(|(i, ..)| i.as_str() == id)
    }

    /// Releases unused capacity once registration is complete.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.languages.shrink_to_fit();
    }

    /// The table for `language` (e.g. `de-AT`), `None` where there is none.
    ///
    /// A data plugin's table on the card comes first: the tag exactly, then
    /// without its last subtag, again and again (`de-ch-1996`, `de-ch`,
    /// `de`), never a longer tag; where several plugins name a language,
    /// the greatest plugin id has it. A table that could not be read (a
    /// second time, after a failed read) is none. Then a table compiled
    /// into a plugin: the tag, its base language (`de`), or another tag of
    /// that base (`en-US` serves `en`); later registrations win.
    pub fn get(&self, language: &str) -> Option<Hyphenator<'_>> {
        if let Some(serial) = self.card.find(language) {
            return Some(Hyphenator::new(Table::Card(&self.card, serial)));
        }
        self.compiled(language)
            .map(|patterns| Hyphenator::new(Table::Compiled(*patterns)))
    }

    /// A table compiled into a plugin for `language`, as [`get`](Self::get)
    /// finds one.
    fn compiled(&self, language: &str) -> Option<&Patterns<'static>> {
        let base = language.split(['-', '_']).next().unwrap_or(language);
        let find = |tag: &str| {
            self.languages
                .iter()
                .rev()
                .find(|(_, l, _)| l.eq_ignore_ascii_case(tag))
                .map(|(.., p)| p)
        };
        find(language).or_else(|| {
            // `en` also serves `en-US`, and `en-US` serves `en`.
            find(base).or_else(|| {
                self.languages
                    .iter()
                    .rev()
                    .find(|(_, l, _)| {
                        l.split(['-', '_'])
                            .next()
                            .is_some_and(|b| b.eq_ignore_ascii_case(base))
                    })
                    .map(|(.., p)| p)
            })
        })
    }
}

/// The card's part, which only the kernel changes.
#[cfg(feature = "kernel")]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
impl Hyphenation {
    /// The storage the tables of data plugins are read from (the card).
    /// Every table open is closed: it is opened anew from `storage`.
    pub fn set_storage(&mut self, storage: alloc::rc::Rc<dyn paperpaw_platform::Storage>) {
        self.card.set_storage(storage);
    }

    /// Adds the tables of data plugin `plugin`, whose folder is `folder`:
    /// `tables` are its language tags and the files, relative to `folder`.
    /// All or none: room is made first, so nothing is added where it cannot
    /// be.
    ///
    /// # Errors
    /// No memory for them; nothing was added.
    pub fn add_tables<'t>(
        &mut self,
        plugin: &str,
        folder: &str,
        tables: impl ExactSizeIterator<Item = (&'t str, &'t str)>,
    ) -> Result<(), alloc::collections::TryReserveError> {
        self.card.add(plugin, folder, tables)
    }

    /// Takes out the tables of `plugin` (removed, or about to be loaded
    /// anew), closing those open: another plugin's table for a language has
    /// it from now on.
    pub fn remove_tables(&mut self, plugin: &str) {
        self.card.remove(plugin);
    }

    /// The tables of `plugin` that another plugin's keeps out, and the
    /// tables of others it keeps out, each as (left out, has the language):
    /// for the warnings as it is added.
    pub fn collisions<'a>(
        &'a self,
        plugin: &'a str,
    ) -> impl Iterator<Item = (CardTable<'a>, CardTable<'a>)> + 'a {
        self.card.collisions(plugin)
    }

    /// The data plugin's table that [`get`](Self::get) reads for
    /// `language`, where one is.
    pub fn card_table(&self, language: &str) -> Option<CardTable<'_>> {
        self.card.table(language)
    }

    /// The card may hold other files than before (a remount, the end of USB
    /// drive mode): every table open is closed, and one that could not be
    /// read is tried again.
    pub fn card_changed(&self) {
        self.card.changed();
    }
}

/// A language's hyphenation table, as [`Hyphenation::get`] finds it.
#[derive(Clone, Copy)]
pub struct Hyphenator<'a> {
    table: Table<'a>,
    left: usize,
    right: usize,
}

#[derive(Clone, Copy)]
enum Table<'a> {
    Compiled(Patterns<'static>),
    /// The card's tables and the serial number of the one to read.
    Card(&'a CardTables, u32),
}

impl<'a> Hyphenator<'a> {
    fn new(table: Table<'a>) -> Self {
        Self {
            table,
            left: 0,
            right: 0,
        }
    }

    /// This table with other minimums of characters before and after a
    /// hyphen, where they are not 0 (0 keeps the table's).
    #[must_use]
    pub fn with_minimums(self, left: usize, right: usize) -> Self {
        Self {
            left: if left > 0 { left } else { self.left },
            right: if right > 0 { right } else { self.right },
            ..self
        }
    }

    /// Calls `out` with each byte offset in `word` where it may be
    /// hyphenated, in ascending order. `word` should consist of letters
    /// only; it is matched without regard to case. A word is left whole
    /// where the table cannot be read.
    pub fn hyphenate(&self, word: &str, out: &mut dyn FnMut(usize)) {
        match self.table {
            Table::Compiled(patterns) => patterns
                .with_minimums(self.left, self.right)
                .hyphenate(word, out),
            Table::Card(card, serial) => card.hyphenate(serial, (self.left, self.right), word, out),
        }
    }
}

impl fmt::Debug for Hyphenator<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let on = match self.table {
            Table::Compiled(_) => "compiled",
            Table::Card(..) => "card",
        };
        f.debug_struct("Hyphenator")
            .field("table", &on)
            .field("left", &self.left)
            .field("right", &self.right)
            .finish()
    }
}
