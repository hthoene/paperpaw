//! Host-only ownership and production dispatch for flat plugins.
//!
//! Shells pin an owner before calling make. The context owns the host table and
//! allocator/service dependencies; the module guard keeps code mapped. Queues
//! and registrations live outside the owner and retain its borrowed leases.
//! No type here is a wire record or a foreign callback signature.

use crate::abi7::{calls, lifecycle};
use alloc::{boxed::Box, vec::Vec};
use core::{cell::Cell, fmt, mem::ManuallyDrop, ops::Deref, pin::Pin};
pub use lifecycle::{Teardown, TeardownStep};

#[path = "native_host/registration_scope.rs"]
pub mod registration_scope;

#[path = "native_host/codec.rs"]
pub(crate) mod codec;

#[path = "native_host/event_tags.rs"]
pub(crate) mod event_tags;
#[path = "native_host/owner_directory.rs"]
pub(crate) mod owner_directory;
#[path = "native_host/shrink.rs"]
pub(crate) mod shrink;

pub(crate) mod device_log;
#[allow(
    dead_code,
    reason = "the dual-boot request of the firmware queue uses it (Core todo 397)"
)]
pub(crate) mod dual_boot_ini;
pub(crate) mod dual_boot_job;
#[path = "native_host/font_removals.rs"]
pub(crate) mod font_removals;
pub(crate) mod registry_generation;

#[path = "native_host/work_sched.rs"]
#[allow(dead_code, reason = "the shell feeds the scheduler; tests do today")]
pub(crate) mod work_sched;

#[path = "native_host/work_service.rs"]
mod work_service;

#[path = "native_host/firmware_confirm.rs"]
#[allow(
    dead_code,
    reason = "the pump opens it once a shell wires the firmware host; tests do today"
)]
pub(crate) mod firmware_confirm;
#[path = "native_host/firmware_driver.rs"]
#[allow(
    dead_code,
    reason = "the firmware commands and the kernel's pump wire it (slice B2a-2); tests do today"
)]
pub(crate) mod firmware_driver;
#[path = "native_host/firmware_events.rs"]
#[allow(
    dead_code,
    reason = "the shell's main loop calls the pump (slice B2a-2 wiring); tests do today"
)]
mod firmware_events;
#[path = "native_host/firmware_install.rs"]
#[allow(
    dead_code,
    reason = "the command arms and the pump (slice B) drive the machine; tests do today"
)]
pub(crate) mod firmware_install;
#[path = "native_host/notice.rs"]
pub(crate) mod notice;

#[path = "native_host/install_grants.rs"]
pub(crate) mod install_grants;
#[path = "native_host/plugin_check.rs"]
#[allow(
    dead_code,
    reason = "the queue's plugin seam wires the job (slice B2c-1 wiring after Core's queue enum); tests do today"
)]
pub(crate) mod plugin_check;
#[path = "native_host/plugin_confirm.rs"]
pub(crate) mod plugin_confirm;
#[path = "native_host/plugin_folder.rs"]
#[allow(dead_code, reason = "the plugin jobs use it; see plugin_check")]
pub(crate) mod plugin_folder;
#[path = "native_host/plugin_install.rs"]
#[allow(
    dead_code,
    reason = "the queue's plugin seam wires it; see plugin_check"
)]
pub(crate) mod plugin_install;
#[path = "native_host/plugin_job.rs"]
#[allow(
    dead_code,
    reason = "the queue's plugin seam wires it; see plugin_check"
)]
pub(crate) mod plugin_job;
#[path = "native_host/plugin_jobs.rs"]
pub(crate) mod plugin_jobs;
#[path = "native_host/plugin_remove.rs"]
#[allow(dead_code, reason = "the queue and the start use it; see plugin_check")]
pub(crate) mod plugin_remove;
#[path = "native_host/plugin_rules.rs"]
#[allow(dead_code, reason = "the install job applies it; see plugin_check")]
pub(crate) mod plugin_rules;
#[path = "native_host/plugin_start.rs"]
#[allow(dead_code, reason = "the builder runs it; see plugin_check")]
pub(crate) mod plugin_start;
#[path = "native_host/plugin_swap.rs"]
#[allow(
    dead_code,
    reason = "the install job runs it and the start replays it; see plugin_check"
)]
pub(crate) mod plugin_swap;
#[path = "native_host/plugin_trust.rs"]
#[allow(dead_code, reason = "the plugin jobs use it; see plugin_check")]
pub(crate) mod plugin_trust;
#[path = "native_host/plugin_unforgotten.rs"]
pub(crate) mod plugin_unforgotten;

#[path = "native_host/secrets_file.rs"]
pub(crate) mod secrets_file;
pub(crate) mod secrets_text;

#[path = "native_host/secrets_service.rs"]
#[allow(
    dead_code,
    reason = "the shell wires the sealer and the source; tests do today"
)]
pub(crate) mod secrets_service;

#[path = "native_host/host_slot.rs"]
pub mod host_slot;

#[path = "native_host/frame_owner.rs"]
pub(crate) mod frame_owner;

#[path = "native_host/plugin_heap.rs"]
pub(crate) mod plugin_heap;

#[path = "native_host/screen_ready.rs"]
mod screen_ready;

#[path = "native_host/ui_service.rs"]
pub(crate) mod ui_service;
pub use ui_service::check_ask;

#[path = "native_host/ui_clipboard.rs"]
pub(crate) mod ui_clipboard;

#[path = "native_host/ui_hints.rs"]
pub(crate) mod ui_hints;

#[path = "native_host/ui_stack_op.rs"]
pub(crate) mod ui_stack_op;

#[path = "native_host/ui_requests.rs"]
#[allow(
    dead_code,
    reason = "consumers are Core's stack and the core.ui ops 8, 2, 11"
)]
pub(crate) mod ui_requests;

#[path = "native_host/ui_store.rs"]
#[allow(
    dead_code,
    reason = "consumers are Core's command stage and the core.ui ops"
)]
pub(crate) mod ui_store;

#[path = "native_host/ui_compose.rs"]
pub(crate) mod ui_compose;

