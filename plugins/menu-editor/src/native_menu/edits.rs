//! Edits retain the old view until the host accepts and publishes the new order.
use super::*;

impl MenuGraph {
    fn entry_refs(&self, menu: &str, extra: usize) -> Result<Vec<&str>, Error> {
        let index = self.index(menu).ok_or(Error::NotFound)?;
        let entries = &self.menus[index].current;
        let capacity = entries.len().checked_add(extra).ok_or(Error::NoMemory)?;
        let mut refs = Vec::new();
        refs.try_reserve_exact(capacity)
            .map_err(|_| Error::NoMemory)?;
        refs.extend(entries.iter().map(String::as_str));
        Ok(refs)
    }

    /// Append an entry without making a submenu cycle or losing access.
    ///
    /// # Errors
    /// Returns Invalid for a duplicate, Denied for a cycle, or the host error.
    pub fn add(
        &self,
        commands: &mut CommandSender<'_, '_>,
        menu: &str,
        entry: &str,
    ) -> Result<(), Error> {
        let mut entries = self.entry_refs(menu, 1)?;
        if entries.contains(&entry) {
            return Err(Error::Invalid);
        }
        if self.makes_circle(menu, entry)? {
            return Err(Error::Denied);
        }
        entries.push(entry);
        self.replace(commands, menu, &entries)
    }

    /// Remove one row; the returned focus remains beside it.
    ///
    /// # Errors
    /// Returns Invalid for a stale index, Denied for loss of access,
    /// or an allocation, encoding or host error. No local state is changed.
    pub fn remove(
        &self,
        commands: &mut CommandSender<'_, '_>,
        menu: &str,
        index: usize,
    ) -> Result<usize, Error> {
        let mut entries = self.entry_refs(menu, 0)?;
        if index >= entries.len() {
            return Err(Error::Invalid);
        }
        entries.remove(index);
        self.replace(commands, menu, &entries)?;
        Ok(index.saturating_sub(1))
    }

    /// Move a dragged row, preserving the order of every other entry.
    ///
    /// # Errors
    /// Returns Invalid for stale indices or an allocation, encoding or host error.
    pub fn move_entry(
        &self,
        commands: &mut CommandSender<'_, '_>,
        menu: &str,
        from: usize,
        to: usize,
    ) -> Result<(), Error> {
        let mut entries = self.entry_refs(menu, 0)?;
        reorder(&mut entries, from, to)?;
        self.replace(commands, menu, &entries)
    }

    /// Restore the menu's declared order, rather than storing a copy of it.
    ///
    /// # Errors
    /// Returns NotFound for an unknown menu or the host's refusal.
    pub fn reset(&self, commands: &mut CommandSender<'_, '_>, menu: &str) -> Result<(), Error> {
        self.index(menu).ok_or(Error::NotFound)?;
        commands.set_menu(menu, None)
    }
}

fn reorder<T>(items: &mut [T], from: usize, to: usize) -> Result<(), Error> {
    if from >= items.len() || to >= items.len() {
        return Err(Error::Invalid);
    }
    if from < to {
        items[from..=to].rotate_left(1);
    } else if from > to {
        items[to..=from].rotate_right(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn dragging_matches_remove_and_insert(mut items in proptest::collection::vec(any::<u8>(), 1..65),
            from in any::<usize>(), to in any::<usize>()) {
            let (from, to) = (from % items.len(), to % items.len());
            let mut expected = items.clone();
            let moved = expected.remove(from);
            expected.insert(to, moved);
            prop_assert_eq!(reorder(&mut items, from, to), Ok(()));
            prop_assert_eq!(items, expected);
        }
    }
    #[test]
    fn stale_indices_leave_rows_unchanged() {
        let mut items = [1, 2, 3];
        assert_eq!(reorder(&mut items, 3, 0), Err(Error::Invalid));
        assert_eq!(reorder(&mut items, 0, 3), Err(Error::Invalid));
        assert_eq!(items, [1, 2, 3]);
    }
}
