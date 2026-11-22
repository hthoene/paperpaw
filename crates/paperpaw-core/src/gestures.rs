//! Input to actions: buttons and fingers recognized, the edges' swipes,
//! the key map resolved, presses that came while a frame was not shown
//! yet dropped.

#[allow(
    unused_imports,
    reason = "a part of the kernel, which sees all of it; not every part uses every name"
)]
use super::*;
use crate::commands::{Reserved, vet_reserved};
use crate::native_host::native_action;

/// On the same screen, how long before the last frame showed a press
/// made more than a frame behind may be and still count: a click made
/// while the device was busy counts, a long tail of them does not (see
/// [`crate::Kernel::shown`]).
const STALE_MS: u64 = 1000;

/// Where input is: gestures being recognized, the edges' swipes, presses
/// dropped while a frame was not shown yet.
#[derive(Debug)]
pub(crate) struct InputState {
    pub(crate) recognizer: Recognizer,
    /// Pointer ids captured by the active screen.
    pub(crate) captured: Vec<u8>,
    /// A finger that came down at an edge, followed by the kernel for a
    /// swipe in from it (see [`keymap::InputKey::Edge`]).
    pub(crate) edge_touch: Option<EdgeTouch>,
    /// A swipe in from an edge on a screen with its own gestures, waiting
    /// for the second one that applies it: the side, and until when.
    pub(crate) edge_pending: Option<(Direction, Instant)>,
    pub(crate) settling: bool,
    /// Presses made before this, while another screen was coming up, are
    /// dropped (see [`Kernel::shown`]), with their releases.
    pub(crate) stale_before: Option<Instant>,
    /// When the last two frames showed: presses made before the earlier
    /// one are more than a frame behind and dropped (see
    /// [`Kernel::shown`]).
    pub(crate) shown_at: [Option<Instant>; 2],
    pub(crate) dropped_buttons: Vec<&'static str>,
    pub(crate) dropped_pointers: Vec<u8>,
    /// Page buttons pressed while they were swapped (see
    /// `Kernel::page_buttons_swapped`): let go, they stay swapped.
    pub(crate) swapped: Vec<&'static str>,
    /// A press or tap held back while a second may still make it double
    /// (something is bound to that): its input, until when, and the
    /// screen it began on.
    pub(crate) pending: Option<(InputKey, Instant, Option<u32>)>,
    /// The screen on top when the last button was pressed or finger came
    /// down: what is held is resolved for it.
    pub(crate) pressed_on: Option<u32>,
    /// The thresholds of recognition, read from the settings once until
    /// they change.
    pub(crate) thresholds: core::cell::Cell<Option<Thresholds>>,
}

impl Kernel {
    pub(crate) fn thresholds(&self) -> Thresholds {
        if let Some(known) = self.input.thresholds.get() {
            return known;
        }
        let metrics = Metrics::new(self.shared.device.display.dpi);
        let read = Thresholds {
            long_press_ms: self
                .shared
                .settings
                .int(core_settings::LONG_PRESS_MS.as_str())
                .max(0) as u64,
            tap_slop: metrics.mm(self.mm_setting(&core_settings::TAP_SLOP, 1, 20)),
            swipe_distance: metrics.mm(self.mm_setting(&core_settings::SWIPE_DISTANCE, 2, 50)),
        };
        self.input.thresholds.set(Some(read));
        read
    }

    /// Millimetres setting `setting` says, kept within `min` and `max`.
    fn mm_setting(&self, setting: &paperpaw_sdk::setting::Setting<i64>, min: i64, max: i64) -> u32 {
        self.shared.settings.read(setting).clamp(min, max) as u32
    }

    /// How long a second press or tap may come after the first to make
    /// it double, in milliseconds.
    fn double_ms(&self) -> u64 {
        let ms = self.shared.settings.read(&core_settings::DOUBLE_TAP_MS);
        ms.clamp(50, 2000) as u64
    }