#[path = "native_host/ui_handles.rs"]
#[allow(
    dead_code,
    reason = "consumers are the core.ui ops 4/14/15 and the compose hook"
)]
pub(crate) mod ui_handles;

#[path = "native_host/stack_rows.rs"]
pub(crate) mod stack_rows;

pub(crate) use crate::abi7::context::ui_guard;

#[path = "native_host/ui_widget_layout.rs"]
pub(crate) mod ui_widget_layout;

#[path = "native_host/ui_widget_ops.rs"]
pub(crate) mod ui_widget_ops;

#[path = "native_host/ui_compose_host.rs"]
#[allow(
    dead_code,
    reason = "Core's frame answers Frame::compose with it when the screen stack is wired"
)]
pub(crate) mod ui_compose_host;

#[path = "native_host/input_chain.rs"]
pub(crate) mod input_chain;
#[path = "native_host/ui_input.rs"]
#[allow(
    dead_code,
    reason = "the router hook (todo 76) and the core.input-filter registration admission call it"
)]
pub(crate) mod ui_input;

#[path = "native_host/shared.rs"]
pub(crate) mod shared;

#[path = "native_host/resources.rs"]
pub mod resources;
pub use fixed_settings::FixedTables;
pub use resources::HostResources;

#[path = "native_host/shell_wiring.rs"]
pub mod shell_wiring;
pub use shell_wiring::{HostWiring, HyphenTables, WiredHosts};

#[path = "native_host/fonts.rs"]
mod fonts;
#[path = "native_host/fonts_data.rs"]
mod fonts_data;

mod admitted_features;
#[path = "native_host/secret_bytes.rs"]
pub(crate) mod secret_bytes;

pub(crate) mod book_data_steps;
pub(crate) mod book_scope;
pub(crate) mod busy_adapters;
#[path = "native_host/call_services.rs"]
mod call_services;
mod cover_job_marker;
mod cover_job_pump;
pub(crate) mod data_plugin_folders;
#[path = "native_host/data_services.rs"]
mod data_services;
mod declared_services;
mod details_job_pump;
pub mod device_service;
pub(crate) mod dialog_pump;
#[path = "native_host/dialogs.rs"]
pub(crate) mod dialogs;
pub(crate) mod doc_cache;
pub(crate) mod doc_host;
mod doc_pump;
pub(crate) mod doc_reading;
#[allow(dead_code, reason = "wired into the shell frame by the DOC adapter")]
pub(crate) mod doc_service;
pub(crate) mod event_call;
pub(crate) mod event_failures;
#[allow(dead_code, reason = "wired into the shell frame by the files adapter")]
pub(crate) mod files_service;
#[allow(dead_code, reason = "wired into the shell frame by the DOC adapter")]
pub(crate) mod fmt_call;
#[allow(dead_code, reason = "wired into the shell frame by the DOC adapter")]
pub(crate) mod format_call;
#[allow(dead_code, reason = "wired into the shell frame by the DOC adapter")]
pub(crate) mod format_index;
#[allow(
    dead_code,
    reason = "entered by the DOC adapter's format calls; read by the file service"
)]
pub(crate) mod format_scope;
#[path = "native_host/formatting.rs"]
pub mod formatting;
pub(crate) mod hint_labels;
pub(crate) mod host_services;
#[allow(
    dead_code,
    reason = "wired into the shell table; a shell wires its host view"
)]
pub(crate) mod hyphen_card;
pub(crate) mod hyphen_service;
pub(crate) mod image_pump;
#[allow(
    dead_code,
    reason = "wired into the shell table; the DOC, LIB and prepare producers fill it"
)]
pub(crate) mod image_service;
pub(crate) mod lib_changed;
pub(crate) mod lib_cover;
#[allow(
    dead_code,
    reason = "wired by HostWiring::library; the write half is todo 307"
)]
pub(crate) mod lib_edits;
pub(crate) mod lib_host;
pub(crate) mod lib_import;
pub(crate) mod lib_jobs;
pub(crate) mod lib_jobs_env;
pub(crate) mod lib_jobs_journal;
pub(crate) mod lib_moves_carry;
pub(crate) mod lib_moves_host;
pub(crate) mod lib_moves_journal;
pub(crate) mod lib_moves_service;
pub(crate) mod lib_moves_walk;
#[allow(dead_code, reason = "wired into the shell frame by the LIB adapter")]
pub(crate) mod lib_reads;
#[allow(dead_code, reason = "wired into the shell frame by the LIB adapter")]
pub(crate) mod lib_service;
mod library_events;
pub(crate) mod linked_plugins;
#[allow(dead_code, reason = "wired into the shell frame by the lookup adapter")]
pub(crate) mod lookup_service;
mod moves_pump;
pub(crate) mod native_book_formats;
pub(crate) mod native_host_call;
#[allow(
    dead_code,
    reason = "the shell builds the pool with the NET host and the SOCK host charges it"
)]
pub(crate) mod net_policy;
pub(crate) mod net_pool;
#[allow(dead_code, reason = "wired into the shell frame by the NET adapter")]
pub(crate) mod net_service;
mod network_demand;
pub(crate) mod owner_paths;
pub(crate) mod paths_in_use;
mod reading_events;
pub(crate) mod save_files;
pub(crate) mod screen_apply;
pub(crate) mod screen_stage;
pub(crate) mod service_events;
mod service_pump;
mod service_slots;
#[path = "native_host/setting_events.rs"]
pub(crate) mod setting_events;
#[allow(dead_code, reason = "wired into the shell frame by the SOCK adapter")]
pub(crate) mod sock_service;
pub(crate) mod speech;
mod stack_events;
#[allow(
    dead_code,
    reason = "the native stage.run host (#828) frames its replies with it"
)]
pub(crate) mod stage_reply_tail;
#[allow(
    dead_code,
    reason = "stage broker wiring follows this framing foundation"
)]
mod stage_wire;
#[allow(
    dead_code,
    reason = "the NET and SOCK adapters name their staged file by it"
)]
pub(crate) mod staged_name;
mod storage_files;
#[path = "native_host/storage_slots.rs"]
mod storage_slots;
mod widget_events;
mod widget_inputs;
mod wifi_events;
mod work_pump;

