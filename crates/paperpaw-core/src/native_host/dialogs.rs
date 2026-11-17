//! The box of the host's own yes or no question, the firmware confirmation
//! (`firmware_confirm`). Every plugin's question and the kernel's own are the
//! dialog plugin's (`core.choice`, `core.confirm`; `docs/abi/ui.md` 3.1 and
//! 3.2, `kernel_asks`): with none on the slot they end as cancelled, as the
//! pump leaves them.
#[path = "dialogs/confirm_box.rs"]
mod confirm_box;

pub(crate) use confirm_box::{ConfirmBox, Touch};