    /// What a filter cancelled (`core.input-filter`): the button
    /// or finger no longer followed; a screen that took the finger is told
    /// it is gone.
    pub(crate) fn cancel_input(&mut self, event: &InputEvent) {
        match *event {
            InputEvent::Button { name, .. } => self.input.recognizer.forget_button(name),
            InputEvent::Pointer(p) => {
                self.input.recognizer.forget_pointer(p.id);
                if self.input.edge_touch.is_some_and(|t| t.id == p.id) {
                    self.input.edge_touch = None;
                }
                if self.input.captured.contains(&p.id) {
                    self.input.captured.retain(|&id| id != p.id);
                    let area = self.input_area();
                    let gone = p
                        .with_phase(PointerPhase::Cancel)
                        .with_point(areas::local(p.point, area));
                    self.with_input(|screen, cx| screen.on_pointer(&gone, cx));
                }
            }
            _ => {}
        }
    }

    pub(crate) fn pointer(&mut self, event: PointerEvent) {
        if event.phase == PointerPhase::Down {
            self.input.pressed_on = self.input_index().map(|index| self.stack[index].serial);
        }
        // Swipes in from an edge come before the screen sees the finger.
        // Held still there, it becomes a long press all the same.
        if let Some(touch) = self.input.edge_touch.filter(|t| t.id == event.id) {
            match event.phase {
                // The finger came down again: the lift of the touch at the
                // edge was lost (a filter took it, a sleep came between).
                // That touch is over; this one is new (#483).
                PointerPhase::Down => {
                    self.input.edge_touch = None;
                    self.input.recognizer.forget_pointer(event.id);
                }
                PointerPhase::Up => {
                    self.input.edge_touch = None;
                    let thresholds = self.thresholds();
                    if !self
                        .input
                        .recognizer
                        .lift_edge(event.id, self.shared.now, &thresholds)
                    {
                        self.edge_lifted(touch, event.point);
                    }
                }
                PointerPhase::Cancel => {
                    self.input.edge_touch = None;
                    self.input.recognizer.forget_pointer(event.id);
                }
                PointerPhase::Move => {
                    let thresholds = self.thresholds();
                    self.input
                        .recognizer
                        .pointer(&event, self.shared.now, &thresholds);
                }
                _ => {}
            }
            if event.phase != PointerPhase::Down {
                return;
            }
        }
        // A screen may take a finger before the edges do: what it takes
        // is meant for it (a grip that moves a row, a handle), as apps on
        // phones keep such places from the system's gestures.
        let captured = self.input.captured.contains(&event.id);
        if captured || event.phase == PointerPhase::Down {
            let area = self.input_area();
            let seen = event.with_point(areas::local(event.point, area));
            let handled = self.with_input(|screen, cx| screen.on_pointer(&seen, cx));
            if captured {
                if matches!(event.phase, PointerPhase::Up | PointerPhase::Cancel) {
                    self.input.captured.retain(|&id| id != event.id);
                }
                return;
            }
            if handled == Handled::Yes {
                self.input.captured.push(event.id);
                return;
            }
        }
        if event.phase == PointerPhase::Down
            && let Some(side) = self.edge_at(event.point)
        {
            self.input.edge_touch = Some(EdgeTouch {
                id: event.id,
                side,
                start: event.point,
                tool: event.tool,
            });
            let edge = match side {
                Direction::Up | Direction::Down => input::Edge::End,
                _ => input::Edge::Side,
            };
            self.input
                .recognizer
                .follow_edge(&event, self.shared.now, edge);
            return;
        }
        let thresholds = self.thresholds();
        if let Some(recognized) =
            self.input
                .recognizer
                .pointer(&event, self.shared.now, &thresholds)
        {
            self.recognized_filtered(recognized);
        }
    }

    /// How the screen shown takes gestures (`gestures.policy`).
    pub(crate) fn policy(&self) -> keymap::Policy {
        keymap::Policy::of(self.resolve(&InputKey::Policy))
    }

