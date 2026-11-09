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

struct Probe {
    name: char,
    subscribed: bool,
    told: Told,
    /// Who was called, in order, for all plugins.
    order: Rc<RefCell<Vec<char>>>,
}

impl NativeInstance for Probe {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        self.subscribed && tag == wire::EVENT_SETTING_CHANGED_TAG
    }
    fn service_event(&self, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        let wire::Record::EventSettingChanged(event) = codec::decode_single(bytes).unwrap() else {
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
    order: Rc<RefCell<Vec<char>>>,
}

/// Plugins `a` (subscribed) and `b` (subscribed when `b_subscribes`).
fn world(b_subscribes: bool) -> World {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Declares("a", ["a.plain", "a.token", "a.widget.token"]))
        .plugin(Declares("b", ["b.plain", "b.token", "b.widget.token"]))
        .plugin(Declares("c", ["c.size", "c.token", "c.widget.token"]))
        .build()
        .unwrap();
    let order = Rc::new(RefCell::new(Vec::new()));
    let mut add = |name: char, subscribed: bool| {
        let told = Told::default();
        let probe = || Probe {
            name,
            subscribed,
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
    let a = add('a', true);
    let b = add('b', b_subscribes);
    World {
        kernel,
        a,
        b,
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
