//! What the device has installed and what is going on with the installs, as the
//! screens read it (`native_screens::Installs`), with no call to the host of its
//! own: [`Snapshot`] is read once from the registry and the record of what the
//! store installed (`.paperpaw/plugin-origin.txt`), when a screen opens or comes
//! on top, and dropped when the last screen closes; [`Activity`] is what the
//! plugin knows of the downloads, the installs and the queue. [`HostInstalls`]
//! puts the two together.
//!
//! **Memory (#248):** nothing is read at the start; a snapshot is one row of two
//! short texts a plugin (about seventy plugins), reserved exactly from the
//! registry's count; the registry is read through a 4 KiB reply block and a
//! 256-byte request on the stack, both freed with the read.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::Storage;
use paperpaw_sdk::native::{
    Error, Host,
    registry::{PageCursor, Registry, RegistryPluginKind},
};
use paperpaw_ui_kit::{
    mem::{SERVICE_BUFFER, buffer, push},
    text::copy,
};

use crate::native_install::Installs as Record;
use crate::native_page::{Installed, Kind, Origin, Trouble};
use crate::native_queue::Queue;
use crate::native_screens::{Installs, Step};

/// Registry pages read at most (the plugins installed here).
const MOST_PAGES: usize = 16;
/// The most rows reserved at once from the registry's total.
const MOST_ROWS: usize = 256;
/// The most a registry query takes (`max_input` of the query ops).
const REQUEST_BYTES: usize = 256;
/// The registry's flag of a plugin that is switched on.
const ENABLED: u32 = 1;
/// The registry's flag of a plugin whose change waits for a restart.
const RESTART_NEEDED: u32 = 2;
/// The host itself, the first row of the plugins.
const HOST: &str = "core";

/// A plugin the device has.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Row {
    pub id: String,
    pub kind: Kind,
    pub enabled: bool,
    pub restart_needed: bool,
    /// The version installed; a code plugin's update that waits for the
    /// restart counts.
    pub version: String,
    /// The version as the store named it where it installed it.
    pub release: Option<String>,
    pub origin: Origin,
    pub trouble: Option<Trouble>,
}

impl Row {
    /// A row as the registry says it, its origin not yet looked up.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(
        id: &str,
        kind: Kind,
        enabled: bool,
        restart_needed: bool,
        version: &str,
        trouble: Option<Trouble>,
    ) -> Result<Self, Error> {
        Ok(Self {
            id: copy(id)?,
            kind,
            enabled,
            restart_needed,
            version: copy(version)?,
            release: None,
            origin: Origin::Local,
            trouble,
        })
    }
}

/// The plugins the device has, sorted by id, with who put them there.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Snapshot {
    rows: Vec<Row>,
}

impl Snapshot {
    /// Reads the plugins from the registry and the record of the store's
    /// installs from `storage`.
    ///
    /// # Errors
    /// `NoMemory`, the host's refusals; `NoSpace` for more plugins than
    /// `MOST_PAGES` pages hold.
    pub fn read(host: &Host<'_>, storage: &dyn Storage) -> Result<Self, Error> {
        let mut out = buffer(SERVICE_BUFFER)?;
        let rows = Self::read_rows(host, &mut out)?;
        drop(out);
        Self::settle(rows, storage)
    }

    fn read_rows(host: &Host<'_>, out: &mut [u8]) -> Result<Vec<Row>, Error> {
        let registry = Registry::open(host)?;
        let result = Self::pages(host, &registry, out);
        registry.release(host).ok();
        result
    }

