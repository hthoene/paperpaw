//! What the native dictionary does without host calls: finds the `.ifo`
//! files of a folder, picks the entry a lookup page asks for among the
//! definitions of all dictionaries, and cuts text to what an entry may
//! hold. Dictionaries are read through the traits here; the StarDict reading
//! on the host's files is a later slice (`PORTING-7.md`).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error,
    lookup::{MAX_ENTRY, MAX_TITLE},
};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

/// Folders below the dictionary folder searched for dictionaries.
pub const MAX_DEPTH: usize = 3;
/// Dictionaries found at most (a folder of thousands is not walked on).
pub const MAX_DICTIONARIES: usize = 64;

/// One entry of a folder.
#[derive(Debug, Clone, Copy)]
pub struct Item<'a> {
    pub name: &'a str,
    pub is_dir: bool,
}

/// A folder listing: the host's files, or a fake in tests.
pub trait Listing {
    /// Calls `each` for every entry of `dir`.
    ///
    /// # Errors
    /// The host's refusal, or `NoMemory`.
    fn list(&mut self, dir: &str, each: &mut dyn FnMut(Item<'_>)) -> Result<(), Error>;
}

/// Whether `name` ends in `.ifo`, in any case.
#[must_use]
pub fn is_ifo(name: &str) -> bool {
    name.len() > 4
        && name
            .get(name.len() - 4..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(".ifo"))
}

/// Keeps `path` among the `MAX_DICTIONARIES` smallest paths found so far,
/// sorted: the first 64 in sorted order, not the first 64 met.
fn keep_sorted(found: &mut Vec<String>, path: String) -> Result<(), Error> {
    if found.len() >= MAX_DICTIONARIES && found.last().is_some_and(|last| path >= *last) {
        return Ok(());
    }
    let at = found.partition_point(|have| *have < path);
    if found.len() >= MAX_DICTIONARIES {
        found.pop();
    }
    found.try_reserve(1).map_err(|_| Error::NoMemory)?;
    found.insert(at, path);
    Ok(())
}

/// The `.ifo` files below `folder` (at most [`MAX_DEPTH`] folders deep, hidden
/// names left out), sorted, the first [`MAX_DICTIONARIES`] of them in that
/// order. An error where `folder` itself cannot be listed; a folder below
/// that cannot be is skipped, except for `NoMemory`.
///
/// # Errors
/// The listing's refusal for `folder`, `NoMemory` anywhere.
pub fn find_dictionaries(listing: &mut impl Listing, folder: &str) -> Result<Vec<String>, Error> {
    let mut found: Vec<String> = Vec::new();
    let mut folders: Vec<(String, usize)> = Vec::new();
    push(&mut folders, (copy(folder)?, 0))?;
    while let Some((dir, depth)) = folders.pop() {
        let mut failed: Option<Error> = None;
        let listed = listing.list(&dir, &mut |item| {
            if failed.is_some() || item.name.starts_with('.') {
                return;
            }
            let path = if dir.is_empty() {
                copy(item.name)
            } else {
                concat(&[&dir, "/", item.name])
            };
            let result = path.and_then(|path| {
                if item.is_dir && depth < MAX_DEPTH {
                    push(&mut folders, (path, depth + 1))
                } else if !item.is_dir && is_ifo(item.name) {
                    keep_sorted(&mut found, path)
                } else {
                    Ok(())
                }
            });
            if let Err(error) = result {
                failed = Some(error);
            }
        });
        if let Some(error) = failed {
            return Err(error);
        }
        match listed {
            Ok(()) => {}
            Err(error) if depth == 0 || error == Error::NoMemory => return Err(error),
            Err(_) => {}
        }
    }
    Ok(found)
}

/// A definition found: the dictionary's name and the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub title: String,
    pub text: String,
}

/// The dictionaries whose indexes are ready (preparing them is the panel's:
/// a provider answers at once).
pub trait Source {
    /// How many dictionaries are ready.
    ///
    /// # Errors
    /// The host's refusal, or `NoMemory`.
    fn ready(&mut self) -> Result<usize, Error>;

    /// The definitions of `word` in ready dictionary `which`.
    ///
    /// # Errors
    /// The host's refusal (a damaged dictionary), or `NoMemory`.
    fn definitions(&mut self, which: usize, word: &str) -> Result<Vec<Found>, Error>;

