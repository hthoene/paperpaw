//! Consumer checks against generated copy-out fixtures, not a live Env bridge.
use super::*;
use crate::{
    observations::Setting,
    snapshot::{Battery, PixelFormat},
};

fn draw_once(p: *const PpPlugin) -> i32 {
    // SAFETY: tests hold a live sample instance and invoke synchronously.
    unsafe { ((*p).draw.unwrap())((*p).state, DRAW_TOKEN, frame()) }
}
fn observed(p: *const PpPlugin) -> Option<observations::Observations> {
    // SAFETY: each test retains exclusive ownership between synchronous callbacks.
    unsafe { (*(*p).state.cast::<State>()).observations }
}

#[test]
fn absent_values_and_present_zero_survive_snapshot_consumption() {
    let table = table();
    let mut ctx = Host::new(0);
    let p = create(&table, &mut ctx);
    assert_eq!(draw_once(p), OK);
    let missing = observed(p).unwrap();
    assert_eq!(missing.snapshot.battery, None);
    assert_eq!(missing.snapshot.unix_seconds, None);
    assert_eq!(missing.setting, Setting::Absent);
    ctx.snapshot.battery = Some(Battery::new(0));
    ctx.snapshot.unix_seconds = Some(0);
    ctx.setting = settings::SettingValueRef::Bool(false);
    assert_eq!(draw_once(p), OK);
    let zero = observed(p).unwrap();
    assert_eq!(zero.snapshot.battery.unwrap().percent, 0);
    assert_eq!(zero.snapshot.battery.unwrap().millivolts, None);
    assert_eq!(zero.snapshot.unix_seconds, Some(0));
    assert_eq!(zero.setting, Setting::Bool(false));
    let battery = ctx.snapshot.battery.as_mut().unwrap();
    battery.millivolts = Some(0);
    battery.charging = true;
    battery.saver = true;
    ctx.snapshot.unix_seconds = Some(-1);
    ctx.snapshot.monotonic_ms = u64::MAX;
    assert_eq!(draw_once(p), OK);
    assert_eq!(observed(p).unwrap().snapshot, ctx.snapshot);
    destroy_plugin(p);
}

#[test]
fn every_pixel_format_and_exact_display_inputs_survive_copy_out() {
    let table = table();
    let mut ctx = Host::new(1);
    let p = create(&table, &mut ctx);
    for (pixel, gray) in [
        (PixelFormat::Mono1, Some(2)),
        (PixelFormat::Gray2, Some(4)),
        (PixelFormat::Gray4, Some(16)),
        (PixelFormat::Gray8, Some(256)),
        (PixelFormat::Rgb565, None),
        (PixelFormat::Rgb888, None),
        (PixelFormat::Other { code: 0, bits: 0 }, None),
        (
            PixelFormat::Other {
                code: u16::MAX,
                bits: u8::MAX,
            },
            None,
        ),
    ] {
        ctx.snapshot.pixel = pixel;
        ctx.snapshot.width = u16::MAX;
        ctx.snapshot.height = 0;
        ctx.snapshot.dpi = 655;
        ctx.snapshot.touch = true;
        assert_eq!(draw_once(p), OK);
        let got = observed(p).unwrap().snapshot;
        assert_eq!(got, ctx.snapshot);
        assert_eq!(got.pixel.gray_levels(), gray);
        assert_eq!(got.pixel.bits_per_pixel(), pixel.bits_per_pixel());
    }
    destroy_plugin(p);
}

#[test]
fn settings_types_and_owned_utf8_are_preserved_without_coercion() {
    let table = table();
    let mut ctx = Host::new(1);
    let p = create(&table, &mut ctx);
    for (input, expected) in [
        (settings::SettingValueRef::Absent, Setting::Absent),
        (settings::SettingValueRef::Bool(true), Setting::Bool(true)),
        (settings::SettingValueRef::Bool(false), Setting::Bool(false)),
        (settings::SettingValueRef::Int(0), Setting::Int(0)),
        (
            settings::SettingValueRef::Int(i64::MIN),
            Setting::Int(i64::MIN),
        ),
    ] {
        ctx.setting = input;
        assert_eq!(draw_once(p), OK);
        assert_eq!(observed(p).unwrap().setting, expected);
    }
    for text in [
        "",
        "Akku · אב",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        ctx.setting = settings::SettingValueRef::Text(text);
        assert_eq!(draw_once(p), OK);
        let Setting::Text { bytes, len } = observed(p).unwrap().setting else {
            panic!("text type lost");
        };
        assert_eq!(core::str::from_utf8(&bytes[..len]).unwrap(), text);
    }
    destroy_plugin(p);
}

#[test]
fn partial_copy_and_refusals_do_not_publish_new_observations() {
    let table = table();
    let mut ctx = Host::new(1);
    let p = create(&table, &mut ctx);
    assert_eq!(draw_once(p), OK);
    let baseline = observed(p);
    ctx.snapshot.monotonic_ms = 42;
    ctx.setting =
        settings::SettingValueRef::Text("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(draw_once(p), ERR_NO_SPACE);
    assert_eq!(observed(p), baseline);
    ctx.setting = settings::SettingValueRef::Bool(true);
    for (reply, expected) in [
        (Some(12), ERR_INVALID),
        (Some(ERR_STATE), ERR_STATE),
        (Some(ERR_IO), ERR_IO),
        (Some(i32::MAX), ERR_NO_SPACE),
    ] {
        ctx.setting_reply = reply;
        assert_eq!(draw_once(p), expected);
        assert_eq!(observed(p), baseline);
    }
    ctx.setting_reply = None;
    for error in [ERR_STATE, ERR_NO_SPACE, ERR_IO] {
        ctx.snapshot_error = error;
        assert_eq!(draw_once(p), error);
        assert_eq!(observed(p), baseline);
    }
    // SAFETY: still a live, synchronously owned instance.
    assert_eq!(unsafe { (*(*p).state.cast::<State>()).draws }, 1);
    destroy_plugin(p);
}
