//! Native screen wrappers encode scoped input without fabricating subscriptions.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_platform::{Instant, PixelFormat};

type DrawCall = (u32, u32, u32, i32, i32, Rect);

#[derive(Default)]
struct Recorder {
    calls: RefCell<Vec<(u32, Vec<u8>)>>,
    draws: RefCell<Vec<DrawCall>>,
    dropped: RefCell<Vec<(u32, i32)>>,
    result: Cell<i32>,
}
impl Dispatch for Recorder {
    fn event(&self, token: u32, bytes: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        self.calls.borrow_mut().push((token, bytes.to_vec()));
        Ok(self.result.get())
    }
    fn input(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32> {
        self.event(token, bytes, env)
    }
    fn draw(
        &self,
        token: u32,
        frame: &wire::DrawFrame<'_>,
        _: &Env<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        assert_eq!((frame.x, frame.y, frame.purpose, frame.id), (0, 0, 0, 0));
        assert!(frame.payload.is_empty());
        self.draws.borrow_mut().push((
            token,
            frame.width,
            frame.height,
            frame.clip_top,
            frame.clip_bottom,
            canvas.clip(),
        ));
        Ok(self.result.get())
    }
    fn dropped(&self, token: u32, status: i32) {
        self.dropped.borrow_mut().push((token, status));
    }
}
fn screen() -> (NativeScreen<Rc<Recorder>>, Rc<Recorder>) {
    let dispatch = Rc::new(Recorder::default());
    (
        NativeScreen::new(Id::new_static("fixture.screen"), 10, Rc::clone(&dispatch)),
        dispatch,
    )
}

#[test]
fn shown_and_resize_use_declared_screen_token_and_full_pane_size() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(123));
    let pane = Rect::new(5, 7, 30, 80);
    let device = env.device_within(pane);
    let env = env.within(&device, pane);
    let (mut screen, dispatch) = screen();
    let mut output = paperpaw_sdk::Output::default();
    let mut cx = Cx::new(&env, &mut output);
    screen.on_show(&mut cx);
    assert_eq!(screen.on_resize(&mut cx), Handled::Yes);
    for (token, bytes) in dispatch.calls.borrow().iter() {
        assert_eq!(*token, 10);
        let Record::EventScreenShown(event) = wire::records_decode_single(bytes).unwrap() else {
            panic!("wrong screen event")
        };
        assert_eq!(event.screen, "fixture.screen");
        assert_eq!((event.now_ms, event.width, event.height), (123, 30, 80));
    }
    assert_eq!(dispatch.calls.borrow().len(), 2);
}

#[test]
fn draw_keeps_full_pane_dimensions_separate_from_band_clip() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(123));
    let pane = Rect::new(5, 7, 30, 80);
    let device = env.device_within(pane);
    let env = env.within(&device, pane);
    let (mut screen, dispatch) = screen();
    let mut frame = paperpaw_graphics::Framebuffer::new(40, 100, PixelFormat::Gray8).unwrap();
    let mut base = frame.canvas();
    let mut band = base.clipped(Rect::new(5, 12, 30, 4)).offset(5, 7);
    screen.draw_visible(&mut band, &env);
    assert_eq!(
        *dispatch.draws.borrow(),
        [(10, 30, 80, 5, 9, Rect::new(0, 5, 30, 4))]
    );
    // No on_show/key means this is not an admitted top-screen instance.
    screen.draw(&mut band, &env);
    assert_eq!(dispatch.draws.borrow().len(), 1);
    dispatch.result.set(2);
    screen.draw_visible(&mut band, &env);
    assert_eq!(*dispatch.dropped.borrow(), [(10, ERR_INVALID)]);
}

