//! Helpers shared by the kernel's integration tests: a small device, a
//! plugin (`drv`) whose screens log what reaches them, and a way to carry
//! out commands as a plugin would, through the kernel's public API only.

#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_core::sdk::builtin::{actions, slots};
use paperpaw_core::sdk::{
    ActionSpec, Canvas, Command, Cx, Env, Gesture, Handled, Id, Manifest, Params, Plugin, Priority,
    RegisterError, Registrar, Screen, ScreenSpec, Version,
};
use paperpaw_platform::{
    ButtonState, Color, DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, PixelFormat,
    Point, PointerEvent, PointerPhase, PointerTool, Rect, Rotation,
};

/// A small device with buttons, touch and a pen.
pub const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(120, 160, 100, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, true, true, &[]),
    paperpaw_platform::MemoryClass::Large,
);

/// The whole screen of [`DEVICE`].
pub const SCREEN: Rect = Rect::new(0, 0, 120, 160);

/// The buttons of the default key map.
pub const BUTTONS: [&str; 11] = [
    "page-forward",
    "page-back",
    "up",
    "down",
    "left",
    "right",
    "confirm",
    "back",
    "menu",
    "home",
    "power",
];

pub type Log = Rc<RefCell<Vec<String>>>;

/// The action that carries out the commands queued with [`Harness::run`].
pub const GO: Id = Id::new_static("drv.go");
/// An action that runs itself again, for ever.
pub const LOOP: Id = Id::new_static("drv.loop");
/// The event the driver's replies come back on (HTTP, sockets, servers,
/// speech): logged as `event drv.reply <params>`.
pub const REPLY: Id = Id::new_static("drv.reply");
/// A parameter of the events the driver hears: it schedules the event
/// again this many milliseconds later.
pub const REPEAT: &str = "repeat";
/// Events the driver hears and logs, e.g. for scheduled ones.
pub const LATER: Id = Id::new_static("drv.later");
pub const LATER2: Id = Id::new_static("drv.later2");
/// The full screen on the home slot.
pub const BASE: Id = Id::new_static("drv.base");
/// Another full screen.
pub const FULL: Id = Id::new_static("drv.full");
/// An overlay: the lower half, or the region in its parameter
/// [`REGION`] (`x,y,width,height`); it dims what lies below with
/// [`DIMS`] set. Bound to the menu slot.
pub const OVERLAY: Id = Id::new_static("drv.overlay");
/// Parameters of the driver's screens.
pub const REGION: &str = "region";
pub const DIMS: &str = "dims";
/// A screen opened with it asks to be woken at this time (milliseconds)
/// when it shows.
pub const WAKE: &str = "wake";

/// What a test drives the kernel with: what the screens and handlers
/// logged, and commands to carry out as the plugin.
#[derive(Clone, Default)]
pub struct Harness {
    pub log: Log,
    queue: Rc<RefCell<Vec<Command>>>,
}

impl Harness {
    pub fn new() -> Self {
        Self::default()
    }

    /// The plugin to build the kernel with.
    pub fn plugin(&self) -> Driver {
        Driver {
            log: self.log.clone(),
            queue: self.queue.clone(),
        }
    }

    /// A kernel with the driver, started at 0, its log empty.
    pub fn kernel(&self) -> Kernel {
        let mut kernel = Kernel::builder(DEVICE)
            .plugin(self.plugin())
            .build()
            .expect("kernel");
        kernel.start(Instant(0));
        render(&mut kernel);
        self.take();
        kernel
    }

    /// Carries out `commands` as the driver's action handler would.
    pub fn run(
        &self,
        kernel: &mut Kernel,
        now: u64,
        commands: impl IntoIterator<Item = Command>,
    ) -> paperpaw_core::Effects {
        self.queue.borrow_mut().extend(commands);
        kernel.run_action(Instant(now), GO)
    }

    /// The lines logged so far, and forgets them.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.borrow_mut())
    }

    /// Forgets the lines logged that start with `prefix`; how many.
    pub fn take_matching(&self, prefix: &str) -> usize {
        let mut log = self.log.borrow_mut();
        let before = log.len();
        log.retain(|l| !l.starts_with(prefix));
        before - log.len()
    }

    /// The lines logged so far that start with `prefix`.
    pub fn lines(&self, prefix: &str) -> Vec<String> {
        self.log
            .borrow()
            .iter()
            .filter(|l| l.starts_with(prefix))
            .cloned()
            .collect()
    }
}

/// `params` as `key=value` pairs, in their order.
pub fn describe(params: &Params) -> String {
    let mut told: Vec<String> = params.iter().map(|(k, v)| format!("{k}={v}")).collect();
    // An HTTP answer's body goes along unwritten, as it came.
    if let Some(reply) = params.carried::<paperpaw_core::sdk::HttpReply>() {
        told.push(format!("body={} bytes={}", reply.text(), reply.body.len()));
    }
    told.join(" ")
}

/// The test plugin `drv`.
pub struct Driver {
    log: Log,
    queue: Rc<RefCell<Vec<Command>>>,
}

pub fn manifest(id: &'static str) -> Manifest {
    Manifest::new(Id::new_static(id), "Test", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NETWORK)
}