/// Bit 0 (`menus`) of `event.registry_changed`: menus and their item state.
pub(crate) const REGISTRY_MENUS: u32 = 1;
/// Every `what` bit of `event.registry_changed` (`registry.md` section 5): a
/// change of the plugin set may change anything the registry holds.
pub(crate) const REGISTRY_ALL: u32 = 0xFF;
/// Bit 31 (`dropped`) of `event.registry_changed`: a staged REG batch of the
/// plugin's own was dropped when applied. It goes to that plugin only.
pub(crate) const REGISTRY_DROPPED: u32 = 1 << 31;

/// The most callback registrations (event subscriptions and draws) one
/// plugin makes over the whole construction. Records live in a `Vec` grown
/// per batch, so the heap cost is what the plugin registered.
const REGISTRATIONS: usize = 32;

/// Callback-scoped drawing for host-owned plugin contexts.
#[path = "native_host/canvas.rs"]
pub mod canvas;
mod canvas_describe;

/// Callback-scoped slot-15 command staging for host-owned plugin contexts.
#[path = "native_host/commands.rs"]
pub mod commands;

#[cfg(test)]
#[path = "native_host/plugin_answers_tests.rs"]
mod plugin_answers_tests;
pub(crate) mod plugin_requests;
#[cfg(test)]
#[path = "native_host/plugin_requests_tests.rs"]
mod plugin_requests_tests;
/// The device-wide table of Wi-Fi join and host requests (DEV).
#[path = "native_host/radio_requests.rs"]
pub mod radio_requests;

#[path = "native_host/declaration_index.rs"]
mod declaration_index;

#[path = "native_host/declaration_checks.rs"]
mod declaration_checks;
mod declaration_refusals;

#[path = "native_host/declarations_uix.rs"]
mod declarations_uix;

#[path = "native_host/declarations_input.rs"]
mod declarations_input;

#[path = "native_host/declaration_reg_checks.rs"]
mod declaration_reg_checks;

#[path = "native_host/reg_catalog.rs"]
#[allow(
    dead_code,
    reason = "REG callback attachment follows the catalog ownership implementation"
)]
pub(crate) mod reg_catalog;

#[path = "native_host/raw_declarations.rs"]
mod raw_declarations;

// Kept crate-private until Kernel's draft-4 SDK source contract lands.
#[allow(
    dead_code,
    reason = "draft-4 SDK registry wiring follows the tested substrate"
)]
#[path = "native_host/declaration_store.rs"]
pub(crate) mod declaration_store;

/// Registry-free syntactic validation of declaration records.
#[path = "native_host/declarations.rs"]
pub mod declarations;
/// Scoped environment copy-out for host-owned plugin contexts.
#[path = "native_host/environment.rs"]
pub mod environment;

/// Interface-7 event records (tags 32-39): host encode, decode and admission.
#[path = "native_host/events.rs"]
pub mod events;
// State is attached to Core's single registry when its publication seam lands.
#[allow(
    dead_code,
    reason = "Core owns production runtime attachment and outermost draining"
)]
#[path = "native_host/event_pending.rs"]
pub(crate) mod event_pending;
#[allow(
    dead_code,
    reason = "Core owns production runtime attachment and outermost draining"
)]
#[path = "native_host/event_producers.rs"]
pub(crate) mod event_producers;

#[allow(
    dead_code,
    reason = "native runtime installation and EVT producer hooks follow this accessor"
)]
#[path = "native_host/event_sink.rs"]
mod event_sink;
/// Production event adapter for a shell-owned flat plugin instance.
#[path = "native_host/flat.rs"]
pub mod flat;
/// The `draw.frame` input of `draw` (N2, `docs/abi/screens-input.md` section 6).
#[path = "native_host/frames.rs"]
pub mod frames;

/// Bounded decoded registration submission during plugin construction.
#[path = "native_host/registrations.rs"]
pub mod registrations;
#[allow(
    dead_code,
    reason = "native built-in registry adapter follows this owner holder"
)]
#[path = "native_host/retained.rs"]
mod retained;

#[path = "native_host/item_state.rs"]
#[allow(
    dead_code,
    reason = "the registry and the production native command path read and fill it"
)]
pub(crate) mod item_state;

#[path = "native_host/native_declarations.rs"]
#[allow(
    dead_code,
    reason = "shared declaration producer is attached by the UI admission slice"
)]
pub(crate) mod native_declarations;

#[path = "native_host/native_widget.rs"]
#[allow(
    dead_code,
    reason = "Frame::widget_call carries ops 7, 12 and 13 of core.ui"
)]
pub(crate) mod native_widget;

#[path = "native_host/native_status.rs"]
#[allow(
    dead_code,
    reason = "Frame hook attachment follows the owner-directed status path"
)]
pub(crate) mod native_status;

#[path = "native_host/core_events.rs"]
pub(crate) mod core_events;
#[path = "native_host/native_call.rs"]
#[allow(
    dead_code,
    reason = "the document and lookup adapters call it through Frame::call_native"
)]
pub(crate) mod native_call;
#[path = "native_host/native_keys.rs"]
mod native_keys;

#[path = "native_host/native_menus.rs"]
#[allow(
    dead_code,
    reason = "core.registry op 11 (Core) reads the declared menus through it"
)]
pub(crate) mod native_menus;
#[path = "native_host/native_tick.rs"]
pub(crate) mod native_tick;
#[path = "native_host/native_ui_directory.rs"]
mod native_ui_directory;
#[path = "native_host/widget_drops.rs"]
mod widget_drops;

#[path = "native_host/fixed_settings.rs"]
mod fixed_settings;
#[path = "native_host/native_enable.rs"]
pub mod native_enable;
#[path = "native_host/native_install.rs"]
mod native_install;
mod native_resources;

#[path = "native_host/native_action.rs"]
pub(crate) mod native_action;
#[path = "native_host/native_screen_factory.rs"]
pub(crate) mod native_screen_factory;
mod owned_texts;

#[path = "native_host/native_screen.rs"]
mod native_screen;

#[allow(
    dead_code,
    reason = "native built-in registry adapter follows these owned values"
)]
#[path = "native_host/native_values.rs"]
mod native_values;

