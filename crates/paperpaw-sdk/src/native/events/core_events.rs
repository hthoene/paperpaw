//! Exact generated EVT records become borrowed typed views.
use super::{
    BluetoothChange, BookKind, ChargeFlags, Error, Event, InstallDone, LibraryChanges, PowerChange,
    ReadingView, WifiDone, WifiLinkState, wire,
};
use crate::{
    native::{
        declare::settings_value as settings,
        image::ImageReady,
        net::{HttpDone, HttpProgress, ServedRequest, ServerEvent},
        settings::SettingValue,
        sockets::{BroadcastEvent, SocketEvent},
        work::{WorkDue, WorkRefused},
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
            chapter_left: $r.chapter_left,
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

event_fn!(setting_changed, wire::EventSettingChanged<'_>, |r| {
    Event::SettingChanged {
        now_ms: r.now_ms,
        key: r.key,
        old: old_value(r.old)?,
    }
});

event_fn!(book_opened, wire::EventBookOpened<'_>, |r| book!(r, Opened));

event_fn!(book_closed, wire::EventBookClosed<'_>, |r| book!(r, Closed));

event_fn!(book_finished, wire::EventBookFinished<'_>, |r| book!(
    r, Finished
));

event_fn!(book_moved, wire::EventBookMoved<'_>, |r| Event::BookMoved {
    now_ms: r.now_ms,
    from: r.from,
    path: r.path,
});

event_fn!(book_removed, wire::EventBookRemoved<'_>, |r| {
    Event::BookRemoved {
        now_ms: r.now_ms,
        path: r.path,
    }
});

event_fn!(library_changed, wire::EventLibraryChanged<'_>, |r| {
    Event::LibraryChanged {
        now_ms: r.now_ms,
        what: LibraryChanges::from_bits(r.what),
        generation: r.generation,
        path: r.path,
    }
});

event_fn!(plugins_changed, wire::EventPluginsChanged, |r| {
    Event::PluginsChanged { now_ms: r.now_ms }
});

event_fn!(wifi_changed, wire::EventWifiChanged, |r| {
    Event::WifiChanged {
        now_ms: r.now_ms,
        state: WifiLinkState::decode(r.state),
    }
});

event_fn!(wifi_scanned, wire::EventWifiScanned, |r| {
    Event::WifiScanned {
        now_ms: r.now_ms,
        networks: r.networks,
    }
});

event_fn!(wifi_done, wire::EventWifiDone<'_>, |r| {
    Event::WifiDone(WifiDone {
        now_ms: r.now_ms,
        request: r.request,
        ssid: r.ssid,
        status: r.status,
        reason: r.reason,
        state: r.state,
    })
});

event_fn!(network_wanted, wire::EventNetworkWanted, |r| {
    Event::NetworkWanted { now_ms: r.now_ms }
});

event_fn!(network_idle, wire::EventNetworkIdle, |r| {
    Event::NetworkIdle { now_ms: r.now_ms }
});

event_fn!(firmware_changed, wire::EventFirmwareChanged, |r| {
    Event::FirmwareChanged {
        now_ms: r.now_ms,
        request: crate::native::device::DeviceRequest::new(r.request),
        state: crate::native::device::FirmwareState::decode(r.state),
    }
});

event_fn!(install_done, wire::EventInstallDone<'_>, |r| {
    Event::InstallDone(InstallDone {
        now_ms: r.now_ms,
        request: r.request,
        kind: r.kind,
        status: r.status,
        reason: r.reason,
        flags: r.flags,
        id: r.id,
    })
});

event_fn!(power_changed, wire::EventPowerChanged<'_>, |r| {
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
});

event_fn!(status_changed, wire::EventStatusChanged, |r| {
    Event::StatusChanged { now_ms: r.now_ms }
});

event_fn!(charge_changed, wire::EventChargeChanged, |r| {
    Event::ChargeChanged {
        now_ms: r.now_ms,
        flags: ChargeFlags::from_bits(r.flags),
    }
});

event_fn!(bluetooth, wire::EventBluetooth<'_>, |r| Event::Bluetooth(
    BluetoothChange {
        now_ms: r.now_ms,
        kind: r.kind,
        device: r.device,
        name: r.name,
        device_kind: r.device_kind,
        signal: r.signal,
        reason: r.reason,
    }
));

event_fn!(http_done, wire::EventHttpDone, |r| Event::HttpDone(
    HttpDone::decode(r)?
));

event_fn!(http_progress, wire::EventHttpProgress, |r| {
    Event::HttpProgress(HttpProgress::decode(r)?)
});

event_fn!(server, wire::EventServer<'_>, |r| Event::Server(
    ServerEvent::decode(r)?
));

event_fn!(served_request, wire::EventServedRequest<'_>, |r| {
    Event::ServedRequest(ServedRequest::decode(r)?)
});

event_fn!(image_ready, wire::EventImageReady<'_>, |r| {
    Event::ImageReady(ImageReady::decode(r)?)
});

event_fn!(socket, wire::EventSocket, |r| Event::Socket(
    SocketEvent::decode(r)?
));

event_fn!(broadcast, wire::EventBroadcast<'_>, |r| Event::Broadcast(
    BroadcastEvent::decode(r)?
));

event_fn!(work_due, wire::EventWorkDue<'_>, |r| Event::WorkDue(
    WorkDue::decode(r)
));

event_fn!(work_refused, wire::EventWorkRefused<'_>, |r| {
    Event::WorkRefused(WorkRefused::decode(r))
});

event_fn!(work_allowed, wire::EventWorkAllowed, |r| {
    Event::WorkAllowed { now_ms: r.now_ms }
});

event_fn!(lookup_ready, wire::EventLookupReady<'_>, |r| {
    Event::LookupReady {
        now_ms: r.now_ms,
        provider: r.provider,
    }
});

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
