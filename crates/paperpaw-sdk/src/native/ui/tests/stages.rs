use super::*;
use crate::native::{
    callbacks::{Input, KeyState},
    stage_schemas::{
        core_input_filter_v1 as filter, core_status_v1 as status, core_widget_v1 as widget,
    },
    ui::stages::{FilterCall, FilterDecision, StatusAnswer, StatusCall, WidgetCall},
};

#[test]
fn filter_calls_and_replacements_use_directional_generated_codecs() {
    let mut inner = [0; 128];
    let mut payload = [0; 4096];
    let mut reply = [0; 4096];
    let count = wire::records_encode(
        &[wire::Record::InputKey(wire::InputKey {
            now_ms: 7,
            key: "next",
            state: 1,
        })],
        &mut inner,
    )
    .unwrap();
    let count = filter::input_encode(
        &[filter::Record::FilterInput(filter::FilterInput::new(
            &inner[..count],
            0,
        ))],
        &mut payload,
    )
    .unwrap();
    let call = FilterCall::decode(&payload[..count]).unwrap();
    assert!(matches!(
        call.input(),
        Input::Key {
            now_ms: 7,
            key: "next",
            state: KeyState::Release
        }
    ));
    let mut replacement = [0; 128];
    let count = FilterDecision::Replace(call.input())
        .encode(&mut replacement, &mut reply)
        .unwrap();
    let mut decoded = filter::reply_decode(&reply[..count]).unwrap();
    let filter::Record::FilterVerdict(row) = decoded.next().unwrap().unwrap() else {
        panic!("verdict");
    };
    assert_eq!(row.kind, 4);
    assert_eq!(Input::decode(row.input).unwrap(), call.input());
    let long = alloc::format!(
        "input.{}",
        "a".repeat(filter::FILTER_VERDICT_RAISE_MAX_BYTES)
    );
    assert!(matches!(
        FilterDecision::Raise(&long).encode(&mut replacement, &mut reply),
        Err(Error::NoSpace)
    ));
    assert!(matches!(
        FilterDecision::Replace(Input::Action {
            now_ms: 0,
            action: "fixture.action"
        })
        .encode(&mut replacement, &mut reply),
        Err(Error::Invalid)
    ));
}
#[test]
fn widget_stage_targets_are_whole_and_bounded_by_generated_rows() {
    let mut payload = [0; 4096];
    let mut reply = [0; 4096];
    let count = widget::input_encode(
        &[widget::Record::WidgetTargets(widget::WidgetTargets::new(
            4, 0, 0, 300, 400,
        ))],
        &mut payload,
    )
    .unwrap();
    let call = WidgetCall::decode(&payload[..count]).unwrap();
    let rows: Vec<_> = (0..widget::REPLY_ROWS)
        .map(|index| {
            WidgetTarget::new(
                index as u32,
                paperpaw_platform::Rect::new(0, index as i32, 30, 4),
            )
        })
        .collect();
    let count = call.reply_targets(&rows, &mut reply).unwrap();
    assert_eq!(
        widget::reply_decode(&reply[..count]).unwrap().len(),
        widget::REPLY_ROWS
    );
    let mut over = rows;
    over.push(WidgetTarget::new(
        99,
        paperpaw_platform::Rect::new(0, 0, 1, 1),
    ));
    assert!(matches!(
        call.reply_targets(&over, &mut reply),
        Err(Error::NoSpace)
    ));
    assert!(matches!(
        call.reply_size(10, 5, &mut reply),
        Err(Error::State)
    ));
}
#[test]
fn status_context_and_values_remain_borrowed_and_refuse_over_caps() {
    let mut context = [0; 1024];
    let mut payload = [0; 4096];
    let mut reply = [0; 4096];
    let count = wire::record_encode_one(
        &wire::Record::ServiceUiParam(wire::ServiceUiParam {
            key: "title",
            value: "界",
        }),
        &mut context,
    )
    .unwrap();
    let count = status::input_encode(
        &[status::Record::StatusAsk(status::StatusAsk::new(
            "fixture.clock",
            &context[..count],
        ))],
        &mut payload,
    )
    .unwrap();
    let call = StatusCall::decode(&payload[..count]).unwrap();
    let param = call.context().next().unwrap().unwrap();
    assert_eq!((param.key(), param.value()), ("title", "界"));
    assert!(payload.as_ptr_range().contains(&param.value().as_ptr()));
    let count = call
        .reply(&[StatusAnswer::drawn("fixture.clock").unwrap()], &mut reply)
        .unwrap();
    let mut rows = status::reply_decode(&reply[..count]).unwrap();
    let status::Record::StatusValue(row) = rows.next().unwrap().unwrap() else {
        panic!("status");
    };
    assert_eq!((row.item, row.flags, row.text), ("fixture.clock", 1, ""));
    let over = "x".repeat(status::STATUS_VALUE_TEXT_MAX_BYTES + 1);
    assert!(matches!(
        StatusAnswer::text("fixture.clock", &over),
        Err(Error::NoSpace)
    ));
    assert!(matches!(
        call.reply(&[StatusAnswer::text("other.item", "").unwrap()], &mut reply),
        Err(Error::State)
    ));
}
