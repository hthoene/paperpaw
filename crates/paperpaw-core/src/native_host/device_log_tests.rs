//! Lines are cut whole, dropped without a sink, and reach the sink as given.
extern crate std;
use super::*;
use core::cell::RefCell;
use std::{string::String, vec::Vec};

std::thread_local! {
    static LINES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn capture(line: &str) {
    LINES.with(|lines| lines.borrow_mut().push(line.into()));
}

fn taken() -> Vec<String> {
    LINES.with(|lines| core::mem::take(&mut *lines.borrow_mut()))
}

#[test]
fn a_line_reaches_the_sink_and_none_means_dropped() {
    taken();
    DeviceLog::new(Some(capture)).line(format_args!(
        "format skipped: {} reason {} count {}",
        "a.b", 2, 3
    ));
    DeviceLog::default().line(format_args!("never"));
    DeviceLog::default().line(format_args!("never"));
    assert_eq!(taken(), ["format skipped: a.b reason 2 count 3"]);
}

#[test]
fn a_long_line_is_cut_at_120_bytes_on_a_character_boundary() {
    taken();
    let sink = DeviceLog::new(Some(capture));
    sink.line(format_args!("{}", "x".repeat(300)));
    // Two-byte characters across the cut: never half of one.
    sink.line(format_args!("{}", "é".repeat(100)));
    sink.line(format_args!("a{}", "é".repeat(100)));
    let lines = taken();
    assert_eq!(lines[0].len(), LINE_MAX);
    assert_eq!(lines[1].len(), LINE_MAX);
    assert_eq!(
        lines[2].len(),
        LINE_MAX - 1,
        "the cut falls before the character"
    );
    assert!(lines.iter().all(|line| !line.contains('\u{fffd}')));
}

#[test]
fn a_line_of_exactly_the_room_is_whole() {
    taken();
    DeviceLog::new(Some(capture)).line(format_args!("{}", "y".repeat(LINE_MAX)));
    assert_eq!(taken()[0].len(), LINE_MAX);
}

#[test]
fn the_builders_sink_reaches_a_frame_and_a_frame_line_is_cut_whole() {
    use crate::abi7::{context::Frame, core_frame::CoreFrame};
    taken();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .device_log(capture)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let frame = CoreFrame {
        callback: None,
        instance: None,
        depth: None,
        directory: None,
        env: &env,
        meter: &kernel.shared.meter,
        stack_generation: kernel.shared.stack_rows.generation(),
        stack_rows: &kernel.shared.stack_rows,
        font_removals: &kernel.shared.font_removals,
        registry_generation: &kernel.shared.registry_generation,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
        log: kernel.shared.device_log,
        ui: None,
    };
    frame.log(format_args!("direct"));
    frame_line(
        &frame,
        format_args!("format skipped: {} reason {} count {}", "a.b", 1, 4),
    );
    frame_line(&frame, format_args!("{}", "z".repeat(500)));
    let lines = taken();
    assert_eq!(lines[0], "direct");
    assert_eq!(lines[1], "format skipped: a.b reason 1 count 4");
    assert_eq!(lines[2].len(), LINE_MAX);
    // A kernel without a sink drops everything and does not fail.
    let quiet = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    quiet.shared.device_log.line(format_args!("never"));
    assert!(taken().is_empty());
}
