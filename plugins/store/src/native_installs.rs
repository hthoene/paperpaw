//! What the device has installed and what is going on with the installs, as the
//! screens read it (`native_screens::Installs`), with no call to the host of its
//! own: [`Snapshot`] is read once from the registry (who put a version in place
//! included: `reg_plugin.flags` bit 5, the host's record of the store's
//! installs), when a screen opens or comes on top, and dropped when the last
//! screen closes; [`Activity`] is what the
//! plugin knows of the downloads, the installs and the queue. [`HostInstalls`]
//! puts the two together.
//!
//! **Memory (#248):** nothing is read at the start; a snapshot is one row of two
//! short texts a plugin (about seventy plugins), reserved exactly from the
//! registry's count; the registry is read through a 4 KiB reply block and a
//! 256-byte request on the stack, both freed with the read.
//!
//! The facts the host knows of the device (what interface 6 read from `env`) are
//! read by [`Activity::read_facts`], once with the snapshot: see there which the
//! host gives and which it does not.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::Storage;
use paperpaw_sdk::native::{
    Error, Host,
    device::{Device as Dev, DeviceCapability},
    registry::{PageCursor, Registry, RegistryNeedKind, RegistryPluginKind},
    settings::{self, SettingValue},
};
use paperpaw_ui_kit::{
    mem::{SERVICE_BUFFER, buffer, push},
    text::copy,
};

use crate::native_page::{Installed, Kind, Origin, Trouble, is_pre_release};
use crate::native_queue::Queue;
use crate::native_screen_list::FONTS;
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
/// The registry's flag of a version a store installed (`registry.md`,
/// `reg_plugin.flags` bit 5): its version is written as the store named it.
const BY_STORE: u32 = 1 << 5;
/// The host itself, the first row of the plugins.
const HOST: &str = "core";
/// The setting of the update plugin that asks for betas (*Beta updates*).
const BETAS: &str = "update.betas";
/// The feature that lets the store install and remove plugins.
const USES_INSTALL: &str = "uses.install";
/// The store's own id, as the registry lists its needs.
const OWN_ID: &str = "store";
/// The actions the screens ask for (`Installs::has_action`).
const ACTIONS: [&str; 1] = [FONTS];
/// Registry pages read at most for the needs of the store and for one plugin's
/// actions.
const MOST_SMALL_PAGES: usize = 4;

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
    /// A row as the registry says it, put in place other than by a store.
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

    /// A row as the registry's `reg_plugin` gives it: its `flags`, its
    /// `version` and the `update` waiting (which counts where there is one),
    /// its `problem`. A version a store installed (`flags` bit 5) is the
    /// store's, its version the release as the store named it (interface 6
    /// `Installed::release`); built-in and linked plugins never are.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn from_registry(
        id: &str,
        kind: Kind,
        flags: u32,
        (version, update): (&str, &str),
        problem: u32,
    ) -> Result<Self, Error> {
        let waiting = !update.is_empty();
        let mut row = Self::new(
            id,
            kind,
            flags & ENABLED != 0,
            flags & RESTART_NEEDED != 0 || waiting,
            if waiting { update } else { version },
            trouble_of(problem),
        )?;
        if flags & BY_STORE != 0 && kind != Kind::BuiltIn {
            row.release = Some(copy(&row.version)?);
            row.origin = Origin::Store;
        }
        Ok(row)
    }
}

/// The plugins the device has, sorted by id, with who put them there.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Snapshot {
    rows: Vec<Row>,
}

impl Snapshot {
    /// Reads the plugins from the registry.
    ///
    /// # Errors
    /// `NoMemory`, the host's refusals; `NoSpace` for more plugins than
    /// `MOST_PAGES` pages hold.
    pub fn read(host: &Host<'_>) -> Result<Self, Error> {
        let mut out = buffer(SERVICE_BUFFER)?;
        let rows = Self::read_rows(host, &mut out, None)?;
        drop(out);
        Ok(Self::settle(rows))
    }

    /// Reads only the rows of the plugins `ids` (those the device has): what
    /// the start and an ask for plugins need, not the whole list.
    ///
    /// # Errors
    /// `NoMemory`, the host's refusals.
    pub fn read_some(host: &Host<'_>, ids: &[&str]) -> Result<Self, Error> {
        let mut out = buffer(SERVICE_BUFFER)?;
        let rows = Self::read_rows(host, &mut out, Some(ids))?;
        drop(out);
        Ok(Self::settle(rows))
    }

    fn read_rows(
        host: &Host<'_>,
        out: &mut [u8],
        only: Option<&[&str]>,
    ) -> Result<Vec<Row>, Error> {
        let registry = Registry::open(host)?;
        let result = Self::pages(host, &registry, out, only);
        registry.release(host).ok();
        result
    }

