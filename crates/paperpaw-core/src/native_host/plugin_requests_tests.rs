//! Plugin requests of native plugins: the ids not on the device go to the
//! installers (`uses.install` and subscribed) as `plugins_requested`, none able
//! to answer means `failed` at once to the requester alone, the room of four
//! open requests, and what the stage refuses.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use crate::native_host::{
    codec,
    commands::{CommandStage, WireCommand, commands_decode, commands_encode},
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    plugin_requests::{MAX_OPEN, OUTCOME_FAILED},
    registrations as wire,
};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_FULL, ERR_INVALID, OK};
use paperpaw_sdk::{Command, Id, Instant};

/// What one plugin was told: `(tag, request, text)`, the text the `by` and
/// ids of 77, the plugin and outcome of 78.
type Told = Rc<RefCell<Vec<(u32, u32, String)>>>;

struct Probe {
    tags: &'static [u32],
    told: Told,
}

impl NativeInstance for Probe {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        self.tags.contains(&tag)
    }
    fn service_event(&self, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        let entry = match codec::decode_any_single(bytes).unwrap() {
            wire::Record::EventPluginsRequested(e) => (
                wire::EVENT_PLUGINS_REQUESTED_TAG,
                e.request,
                alloc::format!("{}:{}", e.by, e.plugins),
            ),
            wire::Record::EventPluginAnswered(e) => (
                wire::EVENT_PLUGIN_ANSWERED_TAG,
                e.request,
                alloc::format!("{}:{}", e.plugin, e.outcome),
            ),
            _ => return Err(ERR_INVALID),
        };
        self.told.borrow_mut().push(entry);
        Ok(OK)
    }
    fn service_event_staged(
        &self,
        bytes: &[u8],
        env: &paperpaw_sdk::Env<'_>,
        _: &mut CommandStage<'_>,
    ) -> Result<i32, i32> {
        self.service_event(bytes, env)
    }
}