    /// The definitions of `word` in ready dictionary `which`, whole (not cut
    /// to what an entry of the lookup service holds), as many as what is left
    /// of `budget` bytes allows; the budget is reduced by what they took.
    ///
    /// # Errors
    /// The host's refusal (a damaged dictionary), or `NoMemory`.
    fn definitions_within(
        &mut self,
        which: usize,
        word: &str,
        budget: &mut usize,
    ) -> Result<Vec<Found>, Error>;
}

/// Every definition of `word` in all ready dictionaries, in dictionary order,
/// whole, together within `budget` bytes (the panel's reading: the lookup
/// service's entry limit does not apply to what a dictionary shows itself). A
/// dictionary that cannot be read is skipped.
///
/// # Errors
/// `NoMemory` only.
pub fn all_within(
    source: &mut impl Source,
    word: &str,
    budget: &mut usize,
) -> Result<Vec<Found>, Error> {
    let ready = source.ready()?;
    let mut all = Vec::new();
    for which in 0..ready {
        if *budget == 0 {
            break;
        }
        let found = match source.definitions_within(which, word, budget) {
            Ok(found) => found,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => continue,
        };
        all.try_reserve(found.len()).map_err(|_| Error::NoMemory)?;
        all.extend(found);
    }
    Ok(all)
}

/// The entry a lookup page asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub found: Found,
    /// Entries follow this one.
    pub more: bool,
}

/// The `cursor`-th definition of `word` over all ready dictionaries, in
/// dictionary order, with whether another follows. A dictionary that cannot
/// be read counts as having no definition. `None` when there is no such
/// definition (no match at all, or the cursor is past the last).
///
/// # Errors
/// `NoMemory` only; a dictionary's own error skips that dictionary.
pub fn page_at(source: &mut impl Source, word: &str, cursor: usize) -> Result<Option<Page>, Error> {
    let ready = source.ready()?;
    let mut seen = 0usize;
    let mut hit: Option<Found> = None;
    for which in 0..ready {
        let definitions = match source.definitions(which, word) {
            Ok(definitions) => definitions,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => continue,
        };
        for found in definitions {
            if seen == cursor {
                hit = Some(found);
            } else if seen > cursor {
                return Ok(hit.map(|found| Page { found, more: true }));
            }
            seen += 1;
        }
    }
    Ok(hit.map(|found| Page { found, more: false }))
}

/// Whether a lookup page the broker gave is no entry: a provider with nothing
/// to say is turned into an empty `lookup_entry` (empty title and text). The
/// panel and the reader's selection menu skip such pages.
#[must_use]
pub fn is_no_entry(title: &str, text: &str) -> bool {
    title.is_empty() && text.is_empty()
}

/// `text` cut to at most `max` bytes at a character boundary; whether it
/// was cut says the entry is shortened (`…` is not added: the bytes are the
/// contract's).
#[must_use]
pub fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

