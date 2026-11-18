//! How much room a screen has: its class by width ([`ScreenClass`]), for
//! layouts that adapt to phones, tablets and a device on its side.

use crate::Place;
use paperpaw_sdk::Rect;
use paperpaw_sdk::builtin::settings;
pub use paperpaw_sdk::ui::ScreenClass;

/// A part of the screen narrower than this many millimetres (at the
/// reader's own size of text, see `ScreenClass::of`) is
/// [`ScreenClass::Compact`].
pub const COMPACT_BELOW_MM: u32 = 100;

/// A part of the screen at least this many millimetres wide (at the
/// reader's own size of text) is [`ScreenClass::Expanded`].
pub const EXPANDED_FROM_MM: u32 = 140;

/// The reader's size of interface text and its default (both in points,
/// at least 1): how much larger than PaperPaw's measures it is set. The
/// same where no size is declared (nothing to scale by).
/// The class of a screen `area` wide on this device, at the interface
/// size chosen (a larger size makes a screen read as narrower).
pub fn class_of(env: &Place<'_>, area: Rect) -> ScreenClass {
    let tenths = u64::from(area.width) * 254 / u64::from(env.metrics().dpi.max(1));
    let (size, default) = ui_sizes(env);
    let tenths = tenths * default / size;
    if tenths < u64::from(COMPACT_BELOW_MM) * 10 {
        ScreenClass::Compact
    } else if tenths < u64::from(EXPANDED_FROM_MM) * 10 {
        ScreenClass::Medium
    } else {
        ScreenClass::Expanded
    }
}

fn ui_sizes(env: &Place<'_>) -> (u64, u64) {
    let chosen = env.settings();
    let Some(paperpaw_sdk::SettingValue::Int(default)) = chosen
        .spec(settings::UI_SIZE.as_str())
        .map(|spec| &spec.default)
    else {
        return (1, 1);
    };
    let points = |value: i64| value.clamp(1, 100) as u64;
    (points(chosen.read(&settings::UI_SIZE)), points(*default))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kernel;
    use paperpaw_platform::{
        DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
    };
    use paperpaw_sdk::SettingValue;
    use paperpaw_sdk::event::ParamValue;

    /// A kernel whose interface text is `size` points, its default
    /// `default` (none declared where `None`), on a 400 by 400 screen at
    /// `dpi` dots per inch (254: 10 a millimetre).
    fn kernel_at(default: Option<i64>, size: i64, dpi: u16) -> Kernel {
        let device = DeviceInfo::new(
            "test",
            "Test",
            DisplayInfo::new(400, 400, dpi, PixelFormat::Gray4, Rotation::Deg0, None),
            InputCaps::new(true, true, false, &[]),
            MemoryClass::Small,
        );
        let builder = Kernel::builder(device);
        let Some(default) = default else {
            return builder.build().unwrap();
        };
        let mut kernel = builder
            .setting_default(&settings::UI_SIZE, default)
            .build()
            .unwrap();
        kernel
            .shared
            .settings
            .set(settings::UI_SIZE.static_str(), SettingValue::Int(size))
            .unwrap();
        kernel
    }

    fn kit(default: Option<i64>, size: i64) -> Kernel {
        kernel_at(default, size, 254)
    }

    fn class(kernel: &Kernel, width_mm: u16) -> ScreenClass {
        class_of(
            &kernel.shared.place(Instant(0)),
            Rect::new(0, 0, width_mm * 10, 100),
        )
    }

    #[test]
    fn classes_by_width_at_the_default_size() {
        let kit = kit(Some(9), 9);
        assert_eq!(class(&kit, 55), ScreenClass::Compact);
        assert_eq!(class(&kit, 92), ScreenClass::Compact);
        assert_eq!(class(&kit, 99), ScreenClass::Compact);
        assert_eq!(class(&kit, 100), ScreenClass::Medium);
        assert_eq!(class(&kit, 139), ScreenClass::Medium);
        assert_eq!(class(&kit, 140), ScreenClass::Expanded);
        assert_eq!(class(&kit, 232), ScreenClass::Expanded);
        assert!(ScreenClass::Compact < ScreenClass::Medium);
        assert!(ScreenClass::Medium < ScreenClass::Expanded);
    }

    /// Larger text makes a screen narrower, smaller text wider; the
    /// default the device gives (a shell's) is what it is measured
    /// against, so a device at its own default is classed by its width.
    #[test]
    fn larger_text_makes_a_screen_narrower() {
        assert_eq!(class(&kit(Some(9), 18), 232), ScreenClass::Medium);
        assert_eq!(class(&kit(Some(9), 18), 145), ScreenClass::Compact);
        assert_eq!(class(&kit(Some(9), 7), 120), ScreenClass::Expanded);
        assert_eq!(class(&kit(Some(12), 12), 145), ScreenClass::Expanded);
        // Nothing declared: by the width alone.
        assert_eq!(class(&kit(None, 0), 145), ScreenClass::Expanded);
        assert_eq!(class(&kit(None, 0), 92), ScreenClass::Compact);
    }

    /// The profiles of the design: the X4 is compact in both orientations.
    #[test]
    fn profiles() {
        let of = |width: u16, dpi: u16| {
            let kernel = kernel_at(Some(9), 9, dpi);
            class_of(
                &kernel.shared.place(Instant(0)),
                Rect::new(0, 0, width, 100),
            )
        };
        assert_eq!(of(480, 220), ScreenClass::Compact);
        assert_eq!(of(800, 220), ScreenClass::Compact);
        assert_eq!(of(792, 260), ScreenClass::Compact);
        assert_eq!(of(800, 167), ScreenClass::Medium);
        assert_eq!(of(1072, 300), ScreenClass::Compact);
        assert_eq!(of(1448, 300), ScreenClass::Medium);
        assert_eq!(of(1080, 420), ScreenClass::Compact);
        assert_eq!(of(2400, 420), ScreenClass::Expanded);
        assert_eq!(of(1600, 280), ScreenClass::Expanded);
    }

    #[test]
    fn env_says_its_parts_class_and_whether_it_is_on_its_side() {
        let kernel = kit(Some(9), 9);
        let env = kernel.shared.place(Instant(0));
        // 400 by 400 at 10 a millimetre: 40 mm.
        assert_eq!(env.screen_class(), ScreenClass::Compact);
        assert!(!env.landscape());
        let device = kernel.shared.device;
        let wide = env.within(&device, Rect::new(0, 0, 1500, 900));
        assert_eq!(wide.screen_class(), ScreenClass::Expanded);
        assert!(wide.landscape());
    }

    #[test]
    fn classes_go_by_their_names_in_events() {
        for class in [
            ScreenClass::Compact,
            ScreenClass::Medium,
            ScreenClass::Expanded,
        ] {
            assert_eq!(ScreenClass::from_param(&class.to_param()), Some(class));
        }
        // One of a later kernel: the widest known.
        assert_eq!(ScreenClass::from_param("huge"), Some(ScreenClass::Expanded));
        assert_eq!(ScreenClass::from_param(""), None);
    }
}
