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

#[test]
fn the_native_account_survives_restart_and_sign_out_removes_it_for_the_next_restart() {
    let card = {
        let mut world = World::new();
        account(&mut world, false);
        world.complete(0, 200, b"{}");
        assert_eq!(world.kernel().settings().text("kosync.user"), "test-reader");
        world.assert_clean();
        world.card.clone()
    };
    {
        let mut world = World::restart(card.clone());
        assert_eq!(world.kernel().settings().text("kosync.user"), "test-reader");
        world
            .kernel()
            .open_screen(Instant(0), Id::new_static("kosync.account"), Params::new());
        world.pump();
        for _ in 0..5 {
            world.action("core.down");
        }
        world.action("core.confirm");
        assert_eq!(world.kernel().settings().text("kosync.user"), "");
        assert!(
            world.network.requests.borrow().is_empty(),
            "sign-out must be local"
        );
        world.assert_clean();
    }
    let mut world = World::restart(card);
    assert_eq!(world.kernel().settings().text("kosync.user"), "");
    world
        .kernel()
        .open_screen(Instant(0), Id::new_static("kosync.account"), Params::new());
    world.pump();
    assert!(world.network.requests.borrow().is_empty());
    let secrets = paperpaw_sdk::read_file(&world.card, ".paperpaw/secrets.txt", 4096).unwrap();
    assert!(
        !secrets
            .windows(b"kosync.key".len())
            .any(|w| w == b"kosync.key"),
        "sign-out must remove the sealed key"
    );
    world.assert_clean();
}

/// The labels the screen shows now.
fn labels(world: &mut World) -> Vec<String> {
    world
        .kernel()
        .describe()
        .into_iter()
        .map(|part| part.label)
        .collect()
}

/// Offline mode holds *Sign in* back on the real host (#459): the host's
/// question offers to go online; *Go online* (the focused button, pressed)
/// sends `command.go_online` and *Sign in* again, which now reaches the server.
#[test]
fn offline_mode_offers_to_go_online_for_sign_in_and_go_online_signs_in_again() {
    let mut world = World::offline();
    account(&mut world, false);
    let asked = labels(&mut world);
    for label in ["Offline mode", "Go online", "Not now"] {
        assert!(asked.iter().any(|l| l == label), "{label}: {asked:?}");
    }
    assert!(world.network.requests.borrow().is_empty(), "held back");
    world.press("confirm");
    assert_eq!(world.kernel().take_warnings(), [], "went online");
    {
        let calls = world.network.requests.borrow();
        assert_eq!(calls.len(), 1, "signed in again");
        assert!(calls[0].url.ends_with("/users/auth"));
    }
    world.complete(0, 200, b"{}");
    assert_eq!(world.kernel().settings().text("kosync.user"), "test-reader");
    world.assert_clean();
}

/// Offline mode holds *Sign in* back on the real host (#459): the host's
/// question offers to go online; *Not now* sends nothing and the account
/// screen says it needs the network.
#[test]
fn offline_mode_offers_to_go_online_for_sign_in_and_not_now_says_no_network() {
    let mut world = World::offline();
    account(&mut world, false);
    let asked = labels(&mut world);
    for label in ["Offline mode", "Go online", "Not now"] {
        assert!(asked.iter().any(|l| l == label), "{label}: {asked:?}");
    }
    assert!(world.network.requests.borrow().is_empty(), "held back");
    world.action("core.down");
    world.action("core.confirm");
    let said = labels(&mut world);
    assert!(
        said.iter()
            .any(|l| l == "This device has no network connection."),
        "{said:?}"
    );
    assert!(!said.iter().any(|l| l == "Offline mode"), "{said:?}");
    assert!(world.network.requests.borrow().is_empty(), "nothing sent");
    assert_eq!(world.kernel().take_warnings(), []);
    world.assert_clean();
}

/// Where going online does not help (here the host ignores `go_online`
/// without a user input), *Sign in* held back again asks no more and says it
/// needs the network; chosen anew, it may offer again.
#[test]
fn held_back_again_after_go_online_it_asks_once_and_says_no_network() {
    let mut world = World::offline();
    account(&mut world, false);
    world.action("core.confirm");
    let warnings = world.kernel().take_warnings();
    assert!(
        warnings.iter().any(|w| w.message.contains("go_online")),
        "{warnings:?}"
    );
    let said = labels(&mut world);
    assert!(
        said.iter()
            .any(|l| l == "This device has no network connection."),
        "{said:?}"
    );
    assert!(!said.iter().any(|l| l == "Offline mode"), "{said:?}");
    assert!(world.network.requests.borrow().is_empty());
    // *Sign in* chosen anew: offered again.
    world.action("core.confirm");
    let asked = labels(&mut world);
    assert!(asked.iter().any(|l| l == "Offline mode"), "{asked:?}");
    world.assert_clean();
}