/// A kernel with `stack` (id, overlay, guards) bottom first, keys 1, 2, …, and a
/// screen that is the one at `mine` (an index into it).
fn stacked(
    stack: &[(&'static str, bool, bool)],
    mine: usize,
) -> (crate::Kernel, NativeScreen<Rc<Recorder>>, Rc<Recorder>) {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    for (n, (id, overlay, guards)) in stack.iter().enumerate() {
        kernel.shared.stack.push(
            paperpaw_sdk::StackEntry::new(
                Id::new_static(id),
                None,
                *overlay,
                paperpaw_sdk::ScreenKey::new(n as u32 + 1),
            )
            .guarding(*guards),
        );
    }
    let dispatch = Rc::new(Recorder::default());
    let mut screen = NativeScreen::new(Id::new_static(stack[mine].0), 10, Rc::clone(&dispatch));
    screen.key = Some(paperpaw_sdk::ScreenKey::new(mine as u32 + 1));
    screen.flags = if stack[mine].2 { 1 << 1 } else { 0 };
    (kernel, screen, dispatch)
}

/// How many times the plugin was drawn by one kernel draw of `screen`.
fn drawn(
    kernel: &crate::Kernel,
    screen: &mut NativeScreen<Rc<Recorder>>,
    dispatch: &Recorder,
) -> usize {
    let env = kernel.shared.env(Instant(1));
    let mut frame = paperpaw_graphics::Framebuffer::new(40, 100, PixelFormat::Gray8).unwrap();
    let mut canvas = frame.canvas();
    let before = dispatch.draws.borrow().len();
    screen.draw(&mut canvas, &env);
    dispatch.draws.borrow().len() - before
}

#[test]
fn a_screen_below_overlays_only_is_drawn_as_if_on_top() {
    // The reader under its menu bar (an overlay): both are drawn, the page first.
    let (kernel, mut page, page_calls) = stacked(
        &[("reader.screen", false, false), ("reader.bar", true, false)],
        0,
    );
    assert_eq!(drawn(&kernel, &mut page, &page_calls), 1);
    let (kernel, mut bar, bar_calls) = stacked(
        &[("reader.screen", false, false), ("reader.bar", true, false)],
        1,
    );
    assert_eq!(drawn(&kernel, &mut bar, &bar_calls), 1);
    // Under two overlays, too.
    let (kernel, mut page, calls) = stacked(
        &[
            ("reader.screen", false, false),
            ("reader.bar", true, false),
            ("reader.dialog", true, false),
        ],
        0,
    );
    assert_eq!(drawn(&kernel, &mut page, &calls), 1);
}

#[test]
fn a_screen_below_a_full_screen_is_not_drawn() {
    let (kernel, mut home, calls) = stacked(
        &[
            ("home.screen", false, false),
            ("reader.screen", false, false),
        ],
        0,
    );
    assert_eq!(drawn(&kernel, &mut home, &calls), 0);
    // Nor one that has a full screen above an overlay's page.
    let (kernel, mut page, calls) = stacked(
        &[
            ("reader.screen", false, false),
            ("files.screen", false, false),
            ("reader.bar", true, false),
        ],
        0,
    );
    assert_eq!(drawn(&kernel, &mut page, &calls), 0);
}

#[test]
fn under_another_plugins_overlay_a_secret_or_guarded_screen_stays_blank() {
    // The PIN screen (it guards) under the sleep screen of another plugin.
    let (kernel, mut pin, calls) = stacked(
        &[("lock.pin", false, true), ("sleep.screen", true, false)],
        0,
    );
    assert_eq!(drawn(&kernel, &mut pin, &calls), 0);
    // The page below a PIN screen's overlay... of the same plugin is drawn.
    let (kernel, mut pin, calls) =
        stacked(&[("lock.pin", false, true), ("lock.keys", true, false)], 0);
    assert_eq!(drawn(&kernel, &mut pin, &calls), 1);
    // A page at or below a guard, under another plugin's overlay.
    let (kernel, mut page, calls) = stacked(
        &[
            ("reader.screen", false, false),
            ("lock.pin", false, true),
            ("sleep.screen", true, false),
        ],
        0,
    );
    // (the guard is a full screen: the reader is covered by it anyway)
    assert_eq!(drawn(&kernel, &mut page, &calls), 0);
    // The top screen draws, guard or not.
    let (kernel, mut top, calls) = stacked(&[("lock.pin", false, true)], 0);
    assert_eq!(drawn(&kernel, &mut top, &calls), 1);
    // A secret screen (bit 4) under another plugin's overlay is blank too.
    let (kernel, mut secret, calls) = stacked(
        &[("keys.entry", false, false), ("menu.bar", true, false)],
        0,
    );
    secret.flags = 1 << 4;
    assert_eq!(drawn(&kernel, &mut secret, &calls), 0);
}

#[test]
fn input_preserves_handled_and_reports_faults_without_claiming_success() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(123));
    let (mut screen, dispatch) = screen();
    let input = Record::InputAction(wire::InputAction {
        now_ms: 123,
        action: "core.next-page",
    });
    for (result, expected) in [
        (0, Handled::No),
        (1, Handled::Yes),
        (2, Handled::No),
        (-7, Handled::No),
    ] {
        dispatch.result.set(result);
        assert_eq!(screen.input(input, &env), expected);
    }
    assert_eq!(*dispatch.dropped.borrow(), [(10, ERR_INVALID), (10, -7)]);
    let calls = dispatch.calls.borrow();
    let (token, bytes) = &calls[0];
    assert_eq!(*token, 10);
    let Record::InputAction(action) = wire::records_decode_single(bytes).unwrap() else {
        panic!("wrong input record")
    };
    assert_eq!(action.action, "core.next-page");
    drop(calls);
    let mut output = paperpaw_sdk::Output::default();
    let mut cx = Cx::new(&env, &mut output);
    assert!(screen.mode().takes_raw_keys());
    assert_eq!(
        screen.on_key(
            &realtime::KeyEvent::pressed(buttons::POWER, Instant(123)),
            &mut cx
        ),
        Handled::No
    );
    assert_eq!(
        screen.on_key(
            &realtime::KeyEvent::pressed("unknown", Instant(123)),
            &mut cx
        ),
        Handled::No
    );
    assert_eq!(
        screen.on_action(&Id::new_static("core.next-page"), &mut cx),
        Handled::No
    );
    assert_eq!(dispatch.calls.borrow().len(), 4);
}

