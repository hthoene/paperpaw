//! Stage schemas keep local tags, share borrowed codecs and reject malformed payloads.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_core::native_host::stages_wire::{
    STAGES, core_input_filter_v1 as filter, core_status_v1 as status, core_widget_v1 as widget,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE};

macro_rules! round_trip {
    ($module:ident, $variant:ident, $value:expr) => {{
        let value = $value;
        let record = $module::Record::$variant(value);
        let mut out = vec![0xa5; 3840];
        let len = value.encode(&mut out).unwrap();
        assert_eq!($module::$variant::decode(&out[..len]), Ok(value));
        let decoded = $module::records_decode(&out[..len])
            .unwrap()
            .collect::<Result<Vec<_>, i32>>()
            .unwrap();
        assert_eq!(decoded, [record]);
        let mut short = vec![0xa5; len - 1];
        assert_eq!(value.encode(&mut short), Err(ERR_NO_SPACE));
        assert!(short.iter().all(|byte| *byte == 0xa5));
        assert_eq!($module::$variant::decode(&out[..len - 1]), Err(ERR_INVALID));
        out[len] = 0;
        assert!($module::records_decode(&out[..=len]).is_err());
    }};
}

fn varint(mut value: u32, out: &mut Vec<u8>) {
    loop {
        let byte = u8::try_from(value & 0x7f).unwrap();
        value >>= 7;
        out.push(byte | if value == 0 { 0 } else { 0x80 });
        if value == 0 {
            break;
        }
    }
}

fn blob(bytes: &[u8], out: &mut Vec<u8>) {
    varint(u32::try_from(bytes.len()).unwrap(), out);
    out.extend_from_slice(bytes);
}

fn frame(tag: u32, body: &[u8]) -> Vec<u8> {
    let mut out = vec![1];
    varint(tag, &mut out);
    varint(u32::try_from(body.len()).unwrap(), &mut out);
    out.extend_from_slice(body);
    out
}

#[test]
fn filter_records_round_trip_at_every_field_cap() {
    let input = [0x5a; 128];
    let action = "a".repeat(128);
    let raise = "r".repeat(64);
    round_trip!(
        filter,
        FilterInput,
        filter::FilterInput::new(&input, u32::MAX)
    );
    round_trip!(
        filter,
        FilterVerdict,
        filter::FilterVerdict::new(5, &action, &input, &raise)
    );
}

#[test]
fn all_widget_records_round_trip_at_scalar_limits() {
    round_trip!(
        widget,
        WidgetMeasure,
        widget::WidgetMeasure::new(u32::MAX, u32::MAX)
    );
    round_trip!(
        widget,
        WidgetPlace,
        widget::WidgetPlace::new(u32::MAX, i32::MIN, i32::MAX, u32::MAX, u32::MAX)
    );
    round_trip!(
        widget,
        WidgetTargets,
        widget::WidgetTargets::new(u32::MAX, i32::MIN, i32::MAX, u32::MAX, u32::MAX)
    );
    round_trip!(
        widget,
        WidgetSize,
        widget::WidgetSize::new(u32::MAX, u32::MAX)
    );
    round_trip!(
        widget,
        WidgetTarget,
        widget::WidgetTarget::new(u32::MAX, i32::MIN, i32::MAX, u32::MAX, u32::MAX)
    );
}

#[test]
fn status_records_round_trip_at_effective_list_and_field_caps() {
    // Eight 128-byte items plus seven separators is the largest valid list.
    let items = vec!["a".repeat(128); 8].join("\n");
    let context = [0xc3; 1024];
    let text = "t".repeat(128);
    round_trip!(status, StatusAsk, status::StatusAsk::new(&items, &context));
    round_trip!(
        status,
        StatusValue,
        status::StatusValue::new(&text, u32::MAX, &text)
    );
}

#[test]
fn filter_input_cap_is_checked_on_encode_and_decode() {
    let input = [0x7f; 129];
    let value = filter::FilterInput::new(&input, 0);
    let mut out = [0xa5; 3840];
    assert_eq!(value.encode(&mut out), Err(ERR_INVALID));
    assert!(out.iter().all(|byte| *byte == 0xa5));
    let mut body = vec![0];
    blob(&input, &mut body);
    body.extend_from_slice(&0_u32.to_le_bytes());
    assert_eq!(
        filter::FilterInput::decode(&frame(filter::FILTER_INPUT_TAG, &body)),
        Err(ERR_INVALID)
    );
}

