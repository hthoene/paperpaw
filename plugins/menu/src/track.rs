//! The track a number's slider moves along. A number's is its range, but
//! the size of book text skips the sizes the reading font draws alike (a
//! font pack has a few pixel sizes): on a track of the range the knob
//! would move one step here and two there. For that size the track is the
//! sizes that look different, evenly spaced, so that every step and every
//! finger's width moves the knob as far.

use alloc::vec::Vec;

use paperpaw_sdk::builtin::settings;
use paperpaw_sdk::ui;
use paperpaw_sdk::ui::slider::Slider;
use paperpaw_sdk::{Env, SettingKind, SettingSpec};

/// A slider and what its positions mean.
#[derive(Debug)]
pub(crate) struct Track {
    slider: Slider,
    /// The sizes of book text that look different, smallest first: the
    /// slider's positions are their indexes. `None` for other numbers,
    /// whose positions are their values.
    sizes: Option<Vec<i64>>,
}

impl Track {
    /// The track of number setting `spec`; `None` for other settings.
    pub(crate) fn of(env: &Env<'_>, spec: &SettingSpec) -> Option<Self> {
        let slider = Slider::of(spec)?;
        Some(match Self::sizes(env, spec) {
            Some(sizes) => Self {
                slider: Slider::new(0, sizes.len() as i64 - 1),
                sizes: Some(sizes),
            },
            None => Self {
                slider,
                sizes: None,
            },
        })
    }

    /// The sizes of book text the reading font draws differently, where
    /// `spec` is that size and there is more than one.
    fn sizes(env: &Env<'_>, spec: &SettingSpec) -> Option<Vec<i64>> {
        let SettingKind::Int { min, max } = spec.kind else {
            return None;
        };
        if spec.id.as_str() != settings::FONT_SIZE.as_str() {
            return None;
        }
        let mut sizes: Vec<i64> = Vec::new();
        for points in min..=max.max(min) {
            let shown = ui::shown_number(env, spec, points);
            if sizes.last() != Some(&shown) {
                sizes.push(shown);
            }
        }
        (sizes.len() > 1).then_some(sizes)
    }

    pub(crate) fn slider(&self) -> &Slider {
        &self.slider
    }

    /// Where the slider is for `value` of the setting: a value the font
    /// draws like a smaller size (one kept in the settings file) is at
    /// that size's place.
    pub(crate) fn at(&self, value: i64) -> i64 {
        match &self.sizes {
            Some(sizes) => sizes.iter().rposition(|s| *s <= value).unwrap_or(0) as i64,
            None => value,
        }
    }

    /// The value of the setting for position `at`, moved to from `before`
    /// (the value when the finger came down): where the knob has not
    /// moved from its place, `before` as it was.
    pub(crate) fn value(&self, env: &Env<'_>, spec: &SettingSpec, before: i64, at: i64) -> i64 {
        match &self.sizes {
            Some(_) if at == self.at(before) => before,
            Some(sizes) => sizes[at.clamp(0, sizes.len() as i64 - 1) as usize],
            None => ui::shown_slider_value(env, spec, before, at),
        }
    }
}
