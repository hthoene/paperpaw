//! Ops 6, 7, 12 and 13: who may lay out an instance, the guard and private
//! rules, the owner's one `core.widget` call each, an owner that fails or
//! answers badly, the queue of taps, and the refused allocations.
use super::*;
use crate::native_host::stages_wire::core_widget_v1 as widget;
use paperpaw_native::host_glue::{ERR_IO, ERR_NO_MEMORY};

/// A target row: id, x, y, width, height.
type Row = (u32, i32, i32, u32, u32);

fn area_record(instance: u32) -> Record<'static> {
    Record::ServiceUiWidgetArea(wire::ServiceUiWidgetArea {
        instance,
        x: 3,
        y: 4,
        width: 200,
        height: 100,
    })
}

fn tap(instance: u32, kind: u32) -> Record<'static> {
    Record::ServiceUiWidgetInput(wire::ServiceUiWidgetInput {
        instance,
        target: 2,
        x: 10,
        y: 11,
        kind,
    })
}

fn stage_reply(records: &[widget::Record]) -> Vec<u8> {
    let mut bytes = std::vec![0; 3840];
    let used = widget::reply_encode(records, &mut bytes).unwrap();
    bytes.truncate(used);
    bytes
}

fn targets(count: u32) -> Vec<u8> {
    let rows: Vec<widget::Record> = (1..=count)
        .map(|n| widget::Record::WidgetTarget(widget::WidgetTarget::new(n, 0, 0, 10, 10)))
        .collect();
    stage_reply(&rows)
}

impl World<'_> {
    /// An instance of `widget` for `ME`; Core's drain is wired, so op 6 is served.
    fn made(&self, widget: &str) -> u32 {
        self.hosts.ui().unwrap().widget_input_drained().unwrap();
        self.make(ME, widget).unwrap().0
    }
    fn measure(&self, owner: u64, instance: u32) -> Result<(u32, u32), i32> {
        let reply = self.call(owner, 12, &[area_record(instance)])?;
        match wire::record_decode_one(&reply).unwrap() {
            Record::ServiceUiSize(size) => Ok((size.height, size.min_height)),
            other => panic!("{other:?}"),
        }
    }
    fn place(&self, owner: u64, instance: u32) -> Result<(), i32> {
        self.call(owner, 13, &[area_record(instance)]).map(|_| ())
    }
    fn tap(&self, owner: u64, instance: u32, kind: u32) -> Result<(), i32> {
        self.call(owner, 6, &[tap(instance, kind)]).map(|_| ())
    }
    /// The rows of op 7 and the `end`'s total.
    fn targets(&self, owner: u64, instance: u32) -> Result<(Vec<Row>, u32), i32> {
        let reply = self.call(owner, 7, &[area_record(instance)])?;
        let (mut rows, mut total) = (Vec::new(), 0);
        crate::native_host::stage_wire::visit_stream(&reply, |_, framed| {
            match wire::record_decode_one(framed).unwrap() {
                Record::ServiceUiTarget(t) => rows.push((t.target, t.x, t.y, t.width, t.height)),
                Record::ServiceEnd(e) => total = e.total,
                other => panic!("{other:?}"),
            }
            Ok(())
        })
        .unwrap();
        Ok((rows, total))
    }
    fn widget_reply(&self, owner: u64, token: u32, reply: Result<Vec<u8>, i32>) {
        let mut replies = self.widget_replies.borrow_mut();
        replies.retain(|(o, t, _)| (*o, *t) != (owner, token));
        replies.push((owner, token, reply));
    }
    fn widget_calls(&self) -> usize {
        self.widget_asked.borrow().len()
    }
}

