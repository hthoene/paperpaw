//! What is kept between syncs, in the plugin's data folder: for each folder
//! of a pair the state of its files at the last sync (`s<pair>-<hash>.txt`: a
//! first line `dir=<sides>/<folder>`, then a line each: name, the card's size
//! and time, the server's size and `ETag` as hex, tab separated), the summary
//! of the last sync (`last.txt`) and the files in the trash (`trash.txt`).
//! `<sides>` is a number made of the pair's two folders ([`Of`]): a pair whose
//! folder or server is set anew has no state, so nothing is deleted by what
//! was synced somewhere else.
//! Every file is written whole under another name and renamed, so a cut leaves
//! the old or the new one.
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::native::{Error, storage::MAX_DATA};
use paperpaw_ui_kit::{
    dav::listing::hash,
    files::{Volume, for_each_line_on, read_all_on, replace_on},
    mem::push,
    text::{concat, copy, format},
};

use crate::native_names::order;
use crate::native_plan::{Kept, Side};
use crate::native_sync::Totals;

/// Files of a folder kept at most; the state's own, listings and the engine's
/// tables stay within it.
pub const MOST: usize = 200;
const TEMP: &str = ".new";

/// Whose state: the pair's number and a number made of the card's folder and
/// the server's address it was synced between.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Of {
    pub pair: usize,
    pub sides: u64,
}

impl Of {
    /// The state of pair `pair` between the card's folder `root` and the
    /// server's folder at `url`.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(pair: usize, root: &str, url: &str) -> Result<Self, Error> {
        let sides = hash(&concat(&[root, "\n", url])?);
        Ok(Self { pair, sides })
    }

    /// The first line of the state of the folder `rel`, after `dir=`.
    fn head(self, rel: &str) -> Result<String, Error> {
        format(format_args!("{:016x}/{rel}", self.sides))
    }
}

/// The state file of the folder `rel` of pair `pair`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn file(pair: usize, rel: &str) -> Result<String, Error> {
    format(format_args!("s{pair}-{:016x}.txt", hash(rel)))
}

fn parse(line: &str) -> Option<Kept> {
    let mut parts = line.split('\t');
    let name = parts.next().filter(|n| !n.is_empty())?;
    let mut number = |hex: bool| -> Option<u64> {
        let part = parts.next()?;
        if hex {
            u64::from_str_radix(part, 16).ok()
        } else {
            part.parse().ok()
        }
    };
    let (lsize, lstamp) = (number(false)?, number(false)?);
    let (rsize, rstamp) = (number(false)?, number(true)?);
    Some(Kept {
        name: copy(name).ok()?,
        local: Side {
            size: lsize,
            stamp: lstamp,
        },
        remote: Side {
            size: rsize,
            stamp: rstamp,
        },
    })
}

/// The state of the folder `rel`, sorted by name as the card tells names
/// apart ([`order`]); none where there is no file or it is another folder's
/// or another pair of folders' (a cut or odd file keeps nothing: never a
/// reason to delete).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn read(volume: &dyn Volume, of: Of, rel: &str) -> Result<Vec<Kept>, Error> {
    let name = file(of.pair, rel)?;
    let head = of.head(rel)?;
    let mut kept = Vec::new();
    let mut first = true;
    let mut ours = false;
    let result = for_each_line_on(volume, &name, &mut |line| {
        if first {
            first = false;
            ours = line.strip_prefix("dir=") == Some(head.as_str());
            return Ok(if ours {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            });
        }
        if kept.len() >= MOST {
            return Ok(ControlFlow::Break(()));
        }
        if let Some(entry) = parse(line) {
            push(&mut kept, entry)?;
        }
        Ok(ControlFlow::Continue(()))
    });
    match result {
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(Vec::new()),
        Ok(()) => {}
    }
    if !ours {
        return Ok(Vec::new());
    }
    paperpaw_sdk::sort::sort_by(&mut kept, |a, b| order(&a.name, &b.name));
    Ok(kept)
}

