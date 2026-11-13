//! A scripted host for native plugin tests (#228, todo 94).
//!
//! It stands where the firmware or the kernel's shell stands: it makes a
//! plugin from its `PpEntry` with a host table whose slots it answers itself,
//! then runs the plugin's callbacks one at a time and shows what the plugin
//! did. Nothing here changes the interface; the slots keep the shell's
//! statuses and lengths.
//!
//! What it can do:
//! - **Load**: [`ScriptedHost::load`] makes the plugin; its registration
//!   batches are kept ([`ScriptedHost::registrations`]).
//! - **Run callbacks**: [`Plugin::event`], [`Plugin::input`] and
//!   [`Plugin::draw`] return a [`Callback`]: the status and
//!   the commands the plugin staged, decoded. Staging is the kernel's own
//!   ([`CommandStage`]),
//!   with its limits (set them with [`ScriptedHost::limits`]) and its
//!   admission of settings (register them with [`ScriptedHost::settings_mut`]);
//!   a callback that returns a negative status loses everything it staged
//!   (see [`Callback::dropped`]); a draw has no stage, so `commands.submit`
//!   there is `ERR_STATE`.
//! - **Script answers**: [`ScriptedHost::service`] adds a `core.*` service
//!   with a closure per call; [`ScriptedHost::set_value`] and
//!   [`ScriptedHost::set_env`] script `settings.read` and `env.snapshot`.
//!   Questions are answered by sending the plugin its `Answered` event
//!   ([`event_batch`]).
//! - **Texts**: [`ScriptedHost::add_catalog`], [`ScriptedHost::add_catalogs_in`]
//!   (a plugin's `i18n` folder) and [`ScriptedHost::set_language`] feed
//!   `i18n.copy`, `i18n.format` and `i18n.digits`, answered by the shell's own
//!   code, so keys, plurals, fallbacks and placeholders give its results.
//! - **Refuse**: [`ScriptedHost::refuse_nth`] makes the n-th call of a slot
//!   fail with a status (`ERR_FULL`, `ERR_BUSY`, `ERR_NO_MEMORY` ...);
//!   [`sweep`] runs a scenario once clean and once per call, each with that
//!   call refused, and [`ScriptedHost::live_allocations`] and
//!   [`ScriptedHost::open_handles`] show what the plugin still holds.
#![allow(
    clippy::missing_errors_doc,
    reason = "test support: statuses are the interface's, named at the types"
)]
mod refusal;
mod registry;
mod slots;
mod ui;

pub use refusal::Slot;
pub use registry::{PAGE_ROWS, Registry, row};
pub use ui::{Answer, Dialogs, Question};

use core::ffi::c_void;
use paperpaw_core::native_host::commands::{
    CALLBACK_COMMAND_BYTES, CALLBACK_COMMANDS, CommandScope, CommandStage,
};
use paperpaw_core::native_host::environment::{SettingValueRef, settings_encode};
use paperpaw_i18n::{Catalog, CatalogError, Translator};
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_UNSUPPORTED, OK, PpBuffer, PpBytes, PpEntry, PpHost, PpPlugin,
};
use paperpaw_sdk::{Command, Id, Settings};
use refusal::{Refusals, Rule};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::pin::Pin;

/// A scripted service's backend: `(op, input)` to the reply bytes or a status.
pub(crate) type Handler = Box<dyn FnMut(u32, &[u8]) -> Result<Vec<u8>, i32>>;

/// A hook run when a callback returns, with its status.
pub(crate) type EndedHook = Box<dyn Fn(i32)>;

/// One scripted `core.*` service: `handler(op, input)` is its backend.
pub(crate) struct ScriptedService {
    pub(crate) key: String,
    pub(crate) major: u16,
    pub(crate) revision: u16,
    pub(crate) handler: Handler,
    /// The reply bound of ops without their own (see [`DEFAULT_REPLY_BOUND`]).
    pub(crate) bound: usize,
    pub(crate) op_bounds: Vec<(u32, usize)>,
}

