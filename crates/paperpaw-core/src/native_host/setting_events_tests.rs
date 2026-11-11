//! `event.setting_changed` to the plugins that subscribed: a change of one
//! key, the empty key, only the subscribed, what each may read, the bounds and
//! the order between plugins.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::super::event_pending::Visibility;
use crate::native_host::{
    codec,
    commands::CommandStage,
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_sdk::{
    Id, Manifest, Plugin, RegisterError, Registrar, SettingExtra, SettingKind, SettingSpec,
    SettingValue, Version,
};

/// What a plugin was told: the key and the old value's bytes.
type Told = Rc<RefCell<Vec<(String, Vec<u8>)>>>;
type Hints = Rc<RefCell<Vec<(u32, u32)>>>;

const SETTING: &[u32] = &[wire::EVENT_SETTING_CHANGED_TAG];
const HINTS: &[u32] = &[70, 71, 72, 73, 74];
const RADIO: &[u32] = &[92, 94, 95];

struct Probe {
    name: char,
    /// The event tags it registered for.
    tags: &'static [u32],
    told: Told,
    /// The hints it was told: tag and the state the record carries.
    hints: Hints,
    /// Who was called, in order, for all plugins.
    order: Rc<RefCell<Vec<char>>>,
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
        let record = codec::decode_any_single(bytes).unwrap();
        let (tag, state) = match &record {
            wire::Record::EventPluginsChanged(_) => (wire::EVENT_PLUGINS_CHANGED_TAG, 0),
            wire::Record::EventWifiChanged(event) => (wire::EVENT_WIFI_CHANGED_TAG, event.state),
            wire::Record::EventPowerChanged(event) => (wire::EVENT_POWER_CHANGED_TAG, event.level),
            wire::Record::EventStatusChanged(_) => (wire::EVENT_STATUS_CHANGED_TAG, 0),
            wire::Record::EventRegistryChanged(event) => {
                (wire::EVENT_REGISTRY_CHANGED_TAG, event.what)
            }
            wire::Record::EventPreviewChanged(_) => (wire::EVENT_PREVIEW_CHANGED_TAG, 0),
            wire::Record::EventWifiScanned(event) => (wire::EVENT_WIFI_SCANNED_TAG, event.networks),
            wire::Record::EventNetworkWanted(_) => (wire::EVENT_NETWORK_WANTED_TAG, 0),
            wire::Record::EventNetworkIdle(_) => (wire::EVENT_NETWORK_IDLE_TAG, 0),
            wire::Record::EventChargeChanged(event) => {
                (wire::EVENT_CHARGE_CHANGED_TAG, event.flags)
            }
            _ => (wire::EVENT_SETTING_CHANGED_TAG, 0),
        };
        if tag != wire::EVENT_SETTING_CHANGED_TAG {
            self.hints.borrow_mut().push((tag, state));
            self.order.borrow_mut().push(self.name);
            return Ok(OK);
        }
        let wire::Record::EventSettingChanged(event) = record else {
            return Err(ERR_INVALID);
        };
        self.told
            .borrow_mut()
            .push((event.key.into(), event.old.to_vec()));
        self.order.borrow_mut().push(self.name);
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

/// Declares settings: its own `id.plain` (readable by all), and one secret
/// and one secret with a placement (`id.widget.token`, placed as
/// `id.widget@2.token`).
struct Declares(&'static str, [&'static str; 3]);
impl Plugin for Declares {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static(self.0), "Declares", Version::new(1, 0, 0))
    }
    fn register(&self, registrar: &mut Registrar) -> Result<(), RegisterError> {
        static SECRET: SettingExtra = SettingExtra::new().secret();
        let [plain, token, widget] = self.1;
        registrar.add_setting(SettingSpec::new(
            Id::new_static(plain),
            "plain",
            SettingKind::Int { min: 0, max: 100 },
            SettingValue::Int(1),
            paperpaw_sdk::builtin::groups::OTHER,
        ))?;
        for id in [token, widget] {
            registrar.add_setting(
                SettingSpec::new(
                    Id::new_static(id),
                    "token",
                    SettingKind::Text,
                    SettingValue::Text("".into()),
                    paperpaw_sdk::builtin::groups::OTHER,
                )
                .with_extra(&SECRET),
            )?;
        }
        Ok(())
    }
}

