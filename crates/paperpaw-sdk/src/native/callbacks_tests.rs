#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use alloc::vec::Vec;

fn encode(records: &[Record<'_>]) -> Vec<u8> {
    let mut bytes = [0; 512];
    let len = wire::records_encode(records, &mut bytes).unwrap();
    bytes[..len].to_vec()
}

#[test]
fn key_states_preserve_borrowed_unknown_key_names() {
    for (state, expected) in [KeyState::Press, KeyState::Release, KeyState::Repeat]
        .into_iter()
        .enumerate()
    {
        let bytes = encode(&[Record::InputKey(wire::InputKey {
            now_ms: u64::MAX,
            key: "future-button",
            state: state as u32,
        })]);
        let Input::Key { now_ms, key, state } = Input::decode(&bytes).unwrap() else {
            panic!("key expected")
        };
        assert_eq!(now_ms, u64::MAX);
        assert_eq!(state, expected);
        assert_eq!(key, "future-button");
        assert!(key.as_ptr() >= bytes.as_ptr());
        assert!(key.as_bytes().as_ptr_range().end <= bytes.as_ptr_range().end);
    }
}

#[test]
fn pointer_phases_and_gesture_kinds_keep_signed_positions() {
    for (phase, expected) in [
        PointerPhase::Down,
        PointerPhase::Move,
        PointerPhase::Up,
        PointerPhase::Cancel,
    ]
    .into_iter()
    .enumerate()
    {
        let bytes = encode(&[Record::InputPointer(wire::InputPointer {
            now_ms: 73,
            x: i32::MIN,
            y: i32::MAX,
            phase: phase as u32,
        })]);
        assert_eq!(
            Input::decode(&bytes),
            Ok(Input::Pointer {
                now_ms: 73,
                position: Point::new(i32::MIN, i32::MAX),
                phase: expected,
            })
        );
    }
    for (kind, expected) in [
        InputGestureKind::Tap,
        InputGestureKind::LongPress,
        InputGestureKind::SwipeLeft,
        InputGestureKind::SwipeRight,
        InputGestureKind::SwipeUp,
        InputGestureKind::SwipeDown,
    ]
    .into_iter()
    .enumerate()
    {
        let bytes = encode(&[Record::InputGesture(wire::InputGesture {
            now_ms: 91,
            x: -17,
            y: 29,
            kind: kind as u32,
        })]);
        assert_eq!(
            Input::decode(&bytes),
            Ok(Input::Gesture {
                now_ms: 91,
                position: Point::new(-17, 29),
                kind: expected,
            })
        );
    }
}

#[test]
fn action_and_invalid_enum_values_are_decoded_strictly() {
    let bytes = encode(&[Record::InputAction(wire::InputAction {
        now_ms: 42,
        action: "core.page-next",
    })]);
    assert_eq!(
        Input::decode(&bytes),
        Ok(Input::Action {
            now_ms: 42,
            action: "core.page-next"
        })
    );
    for record in [
        Record::InputKey(wire::InputKey {
            now_ms: 1,
            key: "Power",
            state: 3,
        }),
        Record::InputPointer(wire::InputPointer {
            now_ms: 1,
            x: 0,
            y: 0,
            phase: 4,
        }),
        Record::InputGesture(wire::InputGesture {
            now_ms: 1,
            kind: 6,
            x: 0,
            y: 0,
        }),
        Record::InputGesture(wire::InputGesture {
            now_ms: 1,
            kind: u32::MAX,
            x: 0,
            y: 0,
        }),
    ] {
        assert_eq!(Input::decode(&encode(&[record])), Err(Error::Invalid));
    }
}

fn frame(purpose: u32) -> wire::DrawFrame<'static> {
    wire::DrawFrame {
        purpose,
        x: i32::MIN,
        y: i32::MAX,
        width: u32::MAX,
        height: 70_000,
        clip_top: -31,
        clip_bottom: 45,
        id: 0x1234_5678,
        payload: b"frame-data",
    }
}

#[test]
fn frames_preserve_every_field_and_unknown_purposes() {
    for (purpose, expected) in [
        DrawPurpose::Screen,
        DrawPurpose::PageLayer,
        DrawPurpose::InsertedBlock,
        DrawPurpose::StatusItem,
        DrawPurpose::Widget,
        DrawPurpose::Other(5),
    ]
    .into_iter()
    .enumerate()
    {
        let bytes = encode(&[Record::DrawFrame(frame(purpose as u32))]);
        let decoded = DrawFrame::decode(&bytes).unwrap();
        assert_eq!(decoded.purpose, expected);
        assert_eq!(
            decoded.rect,
            FrameRect::new(i32::MIN, i32::MAX, u32::MAX, 70_000)
        );
        assert_eq!(
            (decoded.clip_top, decoded.clip_bottom, decoded.id),
            (-31, 45, 0x1234_5678)
        );
        assert_eq!(decoded.payload, b"frame-data");
        assert!(decoded.payload.as_ptr() >= bytes.as_ptr());
        assert!(decoded.payload.as_ptr_range().end <= bytes.as_ptr_range().end);
    }
}

#[test]
fn callback_decoders_require_exact_single_records_and_complete_framing() {
    let input = Record::InputAction(wire::InputAction {
        now_ms: 1,
        action: "core.back",
    });
    let draw = Record::DrawFrame(frame(4));
    assert_eq!(Input::decode(&encode(&[draw])), Err(Error::Unsupported));
    assert_eq!(
        DrawFrame::decode(&encode(&[input])),
        Err(Error::Unsupported)
    );
    for record in [input, draw] {
        let bytes = encode(&[record]);
        for end in 0..bytes.len() {
            assert!(Input::decode(&bytes[..end]).is_err());
            assert!(DrawFrame::decode(&bytes[..end]).is_err());
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(Input::decode(&trailing), Err(Error::Invalid));
        assert_eq!(DrawFrame::decode(&trailing), Err(Error::Invalid));
        let two = encode(&[record, record]);
        assert_eq!(Input::decode(&two), Err(Error::Invalid));
        assert_eq!(DrawFrame::decode(&two), Err(Error::Invalid));
    }
    // An unassigned tag in the input growth block must stay unknown.
    assert_eq!(Input::decode(&[1, 193, 1, 1, 0]), Err(Error::Unsupported));
    assert_eq!(
        DrawFrame::decode(&[1, 193, 1, 1, 0]),
        Err(Error::Unsupported)
    );
}
