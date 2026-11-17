//! Sliders, equally easy by touch and with buttons: values keep to their
//! range and steps however they are set (a tap, a drag, the buttons),
//! a tap where the knob is drawn chooses the value it shows, the parts
//! keep to the margins and nothing is drawn outside the row. Broken
//! ranges (as a broken plugin may give) do not bring it down.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::ui::slider::Slider;
use paperpaw_sdk::ui::slider::SliderDrag;
use paperpaw_sdk::ui::slider::SliderParts;
use paperpaw_sdk::ui::slider::SliderStep;
use paperpaw_sdk::ui::slider::{self, key_step, parts};
use paperpaw_sdk::{Color, PointerPhase, Rect, SettingSpec};
use proptest::prelude::*;

use support::{Device, Harness, drawn_outside, frame, pointer};

/// Sizes for tests: no fonts needed.
fn style() -> UiStyle {
    UiStyle::PLAIN.with_corner(6)
}

fn slider_strategy() -> impl Strategy<Value = Slider> {
    (-1000i64..1000, 0i64..2000, 1i64..50)
        .prop_map(|(min, range, step)| Slider::new(min, min + range).step(step))
}

/// Whether `value` is one a slider offers: on a step from its least, or
/// its greatest.
fn offered(slider: &Slider, value: i64) -> bool {
    (slider.min()..=slider.max()).contains(&value)
        && (value == slider.max() || (value - slider.min()) % slider.step_size() == 0)
}

/// The size of the knob on `track`, as [`Slider::draw_track`] draws it:
/// three quarters of the track's height.
fn knob_size(track: Rect) -> u16 {
    (track.height * 3 / 4).max(4).min(track.width.max(1))
}