/// A kernel whose stack holds the fixture screen on top under `key`.
fn kernel_with_screen(key: paperpaw_sdk::ScreenKey) -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel.shared.stack.push(paperpaw_sdk::StackEntry::new(
        Id::new_static("fixture.screen"),
        None,
        false,
        key,
    ));
    kernel
}

fn pointer(id: u8, phase: PointerPhase, x: i32, y: i32) -> PointerEvent {
    PointerEvent::new(
        id,
        phase,
        paperpaw_platform::Point::new(x, y),
        paperpaw_platform::PointerTool::Finger,
        1000,
    )
}

fn decoded_pointers(dispatch: &Recorder) -> Vec<(i32, i32, u32, Option<u32>)> {
    dispatch
        .calls
        .borrow()
        .iter()
        .map(|(token, bytes)| {
            assert_eq!(*token, 10);
            let Record::InputPointer(p) = wire::records_decode_single(bytes).unwrap() else {
                panic!("not a pointer record")
            };
            (p.x, p.y, p.phase, p.contact)
        })
        .collect()
}

/// Todo 51: interleaved contacts reach the plugin whole, each with the id the
/// platform gave it, from its down to its up.
#[test]
fn interleaved_contacts_keep_their_ids_from_down_to_up() {
    let key = paperpaw_sdk::ScreenKey::new(3);
    let kernel = kernel_with_screen(key);
    let env = kernel.shared.env(Instant(5));
    let (mut screen, dispatch) = screen();
    let mut output = paperpaw_sdk::Output::default();
    let mut cx = Cx::new(&env, &mut output).for_screen(key);
    screen.on_show(&mut cx);
    dispatch.calls.borrow_mut().clear();
    dispatch.result.set(1);
    // A down, B down, B move, B up, A move, A up.
    let (a, b) = (1, 2);
    let script = [
        pointer(a, PointerPhase::Down, 10, 10),
        pointer(b, PointerPhase::Down, 50, 60),
        pointer(b, PointerPhase::Move, 51, 61),
        pointer(b, PointerPhase::Up, 52, 62),
        pointer(a, PointerPhase::Move, 11, 12),
        pointer(a, PointerPhase::Up, 12, 13),
    ];
    for event in &script {
        // Every event is delivered, and a handled one is reported handled.
        assert_eq!(screen.on_pointer(event, &mut cx), Handled::Yes);
    }
    assert_eq!(
        decoded_pointers(&dispatch),
        [
            (10, 10, 0, Some(1)),
            (50, 60, 0, Some(2)),
            (51, 61, 1, Some(2)),
            (52, 62, 2, Some(2)),
            (11, 12, 1, Some(1)),
            (12, 13, 2, Some(1)),
        ]
    );
}

#[test]
fn cancel_keeps_the_contact_id_and_the_extremes_pass_through() {
    let key = paperpaw_sdk::ScreenKey::new(3);
    let kernel = kernel_with_screen(key);
    let env = kernel.shared.env(Instant(5));
    let (mut screen, dispatch) = screen();
    let mut output = paperpaw_sdk::Output::default();
    let mut cx = Cx::new(&env, &mut output).for_screen(key);
    screen.on_show(&mut cx);
    dispatch.calls.borrow_mut().clear();
    // Contact 0 (a single-contact device) is sent as 0, not left out; 255 is
    // the largest the platform has. A cancel names its contact too.
    for (id, phase) in [
        (0, PointerPhase::Down),
        (0, PointerPhase::Cancel),
        (255, PointerPhase::Down),
        (7, PointerPhase::Down),
        (255, PointerPhase::Cancel),
        (7, PointerPhase::Up),
    ] {
        screen.on_pointer(&pointer(id, phase, 1, 2), &mut cx);
    }
    assert_eq!(
        decoded_pointers(&dispatch),
        [
            (1, 2, 0, Some(0)),
            (1, 2, 3, Some(0)),
            (1, 2, 0, Some(255)),
            (1, 2, 0, Some(7)),
            (1, 2, 3, Some(255)),
            (1, 2, 2, Some(7)),
        ]
    );
}

