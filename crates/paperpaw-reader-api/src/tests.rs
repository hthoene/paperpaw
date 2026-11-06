use super::*;

fn item() -> Record<'static> {
    Record::InsertItem(wire::InsertItem {
        side: 0,
        id: 7,
        height: 80,
        flags: 2,
    })
}

#[test]
fn insert_roundtrip_and_every_truncation() {
    let mut out = [0; 4096];
    let n = encode(Stage::Insert, true, &[item()], &mut out).unwrap();
    assert_eq!(
        decode(Stage::Insert, true, &out[..n])
            .unwrap()
            .iter()
            .next(),
        Some(item())
    );
    for end in 0..n {
        assert!(decode(Stage::Insert, true, &out[..end]).is_err());
    }
    assert_eq!(
        decode(Stage::Style, true, &out[..n]),
        Err(Error::WrongStage)
    );
    out[1] = 18; // Adjacent local tag is not recognized by range.
    assert_eq!(decode(Stage::Insert, true, &out[..n]), Err(Error::Wire(-1)));
}

#[test]
fn limits_and_output_atomicity() {
    let mut out = [0xa5; 4096];
    assert_eq!(
        encode(Stage::Insert, true, &[item(); 5], &mut out),
        Err(Error::Limit)
    );
    assert_eq!(out, [0xa5; 4096]);
    let mut tiny = [0xa5; 2];
    assert_eq!(
        encode(Stage::Insert, true, &[item()], &mut tiny),
        Err(Error::Wire(-3))
    );
    assert_eq!(tiny, [0xa5; 2]);
    assert!(decode(Stage::Insert, true, &[0; 4097]).is_err());
    assert!(decode(Stage::Insert, true, &[9]).is_err());
    assert!(decode(Stage::Insert, false, &[0]).is_err());
    assert!(decode(Stage::Insert, true, &[0]).unwrap().is_empty());
}

#[test]
fn every_stage_roundtrips() {
    let records = [
        (
            Stage::Style,
            Record::StyleRun(wire::StyleRun {
                start: 0,
                end: 2,
                set: 0,
                clear: 0,
                color: Some(0x123456),
                weight: None,
                family: "",
                letter_spacing: None,
                word_spacing: None,
            }),
        ),
        (
            Stage::Decor,
            Record::DecorRange(wire::DecorRange {
                start: 0,
                end: 2,
                role: "reader.mark",
                id: 1,
                flags: 1,
            }),
        ),
        (
            Stage::Layer,
            Record::LayerItem(wire::LayerItem {
                id: 1,
                side: 0,
                room: 12,
                flags: 1,
            }),
        ),
        (
            Stage::Status,
            Record::StatusItem(wire::StatusItem {
                id: 1,
                width: 10,
                height: 20,
                text: "ä",
                flags: 0,
            }),
        ),
    ];
    for (stage, record) in records {
        let mut bytes = [0; 4096];
        let n = encode(stage, true, &[record], &mut bytes).unwrap();
        assert_eq!(
            decode(stage, true, &bytes[..n]).unwrap().iter().next(),
            Some(record)
        );
    }
}

#[test]
fn arbitrary_small_inputs_never_panic() {
    for first in 0_u8..=255 {
        for len in 0..=32 {
            let mut bytes = [first; 32];
            if len > 1 {
                bytes[1] = first.wrapping_add(1);
            }
            for stage in [
                Stage::Insert,
                Stage::Style,
                Stage::Decor,
                Stage::Layer,
                Stage::Status,
            ] {
                let _ = decode(stage, true, &bytes[..len]);
            }
        }
    }
}

#[test]
fn pagination_and_user_precedence() {
    assert_eq!(insert_pieces(800, 800, true), Ok(8));
    assert_eq!(insert_pieces(3200, 800, true), Ok(32));
    assert_eq!(insert_pieces(3200, 800, false), Ok(1));
    assert!(insert_pieces(3201, 800, true).is_err());
    assert!(insert_pieces(1, 0, true).is_err());
    assert!(!may_override(true, false));
    assert!(may_override(true, true));
    assert!(may_override(false, false));
}

#[test]
fn aggregate_budget_and_source_boundaries() {
    assert_eq!(admit_insert(32, 3200, 800, true), Ok(64));
    assert_eq!(admit_insert(33, 3200, 800, true), Err(Error::Limit));
    assert_eq!(admit_insert(u32::MAX, 1, 800, false), Err(Error::Limit));
    assert_eq!(source_range(10, "äb", 10, 12), Ok(()));
    for (start, end) in [(9, 12), (11, 12), (10, 11), (12, 10), (10, 14)] {
        assert_eq!(source_range(10, "äb", start, end), Err(Error::Invalid));
    }
}

#[test]
fn fragment_positions_are_unique_and_progress_does_not_move() {
    let mut total_height = 0;
    let mut previous = 32;
    for piece in pieces(32, 801, 800, true, 12345).unwrap() {
        assert_eq!(piece.ordinal, previous + 1);
        assert_eq!(piece.offset, total_height);
        assert_eq!(piece.progress_ppm, 12345);
        total_height += piece.height;
        previous = piece.ordinal;
    }
    assert_eq!(total_height, 801);
    assert_eq!(previous, 41);
    assert!(pieces(64, 1, 800, false, 0).is_err());
}