struct World {
    kernel: crate::Kernel,
    a: (u64, Told),
    b: (u64, Told),
    /// What `b` was told of the hints.
    hints: Hints,
    order: Rc<RefCell<Vec<char>>>,
}

/// Plugins `a` (subscribed) and `b` (subscribed when `b_subscribes`).
fn world(b_subscribes: bool) -> World {
    world_with(if b_subscribes { SETTING } else { &[] })
}

/// Plugins `a` (event 64) and `b` (the tags `b_tags`).
fn world_with(b_tags: &'static [u32]) -> World {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .wifi(true)
        .plugin(Declares("a", ["a.plain", "a.token", "a.widget.token"]))
        .plugin(Declares("b", ["b.plain", "b.token", "b.widget.token"]))
        .plugin(Declares("c", ["c.size", "c.token", "c.widget.token"]))
        .build()
        .unwrap();
    let order = Rc::new(RefCell::new(Vec::new()));
    let hints = Hints::default();
    let mut add = |name: char, tags: &'static [u32]| {
        let told = Told::default();
        let probe = || Probe {
            name,
            tags,
            hints: Rc::clone(&hints),
            told: Rc::clone(&told),
            order: Rc::clone(&order),
        };
        let owner = kernel
            .shared
            .native_instances
            .test_insert(
                Id::try_parse(&String::from(name)).unwrap(),
                Some(NativeLease::try_new(probe()).unwrap()),
                true,
            )
            .unwrap();
        (owner, told)
    };
    let a = add('a', SETTING);
    let b = add('b', b_tags);
    World {
        kernel,
        a,
        b,
        hints,
        order,
    }
}

fn int(value: i64) -> Vec<u8> {
    use crate::abi7::settings_wire::{SettingValueRef, settings_encode};
    let mut out = [core::mem::MaybeUninit::uninit(); 64];
    let len = settings_encode(SettingValueRef::Int(value), &mut out).unwrap();
    // SAFETY: the encoder initialized the prefix.
    out[..len]
        .iter()
        .map(|byte| unsafe { byte.assume_init() })
        .collect()
}

fn run(w: &mut World) {
    while w.kernel.native_setting_pump() {}
}

#[test]
fn a_changed_key_reaches_a_subscriber_with_its_old_value() {
    let mut w = world(false);
    w.kernel.native_setting_publish("c.size", Ok(&int(7)));
    run(&mut w);
    assert_eq!(*w.a.1.borrow(), [("c.size".into(), int(7))]);
}

#[test]
fn an_old_value_that_is_none_comes_empty() {
    let mut w = world(false);
    w.kernel.native_setting_publish("c.size", Ok(&[]));
    run(&mut w);
    assert_eq!(*w.a.1.borrow(), [("c.size".into(), Vec::new())]);
}

#[test]
fn the_empty_key_says_to_read_everything_again() {
    let mut w = world(false);
    // A scope switch and an import raise it.
    w.kernel.native_setting_typed("", None);
    run(&mut w);
    assert_eq!(*w.a.1.borrow(), [(String::new(), Vec::new())]);
}

#[test]
fn only_subscribed_plugins_are_told_and_only_they_hold_a_queue() {
    let mut w = world(false);
    assert!(w.kernel.native_setting_before("c.size").is_some());
    w.kernel.native_setting_publish("c.size", Ok(&int(1)));
    run(&mut w);
    assert_eq!(w.a.1.borrow().len(), 1);
    assert!(w.b.1.borrow().is_empty());
    let queues = w.kernel.shared.native_instances.setting_events.waiting();
    assert!(queues.is_empty(), "nothing waits after the pump");
}

