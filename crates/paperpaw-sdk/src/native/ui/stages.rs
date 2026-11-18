//! Typed owner-side adapters for generated widget and status stage payloads.
mod widget;
pub use widget::WidgetCall;
mod status;
pub use status::{ContextParams, StatusAnswer, StatusCall};
