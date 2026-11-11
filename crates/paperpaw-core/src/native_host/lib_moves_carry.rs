//! What follows a library move in the catalog ([`MoveCarry`] over the
//! kernel's [`Env`]): the books' details and place in the lists, the data
//! plugins keep by a book's path, the reader chosen for a folder, folder
//! settings, and the library's book-moved events (`files.md` 2.6,
//! `library.md` 6).
//!
//! It is bound to one step of the main loop: the kernel's `Env` (the
//! storage, the formats, the plugins' book-data folders, the settings) and
//! the queue the events go to. Every step is idempotent:
//!
//! - a book's key is moved by renaming what is kept under the old one, so
//!   carrying it a second time finds nothing under the old key and changes
//!   nothing; data that is already under the new key is never replaced by an
//!   older copy because the old one is gone;
//! - folder settings and reader choices are moved by what they name, and
//!   after the move nothing names the old folder;
//! - events are sent **at least once**: a job replayed after a power cut
//!   sends those of the step it was cut in again (plugins keep data by path,
//!   so moving a path twice is harmless); a step repeated after a
//!   transient card error in the data's move sends `book_moved` of its books
//!   again the same way.
//!
//! The books of the moved folder are found by walking it (`to`), by format,
//! the way the catalog walk finds them; the old path of a book is `from` plus
//! the rest of its path. A cursor (the number of books passed) saves work in a
//! step; replaying from a smaller one gives the same state. A `.bak` sibling
//! of a book goes with it. A moved single file is one book whatever its
//! name now (`a.epub.bak` may be named back).
use super::book_data_steps::{self, Progress};
use super::lib_moves_host::MoveCarry;
use super::lib_moves_walk::Resume;
use super::lib_service::moves::{Phase, STEP_BOOKS, own};
use alloc::{format, string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};
use paperpaw_platform::Storage;
use paperpaw_sdk::{
    Command, Editor, Env, Formats, SettingValue, Settings,
    builtin::{events, screens, slots},
    files, library,
    library::BookData,
    readers,
};

/// Pieces of a replaced book's data one call removes (see
/// [`book_data_steps::remove_data`]).
const DISCARD_PIECES: u32 = 4;

/// A step's view of the catalog.
pub(crate) struct EnvCarry<'a> {
    storage: &'a dyn Storage,
    formats: &'a Formats,
    book_data: &'a [BookData],
    settings: &'a Settings,
    /// The walk kept between steps.
    resume: &'a Resume,
    /// Where the events and the setting changes go (the kernel's queue).
    out: RefCell<&'a mut dyn FnMut(Command)>,
}

impl<'a> EnvCarry<'a> {
    /// The catalog of `env`, sending commands to `out`.
    pub(crate) fn new(
        env: &'a Env<'a>,
        resume: &'a Resume,
        out: &'a mut dyn FnMut(Command),
    ) -> Self {
        Self::from_parts(
            env.storage(),
            env.formats(),
            env.book_data(),
            env.settings(),
            resume,
            out,
        )
    }

    pub(crate) fn from_parts(
        storage: &'a dyn Storage,
        formats: &'a Formats,
        book_data: &'a [BookData],
        settings: &'a Settings,
        resume: &'a Resume,
        out: &'a mut dyn FnMut(Command),
    ) -> Self {
        Self {
            storage,
            formats,
            book_data,
            settings,
            resume,
            out: RefCell::new(out),
        }
    }

    fn send(&self, command: Command) -> Result<(), i32> {
        let mut out = self.out.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        (*out)(command);
        Ok(())
    }

    /// The books at `to` after the first `cursor` of the pass, at most
    /// [`STEP_BOOKS`] of them, and whether there are more: a folder's books
    /// by the resumable walk ([`Resume`]: a step costs what its books cost,
    /// not the folder), or `to` itself when it is a file (whatever its
    /// name). They are collected first and used after the walk: the walk
    /// holds the storage's listing, and nothing may change the storage in it.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`, the card's status.
    fn window(&self, events: bool, to: &str, cursor: u64) -> Result<(Vec<String>, bool), i32> {
        let to = to.trim_matches('/');
        if !files::is_dir(self.storage, to) {
            let mut one = Vec::new();
            if cursor == 0 && self.storage.exists(to) {
                one.try_reserve_exact(1).map_err(|_| ERR_NO_MEMORY)?;
                one.push(own(to)?);
            }
            return Ok((one, false));
        }
        self.resume
            .books(self.storage, self.formats, events, to, cursor, STEP_BOOKS)
    }

