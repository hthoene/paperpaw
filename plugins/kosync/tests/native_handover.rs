//! Confirmed sync handover through the real kernel and reader, without account UI.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
mod host;
use host::World;
use paperpaw_sdk::{Instant, Params, builtin::slots, positions};

#[test]
fn accepting_a_remote_position_hands_it_to_the_reader_and_persists_it() {
    let mut world = World::signed_in();
    assert_eq!(world.kernel().settings().text("kosync.user"), "test-reader");
    world.kernel().open_slot(
        Instant(0),
        slots::READER,
        Params::new().with("path", "book.txt"),
    );
    world.pump();
    for _ in 0..3 {
        world.kernel().render(&mut |_, _| {});
        world.pump();
    }
    world.assert_clean();
    let active = world.kernel().active_screen().cloned();
    let warnings = world.kernel().take_warnings();
    assert_eq!(
        world.network.requests.borrow().len(),
        1,
        "reader opening must fetch remote progress; active={active:?}, warnings={warnings:?}"
    );
    world.complete(
        0,
        200,
        br#"{"percentage":0.8,"device_id":"another","device":"Other reader","progress":""}"#,
    );
    assert_eq!(
        world.kernel().active_screen().unwrap().as_str(),
        "core.confirm-dialog"
    );
    world.action("core.left");
    world.action("core.confirm");
    assert_eq!(
        world.kernel().active_screen().unwrap().as_str(),
        "reader.screen",
        "the accepted handover must reach Reader after the dialog closes"
    );
    wait_for_position(&mut world);
    world.action("core.back");
    world.assert_clean();
    let position =
        positions::load(&world.card, "book.txt").expect("reader must save the accepted position");
    assert!(
        (position.percent - 80.0).abs() <= 0.1,
        "saved reader position: {position:?}"
    );
    assert!(position.at.offset > 0);
    let before_reopen = world.network.requests.borrow().len();
    let now = world.network.now.get();
    world.kernel().open_slot(
        Instant(now),
        slots::READER,
        Params::new().with("path", "book.txt"),
    );
    world.pump();
    wait_for_position(&mut world);
    let fetches: Vec<_> = world
        .network
        .requests
        .borrow()
        .iter()
        .enumerate()
        .skip(before_reopen)
        .filter(|(_, request)| request.method == "GET")
        .map(|(index, _)| index)
        .collect();
    for index in fetches {
        world.complete(
            index,
            200,
            br#"{"percentage":0.8,"device_id":"another","device":"Other reader","progress":""}"#,
        );
    }
    assert_eq!(
        world.kernel().active_screen().unwrap().as_str(),
        "reader.screen",
        "reopening the saved handover must not offer the same remote position again"
    );
    world.assert_clean();
}

fn wait_for_position(world: &mut World) {
    for _ in 0..20 {
        world.kernel().render(&mut |_, _| {});
        world.pump();
        if world
            .kernel()
            .describe()
            .iter()
            .any(|part| part.label == "80 %")
        {
            return;
        }
    }
    let labels: Vec<_> = world
        .kernel()
        .describe()
        .into_iter()
        .map(|part| part.label)
        .collect();
    let warnings = world.kernel().take_warnings();
    let screens: Vec<_> = world
        .kernel()
        .screens()
        .map(|id| id.as_str().to_owned())
        .collect();
    panic!(
        "reader did not show the accepted 80 % position; screens={screens:?}, labels={labels:?}, warnings={warnings:?}"
    );
}