    /// The edge a finger coming down at `point` may swipe in from: one
    /// with something bound, where the screen lets edges apply.
    pub(crate) fn edge_at(&self, point: Point) -> Option<Direction> {
        if self.policy() == keymap::Policy::Off
            || self
                .input_index()
                .is_some_and(|index| self.stack[index].holds_edges())
        {
            return None;
        }
        // Inside the part of the screen the screen on top has: not on the
        // row of hints.
        let display = self.stack.last().map_or(self.content(), |top| top.area);
        if !display.contains(point) {
            return None;
        }
        // Wide at the top and the bottom: on Android the outermost strip is
        // the system's (its notifications, its home), a swipe begun just
        // inside it is ours. Narrow at the sides, where pages are tapped.
        let metrics = Metrics::new(self.shared.device.display.dpi);
        let mm = |setting: &paperpaw_sdk::setting::Setting<i64>| {
            let value = self.shared.settings.read(setting).clamp(0, 50) as u32;
            metrics.mm(value) as i32
        };
        let (across, along) = (
            mm(&core_settings::EDGE_ENDS),
            mm(&core_settings::EDGE_SIDES),
        );
        let (left, top) = (i32::from(display.x), i32::from(display.y));
        let (right, bottom) = (display.right() as i32, display.bottom() as i32);
        let side = if point.y < top + across {
            Direction::Up
        } else if point.y >= bottom - across {
            Direction::Down
        } else if point.x < left + along {
            Direction::Left
        } else if point.x >= right - along {
            Direction::Right
        } else {
            return None;
        };
        // Bound as the other side where the screen is mirrored.
        let bound = self
            .resolve(&InputKey::Edge(self.mirrored_side(side)))
            .is_some_and(|action| *action != paperpaw_sdk::builtin::actions::NOTHING);
        bound.then_some(side)
    }

    /// Whether the screen on top goes right to left: as the interface does.
    fn mirrored(&self) -> bool {
        use paperpaw_sdk::lang::Direction as Way;
        self.input_index().is_some() && self.shared.translator.direction() == Way::Rtl
    }

    /// `side` the other way, left for right, where the screen on top is
    /// mirrored ([`Kernel::mirrored`]).
    fn mirrored_side(&self, side: Direction) -> Direction {
        match (self.mirrored(), side) {
            (true, Direction::Left) => Direction::Right,
            (true, Direction::Right) => Direction::Left,
            _ => side,
        }
    }

    /// A finger that came down at an edge was lifted at `end`: a swipe in
    /// from the edge applies what it is bound to (on screens with their
    /// own gestures, a second one within a few seconds); anything else
    /// is what it would have been elsewhere, a tap or a swipe.
    pub(crate) fn edge_lifted(&mut self, touch: EdgeTouch, end: Point) {
        let (start, thresholds) = (touch.start, self.thresholds());
        let (dx, dy) = (end.x - start.x, end.y - start.y);
        let (inward, across) = match touch.side {
            Direction::Up => (dy, dx),
            Direction::Down => (-dy, dx),
            Direction::Left => (dx, dy),
            _ => (-dx, dy),
        };
        if inward >= thresholds.swipe_distance as i32 && across.abs() < inward {
            if self.policy() == keymap::Policy::Own {
                let again = self
                    .input
                    .edge_pending
                    .take()
                    .is_some_and(|(side, until)| side == touch.side && self.shared.now < until);
                if !again {
                    self.input.edge_pending = Some((touch.side, self.shared.now.add_millis(3000)));
                    let text = self.shared.translator.tr("core.edge-again").to_string();
                    self.output.commands.push(Ask::Notify(text));
                    return;
                }
            }
            return self.route_key(InputKey::Edge(self.mirrored_side(touch.side)));
        }
        let moved = input::distance(start, end);
        let kind = if moved <= thresholds.tap_slop {
            GestureKind::Tap
        } else if moved >= thresholds.swipe_distance {
            GestureKind::Swipe(input::direction(start, end))
        } else {
            return;
        };
        self.recognized_filtered(Recognized::Gesture(Gesture::new(
            kind, start, end, touch.tool,
        )));
    }

    /// Button `name` pressed (`long`: held): offered to the screen on
    /// top where it takes buttons, else through the key map.
    fn button(&mut self, name: &'static str, long: bool) {
        self.hint.pressed = self.hint.buttons.iter().position(|b| *b == name);
        if self.hint.pressed.is_some() {
            self.show_hints();
        }
        let name = Cow::Borrowed(name);
        if long {
            let on = self.input.pressed_on;
            self.route_key_on(InputKey::LongButton(name), on);
        } else {
            let double = InputKey::DoubleButton(name.clone());
            self.single_or_double(InputKey::Button(name), double);
        }
    }