#[allow(
    dead_code,
    reason = "native registry publication follows this dispatcher"
)]
#[path = "native_host/native_dispatch.rs"]
mod native_dispatch;

#[path = "native_host/lease_slot.rs"]
mod lease_slot;
#[path = "native_host/native_callbacks.rs"]
mod native_callbacks;
#[path = "native_host/park_linked.rs"]
pub(crate) mod park_linked;
#[path = "native_host/parked.rs"]
mod parked;

#[allow(
    dead_code,
    reason = "production native activation and stage broker follow this registry"
)]
#[path = "native_host/native_instances.rs"]
pub(crate) mod native_instances;

// Static-entry activation uses the shared D1 reader; registry wiring follows.
#[allow(
    dead_code,
    reason = "native built-in registry adapter follows this prerequisite"
)]
#[path = "native_host/linked_entry.rs"]
pub(crate) mod linked_entry;
#[path = "native_host/linked_install.rs"]
mod linked_install;

/// The shell's side of one hosted plugin (owner, host table, `make`,
/// scoped dispatch), shared by the desktop and the X4.
#[path = "native_host/shell.rs"]
pub mod shell;
/// Host-owned copies of declaration records staged during construction.
#[path = "native_host/staged.rs"]
pub mod staged;
/// Interface-7 storage slots 20-26 over the platform storage (not wired).
#[path = "native_host/storage.rs"]
pub mod storage;

use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_NO_SPACE};
use registrations::{Record, RecordBatch, Registration};
use staged::Declarations;

/// Kernel-owned dependencies supplied to a shell's code loader.
/// Only the kernel constructs this handle; adapters retain its shared meter.
#[non_exhaustive]
#[derive(Debug)]
pub struct FlatHost {
    pub(crate) stack_rows: alloc::rc::Rc<stack_rows::StackRows>,
    pub(crate) directory: Option<native_declarations::DirectoryWeak>,
    pub(crate) meter: alloc::rc::Rc<crate::meter::Meter>,
    pub(crate) font_removals: alloc::rc::Rc<font_removals::FontRemovals>,
    pub(crate) registry_generation: alloc::rc::Rc<registry_generation::RegistryGeneration>,
    pub(crate) log: device_log::DeviceLog,
}

impl FlatHost {
    pub(crate) fn upgrade_directory(
        &self,
    ) -> Result<Option<native_declarations::DirectoryHandle>, i32> {
        match &self.directory {
            None => Ok(None),
            Some(directory) => directory
                .upgrade()
                .map_err(|_| paperpaw_native::host_glue::ERR_STATE)?
                .map(Some)
                .ok_or(paperpaw_native::host_glue::ERR_STATE),
        }
    }
    pub(crate) fn try_directory(&self) -> Result<Option<native_declarations::DirectoryWeak>, i32> {
        self.directory
            .as_ref()
            .map(|directory| directory.try_clone().ok_or(ERR_NO_MEMORY))
            .transpose()
    }
}

impl crate::Kernel {
    pub(crate) fn flat_host(&self) -> FlatHost {
        FlatHost {
            meter: alloc::rc::Rc::clone(&self.shared.meter),
            directory: None,
            stack_rows: alloc::rc::Rc::clone(&self.shared.stack_rows),
            font_removals: alloc::rc::Rc::clone(&self.shared.font_removals),
            registry_generation: alloc::rc::Rc::clone(&self.shared.registry_generation),
            log: self.shared.device_log,
        }
    }
}

/// Host admission or construction failure.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Registration decoding or validation refused the construction transaction.
    Invalid,
    /// The owner is not in the required lifetime phase.
    State,
    /// Construction, callback execution or retained leases prevent this action.
    Busy,
    /// The bounded registration or lease capacity is exhausted.
    Capacity,
    /// The device-wide callback nesting limit is exhausted.
    Depth,
    /// Host memory for the staged records ran out.
    Memory,
    /// Temporary: removed when the draft-4 SDK declaration views land.
    /// Image declarations cannot activate before those views are wired.
    DeclarationViewsUnavailable,
}

impl From<lifecycle::Error> for Error {
    fn from(error: lifecycle::Error) -> Self {
        match error {
            lifecycle::Error::State => Self::State,
            lifecycle::Error::Busy => Self::Busy,
            lifecycle::Error::Capacity => Self::Capacity,
            lifecycle::Error::Depth => Self::Depth,
        }
    }
}

/// One depth budget shared across all owners on the dispatch thread.
#[non_exhaustive]
pub struct DispatchDepth(calls::Depth);

impl DispatchDepth {
    pub(crate) fn current(&self) -> u8 {
        self.0.current()
    }

    /// Construct an idle budget. Owners must share this value during nesting.
    #[must_use]
    pub const fn new() -> Self {
        Self(calls::Depth::new())
    }
}

impl Default for DispatchDepth {
    fn default() -> Self {
        Self::new()
    }
}

/// Owns the pinned allocation and retains it if plugin code is still active.
/// Shell holders must use this instead of dropping a raw pinned owner Box.
#[allow(
    missing_debug_implementations,
    reason = "holds opaque context and module"
)]
#[must_use]
pub struct PinnedOwner<C, M>(ManuallyDrop<Pin<Box<HostOwner<C, M>>>>);

impl<C, M> PinnedOwner<C, M> {
    /// Allocate the owner fallibly, keeping the pinned Box private forever.
    ///
    /// Returns None if the actual owner allocation fails.
    pub fn try_new(owner: HostOwner<C, M>) -> Option<Self> {
        const { assert!(core::mem::size_of::<HostOwner<C, M>>() != 0) };
        let layout = core::alloc::Layout::new::<HostOwner<C, M>>();
        // SAFETY: lifecycle flags make the owner nonzero-sized; layout is exact.
        let allocation = unsafe { alloc::alloc::alloc(layout) }.cast::<HostOwner<C, M>>();
        if allocation.is_null() {
            return None;
        }
        // SAFETY: uniquely allocated Box storage, initialized before pinning.
        Some(Self(ManuallyDrop::new(unsafe {
            allocation.write(owner);
            Box::into_pin(Box::from_raw(allocation))
        })))
    }

