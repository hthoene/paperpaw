//! The calendar's file in the plugin's data folder: read a line at a time, and
//! written so that a power cut leaves the old file or the new one whole,
//! never a part. A file of one storage call is written in that call (the
//! host replaces it whole); a longer one is written beside the old under
//! another name, flushed, and renamed over it.
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::native::{Error, storage::MAX_DATA};
use paperpaw_ui_kit::files::{Volume, for_each_line_on};

use crate::model::{HEADER, Store};

/// The file of the sources and events.
pub const FILE: &str = "calendar.txt";
/// The name a new file is written under before it takes the old one's place.
pub const TEMP: &str = "calendar.txt.new";
/// Where a file that ends before its end is kept, should it ever be found.
pub const DAMAGED: &str = "calendar.damaged.txt";

/// Writes a file in pieces of one storage call each, from one buffer.
pub struct Writer<'a> {
    volume: &'a dyn Volume,
    name: &'a str,
    buffer: Vec<u8>,
    flushed: bool,
}

impl core::fmt::Debug for Writer<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Writer").field("name", &self.name).finish()
    }
}

impl<'a> Writer<'a> {
    /// A writer of the file `name`.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(volume: &'a dyn Volume, name: &'a str) -> Result<Self, Error> {
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(MAX_DATA)
            .map_err(|_| Error::NoMemory)?;
        Ok(Self {
            volume,
            name,
            buffer,
            flushed: false,
        })
    }

    fn flush(&mut self) -> Result<(), Error> {
        if self.flushed {
            self.volume.append(self.name, &self.buffer)?;
        } else {
            self.volume.write(self.name, &self.buffer)?;
        }
        self.flushed = true;
        self.buffer.clear();
        Ok(())
    }

    /// Appends `bytes` to the file.
    ///
    /// # Errors
    /// What the card says.
    pub fn put(&mut self, mut bytes: &[u8]) -> Result<(), Error> {
        while !bytes.is_empty() {
            if self.buffer.len() >= MAX_DATA {
                self.flush()?;
            }
            let room = MAX_DATA - self.buffer.len();
            let (now, later) = bytes.split_at(room.min(bytes.len()));
            // Within what was reserved: nothing is allocated.
            self.buffer.extend_from_slice(now);
            bytes = later;
        }
        Ok(())
    }

    /// Ends the file under its name.
    ///
    /// # Errors
    /// What the card says.
    pub fn end(mut self) -> Result<(), Error> {
        if !self.flushed || !self.buffer.is_empty() {
            self.flush()?;
        }
        Ok(())
    }

    /// Ends the file and puts it in the place of `target`, whole or not at
    /// all.
    ///
    /// # Errors
    /// What the card says; `target` is as it was then.
    pub fn replace(mut self, target: &str) -> Result<(), Error> {
        if !self.flushed {
            // One call: the host replaces the file whole.
            self.volume.write(target, &self.buffer)?;
            // A file a cut left under the other name is of no use now.
            let _ = self.volume.remove(self.name);
            return Ok(());
        }
        let done = (|| {
            if !self.buffer.is_empty() {
                self.flush()?;
            }
            self.volume.sync()?;
            self.volume.rename(self.name, target)
        })();
        if done.is_err() {
            let _ = self.volume.remove(self.name);
        }
        done
    }
}

/// What the file gave.
#[derive(Debug)]
pub enum Loaded {
    /// The sources and events (none where there is no file).
    Store(Store),
    /// A file that is not this plugin's, or of a later version: it is left
    /// alone and nothing is written over it.
    Unreadable,
}

