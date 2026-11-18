//! How book text is inked: the weight, the tone and whether it is drawn without
//! grays, from the reading settings as Beta 3's `PageType` read them
//! (`core.font-weight`, `core.smooth-text`, `core.text-contrast`,
//! `core.text-sharpness`, `core.text-curve`). The host draws it
//! (`Font::draw_inked`); what the host alone knows, the black threshold of the
//! screen, it applies itself when `sharp` is asked.
use paperpaw_sdk::{
    native::{
        Error, Host,
        fonts::Ink,
        settings::{self, SettingValue},
    },
    text::ToneCurve,
};

/// The settings that ink book text, read once for a draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    /// Font weight steps, 0 (as the font is) to 4.
    weight: u8,
    /// The curve's opacities, for the 16 coverage levels.
    tone: [u8; 16],
    /// Drawn without grays.
    sharp: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            weight: 0,
            tone: ToneCurve::SMOOTH.alphas(),
            sharp: false,
        }
    }
}

/// The contrast levels of `core.text-contrast`, lighter to darkest.
const CONTRAST: [&str; 4] = [
    "core.text-contrast.lighter",
    "core.text-contrast.normal",
    "core.text-contrast.darker",
    "core.text-contrast.darkest",
];
/// The levels of `core.text-sharpness`.
const SHARPNESS: [&str; 3] = [
    "core.text-sharpness.soft",
    "core.text-sharpness.normal",
    "core.text-sharpness.sharp",
];

impl Look {
    /// The look of the settings.
    ///
    /// # Errors
    /// The host's refusal to read a setting.
    pub fn read(host: &Host<'_>, grays: Option<u16>) -> Result<Self, Error> {
        let mut bytes = [0; 256];
        let weight = match settings::read_setting(host, "core.font-weight", &mut bytes)? {
            SettingValue::Int(w) => w,
            _ => 0,
        };
        let smooth = matches!(
            settings::read_setting(host, "core.smooth-text", &mut bytes)?,
            SettingValue::Bool(true)
        );
        let mut curve = [0; 256];
        let own = Self::own_curve(settings::read_setting(host, "core.text-curve", &mut curve))?;
        let contrast = Self::level(host, "core.text-contrast", &CONTRAST)?;
        let sharpness = Self::level(host, "core.text-sharpness", &SHARPNESS)?;
        Ok(Self::from_values(
            weight, smooth, grays, own, contrast, sharpness,
        ))
    }

    /// The look of values read: the weight, smooth text, the panel's `grays`
    /// (`None`: many), a curve of one's own, and the indexes of the contrast
    /// and the sharpness levels. Smooth text off is not where the panel shows
    /// black and white alone: its frame draws the same letters already, as in
    /// Beta 3. A curve of one's own takes the place of the contrast and the
    /// sharpness (no script is told here: the entry without one).
    #[must_use]
    pub fn from_values(
        weight: i64,
        smooth: bool,
        grays: Option<u16>,
        own: Option<ToneCurve>,
        contrast: Option<usize>,
        sharpness: Option<usize>,
    ) -> Self {
        let weight = u8::try_from(weight.clamp(0, 4)).unwrap_or(0);
        let smooth_off = grays != Some(2) && !smooth;
        if let Some(curve) = own {
            return Self::of(weight, curve, smooth_off);
        }
        let step = i8::try_from(contrast.unwrap_or(1)).unwrap_or(1) + ToneCurve::LIGHTEST;
        // Soft a step lighter; sharp a step darker and without grays.
        let (shift, sharp) = match sharpness {
            Some(0) => (-1, false),
            Some(2) => (1, true),
            _ => (0, false),
        };
        Self::of(
            weight,
            ToneCurve::contrast(step + shift),
            smooth_off || sharp,
        )
    }

    fn of(weight: u8, curve: ToneCurve, sharp: bool) -> Self {
        Self {
            weight,
            tone: curve.alphas(),
            sharp,
        }
    }

    /// The curve of one's own a read of `core.text-curve` gives. A value too
    /// long for the buffer is no curve of one's own (the contrast then): the
    /// page is drawn, not refused.
    fn own_curve(read: Result<SettingValue<'_>, Error>) -> Result<Option<ToneCurve>, Error> {
        match read {
            Ok(SettingValue::Text(text)) => Ok(ToneCurve::parse_for(text, None)),
            Ok(_) | Err(Error::NoSpace) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// The index of the level `key` is set to, among `levels`.
    fn level(host: &Host<'_>, key: &str, levels: &[&str]) -> Result<Option<usize>, Error> {
        let mut bytes = [0; 64];
        Ok(match settings::read_setting(host, key, &mut bytes)? {
            SettingValue::Text(text) => levels.iter().position(|l| *l == text),
            _ => None,
        })
    }

    /// How many pixels wider text at `size` pixels is drawn: about one a
    /// weight step at 48 px, at least one for any weight.
    #[must_use]
    pub fn extra(&self, size: f32) -> u32 {
        match self.weight {
            0 => 0,
            w => (f32::from(w) * size / 48.0 + 0.5).max(1.0) as u32,
        }
    }

    /// How text at `size` pixels is inked.
    #[must_use]
    pub fn ink(&self, size: f32) -> Ink {
        let ink = Ink::new().extra(self.extra(size)).tone(self.tone);
        if self.sharp { ink.sharp() } else { ink }
    }
}

#[cfg(test)]
mod tests;
