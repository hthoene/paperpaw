//! The catalogs of one language built into the program as one table
//! ([`BuiltCatalogs`]; English, and German where a program has it too): a
//! build script indexes them together, so a translator keeps one catalog for
//! all of a language's, each plugin's source a word or two, rather than a
//! catalog for each (a hundred bytes and more each, for some eighty plugins
//! on the X4), and the values a source does not give plainly (escape
//! sequences) are kept in the program, not made on the heap.

use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::catalog::{Catalog, CatalogError, cmp_plain_key, fingerprint, plain_key, plain_value};
use crate::plural::PluralRule;

/// In an entry of [`BuiltCatalogs::entries`]: its message is
/// `decoded[low bits]`, not a plain line of a source.
const DECODED: u32 = 1 << 31;
/// Bits of an entry's offset in its source (or its place in `decoded`);
/// the bits above, below [`DECODED`], are its source's slot.
const OFFSET_BITS: u32 = 24;

/// The slot of an entry's source.
const fn slot_of(entry: u32) -> usize {
    ((entry & !DECODED) >> OFFSET_BITS) as usize
}

/// An entry's offset in its source, or its place in `decoded`.
const fn low_of(entry: u32) -> u32 {
    entry & ((1 << OFFSET_BITS) - 1)
}

/// The catalogs of one language of the plugins built in (English unless
/// [`in_language`](Self::in_language) says another), indexed together by a
/// build script (`paperpaw-app`'s, with [`BuiltTable`]): kept in the
/// program, handed to the translator
/// ([`Translator::built_catalogs`](crate::Translator::built_catalogs)).
#[derive(Debug)]
#[non_exhaustive]
pub struct BuiltCatalogs {
    /// Each source's [`fingerprint`], by slot: a catalog added with this
    /// source fills its slot rather than being kept as a catalog.
    pub prints: &'static [u64],
    /// The messages of all, sorted by key: a slot and the offset of the
    /// key's line in that source, or its top bit set and a place in
    /// `decoded`.
    pub entries: &'static [u32],
    /// Keys and messages a source does not give plainly (quoted values,
    /// escape sequences), decoded.
    pub decoded: &'static [(&'static str, &'static str)],
    /// Their language (`en`) and the name it calls itself by.
    pub language: &'static str,
    pub name: &'static str,
}

impl BuiltCatalogs {
    /// None (a program that indexes no catalogs together).
    pub const NONE: BuiltCatalogs = BuiltCatalogs {
        prints: &[],
        entries: &[],
        decoded: &[],
        language: "en",
        name: "English",
    };

    /// Made of what a build script wrote.
    pub const fn new(
        prints: &'static [u64],
        entries: &'static [u32],
        decoded: &'static [(&'static str, &'static str)],
    ) -> Self {
        Self {
            prints,
            entries,
            decoded,
            language: "en",
            name: "English",
        }
    }

    /// Of the catalogs of `language`, which calls itself `name` (all with
    /// the plural rule of English, one and other).
    pub const fn in_language(mut self, language: &'static str, name: &'static str) -> Self {
        self.language = language;
        self.name = name;
        self
    }
}

/// [`BuiltCatalogs`] in a translator: the sources added so far by slot.
#[derive(Debug, Clone)]
pub(crate) struct Built {
    table: &'static BuiltCatalogs,
    sources: Vec<Option<&'static str>>,
}

impl Built {
    pub(crate) fn new(table: &'static BuiltCatalogs) -> Self {
        Self {
            table,
            sources: alloc::vec![None; table.prints.len()],
        }
    }

    /// Takes `source` (of this [`fingerprint`]) into its slot, where the
    /// table holds it.
    pub(crate) fn take(&mut self, print: u64, source: &'static str) -> bool {
        let Some(slot) = self.table.prints.iter().position(|p| *p == print) else {
            return false;
        };
        self.sources[slot] = Some(source);
        true
    }

