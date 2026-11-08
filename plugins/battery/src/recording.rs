//! Interface-7 Battery recording using callback-scoped SDK services.
use crate::log::{self, Counts, Kind, Reading};
use crate::mem;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    events::Event,
    services,
    settings::{self, SettingValue},
    snapshot,
    storage::{self, Root},
};

/// Recording state owns no host handles or borrowed callback data.
#[derive(Debug, Default)]
pub struct Recorder {
    pages: u32,
    awake: bool,
    plugins_noted: Option<u64>,
}

impl Recorder {
    pub(crate) fn reading(&self, host: &Host<'_>) -> Result<Reading, Error> {
        let snapshot = snapshot::capture_snapshot(host)?;
        let usage = services::device_usage(host)?;
        Ok(Reading {
            ms: snapshot.instant().millis(),
            millivolts: snapshot.battery_millivolts(),
            percent: snapshot.battery_percent(),
            charging: snapshot.is_charging(),
            usage: Counts {
                refreshes: [usage.fast, usage.partial, usage.full],
                pixels: usage.pixels,
                inputs: usage.inputs,
                panel_ms: usage.panel_ms,
            },
            pages: self.pages,
            unix: snapshot.unix_seconds(),
        })
    }

    /// Deliver a lifecycle or page event synchronously, including stop/suspend flushes.
    pub fn event(&mut self, host: &Host<'_>, event: Event<'_>) -> Result<(), Error> {
        match event {
            Event::Started { .. } | Event::Resume { .. } => self.record(host, Kind::Start),
            Event::Suspend { .. } | Event::Stopping { .. } => self.record(host, Kind::End),
            Event::BatteryMeasured { .. } => {
                if !self.awake {
                    self.record(host, Kind::Start)?;
                }
                self.record(host, Kind::Sample)
            }
            Event::PageTurned(_) => {
                self.pages = self.pages.saturating_add(1);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn record(&mut self, host: &Host<'_>, kind: Kind) -> Result<(), Error> {
        let mut setting = [0; 32];
        if matches!(
            settings::read_setting(host, "battery.enabled", &mut setting)?,
            SettingValue::Bool(false)
        ) {
            return Ok(());
        }
        if kind == Kind::Start && self.awake {
            return Ok(());
        }
        let now = self.reading(host)?;
        // Logging failures (a refused line, the card) do not prevent sleep or
        // change the historical awake bookkeeping.
        if let Ok(line) = log::try_line(kind, &now) {
            let _ = append_record(host, log::FILE, log::OLD, log::HEADER, &line);
        }
        let due = self
            .plugins_noted
            .is_none_or(|at| now.ms.saturating_sub(at) >= log::PLUGINS_EVERY_MS);
        if kind == Kind::End || (kind == Kind::Sample && due) {
            self.plugins_noted = Some(now.ms);
            if let Ok(rows) = usage_lines(host, now.ms) {
                let _ = append_record(
                    host,
                    log::PLUGINS,
                    log::PLUGINS_OLD,
                    log::PLUGINS_HEADER,
                    &rows,
                );
            }
            let _ = storage::sync(host);
        }
        self.awake = kind != Kind::End;
        Ok(())
    }
}

/// Rotate and append one CSV record in the plugin's own root. A torn tail is
/// terminated before the next record; no existing bytes are rewritten.
pub fn append_record(
    host: &Host<'_>,
    name: &str,
    old: &str,
    header: &str,
    text: &str,
) -> Result<(), Error> {
    match storage::size(host, Root::Data, name) {
        Ok(size) if size >= log::MAX_BYTES => {
            if storage::rename(host, Root::Data, name, old).is_err() {
                let _ = storage::remove(host, Root::Data, old);
                if storage::rename(host, Root::Data, name, old).is_err() {
                    let _ = storage::remove(host, Root::Data, name);
                }
            }
            storage::append(host, Root::Data, name, header.as_bytes())?;
        }
        Ok(size) if size > 0 => {
            let mut tail = [0];
            if storage::read_at(host, Root::Data, name, size - 1, &mut tail)? != 1 {
                return Err(Error::Io);
            }
            if tail[0] != b'\n' {
                storage::append(host, Root::Data, name, b"\n")?;
            }
        }
        Ok(_) => {}
        Err(_) => storage::append(host, Root::Data, name, header.as_bytes())?,
    }
    // Rotate once per logical write, preserving the original usage-log grouping.
    // Each physical append remains within the typed storage slot's bound.
    if text.is_empty() {
        return storage::append(host, Root::Data, name, b"");
    }
    let mut remaining = text.as_bytes();
    while !remaining.is_empty() {
        let end = if remaining.len() > storage::MAX_DATA {
            remaining[..storage::MAX_DATA]
                .iter()
                .rposition(|b| *b == b'\n')
                .map(|index| index + 1)
                .ok_or(Error::NoSpace)?
        } else {
            remaining.len()
        };
        storage::append(host, Root::Data, name, &remaining[..end])?;
        remaining = &remaining[end..];
    }
    Ok(())
}

/// The rows of what each plugin took by `ms`, one after the other. A plugin
/// listed twice (a generation change) keeps its last row. A refused
/// allocation ends the whole write: a partial table would mislead.
fn usage_lines(host: &Host<'_>, ms: u64) -> Result<String, Error> {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut failed = None;
    services::for_each_plugin_usage(host, &mut |p| {
        if failed.is_some() {
            return;
        }
        if let Err(error) = note_row(&mut rows, ms, p) {
            failed = Some(error);
        }
    })?;
    if let Some(error) = failed {
        return Err(error);
    }
    let mut text = String::new();
    let length = rows
        .iter()
        .try_fold(0usize, |sum, (_, line)| sum.checked_add(line.len()))
        .ok_or(Error::NoMemory)?;
    text.try_reserve_exact(length)
        .map_err(|_| Error::NoMemory)?;
    for (_, line) in &rows {
        text.push_str(line);
    }
    Ok(text)
}

fn note_row(
    rows: &mut Vec<(String, String)>,
    ms: u64,
    p: &services::ServicePluginUsage<'_>,
) -> Result<(), Error> {
    let active = p.calls != 0 || p.shown_ms != 0 || p.read != 0 || p.written != 0;
    let line = if active {
        mem::format!(
            "{ms},{},{},{},{},{},{},{},{},{},{}\n",
            p.plugin,
            p.busy_us / 1000,
            p.calls,
            p.shown_ms,
            p.fast,
            p.partial,
            p.full,
            p.pixels / 1000,
            p.read / 1024,
            p.written / 1024
        )?
    } else {
        String::new()
    };
    // A generation change may repeat a row: keep the last value without restarting.
    if let Some((_, value)) = rows.iter_mut().find(|(id, _)| id == p.plugin) {
        *value = line;
        Ok(())
    } else {
        let id = mem::string(p.plugin)?;
        mem::push(rows, (id, line))
    }
}
