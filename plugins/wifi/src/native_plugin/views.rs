//! What the screens keep: the radio as it was read, the Wi-Fi page, the
//! networks in reach, and where the networks read go.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{Item, ItemList, RowItem, mem::push, text::copy};

use crate::native_model::{Network, PageRow};
use crate::native_policy::{Net as Reach, Radio};

/// Where the radio is and what it is in.
#[derive(Debug, Default)]
pub(super) struct Info {
    /// The device has a radio.
    pub(super) present: bool,
    pub(super) radio: Radio,
    pub(super) scanning: bool,
    /// The radio is busy (joining, scanning).
    pub(super) busy: bool,
    /// Work waits for the network.
    pub(super) wanted: bool,
    /// Phones that joined the device's own network.
    pub(super) guests: u32,
}

/// [`Info`] with the texts a few readers need, copied only in the states that
/// have them (every other reader borrows nothing and copies nothing).
#[derive(Debug, Default)]
pub(super) struct Named {
    pub(super) info: Info,
    /// The network joined or being joined (empty in any other state).
    pub(super) ssid: String,
    /// The address online (empty in any other state).
    pub(super) address: String,
}

impl core::ops::Deref for Named {
    type Target = Info;

    fn deref(&self) -> &Info {
        &self.info
    }
}

#[derive(Debug)]
pub(super) struct Page {
    pub(super) rows: Vec<PageRow>,
    pub(super) list: ItemList,
}

impl Page {
    /// No rows: what a page is while it is read again (the old rows are
    /// freed before the new are made).
    pub(super) fn empty() -> Self {
        Self {
            rows: Vec::new(),
            list: ItemList::new(Vec::new()),
        }
    }
}

/// What a row of the networks remembers of its network besides the name,
/// which the row itself holds (the one copy of it): the row is the
/// network, and what choosing it needs is made from the two.
#[derive(Debug, Clone, Copy)]
pub(super) struct Mark {
    pub(super) signal: i32,
    pub(super) secured: bool,
}

/// The networks in reach: a row for each, then the row that looks again.
#[derive(Debug)]
pub(super) struct Networks {
    /// Of the rows of `list`, the first of each network, in order.
    pub(super) marks: Vec<Mark>,
    pub(super) list: ItemList,
}

impl Networks {
    pub(super) fn empty() -> Self {
        Self {
            marks: Vec::new(),
            list: ItemList::new(Vec::new()),
        }
    }

    /// The name of the network of row `index`; `None` for the last row.
    pub(super) fn ssid(&self, index: usize) -> Option<&str> {
        self.marks.get(index)?;
        match self.list.items().get(index)? {
            Item::Row(row) => Some(&row.title),
            Item::Heading(_) => None,
        }
    }

    /// A copy of the network of row `index` (the choice reads the services,
    /// which borrow the plugin); `None` for the last row.
    pub(super) fn network(&self, index: usize) -> Result<Option<Network>, Error> {
        let (Some(mark), Some(ssid)) = (self.marks.get(index), self.ssid(index)) else {
            return Ok(None);
        };
        Ok(Some(Network {
            ssid: copy(ssid)?,
            signal: mark.signal,
            secured: mark.secured,
        }))
    }
}

/// Where the networks in reach go as they are read: kept, or only counted.
pub(super) trait Found {
    /// What is read so far is dropped: the list is read again.
    fn restart(&mut self);
    fn len(&self) -> usize;
    /// Room for `more` networks, exactly.
    fn reserve(&mut self, more: usize) -> Result<(), Error>;
    fn add(&mut self, ssid: &str, signal: i32, secured: bool) -> Result<(), Error>;
}

impl Found for Vec<Network> {
    fn restart(&mut self) {
        self.clear();
    }

    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn reserve(&mut self, more: usize) -> Result<(), Error> {
        self.try_reserve_exact(more).map_err(|_| Error::NoMemory)
    }

    fn add(&mut self, ssid: &str, signal: i32, secured: bool) -> Result<(), Error> {
        push(
            self,
            Network {
                ssid: copy(ssid)?,
                signal,
                secured,
            },
        )
    }
}

impl Found for Vec<Reach> {
    fn restart(&mut self) {
        self.clear();
    }

    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn reserve(&mut self, more: usize) -> Result<(), Error> {
        self.try_reserve_exact(more).map_err(|_| Error::NoMemory)
    }

    fn add(&mut self, ssid: &str, _: i32, secured: bool) -> Result<(), Error> {
        push(
            self,
            Reach {
                ssid: copy(ssid)?,
                secured,
            },
        )
    }
}

/// The networks only counted: no name is kept (what a row that says how many
/// there are needs).
pub(super) struct Count(pub(super) usize);

impl Found for Count {
    fn restart(&mut self) {
        self.0 = 0;
    }

    fn len(&self) -> usize {
        self.0
    }

    fn reserve(&mut self, _: usize) -> Result<(), Error> {
        Ok(())
    }

    fn add(&mut self, _: &str, _: i32, _: bool) -> Result<(), Error> {
        self.0 = self.0.saturating_add(1);
        Ok(())
    }
}

/// A row of a name and its note, the texts moved in.
pub(super) fn row_item(title: String, note: String) -> Item {
    Item::Row(RowItem {
        title,
        note,
        ..RowItem::default()
    })
}
