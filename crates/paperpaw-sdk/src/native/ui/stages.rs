//! Typed owner-side helpers for the host-defined UI stages. Payload framing is
//! entirely generated; a plugin only reads the request and writes its reply.
mod filter;
pub use filter::{FilterCall, FilterDecision};
mod widget;
pub use widget::WidgetCall;
mod status;
pub use status::{ContextParams, StatusAnswer, StatusCall};
