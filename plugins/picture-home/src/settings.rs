//! The settings of the picture home screen: the picture and how it is set,
//! the bars (the values the native code reads; the declarations are in
//! `declarations.rs`).

use paperpaw_sdk::setting::{Setting, Text};

// ---- The picture ----------------------------------------------------------

/// The folder on the card the pictures are in.
pub(crate) const FOLDER: Setting<Text> = Setting::new("picture-home.folder");
/// The picture's file name in that folder; none: no picture.
pub(crate) const PICTURE: Setting<Text> = Setting::new("picture-home.picture");
pub(crate) const FIT: Setting<Text> = Setting::new("picture-home.fit");
pub(crate) const ANCHOR: Setting<Text> = Setting::new("picture-home.anchor");
pub(crate) const ZOOM: Setting<i64> = Setting::new("picture-home.zoom");
pub(crate) const LIGHTEN: Setting<i64> = Setting::new("picture-home.lighten");
/// How light the picture is and how strongly its dark and light parts
/// differ (values of [`BRIGHTNESSES`] and [`CONTRASTS`]): panels differ.
pub(crate) const BRIGHTNESS: Setting<Text> = Setting::new("picture-home.brightness");
pub(crate) const CONTRAST: Setting<Text> = Setting::new("picture-home.contrast");

/// From much darker to much lighter, the middle as it is.
pub(crate) const BRIGHTNESSES: [&str; 5] = [
    "picture-home.brightness.much-darker",
    "picture-home.brightness.darker",
    "picture-home.brightness.normal",
    "picture-home.brightness.lighter",
    "picture-home.brightness.much-lighter",
];
/// Lower, as it is, higher.
pub(crate) const CONTRASTS: [&str; 3] = [
    "picture-home.contrast.lower",
    "picture-home.contrast.normal",
    "picture-home.contrast.higher",
];

/// How the picture is rendered (values of [`RENDERS`]).
pub(crate) const RENDER: Setting<Text> = Setting::new("picture-home.render");

/// Automatic is as the built-in home screen: the screen drawn in the
/// panel's grays where it has them to show, then dots after a change;
/// grey levels always; dots always.
pub(crate) const RENDERS: [&str; 3] = [
    "picture-home.render.auto",
    "picture-home.render.grays",
    "picture-home.render.dither",
];

/// How the picture fills the screen (values of [`FIT`]).
pub(crate) const FITS: [&str; 5] = [
    "picture-home.fit.fill",
    "picture-home.fit.fit",
    "picture-home.fit.stretch",
    "picture-home.fit.original",
    "picture-home.fit.tile",
];

/// Which part of the picture stays where it is cut, or where it sits where
/// it is smaller than the screen: the nine places, row by row (values of
/// [`ANCHOR`]).
pub(crate) const ANCHORS: [&str; 9] = [
    "picture-home.anchor.top-left",
    "picture-home.anchor.top",
    "picture-home.anchor.top-right",
    "picture-home.anchor.left",
    "picture-home.anchor.center",
    "picture-home.anchor.right",
    "picture-home.anchor.bottom-left",
    "picture-home.anchor.bottom",
    "picture-home.anchor.bottom-right",
];

// ---- The bars ---------------------------------------------------------------

/// Whether a bar shows: always, never, or for a few seconds after a press.
pub(crate) const BARS: [&str; 3] = [
    "picture-home.bar.off",
    "picture-home.bar.auto",
    "picture-home.bar.on",
];
pub(crate) const BAR_ATS: [&str; 2] = ["picture-home.bar.top", "picture-home.bar.bottom"];
pub(crate) const BAR_SHAPES: [&str; 2] = ["picture-home.bar.full", "picture-home.bar.compact"];
pub(crate) const BAR_ALIGNS: [&str; 3] = [
    "picture-home.bar.left",
    "picture-home.bar.center",
    "picture-home.bar.right",
];
/// What the bar is on: the page, the page inverted, grays, nothing.
pub(crate) const BAR_LOOKS: [&str; 5] = [
    "picture-home.look.normal",
    "picture-home.look.inverted",
    "picture-home.look.light",
    "picture-home.look.dark",
    "picture-home.look.clear",
];