    /// The source of an entry, where its plugin's source was added.
    fn source(&self, entry: u32) -> Option<&'static str> {
        self.sources.get(slot_of(entry)).copied().flatten()
    }

    /// An entry's key and message where it is decoded (`None` for a plain
    /// one or a place the table does not have).
    fn decoded(&self, entry: u32) -> Option<&(&'static str, &'static str)> {
        (entry & DECODED != 0)
            .then(|| self.table.decoded.get(low_of(entry) as usize))
            .flatten()
    }

    /// How an entry's key compares with `key`; an entry of a source not
    /// added compares by nothing (the build script checks that keys never
    /// collide, so the search below skips such an entry).
    fn key_cmp(&self, entry: u32, key: &str) -> Ordering {
        let Some(source) = self.source(entry) else {
            return Ordering::Less;
        };
        match self.decoded(entry) {
            Some((theirs, _)) => (*theirs).cmp(key),
            None if entry & DECODED != 0 => Ordering::Less,
            None => cmp_plain_key(source, low_of(entry), key),
        }
    }

    /// The message for `key`, where its plugin's source was added.
    pub(crate) fn get(&self, key: &str) -> Option<&'static str> {
        let entries = self.table.entries;
        let (mut low, mut high) = (0, entries.len());
        while low < high {
            let middle = low + (high - low) / 2;
            let entry = entries[middle];
            // An entry of a source not added: look at its neighbours'
            // keys instead (entries are sorted across all sources).
            if self.source(entry).is_none() {
                return self.get_scanning(key, low, high);
            }
            match self.key_cmp(entry, key) {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => return self.message(entry),
            }
        }
        None
    }

    /// The same, through `entries[low..high]` one by one: only where the
    /// search met a source not added (a plugin built in but left out).
    fn get_scanning(&self, key: &str, low: usize, high: usize) -> Option<&'static str> {
        self.table
            .entries
            .get(low..high)?
            .iter()
            .find(|&&entry| self.source(entry).is_some() && self.key_cmp(entry, key).is_eq())
            .and_then(|&entry| self.message(entry))
    }

    fn message(&self, entry: u32) -> Option<&'static str> {
        let source = self.source(entry)?;
        if entry & DECODED != 0 {
            return self.decoded(entry).map(|(_, message)| *message);
        }
        plain_value(source, low_of(entry))
    }

    pub(crate) fn len(&self) -> usize {
        self.table.entries.len()
    }

    /// The keys it answers ([`Built::get`]), sorted.
    pub(crate) fn keys(&self) -> impl Iterator<Item = &str> {
        self.table.entries.iter().filter_map(|&entry| {
            let source = self.source(entry)?;
            if entry & DECODED != 0 {
                return self.decoded(entry).map(|(key, _)| *key);
            }
            Some(plain_key(source, low_of(entry)))
        })
    }

    pub(crate) fn index_on_heap(&self) -> usize {
        self.sources.len() * core::mem::size_of::<Option<&str>>()
    }
}

/// [`BuiltCatalogs`] being made by a build script: the catalogs of one
/// language added together ([`BuiltTable::add`]), written as Rust
/// ([`BuiltTable::to_rust`]).
#[derive(Debug)]
pub struct BuiltTable {
    language: String,
    /// The name of the first source's language; the others must say it.
    name: Option<String>,
    prints: Vec<u64>,
    /// Key, then the entry.
    entries: Vec<(String, u32)>,
    decoded: Vec<(String, String)>,
}

impl Default for BuiltTable {
    fn default() -> Self {
        Self::new()
    }
}

impl BuiltTable {
    /// For English.
    pub fn new() -> Self {
        Self::for_language("en")
    }

    /// For the catalogs of `language` (as a catalog's `[meta] language`).
    pub fn for_language(language: &str) -> Self {
        Self {
            language: String::from(language),
            name: None,
            prints: Vec::new(),
            entries: Vec::new(),
            decoded: Vec::new(),
        }
    }

