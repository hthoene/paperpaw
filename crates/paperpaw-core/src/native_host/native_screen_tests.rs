//! Native screen wrappers encode scoped input without fabricating subscriptions.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::Staging;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_platform::{Instant, PixelFormat};
use paperpaw_sdk::{Manifest, RegisterError};

type DrawCall = (u32, u32, u32, i32, i32, Rect);

#[derive(Default)]
struct Recorder {
    calls: RefCell<Vec<(u32, Vec<u8>)>>,
    draws: RefCell<Vec<DrawCall>>,
    /// The `id` of each draw's frame.
    ids: RefCell<Vec<u32>>,
    /// Whether a draw draws a picture over most of the screen.
    picture: Cell<bool>,
    /// The draws that drew more than pictures (all but those told to
    /// draw only these).
    texts: Cell<usize>,
    dropped: RefCell<Vec<(u32, i32)>>,
    result: Cell<i32>,
    /// What the kernel answers to `secret_now` (`None`: it cannot tell).
    secret_now: Cell<Option<bool>>,
}
impl Dispatch for Recorder {
    fn event(&self, token: u32, bytes: &[u8], _: &Place<'_>) -> Result<i32, i32> {
        self.calls.borrow_mut().push((token, bytes.to_vec()));
        Ok(self.result.get())
    }
    fn input(&self, token: u32, bytes: &[u8], env: &Place<'_>) -> Result<i32, i32> {
        self.event(token, bytes, env)
    }
    fn draw(
        &self,
        token: u32,
        frame: &wire::DrawFrame<'_>,
        env: &Place<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        assert_eq!((frame.x, frame.y, frame.purpose), (0, 0, 0));
        assert_eq!(frame.id & !PICTURES_ONLY_ID, 0);
        assert!(frame.payload.is_empty());
        self.ids.borrow_mut().push(frame.id);
        // A page: its text, left out where only pictures count, and its picture.
        if frame.id & PICTURES_ONLY_ID == 0 {
            self.texts.set(self.texts.get() + 1);
        }
        if self.picture.get() {
            let image = paperpaw_sdk::document::ImageRef {
                path: "p.gray".into(),
                width: 8,
                height: 8,
            };
            let area = Rect::new(10, 10, 60, 60);
            crate::kit::ui::draw_image(canvas, env.storage(), &image, area);
        }
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
    fn secret_now(&self, _: Option<ScreenKey>) -> Option<bool> {
        self.secret_now.get()
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
    let env = kernel.shared.place(Instant(123));
    let pane = Rect::new(5, 7, 30, 80);
    let device = env.device_within(pane);
    let env = env.within(&device, pane);
    let (mut screen, dispatch) = screen();
    let mut output = crate::Output::default();
    let mut cx = Turn::new(&env, &mut output);
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
    let env = kernel.shared.place(Instant(123));
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
    let env = kernel.shared.place(Instant(1));
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
fn a_screen_that_becomes_secret_at_run_time_goes_blank_under_another_plugins_overlay() {
    // Declared plain, made secret by `command.screen_policy`: the kernel says so.
    let (kernel, mut page, calls) = stacked(
        &[("keys.entry", false, false), ("sleep.screen", true, false)],
        0,
    );
    assert_eq!(drawn(&kernel, &mut page, &calls), 1);
    calls.secret_now.set(Some(true));
    assert_eq!(drawn(&kernel, &mut page, &calls), 0);
    // Its own plugin's overlay does not blank it, nor does a kernel that says no.
    let (kernel, mut page, calls) = stacked(
        &[("keys.entry", false, false), ("keys.menu", true, false)],
        0,
    );
    calls.secret_now.set(Some(true));
    assert_eq!(drawn(&kernel, &mut page, &calls), 1);
}

#[test]
fn a_secret_page_stays_blank_under_a_foreign_overlay_however_often_it_is_drawn() {
    // A redraw asked for the secret page (a whole one, then an area) reaches the
    // kernel's draw again and again: the plugin is never called while the sleep
    // screen of another plugin covers it.
    let (kernel, mut page, calls) = stacked(
        &[("keys.entry", false, false), ("sleep.screen", true, false)],
        0,
    );
    calls.secret_now.set(Some(true));
    for _ in 0..3 {
        assert_eq!(drawn(&kernel, &mut page, &calls), 0);
    }
    // The sleep screen gone, it is the top again and draws.
    let (kernel, mut page, calls) = stacked(&[("keys.entry", false, false)], 0);
    calls.secret_now.set(Some(true));
    assert_eq!(drawn(&kernel, &mut page, &calls), 1);
}

#[test]
fn a_dialog_above_a_guard_stays_blank_under_the_sleep_screen() {
    // The lock guards; its PIN dialog (an overlay of the lock, so secret as it
    // lies above a guard) must not show through the sleep screen of another
    // plugin that lies on top.
    let stack = [
        ("lock.screen", false, true),
        ("lock.pin", true, false),
        ("sleep.screen", true, false),
    ];
    let (kernel, mut pin, calls) = stacked(&stack, 1);
    assert_eq!(drawn(&kernel, &mut pin, &calls), 0);
    // The same dialog with nothing foreign on top is drawn.
    let (kernel, mut pin, calls) = stacked(&stack[..2], 1);
    assert_eq!(drawn(&kernel, &mut pin, &calls), 1);
}

#[test]
fn input_preserves_handled_and_reports_faults_without_claiming_success() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.place(Instant(123));
    let mut output = crate::Output::default();
    let mut cx = Turn::new(&env, &mut output);
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
        assert_eq!(screen.input(input, &mut cx), expected);
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
    let mut output = crate::Output::default();
    let mut cx = Turn::new(&env, &mut output);
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
    let env = kernel.shared.place(Instant(5));
    let (mut screen, dispatch) = screen();
    let mut output = crate::Output::default();
    let mut cx = Turn::new(&env, &mut output).for_screen(key);
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
    let env = kernel.shared.place(Instant(5));
    let (mut screen, dispatch) = screen();
    let mut output = crate::Output::default();
    let mut cx = Turn::new(&env, &mut output).for_screen(key);
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
    let env = kernel.shared.place(Instant(5));
    let (mut screen, dispatch) = screen();
    let mut output = crate::Output::default();
    let mut cx = Turn::new(&env, &mut output).for_screen(key);
    screen.on_show(&mut cx);
    dispatch.calls.borrow_mut().clear();
    // Outside the stack (no key), and with another screen over it.
    let mut aside = crate::Output::default();
    let mut outside = Turn::new(&env, &mut aside);
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
    let env = kernel.shared.place(Instant(6));
    let mut covered = crate::Output::default();
    let mut cx = Turn::new(&env, &mut covered).for_screen(key);
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

#[test]
fn a_screen_declared_hints_on_press_has_the_row_only_after_a_press() {
    use paperpaw_sdk::HintVisibility::{Hidden, OnPress, Shown};
    for (flags, hints) in [
        ((2 << 13) | (2 << 11), OnPress),
        (1 << 13, Shown),
        (3 << 13, Hidden),
        ((3 << 13) | (1 << 8), Hidden),
    ] {
        let (screen, _) = screen();
        assert_eq!(screen.with_flags(flags).hints(), Some(hints), "{flags:#x}");
    }
}

#[test]
fn a_screen_declared_to_dim_what_is_below_says_so_and_another_does_not() {
    let (screen, _) = screen();
    assert!(!screen.dims_below());
    let screen = screen.with_flags((1 << 10) | (1 << 2));
    assert!(screen.dims_below());
    // The other policy bits are not it: an overlay alone does not dim.
    let (screen, _) = self::screen();
    let screen = screen.with_flags((1 << 9) | (1 << 8) | (1 << 5) | (1 << 2));
    assert!(!screen.dims_below());
}

#[test]
fn a_screen_declared_grays_says_so_and_another_does_not() {
    let (screen, _) = screen();
    assert!(!screen.grays());
    let screen = screen.with_flags(1 << 9);
    assert!(screen.grays());
    // Beside the other policy bits, which are not it.
    let (screen, _) = self::screen();
    let screen = screen.with_flags((1 << 8) | (1 << 5));
    assert!(!screen.grays());
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
    let env = kernel.shared.place(Instant(5));
    let mut output = crate::Output::default();
    let mut cx = Turn::new(&env, &mut output);
    // Short: one event, as is.
    let (screen, dispatch) = fresh();
    screen.typed("hello", false, &mut cx);
    screen.typed("ok", true, &mut cx);
    assert_eq!(
        typed_calls(&dispatch),
        [("hello".into(), false), ("ok".into(), true)]
    );
    // Exactly the limit is one event; one more byte is two.
    let (screen, dispatch) = fresh();
    let full = "a".repeat(1024);
    screen.typed(&full, true, &mut cx);
    assert_eq!(typed_calls(&dispatch), [(full.clone(), true)]);
    let (screen, dispatch) = fresh();
    let over = "a".repeat(1025);
    screen.typed(&over, true, &mut cx);
    assert_eq!(
        typed_calls(&dispatch),
        [("a".repeat(1024), false), ("a".into(), true)]
    );
    // A character that straddles the limit goes whole into the next event.
    let (screen, dispatch) = fresh();
    let straddle = alloc::format!("{}é{}", "a".repeat(1023), "b".repeat(2000));
    screen.typed(&straddle, false, &mut cx);
    let calls = typed_calls(&dispatch);
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0], ("a".repeat(1023), false));
    assert!(calls.iter().all(|(text, done)| text.len() <= 1024 && !done));
    let joined: alloc::string::String = calls.iter().map(|(text, _)| text.as_str()).collect();
    assert_eq!(joined, straddle);
    // Four-byte characters in a long run are never cut either.
    let (screen, dispatch) = fresh();
    let emoji = "😀".repeat(700);
    screen.typed(&emoji, true, &mut cx);
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
    let env = kernel.shared.place(Instant(5));
    let mut output = crate::Output::default();
    let mut cx = Turn::new(&env, &mut output);
    let (screen, dispatch) = fresh();
    screen.typed("", false, &mut cx);
    assert!(dispatch.calls.borrow().is_empty());
    screen.typed("", true, &mut cx);
    assert_eq!(typed_calls(&dispatch), [("".into(), true)]);
    // The plugin refuses the first of several: its token is dropped, and
    // the others are not sent.
    let (screen, dispatch) = fresh();
    dispatch.result.set(-7);
    screen.typed(&"a".repeat(3000), true, &mut cx);
    assert_eq!(dispatch.calls.borrow().len(), 1);
    assert_eq!(*dispatch.dropped.borrow(), [(10, -7)]);
}

#[test]
fn only_type_text_reaches_a_screen_on_top_and_never_before_it_is_shown() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.place(Instant(5));
    let (mut screen, dispatch) = fresh();
    let mut output = crate::Output::default();
    let mut cx = Turn::new(&env, &mut output);
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
    let env = kernel.shared.place(Instant(5));
    let mut output = crate::Output::default();
    let mut cx = Turn::new(&env, &mut output);
    let (screen, dispatch) = fresh();
    screen.typed(&"a".repeat(TYPED_TOTAL_MAX + 1), true, &mut cx);
    assert!(dispatch.calls.borrow().is_empty());
    assert!(dispatch.dropped.borrow().is_empty());
    screen.typed(&"a".repeat(TYPED_TOTAL_MAX), true, &mut cx);
    let calls = typed_calls(&dispatch);
    assert_eq!(calls.len(), TYPED_TOTAL_MAX / 1024);
    assert_eq!(calls.iter().filter(|(_, done)| *done).count(), 1);
}