/// Whether `slider` is a range of whole steps and `track` has at least
/// two pixels for each, so that every value the slider offers has a
/// place of its own.
fn coarse(slider: &Slider, track: Rect) -> bool {
    let room = i64::from(track.width.saturating_sub(knob_size(track)));
    let range = slider.max() - slider.min();
    range % slider.step_size() == 0 && room >= 2 * (range / slider.step_size())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// A finger anywhere, on the track or past its ends, chooses a value
    /// the slider offers; further right never less.
    #[test]
    fn a_finger_anywhere_chooses_an_offered_value(
        slider in slider_strategy(),
        x in 0u16..400,
        width in 1u16..500,
        height in 4u16..80,
        xs in proptest::collection::vec(-100i32..700, 2..20),
    ) {
        let track = Rect::new(x, 10, width, height);
        let mut xs = xs;
        xs.sort_unstable();
        let mut last = i64::MIN;
        for x in xs {
            let value = slider.value_at(track, x);
            prop_assert!(offered(&slider, value), "{value} of {slider:?}");
            prop_assert!(value >= last, "{value} after {last}");
            last = value;
        }
    }

    /// A tap where the knob of a value is drawn chooses that value, on a
    /// track with two pixels or more for each step (of a range of whole
    /// steps): what the eye sees is what the finger gets.
    #[test]
    fn a_tap_on_the_knob_chooses_the_value_it_shows(
        min in -1000i64..1000,
        steps in 0i64..300,
        step in 1i64..50,
        n in 0i64..2000,
        height in 8u16..60,
    ) {
        let slider = Slider::new(min, min + steps * step).step(step);
        let knob = (height * 3 / 4).max(4);
        let width = (steps as u16).saturating_mul(2).max(50).saturating_add(knob);
        let track = Rect::new(30, 0, width, height);
        prop_assert!(coarse(&slider, track));
        let value = slider.stepped(slider.min(), n % (steps + 1));
        let x = slider.knob_x(track, value);
        prop_assert_eq!(slider.value_at(track, x), value, "at {}", x);
    }

    /// A finger taking the knob along: the knob moves as far as the
    /// finger, within the range and on the steps, also off the band's
    /// height; brought back to where it came down, the knob is back at
    /// its value, and lifted there, that value is kept, for a value it
    /// offers. (On tracks with less than two pixels a step, or ranges
    /// not whole steps, see `a_drag_brought_back_keeps_the_value`.)
    #[test]
    fn a_drag_moves_the_knob_as_far_as_the_finger(
        slider in slider_strategy(),
        value in -3000i64..3000,
        xs in proptest::collection::vec(0i32..400, 1..12),
    ) {
        let band = Rect::new(20, 40, 360, 30);
        let track = parts(&style(), band).track;
        // A value the slider offers (one off its steps comes back on
        // them).
        let value = slider.min() + (slider.clamp(value) - slider.min()) / slider.step_size() * slider.step_size();
        // Where the knob may be: its middle from one end of the track to
        // the other, and as near a finger's place as half a step.
        let (left, right) = (slider.knob_x(track, slider.min()), slider.knob_x(track, slider.max()));
        let half_step = (right - left) as f32 * slider.step_size() as f32
            / (slider.max() - slider.min()).max(1) as f32
            / 2.0;
        let mut drag = SliderDrag::default();
        // Down anywhere in the band: taken, nothing to draw yet.
        let first = xs[0].clamp(20, 379);
        let down = drag.on_pointer(&pointer(PointerPhase::Down, first, 50), &style(), &slider, band, value);
        prop_assert_eq!(down, Some(SliderStep::Same));
        prop_assert_eq!(drag.dragging(), Some(value));
        let mut moved = false;
        for &x in &xs[1..] {
            let step = drag.on_pointer(&pointer(PointerPhase::Move, x, 300), &style(), &slider, band, value);
            match step {
                Some(SliderStep::Moved(shown)) => {
                    moved = true;
                    prop_assert!(offered(&slider, shown), "{} of {:?}", shown, slider);
                    prop_assert_eq!(drag.dragging(), Some(shown));
                    let finger = (slider.knob_x(track, value) + x - first).clamp(left, right);
                    let off = (slider.knob_x(track, shown) - finger).abs() as f32;
                    // Where the range is not whole steps, its last step
                    // may be longer or shorter than the others.
                    let last = (right - slider.knob_x(track, slider.stepped(slider.max(), -1))) as f32;
                    prop_assert!(
                        off <= half_step.max(last / 2.0) + 1.0,
                        "knob {} px from the finger", off
                    );
                }
                Some(SliderStep::Same) => {}
                other => prop_assert!(false, "{:?} for a move", other),
            }
            let shown = drag.dragging().unwrap();
            prop_assert!(offered(&slider, shown) || shown == value);
        }
        if moved && coarse(&slider, track) {
            drag.on_pointer(&pointer(PointerPhase::Move, first, 300), &style(), &slider, band, value);
            prop_assert_eq!(drag.dragging(), Some(value));
            let up = drag.on_pointer(&pointer(PointerPhase::Up, first, 300), &style(), &slider, band, value);
            prop_assert_eq!(up, Some(SliderStep::Chosen(value)));
            prop_assert_eq!(drag.dragging(), None);
        }
    }

    /// The buttons move a value a step at a time, never out of the range;
    /// as many presses one way and back return to the value where the
    /// range does not end between. From an end that is no whole step, the
    /// first step down goes to the last whole step, as a drag gives.
    #[test]
    fn the_buttons_step_within_the_range(
        slider in slider_strategy(),
        start in -1500i64..3500,
        presses in proptest::collection::vec(any::<bool>(), 0..40),
    ) {
        let mut value = slider.clamp(start);
        for up in presses {
            let action = if up { actions::RIGHT } else { actions::LEFT };
            let next = slider.stepped(value, key_step(&action).unwrap());
            prop_assert!((slider.min()..=slider.max()).contains(&next));
            let past = (slider.max() - slider.min()) % slider.step_size();
            let expected = if !up && value == slider.max() && past != 0 {
                slider.max() - past
            } else {
                (value + if up { slider.step_size() } else { -slider.step_size() })
                    .clamp(slider.min(), slider.max())
            };
            prop_assert_eq!(next, expected);
            value = next;
        }
    }

    /// Minus, track and plus lie in the band in this order without
    /// overlapping, as far from either end as the margin (where titles
    /// start and values end on every row).
    #[test]
    fn its_parts_keep_to_the_margins_at_any_width(
        x in 0u16..200,
        width in 200u16..1200,
        height in 20u16..120,
    ) {
        let band = Rect::new(x, 300, width, height);
        let SliderParts {
            minus, track, plus, ..
        } = parts(&style(), band);
        let margin = u32::from(style().margin());
        prop_assert_eq!(u32::from(minus.x), u32::from(band.x) + margin);
        prop_assert_eq!(plus.right() + margin, band.right());
        prop_assert!(minus.right() <= u32::from(track.x));
        prop_assert!(track.right() <= u32::from(plus.x));
        for part in [minus, track, plus] {
            prop_assert!(part.y >= band.y && part.bottom() <= band.bottom(), "{part:?}");
        }
        // The buttons are square and alike, centred in the band.
        prop_assert_eq!(minus.width, minus.height);
        prop_assert_eq!(minus.width, plus.width);
        prop_assert_eq!(minus.y, plus.y);
        prop_assert!((i32::from(minus.y - band.y) - (band.bottom() as i32 - minus.bottom() as i32)).abs() <= 1);
    }

    /// The track, the fill and the knob stay on the track for any value,
    /// in range or not, black or white (on tracks at least as high as
    /// the smallest knob, 4 pixels).
    #[test]
    fn the_track_is_drawn_on_the_track(
        slider in slider_strategy(),
        value in -3000i64..3000,
        width in 1u16..400,
        height in 4u16..80,
        white in any::<bool>(),
        round in any::<bool>(),
    ) {
        let track = Rect::new(40, 20, width, height);
        let style = style().with_corner(if round { 6 } else { 0 });
        let ink = if white { Color::WHITE } else { Color::BLACK };
        let mut frame = frame(500, 120);
        slider.draw_track(&mut frame.canvas(), &style, track, value, ink);
        prop_assert_eq!(drawn_outside(&mut frame, track), None);
    }
}