/// Keeps the state of the folder `rel`.
///
/// # Errors
/// What writing says; [`Error::NoMemory`].
pub fn write(volume: &dyn Volume, of: Of, rel: &str, kept: &[Kept]) -> Result<(), Error> {
    let name = file(of.pair, rel)?;
    let temp = concat(&[&name, TEMP])?;
    let head = of.head(rel)?;
    let done = (|| {
        let mut part = String::new();
        part.try_reserve(MAX_DATA).map_err(|_| Error::NoMemory)?;
        part.push_str("dir=");
        part.push_str(&head);
        part.push('\n');
        let mut first = true;
        for entry in kept {
            let line = format(format_args!(
                "{}\t{}\t{}\t{}\t{:x}\n",
                entry.name,
                entry.local.size,
                entry.local.stamp,
                entry.remote.size,
                entry.remote.stamp
            ))?;
            if part.len() + line.len() > MAX_DATA {
                flush(volume, &temp, &part, first)?;
                first = false;
                part.clear();
            }
            part.push_str(&line);
        }
        flush(volume, &temp, &part, first)?;
        volume.sync()?;
        volume.rename(&temp, &name)
    })();
    if done.is_err() {
        volume.remove(&temp).ok();
    }
    done
}

fn flush(volume: &dyn Volume, temp: &str, part: &str, first: bool) -> Result<(), Error> {
    if first {
        volume.write(temp, part.as_bytes())
    } else {
        volume.append(temp, part.as_bytes())
    }
}

/// What the last sync of all the pairs said.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Last {
    /// When the last run ended that synced every pair, Unix seconds (0:
    /// never): the widget's *Last sync*, and what an automatic sync waits from.
    pub time: i64,
    /// Files fetched and sent.
    pub down: u32,
    pub up: u32,
    /// Files deleted (the card's went to the trash).
    pub deleted: u32,
    /// Files kept under another name.
    pub conflicts: u32,
    /// Files that could not be synced.
    pub errors: u32,
    /// Changes of the card not yet sent, as the last look found them.
    pub pending: u32,
}

const LAST: &str = "last.txt";

impl Last {
    /// What a run that ended says: its totals, the card's `pending` changes,
    /// and `synced` as its time where it synced every pair. A run that failed
    /// keeps the time of the last one that did not.
    #[must_use]
    pub fn ended(self, total: Totals, pending: u32, synced: Option<i64>) -> Self {
        Self {
            time: synced.unwrap_or(self.time),
            down: total.down,
            up: total.up,
            deleted: total.deleted,
            conflicts: total.conflicts,
            errors: total.errors,
            pending,
        }
    }

    /// What was kept.
    #[must_use]
    pub fn read(volume: &dyn Volume) -> Self {
        let Ok(Some(bytes)) = read_all_on(volume, LAST, 512) else {
            return Self::default();
        };
        let mut last = Self::default();
        for line in String::from_utf8_lossy(&bytes).lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let n = value.parse().unwrap_or(0);
            match key {
                "time" => last.time = n,
                "down" => last.down = u32::try_from(n).unwrap_or(0),
                "up" => last.up = u32::try_from(n).unwrap_or(0),
                "deleted" => last.deleted = u32::try_from(n).unwrap_or(0),
                "conflicts" => last.conflicts = u32::try_from(n).unwrap_or(0),
                "errors" => last.errors = u32::try_from(n).unwrap_or(0),
                "pending" => last.pending = u32::try_from(n).unwrap_or(0),
                _ => {}
            }
        }
        last
    }

    /// Keeps it.
    ///
    /// # Errors
    /// What writing says; [`Error::NoMemory`].
    pub fn write(&self, volume: &dyn Volume) -> Result<(), Error> {
        let text = format(format_args!(
            "time={}\ndown={}\nup={}\ndeleted={}\nconflicts={}\nerrors={}\npending={}\n",
            self.time.max(0),
            self.down,
            self.up,
            self.deleted,
            self.conflicts,
            self.errors,
            self.pending
        ))?;
        replace_on(volume, LAST, text.as_bytes())
    }
}

const TRASH: &str = "trash.txt";

/// The card's paths of the files in the trash, the oldest first.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn trash(volume: &dyn Volume) -> Result<Vec<String>, Error> {
    let mut paths = Vec::new();
    match for_each_line_on(volume, TRASH, &mut |line| {
        if !line.is_empty() {
            push(&mut paths, copy(line)?)?;
        }
        Ok(ControlFlow::Continue(()))
    }) {
        Err(Error::NoMemory) => Err(Error::NoMemory),
        _ => Ok(paths),
    }
}

