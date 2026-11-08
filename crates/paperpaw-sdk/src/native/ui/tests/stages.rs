use super::*;
use crate::native::{
    stage_schemas::{core_status_v1 as status, core_widget_v1 as widget},
    ui::stages::{StatusAnswer, StatusCall, WidgetCall},
};

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
                u32::try_from(index).unwrap(),
                crate::native::callbacks::FrameRect::new(0, i32::try_from(index).unwrap(), 30, 4),
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
        crate::native::callbacks::FrameRect::new(0, 0, 1, 1),
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

#[test]
fn widget_requests_are_exact_and_reply_failures_leave_output_untouched() {
    let mut payload = [0; 256];
    let mut out = [0xa5; 256];
    let n = widget::input_encode(
        &[widget::Record::WidgetMeasure(widget::WidgetMeasure::new(
            4, 300,
        ))],
        &mut payload,
    )
    .unwrap();
    let measure = WidgetCall::decode(&payload[..n]).unwrap();
    assert!(matches!(measure,WidgetCall::Measure{instance,width:300} if instance.get()==4));
    assert!(matches!(
        measure.reply_targets(&[], &mut out),
        Err(Error::State)
    ));
    assert_eq!(out, [0xa5; 256]);
    assert!(matches!(
        measure.reply_size(20, 10, &mut out[..1]),
        Err(Error::NoSpace)
    ));
    assert_eq!(out, [0xa5; 256]);
    let written = measure.reply_size(20, 10, &mut out).unwrap();
    let row = widget::WidgetSize::decode(&out[..written]).unwrap();
    assert_eq!((row.height, row.min_height), (20, 10));
    for i in 0..n {
        assert!(WidgetCall::decode(&payload[..i]).is_err());
    }
    payload[n] = 0;
    assert!(WidgetCall::decode(&payload[..=n]).is_err());
    let n = widget::input_encode(
        &[widget::Record::WidgetPlace(widget::WidgetPlace::new(
            4, -8, 7, 300, 400,
        ))],
        &mut payload,
    )
    .unwrap();
    let place = WidgetCall::decode(&payload[..n]).unwrap();
    assert!(matches!(place,WidgetCall::Place{rect,..} if rect.x == -8 && rect.height==400));
    let written = place.reply_place(&mut out).unwrap();
    assert_eq!(widget::reply_decode(&out[..written]).unwrap().len(), 0);
    let n = widget::input_encode(
        &[widget::Record::WidgetMeasure(widget::WidgetMeasure::new(
            0, 300,
        ))],
        &mut payload,
    )
    .unwrap();
    assert!(matches!(
        WidgetCall::decode(&payload[..n]),
        Err(Error::Invalid)
    ));
}

#[test]
fn status_duplicate_unrequested_and_overfull_values_never_publish_bytes() {
    let mut payload = [0; 1024];
    let mut out = [0xa5; 1024];
    let items = (0..status::REPLY_ROWS)
        .map(|n| alloc::format!("fixture.item{n}"))
        .collect::<Vec<_>>();
    let list = items.join("\n");
    let n = status::input_encode(
        &[status::Record::StatusAsk(status::StatusAsk::new(
            &list,
            &[],
        ))],
        &mut payload,
    )
    .unwrap();
    let call = StatusCall::decode(&payload[..n]).unwrap();
    let value = StatusAnswer::text("fixture.item0", "Value").unwrap();
    assert!(matches!(
        call.reply(&[value, value], &mut out),
        Err(Error::Invalid)
    ));
    assert_eq!(out, [0xa5; 1024]);
    assert!(matches!(
        call.reply(&vec![value; status::REPLY_ROWS + 1], &mut out),
        Err(Error::NoSpace)
    ));
    assert_eq!(out, [0xa5; 1024]);
    let values = items
        .iter()
        .map(|item| StatusAnswer::text(item, "值").unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(
        call.reply(&values, &mut out[..1]),
        Err(Error::NoSpace)
    ));
    assert_eq!(out, [0xa5; 1024]);
    let n = call.reply(&values, &mut out).unwrap();
    assert_eq!(
        status::reply_decode(&out[..n]).unwrap().len(),
        status::REPLY_ROWS
    );
    let count = status::input_encode(
        &[status::Record::StatusAsk(status::StatusAsk::new(
            "fixture.item0",
            &[0xff],
        ))],
        &mut payload,
    )
    .unwrap();
    assert!(StatusCall::decode(&payload[..count]).is_err());
}

#[test]
fn status_requests_reject_truncation_trailing_and_wrong_records() {
    let mut context = [0; 256];
    let mut payload = [0; 512];
    let context_len = wire::record_encode_one(
        &wire::Record::ServiceUiParam(wire::ServiceUiParam {
            key: "title",
            value: "Book",
        }),
        &mut context,
    )
    .unwrap();
    let n = status::input_encode(
        &[status::Record::StatusAsk(status::StatusAsk::new(
            "fixture.clock",
            &context[..context_len],
        ))],
        &mut payload,
    )
    .unwrap();
    assert!(StatusCall::decode(&payload[..n]).is_ok());
    for len in 0..n {
        assert!(StatusCall::decode(&payload[..len]).is_err(), "prefix {len}");
    }
    payload[n] = 0;
    assert!(StatusCall::decode(&payload[..=n]).is_err());
    let n = status::reply_encode(
        &[status::Record::StatusValue(status::StatusValue::new(
            "fixture.clock",
            0,
            "12:00",
        ))],
        &mut payload,
    )
    .unwrap();
    assert!(StatusCall::decode(&payload[..n]).is_err());
    let n = wire::record_encode_one(
        &wire::Record::ServiceUiHandle(wire::ServiceUiHandle { handle: 1 }),
        &mut context,
    )
    .unwrap();
    let n = status::input_encode(
        &[status::Record::StatusAsk(status::StatusAsk::new(
            "fixture.clock",
            &context[..n],
        ))],
        &mut payload,
    )
    .unwrap();
    assert!(StatusCall::decode(&payload[..n]).is_err());
    for len in 1..context_len {
        let n = wire::record_encode_one(
            &wire::Record::ServiceUiParam(wire::ServiceUiParam {
                key: "title",
                value: "Book",
            }),
            &mut context,
        )
        .unwrap();
        assert_eq!(n, context_len);
        let n = status::input_encode(
            &[status::Record::StatusAsk(status::StatusAsk::new(
                "fixture.clock",
                &context[..len],
            ))],
            &mut payload,
        )
        .unwrap();
        assert!(
            StatusCall::decode(&payload[..n]).is_err(),
            "context prefix {len}"
        );
    }
}