    /// The old path of `book` (a path at or below `to`).
    fn old_path(from: &str, to: &str, book: &str) -> String {
        let rest = book
            .trim_matches('/')
            .strip_prefix(to.trim_matches('/'))
            .unwrap_or("");
        format!("{}{rest}", from.trim_matches('/'))
    }

    /// One book to its new key: details, cover, place among the recently
    /// opened and the favourites, and what plugins keep by its path. A `.bak`
    /// sibling goes too.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` or the card's status from the data's move: the step
    /// is repeated (idempotent) and goes on from what is moved.
    fn carry_one(&self, old: &str, new: &str) -> Result<(), i32> {
        library::moved(self.storage, old, new);
        book_data_steps::move_data(self.storage, self.book_data, old, new)?;
        let (old_bak, new_bak) = (format!("{old}.bak"), format!("{new}.bak"));
        if self.storage.exists(&new_bak) {
            library::moved(self.storage, &old_bak, &new_bak);
            book_data_steps::move_data(self.storage, self.book_data, &old_bak, &new_bak)?;
        }
        Ok(())
    }

    /// Settings that name folder `from` or one in it, chosen as folders,
    /// name it at `to` now.
    fn follow_folders(&self, from: &str, to: &str) -> Result<(), i32> {
        for spec in self.settings.specs() {
            let folder = spec
                .extra()
                .editor
                .as_ref()
                .is_some_and(|editor| match editor {
                    Editor::Slot(slot) => *slot == slots::CHOOSE_FOLDER,
                    Editor::Screen(screen) => *screen == screens::FOLDER_EDITOR,
                    // A kind of editor added later is not a folder one.
                    _ => false,
                });
            if !folder {
                continue;
            }
            let value = self.settings.text(spec.id.as_str());
            if let Some(moved) = library::moved_folder(self.storage, value, from, to) {
                self.send(Command::SetSetting(
                    spec.id.clone(),
                    SettingValue::Text(moved.into()),
                ))?;
            }
        }
        Ok(())
    }
}

impl MoveCarry for EnvCarry<'_> {
    fn discard(&self, to: &str) -> Result<bool, i32> {
        library::removed(self.storage, to);
        // One bounded piece budget a call: while the book still has data the
        // step answers `false` and runs again (nothing is recorded as
        // discarded until it is all gone, and the repeat finds what is left).
        Ok(
            book_data_steps::remove_data(self.storage, self.book_data, to, DISCARD_PIECES)?
                == Progress::Done,
        )
    }

    fn carry(&self, from: &str, to: &str, phase: Phase) -> Result<Phase, i32> {
        match phase {
            Phase::Books(cursor) => {
                let (books, more) = self.window(false, to, cursor)?;
                for book in &books {
                    self.carry_one(&Self::old_path(from, to, book), book)?;
                }
                Ok(if more {
                    Phase::Books(cursor + books.len() as u64)
                } else {
                    Phase::Assignments
                })
            }
            Phase::Assignments => {
                readers::folder_moved(self.storage, from, to).map_err(super::storage::status)?;
                Ok(Phase::Settings)
            }
            Phase::Settings => {
                self.follow_folders(from, to)?;
                Ok(Phase::Events(0))
            }
            Phase::Events(cursor) => {
                // A file moved is one event, whatever its name now.
                if !files::is_dir(self.storage, to) {
                    self.send(events::BOOK_MOVED.emit(events::BookMoved::new(
                        from.trim_matches('/').into(),
                        to.trim_matches('/').into(),
                    )))?;
                    self.send(events::LIBRARY_CHANGED.emit(()))?;
                    return Ok(Phase::Done);
                }
                let (books, more) = self.window(true, to, cursor)?;
                for book in &books {
                    // Carried once more: a book the first pass missed (a file
                    // added from outside shifted its place) is caught here;
                    // for one already carried it changes nothing.
                    let old = Self::old_path(from, to, book);
                    self.carry_one(&old, book)?;
                    self.send(events::BOOK_MOVED.emit(events::BookMoved::new(old, book.clone())))?;
                }
                if more {
                    return Ok(Phase::Events(cursor + books.len() as u64));
                }
                self.send(events::LIBRARY_CHANGED.emit(()))?;
                Ok(Phase::Done)
            }
            Phase::Done => Ok(Phase::Done),
        }
    }

    fn carry_book(&self, from: &str, to: &str, book: &str) -> Result<(), i32> {
        self.carry_one(&Self::old_path(from, to, book), book.trim_matches('/'))
    }
}

#[cfg(test)]
#[path = "lib_moves_carry_tests.rs"]
mod tests;
