//! UIX event ids are opaque, nonzero and borrowed from exact input records.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use wire::*;
fn encode_ui_event(record: Record<'_>) -> alloc::vec::Vec<u8> {
    let mut bytes = alloc::vec![0;4096];
    let count = records_encode(&[record], &mut bytes).unwrap();
    bytes.truncate(count);
    bytes
}

#[test]
fn opened_closed_and_widget_events_validate_host_identifiers() {
    for instance in [0, 71] {
        let bytes = encode_ui_event(Record::EventScreenOpened(EventScreenOpened {
            now_ms: 1,
            screen: "fixture.screen",
            instance,
            slot: "core.home",
            flags: 3,
            handover: 22,
            params: 2,
            text_bytes: 6000,
        }));
        if instance == 0 {
            assert!(matches!(Event::decode(&bytes), Err(Error::Invalid)));
            continue;
        }
        let Event::ScreenOpened {
            screen,
            instance,
            handover,
            params,
            text_bytes,
            again,
            restored,
            ..
        } = Event::decode(&bytes).unwrap()
        else {
            panic!("opened");
        };
        assert_eq!(instance.get(), 71);
        assert_eq!(handover.unwrap().get(), 22);
        assert_eq!((params, text_bytes, again, restored), (2, 6000, true, true));
        assert!(bytes.as_ptr_range().contains(&screen.as_ptr()));
    }
    for record in [
        Record::EventScreenClosed(EventScreenClosed {
            now_ms: 1,
            screen: "fixture.screen",
            instance: 0,
        }),
        Record::EventWidgetMade(EventWidgetMade {
            now_ms: 1,
            widget: "fixture.widget",
            instance: 0,
            place: "core.home",
        }),
        Record::EventWidgetDropped(EventWidgetDropped {
            now_ms: 1,
            instance: 0,
        }),
        Record::EventWidgetChanged(EventWidgetChanged {
            now_ms: 1,
            instance: 0,
        }),
        Record::EventScreenReady(EventScreenReady {
            now_ms: 1,
            screen: "fixture.screen",
            instance: 0,
        }),
    ] {
        assert!(matches!(
            Event::decode(&encode_ui_event(record)),
            Err(Error::Invalid)
        ));
    }
}
#[test]
fn answered_live_flags_and_unknown_outcomes_are_preserved_safely() {
    for (outcome, expected) in [
        (0, AnswerOutcome::Answered),
        (1, AnswerOutcome::Cancelled),
        (2, AnswerOutcome::Typing),
        (99, AnswerOutcome::Cancelled),
    ] {
        let bytes = encode_ui_event(Record::EventAnswered(EventAnswered {
            now_ms: 2,
            request: 9,
            kind: 0,
            outcome,
            choice: 0,
            length: 512,
            flags: 7,
        }));
        let Event::Answered { answer, .. } = Event::decode(&bytes).unwrap() else {
            panic!("answered");
        };
        assert_eq!(answer.request().get(), 9);
        assert_eq!(answer.outcome(), expected);
        assert_eq!(answer.text_bytes(), 512);
        assert!(answer.typing_stopped() && answer.dialog_closed() && answer.not_started());
    }
    let bytes = encode_ui_event(Record::EventAnswered(EventAnswered {
        now_ms: 2,
        request: 0,
        kind: 0,
        outcome: 1,
        choice: 0,
        length: 0,
        flags: 0,
    }));
    assert!(matches!(Event::decode(&bytes), Err(Error::Invalid)));
}
#[test]
fn ui_events_refuse_every_truncation_and_screen_shown_preserves_missing_ids() {
    let bytes = encode_ui_event(Record::EventAnswered(EventAnswered {
        now_ms: 2,
        request: 9,
        kind: 0,
        outcome: 0,
        choice: 0,
        length: 0,
        flags: 0,
    }));
    for end in 0..bytes.len() {
        assert!(Event::decode(&bytes[..end]).is_err());
    }
    for instance in [None, Some(0), Some(4)] {
        let bytes = encode_ui_event(Record::EventScreenShown(EventScreenShown {
            last_input: 0,
            now_ms: 1,
            screen: "fixture.screen",
            width: 400,
            height: 600,
            instance,
        }));
        match Event::decode(&bytes) {
            Ok(Event::ScreenShown { instance: got, .. }) => {
                assert_eq!(got.map(crate::native::ui::Instance::get), instance)
            }
            Err(Error::Invalid) => assert_eq!(instance, Some(0)),
            other => panic!("unexpected shown:{other:?}"),
        }
    }
}
