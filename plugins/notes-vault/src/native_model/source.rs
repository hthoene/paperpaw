//! Where the lists' entries come from: a folder of the vault, as it is on
//! the storage, or the index and the file of tags, a line at a time.
use alloc::string::String;

use paperpaw_sdk::Storage;
use paperpaw_sdk::native::Error;

use super::index;
use super::note;
use super::page::{Emit, RowRef};
use super::text::{contains_lowercase, join_into};

/// What a list lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A folder: its folders and notes.
    Folder(String),
    /// The notes of the index: those in `folder` and below it (all, if
    /// empty), whose title has every word of `query` (all, if empty).
    Notes { folder: String, query: String },
    /// The notes that have a tag.
    Tag(String),
}

impl Source {
    /// Calls `emit` once for each entry the source has. `vault` is the card,
    /// `data` the plugin's data folder (the index).
    ///
    /// # Errors
    /// [`Error::NoMemory`], or what `emit` says.
    pub fn feed(
        &self,
        vault: &dyn Storage,
        data: &dyn Storage,
        emit: &mut Emit<'_>,
    ) -> Result<(), Error> {
        let mut failed: Option<Error> = None;
        match self {
            Source::Folder(dir) => {
                let mut path = String::new();
                // A folder that cannot be read has no entries.
                vault
                    .list(dir, &mut |entry| {
                        if failed.is_some() || note::is_hidden(entry.name) {
                            return;
                        }
                        let wanted = entry.is_dir || note::is_note(entry.name);
                        if !wanted {
                            return;
                        }
                        let made = join_into(&mut path, dir, entry.name).and_then(|()| {
                            emit(RowRef {
                                title: if entry.is_dir {
                                    entry.name
                                } else {
                                    note::title(&path)
                                },
                                path: &path,
                                folder: entry.is_dir,
                                modified: if entry.is_dir {
                                    0
                                } else {
                                    entry.modified.unwrap_or(0)
                                },
                            })
                        });
                        if let Err(error) = made {
                            failed = Some(error);
                        }
                    })
                    .ok();
            }
            Source::Notes { folder, query } => {
                let root = folder.trim_matches('/');
                index::notes(data, &mut |modified, path| {
                    // `folder/…`, as the interface-6 list tests it.
                    if !folder.is_empty()
                        && !path
                            .strip_prefix(root)
                            .is_some_and(|rest| rest.starts_with('/'))
                    {
                        return true;
                    }
                    let title = note::title(path);
                    if !query
                        .split_whitespace()
                        .all(|w| contains_lowercase(title, w))
                    {
                        return true;
                    }
                    match emit(RowRef {
                        title,
                        path,
                        folder: false,
                        modified,
                    }) {
                        Ok(()) => true,
                        Err(error) => {
                            failed = Some(error);
                            false
                        }
                    }
                })?;
            }
            Source::Tag(tag) => index::notes_with_tag(data, tag, &mut |path| match emit(RowRef {
                title: note::title(path),
                path,
                folder: false,
                modified: 0,
            }) {
                Ok(()) => true,
                Err(error) => {
                    failed = Some(error);
                    false
                }
            })?,
        }
        failed.map_or(Ok(()), Err)
    }
}