/// The title and text of an entry, cut to the contract's limits.
#[must_use]
pub fn fit(found: &Found) -> (&str, &str) {
    (cut(&found.title, MAX_TITLE), cut(&found.text, MAX_ENTRY))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;
    use alloc::{format, string::ToString, vec};

    /// A tree: folder → entries.
    struct Tree(Vec<(&'static str, Vec<(&'static str, bool)>)>);
    impl Listing for Tree {
        fn list(&mut self, dir: &str, each: &mut dyn FnMut(Item<'_>)) -> Result<(), Error> {
            let (_, entries) = self
                .0
                .iter()
                .find(|(d, _)| *d == dir)
                .ok_or(Error::NotFound)?;
            for (name, is_dir) in entries {
                each(Item {
                    name,
                    is_dir: *is_dir,
                });
            }
            Ok(())
        }
    }

    #[test]
    fn ifo_files_are_found_in_any_case_and_below_folders_but_not_hidden() {
        let mut tree = Tree(vec![
            (
                "dict",
                vec![
                    ("a.ifo", false),
                    ("B.IFO", false),
                    ("x.idx", false),
                    (".c.ifo", false),
                    ("sub", true),
                    (".hid", true),
                ],
            ),
            ("dict/sub", vec![("d.Ifo", false), ("deep", true)]),
            ("dict/sub/deep", vec![("e.ifo", false), ("deeper", true)]),
            (
                "dict/sub/deep/deeper",
                vec![("f.ifo", false), ("deepest", true)],
            ),
            ("dict/sub/deep/deeper/deepest", vec![("g.ifo", false)]),
        ]);
        let found = find_dictionaries(&mut tree, "dict").unwrap();
        assert_eq!(
            found,
            [
                "dict/B.IFO",
                "dict/a.ifo",
                "dict/sub/d.Ifo",
                "dict/sub/deep/deeper/f.ifo",
                "dict/sub/deep/e.ifo"
            ]
        );
        assert!(!is_ifo(".ifo") && is_ifo("a.ifo") && !is_ifo("ifo"));
    }

    #[test]
    fn a_folder_that_cannot_be_listed_is_an_error_at_the_top_and_skipped_below() {
        let mut tree = Tree(vec![("dict", vec![("gone", true), ("a.ifo", false)])]);
        assert_eq!(
            find_dictionaries(&mut tree, "dict").unwrap(),
            ["dict/a.ifo"]
        );
        assert_eq!(
            find_dictionaries(&mut tree, "nowhere").err(),
            Some(Error::NotFound)
        );
    }

    #[test]
    fn the_top_of_the_card_has_no_leading_slash() {
        let mut tree = Tree(vec![
            ("", vec![("d.ifo", false), ("x", true)]),
            ("x", vec![("e.ifo", false)]),
        ]);
        assert_eq!(
            find_dictionaries(&mut tree, "").unwrap(),
            ["d.ifo", "x/e.ifo"]
        );
    }

    #[test]
    fn dictionaries_found_are_bounded() {
        let names: Vec<(String, bool)> = (0..80)
            .rev()
            .map(|n| (format!("{n:03}.ifo"), false))
            .collect();
        struct Many(Vec<(String, bool)>);
        impl Listing for Many {
            fn list(&mut self, _: &str, each: &mut dyn FnMut(Item<'_>)) -> Result<(), Error> {
                for (name, is_dir) in &self.0 {
                    each(Item {
                        name,
                        is_dir: *is_dir,
                    });
                }
                Ok(())
            }
        }
        let found = find_dictionaries(&mut Many(names), "d").unwrap();
        assert_eq!(found.len(), MAX_DICTIONARIES);
        // The first 64 in sorted order, however the folder lists them.
        let expected: Vec<String> = (0..64).map(|n| format!("d/{n:03}.ifo")).collect();
        assert_eq!(found, expected);
    }

    #[test]
    fn a_refused_allocation_below_the_top_is_not_skipped() {
        struct Refusing;
        impl Listing for Refusing {
            fn list(&mut self, dir: &str, each: &mut dyn FnMut(Item<'_>)) -> Result<(), Error> {
                if dir == "d" {
                    each(Item {
                        name: "sub",
                        is_dir: true,
                    });
                    Ok(())
                } else {
                    Err(Error::NoMemory)
                }
            }
        }
        assert_eq!(
            find_dictionaries(&mut Refusing, "d").err(),
            Some(Error::NoMemory)
        );
    }

    /// Dictionaries with their definitions of any word.
    struct Fake(Vec<Result<Vec<&'static str>, Error>>);
    impl Source for Fake {
        fn ready(&mut self) -> Result<usize, Error> {
            Ok(self.0.len())
        }
        fn definitions(&mut self, which: usize, _: &str) -> Result<Vec<Found>, Error> {
            match self.0.get(which).unwrap() {
                Ok(texts) => Ok(texts
                    .iter()
                    .map(|t| Found {
                        title: format!("D{which}"),
                        text: (*t).to_string(),
                    })
                    .collect()),
                Err(error) => Err(*error),
            }
        }
        fn definitions_within(
            &mut self,
            which: usize,
            word: &str,
            budget: &mut usize,
        ) -> Result<Vec<Found>, Error> {
            let mut found = self.definitions(which, word)?;
            let mut kept = 0;
            found.retain(|f| {
                kept += f.text.len();
                kept <= *budget
            });
            *budget -= found.iter().map(|f| f.text.len()).sum::<usize>();
            Ok(found)
        }
    }

    #[test]
    fn pages_walk_the_definitions_of_all_dictionaries_in_order() {
        let mut source = Fake(vec![Ok(vec!["a1", "a2"]), Ok(vec![]), Ok(vec!["c1"])]);
        let texts: Vec<_> = (0..4)
            .map(|c| {
                page_at(&mut source, "w", c)
                    .unwrap()
                    .map(|p| (p.found.title, p.found.text, p.more))
            })
            .collect();
        assert_eq!(texts[0], Some(("D0".into(), "a1".into(), true)));
        assert_eq!(texts[1], Some(("D0".into(), "a2".into(), true)));
        assert_eq!(texts[2], Some(("D2".into(), "c1".into(), false)));
        assert_eq!(texts[3], None);
    }

    #[test]
    fn an_unreadable_dictionary_is_skipped_and_nomemory_is_not() {
        let mut source = Fake(vec![Err(Error::Invalid), Ok(vec!["b1"])]);
        let page = page_at(&mut source, "w", 0).unwrap().unwrap();
        assert_eq!((page.found.title.as_str(), page.more), ("D1", false));
        let mut source = Fake(vec![Err(Error::NoMemory), Ok(vec!["b1"])]);
        assert_eq!(page_at(&mut source, "w", 0).err(), Some(Error::NoMemory));
        let mut none = Fake(vec![]);
        assert_eq!(page_at(&mut none, "w", 0).unwrap(), None);
    }

    #[test]
    fn the_panel_reads_every_definition_whole_within_one_shared_budget() {
        let mut source = Fake(vec![Ok(vec!["a1", "a2"]), Ok(vec!["b1"])]);
        let mut budget = 100;
        let all = all_within(&mut source, "w", &mut budget).unwrap();
        let texts: Vec<_> = all
            .iter()
            .map(|f| (f.title.as_str(), f.text.as_str()))
            .collect();
        assert_eq!(texts, [("D0", "a1"), ("D0", "a2"), ("D1", "b1")]);
        assert_eq!(budget, 94);
        // What does not fit the budget is left out, and nothing is read once it is used.
        let mut source = Fake(vec![Ok(vec!["a1", "a2"]), Ok(vec!["b1"])]);
        let mut budget = 4;
        assert_eq!(all_within(&mut source, "w", &mut budget).unwrap().len(), 2);
        assert_eq!(budget, 0);
        // A text over the entry limit of the service stays whole.
        let big = "y".repeat(MAX_ENTRY + 500);
        struct One(String);
        impl Source for One {
            fn ready(&mut self) -> Result<usize, Error> {
                Ok(1)
            }
            fn definitions(&mut self, _: usize, _: &str) -> Result<Vec<Found>, Error> {
                Ok(vec![Found {
                    title: "T".into(),
                    text: self.0.clone(),
                }])
            }
            fn definitions_within(
                &mut self,
                which: usize,
                word: &str,
                budget: &mut usize,
            ) -> Result<Vec<Found>, Error> {
                let found = self.definitions(which, word)?;
                *budget -= found[0].text.len();
                Ok(found)
            }
        }
        let mut budget = 32 * 1024;
        let all = all_within(&mut One(big.clone()), "w", &mut budget).unwrap();
        assert_eq!(all[0].text, big);
        // An unreadable dictionary is skipped, `NoMemory` is not.
        let mut source = Fake(vec![Err(Error::Invalid), Ok(vec!["b1"])]);
        let mut budget = 10;
        assert_eq!(all_within(&mut source, "w", &mut budget).unwrap().len(), 1);
        let mut source = Fake(vec![Err(Error::NoMemory)]);
        assert_eq!(
            all_within(&mut source, "w", &mut budget).err(),
            Some(Error::NoMemory)
        );
    }

    #[test]
    fn an_empty_title_and_text_is_no_entry_and_anything_else_is_shown() {
        assert!(is_no_entry("", ""));
        assert!(!is_no_entry("Dict", ""));
        assert!(!is_no_entry("", "text"));
        assert!(!is_no_entry("Dict", "text"));
    }

    #[test]
    fn text_is_cut_at_a_character_boundary_never_inside_one() {
        assert_eq!(cut("abc", 5), "abc");
        assert_eq!(cut("abcdef", 3), "abc");
        assert_eq!(cut("äöü", 3), "ä");
        assert_eq!(cut("äöü", 1), "");
        let long = Found {
            title: "t".repeat(300),
            text: "ü".repeat(2000),
        };
        let (title, text) = fit(&long);
        assert_eq!((title.len(), text.len()), (MAX_TITLE, 2048));
    }
}