impl ScriptedService {
    /// The reply bound of `op`: the caller's `out` must hold this many bytes.
    pub(crate) fn bound(&self, op: u32) -> usize {
        self.op_bounds
            .iter()
            .find(|(id, _)| *id == op)
            .map_or(self.bound, |(_, bound)| *bound)
    }
}

/// The reply bound of a scripted op unless [`ScriptedHost::reply_bound`] sets
/// it: the generated schemas bound most replies by 4,096 bytes, and the shell
/// refuses a smaller `out` before the service runs.
pub const DEFAULT_REPLY_BOUND: usize = 4096;

/// What the slots share: boxed, so its address is the `ctx` the plugin keeps.
pub(crate) struct State {
    pub(crate) settings: Settings,
    pub(crate) owner: Id,
    pub(crate) limits: Cell<(usize, usize)>,
    pub(crate) env: RefCell<Vec<u8>>,
    pub(crate) values: RefCell<BTreeMap<String, Vec<u8>>>,
    pub(crate) services: RefCell<Vec<ScriptedService>>,
    pub(crate) open: RefCell<Vec<u32>>,
    pub(crate) refusals: Refusals,
    pub(crate) registrations: RefCell<Vec<Vec<u8>>>,
    pub(crate) batches: RefCell<Vec<Vec<u8>>>,
    pub(crate) live: Cell<usize>,
    pub(crate) clock: Cell<u64>,
    pub(crate) scope: CommandScope,
    /// The plugin's catalogs and the language (`i18n.*`).
    pub(crate) translator: RefCell<Translator>,
    /// Hooks run when a callback returns, with its status (the dialogs').
    pub(crate) ended: RefCell<Vec<EndedHook>>,
    table: PpHost,
}

/// The scripted host. Configure it, then [`load`](Self::load) a plugin.
#[derive(Debug)]
pub struct ScriptedHost {
    state: Box<State>,
}

impl core::fmt::Debug for State {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("State").finish_non_exhaustive()
    }
}

impl Default for ScriptedHost {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptedHost {
    /// A host for the plugin `probe` with the kernel's command limits, no
    /// services, no settings and an empty environment.
    #[must_use]
    pub fn new() -> Self {
        Self::for_plugin("probe")
    }

