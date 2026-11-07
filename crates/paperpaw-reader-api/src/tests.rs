use super::*;
use wire::reader_insert_v1::{InsertItem, Record};
fn item() -> Record<'static> {
    Record::InsertItem(InsertItem {
        side: 0,
        id: 7,
        height: 80,
        flags: 2,
    })
}
#[test]
fn insert_roundtrip_and_every_truncation() {
    let mut out = [0; 3840];
    let n = insert::encode(true, &[item()], &mut out).unwrap();
    assert_eq!(
        insert::decode(true, &out[..n]).unwrap().next(),
        Some(Ok(item()))
    );
    for end in 0..n {
        assert!(insert::decode(true, &out[..end]).is_err());
    }
    assert!(style::decode(true, &out[..n]).is_err());
    out[1] = 18;
    assert!(insert::decode(true, &out[..n]).is_err());
}
#[test]
fn limits_and_output_atomicity() {
    let mut out = [0xa5; 3840];
    assert!(insert::encode(true, &[item(); 5], &mut out).is_err());
    assert_eq!(out, [0xa5; 3840]);
    let mut tiny = [0xa5; 2];
    assert!(insert::encode(true, &[item()], &mut tiny).is_err());
    assert_eq!(tiny, [0xa5; 2]);
    assert!(insert::decode(true, &[0; 3841]).is_err());
    assert!(insert::decode(false, &[0]).is_err());
    assert!(insert::decode(true, &[0]).unwrap().next().is_none());
}
#[test]
fn stage_inputs_require_one_head_then_rows() {
    use wire::reader_layer_v1::*;
    let head = Record::LayerQuery(LayerQuery {
        section: 0,
        offset: 0,
        insert: 0,
        width: 100,
        height: 200,
        chunk: 0,
        final_chunk: 1,
    });
    let row = Record::PageLine(PageLine {
        index: 0,
        start: 0,
        end: 2,
        x: 0,
        y: 0,
        width: 10,
        height: 20,
    });
    let mut out = [0xa5; 3840];
    for records in [&[row, head][..], &[head, head][..]] {
        assert_eq!(layer::encode(false, records, &mut out), Err(Error::Invalid));
        assert_eq!(out, [0xa5; 3840]);
        let n = input_encode(records, &mut out).unwrap();
        assert!(layer::decode(false, &out[..n]).is_err());
        out.fill(0xa5);
    }
    let n = layer::encode(false, &[head, row], &mut out).unwrap();
    assert_eq!(layer::decode(false, &out[..n]).unwrap().count(), 2);
    let mut rows = [row; 65];
    rows[0] = head;
    assert!(layer::encode(false, &rows[..64], &mut out).is_ok());
    assert!(layer::encode(false, &rows, &mut out).is_err());
    assert!(layer::decode(true, &out[..n]).is_err());
}
#[test]
fn payloads_are_not_stages_and_reject_trailing_records() {
    use wire::reader_draw_v1::*;
    let record = Record::DrawItem(DrawItem {
        section: 0,
        offset: 12,
        insert: 0,
        id: 4,
    });
    let mut out = [0; 256];
    let n = input_encode(&[record], &mut out).unwrap();
    assert_eq!(input_decode(&out[..n]).unwrap().next(), Some(Ok(record)));
    assert!(input_encode(&[record, record], &mut out).is_err());
    assert_eq!(wire::STAGES.len(), 5);
}
#[test]
fn arbitrary_small_inputs_never_panic() {
    for first in 0_u8..=255 {
        for len in 0..=32 {
            let bytes = [first; 32];
            let _ = insert::decode(true, &bytes[..len]);
            let _ = style::decode(true, &bytes[..len]);
            let _ = decor::decode(true, &bytes[..len]);
            let _ = layer::decode(true, &bytes[..len]);
            let _ = status::decode(true, &bytes[..len]);
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

#[test]
fn all_reply_families_roundtrip() {
    macro_rules! roundtrip {
        ($stage:ident, $codec:ident, $record:expr) => {{
            use wire::$codec::*;
            let record = $record;
            let mut out = [0; 3840];
            let n = $stage::encode(true, &[record], &mut out).unwrap();
            assert_eq!(
                $stage::decode(true, &out[..n]).unwrap().next(),
                Some(Ok(record))
            );
            assert!($stage::decode(false, &out[..n]).is_err());
        }};
    }
    roundtrip!(
        style,
        reader_style_v1,
        Record::StyleRun(StyleRun {
            start: 0,
            end: 2,
            set: 0,
            clear: 0,
            color: Some(0x123456),
            weight: None,
            family: "",
            letter_spacing: None,
            word_spacing: None
        })
    );
    roundtrip!(
        decor,
        reader_decor_v1,
        Record::DecorRange(DecorRange {
            start: 0,
            end: 2,
            role: "reader.mark",
            id: 1,
            flags: 1
        })
    );
    roundtrip!(
        layer,
        reader_layer_v1,
        Record::LayerItem(LayerItem {
            id: 1,
            side: 0,
            room: 12,
            flags: 1
        })
    );
    roundtrip!(
        status,
        reader_status_v1,
        Record::StatusItem(StatusItem {
            id: 1,
            width: 10,
            height: 20,
            text: "ä",
            flags: 0
        })
    );
}

#[test]
fn current_styles_are_distinct_from_proposed_styles() {
    use wire::reader_style_v1::*;
    let head = Record::StyleQuery(StyleQuery {
        section: 0,
        start: 0,
        end: 2,
        text: "ä",
        chunk: 0,
        final_chunk: 1,
    });
    let row = Record::StyleNowRun(StyleNowRun {
        start: 0,
        end: 2,
        set: 1,
        clear: 0,
        color: None,
        weight: None,
        family: "",
        letter_spacing: None,
        word_spacing: None,
    });
    let mut out = [0; 3840];
    let n = style::encode(false, &[head, row], &mut out).unwrap();
    assert_eq!(
        style::decode(false, &out[..n]).unwrap().nth(1),
        Some(Ok(row))
    );
    assert!(style::decode(true, &out[..n]).is_err());
    assert!(style::encode(false, &[row, head], &mut out).is_err());
    let invalid = Record::StyleQuery(StyleQuery {
        section: 0,
        start: 0,
        end: 1,
        text: "ä",
        chunk: 0,
        final_chunk: 1,
    });
    out.fill(0xa5);
    assert_eq!(
        style::encode(false, &[invalid], &mut out),
        Err(Error::Invalid)
    );
    assert_eq!(out, [0xa5; 3840]);
}