    /// Borrow the pinned owner without exposing ownership of its allocation.
    #[must_use]
    pub fn as_ref(&self) -> Pin<&HostOwner<C, M>> {
        self.0.as_ref()
    }
}

impl<C, M> Deref for PinnedOwner<C, M> {
    type Target = HostOwner<C, M>;
    fn deref(&self) -> &Self::Target {
        self.as_ref().get_ref()
    }
}

impl<C, M> Drop for PinnedOwner<C, M> {
    fn drop(&mut self) {
        if self.0.as_ref().inner().abandon_if_active() {
            // Retain the entire allocation: ctx still points inside it.
            return;
        }
        // SAFETY: inactive allocation, released exactly once by this holder.
        unsafe { ManuallyDrop::drop(&mut self.0) };
    }
}

/// Pinned host context and opaque executable-module guard for one plugin.
/// Allocate fallibly and account for C, M and registration storage externally.
/// The shell must prepare teardown, remove subscriptions and queued work, then
/// finish before dropping the owner. Drop is a last-resort host bug path; its
/// hooks must own only host resources that cannot reference plugin state,
/// executable code or its mapped image.
#[non_exhaustive]
pub struct HostOwner<C, M> {
    inner: lifecycle::Owner<C, M>,
    preparing: Cell<bool>,
}

impl<C, M> HostOwner<C, M> {
    /// Own the context and module without calling plugin code or allocating.
    #[must_use]
    pub const fn new(context: C, module: M) -> Self {
        Self {
            inner: lifecycle::Owner::new(context, module),
            preparing: Cell::new(false),
        }
    }

    fn inner(self: Pin<&Self>) -> Pin<&lifecycle::Owner<C, M>> {
        // SAFETY: inner is structurally pinned and is never moved by this API.
        unsafe { self.map_unchecked(|owner| &owner.inner) }
    }

    /// Stable context view for make and host-service forwarding.
    #[must_use]
    pub fn context(self: Pin<&Self>) -> Pin<&C> {
        self.inner().context()
    }

    /// Begin a bounded host-side transaction before invoking make.
    /// No staged record can dispatch until commit after result validation.
    /// `plugin_id` is the host-known id that declaration names are checked
    /// against; it must outlive the transaction.
    ///
    /// # Errors
    /// Returns State outside construction, or Busy for an existing transaction.
    pub fn begin<'a, T>(
        self: Pin<&'a Self>,
        plugin_id: &'a str,
    ) -> Result<Construction<'a, C, M, T>, Error> {
        if !self.inner.is_constructing() {
            return Err(Error::State);
        }
        if self.preparing.replace(true) {
            return Err(Error::Busy);
        }
        Ok(Construction {
            owner: self,
            plugin_id,
            records: Vec::new(),
            declarations: Declarations::default(),
            raw: raw_declarations::RawDeclarations::default(),
            failure: None,
            image: None,
            submitted: false,
            refused: None,
        })
    }

    /// Retain a live owner for a queued callback outside its registrations.
    ///
    /// # Errors
    /// Returns State unless live, or Capacity when all lease slots are held.
    ///
    /// A safe lease cannot escape the host that owns its pinned allocation.
    /// ```compile_fail,E0515
    /// use paperpaw_core::native_host::{CallbackLease, shell::{Context, Hosted}};
    /// use paperpaw_native::host_glue::Services;
    /// fn escape<S: Services + 'static, M: 'static>(host: Hosted<S, M>)
    ///     -> CallbackLease<'static, Context<S>, M>
    /// {
    ///     host.owner().lease().unwrap()
    /// }
    /// ```
    pub fn lease(self: Pin<&Self>) -> Result<CallbackLease<'_, C, M>, Error> {
        Ok(CallbackLease(self.inner().lease()?))
    }

    /// Stop callback admission before removing registrations and queued work.
    pub fn revoke(self: Pin<&Self>) {
        self.inner().revoke();
    }

    /// Whether this owner is inside a callback. After revoke, resource cleanup
    /// must wait for the active call to return; retained leases are separate.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    /// Whether callback admission remains open, including between callbacks.
    pub(crate) fn is_live(&self) -> bool {
        self.inner.is_live()
    }

    /// Revoke immediately and run cancellation then release once, unless a
    /// callback is active. The shell must defer Busy to its next tick. After
    /// success, remove subscriptions and queued work before calling finish.
    ///
    /// # Errors
    /// Returns Busy during construction or an active callback.
    pub fn prepare_teardown(self: Pin<&Self>) -> Result<(), Error> {
        self.revoke();
        if self.preparing.get() {
            return Err(Error::Busy);
        }
        self.inner().prepare_teardown().map_err(Into::into)
    }

    /// Take a non-retryable cleanup-hook error for shell logging. Such a hook
    /// is marked done, so a broken backend cannot cause an infinite retry.
    #[must_use]
    pub fn take_teardown_error(&self) -> Option<Error> {
        self.inner.take_teardown_error()
    }

    /// Destroy the plugin once all leases have been released.
    /// Host context, allocator and module remain alive throughout destruction.
    ///
    /// # Errors
    /// Returns Busy while construction or leases remain, or State before revoke.
    pub fn finish(self: Pin<&Self>) -> Result<(), Error> {
        if self.preparing.get() {
            return Err(Error::Busy);
        }
        self.inner().finish().map_err(Into::into)
    }
}

/// Staged host registration records retained under the constructing owner.
/// Drop rolls back records before releasing the construction borrow. The
/// loader remains responsible for partial foreign output from failed make.
#[non_exhaustive]
pub struct Construction<'a, C, M, T> {
    owner: Pin<&'a HostOwner<C, M>>,
    plugin_id: &'a str,
    records: Vec<T>,
    declarations: Declarations,
    raw: raw_declarations::RawDeclarations,
    failure: Option<Error>,
    image: Option<&'a [u8]>,
    submitted: bool,
    /// The declaration cap the batch was refused at, once `finish` said so.
    refused: Option<declaration_checks::Cap>,
}

type StoredConstruction<'a, C, M, T, R> = (
    RegistrationBatch<'a, C, M, T>,
    R,
    raw_declarations::RawDeclarations,
);