    /// As [`new`](Self::new), for the plugin named `id` (the owner of its
    /// settings and screens).
    ///
    /// # Panics
    /// `id` is not a valid plugin id.
    #[must_use]
    #[allow(
        clippy::expect_used,
        reason = "test support: a bad id is a mistake in the test, reported at once"
    )]
    pub fn for_plugin(id: &str) -> Self {
        let state = Box::new(State {
            settings: Settings::new(),
            owner: Id::try_parse(id).expect("a valid plugin id"),
            limits: Cell::new((CALLBACK_COMMANDS, CALLBACK_COMMAND_BYTES)),
            env: RefCell::new(Vec::new()),
            values: RefCell::new(BTreeMap::new()),
            services: RefCell::new(Vec::new()),
            open: RefCell::new(Vec::new()),
            refusals: Refusals::default(),
            registrations: RefCell::new(Vec::new()),
            batches: RefCell::new(Vec::new()),
            live: Cell::new(0),
            clock: Cell::new(0),
            scope: CommandScope::new(),
            translator: RefCell::new(Translator::new()),
            ended: RefCell::new(Vec::new()),
            table: slots::table(),
        });
        Self { state }
    }

    /// The settings the host knows, for the admission of `set_setting`
    /// commands; define them before loading.
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.state.settings
    }

    /// Adds the catalog `source` (the text of an `i18n/<language>.ini`): the
    /// plugin's texts for `i18n.copy`, `i18n.format` and plurals. Catalogs
    /// added later take precedence for the same language.
    ///
    /// # Errors
    /// The catalog does not parse.
    pub fn add_catalog(&self, source: &str) -> Result<(), CatalogError> {
        let catalog = Catalog::parse(source.to_owned())?;
        self.state.translator.borrow_mut().add(catalog);
        Ok(())
    }

    /// Adds every `*.ini` of `folder` (a plugin's `i18n` folder, by file name
    /// order), as the kernel reads a plugin's catalogs. A directory entry that
    /// cannot be read is skipped (the files that can be read are added), and
    /// so is anything that is not a `*.ini` file.
    ///
    /// # Errors
    /// The folder or a file cannot be read, or a catalog does not parse (the
    /// text names the file).
    pub fn add_catalogs_in(&self, folder: impl AsRef<std::path::Path>) -> Result<usize, String> {
        let mut files: Vec<_> = std::fs::read_dir(folder.as_ref())
            .map_err(|error| format!("{}: {error}", folder.as_ref().display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "ini"))
            .collect();
        files.sort();
        for path in &files {
            let source = std::fs::read_to_string(path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            self.add_catalog(&source)
                .map_err(|error| format!("{}: {error:?}", path.display()))?;
        }
        Ok(files.len())
    }

    /// The UI language the catalogs answer in (`en` by default; a key the
    /// language lacks falls back as the kernel's does, ending in English).
    pub fn set_language(&self, language: &str) {
        self.state.translator.borrow_mut().set_language(language);
    }

    /// The most commands and bytes one callback may stage (the kernel's
    /// `CALLBACK_COMMANDS` and `CALLBACK_COMMAND_BYTES` by default).
    pub fn limits(&self, commands: usize, bytes: usize) {
        self.state.limits.set((commands, bytes));
    }

    /// The bytes `env.snapshot` answers.
    pub fn set_env(&self, snapshot: &[u8]) {
        *self.state.env.borrow_mut() = snapshot.to_vec();
    }

    /// The bytes `settings.read` answers for `key` (changeable between calls);
    /// a key never set answers the encoded `Absent` record.
    pub fn set_value(&self, key: &str, value: &[u8]) {
        self.state
            .values
            .borrow_mut()
            .insert(key.to_owned(), value.to_vec());
    }

    /// As [`set_value`](Self::set_value), with the value encoded as the shell
    /// answers it. A key never set reads as the encoded `Absent` record.
    ///
    /// # Panics
    /// `value` is text over the interface's limit.
    #[allow(
        clippy::expect_used,
        reason = "test support: an oversized value is a mistake in the test"
    )]
    pub fn set_setting(&self, key: &str, value: SettingValueRef<'_>) {
        let mut record = [core::mem::MaybeUninit::<u8>::uninit(); 4096];
        let len = settings_encode(value, &mut record).expect("a setting value within the limit");
        // SAFETY: `settings_encode` initialized the first `len` bytes.
        let bytes: Vec<u8> = record[..len.min(4096)]
            .iter()
            .map(|byte| unsafe { byte.assume_init() })
            .collect();
        self.set_value(key, &bytes);
    }

    /// The microseconds `clock.micros` answers.
    pub fn set_clock(&self, micros: u64) {
        self.state.clock.set(micros);
    }

    /// Adds the service `key` at `major`, up to `revision`; `handler(op,
    /// input)` answers every call with the reply bytes or a status.
    pub fn service(
        &self,
        key: &str,
        major: u16,
        revision: u16,
        handler: impl FnMut(u32, &[u8]) -> Result<Vec<u8>, i32> + 'static,
    ) {
        self.state.services.borrow_mut().push(ScriptedService {
            key: key.to_owned(),
            major,
            revision,
            handler: Box::new(handler),
            bound: DEFAULT_REPLY_BOUND,
            op_bounds: Vec::new(),
        });
    }

    /// Adds `core.ui` for questions (ops 8, 9 and 10) and returns the handle
    /// that shows what was asked and answers it.
    pub fn dialogs(&self) -> Dialogs {
        let dialogs = Dialogs::new();
        let backend = dialogs.clone();
        let ended = dialogs.clone();
        self.state
            .ended
            .borrow_mut()
            .push(Box::new(move |status| ended.callback_ended(status)));
        self.service("core.ui", 1, u16::MAX, move |op, input| {
            backend.call(op, input)
        });
        // The schema's reply bounds: `ask` and `text_read` 4,096 bytes,
        // `text_write` 64 (the default 4,096 covers the first two).
        self.reply_bound("core.ui", 9, 64);
        dialogs
    }

    /// Adds `core.registry` and returns the handle that scripts its rows,
    /// pages and generation.
    pub fn registry(&self) -> Registry {
        let registry = Registry::new();
        let backend = registry.clone();
        self.service(
            "core.registry",
            registry::MAJOR,
            u16::MAX,
            move |op, input| backend.call(op, input),
        );
        registry
    }

    /// The reply bound of `op` of the service `key`: the plugin's `out` for a
    /// call must hold this many bytes (else `ERR_NO_SPACE`, and the handler
    /// is not run), and a longer reply is `ERR_INVALID`. Without it the bound
    /// is [`DEFAULT_REPLY_BOUND`]; set it as the op's schema does.
    pub fn reply_bound(&self, key: &str, op: u32, bytes: usize) {
        if let Some(service) = self
            .state
            .services
            .borrow_mut()
            .iter_mut()
            .find(|service| service.key == key)
        {
            service.op_bounds.retain(|(id, _)| *id != op);
            service.op_bounds.push((op, bytes));
        }
    }

    /// The `nth` call (from 1) of `slot` answers `status` and does nothing
    /// else. Counts run from the host's making or the last
    /// [`reset_counts`](Self::reset_counts).
    pub fn refuse_nth(&self, slot: Slot, nth: usize, status: i32) {
        self.state.refusals.add(Rule {
            slot,
            nth,
            every: false,
            status,
        });
    }

    /// Every call of `slot` from the `nth` on answers `status`.
    pub fn refuse_from(&self, slot: Slot, nth: usize, status: i32) {
        self.state.refusals.add(Rule {
            slot,
            nth,
            every: true,
            status,
        });
    }

    /// Forgets every refusal rule.
    pub fn clear_refusals(&self) {
        self.state.refusals.clear_rules();
    }

    /// How many times the plugin called `slot` since the last reset.
    #[must_use]
    pub fn calls(&self, slot: Slot) -> usize {
        self.state.refusals.calls(slot)
    }

    /// Sets every call count back to zero.
    pub fn reset_counts(&self) {
        self.state.refusals.reset_counts();
    }

    /// Heap blocks the plugin holds (allocated and not freed).
    #[must_use]
    pub fn live_allocations(&self) -> usize {
        self.state.live.get()
    }

    /// Service handles the plugin holds (opened and not released).
    #[must_use]
    pub fn open_handles(&self) -> usize {
        self.state.open.borrow().len()
    }

    /// The registration batches the plugin submitted, as sent.
    #[must_use]
    pub fn registrations(&self) -> Vec<Vec<u8>> {
        self.state.registrations.borrow().clone()
    }

    /// Makes the plugin of `entry` with this host's table.
    ///
    /// # Errors
    /// The factory's status; `ERR_UNSUPPORTED` when the entry has none.
    pub fn load(&self, entry: &PpEntry) -> Result<Plugin<'_>, i32> {
        let make = entry.make.ok_or(ERR_UNSUPPORTED)?;
        let mut plugin: *const PpPlugin = core::ptr::null();
        let ctx = core::ptr::from_ref::<State>(&self.state)
            .cast_mut()
            .cast::<c_void>();
        // SAFETY: the table and the boxed state outlive the plugin (`Plugin`
        // borrows this host); the factory writes `plugin` or returns a status.
        let status = unsafe { make(&raw const self.state.table, ctx, &raw mut plugin) };
        if status != OK {
            return Err(status);
        }
        if plugin.is_null() {
            return Err(ERR_INVALID);
        }
        Ok(Plugin { host: self, plugin })
    }
}

