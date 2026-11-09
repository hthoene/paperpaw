//! `event.widget_changed` (86): the owner's command is checked against the
//! handle table, marks are coalesced per instance, the maker hears of them
//! between callbacks, once, and only while the instance lives and the maker
//! subscribed.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use crate::native_host::{
    HostResources,
    commands::{CommandStage, WireCommand, commands_decode, commands_encode},
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
    shared::Shared,
    ui_service::UiServices,
    ui_store::{Limits, UiStore},
};
use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID, ERR_NOT_FOUND, ERR_UNSUPPORTED, OK};
use paperpaw_platform::Instant;
use paperpaw_sdk::Id;

const TOKEN: u32 = 41;

/// A maker that logs the instances of the `widget_changed` events it hears.
struct Maker {
    log: Rc<RefCell<Vec<u32>>>,
    subscribed: bool,
}

impl NativeInstance for Maker {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        self.subscribed && tag == wire::EVENT_WIDGET_CHANGED_TAG
    }
    fn service_event(&self, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        let wire::Record::EventWidgetChanged(event) =
            crate::native_host::codec::decode_any_single(bytes)?
        else {
            return Err(ERR_INVALID);
        };
        self.log.borrow_mut().push(event.instance);
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

impl Dispatch for Maker {
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

/// A kernel with a maker plugin (`key`) and the widget table; `OWNER` is the
/// owner of the widgets in the table (no instance is needed for it).
struct World {
    pool: Rc<HostResources>,
    kernel: crate::Kernel,
    key: u64,
    log: Rc<RefCell<Vec<u32>>>,
}

const OWNER: u64 = 900;
const OTHER: u64 = 901;

fn world(subscribed: bool) -> World {
    let pool = Rc::new(HostResources::try_new().unwrap());
    pool.hosts()
        .wire_ui(UiServices::new(
            Shared::try_new(UiStore::new(Limits::ELSEWHERE)).unwrap(),
            false,
        ))
        .unwrap();
    let owner = pool.try_owner().unwrap();
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    let lease = NativeLease::try_new(Maker {
        log: Rc::clone(&log),
        subscribed,
    })
    .unwrap();
    let key = kernel
        .shared
        .native_instances
        .insert(
            owner.try_view().unwrap(),
            Id::new_static("home"),
            Some(lease),
            true,
        )
        .unwrap();
    let _ = &mut kernel;
    World {
        pool,
        kernel,
        key,
        log,
    }
}

impl World {
    fn ui(&self) -> &UiServices {
        self.pool.hosts().ui().unwrap()
    }
    /// An instance of OWNER's widget made by the maker's screen `screen`.
    fn made(&self, screen: u32) -> u32 {
        self.ui()
            .widgets
            .handles
            .borrow_mut()
            .make_widget(OWNER, self.key, screen, TOKEN, false)
            .unwrap()
    }
}

#[test]
fn the_maker_hears_once_of_a_change_however_often_the_owner_said_it() {
    let mut w = world(true);
    let (a, b) = (w.made(5), w.made(5));
    w.ui().widgets_changed(OWNER, &[a, a, b, a]).unwrap();
    assert!(!w.kernel.native_widget_pump());
    assert_eq!(&*w.log.borrow(), &[a, b]);
    // Told once: nothing more until it is said again.
    assert!(!w.kernel.native_widget_pump());
    assert_eq!(w.log.borrow().len(), 2);
    w.ui().widgets_changed(OWNER, &[b]).unwrap();
    w.kernel.native_widget_pump();
    assert_eq!(&*w.log.borrow(), &[a, b, b]);
}

#[test]
fn only_the_owner_of_a_live_instance_may_say_it_changed() {
    let w = world(true);
    let a = w.made(5);
    assert_eq!(w.ui().widget_check_changed(OWNER, a), Ok(()));
    assert_eq!(w.ui().widget_check_changed(OTHER, a), Err(ERR_DENIED));
    assert_eq!(
        w.ui().widget_check_changed(OWNER, a + 100),
        Err(ERR_NOT_FOUND)
    );
    assert_eq!(w.ui().widget_check_changed(OWNER, 0), Err(ERR_NOT_FOUND));
    // A mark by another plugin changes nothing.
    w.ui().widgets_changed(OTHER, &[a]).unwrap();
    assert!(!w.ui().widget_change_waits());
    // Nor does a dropped instance keep being told.
    w.ui().widgets_changed(OWNER, &[a]).unwrap();
    assert_eq!(w.ui().screen_closed(5), Ok(1));
    let mut w = w;
    w.kernel.native_widget_pump();
    assert!(w.log.borrow().is_empty());
}

#[test]
fn a_maker_that_did_not_subscribe_is_not_told_and_the_mark_ends() {
    let mut w = world(false);
    let a = w.made(5);
    w.ui().widgets_changed(OWNER, &[a]).unwrap();
    assert!(!w.kernel.native_widget_pump());
    assert!(w.log.borrow().is_empty());
    assert!(!w.ui().widget_change_waits());
}

#[test]
fn a_pass_tells_a_few_and_asks_for_another_while_marks_wait() {
    let mut w = world(true);
    let ids: Vec<u32> = (0..6).map(|_| w.made(5)).collect();
    w.ui().widgets_changed(OWNER, &ids).unwrap();
    assert!(w.kernel.native_widget_pump(), "marks wait");
    assert_eq!(w.log.borrow().len(), super::PER_PASS);
    assert!(!w.kernel.native_widget_pump());
    assert_eq!(&*w.log.borrow(), &ids, "oldest first");
}

#[test]
fn the_stage_checks_the_command_and_lists_it_for_the_table() {
    let w = world(true);
    let a = w.made(5);
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("probe");
    let submit = |stage: &mut CommandStage<'_>, instance: u32| {
        let mut bytes = [0; 64];
        let len = commands_encode(&[WireCommand::WidgetChanged { instance }], &mut bytes).unwrap();
        stage.stage(&commands_decode(&bytes[..len]).unwrap())
    };
    // Without the table the command is unsupported.
    let mut bare =
        CommandStage::new(&owner, env.settings(), None, 8, 4096).with_ui(&env, None, 0, 8);
    assert_eq!(submit(&mut bare, a), ERR_UNSUPPORTED);
    let mut stage = CommandStage::new(&owner, env.settings(), None, 8, 4096)
        .with_ui(&env, None, 0, 8)
        .with_widgets(Some(w.ui()), OWNER);
    assert_eq!(submit(&mut stage, a), OK);
    assert_eq!(submit(&mut stage, a), OK);
    assert_eq!(submit(&mut stage, a + 7), ERR_NOT_FOUND);
    let foreign = CommandStage::new(&owner, env.settings(), None, 8, 4096)
        .with_ui(&env, None, 0, 8)
        .with_widgets(Some(w.ui()), OTHER);
    let mut foreign = foreign;
    assert_eq!(submit(&mut foreign, a), ERR_DENIED);
    assert!(foreign.take_widget_changes().is_empty());
    // Listed for the table and nothing else: no kernel command.
    assert_eq!(stage.take_widget_changes(), [a, a]);
    assert!(stage.into_commands().is_empty());
}
