//! Scoped core views inside a stable host-owned context.
//!
//! The owner context may outlive an event's Env or Canvas. It must not retain
//! their Rust references. This slot admits a frame only for one synchronous
//! dispatch, clears it on every exit, and refuses nested installation.

use super::snapshot_wire::Snapshot;
use core::{
    cell::Cell, marker::PhantomPinned, mem::MaybeUninit, ops::ControlFlow, pin::Pin, ptr::NonNull,
};
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::ERR_STATE;
use paperpaw_platform::Rect;
use paperpaw_sdk::Settings;

#[allow(
    dead_code,
    reason = "service view fields are consumed by slots 17-19 encoders"
)]
#[path = "usage_views.rs"]
pub(crate) mod usage_views;
pub(crate) use usage_views::{DeviceUsageView, PluginRow};

#[path = "registry_views.rs"]
pub(crate) mod registry_views;
#[path = "../native_host/ui_guard.rs"]
pub(crate) mod ui_guard;
#[path = "ui_views.rs"]
pub(crate) mod ui_views;

/// Table position after a scoped visitor stops; counters are not snapshotted.
#[allow(dead_code, reason = "row offsets are read by the service encoder")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RowsEnd {
    pub(crate) total: u32,
    pub(crate) generation: u32,
    pub(crate) next: u32,
}

/// What `kind` names (section 5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code, reason = "read by slot 35 and the frame's compose hook")]
pub(crate) enum ComposeKind {
    /// A widget instance (`draw.frame.purpose` 4).
    Widget,
    /// A drawn status item (purpose 3).
    StatusItem,
    /// A status line (purpose 5).
    StatusLine,
}

/// One checked `ui.compose` call, as the frame's hook gets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code, reason = "read by slot 35 and the frame's compose hook")]
pub(crate) struct ComposeRequest {
    pub(crate) kind: ComposeKind,
    /// The instance or handle that op 4, 14 or 15 gave.
    pub(crate) handle: u32,
    /// The rectangle the caller asked for, in its canvas's coordinates.
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// The part of the rectangle inside the caller's clip, in the same
    /// coordinates; `None` when nothing of it is visible.
    pub(crate) visible: Option<Rect>,
}

#[allow(dead_code, reason = "usage service methods await slots 17-19 adapters")]
pub(crate) trait Frame {
    fn settings(&self) -> &Settings;
    fn translator(&self) -> &Translator;
    fn settings_editor(&self) -> Option<&str> {
        None
    }
    /// Broker access exists only for the duration of the outer callback.
    /// The broker completes checks and subscriber calls before publishing a reply.
    /// On success it initializes exactly the returned prefix using writes only,
    /// with count at most out.len(). On error it leaves every output byte untouched.
    fn stage_run(
        &self,
        _caller_id: &str,
        _full_stage: &str,
        _cursor: u32,
        _input: &[u8],
        _out: &mut [MaybeUninit<u8>],
    ) -> Result<usize, i32> {
        // TODO(#228): once the production broker is wired, a frame without
        // stage authority must return ERR_STATE (stages-services.md frame checks).
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    }
    /// The calling plugin: a never-reused host owner id. Service backends
    /// that keep per-caller state (leases) name it.
    fn caller(&self) -> Result<u64, i32> {
        Err(ERR_STATE)
    }
    /// No callback running is a state error, never epoch zero.
    fn callback_epoch(&self) -> Result<u64, i32> {
        Err(ERR_STATE)
    }
    /// The host-resolved screen instance running this callback.
    fn calling_instance(&self) -> Result<u32, i32> {
        Err(ERR_STATE)
    }
    fn callback_depth(&self) -> Result<u8, i32> {
        Err(ERR_STATE)
    }

