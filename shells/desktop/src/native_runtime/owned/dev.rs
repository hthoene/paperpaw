//! The developer switch's loader for interface-7 plugins (assignment #772,
//! ticket #228): a library in a plugin folder is loaded as the device will
//! load it, and nothing else of it is wired yet.
//!
//! Off unless the one who starts the emulator says so (`--abi7-dev-loader`,
//! or `PAPERPAW_ABI7_DEV_LOADER=1`), the way `--trust-code` is the
//! authorization for interface-6 code: the switch is what makes a library
//! "reader-authorized native code". With it off nothing here runs and the
//! interface-6 loader is exactly as before.
//!
//! The file is `plugins/<id>/code-host<suffix>`, the interface-6 loader's name:
//! the kind is told from the bytes, never from the name ([`classify`]).
//! A library with `PAPERPAW_METADATA` is interface 7 and goes on, its bytes
//! checked before anything of it can run (the strict freestanding rule of
//! `check_freestanding` with `PLUGIN_EXPORTS`, the metadata, the entry), then
//! `PinnedPlugin::open` with the **folder's** id (never one the library
//! gives), `HostOwner::begin(id)`, `make` with its registrations staged, and
//! `commit`. One with `PAPERPAW_API` is interface 6 and is left to the
//! interface-6 loader unchanged; anything else is refused cleanly here.
//!
//! What is staged is kept, not applied: the declarations it holds stay in the
//! plugin ([`DevPlugins::declared`]) and its tick registrations are dispatched
//! ([`DevPlugins::tick`]) without an environment (a tick needs none; slots 4/5 answer
//! `ERR_STATE`). TODO(S2): the kernel's apply step takes the declarations
//! into the registries and hands the kernel's `Env` and canvas to draw
//! dispatch; until then no draw is dispatched from here and nothing a plugin
//! declares is visible to the app.
use super::*;
use crate::host_services::DesktopServices;
use paperpaw_core::native_host::flat::{DropReason, FlatInstance, FlatRegistration};
use paperpaw_core::native_host::registrations::tick_encode;
use paperpaw_core::native_host::staged::Declarations;
use paperpaw_native::preflight::data_export;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, Ordering};

/// The one machine the reviewed profile reads: x86-64.
const MACHINE: u16 = 62;
/// The largest library read.
const MAX_FILE: u64 = 256 << 20;

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Turns the loader on or off for this process (`--abi7-dev-loader`).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Whether the developer switch is on: the flag, or `PAPERPAW_ABI7_DEV_LOADER=1`.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
        || std::env::var_os("PAPERPAW_ABI7_DEV_LOADER").is_some_and(|value| value == "1")
}

/// What a library file is, told from its bytes alone (nothing of it runs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Exports `PAPERPAW_METADATA` and not `PAPERPAW_API`.
    Interface7,
    /// Exports `PAPERPAW_API` and not `PAPERPAW_METADATA`: the interface-6
    /// loader's.
    Interface6,
    /// Neither, both, or not a library this check reads.
    Unclear,
}

/// The kind of the library `bytes`: the interface-7 export first, then the
/// interface-6 one; both or neither is [`Kind::Unclear`], never guessed.
#[must_use]
pub fn classify(bytes: &[u8]) -> Kind {
    let seven = data_export(bytes, MACHINE, host_glue::METADATA_SYMBOL, true).is_ok();
    let six = data_export(bytes, MACHINE, "PAPERPAW_API", false).is_ok();
    match (seven, six) {
        (true, false) => Kind::Interface7,
        (false, true) => Kind::Interface6,
        _ => Kind::Unclear,
    }
}

/// Why a library was not loaded.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The folder's name is no plugin id.
    Id,
    /// The file could not be read, or is too large.
    File,
    /// Not an interface-7 library (see [`Kind`]).
    Kind(Kind),
    /// Refused by the checks before any code ran, or by the factory.
    Runtime(RuntimeError),
    /// Refused while staging or committing what it declared.
    Lifecycle(paperpaw_core::native_host::Error),
    /// Host allocation or construction setup failed.
    Host(String),
    /// It declared records (settings, fonts, hyphenation) and this loader has
    /// no apply step for them yet: refused visibly, never loaded without them.
    Declared(usize),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Id => f.write_str("the folder's name is no plugin id"),
            Self::File => f.write_str("the file cannot be read or is too large"),
            Self::Kind(Kind::Interface6) => {
                f.write_str("an interface-6 library: left to the interface-6 loader")
            }
            Self::Kind(_) => f.write_str("not an interface-7 library (or not one this reads)"),
            Self::Runtime(why) => write!(f, "refused: {why:?}"),
            Self::Lifecycle(why) => write!(f, "refused while staging: {why:?}"),
            Self::Host(why) => write!(f, "host construction refused: {why}"),
            Self::Declared(n) => write!(
                f,
                "declares {n} records, which this loader cannot apply yet: not loaded"
            ),
        }
    }
}

