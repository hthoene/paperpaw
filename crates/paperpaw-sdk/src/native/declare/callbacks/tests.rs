#![allow(
    clippy::unwrap_used,
    reason = "encoding failures must fail fixture tests"
)]
use super::*;
use crate::native::declare::{BATCH_MAX_RECORDS, GroupDecl, ScreenDecl};
use alloc::{vec, vec::Vec};
use records_wire::{Record, records_decode, records_encode, registrations_encode};

#[test]
#[allow(clippy::too_many_lines, reason = "one table row per event kind")]
fn every_event_kind_matches_its_generated_registration_bytes() {
    let kinds = [
        (
            EventKind::StackChanged,
            records_wire::EVENT_STACK_CHANGED_TAG,
        ),
        (EventKind::ScreenReady, records_wire::EVENT_SCREEN_READY_TAG),
        (EventKind::Spoken, records_wire::EVENT_SPOKEN_TAG),
        (
            EventKind::DocumentClosed,
            records_wire::EVENT_DOCUMENT_CLOSED_TAG,
        ),
        (EventKind::Tick, records_wire::EVENT_TICK_TAG),
        (
            EventKind::WidgetChanged,
            records_wire::EVENT_WIDGET_CHANGED_TAG,
        ),
        (EventKind::Started, records_wire::EVENT_STARTED_TAG),
        (EventKind::Resume, records_wire::EVENT_RESUME_TAG),
        (EventKind::Suspend, records_wire::EVENT_SUSPEND_TAG),
        (EventKind::Stopping, records_wire::EVENT_STOPPING_TAG),
        (
            EventKind::BatteryMeasured,
            records_wire::EVENT_BATTERY_MEASURED_TAG,
        ),
        (EventKind::PageTurned, records_wire::EVENT_PAGE_TURNED_TAG),
        (EventKind::ScreenShown, records_wire::EVENT_SCREEN_SHOWN_TAG),
        (EventKind::ActionRun, records_wire::EVENT_ACTION_RUN_TAG),
        (
            EventKind::SettingChanged,
            records_wire::EVENT_SETTING_CHANGED_TAG,
        ),
        (EventKind::BookOpened, records_wire::EVENT_BOOK_OPENED_TAG),
        (EventKind::BookClosed, records_wire::EVENT_BOOK_CLOSED_TAG),
        (
            EventKind::BookFinished,
            records_wire::EVENT_BOOK_FINISHED_TAG,
        ),
        (EventKind::BookMoved, records_wire::EVENT_BOOK_MOVED_TAG),
        (EventKind::BookRemoved, records_wire::EVENT_BOOK_REMOVED_TAG),
        (
            EventKind::LibraryChanged,
            records_wire::EVENT_LIBRARY_CHANGED_TAG,
        ),
        (
            EventKind::PluginsChanged,
            records_wire::EVENT_PLUGINS_CHANGED_TAG,
        ),
        (EventKind::WifiChanged, records_wire::EVENT_WIFI_CHANGED_TAG),
        (EventKind::WifiScanned, records_wire::EVENT_WIFI_SCANNED_TAG),
        (EventKind::WifiDone, records_wire::EVENT_WIFI_DONE_TAG),
        (
            EventKind::NetworkWanted,
            records_wire::EVENT_NETWORK_WANTED_TAG,
        ),
        (EventKind::NetworkIdle, records_wire::EVENT_NETWORK_IDLE_TAG),
        (
            EventKind::FirmwareChanged,
            records_wire::EVENT_FIRMWARE_CHANGED_TAG,
        ),
        (EventKind::InstallDone, records_wire::EVENT_INSTALL_DONE_TAG),
        (
            EventKind::PowerChanged,
            records_wire::EVENT_POWER_CHANGED_TAG,
        ),
        (
            EventKind::StatusChanged,
            records_wire::EVENT_STATUS_CHANGED_TAG,
        ),
        (
            EventKind::ChargeChanged,
            records_wire::EVENT_CHARGE_CHANGED_TAG,
        ),
        (
            EventKind::RegistryChanged,
            records_wire::EVENT_REGISTRY_CHANGED_TAG,
        ),
        (
            EventKind::PreviewChanged,
            records_wire::EVENT_PREVIEW_CHANGED_TAG,
        ),
        (
            EventKind::PluginsRequested,
            records_wire::EVENT_PLUGINS_REQUESTED_TAG,
        ),
        (
            EventKind::PluginAnswered,
            records_wire::EVENT_PLUGIN_ANSWERED_TAG,
        ),
    ];
    for (kind, event_tag) in kinds {
        let mut batches = Vec::new();
        let mut declarations = Declarations::new(|batch: &[u8]| {
            batches.push(batch.to_vec());
            Ok(())
        });
        declarations.subscribe_event(kind, u32::MAX).unwrap();
        declarations.register_draw(0).unwrap();
        declarations.submit().unwrap();
        drop(declarations);
        let mut expected = [77; 64];
        let length = registrations_encode(
            &[
                Registration::Event {
                    event_tag,
                    token: u32::MAX,
                },
                Registration::Draw { token: 0 },
            ],
            &mut expected,
        )
        .unwrap();
        assert_eq!(batches, [expected[..length].to_vec()], "{kind:?}");
    }
}

