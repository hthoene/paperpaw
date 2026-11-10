//! `event.screen_ready` (87) for the screens the host draws: once an
//! instance, only to subscribers, never for one closed before it was drawn.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use crate::native_host::{
    codec,
    commands::CommandStage,
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
};
use alloc::{boxed::Box, rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_platform::Instant;
use paperpaw_sdk::{
    Handled, Id, Manifest, Params, Plugin, RegisterError, Registrar, Screen, ScreenSpec, Version,
};

/// (screen, instance) of each 87 a subscriber was told.
type Told = Rc<RefCell<Vec<(String, u32)>>>;

struct Probe {
    subscribed: bool,
    told: Told,
}

impl NativeInstance for Probe {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        self.subscribed && tag == wire::EVENT_SCREEN_READY_TAG
    }
    fn service_event(&self, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        if let wire::Record::EventScreenReady(event) = codec::decode_any_single(bytes).unwrap() {
            self.told
                .borrow_mut()
                .push((event.screen.into(), event.instance));
        }
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

struct Plain;
impl Screen for Plain {
    fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}
    fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// An overlay that covers all of the content: what lies below is not drawn.
struct Cover;
impl Screen for Cover {
    fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}
    fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> Handled {
        Handled::No
    }
    fn region(&self, env: &paperpaw_sdk::Env<'_>) -> Option<paperpaw_platform::Rect> {
        Some(paperpaw_platform::Rect::new(
            0,
            0,
            env.content().width,
            env.content().height,
        ))
    }
}

struct Screens;
impl Plugin for Screens {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("s"), "Screens", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for name in ["s.one", "s.two"] {
            r.add_screen(ScreenSpec::new(
                Id::try_parse(name).unwrap(),
                "screen",
                Box::new(|_, _| Box::new(Plain)),
            ))?;
        }
        r.add_screen(ScreenSpec::new(
            Id::try_parse("s.cover").unwrap(),
            "screen",
            Box::new(|_, _| Box::new(Cover)),
        ))?;
        Ok(())
    }
}

fn world(subscribed: bool) -> (crate::Kernel, Told) {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Screens)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let told = Told::default();
    kernel
        .shared
        .native_instances
        .test_insert(
            Id::try_parse("help").unwrap(),
            Some(
                NativeLease::try_new(Probe {
                    subscribed,
                    told: Rc::clone(&told),
                })
                .unwrap(),
            ),
            true,
        )
        .unwrap();
    (kernel, told)
}

fn draw(kernel: &mut crate::Kernel) {
    kernel.render(&mut |_, _| {});
    while kernel.native_setting_pump() {}
}

fn open(kernel: &mut crate::Kernel, screen: &str) -> u32 {
    kernel.open_screen(Instant(1), Id::try_parse(screen).unwrap(), Params::new());
    kernel.stack.last().unwrap().serial
}

#[test]
fn a_subscriber_is_told_once_after_the_first_draw_of_each_instance() {
    let (mut kernel, told) = world(true);
    let one = open(&mut kernel, "s.one");
    draw(&mut kernel);
    let first = told.borrow().len();
    assert!(
        told.borrow()
            .iter()
            .any(|(screen, instance)| screen == "s.one" && *instance == one)
    );
    // A later draw raises nothing.
    kernel.output.redraw = true;
    draw(&mut kernel);
    draw(&mut kernel);
    assert_eq!(told.borrow().len(), first);
    // Another instance is another event; a reopened one is a new instance.
    let two = open(&mut kernel, "s.two");
    draw(&mut kernel);
    assert!(
        told.borrow()
            .iter()
            .any(|(screen, instance)| screen == "s.two" && *instance == two)
    );
    assert_ne!(one, two);
}

#[test]
fn nothing_is_delivered_without_a_subscription() {
    let (mut kernel, told) = world(false);
    open(&mut kernel, "s.one");
    draw(&mut kernel);
    assert!(told.borrow().is_empty());
}

#[test]
fn an_instance_closed_before_it_was_drawn_is_never_told() {
    let (mut kernel, told) = world(true);
    draw(&mut kernel);
    let before = told.borrow().len();
    open(&mut kernel, "s.two");
    kernel.apply(paperpaw_sdk::Command::Back);
    kernel.finish();
    draw(&mut kernel);
    assert!(
        told.borrow()
            .iter()
            .skip(before)
            .all(|(screen, _)| screen != "s.two")
    );
}

#[test]
fn a_screen_fully_covered_when_drawn_is_told_only_once_it_is_shown() {
    let (mut kernel, told) = world(true);
    let one = open(&mut kernel, "s.one");
    let cover = open(&mut kernel, "s.cover");
    draw(&mut kernel);
    // The overlay was drawn; what it covers was not, so it is not ready yet.
    assert!(
        told.borrow()
            .iter()
            .any(|(s, i)| s == "s.cover" && *i == cover)
    );
    assert!(!told.borrow().iter().any(|(_, i)| *i == one));
    kernel.apply(paperpaw_sdk::Command::Back);
    kernel.finish();
    kernel.output.redraw = true;
    draw(&mut kernel);
    assert_eq!(
        told.borrow().iter().filter(|(_, i)| *i == one).count(),
        1,
        "told once it was drawn"
    );
}

#[test]
fn a_full_edge_queue_drops_the_event_with_one_log_and_keeps_the_order() {
    let (mut kernel, told) = world(true);
    let screen = Id::try_parse("s.one").unwrap();
    // Nothing tells the plugin between these: 16 edges are held, the rest lost.
    for instance in 0..20 {
        kernel.native_screen_ready(&screen, instance);
    }
    assert_eq!(kernel.take_warnings().len(), 1);
    draw(&mut kernel);
    let told = told.borrow();
    assert_eq!(told.len(), 16);
    assert!(told.iter().map(|(_, i)| *i).eq(0..16));
}