#[test]
fn a_shown_top_screen_not_offered_on_the_keyboard_slot_gets_no_typed_text() {
    let key = paperpaw_sdk::ScreenKey::new(3);
    let kernel = kernel_with_screen(key);
    let env = kernel.shared.place(Instant(5));
    let (mut screen, dispatch) = fresh();
    let mut output = crate::Output::default();
    let mut cx = Turn::new(&env, &mut output).for_screen(key);
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

#[test]
fn only_the_draw_that_counts_pictures_is_marked_as_one() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.place(Instant(0));
    let pane = Rect::new(5, 7, 30, 80);
    let device = env.device_within(pane);
    let env = env.within(&device, pane);
    let (screen, dispatch) = screen();
    // Shown: pixels are kept.
    let mut shown = paperpaw_graphics::Framebuffer::new(40, 100, PixelFormat::Gray8).unwrap();
    screen.draw_visible(&mut shown.canvas().offset(5, 7), &env);
    // Aside for the count of pictures, then aside for its parts.
    let mut aside = paperpaw_graphics::Framebuffer::blind(40, 100);
    screen.draw_visible(&mut aside.canvas().offset(5, 7), &env);
    let mut parts = Vec::new();
    let asked = aside.canvas().describing(&mut parts);
    screen.draw_visible(&mut asked.offset(5, 7), &env);
    assert_eq!(*dispatch.ids.borrow(), [0, PICTURES_ONLY_ID, 0]);
}