#[test]
fn measure_place_and_targets_make_one_owner_call_each_with_the_instance() {
    world(|w| {
        let id = w.made("a.w");
        w.widget_reply(
            A,
            7,
            Ok(stage_reply(&[widget::Record::WidgetSize(
                widget::WidgetSize::new(40, 20),
            )])),
        );
        assert_eq!(w.measure(ME, id), Ok((40, 20)));
        assert_eq!(
            w.widget_asked.borrow().last(),
            Some(&(
                A,
                7,
                widget::Record::WidgetMeasure(widget::WidgetMeasure::new(id, 200))
            ))
        );
        // The place has no reply rows; the owner is told where it sits.
        w.widget_reply(A, 7, Ok(stage_reply(&[])));
        assert_eq!(w.place(ME, id), Ok(()));
        assert_eq!(
            w.widget_asked.borrow().last(),
            Some(&(
                A,
                7,
                widget::Record::WidgetPlace(widget::WidgetPlace::new(id, 3, 4, 200, 100))
            ))
        );
        // Targets keep the owner's order and end with their count.
        w.widget_reply(A, 7, Ok(targets(3)));
        let (rows, total) = w.targets(ME, id).unwrap();
        assert_eq!(total, 3);
        assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(
            w.widget_asked.borrow().last(),
            Some(&(
                A,
                7,
                widget::Record::WidgetTargets(widget::WidgetTargets::new(id, 3, 4, 200, 100))
            ))
        );
        assert_eq!(w.widget_calls(), 3);
    });
}

#[test]
fn only_the_maker_lays_out_an_instance_and_an_own_one_is_busy_without_a_call() {
    world(|w| {
        let id = w.made("a.w");
        for op in [6_u32, 7, 12, 13] {
            // Another plugin's instance, and one nobody made.
            assert_eq!(
                w.call(A, op, &[call_record(op, id)]).err(),
                Some(ERR_DENIED),
                "op {op}"
            );
            assert_eq!(
                w.call(ME, op, &[call_record(op, id + 50)]).err(),
                Some(ERR_NOT_FOUND),
                "op {op}"
            );
            // Instance 0 is the own-widget answer of op 4: ops 6, 7, 12, 13 refuse.
            assert_eq!(
                w.call(ME, op, &[call_record(op, 0)]).err(),
                Some(ERR_BUSY),
                "op {op}"
            );
        }
        // A dropped one is gone.
        w.drop_widget(ME, id).unwrap();
        assert_eq!(w.measure(ME, id), Err(ERR_NOT_FOUND));
        assert_eq!(w.widget_calls(), 0, "no call for a refusal");
    });
}

fn call_record(op: u32, instance: u32) -> Record<'static> {
    if op == 6 {
        tap(instance, 0)
    } else {
        area_record(instance)
    }
}

#[test]
fn malformed_input_is_invalid_before_anything_happens() {
    world(|w| {
        let id = w.made("a.w");
        // A tap's kind is 0 or 1.
        assert_eq!(w.tap(ME, id, 2), Err(ERR_INVALID));
        // The wrong record for the op, and none, are refused.
        let instance = Record::ServiceUiInstance(wire::ServiceUiInstance {
            instance: id,
            flags: 0,
        });
        for op in [6_u32, 7, 12, 13] {
            assert_eq!(
                w.call(ME, op, &[instance]).err(),
                Some(ERR_INVALID),
                "op {op}"
            );
            assert!(w.call(ME, op, &[]).is_err(), "op {op}");
        }
        assert_eq!(w.widget_calls(), 0);
        assert_eq!(
            w.hosts.ui().unwrap().next_widget_input(A, 0, &mut [0; 64]),
            Ok(None)
        );
    });
}

#[test]
#[cfg_attr(feature = "x4-nested", ignore = "x4-nested opens the X4 gate (#250)")]
fn the_nested_gates_come_first_and_a_tap_needs_only_a_callback() {
    world(|w| {
        let id = w.made("a.w");
        w.widget_reply(A, 7, Ok(stage_reply(&[])));
        w.depth.set(2);
        assert_eq!(w.measure(ME, id), Err(ERR_BUSY));
        assert_eq!(w.place(ME, id), Err(ERR_BUSY));
        assert_eq!(w.targets(ME, id).err(), Some(ERR_BUSY));
        // Op 6 runs nothing inside the caller: no depth rule.
        assert_eq!(w.tap(ME, id, 0), Ok(()));
        w.depth.set(1);
        w.x4.set(true);
        assert_eq!(w.measure(ME, id), Err(ERR_UNSUPPORTED));
        assert_eq!(w.place(ME, id), Err(ERR_UNSUPPORTED));
        assert_eq!(w.targets(ME, id).err(), Some(ERR_UNSUPPORTED));
        assert_eq!(w.tap(ME, id, 1), Ok(()));
        w.x4.set(false);
        w.epoch.set(0);
        for op in [6_u32, 7, 12, 13] {
            assert_eq!(
                w.call(ME, op, &[call_record(op, id)]).err(),
                Some(ERR_STATE),
                "op {op}"
            );
        }
        assert_eq!(w.widget_calls(), 0);
    });
}

