//! Host-only declaration views shared by frames and UI services.
#[allow(
    dead_code,
    reason = "consumers are the ops 4, 14 and 15 and Core's directory"
)]
/// What the registry says about a widget, a status item or a status line
/// (REG ops 19 and 21), as the owner declared it. The host (Core) answers it
/// over the registry; it keeps no borrow past the call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Declared {
    /// The declaring plugin's host owner id (the one `OwnerResources` id,
    /// full width; `ui_handles` keeps it as it is).
    pub(crate) owner: u64,
    /// The owner's callback token (`widget.spec`, `status.item`, `status.line`).
    pub(crate) token: u32,
    /// `flags` bit 1 `private` (ui.md 4.3).
    pub(crate) private: bool,
}

/// One help hint as op 3 reads it (`help.hint`, `ui.md` 5): the declaring
/// plugin's record with `owner` named, the strings borrowed from the store for
/// one visit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HintView<'a> {
    /// 0 any, 1 touch, 2 buttons, as declared.
    pub(crate) input: u32,
    /// The zone in thousandths of the screen, as declared (all 0: none).
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// The catalog key, unchanged.
    pub(crate) text: &'a str,
    /// The declaring plugin, whose catalog holds `text`.
    pub(crate) owner: &'a str,
}

/// The registry's widget and status directory, read by ops 4, 14 and 15.
/// Every method answers `ERR_NOT_FOUND` for an id nobody declared (or whose
/// plugin retired) and `ERR_NO_MEMORY` when it cannot answer.
#[allow(dead_code, reason = "Core implements it; ops 4, 14 and 15 consume it")]
pub(crate) trait UiDirectory {
    /// Op 4: the widget `widget` (a base id; a placement `x@2` has been
    /// stripped by the caller) and whether `place` lists it.
    fn widget(&self, widget: &str) -> Result<Declared, i32>;
    /// Op 14: the status item with the full id `item`.
    fn status_item(&self, item: &str) -> Result<Declared, i32>;
    /// Op 15: the status line with the full id `line`.
    fn status_line(&self, line: &str) -> Result<Declared, i32>;
    /// Op 3: the hints declared for the screen `scope`, in plugin order and then
    /// record order, until `visit` breaks. A scope nobody declared a hint for
    /// visits nothing. Read-only; `ERR_NO_MEMORY` when it cannot be ordered.
    fn hints(
        &self,
        _scope: &str,
        _visit: &mut dyn FnMut(HintView<'_>) -> core::ops::ControlFlow<()>,
    ) -> Result<(), i32> {
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    }
}
