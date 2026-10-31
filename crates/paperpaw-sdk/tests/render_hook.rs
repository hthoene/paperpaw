//! A render hook is accepted by the registrar in the plugin's namespace
//! and once per id, the capabilities that tell whether the kernel runs
//! hooks are off in a device that is not told, and a hook gets only the
//! narrow context.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::Cell;
use std::rc::Rc;

use paperpaw_sdk::builtin::settings;
use paperpaw_sdk::render::{BandView, FramePass, HookCx, HookStage, RenderHookSpec};
use paperpaw_sdk::{
    Capabilities, Capability, Contribution, Id, Lang, PixelFormat, Rect, RegisterError, Registrar,
    Settings,
};

fn spec(
    id: &'static str,
    hook: impl Fn(&mut BandView<'_>, &HookCx<'_>) + 'static,
) -> RenderHookSpec {
    RenderHookSpec::new(
        Id::new_static(id),
        "demo.hook.title",
        HookStage::AFTER_BAND,
        Id::new_static("demo.hook-on"),
        Box::new(hook),
    )
}

#[test]
fn a_hook_is_accepted_in_the_plugin_s_namespace_and_once_per_id() {
    let mut r = Registrar::new(Id::new_static("demo"));
    r.add_render_hook(spec("demo.night", |_, _| {})).unwrap();
    assert!(matches!(
        r.add_render_hook(spec("demo.night", |_, _| {})),
        Err(RegisterError::Duplicate { .. })
    ));
    assert!(matches!(
        r.add_render_hook(spec("other.night", |_, _| {})),
        Err(RegisterError::Namespace { .. })
    ));
    let kinds: Vec<&str> = r.contributions().iter().map(Contribution::kind).collect();
    assert_eq!(kinds, ["render-hook"]);
}

#[test]
fn the_capabilities_are_off_until_a_device_says_so() {
    let caps = Capabilities::default();
    for capability in [
        Capability::RENDER_HOOK,
        Capability::RENDER_REGIONS,
        Capability::LOOK_EXTRAS,
    ] {
        assert!(!caps.has(capability));
    }
    // And they are three bits of their own.
    assert_ne!(Capability::RENDER_HOOK, Capability::RENDER_REGIONS);
    assert_ne!(Capability::RENDER_REGIONS, Capability::LOOK_EXTRAS);
}

#[test]
fn a_hook_changes_its_band_and_reads_settings_and_the_language_only() {
    let mut settings = Settings::new();
    let seen = Rc::new(Cell::new((0u16, false)));
    let log = Rc::clone(&seen);
    let spec = spec("demo.night", move |band, cx| {
        let curve = cx.setting(&settings::TEXT_CURVE).len();
        log.set((cx.revision() + curve as u16, cx.script() == Some("Arab")));
        band.map_gray(|g| 255 - g);
    })
    .with_passes(FramePass::GRAY)
    .with_order(-3)
    .with_overlap(9)
    .with_carry(1900);
    assert_eq!((spec.order, spec.overlap, spec.carry), (-3, 4, 1900));
    assert!(spec.passes.contains(FramePass::GRAY) && !spec.passes.contains(FramePass::MONO));
    let mut rows = [0u8; 8];
    let mut band = BandView::new(
        &mut rows,
        Rect::new(0, 0, 16, 2),
        PixelFormat::Gray2,
        FramePass::GRAY,
    );
    let cx = HookCx::new(&settings, Lang::parse("fa-Arab"), 0);
    (spec.hook)(&mut band, &cx);
    assert_eq!(seen.get(), (0, true));
    assert_eq!(rows, [0xff; 8]);
    let _ = &mut settings;
}