    fn pages(
        host: &Host<'_>,
        registry: &Registry,
        out: &mut [u8],
        only: Option<&[&str]>,
    ) -> Result<Vec<Row>, Error> {
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
                // The registry says how many rows there are: reserve them once
                // (a read of a few plugins reserves as it finds them).
                let wanted = match only {
                    Some(_) => 0,
                    None => usize::try_from(page.end().total()).unwrap_or(MOST_ROWS),
                };
                rows.try_reserve_exact(wanted.min(MOST_ROWS).saturating_sub(rows.len()))
                    .map_err(|_| Error::NoMemory)?;
                for plugin in page.rows() {
                    if plugin.id() == HOST || only.is_some_and(|ids| !ids.contains(&plugin.id())) {
                        continue;
                    }
                    let kind = match plugin.kind() {
                        RegistryPluginKind::Code => Kind::Code,
                        RegistryPluginKind::Data => Kind::Data,
                        _ => Kind::BuiltIn,
                    };
                    let row = Row::from_registry(
                        plugin.id(),
                        kind,
                        plugin.flags(),
                        (plugin.version(), plugin.update()),
                        plugin.problem(),
                    )?;
                    push(&mut rows, row)?;
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

    /// The rows, sorted by id.
    #[must_use]
    pub fn settle(mut rows: Vec<Row>) -> Self {
        paperpaw_sdk::sort::sort_by(&mut rows, |a, b| a.id.cmp(&b.id));
        Self { rows }
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

/// What the host says of the device, before it is put into the [`Activity`].
#[derive(Debug, Default)]
struct HostFacts {
    /// The device loads code plugins (`dev_native.code_file` is not empty).
    code: bool,
    /// The device has a network (`dev_device.caps` bit 1).
    network: bool,
    /// The store declares `uses.install` (signed metadata, `device.md` 1).
    install: bool,
    /// The firmware is a pre-release.
    beta_firmware: bool,
    /// The reader asked for betas (`update.betas`).
    beta_setting: bool,
    /// Which of [`ACTIONS`] the registry lists.
    actions: Vec<String>,
}

/// A fact the host could not give leaves its default: only a failure to
/// allocate is an error (the screens work without the hint).
fn soft<T>(result: Result<T, Error>) -> Result<Option<T>, Error> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(None),
    }
}

/// Remembers `id` in `list` once.
fn note(list: &mut Vec<String>, id: &str) -> Result<(), Error> {
    if listed(list, id) {
        return Ok(());
    }
    push(list, copy(id)?)
}

impl Activity {
    /// Reads what the host knows of the device and the install and puts it
    /// into the activity: `can_install`, `takes_betas`, `flash_hint`, `actions`
    /// and, from `snapshot`, `restart_needed`. Called with the snapshot, so
    /// the facts are as old as it; one `core.device` and one `core.registry`
    /// handle, a settings read and the file of the code room, through one
    /// 4 KiB reply block and a 256-byte request on the stack, all freed (the
    /// handles let go on every path). Nothing is changed where an allocation
    /// fails; a fact the host does not give stays false.
    ///
    /// What the host gives: `can_install` is the device loading code plugins
    /// (`dev_native.code_file` not empty), having a network (`dev_device.caps`
    /// bit 1) and the store declaring `uses.install` (`reg_need` of its own
    /// row). The host has no probe of the network being up, as interface 6
    /// had none (`env.caps().network` was the capability too); an install that
    /// finds none ends with the host's reason. `takes_betas` is the setting
    /// `update.betas` (a plugin's setting is public, `registry.md` 2.3) or a
    /// pre-release firmware (`dev_firmware.version`). `flash_hint` is the
    /// `larger` of the code room the shell keeps (`code_room`) on a device that
    /// loads code: the stock layout after an SD install; `core.device` has no
    /// op of its own for it (`dev_native` has the bytes free and largest, not
    /// the layout a full flash would give). `actions` are the registry's.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn read_facts(
        &mut self,
        host: &Host<'_>,
        storage: &dyn Storage,
        snapshot: Option<&Snapshot>,
    ) -> Result<(), Error> {
        let mut restart = Vec::new();
        fill_restart(&mut restart, snapshot)?;
        let mut out = buffer(SERVICE_BUFFER)?;
        let facts = read_host(host, &mut out);
        drop(out);
        let facts = facts?;
        self.can_install = facts.code && facts.network && facts.install;
        self.takes_betas = facts.beta_setting || facts.beta_firmware;
        self.flash_hint = facts.code
            && paperpaw_sdk::native::code_room(storage).is_some_and(|room| room.larger.is_some());
        self.actions = facts.actions;
        self.restart_needed = restart;
        Ok(())
    }

    /// Plugins installed that work only after a restart, from `snapshot`.
    ///
    /// # Errors
    /// `NoMemory`; nothing changes then.
    pub fn set_restart_needed(&mut self, snapshot: Option<&Snapshot>) -> Result<(), Error> {
        let mut ids = Vec::new();
        fill_restart(&mut ids, snapshot)?;
        self.restart_needed = ids;
        Ok(())
    }

    /// A check said there is no room for plugin `id`.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn note_no_room(&mut self, id: &str) -> Result<(), Error> {
        note(&mut self.no_room, id)
    }

