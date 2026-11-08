//! What can still be added to a menu: actions, settings and other menus.
use super::{Error, Host, MenuGraph, MenuQuery, PageCursor, Registry, String, Vec, copy};

/// `reg_menu_row.flags` bit 2: a plugin hides the row.
const HIDDEN: u32 = 1 << 2;

/// One entry that could go into a menu.
#[derive(Debug, PartialEq, Eq)]
pub struct Candidate {
    id: String,
    title: String,
    submenu: bool,
    hidden_by: Option<String>,
}
impl Candidate {
    /// Identity to add.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Host-translated name.
    pub fn title(&self) -> &str {
        &self.title
    }
    /// Whether it opens a menu of its own.
    pub fn is_menu(&self) -> bool {
        self.submenu
    }
    /// Display name of the plugin that took it out of this menu, if one did.
    pub fn hidden_by(&self) -> Option<&str> {
        self.hidden_by.as_deref()
    }
}

/// Notes that `by` (a plugin id) took `item` out of the menu, if it is a candidate.
fn mark_hidden(found: &mut [Candidate], item: &str, by: &str) -> Result<(), Error> {
    if let Some(candidate) = found.iter_mut().find(|c| c.id == item) {
        candidate.hidden_by = Some(copy(by)?);
    }
    Ok(())
}

/// Replaces the plugin id `id` by its display `name` where it is noted as
/// a hider and the name is known.
fn name_hider(found: &mut [Candidate], id: &str, name: &str) -> Result<(), Error> {
    if name.is_empty() {
        return Ok(());
    }
    for candidate in found.iter_mut() {
        if candidate.hidden_by.as_deref() == Some(id) {
            candidate.hidden_by = Some(copy(name)?);
        }
    }
    Ok(())
}

/// Lower-case order of names without allocating, then by identity.
fn by_name(a: &Candidate, b: &Candidate) -> core::cmp::Ordering {
    a.title
        .chars()
        .flat_map(char::to_lowercase)
        .cmp(b.title.chars().flat_map(char::to_lowercase))
        .then_with(|| a.title.cmp(&b.title))
}

impl MenuGraph {
    /// Everything not yet in `menu` that does not make a submenu circle,
    /// sorted by name: actions first, then settings, then menus; an id met
    /// twice is kept once.
    ///
    /// # Errors
    /// NotFound for an unknown menu; paging, generation and allocation errors.
    pub fn candidates(
        &self,
        registry: &Registry,
        host: &Host<'_>,
        menu: &str,
        input: &mut [u8],
        output: &mut [u8],
    ) -> Result<Vec<Candidate>, Error> {
        let at = self.index(menu).ok_or(Error::NotFound)?;
        let present = &self.menus[at].current;
        let mut found: Vec<Candidate> = Vec::new();
        let mut cursor = PageCursor::start();
        loop {
            let page = registry.actions(host, "", cursor, input, output)?;
            for row in page.rows() {
                self.keep(&mut found, present, menu, row.id(), row.title(), false)?;
            }
            let Some(next) = page.end().next_cursor() else {
                break;
            };
            cursor = next;
        }
        let mut cursor = PageCursor::start();
        loop {
            let page = registry.settings(host, "", cursor, input, output)?;
            for row in page.rows() {
                self.keep(&mut found, present, menu, row.key(), row.title(), false)?;
            }
            let Some(next) = page.end().next_cursor() else {
                break;
            };
            cursor = next;
        }
        for other in &self.menus {
            self.keep(&mut found, present, menu, &other.id, &other.title, true)?;
        }
        self.note_hidden(registry, host, menu, &mut found, input, output)?;
        paperpaw_ui_kit::mem::stable_sort_by(&mut found, by_name);
        Ok(found)
    }

    fn keep(
        &self,
        found: &mut Vec<Candidate>,
        present: &[String],
        menu: &str,
        id: &str,
        title: &str,
        submenu: bool,
    ) -> Result<(), Error> {
        if present.iter().any(|p| p == id)
            || found.iter().any(|c| c.id == id)
            || self.makes_circle(menu, id)?
        {
            return Ok(());
        }
        found.try_reserve(1).map_err(|_| Error::NoMemory)?;
        found.push(Candidate {
            id: copy(id)?,
            title: copy(title)?,
            submenu,
            hidden_by: None,
        });
        Ok(())
    }

    /// Says beside each candidate a plugin hid from `menu` which plugin did.
    fn note_hidden(
        &self,
        registry: &Registry,
        host: &Host<'_>,
        menu: &str,
        found: &mut [Candidate],
        input: &mut [u8],
        output: &mut [u8],
    ) -> Result<(), Error> {
        let mut cursor = PageCursor::start();
        loop {
            let page = registry.menu(host, menu, MenuQuery::All, cursor, input, output)?;
            for row in page.rows() {
                if row.flags() & HIDDEN == 0 {
                    continue;
                }
                mark_hidden(found, row.item(), row.by())?;
            }
            let Some(next) = page.end().next_cursor() else {
                break;
            };
            cursor = next;
        }
        // The plugin's name instead of its id, where it is known.
        if found.iter().all(|c| c.hidden_by.is_none()) {
            return Ok(());
        }
        let mut cursor = PageCursor::start();
        loop {
            let page = registry.plugins(host, cursor, input, output)?;
            for row in page.rows() {
                name_hider(found, row.id(), row.name())?;
            }
            let Some(next) = page.end().next_cursor() else {
                break;
            };
            cursor = next;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn candidate(id: &str) -> Candidate {
        Candidate {
            id: String::from(id),
            title: String::from(id),
            submenu: false,
            hidden_by: None,
        }
    }

    #[test]
    fn who_took_an_entry_out_is_noted_beside_it_by_name() {
        let mut found = [candidate("early.a"), candidate("early.b")];
        mark_hidden(&mut found, "early.b", "hider").unwrap();
        // Not a candidate: nothing noted.
        mark_hidden(&mut found, "early.zzz", "other").unwrap();
        assert_eq!(found[0].hidden_by(), None);
        assert_eq!(found[1].hidden_by(), Some("hider"));
        // The plugin's name replaces its id; an unknown name leaves the id.
        name_hider(&mut found, "hider", "").unwrap();
        assert_eq!(found[1].hidden_by(), Some("hider"));
        name_hider(&mut found, "other", "Other").unwrap();
        assert_eq!(found[1].hidden_by(), Some("hider"));
        name_hider(&mut found, "hider", "Hider").unwrap();
        assert_eq!(found[0].hidden_by(), None);
        assert_eq!(found[1].hidden_by(), Some("Hider"));
    }

    #[test]
    fn a_refused_allocation_is_reported_and_recovers() {
        let mut found = [candidate("early.b")];
        crate::fail_alloc::sweep(|| {
            found[0].hidden_by = None;
            mark_hidden(&mut found, "early.b", "hider")?;
            name_hider(&mut found, "hider", "Hider")?;
            Ok(found[0].hidden_by().map(str::len))
        });
    }
}
