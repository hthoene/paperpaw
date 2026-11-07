use super::{Error, MenuGraph, String, Vec, copy};

impl MenuGraph {
    /// Menu indices and display paths, in the original editor's depth-first order.
    /// Roots sort by lowercase title; cycles follow in registry order. Each menu
    /// appears once even when several parents contain it.
    ///
    /// # Errors
    /// Returns NoMemory if a title or traversal buffer cannot be allocated.
    pub fn display_order(&self) -> Result<Vec<(usize, String)>, Error> {
        let count = self.menus.len();
        let mut roots = Vec::new();
        roots.try_reserve(count).map_err(|_| Error::NoMemory)?;
        for (index, menu) in self.menus.iter().enumerate() {
            if !self
                .menus
                .iter()
                .any(|other| other.current.contains(&menu.id))
            {
                let mut key = String::new();
                for ch in menu.title.chars().flat_map(char::to_lowercase) {
                    key.try_reserve(ch.len_utf8())
                        .map_err(|_| Error::NoMemory)?;
                    key.push(ch);
                }
                roots.push((index, key));
            }
        }
        // The registry index preserves the old stable ordering for equal names.
        roots.sort_unstable_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        let mut out = Vec::new();
        out.try_reserve(count).map_err(|_| Error::NoMemory)?;
        let mut seen = Vec::new();
        seen.try_reserve_exact(count).map_err(|_| Error::NoMemory)?;
        seen.resize(count, false);
        let mut stack = Vec::new();
        stack.try_reserve(count).map_err(|_| Error::NoMemory)?;
        for root in roots.iter().map(|row| row.0).chain(0..count) {
            if seen[root] {
                continue;
            }
            seen[root] = true;
            out.push((root, copy(&self.menus[root].title)?));
            stack.push((root, 0, out.len() - 1));
            while let Some((parent, child, label)) = stack.last_mut() {
                let Some(id) = self.menus[*parent].current.get(*child) else {
                    stack.pop();
                    continue;
                };
                *child += 1;
                let Some(index) = self.index(id) else {
                    continue;
                };
                if seen[index] {
                    continue;
                }
                let prefix = &out[*label].1;
                let title = &self.menus[index].title;
                let len = prefix
                    .len()
                    .checked_add(" › ".len())
                    .and_then(|len| len.checked_add(title.len()))
                    .ok_or(Error::NoSpace)?;
                let mut name = String::new();
                name.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
                name.push_str(prefix);
                name.push_str(" › ");
                name.push_str(title);
                seen[index] = true;
                out.push((index, name));
                stack.push((index, 0, out.len() - 1));
            }
        }
        Ok(out)
    }
}
