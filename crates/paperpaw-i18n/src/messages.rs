//! The catalogs of the plugins a program links, built into it without
//! their keys ([`BuiltMessages`]): a build script puts the messages of a
//! language one after another and keeps a hash of each key in place of
//! its text, so the program carries neither the keys (a third of a
//! catalog's bytes, once for each language) nor the sources' comments.
//! Their sources are not added at run time: the table answers for all of
//! them from the start.

use alloc::string::String;
use alloc::vec::Vec;

use crate::catalog::{Catalog, CatalogError, fingerprint};
use crate::plural::PluralRule;

/// Bits of an entry that say where its message starts in the values; the
/// bits above are more of the key's hash, checked when a key is found.
const OFFSET_BITS: u32 = 20;
const OFFSET_MASK: u32 = (1 << OFFSET_BITS) - 1;
/// The offset of a key this language has no message for.
const ABSENT: u32 = OFFSET_MASK;
/// Ends each message in the values.
const END: char = '\0';

/// What `keys` holds of a key: the low half of its hash.
const fn low_of(hash: u64) -> u32 {
    (hash & 0xffff_ffff) as u32
}

/// What an entry holds of a key beside its offset: the next bits of its
/// hash, as many as fit above the offset.
const fn check_of(hash: u64) -> u32 {
    ((hash >> 32) as u32) & (u32::MAX >> OFFSET_BITS)
}

/// The messages of one language of the catalogs built in, without their
/// keys: written by a build script ([`BuiltMessagesTable`]), handed to
/// the translator
/// ([`Translator::built_messages`](crate::Translator::built_messages)).
///
/// A key is known by 44 bits of its hash: one the table does not hold is
/// taken for one it holds once in some 10^10 keys asked for.
#[derive(Debug)]
#[non_exhaustive]
pub struct BuiltMessages {
    /// As a catalog's `[meta] language` (`en`, `de`).
    pub language: &'static str,
    /// As a catalog's `[meta] name` (`English`, `Deutsch`).
    pub name: &'static str,
    /// The low half of each key's hash, sorted, no two the same; the
    /// languages of one table share them.
    pub keys: &'static [u32],
    /// For each of `keys`: more of the hash above, below it where the
    /// message starts in `values` (all bits set: none in this language).
    pub entries: &'static [u32],
    /// The messages, each ended by a NUL.
    pub values: &'static str,
}

impl BuiltMessages {
    /// Made of what a build script wrote.
    pub const fn new(
        language: &'static str,
        name: &'static str,
        keys: &'static [u32],
        entries: &'static [u32],
        values: &'static str,
    ) -> Self {
        Self {
            language,
            name,
            keys,
            entries,
            values,
        }
    }

    /// The message for `key`.
    pub(crate) fn get(&self, key: &str) -> Option<&'static str> {
        let hash = fingerprint(key);
        let at = self.keys.binary_search(&low_of(hash)).ok()?;
        let entry = *self.entries.get(at)?;
        let offset = entry & OFFSET_MASK;
        if entry >> OFFSET_BITS != check_of(hash) || offset == ABSENT {
            return None;
        }
        let rest = self.values.get(offset as usize..)?;
        rest.get(..rest.find(END)?)
    }

    /// Messages it holds.
    pub(crate) fn len(&self) -> usize {
        self.entries
            .iter()
            .filter(|&&entry| entry & OFFSET_MASK != ABSENT)
            .count()
    }
}

/// One language of a [`BuiltMessagesTable`].
#[derive(Debug)]
struct Language {
    language: String,
    /// The name the first source added gives it; the others must say it.
    name: Option<String>,
    /// Sources added.
    sources: usize,
}

/// [`BuiltMessages`] being made by a build script, for the languages a
/// program carries: catalogs added ([`BuiltMessagesTable::add`]), written
/// as Rust ([`BuiltMessagesTable::to_rust`]).
#[derive(Debug)]
pub struct BuiltMessagesTable {
    languages: Vec<Language>,
    /// Each key with its hash, sorted by the low half of the hash, and
    /// its message in each language.
    keys: Vec<(u64, String, Vec<Option<String>>)>,
    /// The bytes of each language's values so far.
    sizes: Vec<usize>,
}

impl BuiltMessagesTable {
    /// For the catalogs of `languages` (as a catalog's `[meta]
    /// language`).
    pub fn new(languages: &[&str]) -> Self {
        Self {
            languages: languages
                .iter()
                .map(|language| Language {
                    language: String::from(*language),
                    name: None,
                    sources: 0,
                })
                .collect(),
            keys: Vec::new(),
            sizes: alloc::vec![0; languages.len()],
        }
    }

    fn place(&self, hash: u64) -> Result<usize, usize> {
        self.keys
            .binary_search_by(|(other, ..)| low_of(*other).cmp(&low_of(hash)))
    }