#[test]
fn a_screen_below_the_top_gets_no_pointer_at_all() {
    let key = paperpaw_sdk::ScreenKey::new(3);
    let mut kernel = kernel_with_screen(key);
    let env = kernel.shared.env(Instant(5));
    let (mut screen, dispatch) = screen();
    let mut output = paperpaw_sdk::Output::default();
    let mut cx = Cx::new(&env, &mut output).for_screen(key);
    screen.on_show(&mut cx);
    dispatch.calls.borrow_mut().clear();
    // Outside the stack (no key), and with another screen over it.
    let mut aside = paperpaw_sdk::Output::default();
    let mut outside = Cx::new(&env, &mut aside);
    assert_eq!(
        screen.on_pointer(&pointer(1, PointerPhase::Down, 1, 1), &mut outside),
        Handled::No
    );
    kernel.shared.stack.push(paperpaw_sdk::StackEntry::new(
        Id::new_static("other.screen"),
        None,
        false,
        paperpaw_sdk::ScreenKey::new(4),
    ));
    let env = kernel.shared.env(Instant(6));
    let mut covered = paperpaw_sdk::Output::default();
    let mut cx = Cx::new(&env, &mut covered).for_screen(key);
    assert_eq!(
        screen.on_pointer(&pointer(1, PointerPhase::Down, 1, 1), &mut cx),
        Handled::No
    );
    assert!(dispatch.calls.borrow().is_empty());
}

#[test]
fn only_native_overlays_show_the_screens_below_them() {
    let (screen, _) = screen();
    assert_eq!(screen.shows_through(), paperpaw_sdk::ShowsThrough::Nothing);
    let screen = screen.with_flags(1 << 2);
    assert_eq!(screen.shows_through(), paperpaw_sdk::ShowsThrough::All);
}

#[test]
fn a_screen_declared_hints_hidden_says_so_and_another_leaves_it_to_its_plugin() {
    let (screen, _) = screen();
    assert_eq!(screen.hints(), None);
    let screen = screen.with_flags(1 << 8);
    assert_eq!(screen.hints(), Some(paperpaw_sdk::HintVisibility::Hidden));
    // Beside the other policy bits.
    let (screen, _) = self::screen();
    let screen = screen.with_flags((1 << 8) | (1 << 5));
    assert_eq!(screen.hints(), Some(paperpaw_sdk::HintVisibility::Hidden));
    assert!(screen.holds_edges());
}

fn fresh() -> (NativeScreen<Rc<Recorder>>, Rc<Recorder>) {
    screen()
}

/// What the recorder was sent as `event.text_typed`: (text, done).
fn typed_calls(dispatch: &Recorder) -> Vec<(alloc::string::String, bool)> {
    dispatch
        .calls
        .borrow()
        .iter()
        .map(|(token, bytes)| {
            assert_eq!(*token, 10);
            let Record::EventTextTyped(event) = wire::records_decode_single(bytes).unwrap() else {
                panic!("wrong typed record")
            };
            assert!(event.flags <= 1, "no other flag bits");
            (alloc::string::String::from(event.text), event.flags == 1)
        })
        .collect()
}

