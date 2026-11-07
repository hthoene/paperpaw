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
