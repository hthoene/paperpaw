//! Exact generated EVT records become borrowed typed views.
use super::{
    BookKind, ChargeFlags, Error, Event, PowerChange, ReadingView, WifiLinkState, wire::Record,
};
use crate::{
    native::{declare::settings_value as settings, settings::SettingValue},
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

pub(super) fn decode(record: Record<'_>) -> Result<Event<'_>, Error> {
    Ok(match record {
        Record::EventSettingChanged(r) => Event::SettingChanged {
            now_ms: r.now_ms,
            key: r.key,
            old: old_value(r.old)?,
        },
        Record::EventBookOpened(r) => book!(r, Opened),
        Record::EventBookClosed(r) => book!(r, Closed),
        Record::EventBookFinished(r) => book!(r, Finished),
        Record::EventBookMoved(r) => Event::BookMoved {
            now_ms: r.now_ms,
            from: r.from,
            path: r.path,
        },
        Record::EventBookRemoved(r) => Event::BookRemoved {
            now_ms: r.now_ms,
            path: r.path,
        },
        Record::EventPluginsChanged(r) => Event::PluginsChanged { now_ms: r.now_ms },
        Record::EventWifiChanged(r) => Event::WifiChanged {
            now_ms: r.now_ms,
            state: WifiLinkState::decode(r.state),
        },
        Record::EventPowerChanged(r) => Event::PowerChanged(PowerChange {
            now_ms: r.now_ms,
            level: match r.level {
                1 => PowerLevel::Saving,
                2 => PowerLevel::Offline,
                _ => PowerLevel::Normal,
            },
            automatic: r.flags & 1 != 0,
            stretch: r.stretch,
            mode: r.mode,
        }),
        Record::EventStatusChanged(r) => Event::StatusChanged { now_ms: r.now_ms },
        Record::EventChargeChanged(r) => Event::ChargeChanged {
            now_ms: r.now_ms,
            flags: ChargeFlags::from_bits(r.flags),
        },
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
