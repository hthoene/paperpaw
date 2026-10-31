//! The next book of a series: the first one after this book, in the
//! order of the series, that is not read to the end and is still on the
//! card.

use alloc::string::String;

use paperpaw_sdk::Env;
use paperpaw_sdk::library::catalog::{Query, Reading, Record, Sort};
use paperpaw_sdk::library::service;

/// Books of the series asked for at once (about 300 bytes each).
const PAGE: usize = 12;
/// Books of a series looked at, at most.
const MOST: usize = 600;

/// The next book of a series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Next {
    pub path: String,
    pub title: String,
    /// Its number in the series, as the book says (may be empty).
    pub number: String,
    /// The books the series has.
    pub total: usize,
}

/// The next book after the one at `path`, which is of `series`.
pub(crate) fn after(env: &Env<'_>, path: &str, series: &str) -> Option<Next> {
    if series.trim().is_empty() {
        return None;
    }
    let library = service::service(env);
    let query = Query::new().series(series).sort(Sort::Series);
    let mut last: Option<Record> = None;
    let (mut here, mut seen) = (false, 0);
    while seen < MOST {
        let page = library.query(env, &query, last.as_ref(), PAGE);
        for book in &page {
            if !here {
                here = book.path == path;
            } else if book.reading() != Reading::Finished && env.storage().exists(&book.path) {
                return Some(Next {
                    path: book.path.clone(),
                    title: book.title.clone(),
                    number: book.number.clone(),
                    total: library.found(env, &query),
                });
            }
        }
        seen += page.len();
        if page.len() < PAGE {
            break;
        }
        last = page.into_iter().next_back();
    }
    None
}