/// A finger that takes the knob along and comes back to where it came
/// down brings the knob back to its value, also where the slider has more
/// steps than the track pixels, or a last step shorter than a pixel: the
/// knob is not moved by where its value falls between two pixels.
#[test]
fn a_drag_brought_back_keeps_the_value() {
    let band = Rect::new(20, 40, 360, 30);
    // Many steps on few pixels; a last step of one (672 to 673).
    for (slider, value) in [
        (Slider::new(138, 1413), 144),
        (Slider::new(0, 673).step(14), 672),
    ] {
        let mut drag = SliderDrag::default();
        let mut at =
            |phase, x| drag.on_pointer(&pointer(phase, x, 50), &style(), &slider, band, value);
        assert_eq!(at(PointerPhase::Down, 20), Some(SliderStep::Same));
        assert!(matches!(
            at(PointerPhase::Move, 28),
            Some(SliderStep::Moved(_))
        ));
        at(PointerPhase::Move, 20);
        assert_eq!(
            at(PointerPhase::Up, 20),
            Some(SliderStep::Chosen(value)),
            "{slider:?}"
        );
    }
}

/// The odd case: a range not whole steps (0 to 12 in fives). The drag
/// offers 0, 5, 10 and 12; the buttons come down from 12 to 7, a value
/// the drag never offers, so the same slider sets other values by touch
/// than by buttons.
#[test]
fn the_buttons_come_back_from_an_end_to_the_steps() {
    let slider = Slider::new(0, 12).step(5);
    assert_eq!(slider.stepped(10, 1), 12);
    let down = slider.stepped(12, -1);
    assert!(offered(&slider, down), "{down}");
    assert_eq!(down, 10);
}