#[test]
fn with_no_subscriber_nothing_is_captured_and_nothing_is_kept() {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    assert!(kernel.native_setting_before("c.size").is_none());
    kernel.native_setting_publish("c.size", Ok(&int(1)));
    assert!(!kernel.native_setting_pump());
    assert!(
        kernel
            .shared
            .native_instances
            .setting_events
            .waiting()
            .is_empty()
    );
}

#[test]
fn the_secret_of_another_plugin_is_not_announced() {
    let mut w = world(true);
    w.kernel.native_setting_publish("b.token", Ok(&int(1)));
    run(&mut w);
    assert!(w.a.1.borrow().is_empty(), "not announced to another plugin");
    // Its own plugin is told, without the old value.
    assert_eq!(*w.b.1.borrow(), [("b.token".into(), Vec::new())]);
}

#[test]
fn a_grant_is_not_announced() {
    let mut w = world(false);
    w.kernel
        .native_setting_publish("core.grant.install.c", Ok(&[]));
    run(&mut w);
    assert!(w.a.1.borrow().is_empty());
}

#[test]
fn nothing_is_told_inside_a_callback() {
    let mut w = world(false);
    w.kernel.native_setting_publish("c.size", Ok(&int(1)));
    w.kernel.shared.native_instances.test_enter_callback();
    assert!(w.kernel.native_setting_pump(), "asks for another pass");
    assert!(w.a.1.borrow().is_empty());
}

#[test]
fn more_edges_than_a_queue_holds_end_in_the_empty_key() {
    let mut w = world(false);
    for n in 0..30 {
        w.kernel.native_setting_publish("c.size", Ok(&int(n)));
        w.kernel.native_setting_publish("c.size", Ok(&int(n)));
    }
    run(&mut w);
    let told = w.a.1.borrow();
    assert!(told.len() <= 17, "bounded: {}", told.len());
    assert_eq!(told.last().unwrap().0, "", "ends in the empty key");
}

#[test]
fn plugins_are_told_in_turn_not_one_after_the_other() {
    let mut w = world(true);
    for n in 0..8 {
        w.kernel.native_setting_publish("c.size", Ok(&int(n)));
    }
    // One pass: at most four each, so both are told before either is done.
    w.kernel.native_setting_pump();
    assert_eq!(w.a.1.borrow().len(), 4);
    assert_eq!(w.b.1.borrow().len(), 4);
    assert_eq!(*w.order.borrow(), ['a', 'a', 'a', 'a', 'b', 'b', 'b', 'b']);
    run(&mut w);
    assert_eq!((w.a.1.borrow().len(), w.b.1.borrow().len()), (8, 8));
}

#[test]
fn a_plugin_that_stopped_loses_what_waited() {
    let mut w = world(false);
    w.kernel.native_setting_publish("c.size", Ok(&int(1)));
    assert!(
        !w.kernel
            .shared
            .native_instances
            .setting_events
            .waiting()
            .is_empty()
    );
    let (owner, _) = &w.a;
    w.kernel.shared.native_instances.stop(*owner).unwrap();
    run(&mut w);
    assert!(w.a.1.borrow().is_empty());
    assert!(
        w.kernel
            .shared
            .native_instances
            .setting_events
            .waiting()
            .is_empty()
    );
}

#[test]
fn the_old_value_is_the_kept_one_through_the_real_hooks() {
    let mut w = world(false);
    let old = w.kernel.native_setting_before("c.size");
    w.kernel.native_setting_after("c.size", old.as_ref());
    run(&mut w);
    assert_eq!(*w.a.1.borrow(), [("c.size".into(), Vec::new())]);
}

#[test]
fn a_hint_reaches_only_the_plugins_subscribed_to_it() {
    let mut w = world_with(&[wire::EVENT_PLUGINS_CHANGED_TAG]);
    w.kernel.native_hint(wire::EVENT_PLUGINS_CHANGED_TAG);
    w.kernel.native_hint(wire::EVENT_STATUS_CHANGED_TAG);
    run(&mut w);
    // `b` asked for 70 only; `a` asked for 64 only.
    assert_eq!(*w.hints.borrow(), [(70, 0)]);
    assert_eq!(*w.order.borrow(), ['b']);
    assert!(w.a.1.borrow().is_empty());
}

