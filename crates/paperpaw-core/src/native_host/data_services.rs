//! Service-key validation and direct encoding of scoped host views.
use super::registrations as wire;
use crate::abi7::context::{Frame, PluginRow};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_NOT_FOUND};

/// Data-service capabilities implemented by the shared host table.
/// Admission must advertise this catalog only when using that implementation.
pub static DATA_SERVICES: &[paperpaw_native::metadata::Service<'static>] = &[
    paperpaw_native::metadata::Service {
        key: "core.plugin",
        major: 1,
        semantic_revision: 0,
        min_table_bytes: 0,
    },
    paperpaw_native::metadata::Service {
        key: "core.plugin-usage",
        major: 1,
        semantic_revision: 0,
        min_table_bytes: 0,
    },
    paperpaw_native::metadata::Service {
        key: "core.usage",
        major: 1,
        semantic_revision: 0,
        min_table_bytes: 0,
    },
    paperpaw_native::metadata::Service {
        key: "core.work-note",
        major: 1,
        semantic_revision: 0,
        min_table_bytes: 0,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Query<'a> {
    Usage,
    PluginUsage(u32),
    Plugin(&'a str),
    WorkNote(&'a str),
}

impl<'a> Query<'a> {
    pub(crate) fn parse(key: &'a str) -> Result<Self, i32> {
        if key.len() > 128 || !key.is_ascii() {
            return Err(ERR_INVALID);
        }
        let (base, arg) = key
            .split_once('/')
            .map_or((key, None), |(base, arg)| (base, Some(arg)));
        let mut count = 0;
        for segment in base.split('.') {
            count += 1;
            if segment.len() > 32
                || !segment
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_lowercase)
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            {
                return Err(ERR_INVALID);
            }
        }
        if count < 2
            || arg.is_some_and(|arg| {
                arg.is_empty()
                    || arg.len() > 64
                    || !arg
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            })
        {
            return Err(ERR_INVALID);
        }
        match (base, arg) {
            ("core.usage", None) => Ok(Self::Usage),
            ("core.usage", Some(_)) => Err(ERR_INVALID),
            ("core.plugin-usage", None) => Ok(Self::PluginUsage(0)),
            ("core.plugin-usage", Some(arg)) => {
                let number = arg.strip_prefix("from.").ok_or(ERR_INVALID)?;
                if number.is_empty()
                    || number.len() > 1 && number.starts_with('0')
                    || !number.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err(ERR_INVALID);
                }
                Ok(Self::PluginUsage(number.parse().map_err(|_| ERR_INVALID)?))
            }
            ("core.plugin" | "core.work-note", arg) => {
                let plugin = arg
                    .filter(|id| paperpaw_native::bundle::is_plugin_id(id))
                    .ok_or(ERR_INVALID)?;
                Ok(if base == "core.plugin" {
                    Self::Plugin(plugin)
                } else {
                    Self::WorkNote(plugin)
                })
            }
            _ => Err(ERR_NOT_FOUND),
        }
    }

    pub(crate) const fn base(self) -> &'static str {
        match self {
            Self::Usage => "core.usage",
            Self::PluginUsage(_) => "core.plugin-usage",
            Self::Plugin(_) => "core.plugin",
            Self::WorkNote(_) => "core.work-note",
        }
    }
}

pub(crate) fn encode(frame: &dyn Frame, query: Query<'_>, out: &mut [u8]) -> Result<usize, i32> {
    match query {
        Query::Usage => {
            let view = frame.device_usage()?;
            wire::record_encode_one(
                &wire::Record::ServiceUsage(wire::ServiceUsage {
                    fast: view.fast,
                    partial: view.partial,
                    full: view.full,
                    pixels: view.pixels,
                    inputs: view.inputs,
                    sleeps: view.sleeps,
                    panel_ms: view.panel_ms,
                    bluetooth_ms: view.bluetooth_ms,
                }),
                out,
            )
        }
        Query::PluginUsage(from) => encode_rows(frame, from, out),
        Query::Plugin(plugin) => {
            let mut result = Err(ERR_NOT_FOUND);
            frame.plugin_name(plugin, &mut |name| {
                result = wire::record_encode_one(
                    &wire::Record::ServicePlugin(wire::ServicePlugin {
                        name: clipped(name, wire::SERVICE_PLUGIN_NAME_MAX_BYTES),
                    }),
                    out,
                );
            })?;
            result
        }
        Query::WorkNote(plugin) => {
            let mut result = Err(ERR_NOT_FOUND);
            frame.work_note(plugin, &mut |at_ms, text| {
                result = wire::record_encode_one(
                    &wire::Record::ServiceWorkNote(wire::ServiceWorkNote { at_ms, text }),
                    out,
                );
            })?;
            result
        }
    }
}

fn encode_rows(frame: &dyn Frame, from: u32, out: &mut [u8]) -> Result<usize, i32> {
    let mut end = wire::ServiceEnd {
        flags: 0,
        total: 0,
        generation: 0,
        next: 0,
    };
    let reserved = wire::record_framed_len(&wire::Record::ServiceEnd(end))?;
    let available = out.len().checked_sub(reserved).ok_or(ERR_NO_SPACE)?;
    let mut used = 0;
    let mut failure = None;
    let rows = frame.plugin_rows(from, &mut |row| {
        if row.plugin.len() > wire::SERVICE_PLUGIN_USAGE_PLUGIN_MAX_BYTES {
            end.flags |= 2;
            return ControlFlow::Continue(());
        }
        let record = row_record(row);
        let length = match wire::record_framed_len(&record) {
            Ok(length) => length,
            Err(error) => {
                failure = Some(error);
                return ControlFlow::Break(());
            }
        };
        if length > available - used {
            return ControlFlow::Break(());
        }
        match wire::record_encode_one(&record, &mut out[used..available]) {
            Ok(length) => {
                used += length;
                ControlFlow::Continue(())
            }
            Err(error) => {
                failure = Some(error);
                ControlFlow::Break(())
            }
        }
    })?;
    if let Some(error) = failure {
        return Err(error);
    }
    end.total = rows.total;
    end.generation = rows.generation;
    end.next = rows.next;
    if end.next < end.total {
        end.flags |= 1;
    }
    let written = wire::record_encode_one(&wire::Record::ServiceEnd(end), &mut out[used..])?;
    Ok(used + written)
}

fn row_record<'a>(row: &'a PluginRow<'a>) -> wire::Record<'a> {
    wire::Record::ServicePluginUsage(wire::ServicePluginUsage {
        plugin: row.plugin,
        busy_us: row.busy_us,
        calls: row.calls,
        shown_ms: u64::from(row.shown_ms),
        fast: row.fast,
        partial: row.partial,
        full: row.full,
        pixels: row.pixels,
        read: u64::from(row.read),
        written: u64::from(row.written),
        syncs: row.syncs,
        requests: row.requests,
        refused: row.refused,
        awake_ms: u64::from(row.awake_ms),
        awake_reason: clipped(
            row.awake_reason,
            wire::SERVICE_PLUGIN_USAGE_AWAKE_REASON_MAX_BYTES,
        ),
    })
}

fn clipped(text: &str, limit: usize) -> &str {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
#[path = "data_services/tests.rs"]
mod tests;