impl From<Error> for Refusal {
    fn from(error: Error) -> Self {
        match error {
            Error::Runtime(why) => Self::Runtime(why),
            Error::Lifecycle(why) => Self::Lifecycle(why),
            other => Self::Host(format!("{other:?}")),
        }
    }
}

/// A plugin folder's id: the segments of `[a-z0-9-]` a plugin's id has.
fn is_id(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// The folder this process copies libraries into before it loads them: its
/// own, made at the first.
fn copies() -> std::io::Result<&'static Path> {
    static COPIES: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    if let Some(dir) = COPIES.get() {
        return Ok(dir);
    }
    let dir = crate::private::dir("paperpaw-abi7")?;
    Ok(COPIES.get_or_init(|| dir))
}

/// The folder of the private copies (for the tests).
#[cfg(test)]
pub(super) fn copies_dir() -> std::io::Result<&'static Path> {
    copies()
}

/// One loaded plugin. Field order is the order of release: the batch (the
/// borrow of the owner) goes first, in `Drop`, then the owner with its
/// library.
pub(super) struct Loaded {
    id: String,
    /// The private copy that is mapped; removed after the unmap.
    copy: std::path::PathBuf,
    /// What it registered, in order (the batch's records).
    records: Vec<Registration>,
    /// Staged by `make` and moved out of the batch at commit; kept, not applied.
    declarations: Declarations,
    batch: Option<Batch<'static, DesktopServices, Registration>>,
    plugin: ManuallyDrop<Box<PinnedPlugin<DesktopServices>>>,
    /// Admission was stopped ([`Loaded::stop`]): nothing of the library is
    /// read or called any more.
    stopped: std::cell::Cell<bool>,
}

impl Loaded {
    fn open(id: &str, source: &Path) -> Result<Self, Refusal> {
        let bytes = read_bounded(source)?;
        Self::from_bytes(id, &bytes)
    }

    /// Loads the library `bytes` (the file as read and checked by the caller)
    /// for the plugin `id`, known to the host: never one the library gives.
    pub(super) fn from_bytes(id: &str, bytes: &[u8]) -> Result<Self, Refusal> {
        if !is_id(id) {
            return Err(Refusal::Id);
        }
        // From the bytes: the kind decides whether this loader is the one.
        match classify(bytes) {
            Kind::Interface7 => {}
            other => return Err(Refusal::Kind(other)),
        }
        let digest = paperpaw_core::sdk::digest::sha256::digest(bytes);
        // A read-only copy of its own (the build may change the file
        // meanwhile): what is checked and loaded is this one file.
        let copy = private_copy(id, bytes, &digest).map_err(|_| Refusal::File)?;
        // SAFETY: the one who started the emulator authorized native code with
        // the developer switch; the copy is read-only and private to this
        // process, and the owner keeps it open until after unload.
        let plugin =
            Box::new(unsafe { PinnedPlugin::open(&copy, &digest, id, DesktopServices::new()) }?);
        let mut transaction = plugin.begin::<Registration>()?;
        // SAFETY: the slice borrows `plugin`, whose owner keeps the library
        // mapped until the batch is gone (the batch is released first, see
        // `Loaded`); declarations are read through `Loaded::image`, which
        // gives none once stopped, never through this borrow.
        transaction
            .set_image(unsafe { plugin.image() })
            .map_err(Refusal::Lifecycle)?;
        let mut batch = plugin.make_registered(transaction)?.commit()?;
        let declarations = batch.take_declarations();
        let depth = DispatchDepth::new();
        let records = (0..batch.len())
            .map(|index| batch.dispatch(index, &depth, |_, record| *record))
            .collect::<Result<Vec<_>, _>>()
            .map_err(Refusal::Lifecycle)?;
        // SAFETY: the batch borrows the owner, which lives on the heap inside
        // `plugin` (a `Box` that is never moved out of nor dropped before the
        // batch: `Drop` releases the batch first and field order does the
        // rest). Nothing else holds the extended lifetime.
        let batch = unsafe {
            std::mem::transmute::<
                Batch<'_, DesktopServices, Registration>,
                Batch<'static, DesktopServices, Registration>,
            >(batch)
        };
        Ok(Self {
            id: id.to_owned(),
            copy,
            records,
            declarations,
            batch: Some(batch),
            plugin: ManuallyDrop::new(plugin),
            stopped: std::cell::Cell::new(false),
        })
    }

    /// Stops callback admission: from here a dispatch answers the owner's
    /// error without reaching the library, and [`Loaded::image`] gives none.
    /// The library stays mapped, with its owner, until the drop.
    pub(super) fn stop(&self) {
        self.stopped.set(true);
        self.plugin.owner.as_ref().revoke();
    }