#[test]
fn every_filter_verdict_blob_cap_is_checked_in_both_directions() {
    for (action, input, raise) in [
        ("a".repeat(129), vec![], String::new()),
        (String::new(), vec![0; 129], String::new()),
        (String::new(), vec![], "r".repeat(65)),
    ] {
        let value = filter::FilterVerdict::new(4, &action, &input, &raise);
        assert_eq!(value.encode(&mut [0; 3840]), Err(ERR_INVALID));
        let mut body = vec![0];
        body.extend_from_slice(&4_u32.to_le_bytes());
        blob(action.as_bytes(), &mut body);
        blob(&input, &mut body);
        blob(raise.as_bytes(), &mut body);
        assert_eq!(
            filter::FilterVerdict::decode(&frame(filter::FILTER_VERDICT_TAG, &body)),
            Err(ERR_INVALID)
        );
    }
}

#[test]
fn status_ask_rejects_bad_lists_and_context_caps_in_both_directions() {
    for items in [
        "\nitem".to_owned(),
        "item\n".to_owned(),
        "a\n\nb".to_owned(),
        ["item"; 9].join("\n"),
        "a".repeat(129),
        "a".repeat(1033),
    ] {
        let ask = status::StatusAsk::new(&items, &[]);
        assert_eq!(ask.encode(&mut [0; 3840]), Err(ERR_INVALID));
        let mut body = vec![0];
        blob(items.as_bytes(), &mut body);
        blob(&[], &mut body);
        assert_eq!(
            status::StatusAsk::decode(&frame(status::STATUS_ASK_TAG, &body)),
            Err(ERR_INVALID)
        );
    }
    let context = [0; 1025];
    let ask = status::StatusAsk::new("item", &context);
    assert_eq!(ask.encode(&mut [0; 3840]), Err(ERR_INVALID));
    let mut body = vec![0];
    blob(b"item", &mut body);
    blob(&context, &mut body);
    assert_eq!(
        status::StatusAsk::decode(&frame(status::STATUS_ASK_TAG, &body)),
        Err(ERR_INVALID)
    );
}

#[test]
fn status_value_rejects_both_text_caps_in_both_directions() {
    for (item, text) in [
        ("i".repeat(129), String::new()),
        (String::new(), "t".repeat(129)),
    ] {
        let value = status::StatusValue::new(&item, 0, &text);
        assert_eq!(value.encode(&mut [0; 3840]), Err(ERR_INVALID));
        let mut body = vec![0];
        blob(item.as_bytes(), &mut body);
        body.extend_from_slice(&0_u32.to_le_bytes());
        blob(text.as_bytes(), &mut body);
        assert_eq!(
            status::StatusValue::decode(&frame(status::STATUS_VALUE_TAG, &body)),
            Err(ERR_INVALID)
        );
    }
}

#[test]
fn every_schema_rejects_foreign_tags_missing_fields_and_trailing_bytes() {
    for bytes in [
        frame(511, &[0]),
        frame(1, &[0]),
        vec![1, 1, 0],
        vec![0, 0],
        vec![0x80, 0],
        vec![1, 0x81, 0, 0],
        vec![],
    ] {
        assert_eq!(filter::records_decode(&bytes).err(), Some(ERR_INVALID));
        assert_eq!(status::records_decode(&bytes).err(), Some(ERR_INVALID));
        assert_eq!(widget::records_decode(&bytes).err(), Some(ERR_INVALID));
    }
    let mut body = vec![0];
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&2_u32.to_le_bytes());
    body.extend_from_slice(&[3, 4, 5]);
    let mut bytes = frame(widget::WIDGET_MEASURE_TAG, &body);
    assert_eq!(
        widget::WidgetMeasure::decode(&bytes),
        Ok(widget::WidgetMeasure::new(1, 2))
    );
    bytes.push(0);
    assert_eq!(widget::WidgetMeasure::decode(&bytes), Err(ERR_INVALID));
}

#[test]
fn local_tags_are_interpreted_only_by_the_selected_stage_schema() {
    assert_eq!(filter::FILTER_INPUT_TAG, status::STATUS_ASK_TAG);
    assert_eq!(status::STATUS_ASK_TAG, widget::WIDGET_MEASURE_TAG);
    let mut bytes = [0; 128];
    let len = (widget::WidgetMeasure::new(1, 2))
        .encode(&mut bytes)
        .unwrap();
    // A local tag has no global family: these bytes also form a valid filter
    // input followed by unknown appended fields. The stage key selects the codec.
    assert_eq!(
        filter::FilterInput::decode(&bytes[..len]),
        Ok(filter::FilterInput::new(&[0], 131_072))
    );
    assert_eq!(
        status::StatusAsk::decode(&bytes[..len]),
        Ok(status::StatusAsk::new("\0", &[]))
    );
}

