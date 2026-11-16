//! What the old adapter's tests pinned of production parts, told through
//! the production route instead (`Kernel::emit`, `core_events.rs`, the
//! pump): the conversion of a page turn (`flat/page.rs` `turn`, `counters`,
//! `optional`, `ppm`, and the caps of `events::page_turned_record`), the
//! guard's rule over native plugins, and the minute tick (`native_tick.rs`).
//! One test here for each of `flat/event_tests.rs` and
//! `flat/routing_tests.rs` that drove these through `FlatPlugin` (#244,
//! `FLATPLUGIN-TESTS.md`), with the same cases.

use super::*;
use paperpaw_sdk::event::EventData;

/// The page-turned records plugin `a` was told, decoded.
fn turns(w: &World) -> Vec<(u32, u32, u32, usize, usize, usize, Option<u32>, Option<u32>)> {
    w.pages
        .borrow()
        .iter()
        .map(|bytes| match codec::decode_any_single(bytes).unwrap() {
            wire::Record::EventPageTurned(r) => (
                r.section,
                r.offset,
                r.progress_ppm,
                r.path.len(),
                r.title.len(),
                r.chapter.len(),
                r.page,
                r.pages,
            ),
            _ => panic!("not a page turn"),
        })
        .collect()
}

/// The page and the pages of the last page turn told.
fn last_counters(w: &World) -> (Option<u32>, Option<u32>) {
    let all = turns(w);
    let last = all.last().expect("a page turn was told");
    (last.6, last.7)
}

/// The page-turn params of `page`, with `key` set to `value`.
fn page_with(key: &'static str, value: &str) -> Params {
    let mut params = page("t");
    params.set(key, value);
    params
}

/// Was `malformed_page_producers_drop_without_invoking_the_plugin`: a page
/// turn without its location, with a section that is no number or a
/// progress that is none, is not told, and the next one is.
#[test]
fn a_page_turn_with_a_part_that_is_no_number_is_dropped_and_the_next_is_told() {
    let mut w = world(ALL, &[]);
    w.kernel.take_warnings();
    for params in [
        Params::new(),
        page_with("section", "bad"),
        page_with("progress", "bad"),
    ] {
        raise(&mut w.kernel, wire::EVENT_PAGE_TURNED_TAG, &params);
    }
    assert!(tags(&w, 'a').is_empty());
    assert!(!w.kernel.take_warnings().is_empty(), "the drop is logged");
    raise(&mut w.kernel, wire::EVENT_PAGE_TURNED_TAG, &page("ok"));
    assert_eq!(tags(&w, 'a'), [wire::EVENT_PAGE_TURNED_TAG]);
}

/// Was `page_fields_are_borrowed_clipped_and_counters_remain_optional`:
/// strings over their caps are cut at a char boundary, the same from params
/// and from a carried `Reading`, the record stays under 1,100 bytes, and
/// counters nobody gave are absent.
#[test]
fn a_page_turn_s_strings_are_cut_to_their_caps_and_its_counters_stay_optional() {
    let mut w = world(ALL, &[]);
    let reading = builtin::Reading::new(
        "é".repeat(300),
        "é".repeat(200),
        33.33,
        2,
        400,
        "é".repeat(200),
    );
    for params in [reading.to_params(), Params::new().carrying(reading)] {
        raise(&mut w.kernel, wire::EVENT_PAGE_TURNED_TAG, &params);
    }
    let pages = w.pages.borrow();
    assert_eq!(pages.len(), 2);
    assert_eq!(
        pages[0], pages[1],
        "params and a carried Reading are told alike"
    );
    assert!(pages[0].len() < 1100);
    drop(pages);
    assert_eq!(
        turns(&w)[0],
        (2, 400, 333_300, 512, 256, 256, None, None),
        "section, offset, progress, the cut strings, no counters"
    );
}