/// What one callback did.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Callback {
    /// The callback's status.
    pub status: i32,
    /// The commands it staged, in order; empty after a negative status.
    pub commands: Vec<Command>,
    /// The command batches it submitted, byte for byte, accepted or not (for
    /// exact-wire assertions; a refused-by-the-host call is not here).
    pub batches: Vec<Vec<u8>>,
    /// How many commands it had staged when it returned a negative status:
    /// the whole stage is dropped, so a half-applied step is never seen.
    pub dropped: usize,
}

/// A made plugin; dropped, it tears the plugin down.
#[derive(Debug)]
pub struct Plugin<'h> {
    host: &'h ScriptedHost,
    plugin: *const PpPlugin,
}

#[derive(Clone, Copy)]
enum Kind {
    Event,
    /// An event with a reply buffer (a stage call): its start and capacity.
    Stage(*mut u8, usize),
    Input,
    Draw,
}

impl Callback {
    /// Whether any batch the callback submitted holds `needle` (for example a
    /// secret that must never be in a command).
    #[must_use]
    pub fn batches_contain(&self, needle: &[u8]) -> bool {
        !needle.is_empty()
            && self
                .batches
                .iter()
                .any(|batch| batch.windows(needle.len()).any(|window| window == needle))
    }
}

impl Plugin<'_> {
    /// `dispatch_event(token, batch)`. An event has no reply buffer (its
    /// capacity is 0, as in the shell) and returns a status only: a positive
    /// result is a plugin fault and is reported as `ERR_INVALID`.
    pub fn event(&self, token: u32, batch: &[u8]) -> Result<Callback, i32> {
        self.run(Kind::Event, token, batch)
    }

    /// `dispatch_event(token, batch)` with a reply buffer of `capacity` bytes,
    /// as the host lends one to a stage call (a lookup provider's answer):
    /// the callback, and the reply the plugin wrote (its positive status is
    /// the reply's length; empty after a negative status).
    pub fn stage_call(
        &self,
        token: u32,
        batch: &[u8],
        capacity: usize,
    ) -> Result<(Callback, Vec<u8>), i32> {
        let mut reply = vec![0_u8; capacity];
        let mut callback = self.run(Kind::Stage(reply.as_mut_ptr(), capacity), token, batch)?;
        let used = usize::try_from(callback.status).unwrap_or(0).min(capacity);
        reply.truncate(used);
        if callback.status > 0 {
            callback.status = 0;
        }
        Ok((callback, reply))
    }

    /// `input(token, batch)`. The status is 0 (not taken), 1 (taken) or an
    /// error; anything above 1 is a plugin fault and is `ERR_INVALID`, as in
    /// the shell.
    pub fn input(&self, token: u32, batch: &[u8]) -> Result<Callback, i32> {
        self.run(Kind::Input, token, batch)
    }

    /// `draw(token, frame)`; no stage is lent.
    pub fn draw(&self, token: u32, frame: &[u8]) -> Result<Callback, i32> {
        self.run(Kind::Draw, token, frame)
    }

    fn run(&self, kind: Kind, token: u32, input: &[u8]) -> Result<Callback, i32> {
        let out = match kind {
            Kind::Stage(ptr, cap) => (ptr, cap),
            _ => (core::ptr::NonNull::dangling().as_ptr(), 0),
        };
        let state = &*self.host.state;
        // SAFETY: the table `make` returned stays valid until the plugin drops.
        let table = unsafe { &*self.plugin };
        let input = PpBytes {
            ptr: if input.is_empty() {
                core::ptr::null()
            } else {
                input.as_ptr()
            },
            len: input.len(),
        };
        let (limit, byte_limit) = state.limits.get();
        state.batches.borrow_mut().clear();
        let mut stage = CommandStage::new(&state.owner, &state.settings, None, limit, byte_limit);
        let call = || -> Result<i32, i32> {
            // SAFETY: plugin state and callbacks are the factory's; the input
            // and the buffer live for the call.
            Ok(unsafe {
                match kind {
                    Kind::Event | Kind::Stage(..) => {
                        (table.dispatch_event.ok_or(ERR_UNSUPPORTED)?)(
                            table.state,
                            token,
                            input,
                            // An event has no reply buffer (a dangling pointer with
                            // capacity 0); a stage call has the caller's.
                            PpBuffer {
                                ptr: out.0,
                                cap: out.1,
                            },
                        )
                    }
                    Kind::Input => (table.input.ok_or(ERR_UNSUPPORTED)?)(table.state, token, input),
                    Kind::Draw => (table.draw.ok_or(ERR_UNSUPPORTED)?)(table.state, token, input),
                }
            })
        };
        let status = match kind {
            Kind::Draw => call()?,
            Kind::Event | Kind::Stage(..) | Kind::Input => {
                // SAFETY: the scope lives in the boxed state and never moves.
                let scope = unsafe { Pin::new_unchecked(&state.scope) };
                scope.scope(&mut stage, call).map_err(|_| ERR_INVALID)??
            }
        };
        let status = match kind {
            Kind::Event if status > 0 => ERR_INVALID,
            Kind::Input if status > 1 => ERR_INVALID,
            _ => status,
        };
        for hook in state.ended.borrow().iter() {
            hook(status);
        }
        let mut commands = stage.into_commands();
        let mut dropped = 0;
        if status < 0 {
            dropped = commands.len();
            commands.clear();
        }
        let batches = state.batches.take();
        Ok(Callback {
            status,
            commands,
            batches,
            dropped,
        })
    }
}

