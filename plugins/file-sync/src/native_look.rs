//! A look at the card alone, without the network: how many files of a pair
//! changed since the last sync (new, changed or gone), for the widget
//! *Pending changes*. Nothing is read from the server, so what changed there
//! is not counted.
use alloc::{string::String, vec::Vec};

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::mem::push;

use crate::native_link::{Entry, Link};
use crate::native_names as names;
use crate::native_state::{self as state, MOST, Of};
use crate::native_sync::{MAX_DEPTH, MOST_DIRS};

/// The changes of the card's folder `root` since the last sync of the pair `of`.
/// A folder with more files than a sync takes counts as none.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn pending(link: &mut dyn Link, of: Of, root: &str) -> Result<u32, Error> {
    let mut dirs: Vec<String> = Vec::new();
    push(&mut dirs, String::new())?;
    let (mut count, mut seen) = (0u32, 0usize);
    while let Some(rel) = dirs.pop() {
        seen += 1;
        if seen > MOST_DIRS {
            break;
        }
        let folder = if rel.is_empty() {
            paperpaw_ui_kit::text::copy(root)?
        } else {
            names::path(root, "", &rel)?
        };
        let mut local: Vec<Entry> = Vec::new();
        match link.list(&folder, &mut local, MOST + 1) {
            Ok(()) | Err(Error::NotFound) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => continue,
        }
        local.retain(|e| !names::left_out(&e.name));
        if local.len() > MOST {
            continue;
        }
        let kept = state::read(link.data(), of, &rel)?;
        for entry in &local {
            if entry.folder {
                let child = names::child(&rel, &entry.name)?;
                if child.split('/').count() <= MAX_DEPTH {
                    push(&mut dirs, child)?;
                }
                continue;
            }
            let found = kept
                .binary_search_by(|k| names::order(&k.name, &entry.name))
                .ok();
            let same = found
                .and_then(|at| kept.get(at))
                .is_some_and(|k| !entry.side.local_changed(k));
            count = count.saturating_add(u32::from(!same));
        }
        for k in &kept {
            let there = local
                .iter()
                .any(|e| !e.folder && names::same(&e.name, &k.name));
            count = count.saturating_add(u32::from(!there));
        }
    }
    Ok(count)
}