/// A number setting a broken plugin gives the widest range: drawing and
/// touching its slider works out the span `max - min`, which overflows.
#[test]
fn the_widest_range_does_not_bring_it_down() {
    let slider = Slider::new(i64::MIN, i64::MAX);
    let track = Rect::new(0, 0, 200, 20);
    let share = slider.share(0);
    assert!((0.0..=1.0).contains(&share));
    let value = slider.value_at(track, 100);
    assert!((slider.min()..=slider.max()).contains(&value));
    slider.draw_track(
        &mut frame(200, 20).canvas(),
        &style(),
        track,
        0,
        Color::BLACK,
    );
}

#[test]
fn wrong_ranges_and_steps_are_made_right() {
    // The greatest below the least: a single value.
    let slider = Slider::new(10, -10);
    assert_eq!((slider.min(), slider.max()), (10, 10));
    assert_eq!(slider.value_at(Rect::new(0, 0, 100, 20), 90), 10);
    assert_eq!(slider.share(10), 0.0);
    // No step, or one backwards: a step of one.
    assert_eq!(Slider::new(0, 9).step(0).step_size(), 1);
    assert_eq!(Slider::new(0, 9).step(-5).step_size(), 1);
    // Huge steps stay in the range.
    let big = Slider::new(0, 100).step(i64::MAX);
    assert_eq!(big.stepped(0, 1), 100);
    assert_eq!(big.stepped(100, -3), 0);
    assert_eq!(Slider::new(0, 100).stepped(50, i64::MAX), 100);
    assert_eq!(Slider::new(0, 100).stepped(50, i64::MIN), 0);
}

#[test]
fn a_finger_beside_the_band_is_left_to_the_screen() {
    let slider = Slider::new(0, 10);
    let band = Rect::new(100, 50, 200, 20);
    let mut drag = SliderDrag::default();
    for (x, y) in [(99, 60), (300, 60), (150, 49), (150, 70)] {
        assert_eq!(
            drag.on_pointer(
                &pointer(PointerPhase::Down, x, y),
                &style(),
                &slider,
                band,
                5
            ),
            None,
            "({x}, {y})"
        );
    }
    // Another finger while one drags: not taken.
    drag.on_pointer(
        &pointer(PointerPhase::Down, 150, 60),
        &style(),
        &slider,
        band,
        5,
    )
    .unwrap();
    let mut other = pointer(PointerPhase::Up, 290, 60);
    other.id = 3;
    assert_eq!(drag.on_pointer(&other, &style(), &slider, band, 5), None);
    assert!(drag.dragging().is_some());
}

#[test]
fn every_way_of_moving_a_step_moves_one_step() {
    for down in [
        actions::LEFT,
        actions::DOWN,
        actions::PREV_PAGE,
        actions::PREVIOUS,
    ] {
        assert_eq!(key_step(&down), Some(-1), "{down}");
    }
    for up in [
        actions::RIGHT,
        actions::UP,
        actions::NEXT_PAGE,
        actions::NEXT,
    ] {
        assert_eq!(key_step(&up), Some(1), "{up}");
    }
    assert_eq!(key_step(&actions::BACK), None);
}

/// A slider in steps of 5.
static SPACING: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new().step(5).slider();

#[test]
fn sliders_show_on_touch_screens_and_numbers_elsewhere() {
    let spec = SettingSpec::number(
        "t.spacing",
        "t.spacing",
        100,
        250,
        135,
        paperpaw_sdk::builtin::groups::OTHER,
    )
    .with_extra(&SPACING);
    let touch = Harness::new(Device::TOUCH);
    let buttons = Harness::new(Device::BUTTONS);
    assert!(slider::shows(&touch.env(), &spec));
    assert!(!slider::shows(&buttons.env(), &spec), "buttons: − and +");
    let plain = SettingSpec::number(
        "t.size",
        "t.size",
        1,
        9,
        3,
        paperpaw_sdk::builtin::groups::OTHER,
    );
    assert!(!slider::shows(&touch.env(), &plain), "not marked");
}
