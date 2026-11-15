//! The reading settings' pages show a sample of text above their rows, as in Beta 3: the
//! reader's widget (`reader.sample`) previews the group, and the Settings page composes
//! it in the room the list keeps. The X4's small memory class shows it too, its nested-call
//! gate being open by default (#250); a host that refuses the widget (the gate closed with
//! the feature `x4-gate-closed`, as the X3 builds) leaves the page without it, rows whole.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;
use paperpaw_platform::MemoryClass;

/// The Layout page of the reading settings on a device of `memory`: what it shows.
fn layout_page(memory: MemoryClass) -> Vec<String> {
    let mut app = App::new(X4.with_memory(memory), Shared::memory(BASE_CONFIG, &[]));
    let mut params = Params::new();
    params.set("id", "core.group.reading-layout");
    app.open_screen(Id::new_static("settings.group"), params);
    app.labels()
}

#[test]
fn the_layout_page_shows_a_sample_of_text_over_its_rows() {
    let labels = layout_page(MemoryClass::Large);
    assert!(labels.iter().any(|l| l == "Preview"), "{labels:?}");
    assert!(
        labels.iter().any(|l| l.contains("Line spacing")),
        "{labels:?}"
    );
}

#[test]
#[cfg_attr(
    feature = "x4-gate-closed",
    ignore = "the gate is closed: the refusing host is the test next to this one"
)]
fn the_small_memory_class_shows_the_sample_too_with_the_gate_open() {
    let labels = layout_page(MemoryClass::Small);
    assert!(labels.iter().any(|l| l == "Preview"), "{labels:?}");
    assert!(
        labels.iter().any(|l| l.contains("Line spacing")),
        "{labels:?}"
    );
}

#[test]
#[cfg_attr(
    not(feature = "x4-gate-closed"),
    ignore = "the X4 gate is open by default (#250); run with x4-gate-closed (CI does)"
)]
fn a_host_that_refuses_the_widget_leaves_the_page_without_it_but_with_its_rows() {
    let labels = layout_page(MemoryClass::Small);
    assert!(!labels.iter().any(|l| l == "Preview"), "{labels:?}");
    assert!(
        labels.iter().any(|l| l.contains("Line spacing")),
        "{labels:?}"
    );
}
