use super::*;
use paperpaw_sdk::usage::Refreshes;

#[test]
fn generation_follows_rows_only_and_wraps() {
    let meter = Meter::new(None);
    assert_eq!(meter.generation.get(), 0);
    assert_eq!(meter.table.borrow()[0].plugin.as_str(), CORE);
    drop(meter.enter("reader"));
    assert_eq!(meter.generation.get(), 1);
    meter.shown("reader", 1);
    meter.awake("reader", 1, "test".into());
    meter.network("reader", false);
    meter.refreshed("reader", RefreshMode::Fast, Rect::new(0, 0, 1, 1));
    meter.synced();
    drop(meter.enter("reader"));
    assert_eq!(meter.generation.get(), 1);
    meter.generation.set(u32::MAX);
    meter.shown("second", 1);
    assert_eq!(meter.generation.get(), 0);
}

#[test]
fn every_plugin_counter_saturates() {
    let meter = Meter::new(None);
    let index = meter.index("reader");
    {
        let mut table = meter.table.borrow_mut();
        let row = &mut table[index];
        row.calls = u32::MAX;
        row.busy_us = u64::MAX;
        row.shown_ms = u32::MAX;
        row.read = u32::MAX;
        row.written = u32::MAX;
        row.requests = u32::MAX;
        row.refused = u32::MAX;
        row.awake_ms = u32::MAX;
        row.refreshes = Refreshes::new(u32::MAX, u32::MAX, u32::MAX);
        row.pixels = u64::MAX;
    }
    let _running = meter.enter("reader");
    meter.syncs.borrow_mut().resize(index + 1, u32::MAX);
    meter.synced();
    meter.add_busy(index, 1);
    meter.io(1, 1);
    meter.shown("reader", 1);
    meter.awake("reader", 1, "test".into());
    meter.network("reader", true);
    meter.network("reader", false);
    for mode in [RefreshMode::Fast, RefreshMode::Partial, RefreshMode::Full] {
        meter.refreshed("reader", mode, Rect::new(0, 0, 1, 1));
    }
    let table = meter.table.borrow();
    let row = &table[index];
    assert_eq!(
        (row.calls, row.shown_ms, row.read, row.written),
        (u32::MAX, u32::MAX, u32::MAX, u32::MAX)
    );
    assert_eq!(
        (row.requests, row.refused, row.awake_ms),
        (u32::MAX, u32::MAX, u32::MAX)
    );
    assert_eq!((row.busy_us, row.pixels), (u64::MAX, u64::MAX));
    assert_eq!(row.refreshes, Refreshes::new(u32::MAX, u32::MAX, u32::MAX));
    assert_eq!(meter.syncs.borrow()[index], u32::MAX);
}

#[test]
fn row_visitors_report_conflicts_and_release_borrows() {
    let meter = Meter::new(None);
    let pointer = meter.table.borrow()[0].plugin.as_str().as_ptr();
    let end = meter
        .visit_rows(0, &mut |row| {
            assert_eq!(row.plugin.as_ptr(), pointer);
            assert_eq!(row.syncs, 0);
            core::ops::ControlFlow::Continue(())
        })
        .unwrap();
    assert_eq!((end.total, end.next, end.generation), (1, 1, 0));
    assert!(meter.table.try_borrow_mut().is_ok());
    let table = meter.table.borrow_mut();
    assert_eq!(
        meter.visit_rows(0, &mut |_| core::ops::ControlFlow::Continue(())),
        Err(paperpaw_native::host_glue::ERR_STATE)
    );
    drop(table);
    let _syncs = meter.syncs.borrow_mut();
    assert_eq!(
        meter.visit_rows(0, &mut |_| core::ops::ControlFlow::Continue(())),
        Err(paperpaw_native::host_glue::ERR_STATE)
    );
}

#[test]
fn awake_reason_caps_preserve_utf8_and_exact_boundary_text() {
    let meter = Meter::new(None);
    for (text, expected) in [
        ("a".repeat(64), 64),
        (alloc::format!("{}é", "a".repeat(63)), 63),
        (alloc::format!("{}🦊", "a".repeat(62)), 62),
    ] {
        meter.awake("reader", 1, text.clone().into());
        meter
            .visit_rows(1, &mut |row| {
                assert_eq!(row.awake_reason, &text[..expected]);
                core::ops::ControlFlow::Continue(())
            })
            .unwrap();
    }
}

#[test]
fn a_scoped_entry_charges_the_inner_plugin_and_pauses_the_outer() {
    let meter = Meter::new(None);
    let _outer = meter.enter("caller");
    {
        let _inner = meter.enter_scoped("callee");
        meter.io(5, 7);
        assert_eq!(meter.running.borrow().len(), 2);
    }
    meter.io(1, 2);
    let table = meter.table.borrow();
    let row = |id: &str| table.iter().find(|p| p.plugin.as_str() == id).unwrap();
    assert_eq!((row("callee").read, row("callee").written), (5, 7));
    assert_eq!((row("caller").read, row("caller").written), (1, 2));
    assert_eq!((row("callee").calls, row("caller").calls), (1, 1));
    drop(table);
    assert_eq!(meter.running.borrow().len(), 1, "the callee left");
}