#[test]
fn under_a_guard_a_private_instance_or_one_below_it_is_denied_for_all_four() {
    world(|w| {
        let plain = w.made("a.w");
        let secret = w.made("a.secret");
        // A guard of A's under ME's screen: neither is below it, the private
        // one is denied, the other works.
        w.rows.borrow_mut().insert(0, row(9, A, true));
        w.widget_reply(A, 7, Ok(stage_reply(&[])));
        w.widget_reply(A, 8, Ok(stage_reply(&[])));
        for op in [6_u32, 7, 12, 13] {
            assert_eq!(
                w.call(ME, op, &[call_record(op, secret)]).err(),
                Some(ERR_DENIED),
                "private, op {op}"
            );
            assert!(w.call(ME, op, &[call_record(op, plain)]).is_ok(), "op {op}");
        }
        // A guard above ME's screen puts the screen below it: both denied.
        w.rows.borrow_mut().clear();
        w.rows
            .borrow_mut()
            .extend([row(10, ME, false), row(11, A, true)]);
        for op in [6_u32, 7, 12, 13] {
            assert_eq!(
                w.call(ME, op, &[call_record(op, plain)]).err(),
                Some(ERR_DENIED),
                "below, op {op}"
            );
        }
        // The guard's own plugin is not restricted.
        w.rows.borrow_mut().clear();
        w.rows.borrow_mut().extend([row(10, ME, true)]);
        assert!(w.measure(ME, plain).is_ok());
    });
}

#[test]
fn an_owner_that_fails_or_answers_badly_is_no_answer_and_the_caller_goes_on() {
    world(|w| {
        let id = w.made("a.w");
        w.widget_reply(A, 7, Err(ERR_IO));
        assert_eq!(w.measure(ME, id), Ok((0, 0)));
        assert_eq!(w.place(ME, id), Ok(()));
        assert_eq!(w.targets(ME, id), Ok((Vec::new(), 0)));
        // Bytes that are no widget reply, and a reply of the wrong rows.
        w.widget_replies.borrow_mut().clear();
        w.widget_reply(A, 7, Ok(std::vec![9, 9, 9]));
        assert_eq!(w.measure(ME, id), Ok((0, 0)));
        assert_eq!(w.targets(ME, id), Ok((Vec::new(), 0)));
        w.widget_replies.borrow_mut().clear();
        w.widget_reply(A, 7, Ok(targets(2)));
        assert_eq!(w.measure(ME, id), Ok((0, 0)));
        w.widget_replies.borrow_mut().clear();
        w.widget_reply(
            A,
            7,
            Ok(stage_reply(&[widget::Record::WidgetSize(
                widget::WidgetSize::new(5, 2),
            )])),
        );
        assert_eq!(w.targets(ME, id), Ok((Vec::new(), 0)));
    });
}

#[test]
fn the_owners_thirty_two_targets_all_fit_the_reply() {
    world(|w| {
        let id = w.made("a.w");
        w.widget_reply(A, 7, Ok(targets(32)));
        let (rows, total) = w.targets(ME, id).unwrap();
        assert_eq!((rows.len(), total), (32, 32));
    });
}

