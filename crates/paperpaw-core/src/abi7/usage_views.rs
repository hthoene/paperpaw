//! Allocation-free host views; wire widening belongs to the service encoder.
use paperpaw_sdk::usage::{PluginUsage, Usage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DeviceUsageView {
    pub(crate) fast: u32,
    pub(crate) partial: u32,
    pub(crate) full: u32,
    pub(crate) pixels: u64,
    pub(crate) inputs: u32,
    pub(crate) sleeps: u32,
    pub(crate) panel_ms: u64,
    pub(crate) bluetooth_ms: u64,
}

impl From<&Usage> for DeviceUsageView {
    fn from(usage: &Usage) -> Self {
        Self {
            fast: usage.refreshes.fast,
            partial: usage.refreshes.partial,
            full: usage.refreshes.full,
            pixels: usage.pixels,
            inputs: usage.inputs,
            sleeps: usage.sleeps,
            panel_ms: usage.panel_ms,
            bluetooth_ms: usage.bluetooth_ms,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PluginRow<'a> {
    pub(crate) plugin: &'a str,
    pub(crate) busy_us: u64,
    pub(crate) calls: u32,
    pub(crate) shown_ms: u32,
    pub(crate) fast: u32,
    pub(crate) partial: u32,
    pub(crate) full: u32,
    pub(crate) pixels: u64,
    pub(crate) read: u32,
    pub(crate) written: u32,
    pub(crate) syncs: u32,
    pub(crate) requests: u32,
    pub(crate) refused: u32,
    pub(crate) awake_ms: u32,
    pub(crate) awake_reason: &'a str,
}

impl<'a> PluginRow<'a> {
    pub(crate) fn new(usage: &'a PluginUsage, syncs: u32) -> Self {
        Self {
            plugin: usage.plugin.as_str(),
            busy_us: usage.busy_us,
            calls: usage.calls,
            shown_ms: usage.shown_ms,
            fast: usage.refreshes.fast,
            partial: usage.refreshes.partial,
            full: usage.refreshes.full,
            pixels: usage.pixels,
            read: usage.read,
            written: usage.written,
            syncs,
            requests: usage.requests,
            refused: usage.refused,
            awake_ms: usage.awake_ms,
            awake_reason: clipped(&usage.awake_reason, 64),
        }
    }
}

pub(crate) fn clipped(text: &str, cap: usize) -> &str {
    let mut end = text.len().min(cap);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