/// Was `progress_conversion_clamps_rounds_and_handles_nonfinite_values`.
#[test]
fn a_page_turn_s_progress_is_clamped_rounded_and_never_not_a_number() {
    let mut w = world(ALL, &[]);
    let cases = [
        (0.0, 0),
        (100.0, 1_000_000),
        (33.33, 333_300),
        (f32::NAN, 0),
        (f32::NEG_INFINITY, 0),
        (f32::INFINITY, 1_000_000),
        (-1.0, 0),
        (101.0, 1_000_000),
        (0.00006, 1),
    ];
    for (percent, _) in cases {
        let progress = alloc::string::ToString::to_string(&percent);
        raise(
            &mut w.kernel,
            wire::EVENT_PAGE_TURNED_TAG,
            &page_with("progress", &progress),
        );
    }
    let told: Vec<u32> = turns(&w).iter().map(|t| t.2).collect();
    let expected: Vec<u32> = cases.iter().map(|c| c.1).collect();
    assert_eq!(told, expected, "every one is told, none dropped");
}

/// The page and the pages a page turn is told with, given `page` and `pages`.
fn told_counters(page: u32, pages: u32) -> (Option<u32>, Option<u32>) {
    let mut w = world(ALL, &[]);
    let mut params = page_with("page", &alloc::format!("{page}"));
    params.set("pages", &alloc::format!("{pages}"));
    raise(&mut w.kernel, wire::EVENT_PAGE_TURNED_TAG, &params);
    assert_eq!(tags(&w, 'a'), [wire::EVENT_PAGE_TURNED_TAG], "not dropped");
    last_counters(&w)
}

/// Was `zero_page_counter_does_not_drop_a_page_turn`.
#[test]
fn a_zero_page_or_count_makes_both_counters_absent_and_keeps_the_page_turn() {
    assert_eq!(told_counters(0, 10), (None, None));
    assert_eq!(told_counters(4, 0), (None, None));
}

/// Was `page_past_count_does_not_drop_a_page_turn`.
#[test]
fn a_page_past_the_count_makes_both_counters_absent_and_keeps_the_page_turn() {
    assert_eq!(told_counters(11, 10), (None, None));
    assert_eq!(told_counters(4, 10), (Some(4), Some(10)));
}

/// Was `unparsable_optional_counter_is_unavailable_without_dropping_the_location`.
#[test]
fn a_counter_that_is_no_number_is_absent_and_the_location_is_still_told() {
    for (key, other, expected) in [
        ("pages", ("page", "4"), (Some(4), None)),
        ("page", ("pages", "10"), (None, Some(10))),
    ] {
        let mut w = world(ALL, &[]);
        let mut params = page_with(key, "bad");
        params.set(other.0, other.1);
        raise(&mut w.kernel, wire::EVENT_PAGE_TURNED_TAG, &params);
        assert_eq!(last_counters(&w), expected);
    }
}

/// Was `independently_available_page_counters_are_preserved`: a page
/// without the count, or the count without the page, is told as it is, from
/// params and from a carried `Reading`.
#[test]
fn a_page_without_the_count_or_the_count_alone_is_told_as_it_is() {
    let reading = || builtin::Reading::new("a".into(), "A".into(), 0.0, 0, 0, "".into());
    for (made, expected) in [
        (reading().page(4), (Some(4), None)),
        (reading().pages(10), (None, Some(10))),
    ] {
        for params in [made.to_params(), Params::new().carrying(made.clone())] {
            let mut w = world(ALL, &[]);
            raise(&mut w.kernel, wire::EVENT_PAGE_TURNED_TAG, &params);
            assert_eq!(last_counters(&w), expected);
        }
    }
}

/// The ticks plugin `name` was told, by their clock.
fn ticks(w: &World, name: char) -> Vec<u64> {
    w.told
        .borrow()
        .iter()
        .filter(|(who, tag, _)| *who == name && *tag == wire::EVENT_TICK_TAG)
        .map(|(_, _, now)| *now)
        .collect()
}

/// Ticks the kernel at `at` and tells what waits.
fn tick_at(w: &mut World, at: u64) {
    w.kernel.tick(Instant(at));
    while w.kernel.native_setting_pump() {}
}

