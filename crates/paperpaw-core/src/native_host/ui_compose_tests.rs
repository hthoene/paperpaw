//! Slot 35: what the host checks before the target is looked up, and that the
//! hook's answer, a child that fails, and the caller's clip and canvas come
//! out as `docs/abi/ui.md` section 5.1 says.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate std;
use super::*;
use crate::abi7::context::Frame;
use crate::native_host::HostResources;
use crate::native_host::shell::DeclaredServices;
use crate::native_host::shell::{Context, Hosted};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::pin::Pin;
use core::ptr;
use paperpaw_graphics::Framebuffer;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED, PpHost,
    PpPlugin, Services,
};
use paperpaw_platform::{Color, Instant, PixelFormat};
use paperpaw_sdk::{Env, Settings};

struct Heap;
impl Services for Heap {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        // SAFETY: the layout is valid and nonzero (checked by the caller).
        core::alloc::Layout::from_size_align(size, align)
            .map_or(ptr::null_mut(), |l| unsafe { std::alloc::alloc(l) })
    }
    unsafe fn dealloc(&self, ptr: *mut u8, size: usize, align: usize) {
        // SAFETY: the trait's contract: this allocator's block, freed once.
        unsafe {
            std::alloc::dealloc(
                ptr,
                core::alloc::Layout::from_size_align(size, align).unwrap(),
            );
        }
    }
    unsafe fn realloc(&self, ptr: *mut u8, old: usize, align: usize, new: usize) -> *mut u8 {
        // SAFETY: as `dealloc`.
        unsafe {
            std::alloc::realloc(
                ptr,
                core::alloc::Layout::from_size_align(old, align).unwrap(),
                new,
            )
        }
    }
    fn micros(&self) -> u64 {
        0
    }
}

fn report(_: &Heap, _: &str, _: crate::native_host::TeardownStep, _: crate::native_host::Error) {}

unsafe extern "C" fn never_made(_: *const PpHost, _: *mut c_void, _: *mut *const PpPlugin) -> i32 {
    ERR_STATE
}

fn hosted() -> Hosted<Heap, ()> {
    // SAFETY: `never_made` is never called; no image.
    unsafe {
        Hosted::new(
            Heap,
            (),
            never_made,
            "caller",
            (ptr::null(), 0),
            report,
            HostResources::try_new().unwrap(),
            DeclaredServices::none(),
        )
    }
    .unwrap()
}

/// What the hook does when it is called.
#[derive(Clone, Copy)]
enum Act {
    /// Returns this.
    Answer(Result<(), i32>),
    /// Fills the whole canvas it was given, then returns `Ok`.
    FillAll,
    /// Fills it, sets its own clip, then fails.
    FillThenFail,
}

struct Hook<'a> {
    env: &'a Env<'a>,
    act: Cell<Act>,
    seen: RefCell<Vec<(ComposeRequest, Rect, alloc::string::String)>>,
}
impl Frame for Hook<'_> {
    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn translator(&self) -> &paperpaw_i18n::Translator {
        self.env.i18n()
    }
    fn compose(
        &self,
        caller: &str,
        request: &ComposeRequest,
        canvas: &mut Canvas<'_>,
    ) -> Result<(), i32> {
        self.seen
            .borrow_mut()
            .push((*request, canvas.clip(), caller.into()));
        match self.act.get() {
            Act::Answer(answer) => answer,
            Act::FillAll => {
                canvas.fill_rect(0, 0, 4096, 4096, Color::BLACK);
                Ok(())
            }
            Act::FillThenFail => {
                canvas.fill_rect(0, 0, 4096, 4096, Color::BLACK);
                Err(ERR_NO_MEMORY)
            }
        }
    }
}

const W: u16 = 40;
const H: u16 = 30;

fn kernel_env() -> (crate::Kernel, Instant) {
    (
        crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap(),
        Instant(0),
    )
}

/// Runs `call` as inside the caller's `draw`: the hook installed as the frame
/// and a `W` x `H` canvas clipped to `clip`.
fn in_draw<R>(
    act: Act,
    clip: Rect,
    call: impl FnOnce(Pin<&Context<Heap>>, *mut c_void) -> R,
) -> (
    R,
    Vec<(ComposeRequest, Rect, alloc::string::String)>,
    Vec<u8>,
) {
    let (kernel, now) = kernel_env();
    let env = kernel.shared.env(now);
    let hook = Hook {
        env: &env,
        act: Cell::new(act),
        seen: RefCell::default(),
    };
    let hosted = hosted();
    let context = hosted.owner().context();
    let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast::<c_void>();
    let mut frame = Framebuffer::new(W, H, PixelFormat::Gray8).unwrap();
    let result = {
        let mut base = frame.canvas();
        let mut canvas = base.clipped(clip);
        // SAFETY: the owner structurally pins these fields.
        let environment = unsafe { context.map_unchecked(|c| &c.environment) };
        let scope = unsafe { context.map_unchecked(|c| &c.canvas) };
        environment
            .scope_frame(&hook, || {
                scope.scope(&mut canvas, || call(context, ctx)).unwrap()
            })
            .unwrap()
    };
    let pixels = frame.view().data.to_vec();
    hosted.owner().revoke();
    hosted.owner().finish().unwrap();
    (result, hook.seen.into_inner(), pixels)
}