    pub(crate) fn recognized(&mut self, recognized: Recognized) {
        match recognized {
            Recognized::Button { name, long } => self.button(name, long),
            Recognized::Gesture(gesture) => {
                if gesture.kind == GestureKind::Tap
                    && let Some(row) = self.hint_row()
                    && row.contains(gesture.start)
                {
                    let count = self.hint.buttons.len() as u32;
                    let x = u32::try_from(gesture.start.x).unwrap_or(0);
                    let cell = (x * count / u32::from(row.width).max(1)) as usize;
                    if let Some(button) = self.hint.buttons.get(cell) {
                        self.hint.pressed = Some(cell);
                        self.route_key(InputKey::Button(Cow::Borrowed(button)));
                    }
                    return;
                }
                let overlay = self
                    .stack
                    .len()
                    .checked_sub(1)
                    .and_then(|top| self.overlay(top));
                if let Some(region) = overlay
                    && gesture.kind == GestureKind::Tap
                    && !region.contains(gesture.start)
                {
                    return self.close_top(paperpaw_sdk::CloseCause::TapOutside);
                }
                // In the coordinates of the screen input goes to.
                let area = self.input_area();
                let gesture = areas::local_gesture(gesture, area);
                if self.offer(|screen, cx| screen.on_gesture(gesture, cx)) == Handled::Yes {
                    return;
                }
                // Screens with gestures of their own: none from the key map.
                if self.policy() != keymap::Policy::Standard {
                    return;
                }
                let mut zone =
                    self.shared
                        .keymap
                        .layout
                        .zone(gesture.start, area.width, area.height);
                // A screen that goes right to left (a book's pages, an
                // Arabic interface): sides and swipes the other way.
                if self.mirrored() {
                    zone = zone.mirrored();
                }
                let long = gesture.kind == GestureKind::LongPress;
                let tapped = matches!(gesture.kind, GestureKind::Tap | GestureKind::DoubleTap);
                if (tapped || long)
                    && let Some(action) = self.own_zone(gesture.start, long)
                {
                    self.route_key_pending();
                    let index = self.input_index();
                    let action = match index {
                        Some(_) => self.zone_action(action),
                        None => Some(action),
                    };
                    return action.map_or((), |action| self.dispatch(action));
                }
                let key = match gesture.kind {
                    GestureKind::Tap | GestureKind::DoubleTap => {
                        return self
                            .single_or_double(InputKey::Tap(zone), InputKey::DoubleTap(zone));
                    }
                    GestureKind::LongPress => {
                        let on = self.input.pressed_on;
                        return self.route_key_on(InputKey::LongTap(zone), on);
                    }
                    GestureKind::TwoFingerTap => InputKey::Tap2,
                    GestureKind::Swipe(direction) => InputKey::Swipe(self.mirrored_side(direction)),
                    GestureKind::Pinch { out } => InputKey::Pinch(out),
                    _ => return,
                };
                self.route_key(key);
            }
        }
    }

    /// Resolves an input through the key map layers shown (see
    /// [`Self::resolve`]); a press or tap held back for a second runs
    /// first, as it came first.
    pub(crate) fn route_key(&mut self, key: InputKey) {
        self.route_key_pending();
        let action = match self.input_index() {
            Some(index) => self.bound_action(index, &key),
            // No screen: the global layer alone, with no guard to keep.
            None => self.resolve(&key).cloned(),
        };
        if let Some(action) = action {
            self.dispatch(action);
        }
    }

    /// Runs the press or tap held back for a second, if any, as bound
    /// on the screen it began on.
    pub(crate) fn route_key_pending(&mut self) {
        if let Some((key, _, on)) = self.input.pending.take() {
            self.route_key_on(key, on);
        }
    }

    /// Runs what `key` is bound to on open screen `serial` (on top, where
    /// none is given); nothing where that screen closed meanwhile.
    pub(crate) fn route_key_on(&mut self, key: InputKey, serial: Option<u32>) {
        self.route_key_pending();
        let index = match serial {
            Some(serial) => self
                .stack
                .iter()
                .rposition(|active| active.serial == serial),
            None => self.stack.len().checked_sub(1),
        };
        let Some(index) = index else {
            return;
        };
        if let Some(action) = self.bound_action(index, &key) {
            self.dispatch(action);
        }
    }

