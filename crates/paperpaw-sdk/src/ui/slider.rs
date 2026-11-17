//! Sliders: numbers better set by eye than by their value, such as the
//! line spacing or a light's brightness. A track filled up to the value,
//! with a knob there, between a `−` and a `+`; its title and value (with
//! its unit) on the line above, as in other rows.
//!
//! Equally easy by touch and with buttons alone:
//!
//! - Touch: the slider's band is as high as a fingertip needs
//!   (`UiStyle::finger`) and as wide as the screen. A finger that comes
//!   down anywhere on it and moves sideways moves the knob as far as it
//!   moves itself (`SliderDrag`), so it never jumps where the finger
//!   lands and the finger need not cover the knob; the value follows in
//!   the slider's steps, and lifting the finger chooses it. A tap on the
//!   track chooses the value there, a tap on `−` or `+` (or beside them)
//!   moves it one step.
//! - Buttons: on a focused slider, *confirm* starts moving it; left and
//!   right (or up and down, or the page buttons) move it one step each
//!   (`key_step`), *confirm* chooses the value, *back* goes back to the
//!   one before. The `−` and `+` are filled while the buttons move it.
//!
//! While a value moves it is best shown without being kept
//! ([`Command::PreviewSettings`](crate::Command::PreviewSettings) for a
//! setting), so that what it changes shows at once; it is kept when
//! chosen.
//!
//! Sliders are for touch screens, where a finger sets a value by eye;
//! with buttons alone a number is changed a step at a time all the same,
//! and a compact row with `−` and `+` serves it better ([`shows`]).
//!
//! In a right-to-left interface a slider is mirrored
//! (`Slider::mirrored` with `UiStyle::is_rtl`): its lowest value at
//! the right, the track filled from there, `−` at the right and `+` at
//! the left, and the left button moving it up (`Slider::key_step`).
//!
//! A screen keeps a `Slider` for each (from a setting with
//! `Slider::of`), draws rows of `row_height` with
//! `Slider::draw_row` (what it shows in a `SliderView`), or only the
//! track with `Slider::draw_track` where it places the buttons itself,
//! and passes pointer samples to `SliderDrag::on_pointer` from
//! `on_pointer`. While the knob
//! moves, the slider alone is best drawn again with a quick refresh
//! ([`RefreshMode::Fast`](crate::RefreshMode::Fast)), and the screen
//! with a clean one when the finger is lifted.
