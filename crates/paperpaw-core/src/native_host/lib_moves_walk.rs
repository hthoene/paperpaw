//! The walk over the books of a moved folder, resumable: a step of the
//! library's move job costs `O(books it takes + a bounded skip)` directory
//! reads, not `O(folder)` (`files.md` 2.6: the main loop is never stalled by a
//! big folder, and the device is not kept awake by it).
//!
//! The walk is the catalog's (every file a format opens, by extension or by
//! its first bytes, not in hidden entries; subfolders after the folder that
//! holds them, the last found first) but it is kept between steps: the folders
//! still to read and the place in the one being read (`Storage::list_from`,
//! which stops at the entry `visit` refuses and returns its place). The next
//! step goes on from there; nothing is read twice.
//!
//! The saved walk is in memory only, for a job the kernel runs from its main
//! loop. It is used when the step asks for the book the saved walk has
//! reached (`cursor`); otherwise (a restart after a power cut, a step that
//! failed, a pass that began) the walk begins again and passes `cursor` books
//! first, which costs what those books cost and gives the same state, as every
//! phase is idempotent.
//!
//! **Limit.** The places are the storage's: a file added to the folder from
//! outside `core.files` (a transfer upload, a computer over USB) while the job
//! runs may make a step miss a book or take one twice. A book missed there
//! keeps its details and list entries under the old path until the events
//! pass, which walks again and carries each book once more (carrying a book
//! is idempotent), so only an addition that shifts both passes leaves one
//! behind; the next catalog walk then makes it afresh.
use super::lib_service::moves::own;
use alloc::{string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};
use paperpaw_platform::Storage;
use paperpaw_sdk::Formats;

/// What a saved walk is for.
#[derive(PartialEq)]
struct Pass {
    /// The events pass (the books pass otherwise).
    events: bool,
    to: String,
    /// Books the walk has passed.
    at: u64,
}

#[derive(Default)]
struct Saved {
    pass: Option<Pass>,
    /// Folders still to read; the last is next.
    stack: Vec<String>,
    /// The folder being read and the place to go on from.
    current: Option<(String, u64)>,
}

/// The walk kept between steps.
#[derive(Default)]
pub(crate) struct Resume(RefCell<Saved>);

impl Saved {
    fn clear(&mut self) {
        *self = Saved::default();
    }

    fn exhausted(&self) -> bool {
        self.stack.is_empty() && self.current.is_none()
    }
}

fn joined(folder: &str, name: &str) -> Result<String, i32> {
    let mut path = String::new();
    path.try_reserve_exact(folder.len() + 1 + name.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    if !folder.is_empty() {
        path.push_str(folder);
        path.push('/');
    }
    path.push_str(name);
    Ok(path)
}

/// Up to `want` books after the walk's place: whether the walk has more.
fn take(
    saved: &mut Saved,
    storage: &dyn Storage,
    formats: &Formats,
    want: usize,
) -> Result<(Vec<String>, bool), i32> {
    let mut found: Vec<String> = Vec::new();
    found.try_reserve_exact(want).map_err(|_| ERR_NO_MEMORY)?;
    while found.len() < want {
        let (folder, place) = match saved.current.take() {
            Some(current) => current,
            None => match saved.stack.pop() {
                Some(folder) => (folder, 0),
                None => break,
            },
        };
        let room = want - found.len();
        let (mut direct, mut unknown, mut folders): (Vec<String>, Vec<String>, Vec<String>) =
            (Vec::new(), Vec::new(), Vec::new());
        let mut failed = Ok(());
        let next = storage
            .list_from(&folder, place, &mut |entry| {
                if entry.name.starts_with('.') {
                    return true;
                }
                let book = !entry.is_dir && formats.supports(entry.name);
                let maybe = !entry.is_dir && !book && formats.may_sniff(entry.name);
                if (book || maybe) && direct.len() + unknown.len() == room {
                    // Enough: this entry is where the next step starts.
                    return false;
                }
                if !(entry.is_dir || book || maybe) {
                    return true;
                }
                let kept = joined(&folder, entry.name).and_then(|path| {
                    let list = if entry.is_dir {
                        &mut folders
                    } else if book {
                        &mut direct
                    } else {
                        &mut unknown
                    };
                    list.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
                    list.push(path);
                    Ok(())
                });
                failed = kept;
                failed.is_ok()
            })
            .map_err(super::storage::status);
        failed?;
        let next = next?;
        saved
            .stack
            .try_reserve(folders.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        saved.stack.extend(folders);
        saved.current = next.map(|place| (folder, place));
        found.extend(direct);
        for path in unknown {
            if found.len() < want && formats.for_file(storage, &path).is_some() {
                found.push(path);
            }
        }
    }
    Ok((found, !saved.exhausted()))
}

impl Resume {
    /// The books of folder `to` after the first `cursor` of the pass, at most
    /// `want` of them, and whether there are more. Goes on from the saved walk
    /// when it is at `cursor`, else begins again and passes `cursor` books.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`, the card's status; the saved walk is dropped then.
    pub(crate) fn books(
        &self,
        storage: &dyn Storage,
        formats: &Formats,
        events: bool,
        to: &str,
        cursor: u64,
        want: usize,
    ) -> Result<(Vec<String>, bool), i32> {
        let mut saved = self.0.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let result = Self::walk(&mut saved, storage, formats, events, to, cursor, want);
        match &result {
            Ok((books, more)) if *more => {
                if let Some(pass) = saved.pass.as_mut() {
                    pass.at = cursor + books.len() as u64;
                }
            }
            // Done (or failed): nothing is kept.
            _ => saved.clear(),
        }
        result
    }

    fn walk(
        saved: &mut Saved,
        storage: &dyn Storage,
        formats: &Formats,
        events: bool,
        to: &str,
        cursor: u64,
        want: usize,
    ) -> Result<(Vec<String>, bool), i32> {
        let continues = saved
            .pass
            .as_ref()
            .is_some_and(|p| p.events == events && p.to == to && p.at == cursor);
        if !continues {
            saved.clear();
            saved.stack.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
            saved.stack.push(own(to)?);
            saved.pass = Some(Pass {
                events,
                to: own(to)?,
                at: cursor,
            });
            // Pass the books before `cursor`, a piece at a time.
            let mut left = cursor;
            while left > 0 {
                let piece = usize::try_from(left).map_or(want, |n| n.min(want.max(1)));
                let (books, more) = take(saved, storage, formats, piece)?;
                left = left.saturating_sub(books.len() as u64);
                if !more || books.is_empty() && saved.exhausted() {
                    break;
                }
            }
        }
        take(saved, storage, formats, want)
    }
}