impl<'a, C, M, T> Construction<'a, C, M, T> {
    /// Supply the module's immutable read-only image before make submits data.
    /// The borrow remains live through the committed registration batch.
    /// The shell must keep the mapping alive until the batch and owner leases
    /// are dropped, including when it extends a mapped image's lifetime.
    /// Post-construction access must use the live owner's image visitor and
    /// generation checks. A stopped owner supplies no image.
    ///
    /// # Errors
    /// Returns State after any submission or a previous image installation.
    pub fn set_image(&mut self, image: &'a [u8]) -> Result<(), Error> {
        if self.submitted || self.image.is_some() || self.failure.is_some() {
            return Err(Error::State);
        }
        self.image = Some(image);
        Ok(())
    }

    fn declaration_view(&self) -> declaration_checks::View<'_> {
        declaration_checks::View {
            raw: &self.raw,
            owned: &self.declarations,
            image: self.image,
            plugin: self.plugin_id,
            refused: core::cell::Cell::new(None),
        }
    }

    fn finish_declarations(&mut self) -> Result<(), i32> {
        self.raw.sort_index();
        let view = self.declaration_view();
        let result = view.finish();
        self.refused = view.refused.get();
        result
    }

    /// Poison this transaction after any registration decoding or validation
    /// refusal, even if foreign code ignores the returned status. Preserve an
    /// earlier staging failure. Only a new transaction can clear this latch.
    pub fn reject(&mut self) {
        self.reject_with(Error::Invalid);
    }

    /// [`reject`](Self::reject) with a cause other than Invalid, such as
    /// Memory when the host could not allocate to decode a batch.
    pub fn reject_with(&mut self, cause: Error) {
        if self.failure.is_none() {
            self.failure = Some(cause);
        }
    }

    /// Stage a validated host record, returning it untouched on refusal.
    /// Generic T is not a wire schema; decoding and retained-byte limits apply
    /// before calling this method. 32 records is the count cap per plugin.
    ///
    /// # Errors
    /// Returns Capacity with the unchanged record if all 32 slots are filled,
    /// Memory (latched, record returned) if the room for it cannot be
    /// allocated, or the already latched failure if this transaction was
    /// rejected.
    pub fn stage(&mut self, record: T) -> Result<(), (Error, T)> {
        self.submitted = true;
        if let Some(error) = self.failure {
            return Err((error, record));
        }
        if self.records.len() == REGISTRATIONS {
            self.failure = Some(Error::Capacity);
            return Err((Error::Capacity, record));
        }
        if self.records.try_reserve_exact(1).is_err() {
            self.failure = Some(Error::Memory);
            return Err((Error::Memory, record));
        }
        self.records.push(record);
        Ok(())
    }

    /// Transfer a validated successful instance and publish its records.
    /// Store the successful make output in C first. Only Ok transfers its
    /// destruction to this owner. The destructor must not unwind or reenter.
    ///
    /// # Errors
    /// Returns Invalid after validation refusal or when the staged declarations
    /// fail their cross-record checks ([`Declarations::finish`]), Capacity if staging overflowed
    /// a slot, count or byte limit, Memory if staging ran out of memory, or State if revoked
    /// before commit. DeclarationViewsUnavailable refuses wire declarations
    /// until the draft-4 SDK registry views are wired.
    /// Every Err drops staged records before returning and leaves destruction
    /// unarmed. The loader still owns its validated successful output and must
    /// destroy it exactly once while context and module remain alive. This is
    /// distinct from cleaning partial or invalid output from a failed make.
    pub fn commit(
        mut self,
        teardown: Teardown<C>,
    ) -> Result<RegistrationBatch<'a, C, M, T>, Error> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        // Cross-record checks come first: a refusal leaves destruction unarmed.
        self.finish_declarations().map_err(|_| Error::Invalid)?;
        if !self.raw.batches.is_empty() {
            return Err(Error::DeclarationViewsUnavailable);
        }
        self.publish(teardown)
    }

    /// Validate and map every native declaration before callback activation.
    /// Mapping returns only host-owned staging; nothing is visible on failure.
    #[allow(
        dead_code,
        reason = "native built-in registry adapter follows this transaction"
    )]
    pub(crate) fn commit_native<R>(
        self,
        teardown: Teardown<C>,
        map: impl FnOnce(&declaration_checks::View<'_>) -> Result<R, i32>,
    ) -> Result<(RegistrationBatch<'a, C, M, T>, R), i32> {
        let (batch, staging, raw) = self.commit_native_stored(teardown, map, |_, _| {})?;
        drop(raw);
        Ok((batch, staging))
    }

    /// Transfer original declaration batches to the shared store's producer.
    /// No address from a runtime submission survives its construction call.
    pub(crate) fn commit_native_stored<R>(
        mut self,
        teardown: Teardown<C>,
        map: impl FnOnce(&declaration_checks::View<'_>) -> Result<R, i32>,
        refused: impl FnOnce(&str, declaration_checks::Cap),
    ) -> Result<StoredConstruction<'a, C, M, T, R>, i32> {
        if let Some(error) = self.failure {
            return Err(registrations::status(error));
        }
        if let Err(status) = self.finish_declarations() {
            if let Some(cap) = self.refused {
                refused(self.plugin_id, cap);
            }
            return Err(status);
        }
        let staging = map(&self.declaration_view())?;
        self.declarations = Declarations::default();
        let raw = core::mem::take(&mut self.raw);
        let batch = self.publish(teardown).map_err(registrations::status)?;
        Ok((batch, staging, raw))
    }

    fn publish(mut self, teardown: Teardown<C>) -> Result<RegistrationBatch<'a, C, M, T>, Error> {
        let lease = CallbackLease(self.owner.inner().activate_leased(teardown)?);
        Ok(RegistrationBatch {
            declarations: core::mem::take(&mut self.declarations),
            raw: core::mem::take(&mut self.raw),
            records: PublishedRecords {
                records: core::mem::take(&mut self.records),
                _image: core::marker::PhantomData,
                lease,
            },
        })
    }

    /// Decoded-only host declarations staged so far. Wire submissions are kept
    /// separately as raw bytes or image ranges and do not appear here.
    #[must_use]
    pub const fn declarations(&self) -> &Declarations {
        &self.declarations
    }

    /// Stage a decoded, mixed batch: its callback registrations (converted by
    /// `convert`) and its declarations, all or nothing for the batch.
    /// Capacity for both is checked before either is changed. A refusal latches
    /// the transaction: Capacity when the 32 callback slots, a declaration
    /// count or the byte budget would overflow, Memory when an allocation fails,
    /// Invalid for any other declaration refusal.
    ///
    /// # Errors
    /// The `ERR_*` status of the refusal, or of the earlier latched failure.
    pub fn stage_batch(
        &mut self,
        batch: &RecordBatch<'_>,
        convert: fn(Registration) -> T,
    ) -> Result<(), i32> {
        self.stage_with_raw(batch, convert, None)
    }

    fn stage_with_raw(
        &mut self,
        batch: &RecordBatch<'_>,
        convert: fn(Registration) -> T,
        wire: Option<&[u8]>,
    ) -> Result<(), i32> {
        if let Some(error) = self.failure {
            return Err(registrations::status(error));
        }
        self.submitted = true;
        let result = self.stage_inner(batch, convert, wire);
        if let Err(code) = result {
            self.failure = Some(match code {
                ERR_NO_SPACE => Error::Capacity,
                ERR_NO_MEMORY => Error::Memory,
                _ => Error::Invalid,
            });
        }
        result
    }

    fn stage_inner(
        &mut self,
        batch: &RecordBatch<'_>,
        convert: fn(Registration) -> T,
        wire: Option<&[u8]>,
    ) -> Result<(), i32> {
        let tokens = batch
            .iter()
            .filter(|record| matches!(record, Record::Registration(_)))
            .count();
        if self.records.len() + tokens > REGISTRATIONS {
            return Err(ERR_NO_SPACE);
        }
        // Room for the records first: a refusal here changes nothing staged.
        self.records
            .try_reserve_exact(tokens)
            .map_err(|_| ERR_NO_MEMORY)?;
        declarations::validate_batch(self.plugin_id, batch)?;
        let raw = self
            .raw
            .prepare_retaining(self.image, wire, batch, self.declarations.bytes())?;
        self.raw
            .check_counts(&self.declarations, batch, raw.as_ref())?;
        let retained = self.raw.retained(raw.as_ref());
        if raw.is_none() {
            self.declarations
                .stage_retaining(self.plugin_id, batch, retained)?;
        } else if self
            .declarations
            .bytes()
            .checked_add(retained)
            .is_none_or(|n| n > staged::DECLARATIONS_MAX_BYTES)
        {
            return Err(ERR_NO_SPACE);
        }
        self.raw.commit(raw);
        for record in batch.iter() {
            if let Record::Registration(registration) = record {
                // The room was reserved above; the cap was checked there too.
                self.records.push(convert(registration));
            }
        }
        Ok(())
    }

    /// Discard staged records while context and code remain alive.
    pub fn abort(self) {
        drop(self);
    }

    fn clear(&mut self) {
        self.records.clear();
    }
}