#[test]
fn hints_raised_before_they_were_told_are_one_event() {
    let mut w = world_with(HINTS);
    for _ in 0..5 {
        w.kernel.native_hint(wire::EVENT_PLUGINS_CHANGED_TAG);
    }
    w.kernel.native_hint(wire::EVENT_CHARGE_CHANGED_TAG);
    w.kernel.native_hint(wire::EVENT_PLUGINS_CHANGED_TAG);
    run(&mut w);
    // Each told once, in the order of the last time it was raised.
    let tags: Vec<u32> = w.hints.borrow().iter().map(|hint| hint.0).collect();
    assert_eq!(tags, [74, 70]);
}

#[test]
fn a_hint_carries_the_state_of_the_time_it_is_told() {
    let mut w = world_with(HINTS);
    w.kernel.native_hint(wire::EVENT_POWER_CHANGED_TAG);
    w.kernel.native_hint(wire::EVENT_WIFI_CHANGED_TAG);
    w.kernel.native_hint(wire::EVENT_CHARGE_CHANGED_TAG);
    run(&mut w);
    // No Wi-Fi in the fixture device: off (0); level 0 normal; no charge.
    assert_eq!(*w.hints.borrow(), [(72, 0), (71, 0), (74, 0)]);
}

#[test]
fn hints_and_setting_edges_share_a_queue_and_its_order() {
    let mut w = world_with(HINTS);
    w.kernel.native_hint(wire::EVENT_STATUS_CHANGED_TAG);
    w.kernel.native_setting_publish("c.size", Ok(&int(1)));
    run(&mut w);
    // `a` was told the edge, `b` the hint; `b` never gets an edge.
    assert_eq!(w.a.1.borrow().len(), 1);
    assert!(w.b.1.borrow().is_empty());
    assert_eq!(*w.hints.borrow(), [(73, 0)]);
}

#[test]
fn a_hint_for_a_plugin_that_stopped_is_dropped() {
    let mut w = world_with(HINTS);
    w.kernel.native_hint(wire::EVENT_PLUGINS_CHANGED_TAG);
    let owner = w.b.0;
    w.kernel.shared.native_instances.stop(owner).unwrap();
    run(&mut w);
    assert!(w.hints.borrow().is_empty());
    assert!(
        w.kernel
            .shared
            .native_instances
            .setting_events
            .waiting()
            .is_empty()
    );
}

#[test]
fn a_placed_secret_of_another_plugin_is_not_announced() {
    let mut w = world(true);
    // `b.widget@2.token` is the placement of `b.widget.token`, a secret.
    w.kernel
        .native_setting_publish("b.widget@2.token", Ok(&int(1)));
    run(&mut w);
    assert!(w.a.1.borrow().is_empty(), "not announced to another plugin");
    // Its own plugin is told, without the old value.
    assert_eq!(*w.b.1.borrow(), [("b.widget@2.token".into(), Vec::new())]);
}

#[test]
fn a_placed_secret_of_the_plugin_itself_comes_without_its_old_value() {
    let mut w = world(true);
    w.kernel
        .native_setting_publish("a.widget@3.token", Ok(&int(1)));
    run(&mut w);
    assert_eq!(*w.a.1.borrow(), [("a.widget@3.token".into(), Vec::new())]);
    assert!(w.b.1.borrow().is_empty());
}

#[test]
fn a_placed_setting_that_is_no_secret_keeps_its_old_value() {
    let mut w = world(false);
    // `c.size` has no placement; a placed plain one is read as its definition.
    w.kernel.native_setting_publish("c.size", Ok(&int(5)));
    run(&mut w);
    assert_eq!(*w.a.1.borrow(), [("c.size".into(), int(5))]);
}

