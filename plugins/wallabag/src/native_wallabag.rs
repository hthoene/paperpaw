//! What the wallabag plugin decides without the host: which texts are
//! article ids, what the set-up's rows are called, the lists it keeps of the
//! articles fetched and read, and when an article counts as read. The same
//! rules as the interface-6 plugin (`lib.rs`), with fallible allocation and
//! the text given by the caller instead of read from a storage. Fetching,
//! the files and the set-up screen are the native plugin's (see
//! `PORTING-7.md`).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;

/// Unread articles fetched at most.
pub const MAX_ARTICLES: usize = 30;
/// The most `articles.txt` is read to change, as in interface 6 (64 KiB).
pub const ARTICLES_CAP: usize = 64 * 1024;
/// The most `finished.txt` is read to change, as in interface 6 (16 KiB).
pub const FINISHED_CAP: usize = 16 * 1024;
/// Reading progress, in millionths (the native event's), from which an
/// article counts as read where its pages are not counted yet: 95 %.
pub const READ_PPM: u32 = 950_000;

/// Whether `id` is an id as Wallabag gives them: a number.
pub fn is_id(id: &str) -> bool {
    (1..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

/// Whether a page turn is the article read to the end: on the last page, or
/// near the end where pages are not counted yet (`progress_ppm` as the
/// reading event gives it, from 0 to 1,000,000).
pub fn is_read(page: Option<u32>, pages: Option<u32>, progress_ppm: u32) -> bool {
    match (page, pages) {
        (Some(page), Some(pages)) => page >= pages,
        _ => progress_ppm >= READ_PPM,
    }
}

/// A row of the set-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Server,
    User,
    Password,
    Client,
    Secret,
    Fetch,
}

pub const ROWS: [Row; 6] = [
    Row::Server,
    Row::User,
    Row::Password,
    Row::Client,
    Row::Secret,
    Row::Fetch,
];

impl Row {
    /// The translation key of the row's name.
    pub fn title(self) -> &'static str {
        match self {
            Row::Server => "wallabag.server",
            Row::User => "wallabag.user",
            Row::Password => "wallabag.password",
            Row::Client => "wallabag.client-id",
            Row::Secret => "wallabag.client-secret",
            Row::Fetch => "wallabag.fetch",
        }
    }

    /// What the keyboard is asked for it with; none for *Fetch*.
    pub fn field(self) -> &'static str {
        match self {
            Row::Server => "server",
            Row::User => "user",
            Row::Password => "password",
            Row::Client => "client",
            Row::Secret => "secret",
            Row::Fetch => "",
        }
    }

    /// The row asked for with `field`.
    pub fn of(field: &str) -> Option<Self> {
        ROWS.into_iter()
            .find(|r| r.field() == field && !field.is_empty())
    }
}

/// An article fetched: its id in Wallabag and its book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Article {
    pub id: String,
    pub path: String,
}

fn copy(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(out)
}

fn push<T>(list: &mut Vec<T>, item: T) -> Result<(), Error> {
    list.try_reserve(1).map_err(|_| Error::NoMemory)?;
    list.push(item);
    Ok(())
}

/// `text`, the file as read, without a UTF-8 byte order mark; `NoSpace` over
/// `cap` bytes. A list too large is not an empty one: the caller must not
/// write the file then (what it holds would be lost), as in interface 6.
fn within(text: &str, cap: usize) -> Result<&str, Error> {
    if text.len() > cap {
        return Err(Error::NoSpace);
    }
    Ok(text.strip_prefix('\u{feff}').unwrap_or(text))
}

/// The articles fetched, kept as `id<TAB>path` lines (lines of another shape
/// are left out). `text` is the file as read, at most [`ARTICLES_CAP`] bytes.
///
/// # Errors
/// `NoSpace` over [`ARTICLES_CAP`]; `NoMemory` when the list cannot be kept.
pub fn articles(text: &str) -> Result<Vec<Article>, Error> {
    let mut list = Vec::new();
    for line in within(text, ARTICLES_CAP)?.lines() {
        if let Some((id, path)) = line.split_once('\t') {
            push(
                &mut list,
                Article {
                    id: copy(id)?,
                    path: copy(path)?,
                },
            )?;
        }
    }
    Ok(list)
}

/// The list of articles as it is kept (`id<TAB>path` lines).
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn articles_text(list: &[Article]) -> Result<String, Error> {
    let size = list.iter().map(|a| a.id.len() + a.path.len() + 2).sum();
    let mut out = String::new();
    out.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    for a in list {
        out.push_str(&a.id);
        out.push('\t');
        out.push_str(&a.path);
        out.push('\n');
    }
    Ok(out)
}

/// The ids of articles read to the end, to archive: one a line. `text` is
/// the file as read, at most [`FINISHED_CAP`] bytes.
///
/// # Errors
/// `NoSpace` over [`FINISHED_CAP`]; `NoMemory` when the list cannot be kept.
pub fn finished(text: &str) -> Result<Vec<String>, Error> {
    let mut ids = Vec::new();
    for line in within(text, FINISHED_CAP)?
        .lines()
        .filter(|l| !l.is_empty())
    {
        push(&mut ids, copy(line)?)?;
    }
    Ok(ids)
}