/// A page that tells `Recorder` how it was drawn: aside for the count of its
/// pictures, or shown.
struct Page(Rc<Recorder>);

impl crate::testing::Fake for Page {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Place<'_>) {
        let counted = canvas.is_blind() && !canvas.is_describing();
        let frame = wire::DrawFrame {
            id: if counted { PICTURES_ONLY_ID } else { 0 },
            ..super::super::frames::screen(0, 0, 1, 1, (0, 1))
        };
        let _ = self.0.draw(10, &frame, env, canvas);
    }
}

/// A plugin whose home screen is a native one, drawn by `Recorder`.
fn page_plugin(kept: Rc<Recorder>) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        let dispatch = Rc::clone(&kept);
        const ID: Id = Id::new_static("page.screen");
        r.add_fake(crate::testing::FakeSpec::new(
            ID,
            "page",
            alloc::boxed::Box::new(move |_, _| alloc::boxed::Box::new(Page(Rc::clone(&dispatch)))),
        ))?;
        r.bind_slot(slots::HOME, ID, paperpaw_sdk::Priority::DEFAULT)
    }
}

/// What [`page_plugin`] registers under.
fn page_plugin_manifest() -> Manifest {
    let id = Id::new_static("page");
    paperpaw_sdk::Manifest::new(id, "Page", paperpaw_sdk::Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

/// A black and white panel coming up with [`PagePlugin`]'s page, with a
/// picture on it or none: whether it came up in grays, and its draws.
fn page_comes_up(picture: bool) -> (bool, Rc<Recorder>) {
    use paperpaw_sdk::image::{GrayWriter, ImageSink};
    let storage = paperpaw_sdk::MemoryStorage::new();
    let mut writer = GrayWriter::new(&storage, "p.gray");
    writer.start(8, 8).unwrap();
    for _ in 0..8 {
        writer.row(&[90; 8]).unwrap();
    }
    writer.finish().unwrap();
    let device = crate::tests::DEVICE;
    let device = device.with_display(device.display.with_format(PixelFormat::Mono1));
    let dispatch = Rc::new(Recorder::default());
    dispatch.picture.set(picture);
    let mut kernel = crate::Kernel::builder(device)
        .storage(storage)
        .items(page_plugin_manifest(), page_plugin(Rc::clone(&dispatch)))
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    assert!(effects.refresh().is_some());
    (effects.grays(), dispatch)
}

#[test]
fn a_page_with_pictures_comes_up_in_black_and_white_without_a_draw_to_count_them() {
    // Black and white first (the grays follow after a calm, `gray_later`): no
    // draw aside to find out whether there are pictures, which was the
    // slowest part of the frame.
    let (grays, dispatch) = page_comes_up(true);
    assert!(!grays);
    let marked = |id: &&u32| **id == PICTURES_ONLY_ID;
    assert_eq!(dispatch.ids.borrow().iter().filter(marked).count(), 0);
    let (grays, dispatch) = page_comes_up(false);
    assert!(!grays);
    assert_eq!(dispatch.ids.borrow().iter().filter(marked).count(), 0);
}

/// A plugin that refuses the open of its screen (`ERR_INVALID`: wrong
/// parameters) leaves no blank screen on the stack: it is closed again and
/// the user is told.
#[test]
fn a_screen_whose_open_the_plugin_refused_is_closed_with_a_notice() {
    let key = paperpaw_sdk::ScreenKey::new(3);
    let kernel = kernel_with_screen(key);
    let env = kernel.shared.place(Instant(5));
    let (mut screen, dispatch) = screen();
    dispatch.result.set(2);
    let mut output = crate::Output::default();
    let mut cx = Turn::new(&env, &mut output).for_screen(key);
    screen.on_show(&mut cx);
    assert_eq!(*dispatch.dropped.borrow(), [(10, ERR_INVALID)]);
    // Closed, told, and the plugin not asked to show what it refused.
    assert_eq!(dispatch.calls.borrow().len(), 1);
    assert_eq!(output.commands.len(), 2);
    assert_eq!(output.commands[0], Ask::Close(key));
    assert!(matches!(&output.commands[1], Ask::Notify(text) if !text.is_empty()));
}

/// What `event.screen_hidden` the plugin was sent, as (screen, instance, time).
fn hidden_sent(dispatch: &Recorder) -> Vec<(alloc::string::String, u32, u64)> {
    dispatch
        .calls
        .borrow()
        .iter()
        .map(|(token, bytes)| {
            assert_eq!(*token, 10);
            let Record::EventScreenHidden(event) = wire::records_decode_single(bytes).unwrap()
            else {
                panic!("wrong screen event")
            };
            (event.screen.into(), event.instance, event.now_ms)
        })
        .collect()
}

/// The kernel tells the screen at `mine` it is covered, as `Kernel::tell_hidden` does.
fn told_hidden(kernel: &crate::Kernel, screen: &mut NativeScreen<Rc<Recorder>>, mine: u32) {
    let env = kernel.shared.place(Instant(5));
    let mut output = crate::Output::default();
    let mut cx = Turn::new(&env, &mut output).for_screen(ScreenKey::new(mine));
    screen.on_event(
        &Id::new_static(HIDDEN),
        &paperpaw_sdk::Params::new(),
        &mut cx,
    );
}

#[test]
fn a_covered_open_native_screen_is_sent_screen_hidden_and_nothing_else_is() {
    let below = [
        ("home.screen", false, false),
        ("files.screen", false, false),
    ];
    let (kernel, mut screen, dispatch) = stacked(&below, 0);
    // Not announced (`screen_opened` not sent yet): nothing.
    told_hidden(&kernel, &mut screen, 1);
    assert!(hidden_sent(&dispatch).is_empty());
    screen.opened = true;
    told_hidden(&kernel, &mut screen, 1);
    assert_eq!(hidden_sent(&dispatch), [("home.screen".into(), 1, 5)]);
    // On top (nothing above): an event of that name emitted by a plugin does nothing.
    let (kernel, mut top, dispatch) = stacked(&below[..1], 0);
    top.opened = true;
    told_hidden(&kernel, &mut top, 1);
    assert!(hidden_sent(&dispatch).is_empty());
    // A screen that is closed is not told.
    screen.opened = false;
    told_hidden(&kernel, &mut screen, 1);
    assert_eq!(hidden_sent(&dispatch).len(), 0);
}