fn whole() -> Rect {
    Rect::new(0, 0, W, H)
}

fn call(ctx: *mut c_void, kind: u32, handle: u32, x: i32, y: i32, w: u32, h: u32) -> i32 {
    // SAFETY: ctx is the pinned context of a live owner; scalars only.
    unsafe { compose::<Heap>(ctx, kind, handle, x, y, w, h) }
}

#[test]
fn a_null_context_and_an_unknown_kind_are_invalid_before_any_lookup() {
    // SAFETY: a null context is refused first.
    assert_eq!(
        unsafe { compose::<Heap>(ptr::null_mut(), 0, 1, 0, 0, 1, 1) },
        ERR_INVALID
    );
    for kind in [3, 4, 255, u32::MAX] {
        let (status, seen, _) = in_draw(Act::Answer(Ok(())), whole(), |_, ctx| {
            call(ctx, kind, 1, 0, 0, 10, 10)
        });
        assert_eq!(status, ERR_INVALID, "kind {kind}");
        assert!(seen.is_empty(), "kind {kind} reached the hook");
    }
}

#[test]
fn outside_the_callers_draw_it_is_a_state_error_without_a_lookup() {
    let (kernel, now) = kernel_env();
    let env = kernel.shared.env(now);
    let hook = Hook {
        env: &env,
        act: Cell::new(Act::Answer(Ok(()))),
        seen: RefCell::default(),
    };
    let hosted = hosted();
    let context = hosted.owner().context();
    let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast::<c_void>();
    // No dispatch at all.
    assert_eq!(call(ctx, 0, 1, 0, 0, 10, 10), ERR_STATE);
    // A frame but no canvas (an event or input callback).
    // SAFETY: the owner structurally pins this field.
    let environment = unsafe { context.map_unchecked(|c| &c.environment) };
    let status = environment
        .scope_frame(&hook, || call(ctx, 0, 1, 0, 0, 10, 10))
        .unwrap();
    assert_eq!(status, ERR_STATE);
    assert!(hook.seen.borrow().is_empty());
    hosted.owner().revoke();
    hosted.owner().finish().unwrap();
}

#[test]
fn a_canvas_without_a_frame_is_a_state_error() {
    let hosted = hosted();
    let context = hosted.owner().context();
    let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast::<c_void>();
    let mut frame = Framebuffer::new(W, H, PixelFormat::Gray8).unwrap();
    let mut canvas = frame.canvas();
    // SAFETY: the owner structurally pins this field.
    let scope = unsafe { context.map_unchecked(|c| &c.canvas) };
    let status = scope
        .scope(&mut canvas, || call(ctx, 1, 1, 0, 0, 10, 10))
        .unwrap();
    assert_eq!(status, ERR_STATE);
    hosted.owner().revoke();
    hosted.owner().finish().unwrap();
}

#[test]
fn the_default_hook_is_unsupported_which_is_the_x4_answer() {
    let (kernel, now) = kernel_env();
    let env = kernel.shared.env(now);
    let hosted = hosted();
    let context = hosted.owner().context();
    let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast::<c_void>();
    let mut frame = Framebuffer::new(W, H, PixelFormat::Gray8).unwrap();
    let mut canvas = frame.canvas();
    // SAFETY: the owner structurally pins these fields.
    let environment = unsafe { context.map_unchecked(|c| &c.environment) };
    let scope = unsafe { context.map_unchecked(|c| &c.canvas) };
    // An ordinary environment frame composes nothing.
    let status = environment
        .scope(&env, || {
            scope
                .scope(&mut canvas, || call(ctx, 0, 1, 0, 0, 10, 10))
                .unwrap()
        })
        .unwrap();
    assert_eq!(status, ERR_UNSUPPORTED);
    hosted.owner().revoke();
    hosted.owner().finish().unwrap();
}