    /// The library's read-only data (`DeclRef::Image` offsets index it);
    /// `None` once stopped. Decoders re-validate: a plugin has no sandbox.
    pub(super) fn image(&self) -> Option<&[u8]> {
        if self.stopped.get() {
            return None;
        }
        // SAFETY: borrowed from `self`, which holds the owner and its mapped
        // library until the drop; none after `stop`.
        Some(unsafe { self.plugin.image() })
    }

    /// What it declared and nobody applied yet (kept in the bundle, never
    /// dropped): the count of declaration records.
    pub(super) fn declared_records(&self) -> usize {
        self.declarations.specs().len()
            + self.declarations.groups().len()
            + self.declarations.extras().len()
            + self.declarations.hyphenations().len()
            + self.declarations.font_faces().len()
            + self.declarations.font_extensions().len()
    }

    /// The tick registrations, each called with `now_ms`; their statuses.
    fn tick(&self, now_ms: u64) -> Vec<i32> {
        let Some(batch) = &self.batch else {
            return Vec::new();
        };
        let mut bytes = [0u8; 64];
        let Ok(len) = tick_encode(now_ms, &mut bytes) else {
            return Vec::new();
        };
        let depth = DispatchDepth::new();
        self.records
            .iter()
            .enumerate()
            .filter(|(_, record)| matches!(record, Registration::Event { .. }))
            .map(|(index, _)| {
                match dispatch_registration(batch, index, &depth, Invocation::Tick(&bytes[..len])) {
                    Ok(status) => status,
                    Err(Error::Status(status)) => status,
                    Err(_) => host_glue::ERR_STATE,
                }
            })
            .collect()
    }
}

impl Drop for Loaded {
    /// Release order: the batch (its lease on the owner), then admission
    /// revoked and finished, then the owner drops (the plugin's destructor,
    /// then the services, then the library is unmapped).
    fn drop(&mut self) {
        self.stopped.set(true);
        drop(self.batch.take());
        let owner = self.plugin.owner.as_ref();
        owner.revoke();
        if owner.finish().is_ok() {
            // SAFETY: dropped exactly once, here; nothing borrows the owner
            // any longer (the batch is gone and `finish` saw no lease).
            unsafe { ManuallyDrop::drop(&mut self.plugin) };
            // Unmapped: the copy has done its work (a later load writes it anew).
            let _ = std::fs::remove_file(&self.copy);
        } else {
            // Not reachable while no lease is held. If it ever is, whatever
            // holds the library must outlive it: kept mapped, never unmapped.
            eprintln!("abi7 dev loader: {}: still in use at unload, kept", self.id);
        }
    }
}

/// The wire tag of a draw registration (`abi/`, registration records).
const DRAW_RECORD_TAG: u32 = 2;

impl FlatInstance for Loaded {
    fn registrations(&self, visit: &mut dyn FnMut(FlatRegistration)) {
        for (index, record) in self.records.iter().enumerate() {
            visit(match *record {
                Registration::Event { event_tag, .. } => {
                    FlatRegistration::Event { event_tag, index }
                }
                Registration::Draw { .. } => FlatRegistration::Other {
                    record_tag: DRAW_RECORD_TAG,
                },
                // A kind this build does not know is no event: it is told by
                // the tag 0, which `FlatPlugin::new` refuses as unrouted.
                _ => FlatRegistration::Other { record_tag: 0 },
            });
        }
    }

    fn environment(&self) -> Pin<&EnvironmentScope> {
        let context = self.plugin.owner.as_ref().context();
        // SAFETY: the owner structurally pins its context; this projects one
        // field and never moves it.
        unsafe { context.map_unchecked(|value| &value.environment) }
    }

    fn dispatch_event(
        &self,
        index: usize,
        payload: &[u8],
        depth: &DispatchDepth,
    ) -> Result<i32, native_host::Error> {
        // Stopped: the owner's error, before anything of the library is read.
        let batch = match (&self.batch, self.stopped.get()) {
            (Some(batch), false) => batch,
            _ => return Err(native_host::Error::State),
        };
        match dispatch_registration(batch, index, depth, Invocation::Event(payload)) {
            Ok(status) => Ok(status),
            Err(Error::Status(status)) => Ok(status),
            Err(Error::Lifecycle(why)) => Err(why),
            Err(_) => Err(native_host::Error::State),
        }
    }

    fn event_dropped(&self, tag: u32, why: DropReason) {
        eprintln!("abi7 code plugin {}: event {tag} dropped: {why:?}", self.id);
    }
}