/// Keeps the list of the trash.
///
/// # Errors
/// What writing says; [`Error::NoMemory`].
pub fn write_trash(volume: &dyn Volume, paths: &[String]) -> Result<(), Error> {
    let mut text = String::new();
    for path in paths {
        text.try_reserve(path.len() + 1)
            .map_err(|_| Error::NoMemory)?;
        text.push_str(path);
        text.push('\n');
    }
    replace_on(volume, TRASH, text.as_bytes())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;
    use crate::test_volume::Mem;

    fn of(pair: usize) -> Of {
        Of::new(pair, "Notes", "http://h/dav/").unwrap()
    }

    fn entry(name: &str, n: u64) -> Kept {
        Kept {
            name: name.into(),
            local: Side { size: n, stamp: 5 },
            remote: Side {
                size: n + 1,
                stamp: u64::MAX,
            },
        }
    }

    #[test]
    fn a_state_is_kept_and_read_back_sorted_and_another_folders_file_is_none() {
        let volume = Mem::default();
        let (one, two) = (of(1), of(2));
        assert!(read(&volume, one, "a").unwrap().is_empty());
        let kept = [entry("b.md", 2), entry("a ü.md", 1), entry("B2.md", 3)];
        write(&volume, one, "a", &kept).unwrap();
        let back = read(&volume, one, "a").unwrap();
        // Sorted as the card tells names apart: without case.
        assert_eq!(
            back,
            [entry("a ü.md", 1), entry("b.md", 2), entry("B2.md", 3)]
        );
        // A file of another folder (a hash that met another) says nothing.
        let name = file(1, "a").unwrap();
        let bytes = volume.get(&name).unwrap();
        volume.write(&file(1, "b").unwrap(), &bytes).unwrap();
        assert!(read(&volume, one, "b").unwrap().is_empty());
        // The other pair has its own.
        assert!(read(&volume, two, "a").unwrap().is_empty());
        // A pair set to another folder of the card, or of the server, has none.
        for other in [
            Of::new(1, "Books", "http://h/dav/").unwrap(),
            Of::new(1, "Notes", "http://h/other/").unwrap(),
        ] {
            assert!(read(&volume, other, "a").unwrap().is_empty());
        }
    }

    #[test]
    fn a_long_state_is_written_in_pieces_and_a_cut_leaves_the_old_one() {
        let volume = Mem::default();
        let many: Vec<Kept> = (0..MOST)
            .map(|n| entry(&alloc::format!("{}{n:03}.md", "x".repeat(60)), n as u64))
            .collect();
        write(&volume, of(1), "", &many).unwrap();
        assert_eq!(read(&volume, of(1), "").unwrap().len(), MOST);
        assert!(*volume.longest.borrow() <= MAX_DATA);
        // A temp file a cut left is no state.
        let name = file(1, "").unwrap();
        volume
            .write(&concat(&[&name, TEMP]).unwrap(), b"dir=\nbroken")
            .unwrap();
        assert_eq!(read(&volume, of(1), "").unwrap().len(), MOST);
    }

    #[test]
    fn the_summary_and_the_trash_are_kept() {
        let volume = Mem::default();
        assert_eq!(Last::read(&volume), Last::default());
        let last = Last {
            time: 1_700_000_000,
            down: 3,
            up: 2,
            deleted: 1,
            conflicts: 4,
            errors: 5,
            pending: 6,
        };
        last.write(&volume).unwrap();
        assert_eq!(Last::read(&volume), last);
        assert!(trash(&volume).unwrap().is_empty());
        write_trash(&volume, &["x/1".into(), "x/2".into()]).unwrap();
        assert_eq!(trash(&volume).unwrap(), ["x/1", "x/2"]);
    }

    #[test]
    fn a_run_that_failed_keeps_the_time_of_the_last_sync() {
        let synced = Last {
            time: 1_700_000_000,
            ..Last::default()
        };
        let total = crate::native_sync::Totals {
            errors: 2,
            ..Default::default()
        };
        let failed = synced.ended(total, 3, None);
        assert_eq!(failed.time, 1_700_000_000);
        assert_eq!((failed.errors, failed.pending), (2, 3));
        assert_eq!(
            failed.ended(total, 0, Some(1_700_009_000)).time,
            1_700_009_000
        );
    }
}
