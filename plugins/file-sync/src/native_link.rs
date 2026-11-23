//! What the sync needs of the world: the card's folders (the files service on
//! the user's files), the plugin's data folder and the network. The sync is a
//! state machine over [`Link`]: a request starts and its end is told to the
//! sync ([`crate::native_sync::Sync::done`]); the tests run it against a fake
//! card and a fake server.
use alloc::{string::String, vec::Vec};

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{files::Volume, mem::push, text::copy};

use crate::native_names::left_out;
use crate::native_plan::Side;

/// The token of the one request that is under way.
pub const TOKEN: u32 = 1;

/// One entry of a folder of the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub folder: bool,
    pub side: Side,
}

/// The card, the data folder and the server.
pub trait Link {
    /// The plugin's data folder.
    fn data(&self) -> &dyn Volume;
    /// The entries of the card's folder `folder` into `out`, at most `most`
    /// of them, without the names a sync leaves out ([`take`]); `true` where
    /// the folder holds more.
    ///
    /// # Errors
    /// [`Error::NotFound`] for a folder that is not there; the host's refusal.
    fn list(&mut self, folder: &str, out: &mut Vec<Entry>, most: usize) -> Result<bool, Error>;
    /// A file of the card; `None` where there is none.
    ///
    /// # Errors
    /// The host's refusal.
    fn stat(&mut self, path: &str) -> Result<Option<Side>, Error>;
    /// Makes a folder of the card and its parents.
    ///
    /// # Errors
    /// The host's refusal.
    fn make_dir(&mut self, path: &str) -> Result<(), Error>;
    /// Moves a file of the card; an existing target is never replaced.
    ///
    /// # Errors
    /// The host's refusal; [`Error::State`] where `to` exists.
    fn rename(&mut self, from: &str, to: &str) -> Result<(), Error>;
    /// Moves the file `from` of the card to `to` in the data folder (the
    /// trash); the data folder's `trash` is made where it is not there.
    ///
    /// # Errors
    /// The host's refusal; [`Error::State`] where `to` exists.
    fn trash(&mut self, from: &str, to: &str) -> Result<(), Error>;
    /// Removes a file of the card.
    ///
    /// # Errors
    /// The host's refusal.
    fn remove(&mut self, path: &str) -> Result<(), Error>;
    /// Starts listing the folder at `url` (saved to the listing file of the
    /// data folder).
    ///
    /// # Errors
    /// The host's refusal.
    fn propfind(&mut self, url: &str) -> Result<(), Error>;
    /// Starts fetching `url` to the file `path` of the card (at most `max`
    /// bytes); the host writes it whole or not at all.
    ///
    /// # Errors
    /// The host's refusal.
    fn get(&mut self, url: &str, path: &str, max: u64) -> Result<(), Error>;
    /// Starts sending the file `path` of the card to `url`.
    ///
    /// # Errors
    /// The host's refusal.
    fn put(&mut self, url: &str, path: &str) -> Result<(), Error>;
    /// Starts deleting `url`.
    ///
    /// # Errors
    /// The host's refusal.
    fn delete(&mut self, url: &str) -> Result<(), Error>;
    /// Starts making the folder at `url`.
    ///
    /// # Errors
    /// The host's refusal.
    fn mkcol(&mut self, url: &str) -> Result<(), Error>;
    /// Cancels the request under way.
    fn cancel(&mut self);
}

/// Takes the entries of one page of a card's folder (name, folder or not,
/// size and time) into `out`, at most `most` of them. A name a sync leaves out
/// ([`left_out`]) takes no room: counted, the hidden names of a folder would
/// crowd out the files it syncs, and those would be taken as deleted.
/// `true` where an entry was left over: the folder holds more than `most`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn take<'a>(
    rows: impl IntoIterator<Item = (&'a str, bool, Side)>,
    out: &mut Vec<Entry>,
    most: usize,
) -> Result<bool, Error> {
    for (name, folder, side) in rows {
        if left_out(name) {
            continue;
        }
        if out.len() >= most {
            return Ok(true);
        }
        push(
            out,
            Entry {
                name: copy(name)?,
                folder,
                side,
            },
        )?;
    }
    Ok(false)
}