impl Dispatch for Probe {
    fn event(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &paperpaw_sdk::Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn dropped(&self, _: u32, _: i32) {}
}

const REQ_TAGS: &[u32] = &[wire::EVENT_PLUGIN_ANSWERED_TAG];
const STORE_TAGS: &[u32] = &[wire::EVENT_PLUGINS_REQUESTED_TAG];

struct World {
    kernel: crate::Kernel,
    told: [Told; 3],
}

/// `req` (hears answers), `store` (hears requests, an installer when
/// `store_installs`) and `other` (hears requests, no installer).
fn world(store_installs: bool) -> World {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let told: [Told; 3] = Default::default();
    for (at, (name, tags, installs)) in [
        ("req", REQ_TAGS, false),
        ("store", STORE_TAGS, store_installs),
        ("other", STORE_TAGS, false),
    ]
    .into_iter()
    .enumerate()
    {
        let probe = Probe {
            tags,
            told: told[at].clone(),
        };
        let key = kernel
            .shared
            .native_instances
            .test_insert(
                Id::try_parse(name).unwrap(),
                Some(NativeLease::try_new(probe).unwrap()),
                true,
            )
            .unwrap();
        kernel.shared.native_instances.set_installer(key, installs);
    }
    World { kernel, told }
}

impl World {
    fn request(&mut self, by: &str, plugins: &[&str]) {
        let ids = plugins.iter().map(|id| Id::try_parse(id).unwrap());
        self.kernel
            .output
            .commands
            .push(Command::request_plugins(Id::try_parse(by).unwrap(), ids));
        self.kernel.run_commands();
        while self.kernel.native_setting_pump() {}
    }
}

#[test]
fn a_request_goes_to_the_installers_that_subscribed_and_opens_one_entry() {
    let mut w = world(true);
    w.request("req", &["dictionary", "news"]);
    assert_eq!(
        *w.told[1].borrow(),
        [(
            wire::EVENT_PLUGINS_REQUESTED_TAG,
            1,
            "req:dictionary\nnews".into()
        )]
    );
    // `other` subscribed but cannot answer: it hears nothing; the requester
    // hears nothing yet.
    assert!(w.told[2].borrow().is_empty());
    assert!(w.told[0].borrow().is_empty());
    assert_eq!(w.kernel.native_requests_open(), 1);
}

#[test]
fn with_no_installer_every_id_is_answered_failed_at_once_to_the_requester_alone() {
    let mut w = world(false);
    w.request("req", &["dictionary", "news"]);
    let told: Vec<(u32, String)> = w.told[0]
        .borrow()
        .iter()
        .map(|(tag, _, text)| (*tag, text.clone()))
        .collect();
    assert_eq!(
        told,
        [
            (
                wire::EVENT_PLUGIN_ANSWERED_TAG,
                alloc::format!("dictionary:{OUTCOME_FAILED}")
            ),
            (
                wire::EVENT_PLUGIN_ANSWERED_TAG,
                alloc::format!("news:{OUTCOME_FAILED}")
            ),
        ]
    );
    assert!(w.told[1].borrow().is_empty() && w.told[2].borrow().is_empty());
    assert_eq!(w.kernel.native_requests_open(), 0, "nothing is kept");
}

#[test]
fn ids_already_on_the_device_are_dropped_and_none_left_sends_nothing() {
    let mut w = world(true);
    // `store` itself is on the device.
    w.request("req", &["store", "req"]);
    assert!(w.told.iter().all(|told| told.borrow().is_empty()));
    assert_eq!(w.kernel.native_requests_open(), 0);
    w.request("req", &["store", "news", "news"]);
    assert_eq!(w.told[1].borrow()[0].2, "req:news");
}

#[test]
fn an_interface_6_requester_keeps_its_own_path() {
    let mut w = world(true);
    // Not a native plugin: the bus path (no installer there, answered by the
    // kernel on the bus), nothing in the native table or queues.
    w.request("not.native", &["news"]);
    assert!(w.told.iter().all(|told| told.borrow().is_empty()));
    assert_eq!(w.kernel.native_requests_open(), 0);
}

#[test]
fn the_fifth_open_request_finds_no_room_and_is_answered_failed() {
    let mut w = world(true);
    for at in 0..=MAX_OPEN {
        w.request("req", &[&alloc::format!("plugin{at}")]);
    }
    assert_eq!(w.kernel.native_requests_open(), MAX_OPEN);
    assert_eq!(w.told[1].borrow().len(), MAX_OPEN);
    assert_eq!(
        *w.told[0].borrow(),
        [(
            wire::EVENT_PLUGIN_ANSWERED_TAG,
            0,
            alloc::format!("plugin{MAX_OPEN}:{OUTCOME_FAILED}")
        )]
    );
}

fn stage_statuses(open: usize, batches: &[&[&str]]) -> Vec<i32> {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("req");
    let mut stage =
        CommandStage::new(&owner, env.settings(), None, 16, 8192).with_ui(&env, None, 0, 100);
    stage.set_requests_open(open);
    batches
        .iter()
        .map(|batch| {
            let commands: Vec<WireCommand<'_>> = batch
                .iter()
                .map(|plugins| WireCommand::RequestPlugins { plugins })
                .collect();
            let mut bytes = [0; 2048];
            let len = commands_encode(&commands, &mut bytes).unwrap();
            stage.stage(&commands_decode(&bytes[..len]).unwrap())
        })
        .collect()
}

#[test]
fn the_stage_refuses_a_bad_id_and_a_request_beyond_four_open() {
    assert_eq!(stage_statuses(0, &[&["dictionary\nnews"]]), [OK]);
    assert_eq!(stage_statuses(0, &[&["Bad Id!"]]), [ERR_INVALID]);
    // Three open: one more fits, the next does not (also staged ones count).
    assert_eq!(stage_statuses(3, &[&["a1"], &["a2"]]), [OK, ERR_FULL]);
    assert_eq!(
        stage_statuses(0, &[&["a1", "a2", "a3", "a4", "a5"]]),
        [ERR_FULL]
    );
}
