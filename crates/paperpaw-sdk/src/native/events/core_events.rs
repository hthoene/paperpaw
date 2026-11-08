//! Exact generated EVT records become borrowed typed views.
use super::{
    BluetoothChange, BookKind, ChargeFlags, Error, Event, LibraryChanges, PowerChange, ReadingView,
    WifiLinkState, wire,
};
use crate::{
    native::{
        declare::settings_value as settings,
        net::{HttpDone, HttpProgress},
        settings::SettingValue,
    },
    power::PowerLevel,
};

macro_rules! book {
    ($r:ident, $kind:ident) => {{
        let reading = ReadingView {
            now_ms: $r.now_ms,
            section: $r.section,
            offset: $r.offset,
            insert: $r.insert.unwrap_or(0),
            progress_ppm: $r.progress_ppm,
            path: $r.path,
            title: $r.title,
            chapter: $r.chapter,
            page: $r.page,
            pages: $r.pages,
        }
        .validate()?;
        let kind = BookKind::$kind;
        let reason = u32::from(kind != BookKind::Finished && $r.reason == 1);
        Event::Book {
            kind,
            reason,
            reading,
        }
    }};
}

pub(super) fn decode(raw: wire::RawRecord<'_>) -> Result<Event<'_>, Error> {
    Ok(match raw.tag() {
        wire::EVENT_SETTING_CHANGED_TAG => {
            let r: wire::EventSettingChanged<'_> = raw.decode().map_err(Error::from_status)?;
            Event::SettingChanged {
                now_ms: r.now_ms,
                key: r.key,
                old: old_value(r.old)?,
            }
        }
        wire::EVENT_BOOK_OPENED_TAG => {
            let r: wire::EventBookOpened<'_> = raw.decode().map_err(Error::from_status)?;
            book!(r, Opened)
        }
        wire::EVENT_BOOK_CLOSED_TAG => {
            let r: wire::EventBookClosed<'_> = raw.decode().map_err(Error::from_status)?;
            book!(r, Closed)
        }
        wire::EVENT_BOOK_FINISHED_TAG => {
            let r: wire::EventBookFinished<'_> = raw.decode().map_err(Error::from_status)?;
            book!(r, Finished)
        }
        wire::EVENT_BOOK_MOVED_TAG => {
            let r: wire::EventBookMoved<'_> = raw.decode().map_err(Error::from_status)?;
            Event::BookMoved {
                now_ms: r.now_ms,
                from: r.from,
                path: r.path,
            }
        }
        wire::EVENT_BOOK_REMOVED_TAG => {
            let r: wire::EventBookRemoved<'_> = raw.decode().map_err(Error::from_status)?;
            Event::BookRemoved {
                now_ms: r.now_ms,
                path: r.path,
            }
        }
        wire::EVENT_LIBRARY_CHANGED_TAG => library_changed(raw)?,
        wire::EVENT_PLUGINS_CHANGED_TAG => {
            let r: wire::EventPluginsChanged = raw.decode().map_err(Error::from_status)?;
            Event::PluginsChanged { now_ms: r.now_ms }
        }
        wire::EVENT_WIFI_CHANGED_TAG => {
            let r: wire::EventWifiChanged = raw.decode().map_err(Error::from_status)?;
            Event::WifiChanged {
                now_ms: r.now_ms,
                state: WifiLinkState::decode(r.state),
            }
        }
        wire::EVENT_POWER_CHANGED_TAG => {
            let r: wire::EventPowerChanged<'_> = raw.decode().map_err(Error::from_status)?;
            Event::PowerChanged(PowerChange {
                now_ms: r.now_ms,
                level: match r.level {
                    1 => PowerLevel::Saving,
                    2 => PowerLevel::Offline,
                    _ => PowerLevel::Normal,
                },
                automatic: r.flags & 1 != 0,
                stretch: r.stretch,
                mode: r.mode,
            })
        }
        wire::EVENT_STATUS_CHANGED_TAG => {
            let r: wire::EventStatusChanged = raw.decode().map_err(Error::from_status)?;
            Event::StatusChanged { now_ms: r.now_ms }
        }
        wire::EVENT_CHARGE_CHANGED_TAG => {
            let r: wire::EventChargeChanged = raw.decode().map_err(Error::from_status)?;
            Event::ChargeChanged {
                now_ms: r.now_ms,
                flags: ChargeFlags::from_bits(r.flags),
            }
        }
        wire::EVENT_BLUETOOTH_TAG => {
            let r: wire::EventBluetooth<'_> = raw.decode().map_err(Error::from_status)?;
            Event::Bluetooth(BluetoothChange {
                now_ms: r.now_ms,
                kind: r.kind,
                device: r.device,
                name: r.name,
                device_kind: r.device_kind,
                signal: r.signal,
                reason: r.reason,
            })
        }
        wire::EVENT_HTTP_DONE_TAG => {
            let r: wire::EventHttpDone = raw.decode().map_err(Error::from_status)?;
            Event::HttpDone(HttpDone::decode(r)?)
        }
        wire::EVENT_HTTP_PROGRESS_TAG => {
            let r: wire::EventHttpProgress = raw.decode().map_err(Error::from_status)?;
            Event::HttpProgress(HttpProgress::decode(r)?)
        }
        _ => return Err(Error::Unsupported),
    })
}

fn old_value(bytes: &[u8]) -> Result<Option<SettingValue<'_>>, Error> {
    if bytes.is_empty() {
        return Ok(None);
    }
    // A present malformed value must not masquerade as a missing previous value.
    let value = settings::settings_decode(bytes).map_err(|_| Error::Invalid)?;
    Ok(Some(match value {
        settings::SettingValueRef::Absent => SettingValue::Absent,
        settings::SettingValueRef::Bool(value) => SettingValue::Bool(value),
        settings::SettingValueRef::Int(value) => SettingValue::Int(value),
        settings::SettingValueRef::Text(value) => SettingValue::Text(value),
    }))
}

fn library_changed(raw: wire::RawRecord<'_>) -> Result<Event<'_>, Error> {
    let r: wire::EventLibraryChanged<'_> = raw.decode().map_err(Error::from_status)?;
    Ok(Event::LibraryChanged {
        now_ms: r.now_ms,
        what: LibraryChanges::from_bits(r.what),
        generation: r.generation,
        path: r.path,
    })
}
