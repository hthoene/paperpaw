//! Ops 5 `plugins` and 6 `plugin`: every plugin the host knows, loaded or
//! not, and one plugin with its description (`registry.md` 2.2, 3).
use super::{
    Row,
    fixed::{Fixed, caps, clip},
    sources::Sources,
};
use crate::native_host::registrations::{self as wire, Record};
use alloc::vec::Vec;
use core::{fmt::Write, ops::ControlFlow};
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_NOT_FOUND};
use paperpaw_platform::Storage;
use paperpaw_sdk::{PluginInfo, PluginKind, PluginProblem, PluginState, Version};

/// `reg_plugin.flags`.
const ENABLED: u32 = 1;
const RESTART_NEEDED: u32 = 1 << 1;
const REQUIRED: u32 = 1 << 2;
const LINKED: u32 = 1 << 3;
const SAFE_MODE: u32 = 1 << 4;
const FROM_STORE: u32 = 1 << 5;

/// The host's record of the versions a store installed (`<id>\t<release>`
/// lines, the newest last; `paperpaw_sdk::install::Origins`), out of reach of
/// plugins and of the card transfers.
const STORE_INSTALLS: &str = ".paperpaw/plugin-origin.txt";
/// The longest record read (as the SDK's reader).
const RECORD_MAX: u64 = 64 * 1024;

/// The record, read once for a call (`reg_plugin.flags` bit 5).
struct StoreRecord(Vec<u8>);

