//! The generated stage batch writers write, record by record, the bytes the
//! slice encoders write, and refuse what those refuse without touching the
//! rows written before.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_core::native_host::stages_wire::{
    core_format_v1 as format, core_status_v1 as status, core_widget_v1 as widget,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE};

fn target(n: i32) -> widget::Record {
    widget::Record::WidgetTarget(widget::WidgetTarget::new(1, n, 2, 3, 4))
}

#[test]
fn a_reply_writer_writes_the_bytes_of_reply_encode_for_every_row_count() {
    for rows in [0, 1, 2, 31, widget::REPLY_ROWS] {
        let records: Vec<_> = (0..rows)
            .map(|n| target(i32::try_from(n).unwrap()))
            .collect();
        let mut want = [0xa5; 3840];
        let len = widget::reply_encode(&records, &mut want).unwrap();
        let mut out = [0x5a; 3840];
        let mut writer = widget::ReplyWriter::new(&mut out).unwrap();
        for record in &records {
            writer.push(record).unwrap();
        }
        assert_eq!(writer.len(), rows);
        assert_eq!(writer.is_empty(), rows == 0);
        assert_eq!(writer.finish(), Ok(len));
        assert_eq!(out[..len], want[..len], "{rows} rows");
        assert!(out[len..].iter().all(|byte| *byte == 0x5a));
    }
}

#[test]
fn a_refused_push_writes_nothing_and_keeps_the_rows_before() {
    let input = widget::Record::WidgetMeasure(widget::WidgetMeasure::new(7, 8));
    let records = [target(1), target(2)];
    let mut want = [0; 256];
    let len = widget::reply_encode(&records, &mut want).unwrap();
    // Room for exactly the two rows: a third does not fit `out`.
    let mut out = vec![0x5a; len + 3];
    let mut writer = widget::ReplyWriter::new(&mut out).unwrap();
    for record in &records {
        writer.push(record).unwrap();
    }
    assert_eq!(writer.push(&input), Err(ERR_INVALID), "an input tag");
    assert_eq!(writer.push(&target(3)), Err(ERR_NO_SPACE));
    assert_eq!(writer.len(), 2);
    assert_eq!(writer.finish(), Ok(len));
    assert_eq!(out[..len], want[..len]);
    assert_eq!(out[len..], [0x5a; 3]);
}

#[test]
fn a_writer_keeps_the_row_cap_and_the_byte_cap() {
    let mut out = [0; 8000];
    let mut writer = widget::ReplyWriter::new(&mut out).unwrap();
    for n in 0..widget::REPLY_ROWS {
        writer.push(&target(i32::try_from(n).unwrap())).unwrap();
    }
    assert_eq!(
        writer.push(&target(0)),
        Err(ERR_INVALID),
        "one row too many"
    );
    assert_eq!(writer.len(), widget::REPLY_ROWS);
    // MAX_REPLY counts even where `out` has room: 64 images of 256-byte paths
    // are more than a format reply holds.
    let path = "p".repeat(256);
    let image = format::Record::FmtImage(format::FmtImage::new(&path, 1, 2, 3, 0));
    let mut out = vec![0; 3 * format::MAX_REPLY];
    let mut writer = format::ReplyWriter::new(&mut out).unwrap();
    let mut pushed = 0;
    let refused = loop {
        match writer.push(&image) {
            Ok(()) => pushed += 1,
            Err(status) => break status,
        }
    };
    assert_eq!(refused, ERR_NO_SPACE);
    assert!(pushed > 0 && pushed < format::REPLY_ROWS);
    let len = writer.finish().unwrap();
    assert!(len <= format::MAX_REPLY);
    let mut want = vec![0; format::MAX_REPLY];
    let mut rows = vec![image; pushed];
    assert_eq!(format::reply_encode(&rows, &mut want), Ok(len));
    assert_eq!(out[..len], want[..len]);
    rows.push(image);
    assert_eq!(format::reply_encode(&rows, &mut out), Err(ERR_NO_SPACE));
}

#[test]
fn an_input_writer_takes_one_record_and_refuses_none_or_bad_fields() {
    let input = widget::Record::WidgetMeasure(widget::WidgetMeasure::new(7, 8));
    let mut want = [0; 64];
    let len = widget::input_encode(&[input], &mut want).unwrap();
    let mut out = [0; 64];
    let mut writer = widget::InputWriter::new(&mut out).unwrap();
    assert_eq!(writer.push(&target(1)), Err(ERR_INVALID), "a reply tag");
    writer.push(&input).unwrap();
    assert_eq!(writer.push(&input), Err(ERR_INVALID), "one input record");
    assert_eq!(writer.finish(), Ok(len));
    assert_eq!(out[..len], want[..len]);
    let mut out = [0; 64];
    assert_eq!(
        widget::InputWriter::new(&mut out).unwrap().finish(),
        Err(ERR_INVALID)
    );
    // A list field out of its bounds, as `input_encode` refuses it.
    let ask = status::Record::StatusAsk(status::StatusAsk::new("a\n\nb", &[]));
    let mut out = [0x5a; 64];
    let mut writer = status::InputWriter::new(&mut out).unwrap();
    assert_eq!(writer.push(&ask), Err(ERR_INVALID));
    assert!(writer.is_empty());
    assert_eq!(out[1..], [0x5a; 63]);
}

#[test]
fn a_writer_needs_room_for_the_count() {
    assert_eq!(widget::ReplyWriter::new(&mut []).err(), Some(ERR_NO_SPACE));
    let mut one = [9];
    assert_eq!(widget::ReplyWriter::new(&mut one).unwrap().finish(), Ok(1));
    assert_eq!(one, [0]);
    assert!(widget::reply_decode(&one).unwrap().is_empty());
}
