//! Every allocation of the folder walk and of the page choice may be
//! refused: each ends in `NoMemory` or succeeds, never aborts.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_dictionary::native_model::{self, Found, Item, Listing, Source};
use paperpaw_sdk::native::Error;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

/// Two folders with `.ifo` files; names are `'static`, nothing is built per call.
struct Tree;
impl Listing for Tree {
    fn list(&mut self, dir: &str, each: &mut dyn FnMut(Item<'_>)) -> Result<(), Error> {
        let entries: &[(&str, bool)] = match dir {
            "d" => &[("a.ifo", false), ("sub", true), ("x.idx", false)],
            "d/sub" => &[("b.ifo", false)],
            _ => return Err(Error::NotFound),
        };
        for (name, is_dir) in entries {
            each(Item {
                name,
                is_dir: *is_dir,
            });
        }
        Ok(())
    }
}

/// Definitions made from text that exists before the refused runs.
struct Fixed(Vec<Found>);
impl Source for Fixed {
    fn ready(&mut self) -> Result<usize, Error> {
        Ok(2)
    }
    fn definitions(&mut self, which: usize, _: &str) -> Result<Vec<Found>, Error> {
        let mut out = Vec::new();
        out.try_reserve(1).map_err(|_| Error::NoMemory)?;
        if let Some(found) = self.0.get(which) {
            let copy = Found {
                title: paperpaw_ui_kit::text::copy(&found.title)?,
                text: paperpaw_ui_kit::text::copy(&found.text)?,
            };
            out.push(copy);
        }
        Ok(out)
    }
    fn definitions_within(
        &mut self,
        which: usize,
        word: &str,
        budget: &mut usize,
    ) -> Result<Vec<Found>, Error> {
        self.within(which, word, budget)
    }
}

impl Fixed {
    fn within(
        &mut self,
        which: usize,
        word: &str,
        budget: &mut usize,
    ) -> Result<Vec<Found>, Error> {
        let found = self.definitions(which, word)?;
        *budget = budget.saturating_sub(found.iter().map(|f| f.text.len()).sum());
        Ok(found)
    }
}

#[test]
fn the_walk_and_the_pages_survive_refused_allocations() {
    let found = vec![
        Found {
            title: "A".into(),
            text: "one".into(),
        },
        Found {
            title: "B".into(),
            text: "two".into(),
        },
    ];
    let mut fixed = Fixed(found);
    fail_alloc::sweep(|| {
        native_model::find_dictionaries(&mut Tree, "d")?;
        native_model::page_at(&mut fixed, "w", 1)?;
        native_model::all_within(&mut fixed, "w", &mut 1024)?;
        Ok(())
    });
}