impl Plugin for Driver {
    fn manifest(&self) -> Manifest {
        manifest("drv")
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for (id, name) in [(BASE, "base"), (FULL, "full"), (OVERLAY, "overlay")] {
            let log = self.log.clone();
            r.add_screen(ScreenSpec::new(
                id,
                "drv.screen",
                Box::new(move |_, params| Box::new(LogScreen::new(name, log.clone(), params))),
            ))?;
        }
        r.bind_slot(slots::HOME, BASE, Priority::DEFAULT)?;
        r.bind_slot(slots::MENU, OVERLAY, Priority::DEFAULT)?;
        r.bind_slot(slots::NOTICE, OVERLAY, Priority::DEFAULT)?;
        let queue = self.queue.clone();
        r.add_action(ActionSpec::new(GO, "drv.go").handler(move |_, _, out| {
            out.extend(queue.borrow_mut().drain(..));
        }))?;
        r.add_action(ActionSpec::new(LOOP, "drv.loop").handler(|_, _, out| {
            out.push(Command::run(LOOP));
        }))?;
        for event in [REPLY, LATER, LATER2] {
            let log = self.log.clone();
            r.subscribe(
                event.clone(),
                Box::new(move |env, params, out| {
                    log.borrow_mut().push(format!(
                        "event {event} at {} {}",
                        env.now().millis(),
                        describe(params)
                    ));
                    // A timer that comes again every `repeat` milliseconds.
                    if let Some(every) = params.get(REPEAT).and_then(|r| r.parse().ok()) {
                        let at = env.now().add_millis(every);
                        out.push(Command::work(
                            paperpaw_core::sdk::work::Work::at(event.clone(), at)
                                .with(params.clone()),
                        ));
                    }
                }),
            )?;
        }
        Ok(())
    }
}

/// A screen that logs what reaches it: `<name>:<what>`.
pub struct LogScreen {
    name: &'static str,
    log: Log,
    region: Option<Rect>,
    dims: bool,
    wake: Option<u64>,
}

impl LogScreen {
    fn new(name: &'static str, log: Log, params: &Params) -> Self {
        let region = (name == "overlay").then(|| {
            params
                .get(REGION)
                .and_then(parse_rect)
                .unwrap_or(Rect::new(0, 80, 120, 80))
        });
        Self {
            name,
            log,
            region,
            dims: params.get(DIMS).is_some(),
            wake: params.get(WAKE).and_then(|w| w.parse().ok()),
        }
    }

    fn note(&self, what: String) {
        self.log.borrow_mut().push(format!("{}:{what}", self.name));
    }
}

fn parse_rect(text: &str) -> Option<Rect> {
    let parts: Vec<u16> = text.split(',').filter_map(|p| p.parse().ok()).collect();
    match parts[..] {
        [x, y, width, height] => Some(Rect::new(x, y, width, height)),
        _ => None,
    }
}

impl Screen for LogScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        let area = self.region.unwrap_or(SCREEN);
        // Overlays paint all of their region (see `Screen::draw`).
        if self.region.is_some() {
            canvas.fill_rect(
                i32::from(area.x),
                i32::from(area.y),
                u32::from(area.width),
                u32::from(area.height),
                Color::gray(170),
            );
        }
        // A mark in the corner, to tell the screens apart.
        canvas.fill_rect(
            i32::from(area.x),
            i32::from(area.y),
            u32::from(area.width.min(4)),
            u32::from(area.height.min(4)),
            Color::BLACK,
        );
    }

    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        self.region
    }

    fn dims_below(&self) -> bool {
        self.dims
    }

    fn on_action(&mut self, action: &Id, _: &mut Cx<'_, '_>) -> Handled {
        if action.namespace() == "drv" {
            return Handled::No;
        }
        self.note(format!("action {action}"));
        let own = [
            actions::NEXT_PAGE,
            actions::PREV_PAGE,
            actions::CONFIRM,
            actions::UP,
            actions::DOWN,
            actions::LEFT,
            actions::RIGHT,
        ];
        if own.contains(action) {
            Handled::Yes
        } else {
            Handled::No
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, _: &mut Cx<'_, '_>) -> Handled {
        self.note(format!("gesture {:?}", gesture.kind));
        Handled::No
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.note(String::from("show"));
        if let Some(at) = self.wake {
            cx.wake_at(Instant(at));
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        self.note(format!("timer at {}", cx.env().now().millis()));
    }

    fn on_close(&mut self, _: &mut Cx<'_, '_>) {
        self.note(String::from("close"));
    }
}

/// Draws what changed, like a shell after each call, and checks that
/// every row passed lies on the screen and is as wide as it.
pub fn render(kernel: &mut Kernel) -> Vec<(u16, u16)> {
    let (width, height) = {
        let display = kernel.device().display;
        (display.width, display.height)
    };
    let mut bands = Vec::new();
    kernel.render(&mut |top, view| {
        assert!(
            u32::from(top) + u32::from(view.height) <= u32::from(height),
            "rows {top}..{} passed on a screen {height} high",
            u32::from(top) + u32::from(view.height)
        );
        assert!(view.width <= width, "{} wide on {width}", view.width);
        bands.push((top, view.height));
    });
    bands
}

pub fn button(name: &'static str, state: ButtonState) -> InputEvent {
    InputEvent::Button { name, state }
}

pub fn pointer(id: u8, phase: PointerPhase, x: i32, y: i32, tool: PointerTool) -> InputEvent {
    InputEvent::Pointer(PointerEvent::new(id, phase, Point::new(x, y), tool, 1000))
}

/// A short press of `name` at `at`.
pub fn press(kernel: &mut Kernel, at: u64, name: &'static str) {
    kernel.handle(Instant(at), button(name, ButtonState::Pressed));
    kernel.handle(Instant(at + 50), button(name, ButtonState::Released));
    render(kernel);
}

/// A tap with a finger at `(x, y)` at `at`.
pub fn tap(kernel: &mut Kernel, at: u64, x: i32, y: i32) {
    kernel.handle(
        Instant(at),
        pointer(0, PointerPhase::Down, x, y, PointerTool::Finger),
    );
    kernel.handle(
        Instant(at + 60),
        pointer(0, PointerPhase::Up, x, y, PointerTool::Finger),
    );
    render(kernel);
}

/// Whether `inner` lies within `outer`.
pub fn within(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}