impl StoreRecord {
    /// None kept, or a card that cannot read it, is an empty record (no
    /// plugin counts as a store's, as the SDK's reader takes it).
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`: the call can be asked again.
    fn read(storage: &dyn Storage) -> Result<Self, i32> {
        let Ok(size) = storage.size(STORE_INSTALLS) else {
            return Ok(Self(Vec::new()));
        };
        let size = usize::try_from(size.min(RECORD_MAX)).map_err(|_| ERR_NO_MEMORY)?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(size).map_err(|_| ERR_NO_MEMORY)?;
        bytes.resize(size, 0);
        let mut filled = 0;
        while filled < size {
            match storage.read_at(STORE_INSTALLS, filled as u64, &mut bytes[filled..]) {
                Ok(0) | Err(_) => break,
                Ok(read) => filled += read,
            }
        }
        bytes.truncate(filled);
        Ok(Self(bytes))
    }

    /// The release of `plugin` a store installed with the numbers of
    /// `version`, as the store named it (`1.2.0-beta.1`): the last kept.
    fn release(&self, plugin: &str, version: Version) -> Option<&str> {
        self.0.rsplit(|&byte| byte == b'\n').find_map(|line| {
            let line = core::str::from_utf8(line).ok()?;
            let (id, written) = line.split_once('\t')?;
            let written = written.trim();
            (id == plugin && Version::parse(written) == Some(version)).then_some(written)
        })
    }
}

/// Room for `<id>.plugin.description`: an id (64), a dot and the suffix.
type Key = Fixed<96>;

/// Interface 6's `PluginProblem` in its order (0 none), and its number.
fn problem(problem: Option<PluginProblem>) -> (u32, u32) {
    match problem {
        None => (0, 0),
        Some(PluginProblem::OtherBuild) => (1, 0),
        Some(PluginProblem::Damaged) => (2, 0),
        Some(PluginProblem::Mismatch) => (3, 0),
        Some(PluginProblem::NoRoom) => (4, 0),
        Some(PluginProblem::Unsupported) => (5, 0),
        Some(PluginProblem::SafeMode) => (6, 0),
        Some(PluginProblem::Crashed) => (7, 0),
        Some(PluginProblem::NotInstalled) => (8, 0),
        Some(PluginProblem::Changed) => (9, 0),
        Some(PluginProblem::OlderInterface { built_for }) => (10, u32::from(built_for)),
        Some(PluginProblem::NeedsNewerPaperPaw) => (11, 0),
        Some(PluginProblem::Revoked) => (12, 0),
        // A cause a later build adds, by its code.
        Some(PluginProblem::Other(code)) => (13, u32::from(code)),
        Some(_) => (13, 0),
    }
}

fn flags(info: &PluginInfo, from_store: bool) -> u32 {
    let mut flags = if from_store { FROM_STORE } else { 0 };
    if info.enabled {
        flags |= ENABLED;
    }
    if info.restart_needed() {
        flags |= RESTART_NEEDED;
    }
    // Interim: the slots only this plugin fills; the plugins a plugin
    // requires are not kept by the host yet (op 20).
    if !info.required_for.is_empty() {
        flags |= REQUIRED;
    }
    if info.kind == PluginKind::BuiltIn {
        flags |= LINKED;
    }
    if matches!(info.problem, Some(PluginProblem::SafeMode)) {
        flags |= SAFE_MODE;
    }
    flags
}

/// The row of `info` handed to `visit` (its strings live on this stack).
fn with_row<R>(
    sources: &Sources<'_>,
    record: &StoreRecord,
    info: &PluginInfo,
    visit: impl FnOnce(&dyn Row) -> R,
) -> R {
    let id = info.manifest.id.as_str();
    // Bit 5: the version installed, or the one waiting, is a store's; it is
    // written as the store named it. Never a plugin built in or linked.
    let store = |version: Version| {
        (info.kind != PluginKind::BuiltIn)
            .then(|| record.release(id, version))
            .flatten()
    };
    let installed = store(info.manifest.version);
    let waiting = info.update.and_then(store);
    let name = Key::of(&[id, ".plugin.name"])
        .and_then(|key| sources.env.i18n().get(key.as_str()))
        .unwrap_or(&info.manifest.name);
    let mut version = Fixed::<32>::new();
    // A version too long for its field is cut, never refused.
    let _ = match installed {
        Some(release) => version.write_str(release),
        None => write!(version, "{}", info.manifest.version),
    };
    let mut update = Fixed::<32>::new();
    if let Some(next) = &info.update {
        let _ = match waiting {
            Some(release) => update.write_str(release),
            None => write!(update, "{next}"),
        };
    }
    let (mut problem, mut problem_arg) = problem(info.problem);
    // A plugin refused at a declaration cap says which (14, 15; the cap).
    if matches!(info.state, PluginState::Failed)
        && let Some(cap) = sources
            .directory
            .and_then(|directory| directory.refusal(id))
    {
        (problem, problem_arg) = (cap.problem, cap.arg);
    }
    visit(&Record::ServiceRegPlugin(wire::ServiceRegPlugin {
        id,
        name: clip(name, caps::PLUGIN_NAME),
        version: version.as_str(),
        kind: match info.kind {
            PluginKind::BuiltIn => 0,
            PluginKind::Data => 1,
            // A kind a later build adds reads as code this build cannot run.
            _ => 2,
        },
        state: match info.state {
            PluginState::Running => 0,
            PluginState::Off => 1,
            _ => 2,
        },
        flags: flags(info, installed.is_some() || waiting.is_some()),
        problem,
        problem_arg,
        update: update.as_str(),
    }))
}

/// Op 5: the host itself first (`core`, its version), then the others in the
/// order they were given to the kernel.
pub(super) fn rows(
    sources: &Sources<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let record = StoreRecord::read(sources.env.storage())?;
    let plugins = sources.env.plugins();
    let core = plugins.kernel();
    if let Some(info) = core
        && with_row(sources, &record, info, &mut *visit).is_break()
    {
        return Ok(());
    }
    for info in plugins.iter() {
        if core.is_some_and(|core| core.manifest.id == info.manifest.id) {
            continue;
        }
        if with_row(sources, &record, info, &mut *visit).is_break() {
            break;
        }
    }
    Ok(())
}

/// Op 6: the row and, where the plugin has a description, its text.
pub(super) fn detail(
    sources: &Sources<'_>,
    key: &str,
    visit: &mut dyn FnMut(&dyn Row) -> Result<(), i32>,
) -> Result<(), i32> {
    let info = sources.env.plugins().get(key).ok_or(ERR_NOT_FOUND)?;
    let record = StoreRecord::read(sources.env.storage())?;
    let mut failure = None;
    with_row(sources, &record, info, |row| {
        if let Err(error) = visit(row) {
            failure = Some(error);
        }
    });
    if let Some(error) = failure {
        return Err(error);
    }
    let id = info.manifest.id.as_str();
    let translated =
        Key::of(&[id, ".plugin.description"]).and_then(|key| sources.env.i18n().get(key.as_str()));
    let description = translated.unwrap_or(&*info.manifest.description);
    if description.is_empty() {
        return Ok(());
    }
    visit(&Record::ServiceRegText(wire::ServiceRegText {
        text: clip(description, caps::TEXT),
    }))
}