    /// Adds the catalog `source` of the table's language; `false`, leaving
    /// it out (it then stays a catalog of its own), where it is of another
    /// language or name, has another plural rule than English's or a
    /// direction, script, digits or calendar of its own (the table says
    /// none), holds a key another source added holds, or its slot or an
    /// offset would not fit.
    pub fn add(&mut self, source: &str) -> Result<bool, CatalogError> {
        let catalog = Catalog::parse(String::from(source))?;
        let print = fingerprint(source);
        if catalog.language() != self.language
            || self.prints.contains(&print)
            || catalog.plural_rule() != PluralRule::OneOther
            || catalog.says_more_than_language()
            || (self.language != "en"
                && self
                    .name
                    .as_deref()
                    .is_some_and(|name| name != catalog.name()))
        {
            return Ok(false);
        }
        let slot = self.prints.len() as u32;
        if slot >= (DECODED >> OFFSET_BITS) || source.len() >= 1 << OFFSET_BITS {
            return Ok(false);
        }
        let mut added = Vec::new();
        let mut decoded = Vec::new();
        for key in catalog.keys() {
            if self
                .entries
                .binary_search_by(|(k, _)| k.as_str().cmp(key))
                .is_ok()
            {
                return Ok(false);
            }
            match catalog.plain_entry(key) {
                Some(offset) => added.push((String::from(key), slot << OFFSET_BITS | offset)),
                None => {
                    let message = catalog.get(key).unwrap_or_default();
                    decoded.push((String::from(key), String::from(message)));
                }
            }
        }
        // Places in `decoded` fit below the slot, as offsets do.
        if self.decoded.len() + decoded.len() > 1 << OFFSET_BITS {
            return Ok(false);
        }
        self.prints.push(print);
        // English is always called English (a source may not say its name).
        if self.language != "en" {
            self.name
                .get_or_insert_with(|| String::from(catalog.name()));
        }
        for (key, message) in decoded {
            let entry = DECODED | slot << OFFSET_BITS | self.decoded.len() as u32;
            self.decoded.push((key.clone(), message));
            added.push((key, entry));
        }
        self.entries.extend(added);
        paperpaw_sort::sort_by(&mut self.entries, |(a, _), (b, _)| a.cmp(b));
        Ok(true)
    }

    /// The table as a Rust expression of [`BuiltCatalogs`], for
    /// `include!`.
    pub fn to_rust(&self) -> String {
        use core::fmt::Write;
        let mut out = String::from("paperpaw_i18n::BuiltCatalogs::new(&[");
        for print in &self.prints {
            let _ = write!(out, "{print:#x},");
        }
        out.push_str("], &[");
        for (_, entry) in &self.entries {
            let _ = write!(out, "{entry:#x},");
        }
        out.push_str("], &[");
        for (key, message) in &self.decoded {
            let _ = write!(out, "({key:?}, {message:?}),");
        }
        out.push_str("])");
        if self.language != "en" {
            let name = self.name.as_deref().unwrap_or_default();
            let _ = write!(out, ".in_language({:?}, {name:?})", self.language);
        }
        out
    }

    /// The table as [`BuiltCatalogs`] at run time, its memory never given
    /// back: for tests and tools (a program writes it with
    /// [`to_rust`](Self::to_rust)).
    pub fn leak(self) -> &'static BuiltCatalogs {
        let prints = alloc::boxed::Box::leak(self.prints.into_boxed_slice());
        let entries: Vec<u32> = self.entries.into_iter().map(|(_, entry)| entry).collect();
        let entries = alloc::boxed::Box::leak(entries.into_boxed_slice());
        let decoded: Vec<(&'static str, &'static str)> = self
            .decoded
            .into_iter()
            .map(|(key, message)| {
                (
                    &*alloc::boxed::Box::leak(key.into_boxed_str()),
                    &*alloc::boxed::Box::leak(message.into_boxed_str()),
                )
            })
            .collect();
        let decoded = alloc::boxed::Box::leak(decoded.into_boxed_slice());
        let language = &*alloc::boxed::Box::leak(self.language.into_boxed_str());
        let name = self
            .name
            .unwrap_or_else(|| String::from(if language == "en" { "English" } else { "" }));
        let name = &*alloc::boxed::Box::leak(name.into_boxed_str());
        alloc::boxed::Box::leak(alloc::boxed::Box::new(
            BuiltCatalogs::new(prints, entries, decoded).in_language(language, name),
        ))
    }

    /// Sources added.
    pub fn len(&self) -> usize {
        self.prints.len()
    }

    pub fn is_empty(&self) -> bool {
        self.prints.is_empty()
    }
}
