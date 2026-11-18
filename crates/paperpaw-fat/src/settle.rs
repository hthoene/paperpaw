//! A card after a cut: the journal of moves (`~PPMOVE.BIN`), finished
//! at the next start; folders settled when first looked at; the
//! temporary files of earlier versions taken away where they are
//! provably copies. See the [crate documentation](crate).

use super::*;

pub(super) const MOVE: &[u8; 4] = b"PPMV";

pub(super) const MOVE_HEAD: usize = 12;

/// What [`JOURNAL`] holds for a move from `from` to `to`.
pub(super) fn move_record(from: &str, to: &str) -> Result<Vec<u8>, StorageError> {
    let paths = || {
        from.bytes()
            .chain(core::iter::once(b'\n'))
            .chain(to.bytes())
    };
    let len = from.len() + 1 + to.len();
    let mut record = Vec::new();
    record
        .try_reserve_exact(MOVE_HEAD + len)
        .map_err(|_| StorageError::OutOfMemory)?;
    record.extend_from_slice(MOVE);
    record.extend_from_slice(
        &u32::try_from(len)
            .map_err(|_| StorageError::TooLarge)?
            .to_le_bytes(),
    );
    record.extend_from_slice(&(fnv(paths()) as u32).to_le_bytes());
    record.extend(paths());
    Ok(record)
}

/// The move `record` holds (all of [`JOURNAL`] it says), if it is whole.
pub(super) fn recorded_move(record: &[u8]) -> Option<(&str, &str)> {
    let (head, paths) = record.split_at_checked(MOVE_HEAD)?;
    let sum = u32::from_le_bytes([head[8], head[9], head[10], head[11]]);
    if head[..4] != MOVE[..] || fnv(paths.iter().copied()) as u32 != sum {
        return None;
    }
    let (from, to) = core::str::from_utf8(paths).ok()?.split_once('\n')?;
    (is_valid_path(from) && is_valid_path(to)).then_some((from, to))
}

/// Bytes of a temporary file of an earlier version compared with files
/// beside it, at most (see `clean_old`): those versions wrote no bigger
/// files through them but from memory.
pub(super) const OLD_SAME: u64 = 64 * 1024;

/// Bytes compared in a folder at most, once a start (see `clean_old`).
pub(super) const OLD_BUDGET: u64 = 256 * 1024;

/// Whether file `temp` in `dir` holds the bytes of another file there,
/// not a temporary file of an earlier version, compared while `budget`
/// lasts.
pub(super) fn same_as_beside<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    temp: &FatEntry<IO>,
    budget: &mut u64,
) -> bool {
    let len = temp.len();
    for entry in entries(dir) {
        let Ok(entry) = entry else {
            return false;
        };
        if !entry.is_file() || entry.len() != len || is_old_temp(&entry) {
            continue;
        }
        if *budget < len {
            return false;
        }
        *budget -= len;
        if same_bytes(&mut temp.to_file(), &mut entry.to_file(), len) {
            return true;
        }
    }
    false
}

/// Whether files `a` and `b` begin with the same `len` bytes, read a
/// small piece at a time.
pub(super) fn same_bytes<IO: ReadWriteSeek + 'static>(
    a: &mut FatFile<IO>,
    b: &mut FatFile<IO>,
    len: u64,
) -> bool {
    let (mut x, mut y) = ([0u8; 128], [0u8; 128]);
    let mut left = len;
    while left > 0 {
        let n = usize::try_from(left).map_or(x.len(), |left| left.min(x.len()));
        if a.read_exact(&mut x[..n]).is_err() || b.read_exact(&mut y[..n]).is_err() {
            return false;
        }
        if x[..n] != y[..n] {
            return false;
        }
        left -= n as u64;
    }
    true
}

