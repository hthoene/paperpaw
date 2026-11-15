//! Full native account and sync acceptance on the real host, without network I/O.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
mod host;
use host::World;
use paperpaw_sdk::{Id, Instant, Params};

fn account(world: &mut World, create: bool) {
    world
        .kernel()
        .open_screen(Instant(0), Id::new_static("kosync.account"), Params::new());
    world.pump();
    assert!(
        world.kernel().keyboard_screen().is_some(),
        "keyboard slot must be bound"
    );
    world.assert_clean();
    world.action("core.down");
    for text in ["test-reader", "dummy-test-password"] {
        world.assert_clean();
        world.action("core.confirm");
        world.assert_clean();
        let now = world.network.now.get();
        let active = world.kernel().active_screen().cloned();
        let warnings = world.kernel().take_warnings();
        assert!(
            world.kernel().type_text(Instant(now), text, true).is_some(),
            "native keyboard must accept text; active={:?}, warnings={:?}",
            active,
            warnings
        );
        world.pump();
    }
    for _ in 0..if create { 1 } else { 0 } {
        world.action("core.down");
    }
    world.action("core.confirm");
}

#[test]
fn the_native_account_logs_in_and_creates_against_the_fake_server() {
    for create in [false, true] {
        let mut world = World::new();
        account(&mut world, create);
        {
            let calls = world.network.requests.borrow();
            assert_eq!(calls.len(), 1);
            assert!(calls[0].url.ends_with(if create {
                "/users/create"
            } else {
                "/users/auth"
            }));
            assert_eq!(calls[0].method, if create { "POST" } else { "GET" });
        }
        world.complete(0, if create { 201 } else { 200 }, b"{}");
        assert_eq!(world.kernel().settings().text("kosync.user"), "test-reader");
        world.assert_clean();
    }
}