#[test]
fn a_key_nothing_defines_is_hidden_from_other_plugins() {
    let mut w = world(true);
    // The plugin that defined `c.gone` is being uninstalled: fail closed.
    w.kernel.native_setting_publish("c.gone", Ok(&int(1)));
    w.kernel
        .native_setting_publish("c.widget@2.gone", Ok(&int(1)));
    run(&mut w);
    assert!(w.a.1.borrow().is_empty() && w.b.1.borrow().is_empty());
}

#[test]
fn an_own_key_nothing_defines_comes_without_its_old_value() {
    let mut w = world(false);
    w.kernel.native_setting_publish("a.gone", Ok(&int(1)));
    run(&mut w);
    assert_eq!(*w.a.1.borrow(), [("a.gone".into(), Vec::new())]);
}

#[test]
fn a_key_that_turned_secret_before_delivery_is_not_told() {
    let mut w = world(false);
    // An edge of `c.late`, queued with its old value while nothing was secret
    // (the queue is filled by hand: a definition cannot change in place).
    let owner = w.a.0;
    {
        let events = &w.kernel.shared.native_instances.setting_events;
        let mut queues = events.queues.borrow_mut();
        events
            .queue_of(&mut queues, owner)
            .unwrap()
            .setting(1, "c.late", Ok(&int(9)), Visibility::Readable)
            .unwrap();
    }
    // Before it is told, the key is defined as a secret.
    static SECRET: SettingExtra = SettingExtra::new().secret();
    w.kernel
        .shared
        .settings
        .define(
            SettingSpec::new(
                Id::new_static("c.late"),
                "late",
                SettingKind::Text,
                SettingValue::Text("".into()),
                paperpaw_sdk::builtin::groups::OTHER,
            )
            .with_extra(&SECRET),
        )
        .unwrap();
    run(&mut w);
    assert!(w.a.1.borrow().is_empty(), "checked again at delivery");
}

const REGISTRY: &[u32] = &[
    wire::EVENT_REGISTRY_CHANGED_TAG,
    wire::EVENT_PREVIEW_CHANGED_TAG,
];

#[test]
fn registry_bits_raised_before_they_were_told_are_one_event_with_every_bit() {
    let mut w = world_with(REGISTRY);
    w.kernel.native_registry_changed(1);
    w.kernel.native_registry_changed(1 << 4);
    w.kernel.native_registry_changed(1);
    run(&mut w);
    assert_eq!(*w.hints.borrow(), [(75, 0b1_0001)]);
    assert_eq!(*w.order.borrow(), ['b']);
    assert!(w.a.1.borrow().is_empty(), "a did not subscribe");
    // Told: the next one starts from none.
    w.kernel.native_registry_changed(1 << 2);
    run(&mut w);
    assert_eq!(w.hints.borrow()[1], (75, 0b100));
}

#[test]
fn no_registry_bit_is_no_event_and_a_plugin_that_stopped_is_not_called() {
    let mut w = world_with(REGISTRY);
    w.kernel.native_registry_changed(0);
    run(&mut w);
    assert!(w.hints.borrow().is_empty());
    w.kernel.native_registry_changed(1);
    let owner = w.b.0;
    w.kernel.shared.native_instances.stop(owner).unwrap();
    run(&mut w);
    assert!(w.hints.borrow().is_empty());
    assert!(
        w.kernel
            .shared
            .native_instances
            .setting_events
            .waiting()
            .is_empty()
    );
}

#[test]
fn a_changed_preview_reaches_its_subscribers_once_and_in_order_with_the_registry() {
    let mut w = world_with(REGISTRY);
    w.kernel.native_hint(wire::EVENT_PREVIEW_CHANGED_TAG);
    w.kernel.native_registry_changed(2);
    w.kernel.native_hint(wire::EVENT_PREVIEW_CHANGED_TAG);
    run(&mut w);
    // Each once, in the order of the last time it was raised.
    assert_eq!(*w.hints.borrow(), [(75, 2), (76, 0)]);
}

