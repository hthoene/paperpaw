//! `Incoming::decode_as` reads what `Event::decode` reads, kind by kind.
#![allow(clippy::unwrap_used, reason = "fixtures must fail the test")]
use super::*;
use crate::native::declare::records_wire::Record;
use alloc::{vec, vec::Vec};

fn encode(record: Record<'_>) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    let n = wire::records_encode(&[record], &mut bytes).unwrap();
    bytes.truncate(n);
    bytes
}

fn tick() -> Vec<u8> {
    let mut bytes = vec![0; 64];
    let n = wire::tick_encode(42, &mut bytes).unwrap();
    bytes.truncate(n);
    bytes
}

fn book<T>(make: impl Fn(wire::EventBookOpened<'static>) -> T) -> T {
    make(wire::EventBookOpened {
        now_ms: 42,
        section: 2,
        offset: 300,
        insert: Some(3),
        progress_ppm: 500_000,
        path: "books/a.epub",
        title: "A title",
        chapter: "Chapter",
        page: Some(1),
        pages: Some(2),
        reason: 1,
        chapter_left: None,
    })
}

/// One valid record of every kind.
#[allow(clippy::too_many_lines, reason = "one record of every kind")]
fn fixtures() -> Vec<Vec<u8>> {
    use wire::*;
    let opened = book(|r| r);
    let mut all = vec![tick()];
    all.extend(
        [
            Record::StageCall(StageCall {
                stage: "reader.insert",
                schema: 1,
                payload: &[1, 2, 3],
            }),
            Record::EventStarted(EventStarted { now_ms: 42 }),
            Record::EventResume(EventResume { now_ms: 42 }),
            Record::EventSuspend(EventSuspend { now_ms: 42 }),
            Record::EventStopping(EventStopping { now_ms: 42 }),
            Record::EventBatteryMeasured(EventBatteryMeasured { now_ms: 42 }),
            Record::EventPageTurned(EventPageTurned {
                now_ms: 42,
                section: 2,
                offset: 300,
                progress_ppm: 500_000,
                path: "books/a.epub",
                title: "A title",
                chapter: "Chapter",
                page: Some(1),
                pages: Some(2),
                insert: Some(3),
                chapter_left: None,
            }),
            Record::EventScreenShown(EventScreenShown {
                last_input: 0,
                now_ms: 42,
                screen: "fixture.screen",
                width: 480,
                height: 800,
                instance: Some(7),
            }),
            Record::EventActionRun(EventActionRun {
                now_ms: 42,
                action: "fixture.run",
                flags: 0,
            }),
            Record::EventRegistryChanged(EventRegistryChanged {
                now_ms: 42,
                what: 5,
            }),
            Record::EventPreviewChanged(EventPreviewChanged { now_ms: 42 }),
            Record::EventPluginsRequested(EventPluginsRequested {
                now_ms: 42,
                request: 9,
                by: "fixture",
                plugins: "a,b",
            }),
            Record::EventPluginAnswered(EventPluginAnswered {
                now_ms: 42,
                request: 9,
                plugin: "a",
                outcome: 1,
            }),
            Record::EventSettingChanged(EventSettingChanged {
                now_ms: 42,
                key: "reader.size",
                old: &[],
            }),
            Record::EventBookOpened(opened),
            Record::EventBookClosed(book(|r| EventBookClosed {
                now_ms: r.now_ms,
                section: r.section,
                offset: r.offset,
                insert: r.insert,
                progress_ppm: r.progress_ppm,
                path: r.path,
                title: r.title,
                chapter: r.chapter,
                page: r.page,
                pages: r.pages,
                reason: r.reason,
                chapter_left: None,
            })),
            Record::EventBookFinished(book(|r| EventBookFinished {
                now_ms: r.now_ms,
                section: r.section,
                offset: r.offset,
                insert: r.insert,
                progress_ppm: r.progress_ppm,
                path: r.path,
                title: r.title,
                chapter: r.chapter,
                page: r.page,
                pages: r.pages,
                reason: r.reason,
                chapter_left: None,
            })),
            Record::EventBookMoved(EventBookMoved {
                now_ms: 42,
                from: "old.epub",
                path: "new.epub",
            }),
            Record::EventBookRemoved(EventBookRemoved {
                now_ms: 42,
                path: "old.epub",
            }),
            Record::EventLibraryChanged(EventLibraryChanged {
                now_ms: 42,
                what: 3,
                generation: 9,
                path: "books/a.epub",
            }),
            Record::EventWifiScanned(EventWifiScanned {
                now_ms: 42,
                networks: 5,
            }),
            Record::EventWifiDone(EventWifiDone {
                now_ms: 42,
                request: 3,
                ssid: "home",
                status: 0,
                reason: 0,
                state: 2,
            }),
            Record::EventNetworkWanted(EventNetworkWanted { now_ms: 42 }),
            Record::EventNetworkIdle(EventNetworkIdle { now_ms: 42 }),
            Record::EventFirmwareChanged(EventFirmwareChanged {
                now_ms: 42,
                request: 4,
                state: 1,
            }),
            Record::EventInstallDone(EventInstallDone {
                now_ms: 42,
                request: 6,
                kind: 0,
                status: 0,
                reason: 0,
                flags: 0,
                id: "fixture.plugin",
            }),
            Record::EventPluginsChanged(EventPluginsChanged { now_ms: 42 }),
            Record::EventWifiChanged(EventWifiChanged {
                now_ms: 42,
                state: 4,
            }),
            Record::EventPowerChanged(EventPowerChanged {
                now_ms: 42,
                level: 2,
                flags: 1,
                stretch: 4,
                mode: "core.offline",
            }),
            Record::EventStatusChanged(EventStatusChanged { now_ms: 42 }),
            Record::EventChargeChanged(EventChargeChanged {
                now_ms: 42,
                flags: 7,
            }),
            Record::EventBluetooth(EventBluetooth {
                now_ms: 42,
                kind: 1,
                device: 3,
                name: "pad",
                device_kind: 1,
                signal: -40,
                reason: 0,
            }),
            Record::EventHttpDone(EventHttpDone {
                now_ms: 42,
                id: 1,
                token: 2,
                status: 200,
                reason: 0,
                flags: 0,
                body_len: 10,
            }),
            Record::EventHttpProgress(EventHttpProgress {
                now_ms: 42,
                id: 1,
                token: 2,
                received: 5,
                total: 10,
                flags: 0,
            }),
            Record::EventServer(EventServer {
                now_ms: 42,
                server: 3,
                state: 0,
                reason: 0,
                address: "192.168.1.20:8080",
            }),
            Record::EventServedRequest(EventServedRequest {
                now_ms: 42,
                server: 3,
                request: 77,
                method: "POST",
                target: "/upload?x=1",
                flags: 1,
                body_len: 5_000_000_000,
            }),
            Record::EventImageReady(EventImageReady {
                now_ms: 42,
                cookie: 5,
                status: 0,
                image: 3,
                width: 480,
                height: 800,
                luma: &[],
            }),
            Record::EventSocket(EventSocket {
                now_ms: 42,
                socket: 3,
                token: 9,
                kind: 3,
                reason: 0,
                len: 12,
            }),
            Record::EventBroadcast(EventBroadcast {
                now_ms: 42,
                socket: 4,
                token: 9,
                kind: 0,
                reason: 0,
                len: 20,
                from: "192.168.0.7:9090",
            }),
            Record::EventWorkDue(EventWorkDue {
                now_ms: 42,
                token: 7,
                why: 3,
                key: "mail.sync",
            }),
            Record::EventWorkRefused(EventWorkRefused {
                now_ms: 42,
                token: 7,
                reason: 13,
                key: "mail.sync",
            }),
            Record::EventWorkAllowed(EventWorkAllowed { now_ms: 42 }),
            Record::EventLookupReady(EventLookupReady {
                now_ms: 42,
                provider: "wikipedia.lookup",
            }),
            Record::EventDocumentClosed(EventDocumentClosed {
                now_ms: 42,
                document: 3,
                reason: 4,
            }),
            Record::EventTextTyped(EventTextTyped {
                now_ms: 42,
                text: "héllo",
                flags: 1,
            }),
            Record::EventScreenOpened(EventScreenOpened {
                now_ms: 42,
                screen: "fixture.screen",
                instance: 71,
                slot: "core.home",
                flags: 3,
                handover: 22,
                params: 2,
                text_bytes: 600,
            }),
            Record::EventScreenClosed(EventScreenClosed {
                now_ms: 42,
                screen: "fixture.screen",
                instance: 71,
            }),
            Record::EventScreenHidden(EventScreenHidden {
                now_ms: 42,
                screen: "fixture.screen",
                instance: 71,
            }),
            Record::EventStackChanged(EventStackChanged { now_ms: 42 }),
            Record::EventAnswered(EventAnswered {
                now_ms: 42,
                request: 5,
                kind: 0,
                outcome: 0,
                choice: 1,
                length: 3,
                flags: 1,
            }),
            Record::EventWidgetMade(EventWidgetMade {
                now_ms: 42,
                widget: "fixture.widget",
                instance: 71,
                place: "core.home",
            }),
            Record::EventWidgetDropped(EventWidgetDropped {
                now_ms: 42,
                instance: 71,
            }),
            Record::EventWidgetChanged(EventWidgetChanged {
                now_ms: 42,
                instance: 71,
            }),
            Record::EventScreenReady(EventScreenReady {
                now_ms: 42,
                screen: "fixture.screen",
                instance: 71,
            }),
            Record::EventSpoken(EventSpoken {
                now_ms: 42,
                cookie: 8,
                status: 1,
            }),
            Record::EventDocumentChanged(EventDocumentChanged {
                now_ms: 42,
                document: 3,
                state: 1,
                progress_ppm: 250_000,
            }),
        ]
        .into_iter()
        .map(encode),
    );
    all
}

#[test]
fn decode_as_agrees_with_decode_for_every_kind() {
    let fixtures = fixtures();
    assert_eq!(
        fixtures.len(),
        kinds::ALL.len(),
        "a fixture for every kind and a kind for every fixture"
    );
    let mut seen = Vec::new();
    for bytes in &fixtures {
        let incoming = Incoming::new(bytes).unwrap();
        let full = Event::decode(bytes).unwrap();
        assert_eq!(incoming.decode(), Ok(full));
        let tag = wire::RawRecord::single(bytes).unwrap().tag();
        assert!(!seen.contains(&tag), "two fixtures of tag {tag}");
        seen.push(tag);
        for (kind_tag, probe) in kinds::ALL {
            if *kind_tag == tag {
                assert_eq!(probe(&incoming), Ok(Some(full)), "tag {tag}");
            } else {
                assert_eq!(probe(&incoming), Ok(None), "tag {tag} asked as {kind_tag}");
            }
        }
    }
    let mut kinds_seen: Vec<u32> = kinds::ALL.iter().map(|(t, _)| *t).collect();
    kinds_seen.sort_unstable();
    seen.sort_unstable();
    assert_eq!(seen, kinds_seen);
}

#[test]
fn another_kind_is_none_without_reading_the_body() {
    // The screen's text is not UTF-8: reading it fails, asking for another kind does not.
    let mut bytes = encode(Record::EventScreenShown(wire::EventScreenShown {
        last_input: 0,
        now_ms: 42,
        screen: "zz",
        width: 1,
        height: 1,
        instance: None,
    }));
    let at = bytes.windows(2).position(|w| w == b"zz").unwrap();
    bytes[at] = 0xff;
    bytes[at + 1] = 0xfe;
    let incoming = Incoming::new(&bytes).unwrap();
    assert!(incoming.is::<kinds::ScreenShown>());
    assert!(!incoming.is::<kinds::Tick>());
    assert_eq!(incoming.decode_as::<kinds::Tick>(), Ok(None));
    assert_eq!(incoming.decode_as::<kinds::ScreenShown>(), Err(Error::Utf8));
    assert_eq!(incoming.decode(), Err(Error::Utf8));
    assert_eq!(Event::decode(&bytes), Err(Error::Utf8));
}

#[test]
fn framing_and_unknown_tags_are_refused_as_before() {
    assert_eq!(Incoming::new(&[0]).unwrap_err(), Error::Invalid);
    let tick = tick();
    let mut long = tick.clone();
    long.push(0);
    assert_eq!(
        Incoming::new(&long).unwrap_err(),
        Event::decode(&long).unwrap_err()
    );
    assert_eq!(
        Incoming::new(&tick[..tick.len() - 1]).unwrap_err(),
        Event::decode(&tick[..tick.len() - 1]).unwrap_err()
    );
}

#[test]
fn debug_prints_the_tag_only() {
    let incoming_bytes = tick();
    let text = alloc::format!("{:?}", Incoming::new(&incoming_bytes).unwrap());
    assert_eq!(
        text,
        alloc::format!("Incoming {{ tag: {} }}", wire::EVENT_TICK_TAG)
    );
}
