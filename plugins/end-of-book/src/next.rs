//! The next book of a series: the first one after this book, in the
//! order of the series, that is not read to the end.

use alloc::string::String;

use paperpaw_sdk::Env;
use paperpaw_sdk::library::catalog::{Query, Reading, Sort, series_order};
use paperpaw_sdk::library::service;

/// How many books of a series are looked at.
const MOST: usize = 60;

/// The next book of the series of the book at `path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Next {
    pub path: String,
    pub title: String,
    pub number: String,
    /// The books the series has.
    pub total: usize,
    pub series: String,
}

pub(crate) fn after(env: &Env<'_>, path: &str, series: &str) -> Option<Next> {
    if series.trim().is_empty() {
        return None;
    }
    let library = service::service(env);
    let books = library.query(
        env,
        &Query::new().series(series).sort(Sort::Series),
        None,
        MOST,
    );
    let here = books.iter().position(|b| b.path == path)?;
    let number = series_order(&books[here].number);
    books
        .iter()
        .enumerate()
        .find(|(i, b)| {
            *i != here
                && b.reading() != Reading::Finished
                && (series_order(&b.number) > number || (*i > here && number == u32::MAX))
        })
        .map(|(_, b)| Next {
            path: b.path.clone(),
            title: b.title.clone(),
            number: b.number.clone(),
            total: books.len(),
            series: b.series.clone(),
        })
}