    /// Input `single` came. With something bound to `double`, it waits
    /// for a second ([`core_settings::DOUBLE_TAP_MS`]): the second makes
    /// `double`, none `single`. Else `single` runs at once.
    fn single_or_double(&mut self, single: InputKey, double: InputKey) {
        if self.resolve(&double).is_none() {
            return self.route_key(single);
        }
        match self.input.pending.take() {
            Some((first, until, _)) if first == single && self.shared.now < until => {
                self.route_key(double)
            }
            other => {
                self.input.pending = other;
                self.route_key_pending();
                let until = self.shared.now.add_millis(self.double_ms());
                let on = self.input_index().map(|index| self.stack[index].serial);
                self.input.pending = Some((single, until, on));
            }
        }
    }

    /// What the tap (or long tap) at `point` in a zone of a layer's own
    /// (`[zones.<layer>]`) is bound to, if anything.
    fn own_zone(&self, point: Point, long: bool) -> Option<Id> {
        let display = self.input_area();
        let percent = |at: i32, of: u16| {
            let at = u64::try_from(at).unwrap_or(0);
            (at.saturating_mul(100) / u64::from(of).max(1)).min(100) as u8
        };
        let (layers, count) = self.layers();
        let (x, y) = (
            percent(point.x, display.width),
            percent(point.y, display.height),
        );
        let name = self.shared.keymap.zone_at(&layers[..count], x, y)?;
        let keymap = &self.shared.keymap;
        keymap.resolve_zone(&layers[..count], long, name).cloned()
    }

    /// The key map layers shown, the most specific first: the screen on
    /// top, the layer put in front for it ([`Ask::SetKeyLayer`]), the
    /// screen's slot, the global layer; and how many there are.
    fn layers(&self) -> ([&str; 4], usize) {
        match self.input_index() {
            Some(index) => self.layers_of(index),
            None => ([GLOBAL_KEYMAP_LAYER; 4], 1),
        }
    }

    /// The key map layers of open screen `index`, the most specific
    /// first (see [`Self::layers`]).
    pub(crate) fn layers_of(&self, index: usize) -> ([&str; 4], usize) {
        let mut layers = [GLOBAL_KEYMAP_LAYER; 4];
        let mut count = 0;
        let top = self.stack.get(index);
        // No layer of a screen below one that guards the device.
        let guarded = self.guard().is_some_and(|guard| index < guard);
        let named = [
            top.map(|top| top.id.as_str()),
            top.and_then(|top| top.key_layer.as_ref())
                .filter(|_| !guarded)
                .map(Id::as_str),
            top.and_then(|top| top.slot.as_ref()).map(Id::as_str),
        ];
        for layer in named.into_iter().flatten() {
            layers[count] = layer;
            count += 1;
        }
        // The global layer last, already in place.
        (layers, count + 1)
    }

    /// What `key` is bound to in the key map layers shown (see
    /// [`Self::layers`]).
    /// It runs many times in each call (the row of hints asks for every
    /// button in it), so it allocates nothing.
    pub(crate) fn resolve(&self, key: &InputKey) -> Option<&Id> {
        let (layers, count) = self.layers();
        self.shared.keymap.resolve(&layers[..count], key)
    }

    /// Offers input to the screen on top with `f`, and on to the screens
    /// below it while the one asked lets what it does not take pass
    /// ([`Screen::passes_through`]).
    /// The topmost screen that gets input: not one above a screen guarding
    /// the device that the guard did not open (a sleep screen).
    /// Beside a detail, the list where input went to it
    /// ([`Kernel::input_index`]; no screen guards the device then).
    pub(crate) fn input_top(&self) -> Option<usize> {
        match self.guard() {
            None => self.input_index(),
            Some(_) => self.unshut_top(),
        }
    }