#[test]
fn typed_text_comes_in_chunks_of_1024_bytes_cut_between_characters_done_on_the_last() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(5));
    // Short: one event, as is.
    let (screen, dispatch) = fresh();
    screen.typed("hello", false, &env);
    screen.typed("ok", true, &env);
    assert_eq!(
        typed_calls(&dispatch),
        [("hello".into(), false), ("ok".into(), true)]
    );
    // Exactly the limit is one event; one more byte is two.
    let (screen, dispatch) = fresh();
    let full = "a".repeat(1024);
    screen.typed(&full, true, &env);
    assert_eq!(typed_calls(&dispatch), [(full.clone(), true)]);
    let (screen, dispatch) = fresh();
    let over = "a".repeat(1025);
    screen.typed(&over, true, &env);
    assert_eq!(
        typed_calls(&dispatch),
        [("a".repeat(1024), false), ("a".into(), true)]
    );
    // A character that straddles the limit goes whole into the next event.
    let (screen, dispatch) = fresh();
    let straddle = alloc::format!("{}é{}", "a".repeat(1023), "b".repeat(2000));
    screen.typed(&straddle, false, &env);
    let calls = typed_calls(&dispatch);
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0], ("a".repeat(1023), false));
    assert!(calls.iter().all(|(text, done)| text.len() <= 1024 && !done));
    let joined: alloc::string::String = calls.iter().map(|(text, _)| text.as_str()).collect();
    assert_eq!(joined, straddle);
    // Four-byte characters in a long run are never cut either.
    let (screen, dispatch) = fresh();
    let emoji = "😀".repeat(700);
    screen.typed(&emoji, true, &env);
    let calls = typed_calls(&dispatch);
    let joined: alloc::string::String = calls.iter().map(|(text, _)| text.as_str()).collect();
    assert_eq!(joined, emoji);
    assert!(calls.iter().all(|(text, _)| text.len() <= 1024));
    assert_eq!(calls.iter().filter(|(_, done)| *done).count(), 1);
    assert!(calls.last().unwrap().1);
}

#[test]
fn empty_typed_text_confirms_only_with_done_and_a_refused_event_stops_the_rest() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(5));
    let (screen, dispatch) = fresh();
    screen.typed("", false, &env);
    assert!(dispatch.calls.borrow().is_empty());
    screen.typed("", true, &env);
    assert_eq!(typed_calls(&dispatch), [("".into(), true)]);
    // The plugin refuses the first of several: its token is dropped, and
    // the others are not sent.
    let (screen, dispatch) = fresh();
    dispatch.result.set(-7);
    screen.typed(&"a".repeat(3000), true, &env);
    assert_eq!(dispatch.calls.borrow().len(), 1);
    assert_eq!(*dispatch.dropped.borrow(), [(10, -7)]);
}

#[test]
fn only_type_text_reaches_a_screen_on_top_and_never_before_it_is_shown() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(5));
    let (mut screen, dispatch) = fresh();
    let mut output = paperpaw_sdk::Output::default();
    let mut cx = Cx::new(&env, &mut output);
    let typed = events::TypeText::new("x".into()).done(true).to_params();
    // Not shown yet (no instance key): nothing, whatever the event.
    screen.on_event(events::TYPE_TEXT.id(), &typed, &mut cx);
    screen.on_event(events::SETTING_CHANGED.id(), &typed, &mut cx);
    // Undecodable data is ignored too.
    screen.on_event(
        events::TYPE_TEXT.id(),
        &paperpaw_sdk::Params::new(),
        &mut cx,
    );
    assert!(dispatch.calls.borrow().is_empty());
}

#[test]
fn typed_text_over_the_total_cap_is_dropped_whole_and_the_cap_itself_is_sent() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(5));
    let (screen, dispatch) = fresh();
    screen.typed(&"a".repeat(TYPED_TOTAL_MAX + 1), true, &env);
    assert!(dispatch.calls.borrow().is_empty());
    assert!(dispatch.dropped.borrow().is_empty());
    screen.typed(&"a".repeat(TYPED_TOTAL_MAX), true, &env);
    let calls = typed_calls(&dispatch);
    assert_eq!(calls.len(), TYPED_TOTAL_MAX / 1024);
    assert_eq!(calls.iter().filter(|(_, done)| *done).count(), 1);
}

#[test]
fn a_shown_top_screen_not_offered_on_the_keyboard_slot_gets_no_typed_text() {
    let key = paperpaw_sdk::ScreenKey::new(3);
    let kernel = kernel_with_screen(key);
    let env = kernel.shared.env(Instant(5));
    let (mut screen, dispatch) = fresh();
    let mut output = paperpaw_sdk::Output::default();
    let mut cx = Cx::new(&env, &mut output).for_screen(key);
    screen.on_show(&mut cx);
    dispatch.calls.borrow_mut().clear();
    // It is shown and on top (a pointer reaches it), but no screen is
    // offered on `core.keyboard`, and the event is the real one.
    assert!(screen.is_top(&cx));
    assert_eq!(cx.env().slot_screen(slots::KEYBOARD.as_str()), None);
    let typed = events::TypeText::new("secret".into())
        .done(true)
        .to_params();
    screen.on_event(events::TYPE_TEXT.id(), &typed, &mut cx);
    assert!(dispatch.calls.borrow().is_empty());
}
