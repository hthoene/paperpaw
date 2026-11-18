//! The tasks' file in the plugin's data folder: read a line at a time, and
//! written so that a power cut leaves the old file or the new one whole,
//! never a part. A file of one storage call is written in that call (the
//! host replaces it whole); a longer one is written beside the old under
//! another name, flushed, and renamed over it.
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::native::{Error, storage::MAX_DATA};
use paperpaw_ui_kit::files::{Volume, for_each_line_on};

use crate::model::{HEADER, Store};

/// The file of the lists and tasks.
pub const FILE: &str = "tasks.txt";
/// The name a new file is written under before it takes the old one's place.
pub const TEMP: &str = "tasks.txt.new";
/// Where a file that ends before its end is kept, should it ever be found.
pub const DAMAGED: &str = "tasks.damaged.txt";

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
    /// The lists and tasks (none where there is no file).
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
        if store.read_line(line)?.is_some() {
            whole = true;
            return Ok(ControlFlow::Break(()));
        }
        Ok(ControlFlow::Continue(()))
    })?;
    if !header {
        return Ok(Loaded::Unreadable);
    }
    store.tidy();
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
    use crate::model::Seed;
    use crate::test_volume::Fake;
    use alloc::{format, string::String};

    fn store(tasks: usize, mark: &str) -> Store {
        let mut store = Store::default();
        store.add_list("Home", "").unwrap();
        store.add_list("Work", "/dav/work/").unwrap();
        for n in 0..tasks {
            let title = format!("{mark} task number {n} with a title of some length");
            store
                .add_task(1 + (n % 2) as u8, &title, Some(20_000), Seed::default())
                .unwrap();
        }
        store
    }

    fn text_of(store: &Store) -> String {
        let mut text = String::new();
        store
            .write(&mut |piece| {
                text.push_str(piece);
                Ok(())
            })
            .unwrap();
        text
    }

    fn loaded(volume: &Fake) -> Store {
        match load(volume).unwrap() {
            Loaded::Store(store) => store,
            Loaded::Unreadable => panic!("unreadable"),
        }
    }

    #[test]
    fn no_file_is_no_tasks_and_a_saved_file_reads_back() {
        let volume = Fake::default();
        assert!(loaded(&volume).tasks.is_empty());
        for tasks in [0, 3, 150] {
            let store = store(tasks, "a");
            save(&volume, &store).unwrap();
            assert_eq!(volume.text(FILE), text_of(&store));
            assert_eq!(text_of(&loaded(&volume)), text_of(&store));
            assert_eq!(volume.get(TEMP), None);
        }
    }

    #[test]
    fn a_power_cut_at_any_step_leaves_the_old_tasks_or_the_new_whole() {
        // A file of one storage call and one of several, each way round.
        for (old_tasks, new_tasks) in [(3, 4), (3, 150), (150, 151), (150, 2)] {
            let (old, new) = (store(old_tasks, "old"), store(new_tasks, "new"));
            let volume = Fake::default();
            save(&volume, &old).unwrap();
            volume.restart();
            save(&volume, &new).unwrap();
            let steps = volume.ops();
            assert!(steps >= 1);
            for cut in 1..=steps {
                let volume = Fake::default();
                save(&volume, &old).unwrap();
                volume.cut_at(cut);
                let saved = save(&volume, &new);
                volume.restart();
                let found = text_of(&loaded(&volume));
                // Said to be saved only if it is.
                assert!(saved == Err(Error::Io) || found == text_of(&new));
                assert!(
                    found == text_of(&old) || found == text_of(&new),
                    "{old_tasks} -> {new_tasks}, cut at step {cut} of {steps}: a part"
                );
                assert_eq!(volume.text(FILE), found, "the file itself is whole");
                assert_eq!(volume.get(DAMAGED), None, "nothing looked damaged");
                // What a cut left under the other name is no trouble later.
                save(&volume, &new).unwrap();
                assert_eq!(text_of(&loaded(&volume)), text_of(&new));
                assert_eq!(volume.get(TEMP), None);
            }
        }
    }

    #[test]
    fn a_file_of_another_kind_is_left_alone() {
        let volume = Fake::default();
        volume.put(FILE, b"tasks 2\nsomething newer\n");
        assert!(matches!(load(&volume).unwrap(), Loaded::Unreadable));
        assert_eq!(volume.text(FILE), "tasks 2\nsomething newer\n");
    }

    #[test]
    fn a_file_that_ends_early_is_used_as_far_as_it_goes_and_kept_aside() {
        let store = store(5, "a");
        let text = text_of(&store);
        let cut = text.rfind("\nT\t").unwrap();
        let volume = Fake::default();
        volume.put(FILE, &text.as_bytes()[..cut + 1]);
        let found = loaded(&volume);
        assert_eq!(found.lists.len(), 2);
        assert_eq!(found.tasks.len(), 4);
        assert_eq!(volume.get(DAMAGED).unwrap(), &text.as_bytes()[..cut + 1]);
    }

    #[test]
    fn a_card_that_cannot_be_read_is_an_error_not_an_empty_list() {
        struct Broken;
        impl Volume for Broken {
            fn size(&self, _: &str) -> Result<u64, Error> {
                Ok(100)
            }
            fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, Error> {
                Err(Error::Io)
            }
            fn write(&self, _: &str, _: &[u8]) -> Result<(), Error> {
                panic!("nothing is written")
            }
            fn append(&self, _: &str, _: &[u8]) -> Result<(), Error> {
                panic!("nothing is written")
            }
            fn rename(&self, _: &str, _: &str) -> Result<(), Error> {
                panic!("nothing is renamed")
            }
            fn remove(&self, _: &str) -> Result<(), Error> {
                panic!("nothing is removed")
            }
            fn sync(&self) -> Result<(), Error> {
                Ok(())
            }
        }
        assert_eq!(load(&Broken).unwrap_err(), Error::Io);
    }
}