#[test]
fn every_kind_maps_to_its_purpose_and_the_frame_carries_the_handle() {
    for (kind, purpose) in [(0, 4), (1, 3), (2, 5)] {
        let (status, seen, _) = in_draw(Act::Answer(Ok(())), whole(), |_, ctx| {
            call(ctx, kind, 77, 5, 6, 10, 8)
        });
        assert_eq!(status, OK);
        let (request, _, caller) = &seen[0];
        assert_eq!(caller, "caller");
        let frame = request.draw_frame().unwrap();
        assert_eq!(
            (
                frame.purpose,
                frame.id,
                frame.x,
                frame.y,
                frame.width,
                frame.height
            ),
            (purpose, 77, 5, 6, 10, 8)
        );
        assert_eq!((frame.clip_top, frame.clip_bottom), (6, 14));
        assert!(frame.payload.is_empty());
        // The frame the owner gets is one the encoder accepts.
        let mut bytes = [0; 320];
        crate::native_host::frames::encode(&frame, &mut bytes).unwrap();
    }
}

#[test]
fn the_hooks_refusals_reach_the_caller_unchanged() {
    for answer in [
        Err(ERR_NOT_FOUND),
        Err(ERR_DENIED),
        Err(ERR_BUSY),
        Err(ERR_NO_MEMORY),
        Err(ERR_UNSUPPORTED),
        Ok(()),
    ] {
        let (status, seen, pixels) = in_draw(Act::Answer(answer), whole(), |_, ctx| {
            call(ctx, 0, 9, 0, 0, 10, 10)
        });
        assert_eq!(status, answer.err().unwrap_or(OK));
        assert_eq!(seen.len(), 1);
        // A refusal draws nothing.
        assert!(pixels.iter().all(|p| *p == pixels[0]));
    }
}

#[test]
fn the_rectangle_is_cut_to_the_callers_clip_at_the_limit_and_one_beyond() {
    let clip = Rect::new(10, 5, 20, 10); // x 10..30, y 5..15
    // (x, y, w, h) -> visible (x, y, w, h)
    type Case = ((i32, i32, u32, u32), Option<(u16, u16, u16, u16)>);
    let cases: &[Case] = &[
        // Exactly the clip: the limit.
        ((10, 5, 20, 10), Some((10, 5, 20, 10))),
        // One column and one row beyond each side: cut to the clip.
        ((9, 4, 22, 12), Some((10, 5, 20, 10))),
        // One pixel inside on each side.
        ((11, 6, 18, 8), Some((11, 6, 18, 8))),
        // Touching from outside: nothing visible.
        ((30, 5, 5, 10), None),
        ((10, 15, 20, 5), None),
        ((0, 5, 10, 10), None),
        ((10, 0, 20, 5), None),
        // One pixel overlapping.
        ((29, 14, 5, 5), Some((29, 14, 1, 1))),
        // Empty rectangles and the extremes never overflow.
        ((12, 7, 0, 5), None),
        ((12, 7, 5, 0), None),
        (
            (i32::MIN, i32::MIN, u32::MAX, u32::MAX),
            Some((10, 5, 20, 10)),
        ),
        ((i32::MAX, i32::MAX, u32::MAX, u32::MAX), None),
        ((i32::MIN, 5, u32::MAX, u32::MAX), Some((10, 5, 20, 10))),
        ((12, 7, u32::MAX, u32::MAX), Some((12, 7, 18, 8))),
    ];
    for &((x, y, w, h), want) in cases {
        let (status, seen, _) = in_draw(Act::Answer(Ok(())), clip, |_, ctx| {
            call(ctx, 0, 1, x, y, w, h)
        });
        assert_eq!(status, OK, "{x} {y} {w} {h}");
        // The hook is asked in every case: its refusals come before the
        // emptiness check.
        assert_eq!(seen.len(), 1, "{x} {y} {w} {h}");
        let (request, child_clip, _) = &seen[0];
        let want = want.map(|(x, y, w, h)| Rect::new(x, y, w, h));
        assert_eq!(request.visible, want, "{x} {y} {w} {h}");
        // The canvas the child gets shows exactly the visible part.
        assert_eq!(*child_clip, want.unwrap_or_default(), "{x} {y} {w} {h}");
        // The rectangle given is passed unchanged.
        assert_eq!(
            (request.x, request.y, request.width, request.height),
            (x, y, w, h)
        );
        match request.draw_frame() {
            None => assert!(want.is_none()),
            Some(frame) => {
                let visible = want.unwrap();
                assert_eq!(
                    (frame.clip_top, frame.clip_bottom),
                    (
                        i32::from(visible.y),
                        i32::from(visible.y) + i32::from(visible.height)
                    )
                );
                let mut bytes = [0; 320];
                crate::native_host::frames::encode(&frame, &mut bytes)
                    .unwrap_or_else(|e| panic!("{x} {y} {w} {h}: {e}"));
            }
        }
    }
}

