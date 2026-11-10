use super::*;

fn smooth() -> Look {
    Look::from_values(0, true, None, None, Some(1), Some(1))
}

#[test]
fn the_normal_look_is_the_plain_curve_at_no_weight() {
    let look = smooth();
    assert_eq!(look, Look::default());
    assert_eq!(
        look.ink(40.0),
        Ink::new().extra(0).tone(ToneCurve::SMOOTH.alphas())
    );
}

#[test]
fn weight_widens_by_a_pixel_a_step_at_48_pixels_and_at_least_one() {
    let extra =
        |w: i64, size: f32| Look::from_values(w, true, None, None, Some(1), Some(1)).extra(size);
    assert_eq!(extra(0, 48.0), 0);
    assert_eq!(extra(1, 48.0), 1);
    assert_eq!(extra(4, 48.0), 4);
    // Small text still thickens by one for any weight.
    assert_eq!(extra(1, 10.0), 1);
    // Out of range is the nearest.
    assert_eq!(extra(99, 48.0), 4);
    assert_eq!(extra(-3, 48.0), 0);
}

#[test]
fn contrast_and_sharpness_pick_the_curve_as_in_beta_3() {
    let curve = |contrast, sharp| Look::from_values(0, true, None, None, contrast, sharp).tone;
    assert_eq!(curve(Some(1), Some(1)), ToneCurve::contrast(0).alphas());
    assert_eq!(curve(Some(3), Some(1)), ToneCurve::contrast(2).alphas());
    // Soft is a step lighter, sharp a step darker; unknown is as normal.
    assert_eq!(curve(Some(1), Some(0)), ToneCurve::contrast(-1).alphas());
    assert_eq!(curve(Some(1), Some(2)), ToneCurve::contrast(1).alphas());
    assert_eq!(curve(None, None), ToneCurve::contrast(0).alphas());
    // Past the steps is the nearest.
    assert_eq!(curve(Some(3), Some(2)), ToneCurve::contrast(2).alphas());
}

#[test]
fn sharp_comes_from_sharpness_or_smooth_text_off_except_on_black_and_white() {
    let sharp = |smooth, grays, sharpness| {
        Look::from_values(0, smooth, grays, None, Some(1), sharpness).sharp
    };
    assert!(sharp(true, None, Some(2)));
    assert!(!sharp(true, None, Some(1)));
    assert!(sharp(false, None, Some(1)));
    assert!(sharp(false, Some(16), Some(1)));
    // A panel of black and white draws the same letters already.
    assert!(!sharp(false, Some(2), Some(1)));
    assert!(sharp(false, Some(2), Some(2)));
}

#[test]
fn a_curve_of_its_own_replaces_contrast_and_sharpness() {
    let own = ToneCurve::from_alphas([9; 16]);
    let look = Look::from_values(2, true, None, Some(own), Some(3), Some(2));
    assert_eq!(look.tone, own.alphas());
    assert!(!look.sharp);
    assert!(Look::from_values(0, false, None, Some(own), None, None).sharp);
}

#[test]
fn a_curve_too_long_to_read_is_no_curve_and_does_not_refuse_the_page() {
    assert_eq!(Look::own_curve(Err(Error::NoSpace)), Ok(None));
    assert_eq!(Look::own_curve(Ok(SettingValue::Absent)), Ok(None));
    assert_eq!(Look::own_curve(Err(Error::State)), Err(Error::State));
    assert_eq!(
        Look::own_curve(Ok(SettingValue::Text("nonsense"))),
        Ok(None)
    );
}