impl<C, M, T> Drop for Construction<'_, C, M, T> {
    fn drop(&mut self) {
        // Keep construction excluded during arbitrary host-record destruction.
        self.clear();
        self.owner.preparing.set(false);
    }
}

/// A committed batch: its records with their lease, and the declarations
/// staged beside them, which the caller moves out before it retains the
/// records (`into_retained` keeps only those).
/// Declarations drop before the records, and the lease drops last.
#[non_exhaustive]
pub struct RegistrationBatch<'a, C, M, T> {
    declarations: Declarations,
    raw: raw_declarations::RawDeclarations,
    records: PublishedRecords<'a, C, M, T>,
}

impl<C, M, T> RegistrationBatch<'_, C, M, T> {
    /// The records alone, for as long as the caller retains the pinned owner
    /// and image. What is left of the declarations is dropped here: a retained
    /// batch keeps no room for them (they are moved out before, or unused).
    ///
    /// # Safety
    /// The caller retains the pinned owner and image until the result drops.
    unsafe fn into_retained(self) -> PublishedRecords<'static, C, M, T>
    where
        T: 'static,
    {
        let records = self.records;
        PublishedRecords {
            records: records.records,
            _image: core::marker::PhantomData,
            // SAFETY: the caller provides the retained pinned owner lifetime.
            lease: CallbackLease(unsafe { records.lease.0.into_retained() }),
        }
    }

    // The draft-4 bridge consumes these snapshots together with Declarations.
    #[allow(dead_code, reason = "draft-4 registry bridge follows the substrate")]
    pub(crate) fn take_raw_declarations(&mut self) -> raw_declarations::RawDeclarations {
        core::mem::take(&mut self.raw)
    }

    /// Move decoded-only host declarations out, leaving an empty set behind.
    /// Wire submissions are stored separately and do not appear here. They
    /// currently refuse commit until the SDK declaration views are wired.
    pub fn take_declarations(&mut self) -> Declarations {
        core::mem::take(&mut self.declarations)
    }
}

impl<'a, C, M, T> Deref for RegistrationBatch<'a, C, M, T> {
    type Target = PublishedRecords<'a, C, M, T>;

    fn deref(&self) -> &Self::Target {
        &self.records
    }
}

/// Published records share a lease which keeps their context and code alive.
/// Record destructors run before that lease is released. External queues need
/// their own leases and must be cancelled before owner teardown. This is all
/// a retained batch keeps for each plugin.
#[non_exhaustive]
pub struct PublishedRecords<'a, C, M, T> {
    records: Vec<T>,
    _image: core::marker::PhantomData<&'a [u8]>,
    lease: CallbackLease<'a, C, M>,
}