#[test]
fn a_clip_the_caller_set_with_slot_9_limits_what_the_child_sees() {
    let (status, seen, _) = in_draw(Act::Answer(Ok(())), whole(), |context, ctx| {
        assert_eq!(context.canvas.clip(5, 5, 10, 10), OK);
        call(ctx, 0, 1, 0, 0, 40, 30)
    });
    assert_eq!(status, OK);
    assert_eq!(seen[0].0.visible, Some(Rect::new(5, 5, 10, 10)));
}

#[test]
fn the_child_draws_only_inside_the_visible_part() {
    let clip = Rect::new(4, 4, 30, 20);
    let (status, _, pixels) = in_draw(Act::FillAll, clip, |_, ctx| call(ctx, 0, 1, 10, 8, 12, 6));
    assert_eq!(status, OK);
    let mut expected = Framebuffer::new(W, H, PixelFormat::Gray8).unwrap();
    expected
        .canvas()
        .clipped(Rect::new(10, 8, 12, 6))
        .fill_rect(0, 0, 4096, 4096, Color::BLACK);
    assert_eq!(pixels, expected.view().data);
}

#[test]
fn a_failed_child_leaves_the_callers_frame_and_clip_as_they_were() {
    let (status, _, pixels) = in_draw(Act::FillThenFail, whole(), |context, ctx| {
        assert_eq!(context.canvas.clip(2, 2, 30, 20), OK);
        let status = call(ctx, 1, 1, 10, 8, 12, 6);
        // The caller goes on in its own clip, not the child's.
        assert_eq!(context.canvas.fill_rect(2, 2, 1, 1, Color::BLACK), OK);
        assert_eq!(context.canvas.fill_rect(0, 0, 1, 1, Color::BLACK), OK);
        status
    });
    // The child's status, and nothing more, comes back.
    assert_eq!(status, ERR_NO_MEMORY);
    let mut expected = Framebuffer::new(W, H, PixelFormat::Gray8).unwrap();
    {
        let mut base = expected.canvas();
        base.clipped(Rect::new(10, 8, 12, 6))
            .fill_rect(0, 0, 4096, 4096, Color::BLACK);
        // (2, 2) is inside the caller's clip, (0, 0) is outside it.
        base.clipped(Rect::new(2, 2, 30, 20))
            .fill_rect(2, 2, 1, 1, Color::BLACK);
    }
    assert_eq!(pixels, expected.view().data);
}

#[test]
fn a_second_compose_from_inside_the_child_cannot_reach_the_callers_canvas() {
    // While the hook runs, the caller's canvas is lent to it: slot 35 or a
    // drawing slot of the caller reached from there finds the canvas busy.
    let (kernel, now) = kernel_env();
    let env = kernel.shared.env(now);
    let hosted = hosted();
    let context = hosted.owner().context();
    let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast::<c_void>();
    struct Reentrant<'a> {
        env: &'a Env<'a>,
        ctx: *mut c_void,
        context: Pin<&'a Context<Heap>>,
        inner: Cell<Option<(i32, i32)>>,
    }
    impl Frame for Reentrant<'_> {
        fn settings(&self) -> &Settings {
            self.env.settings()
        }
        fn translator(&self) -> &paperpaw_i18n::Translator {
            self.env.i18n()
        }
        fn compose(&self, _: &str, _: &ComposeRequest, _: &mut Canvas<'_>) -> Result<(), i32> {
            self.inner.set(Some((
                call(self.ctx, 0, 1, 0, 0, 1, 1),
                self.context.canvas.fill_rect(0, 0, 1, 1, Color::BLACK),
            )));
            Ok(())
        }
    }
    let frame_hook = Reentrant {
        env: &env,
        ctx,
        context,
        inner: Cell::new(None),
    };
    let mut frame = Framebuffer::new(W, H, PixelFormat::Gray8).unwrap();
    let mut canvas = frame.canvas();
    // SAFETY: the owner structurally pins these fields.
    let environment = unsafe { context.map_unchecked(|c| &c.environment) };
    let scope = unsafe { context.map_unchecked(|c| &c.canvas) };
    let status = environment
        .scope_frame(&frame_hook, || {
            scope
                .scope(&mut canvas, || call(ctx, 0, 1, 0, 0, 10, 10))
                .unwrap()
        })
        .unwrap();
    assert_eq!(status, OK);
    assert_eq!(frame_hook.inner.get(), Some((ERR_BUSY, ERR_BUSY)));
    hosted.owner().revoke();
    hosted.owner().finish().unwrap();
}

#[test]
fn the_new_purpose_is_on_the_wire() {
    assert_eq!(Purpose::StatusLine.wire(), 5);
    assert_eq!(ComposeKind::from_wire(2), Some(ComposeKind::StatusLine));
    assert_eq!(ComposeKind::from_wire(3), None);
}