    /// The topmost screen not shut out by a screen guarding the device
    /// (see [`Self::input_top`]).
    fn unshut_top(&self) -> Option<usize> {
        let guard = self.guard();
        (0..self.stack.len())
            .rev()
            .find(|&i| guard.is_none_or(|g| i <= g || self.stack[i].by_guard))
    }

    /// Whether the screen on top gets no input (see [`Self::input_top`]):
    /// one above a guard, not a detail whose list input went to.
    pub(crate) fn top_shut_out(&self) -> bool {
        self.unshut_top() != self.stack.len().checked_sub(1)
    }

    pub(crate) fn offer(
        &mut self,
        f: impl Fn(&mut crate::screen::Screen, &mut Turn<'_, '_>) -> Handled,
    ) -> Handled {
        let Some(index) = self.input_top() else {
            self.offered_down_to = 0;
            return Handled::No;
        };
        self.offered_down_to = index;
        self.with_screen(index, &f)
    }

    pub(crate) fn dispatch(&mut self, action: Id) {
        self.dispatch_with(action, Params::new());
    }

    /// Runs `action` with `params`: seen by the observers of actions,
    /// offered to the screens (which read the params from their
    /// context), else the handler put in its place with the highest
    /// priority or its own.
    pub(crate) fn dispatch_with(&mut self, action: Id, params: Params) {
        let before = mem::replace(&mut self.action_params, params);
        let handled = self.offer(|screen, cx| screen.on_action(&action, cx));
        let params = mem::replace(&mut self.action_params, before);
        if handled == Handled::Yes {
            return;
        }
        // The guard let it pass only where it was asked (not where a screen
        // above it took no action and passed nothing on), and could answer: a
        // plugin's screen hears input only on top, so a guard below a screen
        // it shuts out was never asked.
        let passed = !self.top_shut_out()
            && self
                .guard()
                .is_some_and(|guard| self.offered_down_to <= guard);
        self.run_handler(&action, &params, passed);
    }

    /// Runs the handler of `action` with `params`. What it asks is trusted
    /// to show above a screen guarding the device only where the guard was
    /// offered the action and let it pass (`passed`).
    pub(crate) fn run_handler(&mut self, action: &Id, params: &Params, passed: bool) {
        let env = self.shared.place(self.shared.now);
        let from = self.output.commands.len();
        let (reader, now) = (self.shared.reader, self.shared.now);
        let top = self.stack.last().map(|s| s.id.namespace());
        let mut refused_secret = false;
        match self.shared.actions.get(action.as_str()) {
            // An interface-7 plugin's: its callback runs from the pump.
            Some(spec) if spec.native().is_some() => {
                if let Some((owner, token)) = spec.native() {
                    let instances = &self.shared.native_instances;
                    if !native_action::queue_run(instances, owner, token, action.as_str(), &env) {
                        self.warn_runtime(format!("action `{action}`: its plugin is not running"));
                    }
                }
            }
            Some(spec) if spec.runs_itself() => {
                let _running = self.shared.meter.enter(action.namespace());
                let out = &mut self.output.commands;
                match self.data_actions.iter().find(|data| data.id == *action) {
                    // A data plugin's: its steps, each a command.
                    Some(data) => {
                        let steps = data.steps.iter().filter_map(|step| step.command(&env));
                        out.extend(steps);
                    }
                    None => commands::handing(out, |out| {
                        spec.run(&env, params, out);
                    }),
                }
                let owner = action.namespace();
                work::quiet(reader, top, now, from, owner, out, &mut self.works);
                let reserved = Reserved::of(&self.shared.settings, &self.shared.registry);
                refused_secret |= vet_reserved(out, from, owner, &reserved);
            }
            Some(_) => return,
            None => return self.warn_runtime(format!("unknown action `{action}`")),
        }
        if refused_secret {
            self.warn_runtime(format!(
                "`{action}`: another plugin's secret is not changed"
            ));
        }
        self.drop_self_runs(action, params, from);
        // An action a screen guarding the device let pass (the lock lets
        // sleep pass): what it opens may show above.
        if passed && self.guard().is_some() {
            self.trusted.extend(from..self.output.commands.len());
        }
    }