/// Was `a_native_plugin_gets_one_tick_a_minute`: the first call starts the
/// minute, nothing comes early, a long sleep gives one tick, a clock that
/// went back starts the minute again, and the plugin's other subscriptions
/// get none.
#[test]
fn a_native_plugin_gets_one_tick_a_minute_on_the_production_route() {
    let mut w = world(ALL, &[]);
    let at = |w: &mut World, ms: u64| {
        tick_at(w, ms);
        ticks(w, 'a')
    };
    assert!(
        at(&mut w, 1_000).is_empty(),
        "the first call starts the minute"
    );
    assert!(at(&mut w, 60_999).is_empty(), "not a minute yet");
    assert_eq!(at(&mut w, 61_000), [61_000]);
    assert_eq!(at(&mut w, 100_000), [61_000], "none before the next minute");
    // A device that slept for an hour gets one tick when it wakes.
    assert_eq!(at(&mut w, 3_700_000), [61_000, 3_700_000]);
    assert_eq!(at(&mut w, 3_700_500), [61_000, 3_700_000]);
    // The clock went back: the minute starts again there.
    assert_eq!(at(&mut w, 10_000), [61_000, 3_700_000]);
    assert_eq!(at(&mut w, 69_999), [61_000, 3_700_000]);
    assert_eq!(at(&mut w, 70_000), [61_000, 3_700_000, 70_000]);
    let others = w
        .told
        .borrow()
        .iter()
        .filter(|t| t.1 != wire::EVENT_TICK_TAG)
        .count();
    assert_eq!(others, 0, "a tick is no other event");
}

/// Was `a_failing_tick_plugin_does_not_stop_the_healthy_one`.
#[test]
fn a_failing_tick_plugin_does_not_stop_the_healthy_one_on_the_production_route() {
    let mut w = world(&[wire::EVENT_TICK_TAG], &[wire::EVENT_TICK_TAG]);
    w.b_fails.set(true);
    for at in [1_000, 61_000, 121_000] {
        tick_at(&mut w, at);
    }
    assert_eq!(
        ticks(&w, 'a'),
        [61_000, 121_000],
        "the healthy one hears each"
    );
    assert!(ticks(&w, 'b').is_empty());
}

/// A screen of plugin `a` that guards the device and raises `STARTED` on
/// confirm.
struct Guard;
impl paperpaw_sdk::Screen for Guard {
    fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}
    fn guards(&self) -> bool {
        true
    }
    fn on_action(
        &mut self,
        action: &Id,
        cx: &mut paperpaw_sdk::Cx<'_, '_>,
    ) -> paperpaw_sdk::Handled {
        if *action != paperpaw_sdk::builtin::actions::CONFIRM {
            return paperpaw_sdk::Handled::No;
        }
        cx.command(builtin::STARTED.emit(()));
        paperpaw_sdk::Handled::Yes
    }
}

/// Was the native half of `guard_owned_events_filter_flat_and_rust_handlers_identically`:
/// an event a guarding screen raises reaches the native plugin the guard
/// belongs to and no other; the same event raised by the shell reaches every
/// subscriber.
#[test]
fn an_event_a_guard_raises_reaches_its_own_plugin_alone() {
    let manifest = paperpaw_sdk::Manifest::new(
        Id::new_static("a"),
        "A",
        paperpaw_sdk::Version::new(1, 0, 0),
    );
    let builder = crate::Kernel::builder(crate::tests::DEVICE).items(manifest, |items| {
        items.add_screen(paperpaw_sdk::ScreenSpec::new(
            Id::new_static("a.guard"),
            "guard",
            alloc::boxed::Box::new(|_, _| alloc::boxed::Box::new(Guard)),
        ))
    });
    let mut w = world_on(builder, ALL, ALL);
    w.kernel
        .open_screen(Instant(1), Id::new_static("a.guard"), Params::new());
    w.kernel
        .run_action(Instant(2), paperpaw_sdk::builtin::actions::CONFIRM);
    while w.kernel.native_setting_pump() {}
    assert_eq!(
        tags(&w, 'a'),
        [wire::EVENT_STARTED_TAG],
        "the guard's own plugin"
    );
    assert!(tags(&w, 'b').is_empty(), "no other plugin");
    w.kernel
        .send_event(Instant(3), builtin::STARTED.id().clone(), Params::new());
    while w.kernel.native_setting_pump() {}
    assert_eq!(
        tags(&w, 'b'),
        [wire::EVENT_STARTED_TAG],
        "the shell's reaches every one"
    );
}