    /// Adds the catalog `source`; `false`, leaving it out (the program
    /// then carries it as a catalog of its own), where its language is
    /// none of the table's or it calls it by another name than the first
    /// source did, it has another plural rule than one and other or a
    /// direction, script, digits or calendar of its own (the table says
    /// none), it holds a key another source of its language holds, a key
    /// whose hash is taken for another's, a message with a NUL, or its
    /// messages would not fit.
    pub fn add(&mut self, source: &str) -> Result<bool, CatalogError> {
        let catalog = Catalog::parse(String::from(source))?;
        let Some(at) = self
            .languages
            .iter()
            .position(|l| l.language == catalog.language())
        else {
            return Ok(false);
        };
        let named = self.languages[at].name.as_deref();
        if catalog.plural_rule() != PluralRule::OneOther
            || catalog.says_more_than_language()
            || named.is_some_and(|name| name != catalog.name())
        {
            return Ok(false);
        }
        let mut size = self.sizes[at];
        let mut added = Vec::new();
        for key in catalog.keys() {
            let message = catalog.get(key).unwrap_or_default();
            let hash = fingerprint(key);
            let taken = match self.place(hash) {
                Ok(place) => {
                    let (_, theirs, messages) = &self.keys[place];
                    theirs != key || messages[at].is_some()
                }
                Err(_) => false,
            };
            let twice = added
                .iter()
                .any(|(other, ..): &(u64, &str, &str)| low_of(*other) == low_of(hash));
            if taken || twice || message.contains(END) {
                return Ok(false);
            }
            size += message.len() + END.len_utf8();
            added.push((hash, key, message));
        }
        if size >= ABSENT as usize {
            return Ok(false);
        }
        for (hash, key, message) in added {
            let place = match self.place(hash) {
                Ok(place) => place,
                Err(place) => {
                    let none = alloc::vec![None; self.languages.len()];
                    self.keys.insert(place, (hash, String::from(key), none));
                    place
                }
            };
            self.keys[place].2[at] = Some(String::from(message));
        }
        self.sizes[at] = size;
        let language = &mut self.languages[at];
        language.sources += 1;
        if language.name.is_none() {
            language.name = Some(String::from(catalog.name()));
        }
        Ok(true)
    }

    /// The languages a source was added for, each with its entries and
    /// values.
    fn made(&self) -> Vec<(&Language, Vec<u32>, String)> {
        self.languages
            .iter()
            .enumerate()
            .filter(|(_, language)| language.sources > 0)
            .map(|(at, language)| {
                let mut values = String::new();
                let entries = self
                    .keys
                    .iter()
                    .map(|(hash, _, messages)| {
                        let offset = match &messages[at] {
                            Some(message) => {
                                let offset = values.len() as u32;
                                values.push_str(message);
                                values.push(END);
                                offset
                            }
                            None => ABSENT,
                        };
                        check_of(*hash) << OFFSET_BITS | offset
                    })
                    .collect();
                (language, entries, values)
            })
            .collect()
    }

    fn keys(&self) -> Vec<u32> {
        self.keys.iter().map(|(hash, ..)| low_of(*hash)).collect()
    }

    /// The table as a Rust expression of `&'static [BuiltMessages]`, for
    /// `include!`: the keys once, shared by its languages.
    pub fn to_rust(&self) -> String {
        use core::fmt::Write;
        let keys = self.keys();
        let mut out = alloc::format!("{{ static KEYS: [u32; {}] = [", keys.len());
        for key in keys {
            let _ = write!(out, "{key:#x},");
        }
        out.push_str("]; &[");
        for (language, entries, values) in self.made() {
            let _ = write!(
                out,
                "paperpaw_i18n::BuiltMessages::new({:?}, {:?}, &KEYS, &[",
                language.language,
                language.name.as_deref().unwrap_or_default()
            );
            for entry in entries {
                let _ = write!(out, "{entry:#x},");
            }
            out.push_str("], \"");
            for c in values.chars() {
                // Not `\0`: before a digit it reads as an octal escape.
                match c {
                    END => out.push_str("\\u{0}"),
                    c => out.extend(c.escape_debug()),
                }
            }
            out.push_str("\"),");
        }
        out.push_str("] }");
        out
    }

    /// The table at run time, its memory never given back: for tests and
    /// tools (a program writes it with [`to_rust`](Self::to_rust)).
    pub fn leak(self) -> &'static [BuiltMessages] {
        use alloc::boxed::Box;
        let keys: &'static [u32] = Box::leak(self.keys().into_boxed_slice());
        let made: Vec<BuiltMessages> = self
            .made()
            .into_iter()
            .map(|(language, entries, values)| {
                let name = language.name.clone().unwrap_or_default();
                BuiltMessages::new(
                    Box::leak(language.language.clone().into_boxed_str()),
                    Box::leak(name.into_boxed_str()),
                    keys,
                    Box::leak(entries.into_boxed_slice()),
                    Box::leak(values.into_boxed_str()),
                )
            })
            .collect();
        Box::leak(made.into_boxed_slice())
    }

    /// Sources added.
    pub fn len(&self) -> usize {
        self.languages.iter().map(|l| l.sources).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The bytes the tables take in the program: keys, entries and
    /// values.
    pub fn size(&self) -> usize {
        let made = self.made();
        4 * self.keys.len()
            + made
                .iter()
                .map(|(_, entries, values)| 4 * entries.len() + values.len())
                .sum::<usize>()
    }
}
