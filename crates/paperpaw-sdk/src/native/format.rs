//! Allocation-free provider adapters for the host-defined `core.format` stage.
use super::{Error, stage_schemas::core_format_v1 as schema};
mod request;
pub use request::{FormatCall, FormatOp, FormatOption, FormatOptions};
mod metadata;
mod replies;
pub use replies::{FormatFeatures, FormatState};
#[cfg(test)]
mod tests;

#[cfg(test)]
mod metadata_tests;
