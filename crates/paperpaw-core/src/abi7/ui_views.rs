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
}