    /// The kernel's `Env` for this call, where the frame has one: `core.device`
    /// answers Wi-Fi, Bluetooth and power from its models (ops 8, 10, 11, 12).
    /// A fixture frame has none and the shell's host answers.
    fn kernel_env(&self) -> Option<&paperpaw_sdk::Env<'_>> {
        None
    }

    fn with_stack(&self, _visit: &mut dyn FnMut(&dyn ui_guard::UiStackView)) -> Result<(), i32> {
        Err(ERR_STATE)
    }
    fn ui_x4(&self) -> Result<bool, i32> {
        Err(ERR_STATE)
    }
    fn close_dialog(&self, _instance: u32) -> Result<(), i32> {
        Err(ERR_STATE)
    }

    /// One line of the host's own diagnostics (`KernelBuilder::device_log`):
    /// at most 120 bytes, plugin ids and numbers only, never plugin text,
    /// paths or secrets; one per call site per sync or job. A frame without a
    /// sink drops it and formats nothing.
    fn log(&self, _line: core::fmt::Arguments<'_>) {}

    /// The tables `core.registry` reads (`registry.md` section 2). A frame
    /// that has none refuses with `ERR_UNSUPPORTED`.
    fn registry(&self) -> Result<&dyn registry_views::RegistryView, i32> {
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    }

    fn ui_directory(&self) -> Result<&dyn ui_views::UiDirectory, i32> {
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    }
    fn status_ask(
        &self,
        _owner: u64,
        _token: u32,
        _items: &[&str],
        _context: &[u8],
        _out: &mut [u8],
    ) -> Result<usize, i32> {
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    }
    /// One call of the `core.widget` stage to `owner` alone (`ui.md` 5.2, ops
    /// 7, 12 and 13): the owner's `token` gets `payload`, one stage-local input
    /// record already encoded (`core_widget_v1::input_encode`); its reply
    /// (stage-local records, checked) is written to the start of `out` and its
    /// length returned. Nested like `status_ask`: depth 1 callers only, never
    /// the caller's own plugin, the owner's own refusal is the call's status.
    fn widget_call(
        &self,
        _owner: u64,
        _token: u32,
        _payload: &[u8],
        _out: &mut [u8],
    ) -> Result<usize, i32> {
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    }

    /// One directed stage call to the native instance of `owner`'s `token`,
    /// for a host service that runs another plugin (the document service's
    /// format call, the lookup providers). The call goes through the stage
    /// dispatch of the owner's instance: that pushes the owner's own frame,
    /// the depth budget holds (the callee runs at depth 2, a third level is
    /// `ERR_BUSY`), a plugin never calls itself (`ERR_BUSY`), and nothing of
    /// the instance outlives the call.
    ///
    /// `accept` checks the callee's answer (`reply[..n]`) before it counts:
    /// an `Err` is the status of the call and the callee is told its answer
    /// was dropped. On success the answer is the first `n` bytes of `reply`;
    /// on an error `reply` is unspecified. `ERR_UNSUPPORTED` for a frame
    /// without stage authority.
    fn call_native(
        &self,
        _owner: u64,
        _token: u32,
        _input: &[u8],
        _reply: &mut [u8],
        _accept: &mut dyn FnMut(&[u8]) -> Result<(), i32>,
    ) -> Result<usize, i32> {
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    }

    /// The declaration store (a `DeclarationStore`, as `dyn Any`: this file is
    /// also compiled by the integration tests, which have no such type), read
    /// for the length of `visit` (the document service reads the formats and
    /// reading apps there). `visit` must not invoke plugins or reenter the
    /// store. `ERR_BUSY` while the store is being changed, `ERR_UNSUPPORTED`
    /// for a frame without a directory.
    fn with_declarations(&self, _visit: &mut dyn FnMut(&dyn core::any::Any)) -> Result<(), i32> {
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    }

    /// A host view kept behind a service key, as `dyn Any`. **Not used by the
    /// families any more**: `core.device`, `core.document`, `core.library` and
    /// `core.lookup` reach theirs through the caller's `hosts`
    /// (`Caller::hosts`, wired once per device by `HostServices::wire_*`), and
    /// a family without one answers `ERR_UNSUPPORTED`. No frame overrides this.
    fn host_service(&self, _key: &str) -> Option<&dyn core::any::Any> {
        None
    }
    /// Slot 35 (`docs/abi/ui.md` section 5.1), after the slot checked `kind`,
    /// that the caller is inside its own `draw`, and worked out
    /// `request.visible`. `canvas` is the caller's canvas already clipped to
    /// that part (empty when nothing is visible).
    ///
    /// The hook decides, in this order: an unknown or closed handle
    /// `ERR_NOT_FOUND`; a private target, or one made below a guard,
    /// `ERR_DENIED`; a target of the caller's own plugin, or a call made from
    /// a child (depth 2), `ERR_BUSY`; then, only if something is visible, it
    /// runs the owner's `draw` with `request.draw_frame()`. A child's failure
    /// is its status, logged, and the hook returns `Ok`: the caller's frame
    /// stays. A frame that composes nothing refuses with `ERR_UNSUPPORTED`
    /// (the X4 until its stack gate passes), and every frame handed to a
    /// child must answer `ERR_BUSY` here.
    fn compose(
        &self,
        _caller_id: &str,
        _request: &ComposeRequest,
        _canvas: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<(), i32> {
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    }
    fn fonts(&self) -> Option<&paperpaw_sdk::Fonts> {
        None
    }
    fn ink_threshold(&self) -> Result<u8, i32> {
        Err(ERR_STATE)
    }
    fn font_generation(&self) -> Option<u64> {
        None
    }
    fn font_valid(&self, _provider: &str, _acquired: u64) -> bool {
        false
    }
    fn storage(&self) -> Option<&dyn paperpaw_platform::Storage> {
        None
    }
    fn device_usage(&self) -> Result<DeviceUsageView, i32> {
        Err(ERR_STATE)
    }
    /// Continue consumes the row; Break leaves it for the next page.
    /// The visitor is host-only and must never call plugin code while borrowing rows.
    fn plugin_rows(
        &self,
        _from: u32,
        _visitor: &mut dyn FnMut(&PluginRow<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        Err(ERR_STATE)
    }
    fn plugin_name(&self, _plugin: &str, _visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        Err(ERR_STATE)
    }
    fn work_note(&self, _plugin: &str, _visit: &mut dyn FnMut(u64, &str)) -> Result<(), i32> {
        Err(ERR_STATE)
    }
    /// Query-only frames need not provide an environment snapshot.
    fn snapshot(&self) -> Result<Snapshot, i32> {
        Err(paperpaw_native::host_glue::ERR_STATE)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    Busy,
    NoFrame,
}

pub(crate) struct FrameSlot {
    current: Cell<Option<NonNull<dyn Frame>>>,
    _pin: PhantomPinned,
}

struct Reset<'a>(&'a FrameSlot);

impl Drop for Reset<'_> {
    fn drop(&mut self) {
        self.0.current.set(None);
    }
}

impl FrameSlot {
    pub(crate) const fn new() -> Self {
        Self {
            current: Cell::new(None),
            _pin: PhantomPinned,
        }
    }

    /// The closure is synchronous. Neither it nor the raw ABI may keep a frame
    /// view after it returns. Pinning also keeps a surrounding owner context's
    /// address stable while foreign code holds its pointer.
    pub(crate) fn scope<R>(
        self: Pin<&Self>,
        frame: &dyn Frame,
        call: impl FnOnce() -> R,
    ) -> Result<R, Error> {
        if self.current.get().is_some() {
            return Err(Error::Busy);
        }
        let borrowed = NonNull::from(frame);
        // SAFETY: the erased lifetime is restored only inside `with`. The
        // Reset guard clears the pointer before this borrow ends, including
        // unwinding. Nested scope replacement is refused and Cell prevents
        // cross-thread sharing. No pointer or borrowed frame is returned.
        let erased = unsafe {
            core::mem::transmute::<NonNull<dyn Frame + '_>, NonNull<dyn Frame + 'static>>(borrowed)
        };
        self.current.set(Some(erased));
        let _reset = Reset(self.get_ref());
        Ok(call())
    }

    pub(crate) fn with<R>(&self, read: impl FnOnce(&dyn Frame) -> R) -> Result<R, Error> {
        let pointer = self.current.get().ok_or(Error::NoFrame)?;
        // SAFETY: a present pointer belongs to a live scope. Its shared frame
        // reference is limited to `read`; R cannot borrow that reference.
        Ok(read(unsafe { pointer.as_ref() }))
    }
}