#[test]
fn callbacks_and_declarations_keep_order_across_batch_flushes() {
    let mut batches = Vec::new();
    let mut declarations = Declarations::new(|batch: &[u8]| {
        batches.push(batch.to_vec());
        Ok(())
    });
    declarations
        .screen(ScreenDecl::new("battery.screen", "battery.title", 10))
        .unwrap();
    for token in 0..BATCH_MAX_RECORDS as u32 {
        declarations
            .subscribe_event(EventKind::Started, token)
            .unwrap();
    }
    declarations.register_draw(11).unwrap();
    assert_eq!(declarations.len(), BATCH_MAX_RECORDS + 2);
    declarations.submit().unwrap();
    drop(declarations);
    assert_eq!(batches.len(), 2);
    let first = records_decode(&batches[0]).unwrap();
    let second = records_decode(&batches[1]).unwrap();
    assert_eq!(first.len(), BATCH_MAX_RECORDS);
    assert_eq!(second.len(), 2);
    let mut expected = vec![Record::ScreenSpec(records_wire::ScreenSpec {
        name: "battery.screen",
        title: "battery.title",
        token: 10,
        flags: 0,
    })];
    expected.extend((0..BATCH_MAX_RECORDS as u32).map(|token| {
        Record::Registration(Registration::Event {
            event_tag: records_wire::EVENT_STARTED_TAG,
            token,
        })
    }));
    expected.push(Record::Registration(Registration::Draw { token: 11 }));
    assert_eq!(
        first.iter().chain(second.iter()).collect::<Vec<_>>(),
        expected
    );
    for (batch, records) in batches.iter().zip(expected.chunks(BATCH_MAX_RECORDS)) {
        let mut encoded = [0; 512];
        let length = records_encode(records, &mut encoded).unwrap();
        assert_eq!(batch, &encoded[..length]);
    }
}

#[test]
fn callback_additions_preserve_sink_failure_latching() {
    let mut calls = 0;
    let mut declarations = Declarations::new(|_: &[u8]| {
        calls += 1;
        Err(-77)
    });
    for token in 0..BATCH_MAX_RECORDS as u32 {
        declarations.register_draw(token).unwrap();
    }
    let error = DeclareError::Sink(-77);
    assert_eq!(
        declarations
            .subscribe_event(EventKind::Tick, 9)
            .unwrap_err(),
        error
    );
    assert_eq!(declarations.register_draw(10).unwrap_err(), error);
    assert_eq!(declarations.submit(), Err(error));
    drop(declarations);
    assert_eq!(calls, 1);
}

#[test]
fn local_failure_discards_pending_callbacks_and_aborts_once() {
    let mut batches = Vec::new();
    let mut declarations = Declarations::new(|batch: &[u8]| {
        batches.push(batch.to_vec());
        Ok(())
    });
    declarations
        .subscribe_event(EventKind::Stopping, 4)
        .unwrap();
    let error = DeclareError::Empty("group title");
    assert_eq!(
        declarations
            .group(GroupDecl::new("battery.group", ""))
            .unwrap_err(),
        error
    );
    assert_eq!(declarations.register_draw(5).unwrap_err(), error);
    assert_eq!(
        declarations
            .subscribe_event(EventKind::Resume, 6)
            .unwrap_err(),
        error
    );
    assert_eq!(declarations.submit(), Err(error));
    assert_eq!(declarations.submit(), Err(error));
    drop(declarations);
    assert_eq!(batches, [Vec::<u8>::new()]);
}