/// Loads the interface-7 library `bytes` of the installed plugin `id` as a
/// flat instance the core adapter can run, or says why not. Nothing it declared
/// is applied by this loader yet: a library that declares anything is refused
/// (`Refusal::Declared`) rather than losing it.
pub(crate) fn load_flat(id: &str, bytes: &[u8]) -> Result<Box<dyn FlatInstance>, Refusal> {
    let loaded = Loaded::from_bytes(id, bytes)?;
    let declared = loaded.declared_records();
    if declared != 0 {
        return Err(Refusal::Declared(declared));
    }
    Ok(Box::new(loaded))
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, Refusal> {
    let file = std::fs::File::open(path).map_err(|_| Refusal::File)?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(file, MAX_FILE + 1), &mut bytes)
        .map_err(|_| Refusal::File)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(Refusal::File);
    }
    Ok(bytes)
}

fn private_copy(id: &str, bytes: &[u8], digest: &[u8; 32]) -> std::io::Result<std::path::PathBuf> {
    let copy = copies()?.join(format!(
        "{id}-{}{}",
        paperpaw_core::sdk::digest::hex::encode(&digest[..8]),
        std::env::consts::DLL_SUFFIX
    ));
    if !copy.exists() {
        // Written under a name of its own, then renamed into place: a copy cut
        // short is never found under the name that is trusted, and is removed.
        static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let temp = copy.with_extension(format!(
            "part{}",
            COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let written = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            std::io::Write::write_all(&mut file, bytes)?;
            let mut permissions = file.metadata()?.permissions();
            permissions.set_readonly(true);
            file.set_permissions(permissions)?;
            std::fs::rename(&temp, &copy)
        })();
        if let Err(e) = written {
            let _ = std::fs::remove_file(&temp);
            return Err(e);
        }
    }
    Ok(copy)
}

/// The interface-7 plugins of a card, loaded under the developer switch.
#[derive(Default)]
pub struct DevPlugins {
    loaded: Vec<Loaded>,
    refused: Vec<(String, Refusal)>,
}

impl std::fmt::Debug for DevPlugins {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DevPlugins")
            .field("loaded", &self.ids())
            .field("refused", &self.refused)
            .finish()
    }
}

impl DevPlugins {
    /// Loads what is in `root`'s `plugins/<id>/code-host<suffix>` files, if
    /// the developer switch is on; none otherwise. A file that is not
    /// interface 7 is passed over (interface 6 is the other loader's) or
    /// refused and said; one refusal never stops the others.
    #[must_use]
    pub fn load_folder(root: &Path) -> Self {
        Self::load_folder_if(root, enabled())
    }

    /// [`load_folder`](Self::load_folder) with the switch given.
    #[must_use]
    pub(super) fn load_folder_if(root: &Path, on: bool) -> Self {
        let mut found = Self::default();
        if !on {
            return found;
        }
        let plugins = root.join(paperpaw_core::sdk::builtin::PLUGIN_DIR);
        let Ok(folders) = std::fs::read_dir(&plugins) else {
            return found;
        };
        let mut folders: Vec<_> = folders.flatten().collect();
        folders.sort_by_key(|folder| folder.file_name());
        for folder in folders {
            let file = folder.path().join(crate::native::file_name());
            if !file.is_file() {
                continue;
            }
            let id = folder.file_name().to_string_lossy().into_owned();
            match Loaded::open(&id, &file) {
                Ok(loaded) => found.loaded.push(loaded),
                Err(Refusal::Kind(Kind::Interface6)) => {}
                Err(why) => {
                    eprintln!("abi7 dev loader: {id}: {why}");
                    found.refused.push((id, why));
                }
            }
        }
        found
    }

    /// The ids loaded, in folder order.
    #[must_use]
    pub fn ids(&self) -> Vec<&str> {
        self.loaded.iter().map(|l| l.id.as_str()).collect()
    }

    /// What was refused, by folder id.
    #[must_use]
    pub fn refused(&self) -> &[(String, Refusal)] {
        &self.refused
    }

    /// How many declaration records plugin `id` staged (kept, not applied).
    #[must_use]
    pub fn declared(&self, id: &str) -> Option<usize> {
        self.loaded.iter().find(|l| l.id == id).map(|l| {
            l.declarations.specs().len()
                + l.declarations.groups().len()
                + l.declarations.extras().len()
                + l.declarations.hyphenations().len()
                + l.declarations.font_faces().len()
                + l.declarations.font_extensions().len()
        })
    }

    /// Calls every tick registration of every plugin with `now_ms`; the
    /// statuses by plugin id.
    #[must_use]
    pub fn tick(&self, now_ms: u64) -> Vec<(&str, Vec<i32>)> {
        self.loaded
            .iter()
            .map(|l| (l.id.as_str(), l.tick(now_ms)))
            .collect()
    }

    /// Unloads all, in the reverse of their loading.
    pub fn unload(&mut self) {
        while self.loaded.pop().is_some() {}
        self.refused.clear();
    }
}

impl Drop for DevPlugins {
    fn drop(&mut self) {
        self.unload();
    }
}
