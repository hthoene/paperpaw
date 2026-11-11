//! Names looked up in a folder: from its start, or for a folder read
//! whole a moment ago, from where the last name was found (rust-fatfs's
//! note), so that names looked up in the order the folder lists them
//! cost a few blocks each. See the [crate documentation](crate).

use super::*;

/// What a name stands for in a folder.
pub(super) struct Lookup<IO: ReadWriteSeek + 'static> {
    /// The entry it names: by its long name, or by its short name where
    /// it has no long one.
    pub(super) entry: Option<FatEntry<IO>>,
    /// rust-fatfs could take another entry for the name (one whose short
    /// name it is, as `PAPERP~1` for `.paperpaw`): the name is not handed
    /// to rust-fatfs then.
    pub(super) others: bool,
    /// Another entry has its long name too: the old file and the new one,
    /// a replace cut off between (see `settled`).
    pub(super) twice: bool,
    /// Its file of [`Temps::tmp`], where looked for.
    pub(super) tmp: Option<FatEntry<IO>>,
    /// Its file of [`Temps::ready`], where looked for.
    pub(super) ready: Option<FatEntry<IO>>,
    /// A temporary file of an earlier version is in the folder, where
    /// looked for (see [`is_old_temp`]).
    pub(super) old: bool,
}

/// Looks up `name` in `dir`, as PaperPaw names entries (see the [crate
/// documentation](crate)): every entry, where the name goes to rust-fatfs
/// after (`whole`), else up to the one it names.
pub(super) fn lookup<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
    whole: bool,
) -> Result<Lookup<IO>, StorageError> {
    lookup_with(dir, name, whole, None)
}

/// [`lookup`], looking for the files of `temps` on the way (all of them
/// where `name` is not there, or `whole`).
pub(super) fn lookup_with<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
    whole: bool,
    temps: Option<&Temps>,
) -> Result<Lookup<IO>, StorageError> {
    let mut found = Lookup {
        entry: None,
        others: false,
        twice: false,
        tmp: None,
        ready: None,
        old: false,
    };
    if let Some(quick) = quick_lookup(dir, name) {
        found.entry = quick?;
        return Ok(found);
    }
    let mut read = Scan::of(dir);
    for entry in entries(dir) {
        let entry = entry?;
        read.met(&entry);
        let by_short = short_name_is(&entry, name);
        let (named, other) = match long_name_is(&entry, name) {
            Some(by_long) => (by_long, by_short),
            None => (by_short, false),
        };
        if named && found.entry.is_none() {
            found.entry = Some(entry);
            if !whole {
                return Ok(found);
            }
            continue;
        } else if named {
            found.twice = true;
        } else if other {
            found.others = true;
        }
        let Some(temps) = temps.filter(|_| entry.is_file() && may_be_temp(&entry)) else {
            continue;
        };
        // By its short name too: a temporary's long name, if lost, was
        // the same.
        let is = |temp| long_name_is(&entry, temp) == Some(true) || short_name_is(&entry, temp);
        if is(temps.tmp()) && found.tmp.is_none() {
            found.tmp = Some(entry);
        } else if is(temps.ready()) && found.ready.is_none() {
            found.ready = Some(entry);
        } else {
            found.old |= is_old_temp(&entry);
        }
    }
    read.note(dir);
    Ok(found)
}

/// Folders of at least this many entries, read whole without a file a
/// write goes through in them (see [`may_be_temp`]), are noted
/// (`Dir::set_note`, kept by rust-fatfs until an entry is added to the
/// folder): a name is looked for in them from where the last one was
/// found ([`quick_lookup`]). So names taken one after the other as the
/// folder lists them (its books' details moved elsewhere, say) are each
/// found at once, not by reading the folder up to them.
pub(super) const BIG: usize = 256;

/// What a folder read whole held: how many entries, whether one may be
/// a file a write goes through; and how many entries the file system had
/// written when it was begun.
pub(super) struct Scan {
    pub(super) entries: usize,
    pub(super) temps: bool,
    pub(super) written: u64,
}

impl Scan {
    pub(super) fn of<IO: ReadWriteSeek + 'static>(dir: &FatDir<IO>) -> Self {
        Self {
            entries: 0,
            temps: false,
            written: dir.entries_written(),
        }
    }

    pub(super) fn met<IO: ReadWriteSeek + 'static>(&mut self, entry: &FatEntry<IO>) {
        self.entries += 1;
        self.temps |= may_be_temp(entry);
    }

    /// Notes folder `dir`, read whole, where it is [`BIG`] and holds no
    /// file a write goes through: names are looked for from its start.
    /// Not where an entry was written meanwhile (by whoever was shown the
    /// entries as they were read), in it or not.
    pub(super) fn note<IO: ReadWriteSeek + 'static>(&self, dir: &FatDir<IO>) {
        if self.entries >= BIG && !self.temps && dir.entries_written() == self.written {
            dir.set_note(0);
        }
    }
}

/// [`lookup_with`] in a folder noted (see [`BIG`]), for a name that no
/// short name can be (longer than 8.3): no entry but the one of that long
/// name stands for it, and no file a write goes through is there (none
/// was when the folder was read whole, and no entry was added since), so
/// none is looked for. The first entry of the name from where the last
/// one was found on, then from the start up to there, is the one; where
/// it begins, the next is looked for. `None` where the folder is not noted or
/// the name may be a short one.
#[inline(never)]
pub(super) fn quick_lookup<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    name: &str,
) -> Option<Result<Option<FatEntry<IO>>, StorageError>> {
    if name.chars().count() <= 12 {
        return None;
    }
    let at = dir.note()?;
    let named = |entry: &FatEntry<IO>| long_name_is(entry, name) == Some(true);
    // Looked for from where it begins: found again at once (a move looks
    // a name up more than once), the next of the folder's order soon after.
    let found = |entry: FatEntry<IO>| {
        dir.set_note(entry.offset_range().0);
        Ok(Some(entry))
    };
    // From the start where the folder cannot be read from there.
    let (after, before) = match dir.iter_from(at) {
        Ok(after) => (Some(after), at),
        Err(_) => (None, u64::MAX),
    };
    for entry in after
        .into_iter()
        .flat_map(|after| bounded(after.map(|e| e.map_err(error))))
    {
        match entry {
            Ok(entry) if named(&entry) => return Some(found(entry)),
            Ok(_) => {}
            Err(e) => return Some(Err(e)),
        }
    }
    for entry in entries(dir) {
        match entry {
            Ok(entry) if entry.offset_range().0 >= before => break,
            Ok(entry) if named(&entry) => return Some(found(entry)),
            Ok(_) => {}
            Err(e) => return Some(Err(e)),
        }
    }
    Some(Ok(None))
}