#[test]
fn a_tap_is_queued_for_its_owner_until_delivered_and_never_for_a_dropped_instance() {
    world(|w| {
        let ui = w.hosts.ui().unwrap();
        let id = w.made("a.w");
        let other = w.made("a.w");
        assert_eq!(w.tap(ME, id, 0), Ok(()));
        assert_eq!(w.tap(ME, other, 1), Ok(()));
        let mut out = [0; 128];
        // Nothing for another plugin.
        assert_eq!(ui.next_widget_input(B, 5, &mut out), Ok(None));
        let (first, length) = ui.next_widget_input(A, 5, &mut out).unwrap().unwrap();
        assert_eq!((first.instance, first.kind, first.token), (id, 0, 7));
        let Record::InputWidget(record) = wire::records_decode_single(&out[..length]).unwrap()
        else {
            panic!("not input.widget")
        };
        assert_eq!(
            (
                record.now_ms,
                record.instance,
                record.target,
                record.x,
                record.y,
                record.kind
            ),
            (5, id, 2, 10, 11, 0)
        );
        ui.widget_input_delivered(&first).unwrap();
        // The next is the other instance's; dropping it discards the tap.
        w.drop_widget(ME, other).unwrap();
        assert_eq!(ui.next_widget_input(A, 6, &mut out), Ok(None));
        // A stopped owner loses what waits for it.
        assert_eq!(w.tap(ME, id, 0), Ok(()));
        ui.plugin_stopped(A).unwrap();
        assert_eq!(ui.next_widget_input(A, 7, &mut out), Ok(None));
    });
}

#[test]
fn the_queue_holds_eight_and_the_ninth_is_full() {
    world(|w| {
        let id = w.made("a.w");
        for _ in 0..crate::native_host::ui_widget_layout::INPUTS_MAX {
            assert_eq!(w.tap(ME, id, 0), Ok(()));
        }
        assert_eq!(w.tap(ME, id, 0), Err(ERR_FULL));
        let ui = w.hosts.ui().unwrap();
        let (first, _) = ui.next_widget_input(A, 0, &mut [0; 128]).unwrap().unwrap();
        ui.widget_input_delivered(&first).unwrap();
        assert_eq!(w.tap(ME, id, 0), Ok(()));
    });
}

#[test]
fn a_refused_allocation_is_no_memory_and_leaves_nothing_queued_or_called() {
    world(|w| {
        let id = w.made("a.w");
        w.widget_reply(
            A,
            7,
            Ok(stage_reply(&[widget::Record::WidgetSize(
                widget::WidgetSize::new(1, 1),
            )])),
        );
        for nth in 1..=3 {
            w.fail.set(nth);
            let got = w.measure(ME, id);
            w.fail.set(0);
            if let Err(error) = got {
                assert_eq!(error, ERR_NO_MEMORY, "measure, allocation {nth}");
            }
        }
        w.fail.set(1);
        let got = w.tap(ME, id, 0);
        w.fail.set(0);
        assert_eq!(got, Err(ERR_NO_MEMORY));
        // Nothing was queued; the same tap works now.
        assert_eq!(
            w.hosts.ui().unwrap().next_widget_input(A, 0, &mut [0; 128]),
            Ok(None)
        );
        assert_eq!(w.tap(ME, id, 0), Ok(()));
    });
}

#[test]
fn op_6_is_unsupported_until_the_drain_is_wired_and_ops_7_12_13_never_lay_out_an_own_instance() {
    world(|w| {
        // No drain: not even a malformed tap is looked at.
        assert_eq!(w.tap(ME, 1, 0), Err(ERR_UNSUPPORTED));
        assert_eq!(w.call(ME, 6, &[]).err(), Some(ERR_UNSUPPORTED));
        assert_eq!(
            w.hosts.ui().unwrap().next_widget_input(A, 0, &mut [0; 64]),
            Ok(None)
        );
        let id = w.made("a.w");
        assert_eq!(w.tap(ME, id, 0), Ok(()));
        // An instance whose owner is its maker: ERR_BUSY before any call.
        let own = w
            .hosts
            .ui()
            .unwrap()
            .widgets
            .handles
            .try_borrow_mut()
            .unwrap()
            .make_widget(ME, ME, 10, 9, false)
            .unwrap();
        for op in [7_u32, 12, 13] {
            assert_eq!(
                w.call(ME, op, &[call_record(op, own)]).err(),
                Some(ERR_BUSY),
                "op {op}"
            );
        }
        assert_eq!(w.widget_calls(), 0);
    });
}