    /// Drops what was asked since `from` that runs `action` again with
    /// the same `params`: it would run itself without end (a handler in
    /// place of the action's own runs the original with
    /// [`Ask::RunOriginal`]). With other params it runs (stepping
    /// through profiles).
    fn drop_self_runs(&mut self, action: &Id, params: &Params, from: usize) {
        let before = self.output.commands.len();
        let mut index = 0;
        self.output.commands.retain(|command| {
            let again = index >= from
                && match command {
                    Ask::RunAction(a, p) | Ask::RunOriginal(a, p) => a == action && p == params,
                    _ => false,
                };
            index += 1;
            !again
        });
        if self.output.commands.len() != before {
            self.warn_runtime(format!("`{action}` runs itself; not run again"));
        }
    }

    /// Lays key map layer `layer` in front of the slot's for open screen
    /// `key` ([`Ask::SetKeyLayer`]); not for a screen below one that
    /// guards the device.
    pub(crate) fn set_key_layer(&mut self, key: paperpaw_sdk::ScreenKey, layer: Option<Id>) {
        let Some(index) = self
            .stack
            .iter()
            .position(|active| paperpaw_sdk::ScreenKey::new(active.serial) == key)
        else {
            return;
        };
        if self.guard().is_some_and(|guard| index < guard) {
            return;
        }
        self.stack[index].key_layer = layer;
    }

    /// The name the recognizer knows button `name` by while it is held:
    /// the other page button's where it was pressed swapped (see
    /// `Kernel::page_buttons_swapped`).
    fn held_as(&self, name: &'static str) -> &'static str {
        use paperpaw_platform::buttons::{PAGE_BACK, PAGE_FORWARD};
        match name {
            PAGE_FORWARD if self.input.swapped.contains(&name) => PAGE_BACK,
            PAGE_BACK if self.input.swapped.contains(&name) => PAGE_FORWARD,
            _ => name,
        }
    }

    /// Whether `event` belongs to a press dropped (see [`Kernel::shown`]).
    /// A button or a finger is down once at a time: its release ends the
    /// press of it held, the one taken or the one dropped, and a press
    /// taken ends one dropped before whose release was missed.
    pub(crate) fn stale(&mut self, now: Instant, event: &InputEvent) -> bool {
        // More than a frame behind, and long ago when the last showed.
        let behind = self.input.shown_at[0].is_some_and(|before| now < before)
            && self.input.shown_at[1].is_some_and(|last| last.0.saturating_sub(now.0) > STALE_MS);
        let early = self.input.stale_before.is_some_and(|before| now < before) || behind;
        match event {
            InputEvent::Button { name, state } => {
                let held = self.input.recognizer.holds(self.held_as(name));
                let dropped = self.input.dropped_buttons.iter().position(|b| b == name);
                let pressed = *state == ButtonState::Pressed;
                if let Some(at) = dropped
                    && (!pressed || !early)
                {
                    self.input.dropped_buttons.swap_remove(at);
                }
                // Pressed again while held: the press held goes on, its
                // release is still to come.
                if pressed && early && !held && dropped.is_none() {
                    self.input.dropped_buttons.push(name);
                }
                if pressed {
                    early
                } else {
                    !held && dropped.is_some()
                }
            }
            InputEvent::Pointer(p) if p.phase == PointerPhase::Down => {
                let dropped = self
                    .input
                    .dropped_pointers
                    .iter()
                    .position(|&id| id == p.id);
                if !early {
                    if let Some(at) = dropped {
                        self.input.dropped_pointers.swap_remove(at);
                    }
                    return false;
                }
                // Down again: the touch of that finger before is over.
                self.input.recognizer.forget_pointer(p.id);
                if self.input.edge_touch.is_some_and(|t| t.id == p.id) {
                    self.input.edge_touch = None;
                }
                if dropped.is_none() {
                    self.input.dropped_pointers.push(p.id);
                }
                true
            }
            InputEvent::Pointer(p) => match self
                .input
                .dropped_pointers
                .iter()
                .position(|&id| id == p.id)
            {
                Some(at) => {
                    if matches!(p.phase, PointerPhase::Up | PointerPhase::Cancel) {
                        self.input.dropped_pointers.remove(at);
                    }
                    true
                }
                None => false,
            },
            _ => false,
        }
    }
}
