//! Native menu reads and edits. Only typed SDK operations cross the boundary.
//!
//! A snapshot is replaced only after every page succeeds. Re-read it after a
//! registry-change event; the host is still authoritative when accepting edits.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    registry::{MenuQuery, PageCursor, Registry},
};

const PROTECTED: [&str; 2] = ["core.settings", "menu-editor.open"];

/// One menu's current and declared ordering, copied out of borrowed pages.
#[derive(Debug)]
pub struct Menu {
    id: String,
    title: String,
    current: Vec<String>,
    declared: Vec<String>,
}
impl Menu {
    /// Full menu identity.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Host-translated menu name.
    pub fn title(&self) -> &str {
        &self.title
    }
    /// Entries in the current ordering.
    pub fn entries(&self) -> &[String] {
        &self.current
    }
}

/// A consistent registry generation used for navigation and access checks.
#[derive(Debug)]
pub struct MenuGraph {
    menus: Vec<Menu>,
    generation: u32,
}
impl MenuGraph {
    /// Read every menu and both orderings, retaining no host-backed references.
    ///
    /// # Errors
    /// Propagates host, paging and allocation errors. A generation change
    /// refuses the whole snapshot with State; the caller keeps its old view.
    pub fn read(
        registry: &Registry,
        host: &Host<'_>,
        input: &mut [u8],
        output: &mut [u8],
    ) -> Result<Self, Error> {
        let mut result = Self {
            menus: Vec::new(),
            generation: 0,
        };
        let mut cursor = PageCursor::start();
        loop {
            let page = registry.menus(host, cursor, input, output)?;
            result.check_generation(page.end().generation())?;
            result
                .menus
                .try_reserve(page.len())
                .map_err(|_| Error::NoMemory)?;
            for row in page.rows() {
                result.menus.push(Menu {
                    id: copy(row.id())?,
                    title: copy(row.title())?,
                    current: Vec::new(),
                    declared: Vec::new(),
                });
            }
            let Some(next) = page.end().next_cursor() else {
                break;
            };
            cursor = next;
        }
        for menu in &mut result.menus {
            menu.current = read_entries(
                registry,
                host,
                &menu.id,
                MenuQuery::Current,
                result.generation,
                input,
                output,
            )?;
            menu.declared = read_entries(
                registry,
                host,
                &menu.id,
                MenuQuery::Declared,
                result.generation,
                input,
                output,
            )?;
        }
        Ok(result)
    }
    fn check_generation(&mut self, generation: u32) -> Result<(), Error> {
        if self.generation != 0 && self.generation != generation {
            return Err(Error::State);
        }
        self.generation = generation;
        Ok(())
    }
    /// Menus in the host's order.
    pub fn menus(&self) -> &[Menu] {
        &self.menus
    }
    /// Registry generation shared by all retained pages.
    pub fn generation(&self) -> u32 {
        self.generation
    }
    fn index(&self, id: &str) -> Option<usize> {
        self.menus.iter().position(|menu| menu.id == id)
    }
    /// Whether adding an entry creates a submenu cycle.
    ///
    /// # Errors
    /// Returns NoMemory if traversal scratch cannot be reserved.
    pub fn makes_circle(&self, menu: &str, entry: &str) -> Result<bool, Error> {
        if menu == entry {
            return Ok(true);
        }
        let Some(index) = self.index(entry) else {
            return Ok(false);
        };
        self.reaches(index, menu, false, None)
    }
    /// Check all roots, including menus caught in a pre-existing cycle.
    ///
    /// # Errors
    /// Returns NotFound for an unknown menu or NoMemory for traversal scratch.
    pub fn keeps_access(&self, menu: &str, proposed: &[&str]) -> Result<bool, Error> {
        let changed = self.index(menu).ok_or(Error::NotFound)?;
        for root in 0..self.menus.len() {
            for target in PROTECTED {
                if self.reaches(root, target, true, None)?
                    && !self.reaches(root, target, false, Some((changed, proposed)))?
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    // Iterative traversal: arbitrary submenu depth does not consume X4 stack.
    fn reaches(
        &self,
        root: usize,
        target: &str,
        declared: bool,
        changed: Option<(usize, &[&str])>,
    ) -> Result<bool, Error> {
        let mut seen = Vec::new();
        seen.try_reserve_exact(self.menus.len())
            .map_err(|_| Error::NoMemory)?;
        seen.resize(self.menus.len(), false);
        let mut pending = Vec::new();
        pending
            .try_reserve_exact(self.menus.len())
            .map_err(|_| Error::NoMemory)?;
        pending.push(root);
        seen[root] = true;
        while let Some(index) = pending.pop() {
            let menu = &self.menus[index];
            let visit = |id: &str, seen: &mut [bool], pending: &mut Vec<usize>| {
                if id == target {
                    return true;
                }
                if let Some(child) = self.index(id)
                    && !seen[child]
                {
                    seen[child] = true;
                    pending.push(child);
                }
                false
            };
            if let Some((changed, proposed)) = changed
                && changed == index
            {
                for id in proposed {
                    if visit(id, &mut seen, &mut pending) {
                        return Ok(true);
                    }
                }
            } else {
                let entries = if declared {
                    &menu.declared
                } else {
                    &menu.current
                };
                for id in entries {
                    if visit(id, &mut seen, &mut pending) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
    /// Submit an edited ordering only if Settings and this editor stay reachable.
    /// No local state changes before the host accepts the command.
    ///
    /// # Errors
    /// Returns Denied on loss of access, otherwise validation or host errors.
    pub fn replace(
        &self,
        commands: &mut CommandSender<'_, '_>,
        menu: &str,
        proposed: &[&str],
    ) -> Result<(), Error> {
        if !self.keeps_access(menu, proposed)? {
            return Err(Error::Denied);
        }
        commands.set_menu(menu, Some(proposed))
    }
}

fn copy(text: &str) -> Result<String, Error> {
    let mut result = String::new();
    result
        .try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    result.push_str(text);
    Ok(result)
}

#[allow(
    clippy::too_many_arguments,
    reason = "one typed query and caller-owned buffers"
)]
fn read_entries(
    registry: &Registry,
    host: &Host<'_>,
    menu: &str,
    query: MenuQuery,
    generation: u32,
    input: &mut [u8],
    output: &mut [u8],
) -> Result<Vec<String>, Error> {
    let mut result = Vec::new();
    let mut cursor = PageCursor::start();
    loop {
        let page = registry.menu(host, menu, query, cursor, input, output)?;
        if page.end().generation() != generation {
            return Err(Error::State);
        }
        result
            .try_reserve(page.len())
            .map_err(|_| Error::NoMemory)?;
        for row in page.rows() {
            result.push(copy(row.item())?);
        }
        let Some(next) = page.end().next_cursor() else {
            break;
        };
        cursor = next;
    }
    Ok(result)
}

mod edits;
mod navigation;
mod render;
pub use navigation::ScreenState;
pub use render::ListRenderer;

#[cfg(test)]
mod tests;