impl<C, M, T> PublishedRecords<'_, C, M, T> {
    /// Number of published host records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// The record published at `index`, read in place (no dispatch, no
    /// copy); `None` past the last one.
    #[must_use]
    pub fn record(&self, index: usize) -> Option<&T> {
        self.records.get(index)
    }

    /// Whether this batch contains no records.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Dispatch a declared contribution's token through this batch's owner.
    /// The host resolves and validates the declaration before calling this.
    pub(crate) fn dispatch_owner<R>(
        &self,
        depth: &DispatchDepth,
        callback: impl FnOnce(Pin<&C>) -> R,
    ) -> Result<R, Error> {
        self.lease.dispatch(depth, callback)
    }

    /// Dispatch one registered record after owner and shared-state admission.
    /// The supplied host wrapper performs scoped frame setup and foreign ABI
    /// validation. This method alone does not execute a foreign function table.
    ///
    /// # Errors
    /// Returns State for revoked admission or an invalid record index, Busy for
    /// same-instance reentry, or Depth when nesting capacity is exhausted.
    pub fn dispatch<R>(
        &self,
        index: usize,
        depth: &DispatchDepth,
        callback: impl FnOnce(Pin<&C>, &T) -> R,
    ) -> Result<R, Error> {
        let record = self.records.get(index).ok_or(Error::State)?;
        self.lease
            .dispatch(depth, |context| callback(context, record))
    }
}

/// A queued or in-flight callback's borrowed context/module lifetime.
#[non_exhaustive]
pub struct CallbackLease<'a, C, M>(lifecycle::Lease<'a, C, M>);

impl<C, M> CallbackLease<'_, C, M> {
    /// Run host dispatch only after checking revocation, reentry and depth.
    ///
    /// # Errors
    /// Returns State after revocation, Busy on same-instance reentry, or Depth
    /// when the shared nesting capacity is exhausted.
    pub fn dispatch<R>(
        &self,
        depth: &DispatchDepth,
        callback: impl FnOnce(Pin<&C>) -> R,
    ) -> Result<R, Error> {
        self.0.dispatch(&depth.0, callback).map_err(Into::into)
    }
}

impl fmt::Debug for DispatchDepth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DispatchDepth").finish_non_exhaustive()
    }
}

impl<C, M> fmt::Debug for HostOwner<C, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostOwner")
            .field("preparing", &self.preparing.get())
            .finish_non_exhaustive()
    }
}

impl<C, M, T> fmt::Debug for Construction<'_, C, M, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Construction")
            .field("len", &self.records.len())
            .field("declarations", &self.declarations)
            .field("failure", &self.failure)
            .finish_non_exhaustive()
    }
}

impl<C, M, T> fmt::Debug for RegistrationBatch<'_, C, M, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistrationBatch")
            .field("len", &self.records.len())
            .finish_non_exhaustive()
    }
}

impl<C, M, T> fmt::Debug for PublishedRecords<'_, C, M, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PublishedRecords")
            .field("len", &self.records.len())
            .finish_non_exhaustive()
    }
}

impl<C, M> fmt::Debug for CallbackLease<'_, C, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallbackLease").finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "native_host/declaration_image_tests.rs"]
mod declaration_image_tests;

#[cfg(test)]
mod pinned_owner_tests {
    use super::*;
    use alloc::rc::Rc;

    struct Context {
        canary: u32,
        reports: Rc<Cell<u8>>,
        dropped: Rc<Cell<bool>>,
    }
    impl Drop for Context {
        fn drop(&mut self) {
            self.dropped.set(true);
        }
    }
    struct Module(Rc<Cell<bool>>);
    impl Drop for Module {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    #[test]
    fn active_pinned_owner_retains_the_allocation_and_reports_once() {
        active_holder(false);
    }

    #[test]
    fn last_shared_owner_retains_the_active_pinned_allocation() {
        active_holder(true);
    }

    fn active_holder(shared: bool) {
        let reports = Rc::new(Cell::new(0));
        let context_dropped = Rc::new(Cell::new(false));
        let module_dropped = Rc::new(Cell::new(false));
        let holder = PinnedOwner::try_new(HostOwner::new(
            Context {
                canary: 0x1234_5678,
                reports: Rc::clone(&reports),
                dropped: Rc::clone(&context_dropped),
            },
            Module(Rc::clone(&module_dropped)),
        ))
        .unwrap();
        let batch = holder
            .as_ref()
            .begin::<()>("fixture")
            .unwrap()
            .commit(Teardown {
                report: |context, step, error| {
                    assert_eq!((step, error), (TeardownStep::Release, Error::Busy));
                    context.reports.set(context.reports.get() + 1);
                },
                cancel: |_| panic!("active owner must not cancel"),
                release: |_| panic!("active owner must not release"),
                destroy: |_| panic!("active owner must not destroy"),
            })
            .unwrap();
        drop(batch);
        let context = holder.as_ref().context().get_ref() as *const Context;
        holder.as_ref().inner().force_active();
        if shared {
            let holder = super::shared::Shared::try_new(holder).unwrap();
            let retained = holder.try_clone().unwrap();
            drop(holder);
            assert_eq!(reports.get(), 0);
            drop(retained);
        } else {
            drop(holder);
        }
        assert_eq!(reports.get(), 1);
        assert!(!context_dropped.get() && !module_dropped.get());
        // SAFETY: the active holder deliberately retained the entire allocation.
        assert_eq!(unsafe { (*context).canary }, 0x1234_5678);
    }
}

#[cfg(test)]
#[path = "native_host/event_hook_tests.rs"]
mod event_hook_tests;
/// Generated stage-local payload records, shared byte for byte with the SDK.
#[path = "abi7/stages_wire.rs"]
#[allow(clippy::all, clippy::pedantic, reason = "generated stage wire codecs")]
pub mod stages_wire;

#[path = "native_host/stage_broker.rs"]
#[allow(
    dead_code,
    reason = "host stage entry point is wired by the reader port"
)]
pub(crate) mod stage_broker;
pub(crate) mod stage_members;
pub(crate) mod stage_run;

mod registry_page;
pub(crate) mod registry_service;

#[cfg(test)]
#[path = "native_host/declaration_reg_index_tests.rs"]
mod declaration_reg_index_tests;

#[cfg(test)]
#[path = "native_host/native_ui_cleanup_tests.rs"]
mod native_ui_cleanup_tests;

#[cfg(all(test, target_arch = "x86_64", target_os = "linux"))]
#[path = "native_host/native_image_declarations_tests.rs"]
mod native_image_declarations_tests;

#[cfg(test)]
#[path = "native_host/ui_hooks_tests.rs"]
mod ui_hooks_tests;

#[cfg(test)]
#[path = "native_host/widget_drops_tests.rs"]
mod widget_drops_tests;