#[test]
fn a_registry_event_is_not_queued_for_a_plugin_that_subscribed_to_nothing_of_it() {
    let w = world(false);
    w.kernel.native_registry_changed(1);
    w.kernel.native_hint(wire::EVENT_PREVIEW_CHANGED_TAG);
    assert!(
        w.kernel
            .shared
            .native_instances
            .setting_events
            .waiting()
            .is_empty()
    );
}

#[test]
fn switching_a_plugin_off_tells_every_registry_bit_and_a_preview_tells_the_preview() {
    let mut w = world_with(REGISTRY);
    let id = Id::try_parse("c").unwrap();
    w.kernel.set_plugin_enabled(&id, false);
    run(&mut w);
    assert_eq!(*w.hints.borrow(), [(75, 0xFF)]);
    w.hints.borrow_mut().clear();
    let key = Id::try_parse("c.size").unwrap();
    w.kernel
        .preview_settings(alloc::vec![(key, Some(paperpaw_sdk::SettingValue::Int(3)))]);
    run(&mut w);
    assert_eq!(*w.hints.borrow(), [(76, 0)]);
    // Ending the shown preview tells; ending it again tells nothing.
    w.hints.borrow_mut().clear();
    w.kernel.preview_settings(alloc::vec::Vec::new());
    run(&mut w);
    assert_eq!(*w.hints.borrow(), [(76, 0)]);
    w.hints.borrow_mut().clear();
    w.kernel.preview_settings(alloc::vec::Vec::new());
    run(&mut w);
    assert!(w.hints.borrow().is_empty());
}

#[test]
fn the_dropped_bit_goes_to_the_submitter_alone_and_only_if_it_subscribed() {
    use crate::native_host::REGISTRY_DROPPED;
    let mut w = world_with(REGISTRY);
    // `a` did not subscribe to 75, `b` did: a drop of a's batch tells nobody,
    // a drop of b's tells b alone.
    w.kernel
        .native_registry_dropped(&Id::try_parse("a").unwrap());
    run(&mut w);
    assert!(w.hints.borrow().is_empty());
    w.kernel
        .native_registry_dropped(&Id::try_parse("b").unwrap());
    w.kernel.native_registry_changed(1);
    run(&mut w);
    // One event, the bits of both together; never a bit for another plugin.
    assert_eq!(*w.hints.borrow(), [(75, REGISTRY_DROPPED | 1)]);
    assert_eq!(*w.order.borrow(), ['b']);
}

#[test]
fn a_registry_bit_for_a_plugin_that_is_not_running_is_not_kept() {
    let mut w = world_with(REGISTRY);
    let id = Id::try_parse("b").unwrap();
    let owner = w.b.0;
    w.kernel.shared.native_instances.stop(owner).unwrap();
    w.kernel.native_registry_dropped(&id);
    assert!(
        w.kernel
            .shared
            .native_instances
            .setting_events
            .waiting()
            .is_empty()
    );
}

#[test]
fn a_registry_bit_raised_while_the_queues_are_borrowed_is_dropped_without_a_panic() {
    let mut w = world_with(REGISTRY);
    let events = &w.kernel.shared.native_instances.setting_events;
    {
        let _held = events.queues.borrow_mut();
        w.kernel.native_registry_changed(1);
        w.kernel
            .native_registry_dropped(&Id::try_parse("b").unwrap());
    }
    run(&mut w);
    assert!(w.hints.borrow().is_empty());
}

/// The queues now held, and the room the list keeps.
fn held(w: &World) -> (usize, usize) {
    let events = &w.kernel.shared.native_instances.setting_events;
    let queues = events.queues.borrow();
    (queues.len(), queues.capacity())
}