impl<IO: ReadWriteSeek + 'static> FatStorage<IO> {
    /// Records in [`JOURNAL`] a move from `from` to `to`, makes it (`op`)
    /// and clears the record: cut off between, or where `op` fails, the
    /// next call that changes the card settles it (`settle_moves`).
    pub(super) fn journaled(
        &self,
        from: &str,
        to: &str,
        op: impl FnOnce() -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        self.write_move(&move_record(from, to)?)?;
        let done = op().and_then(|()| self.write_move(&[0; 4]));
        if done.is_err() {
            self.moves_settled.set(false);
        }
        done
    }

    /// Writes `record` at the start of [`JOURNAL`]: in its place where it
    /// fits (taking no cluster, cut off or not), else as the file anew.
    pub(super) fn write_move(&self, record: &[u8]) -> Result<(), StorageError> {
        self.close(JOURNAL)?;
        match find_file(&self.root(), JOURNAL, false) {
            Ok((mut file, len)) if len >= record.len() as u64 => {
                file.set_attributes(FileAttributes::HIDDEN);
                Self::write_all(&mut file, record)
            }
            // Nothing to clear.
            Err(StorageError::NotFound) if record == [0; 4] => Ok(()),
            Ok(_) | Err(StorageError::NotFound) => {
                let len = record.len().max(BLOCK);
                let mut whole = Vec::new();
                whole
                    .try_reserve_exact(len)
                    .map_err(|_| StorageError::OutOfMemory)?;
                whole.extend_from_slice(record);
                whole.resize(len, 0);
                self.write_whole(JOURNAL, &whole)
            }
            Err(e) => Err(e),
        }
    }

    /// The move [`JOURNAL`] records, if any.
    #[inline(never)]
    pub(super) fn recorded(&self) -> Result<Option<(String, String)>, StorageError> {
        self.close(JOURNAL)?;
        let (mut file, len) = match find_file(&self.root(), JOURNAL, false) {
            Ok(found) => found,
            Err(StorageError::NotFound) => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut head = [0u8; MOVE_HEAD];
        if len < MOVE_HEAD as u64 {
            return Ok(None);
        }
        file.read_exact(&mut head).map_err(error)?;
        let paths = u64::from(u32::from_le_bytes([head[4], head[5], head[6], head[7]]));
        if head[..4] != MOVE[..] || MOVE_HEAD as u64 + paths > len {
            return Ok(None);
        }
        let paths = usize::try_from(paths).map_err(|_| StorageError::TooLarge)?;
        let mut record = Vec::new();
        record
            .try_reserve_exact(MOVE_HEAD + paths)
            .map_err(|_| StorageError::OutOfMemory)?;
        record.extend_from_slice(&head);
        record.resize(MOVE_HEAD + paths, 0);
        file.read_exact(&mut record[MOVE_HEAD..]).map_err(error)?;
        Ok(recorded_move(&record).map(|(from, to)| (from.to_string(), to.to_string())))
    }

    /// Settles a move cut off before, as [`JOURNAL`] records it: once
    /// after the start, before the card is changed otherwise. Where it
    /// cannot be settled, its record is kept for the next start and its
    /// paths are held up until then (see `held_up`): all else goes on.
    /// Where the record cannot be read, it fails, tried again at the
    /// next change.
    pub(super) fn settle_moves(&self) -> Result<(), StorageError> {
        if self.moves_settled.get() || self.settling.get() {
            return Ok(());
        }
        self.settle_recorded()
    }

    /// [`Self::settle_moves`] where the record was not looked for yet: once
    /// after the start, so apart from the call every folder makes, whose frame
    /// is held while the card is read below it.
    #[inline(never)]
    fn settle_recorded(&self) -> Result<(), StorageError> {
        // No folder is settled meanwhile: a file's new content there may
        // be the file moved, which only the record tells.
        self.settling.set(true);
        let recorded = self.recorded();
        if let Ok(Some((from, to))) = &recorded {
            let settled = self
                .finish_move(from, to)
                .and_then(|()| self.write_move(&[0; 4]));
            if settled.is_err() {
                *self.stuck.borrow_mut() = Some((from.clone(), to.clone()));
            }
        }
        self.settling.set(false);
        self.moves_settled.set(recorded.is_ok());
        recorded.map(|_| ())
    }

    /// Whether `path` is held up by a move that could not be settled
    /// (see `settle_moves`): it is at either of its paths, or lies in
    /// them, or they lie in it.
    pub(super) fn held_up(&self, path: &str) -> bool {
        self.stuck.borrow().as_ref().is_some_and(|(from, to)| {
            [from, to]
                .into_iter()
                .any(|held| is_within(path, held) || is_within(held, path))
        })
    }

    /// Refuses `path` where it is [`held_up`](Self::held_up).
    pub(super) fn hold(&self, path: &str) -> Result<(), StorageError> {
        match self.held_up(path) {
            true => Err(StorageError::Io),
            false => Ok(()),
        }
    }

    /// Settles folder `path` (`dir`) when first looked at after the start,
    /// before any file in it is opened: what cuts left of writes of its
    /// files is finished (see `settled`). Its entries read once; again
    /// only for files written through files beside them. Never where the
    /// record of moves cannot be read: a file's new content may be the
    /// file moved, which only the record tells. A file that cannot be
    /// settled is left as it is (whole, its old content or its new) and
    /// tried again when the folder is next looked up: the folder stays
    /// in reach.
    pub(super) fn settle_folder(&self, path: &str, dir: &FatDir<IO>) {
        let key = hash(path);
        let moves = self.settle_moves();
        if moves.is_err() || self.settling.get() || self.settled_folders.borrow().contains(&key) {
            return;
        }
        if !self.settle_files(path, dir) {
            return;
        }
        let mut folders = self.settled_folders.borrow_mut();
        if folders.len() == SETTLED {
            folders.remove(0);
        }
        folders.push(key);
    }

    /// Settles the files of folder `path` (`dir`) that cuts left written
    /// through files beside them (see `settle_folder`): whether all are.
    #[inline(never)]
    fn settle_files(&self, path: &str, dir: &FatDir<IO>) -> bool {
        let Some(temps) = Self::temps_in(dir) else {
            return false;
        };
        let mut names = Vec::new();
        if !temps.is_empty() && !Self::files_of_temps(dir, &temps, &mut names) {
            return false;
        }
        let mut all = true;
        for name in names {
            let file = child(path, &name);
            all &= !self.held_up(&file) && self.close(&file).is_ok() && settled(dir, &name).is_ok();
        }
        all
    }

    /// The short names of the files in `dir` that a write goes through
    /// (see [`is_temp_name`]); `None` where `dir` cannot be read. A call of
    /// its own, so that its entries do not add to the frame of its caller.
    #[inline(never)]
    fn temps_in(dir: &FatDir<IO>) -> Option<Vec<Vec<u8>>> {
        let mut temps = Vec::new();
        for entry in entries(dir) {
            let entry = entry.ok()?;
            let short = entry.short_file_name_as_bytes();
            if entry.is_file() && core::str::from_utf8(short).is_ok_and(is_temp_name) {
                temps.push(short.to_ascii_uppercase());
            }
        }
        Some(temps)
    }

    /// The names of the files in `dir` that one of `temps` belongs to, into
    /// `names`: whether `dir` could be read to its end.
    #[inline(never)]
    fn files_of_temps(dir: &FatDir<IO>, temps: &[Vec<u8>], names: &mut Vec<String>) -> bool {
        for entry in entries(dir) {
            let Ok(entry) = entry else {
                return false;
            };
            let name = entry.file_name();
            let of = Temps::of(&name);
            let has = |temp: &str| temps.iter().any(|t| t == temp.as_bytes());
            if entry.is_file() && !is_temp_name(&name) && (has(of.tmp()) || has(of.ready())) {
                names.push(name);
            }
        }
        true
    }

    /// Finishes the move from `from` to `to` that [`JOURNAL`] records:
    /// where the file or folder is at both places, its old entry is taken
    /// away, its clusters left to the new one; where it is still under
    /// the name of its new content, it takes its place. Where it did not
    /// reach its new place, it stays where it was.
    #[inline(never)]
    pub(super) fn finish_move(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.close(from)?;
        self.close(to)?;
        self.forget(from);
        self.forget(to);
        let to_dir = match self.dir(parent(to)) {
            Ok(dir) => dir,
            Err(StorageError::NotFound) => return Ok(()),
            Err(e) => return Err(e),
        };
        let temps = Temps::of(name(to));
        let there = lookup(&to_dir, name(to), true)?;
        // A file or a folder (moved to another case of its name).
        let new = lookup(&to_dir, temps.ready(), true)?.entry;
        let source = match self.dir(parent(from)) {
            Ok(dir) => match lookup(&dir, name(from), true)? {
                // Never moved: rust-fatfs could take another entry.
                Lookup { others: true, .. } => return Ok(()),
                found => found.entry.map(|entry| (dir, entry)),
            },
            Err(StorageError::NotFound) => None,
            Err(e) => return Err(e),
        };
        if there.others {
            return Ok(());
        }
        let same = same_path(from, to);
        if let Some((dir, entry)) = source {
            let moved = new.as_ref().is_some_and(|new| holds_same(&entry, new))
                || (!same && there.entry.as_ref().is_some_and(|e| holds_same(&entry, e)));
            if !moved {
                return Ok(());
            }
            dir.unlink(name(from)).map_err(error)?;
        }
        let folder_there = there.entry.as_ref().is_some_and(FatEntry::is_dir);
        match new {
            Some(new) if new.is_dir() && (same || there.entry.is_none()) => {
                rename_in(&to_dir, temps.ready(), &to_dir, name(to))?;
            }
            Some(new) if new.is_file() && !folder_there => {
                to_dir.replace(temps.ready(), name(to)).map_err(error)?;
            }
            _ => {}
        }
        // A folder moved names its new parent `..`, the last step of a
        // move: folders are followed only where it does (see `damaged`).
        // Not in a damaged one (nothing is written there), and the move
        // settled even where it cannot be done: the folder is then cut
        // off when removed, its space left.
        #[expect(
            clippy::let_underscore_must_use,
            reason = "a folder not pointed up is cut off when removed"
        )]
        let _ = self.point_up(&to_dir, name(to));
        Ok(())
    }

    /// Points the `..` of folder `name` in `dir` at `dir`, where it is
    /// intact (see [`FatStorage::is_intact`]).
    fn point_up(&self, dir: &FatDir<IO>, name: &str) -> Result<(), StorageError> {
        match lookup(dir, name, true)? {
            Lookup {
                entry: Some(entry),
                others: false,
                ..
            } if entry.is_dir() && self.is_intact(&entry)? => {
                entry.to_dir().point_up(dir).map_err(error)
            }
            _ => Ok(()),
        }
    }

    /// Takes away in folder `path` (`dir`) the temporary files of earlier
    /// versions that are provably no file's only copy: empty, or holding
    /// the bytes of a file beside them (the only copy of a file whose name
    /// is lost, its bytes stay in that one). Their entries only: their
    /// clusters stay taken, as they may be another file's (freed and
    /// given again after a cut in an earlier version). Once a start a
    /// folder, comparing at most [`OLD_BUDGET`] bytes; what is not
    /// provably superseded stays.
    pub(super) fn clean_old(&self, path: &str, dir: &FatDir<IO>) {
        let key = hash(path);
        {
            let mut cleaned = self.cleaned.borrow_mut();
            if cleaned.contains(&key) {
                return;
            }
            if cleaned.len() == CLEANED {
                cleaned.remove(0);
            }
            cleaned.push(key);
        }
        let mut old = Vec::new();
        for entry in entries(dir) {
            match entry {
                Ok(entry) if is_old_temp(&entry) => old.push(entry.file_name()),
                Ok(_) => {}
                Err(_) => return,
            }
        }
        let mut budget = OLD_BUDGET;
        for name in old {
            let temp = match lookup(dir, &name, true) {
                Ok(Lookup {
                    entry: Some(temp),
                    others: false,
                    ..
                }) => temp,
                _ => continue,
            };
            let superseded = temp.len() == 0
                || (temp.len() <= OLD_SAME && same_as_beside(dir, &temp, &mut budget));
            if !superseded || self.close(&child(path, &name)).is_err() {
                continue;
            }
            if dir.unlink(&name).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
#[path = "settle_tests.rs"]
mod tests;
