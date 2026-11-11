//! Answers to plugin requests: who may answer, what the stage refuses, what
//! the requester hears (and nobody else), the first answer winning, a request
//! closing with its last answer, and a request nobody can answer any more.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::plugin_requests_tests::{World, world, world_with};
use crate::native_host::{
    commands::{CommandStage, WireCommand, commands_decode, commands_encode},
    plugin_requests::{OUTCOME_FAILED, OUTCOME_INSTALLED},
    registrations as wire,
};
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID, ERR_STATE, OK};
use paperpaw_sdk::{Id, Instant};

/// What `owner` (`req`, `store` or `other`) stages for `commands`, from the
/// state of the requests now: the status and the answers it admitted.
fn stage_for(
    w: &World,
    owner: &str,
    commands: &[WireCommand<'_>],
) -> (i32, Vec<(u32, String, u32)>) {
    let env = w.kernel.shared.env(Instant(0));
    let id = Id::try_parse(owner).unwrap();
    let key = w.kernel.shared.native_instances.key_of(owner).unwrap();
    let mut stage =
        CommandStage::new(&id, env.settings(), None, 16, 8192).with_ui(&env, None, 0, 100);
    stage.set_answer_view(w.kernel.native_answer_view(key));
    let mut bytes = [0; 2048];
    let len = commands_encode(commands, &mut bytes).unwrap();
    let status = stage.stage(&commands_decode(&bytes[..len]).unwrap());
    let admitted = stage
        .take_answers()
        .into_iter()
        .map(|a| (a.request, String::from(&*a.plugin), a.outcome))
        .collect();
    (status, admitted)
}

fn answer(request: u32, plugin: &str, outcome: u32) -> WireCommand<'_> {
    WireCommand::AnswerPlugin {
        request,
        plugin,
        outcome,
    }
}

/// `store` answers `plugin` of `request` and the kernel applies it.
fn answered(w: &mut World, request: u32, plugin: &str, outcome: u32) {
    let (status, admitted) = stage_for(w, "store", &[answer(request, plugin, outcome)]);
    assert_eq!(status, OK);
    let answers = admitted
        .into_iter()
        .map(
            |(request, plugin, outcome)| crate::native_host::plugin_requests::Answer {
                request,
                plugin: plugin.into_boxed_str(),
                outcome,
            },
        )
        .collect();
    w.kernel.native_apply_answers(answers);
    while w.kernel.native_setting_pump() {}
}

fn heard(w: &World, who: usize) -> Vec<(u32, u32, String)> {
    w.told[who].borrow().clone()
}

#[test]
fn only_an_installer_may_answer_and_the_whole_batch_is_refused_otherwise() {
    let mut w = world(true);
    w.request("req", &["dictionary"]);
    for owner in ["req", "other"] {
        let batch = [WireCommand::Redraw { mode: 0 }, answer(1, "dictionary", 1)];
        assert_eq!(stage_for(&w, owner, &batch), (ERR_DENIED, Vec::new()));
    }
    let (status, admitted) = stage_for(&w, "store", &[answer(1, "dictionary", 1)]);
    assert_eq!((status, admitted.len()), (OK, 1));
}