#[test]
fn an_idle_device_holds_no_event_queue_once_the_events_are_told() {
    let mut w = world(false);
    w.kernel.native_setting_publish("c.size", Ok(&int(7)));
    assert_eq!(held(&w).0, 1, "waits for the pump");
    run(&mut w);
    assert_eq!(*w.a.1.borrow(), [("c.size".into(), int(7))]);
    assert_eq!(held(&w), (0, 0), "told: the queue and the list are freed");
    // The next event makes the queue again, and it is told as before.
    w.kernel.native_setting_publish("c.size", Ok(&int(8)));
    run(&mut w);
    assert_eq!(w.a.1.borrow().len(), 2);
    assert_eq!(held(&w), (0, 0));
}

#[test]
fn a_change_nobody_may_be_told_leaves_no_empty_queue_behind() {
    let w = world(false);
    // The secret of another plugin is not announced: nothing is queued.
    w.kernel.native_setting_publish("b.secret", Ok(&int(1)));
    assert_eq!(held(&w), (0, 0));
}

#[test]
fn a_lost_edge_is_made_up_by_the_reread_on_the_queue_that_is_still_there() {
    let mut w = world(false);
    w.kernel.native_setting_publish("c.size", Ok(&int(7)));
    let events = &w.kernel.shared.native_instances.setting_events;
    let owner = events.waiting()[0];
    // The edge is taken and its encode fails: the queue is not freed yet, so
    // the reread needs no allocation in that state.
    assert!(events.take(owner).is_some());
    assert_eq!(
        held(&w).0,
        1,
        "the empty queue stays until the delivery ends"
    );
    events.reread(owner, 5);
    assert_eq!(events.waiting(), [owner]);
    run(&mut w);
    assert_eq!(*w.a.1.borrow(), [(String::new(), Vec::new())]);
    assert_eq!(held(&w), (0, 0), "told: given back");
}

/// Makes plugin `manager` the network manager (`core.network-manager`, which
/// the kernel defines with the default `wifi`).
fn manager(w: &mut World, manager: &'static str) {
    w.kernel
        .shared
        .settings
        .set(
            crate::native_host::commands::dev::NETWORK_MANAGER,
            paperpaw_sdk::SettingValue::Text(manager.into()),
        )
        .unwrap();
}

#[test]
fn a_scan_tells_its_subscribers_how_many_networks_it_found() {
    use crate::WifiEvent;
    use paperpaw_platform::Instant;
    use paperpaw_sdk::wifi::WifiNetwork;
    let mut w = world_with(RADIO);
    let net = |ssid: &str, signal| WifiNetwork::new(String::from(ssid), signal, true);
    w.kernel.wifi_event(
        Instant(1),
        WifiEvent::Scanned(alloc::vec![
            net("Home", -50),
            net("", -40),
            net("Cafe", -70)
        ]),
    );
    run(&mut w);
    // Hidden names are left out: the count is what Wi-Fi keeps.
    assert_eq!(*w.hints.borrow(), [(92, 2)]);
    assert!(w.a.1.borrow().is_empty());
}

#[test]
fn wanted_and_idle_go_to_the_network_manager_alone_and_scanned_to_every_subscriber() {
    let mut w = world_with(RADIO);
    manager(&mut w, "a");
    for tag in [92, 94, 95] {
        w.kernel.native_hint(tag);
    }
    run(&mut w);
    // `b` is no manager: only the scan; `a` subscribed to none of them.
    assert_eq!(*w.hints.borrow(), [(92, 0)]);
    let mut w = world_with(RADIO);
    manager(&mut w, "b");
    for tag in [94, 95, 94] {
        w.kernel.native_hint(tag);
    }
    run(&mut w);
    assert_eq!(*w.hints.borrow(), [(95, 0), (94, 0)]);
}

#[test]
fn without_a_network_manager_wanted_and_idle_go_nowhere() {
    let mut w = world_with(RADIO);
    w.kernel.native_hint(wire::EVENT_NETWORK_WANTED_TAG);
    w.kernel.native_hint(wire::EVENT_NETWORK_IDLE_TAG);
    run(&mut w);
    assert!(w.hints.borrow().is_empty());
    assert!(
        w.kernel
            .shared
            .native_instances
            .setting_events
            .waiting()
            .is_empty()
    );
}