#[test]
fn stage_catalog_exposes_the_three_schema_caps_and_local_tags() {
    assert_eq!(STAGES.len(), 3);
    assert_eq!(filter::MAX_SUBSCRIBERS, 4);
    assert_eq!(widget::MAX_SUBSCRIBERS, 32);
    assert_eq!(status::MAX_SUBSCRIBERS, 32);
    assert_eq!(
        (filter::REPLY_ROWS, widget::REPLY_ROWS, status::REPLY_ROWS),
        (1, 32, 8)
    );
    assert_eq!(filter::INPUT_TAGS, &[1]);
    assert_eq!(filter::REPLY_TAGS, &[2]);
    assert_eq!(widget::INPUT_TAGS, &[1, 2, 3]);
    assert_eq!(widget::REPLY_TAGS, &[4, 5]);
    assert_eq!((status::MAX_INPUT, status::MAX_REPLY), (3840, 3840));
}

#[test]
fn a_later_foreign_tag_is_rejected_before_the_stream_is_exposed() {
    let mut bytes = [0; 128];
    let len = (widget::WidgetMeasure::new(1, 2))
        .encode(&mut bytes)
        .unwrap();
    let mut batch = bytes[..len].to_vec();
    batch[0] = 2;
    batch.extend_from_slice(&frame(511, &[0])[1..]);
    assert_eq!(widget::records_decode(&batch).err(), Some(ERR_INVALID));
}

#[test]
fn widget_target_stream_holds_the_maximum_reply_rows() {
    let target = widget::WidgetTarget::new(1, -1, 2, 3, 4);
    let records = vec![widget::Record::WidgetTarget(target); 32];
    let mut bytes = [0; 3840];
    let len = widget::records_encode(&records, &mut bytes).unwrap();
    let decoded = widget::records_decode(&bytes[..len])
        .unwrap()
        .collect::<Result<Vec<_>, i32>>()
        .unwrap();
    assert_eq!(decoded, records);
}

#[test]
fn directional_decoders_check_tags_count_and_empty_input() {
    let input = widget::Record::WidgetMeasure(widget::WidgetMeasure::new(7, 8));
    let reply = widget::Record::WidgetSize(widget::WidgetSize::new(9, 10));
    let mut bytes = [0xa5; 3840];
    let len = widget::input_encode(&[input], &mut bytes).unwrap();
    assert_eq!(widget::input_decode(&bytes[..len]).unwrap().len(), 1);
    assert_eq!(widget::reply_decode(&bytes[..len]).err(), Some(ERR_INVALID));
    let len = widget::reply_encode(&[reply], &mut bytes).unwrap();
    assert_eq!(widget::reply_decode(&bytes[..len]).unwrap().len(), 1);
    assert_eq!(widget::input_decode(&bytes[..len]).err(), Some(ERR_INVALID));
    let len = widget::records_encode(&[input, input], &mut bytes).unwrap();
    assert_eq!(widget::input_decode(&bytes[..len]).err(), Some(ERR_INVALID));
    assert!(widget::reply_decode(&[0]).unwrap().is_empty());
    assert_eq!(widget::input_decode(&[0]).err(), Some(ERR_INVALID));
    let mut untouched = [0xa5; 128];
    assert_eq!(
        widget::input_encode(&[reply], &mut untouched),
        Err(ERR_INVALID)
    );
    assert_eq!(
        widget::reply_encode(&[input], &mut untouched),
        Err(ERR_INVALID)
    );
    assert_eq!(widget::input_encode(&[], &mut untouched), Err(ERR_INVALID));
    assert_eq!(untouched, [0xa5; 128]);
    let replies = [reply; 33];
    assert_eq!(
        widget::reply_encode(&replies, &mut untouched),
        Err(ERR_INVALID)
    );
}

#[test]
fn unknown_extended_bitmap_and_fields_stay_inside_the_record() {
    let mut body = vec![2, 0, 1];
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&8_u32.to_le_bytes());
    body.extend_from_slice(&9_u32.to_le_bytes());
    let bytes = frame(widget::WIDGET_MEASURE_TAG, &body);
    assert_eq!(
        widget::WidgetMeasure::decode(&bytes),
        Ok(widget::WidgetMeasure::new(7, 8))
    );
    assert_eq!(widget::input_decode(&bytes).unwrap().len(), 1);
    assert_eq!(
        widget::input_decode(&frame(widget::WIDGET_MEASURE_TAG, &[255])).err(),
        Some(ERR_INVALID)
    );
}