/// Reads the file.
///
/// # Errors
/// `NoMemory`, or what the card says (the file is then not written over).
pub fn load(volume: &dyn Volume) -> Result<Loaded, Error> {
    match volume.size(FILE) {
        Ok(0) | Err(Error::NotFound) => return Ok(Loaded::Store(Store::default())),
        Ok(_) => {}
        Err(error) => return Err(error),
    }
    let mut store = Store::default();
    let (mut lines, mut header, mut whole) = (0usize, false, false);
    for_each_line_on(volume, FILE, &mut |line| {
        lines += 1;
        if lines == 1 {
            header = line == HEADER;
            return Ok(if header {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            });
        }
        if let Some(agrees) = store.read_line(line)? {
            whole = agrees;
            return Ok(ControlFlow::Break(()));
        }
        Ok(ControlFlow::Continue(()))
    })?;
    if !header {
        return Ok(Loaded::Unreadable);
    }
    if !whole {
        // It ends before its end, which the plugin's own writing never
        // leaves: what was read is used, and the file kept beside the new.
        let _ = volume.remove(DAMAGED);
        let _ = volume.rename(FILE, DAMAGED);
    }
    Ok(Loaded::Store(store))
}

/// Writes the file: after a power cut at any moment the old file or the new
/// one is there, whole.
///
/// # Errors
/// `NoMemory`, or what the card says; the old file is as it was then.
pub fn save(volume: &dyn Volume, store: &Store) -> Result<(), Error> {
    let mut writer = Writer::new(volume, TEMP)?;
    match store.write(&mut |text| writer.put(text.as_bytes())) {
        Ok(()) => writer.replace(FILE),
        Err(error) => {
            let _ = volume.remove(TEMP);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;
    use crate::model::{Entry, Kind, Mark, NO_TIME};
    use crate::test_volume::Fake;
    use alloc::{format, string::String, vec::Vec};

    fn store(events: usize, mark: &str) -> Store {
        let mut store = Store::default();
        store
            .add_source(Kind::Ics, "Holidays", "https://x.example/h.ics", "")
            .unwrap();
        for n in 0..events {
            store
                .add(Entry {
                    src: 1,
                    mark: Mark::Event,
                    day: 20_800 + n as i32,
                    secs: NO_TIME,
                    span: 1,
                    known: false,
                    uid: n as u32,
                    cal: crate::model::WHOLE_SOURCE,
                    over: None,
                    rule: String::new(),
                    skip: Vec::new(),
                    title: format!("{mark} event number {n} with a title of some length"),
                    place: String::new(),
                })
                .unwrap();
        }
        store
    }

    fn titles(store: &Store) -> Vec<String> {
        store.entries.iter().map(|e| e.title.clone()).collect()
    }

    #[test]
    fn a_saved_file_reads_back() {
        let volume = Fake::default();
        let first = store(40, "old");
        save(&volume, &first).unwrap();
        let Loaded::Store(back) = load(&volume).unwrap() else {
            panic!("unreadable");
        };
        assert_eq!(titles(&back), titles(&first));
        assert_eq!(back.sources, first.sources);
    }

    #[test]
    fn a_power_cut_at_every_call_leaves_the_old_file_or_the_new_one_whole() {
        let volume = Fake::default();
        let old = store(60, "old");
        let new = store(70, "new");
        save(&volume, &old).unwrap();
        let calls = {
            volume.restart();
            save(&volume, &new).unwrap();
            volume.ops()
        };
        assert!(calls > 2, "a file of more than one call");
        for cut in 1..=calls {
            let volume = Fake::default();
            save(&volume, &old).unwrap();
            volume.cut_at(cut);
            let result = save(&volume, &new);
            volume.restart();
            let Loaded::Store(back) = load(&volume).unwrap() else {
                panic!("unreadable at {cut}");
            };
            let read = titles(&back);
            assert!(
                read == titles(&old) || read == titles(&new),
                "cut at {cut} ({result:?}) left {} events",
                read.len()
            );
        }
    }

    #[test]
    fn a_file_that_is_not_ours_is_left_alone() {
        let volume = Fake::default();
        volume.put(FILE, b"something else\nentirely\n");
        assert!(matches!(load(&volume).unwrap(), Loaded::Unreadable));
        assert_eq!(volume.text(FILE), "something else\nentirely\n");
    }
}