impl Drop for Plugin<'_> {
    fn drop(&mut self) {
        // SAFETY: the table is valid until its own `drop` ran; called once.
        unsafe {
            let table = &*self.plugin;
            if let Some(drop) = table.drop {
                drop(table.state);
            }
        }
    }
}

/// The batch of exactly one record, as a callback's input: use it to send an
/// event (or an `Answered` reply to a question) to a plugin.
///
/// # Errors
/// The codec's status for a record it cannot encode.
pub fn event_batch(
    record: &paperpaw_core::native_host::registrations::Record<'_>,
) -> Result<Vec<u8>, i32> {
    let mut bytes = vec![0_u8; 4096];
    let len = paperpaw_core::native_host::registrations::records_encode(
        core::slice::from_ref(record),
        &mut bytes,
    )?;
    bytes.truncate(len);
    Ok(bytes)
}

/// Runs `scenario` once clean, then once per call of `slot` it made, each time
/// on a fresh host with that call refused with `status`. `scenario(host,
/// refused)` configures the host, loads and drives the plugin and returns what
/// the test wants to check; `refused` is `None` for the clean run and
/// `Some(n)` for the run with the n-th call of `slot` refused. The results
/// come back in order (the clean one first).
pub fn sweep<R>(
    slot: Slot,
    status: i32,
    mut scenario: impl FnMut(&mut ScriptedHost, Option<usize>) -> R,
) -> Vec<(Option<usize>, R)> {
    let mut results = Vec::new();
    let mut clean = ScriptedHost::new();
    let first = scenario(&mut clean, None);
    let calls = clean.calls(slot);
    results.push((None, first));
    for nth in 1..=calls {
        let mut host = ScriptedHost::new();
        host.refuse_nth(slot, nth, status);
        results.push((Some(nth), scenario(&mut host, Some(nth))));
    }
    results
}

/// As [`sweep`], and after every run (the scenario has dropped its plugin)
/// the host must hold no heap block and no service handle: a path that
/// leaves one fails the test, naming the refused call.
///
/// # Panics
/// A run left a block or a handle held.
pub fn sweep_released<R>(
    slot: Slot,
    status: i32,
    mut scenario: impl FnMut(&mut ScriptedHost, Option<usize>) -> R,
) -> Vec<(Option<usize>, R)> {
    let mut results = Vec::new();
    let mut clean = ScriptedHost::new();
    let first = scenario(&mut clean, None);
    assert_released(&clean, None);
    let calls = clean.calls(slot);
    results.push((None, first));
    for nth in 1..=calls {
        let mut host = ScriptedHost::new();
        host.refuse_nth(slot, nth, status);
        let result = scenario(&mut host, Some(nth));
        assert_released(&host, Some(nth));
        results.push((Some(nth), result));
    }
    results
}

fn assert_released(host: &ScriptedHost, refused: Option<usize>) {
    assert_eq!(
        (host.live_allocations(), host.open_handles()),
        (0, 0),
        "after the run with call {refused:?} refused: heap blocks and service handles still held"
    );
}