    /// Plugin `id` is tried again: no word of room is kept.
    pub fn clear_no_room(&mut self, id: &str) {
        self.no_room.retain(|known| known != id);
    }

    /// The install of plugin `id`, shown as `name`, goes on.
    ///
    /// # Errors
    /// `NoMemory`; the job is none then.
    pub fn set_other_job(&mut self, id: &str, name: &str) -> Result<(), Error> {
        self.other_job = None;
        self.other_job = Some((copy(id)?, copy(name)?));
        Ok(())
    }

    /// No install goes on.
    pub fn clear_other_job(&mut self) {
        self.other_job = None;
    }

    /// The kernel stopped plugin `id`, its author's key withdrawn.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn note_revoked(&mut self, id: &str) -> Result<(), Error> {
        note(&mut self.revoked, id)
    }
}

/// The ids of the plugins of `snapshot` that wait for a restart, into `ids`.
fn fill_restart(ids: &mut Vec<String>, snapshot: Option<&Snapshot>) -> Result<(), Error> {
    let Some(snapshot) = snapshot else {
        return Ok(());
    };
    let waiting = snapshot.rows().iter().filter(|row| row.restart_needed);
    ids.try_reserve_exact(waiting.clone().count())
        .map_err(|_| Error::NoMemory)?;
    for row in waiting {
        push(ids, copy(&row.id)?)?;
    }
    Ok(())
}

fn read_host(host: &Host<'_>, out: &mut [u8]) -> Result<HostFacts, Error> {
    let mut input = [0; REQUEST_BYTES];
    let mut facts = HostFacts::default();
    if let Some(dev) = soft(Dev::open(host))? {
        let read = read_device(host, &dev, &mut input, out, &mut facts);
        dev.release(host).ok();
        read?;
    }
    if let Some(registry) = soft(Registry::open(host))? {
        let read = read_registry(host, &registry, &mut input, out, &mut facts);
        registry.release(host).ok();
        read?;
    }
    let mut scratch = [0u8; 64];
    facts.beta_setting = matches!(
        soft(settings::read_setting(host, BETAS, &mut scratch))?,
        Some(SettingValue::Bool(true))
    );
    Ok(facts)
}

fn read_device(
    host: &Host<'_>,
    dev: &Dev,
    input: &mut [u8],
    out: &mut [u8],
    facts: &mut HostFacts,
) -> Result<(), Error> {
    if let Some(identity) = soft(dev.device(host, input, out))? {
        facts.network = identity.supports(DeviceCapability::Network);
    }
    if let Some(native) = soft(dev.native(host, input, out))? {
        facts.code = !native.code_file().is_empty();
    }
    if let Some(firmware) = soft(dev.firmware(host, input, out))? {
        facts.beta_firmware = is_pre_release(firmware.version());
    }
    Ok(())
}

fn read_registry(
    host: &Host<'_>,
    registry: &Registry,
    input: &mut [u8],
    out: &mut [u8],
    facts: &mut HostFacts,
) -> Result<(), Error> {
    let mut cursor = PageCursor::start();
    for _ in 0..MOST_SMALL_PAGES {
        let Some(page) = soft(registry.needs(host, OWN_ID, cursor, input, out))? else {
            break;
        };
        facts.install |= page
            .rows()
            .any(|need| need.kind() == RegistryNeedKind::Feature && need.key() == USES_INSTALL);
        match page.end().next_cursor() {
            Some(next) if !facts.install => cursor = next,
            _ => break,
        }
    }
    for wanted in ACTIONS {
        // The plugin that owns an action is the id before its dot.
        let scope = wanted.split_once('.').map_or(wanted, |(plugin, _)| plugin);
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_SMALL_PAGES {
            let Some(page) = soft(registry.actions(host, scope, cursor, input, out))? else {
                break;
            };
            if page.rows().any(|item| item.id() == wanted) {
                push(&mut facts.actions, copy(wanted)?)?;
                break;
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
    }
    Ok(())
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