#[test]
fn the_stage_refuses_an_unknown_outcome_a_bad_id_and_what_is_not_open() {
    let mut w = world(true);
    w.request("req", &["dictionary", "news"]);
    let check = |commands: &[WireCommand<'_>]| stage_for(&w, "store", commands).0;
    assert_eq!(check(&[answer(1, "dictionary", 4)]), ERR_INVALID);
    assert_eq!(check(&[answer(1, "Bad Id!", 1)]), ERR_INVALID);
    assert_eq!(
        check(&[answer(9, "dictionary", 1)]),
        ERR_STATE,
        "no such request"
    );
    assert_eq!(
        check(&[answer(1, "weather", 1)]),
        ERR_STATE,
        "not asked for"
    );
    // The same id twice in one batch: the first wins.
    assert_eq!(
        check(&[answer(1, "news", 1), answer(1, "news", 2)]),
        ERR_STATE
    );
    // A refused batch admits none of its answers.
    assert_eq!(
        stage_for(&w, "store", &[answer(1, "news", 1), answer(2, "x", 1)]),
        (ERR_STATE, Vec::new())
    );
}

#[test]
fn an_answer_reaches_the_requester_alone_and_the_last_one_closes_the_request() {
    let mut w = world(true);
    w.request("req", &["dictionary", "news"]);
    w.told[1].borrow_mut().clear();
    answered(&mut w, 1, "news", 1);
    assert_eq!(
        heard(&w, 0),
        [(wire::EVENT_PLUGIN_ANSWERED_TAG, 1, "news:1".into())]
    );
    assert!(heard(&w, 1).is_empty() && heard(&w, 2).is_empty());
    assert_eq!(w.kernel.native_requests_open(), 1);
    answered(&mut w, 1, "dictionary", 3);
    assert_eq!(heard(&w, 0).len(), 2);
    assert_eq!(w.kernel.native_requests_open(), 0);
}

#[test]
fn the_first_answer_for_an_id_wins() {
    let mut w = world(true);
    w.request("req", &["dictionary", "news"]);
    answered(&mut w, 1, "news", 1);
    // Staged before the first one ran, applied after it: dropped.
    let late = crate::native_host::plugin_requests::Answer {
        request: 1,
        plugin: "news".into(),
        outcome: 3,
    };
    w.kernel.native_apply_answers(alloc::vec![late]);
    while w.kernel.native_setting_pump() {}
    assert_eq!(heard(&w, 0).len(), 1);
    // And a stage that sees it answered refuses a second answer.
    assert_eq!(stage_for(&w, "store", &[answer(1, "news", 3)]).0, ERR_STATE);
}

#[test]
fn installed_is_checked_by_the_host_and_stored_as_failed_when_the_plugin_is_not_there() {
    let mut w = world(true);
    w.request("req", &["dictionary", "extra"]);
    // `extra` is on the device now, `dictionary` is not.
    w.kernel
        .shared
        .native_instances
        .test_insert(Id::new_static("extra"), None, true)
        .unwrap();
    answered(&mut w, 1, "dictionary", OUTCOME_INSTALLED);
    answered(&mut w, 1, "extra", OUTCOME_INSTALLED);
    assert_eq!(
        heard(&w, 0),
        [
            (
                wire::EVENT_PLUGIN_ANSWERED_TAG,
                1,
                alloc::format!("dictionary:{OUTCOME_FAILED}")
            ),
            (wire::EVENT_PLUGIN_ANSWERED_TAG, 1, "extra:0".into()),
        ]
    );
}

#[test]
fn when_the_last_installer_stops_the_open_ids_are_answered_failed() {
    let mut w = world(true);
    w.request("req", &["dictionary", "news"]);
    answered(&mut w, 1, "news", 1);
    w.told[0].borrow_mut().clear();
    // `store` stops: nobody is left to answer.
    w.up[1].set(false);
    while w.kernel.native_setting_pump() {}
    assert_eq!(
        heard(&w, 0),
        [(
            wire::EVENT_PLUGIN_ANSWERED_TAG,
            1,
            alloc::format!("dictionary:{OUTCOME_FAILED}")
        )]
    );
    assert_eq!(w.kernel.native_requests_open(), 0);
}

#[test]
fn an_installer_that_no_longer_declares_the_feature_leaves_the_request_unanswerable() {
    let mut w = world(true);
    w.request("req", &["dictionary"]);
    let key = w.kernel.shared.native_instances.key_of("store").unwrap();
    w.kernel.shared.native_instances.set_installer(key, false);
    while w.kernel.native_setting_pump() {}
    assert_eq!(heard(&w, 0).len(), 1);
    assert_eq!(w.kernel.native_requests_open(), 0);
}

#[test]
fn a_request_whose_requester_is_gone_is_dropped_without_telling_anyone() {
    let mut w = world(true);
    w.request("req", &["dictionary"]);
    let key = w.kernel.shared.native_instances.key_of("req").unwrap();
    w.kernel.shared.native_instances.abandon(key).unwrap();
    while w.kernel.native_setting_pump() {}
    assert_eq!(w.kernel.native_requests_open(), 0);
    assert!(heard(&w, 0).is_empty() && heard(&w, 2).is_empty());
}

#[test]
fn with_no_request_open_the_settle_does_nothing() {
    let mut w = world(true);
    w.kernel.native_requests_settle();
    assert!(!w.kernel.native_setting_pump());
    assert_eq!(w.kernel.native_requests_open(), 0);
    assert!(w.told.iter().all(|told| told.borrow().is_empty()));
}

#[test]
fn a_request_whose_event_could_not_be_held_is_not_left_open() {
    let mut w = world_with(
        true,
        &[
            wire::EVENT_PLUGINS_REQUESTED_TAG,
            wire::EVENT_SCREEN_READY_TAG,
        ],
    );
    // `store`'s queue is full (nothing is told before the pump runs).
    let screen = Id::new_static("some.screen");
    for instance in 0..200 {
        w.kernel.native_screen_ready(&screen, instance);
    }
    w.request("req", &["dictionary"]);
    assert_eq!(w.kernel.native_requests_open(), 0);
    assert!(
        heard(&w, 0)
            .iter()
            .any(|(tag, _, text)| *tag == wire::EVENT_PLUGIN_ANSWERED_TAG
                && *text == alloc::format!("dictionary:{OUTCOME_FAILED}"))
    );
}

#[test]
fn a_request_naming_core_is_refused_whole() {
    let w = world(true);
    let env = w.kernel.shared.env(Instant(0));
    let id = Id::new_static("req");
    let mut stage =
        CommandStage::new(&id, env.settings(), None, 16, 8192).with_ui(&env, None, 0, 100);
    let commands = [
        WireCommand::RequestPlugins { plugins: "news" },
        WireCommand::RequestPlugins {
            plugins: "weather\ncore",
        },
    ];
    let mut bytes = [0; 2048];
    let len = commands_encode(&commands, &mut bytes).unwrap();
    assert_eq!(
        stage.stage(&commands_decode(&bytes[..len]).unwrap()),
        ERR_DENIED
    );
    // Nothing of the batch was staged.
    let len = commands_encode(&commands[..1], &mut bytes).unwrap();
    assert_eq!(stage.stage(&commands_decode(&bytes[..len]).unwrap()), OK);
}