    fn pages(host: &Host<'_>, registry: &Registry, out: &mut [u8]) -> Result<Vec<Row>, Error> {
        let mut input = [0; REQUEST_BYTES];
        let mut rows: Vec<Row> = Vec::new();
        'again: for _ in 0..2 {
            rows.clear();
            let mut cursor = PageCursor::start();
            for _ in 0..MOST_PAGES {
                let page = match registry.plugins(host, cursor, &mut input, out) {
                    Ok(page) => page,
                    // The list changed under the reading: start over.
                    Err(Error::State) => continue 'again,
                    Err(error) => return Err(error),
                };
                // The registry says how many rows there are: reserve them once.
                let wanted = usize::try_from(page.end().total()).unwrap_or(MOST_ROWS);
                rows.try_reserve_exact(wanted.min(MOST_ROWS).saturating_sub(rows.len()))
                    .map_err(|_| Error::NoMemory)?;
                for plugin in page.rows() {
                    if plugin.id() == HOST {
                        continue;
                    }
                    let kind = match plugin.kind() {
                        RegistryPluginKind::Code => Kind::Code,
                        RegistryPluginKind::Data => Kind::Data,
                        _ => Kind::BuiltIn,
                    };
                    let flags = plugin.flags();
                    let waiting = plugin.update();
                    let version = if waiting.is_empty() {
                        plugin.version()
                    } else {
                        waiting
                    };
                    push(
                        &mut rows,
                        Row::new(
                            plugin.id(),
                            kind,
                            flags & ENABLED != 0,
                            flags & RESTART_NEEDED != 0 || !waiting.is_empty(),
                            version,
                            trouble_of(plugin.problem()),
                        )?,
                    )?;
                }
                match page.end().next_cursor() {
                    Some(next) => cursor = next,
                    None => return Ok(rows),
                }
            }
            return Err(Error::NoSpace);
        }
        Err(Error::State)
    }

    /// The rows sorted, and each looked up in the record of the store's
    /// installs. Where no record was ever kept, the plugins installed (those
    /// built in left out) are taken for the store's, once, as interface 6 did.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn settle(mut rows: Vec<Row>, storage: &dyn Storage) -> Result<Self, Error> {
        rows.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        // A record that cannot be kept is not the end of the list.
        if let Err(Error::NoMemory) = Record::adopt(
            storage,
            rows.iter()
                .filter(|row| row.kind != Kind::BuiltIn)
                .map(|row| (row.id.as_str(), row.version.as_str())),
        ) {
            return Err(Error::NoMemory);
        }
        let record = Record::read(storage)?;
        for row in &mut rows {
            if row.kind == Kind::BuiltIn || !record.of(&row.id, &row.version) {
                continue;
            }
            row.origin = Origin::Store;
            row.release = record
                .release(&row.id, &row.version)
                .map(copy)
                .transpose()?;
        }
        Ok(Self { rows })
    }

    /// The plugins, by id.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// What the device has of plugin `id`.
    #[must_use]
    pub fn have(&self, id: &str) -> Option<Installed<'_>> {
        let at = self
            .rows
            .binary_search_by(|row| row.id.as_str().cmp(id))
            .ok()?;
        let row = self.rows.get(at)?;
        Some(Installed {
            kind: row.kind,
            enabled: row.enabled,
            restart_needed: row.restart_needed,
            version: &row.version,
            origin: row.origin,
            release: row.release.as_deref(),
            trouble: row.trouble,
        })
    }
}

/// What the wire number of a plugin's problem says for the page: 1 and 10 are a
/// build for another PaperPaw (`registry.md`: `OtherBuild`, an older interface).
const fn trouble_of(problem: u32) -> Option<Trouble> {
    match problem {
        0 => None,
        1 | 10 => Some(Trouble::OtherBuild),
        _ => Some(Trouble::Other),
    }
}

/// What the plugin knows of the downloads, the installs and the queue.
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct Activity {
    /// How the install of each plugin stands; a plugin with none is idle.
    pub step: Vec<(String, Step)>,
    /// The queue.
    pub queue: Queue,
    /// Plugins the kernel stopped, their author's key withdrawn.
    pub revoked: Vec<String>,
    /// Plugins a check said there is no room for.
    pub no_room: Vec<String>,
    /// The plugin whose install goes on, as its id and its name.
    pub other_job: Option<(String, String)>,
    /// The device runs the stock layout of the code flash.
    pub flash_hint: bool,
    /// Plugins installed that work only after a restart.
    pub restart_needed: Vec<String>,
    /// The device loads code plugins and there is a network.
    pub can_install: bool,
    /// The device takes betas as updates.
    pub takes_betas: bool,
    /// The actions registered (the fonts browser's, for one).
    pub actions: Vec<String>,
}

/// The snapshot and the activity as the screens read them.
#[derive(Debug, Clone, Copy)]
pub struct HostInstalls<'a> {
    snapshot: &'a Snapshot,
    activity: &'a Activity,
}

impl<'a> HostInstalls<'a> {
    /// What is installed and what goes on.
    #[must_use]
    pub const fn new(snapshot: &'a Snapshot, activity: &'a Activity) -> Self {
        Self { snapshot, activity }
    }
}

fn listed(ids: &[String], id: &str) -> bool {
    ids.iter().any(|known| known == id)
}

impl Installs for HostInstalls<'_> {
    fn step(&self, id: &str) -> Step {
        self.activity
            .step
            .iter()
            .find(|(known, _)| known == id)
            .map_or(Step::Idle, |(_, step)| step.clone())
    }

    fn queued(&self) -> bool {
        !self.activity.queue.lines().is_empty()
    }

    fn have(&self, id: &str) -> Option<Installed<'_>> {
        self.snapshot.have(id)
    }

    fn takes_betas(&self) -> bool {
        self.activity.takes_betas
    }

    fn has_action(&self, action: &str) -> bool {
        listed(&self.activity.actions, action)
    }

    fn waiting(&self, id: &str) -> bool {
        self.activity.queue.lines().iter().any(|line| line.id == id)
    }

    fn revoked(&self, id: &str) -> bool {
        listed(&self.activity.revoked, id)
    }

    fn no_room(&self, id: &str) -> bool {
        listed(&self.activity.no_room, id)
    }

    fn other_job(&self, id: &str) -> Option<&str> {
        match &self.activity.other_job {
            Some((job, name)) if job != id => Some(name.as_str()),
            _ => None,
        }
    }

    fn flash_hint(&self) -> bool {
        self.activity.flash_hint
    }

    fn can_install(&self) -> bool {
        self.activity.can_install
    }

    fn restart_needed(&self, id: &str) -> bool {
        listed(&self.activity.restart_needed, id)
    }
}
