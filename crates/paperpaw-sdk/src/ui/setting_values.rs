//! The value of a setting as it is shown, and stepping it: the part of
//! the settings' rows that draws nothing.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

/// The value of a setting for display, with its unit.
pub fn setting_value(env: &Env<'_>, spec: &SettingSpec) -> String {
    let value = plain_setting_value(env, spec);
    // Set for the scope (e.g. this book only): marked.
    if env.settings().is_scoped(spec.id.as_str()) {
        format!("• {value}")
    } else {
        value
    }
}

pub(super) fn plain_setting_value(env: &Env<'_>, spec: &SettingSpec) -> String {
    match env.settings().get(spec.id.as_str()) {
        Some(SettingValue::Bool(true)) => env.i18n().tr("core.on").to_string(),
        Some(SettingValue::Bool(false)) => env.i18n().tr("core.off").to_string(),
        Some(SettingValue::Int(v)) => match &spec.extra().unit {
            Some(unit) => env.i18n().plural(unit, *v, &[]),
            None => format!("{v}"),
        },
        // Kept with the secrets: dots where there is one.
        _ if spec.extra().secret => {
            if env
                .secrets
                .get(spec.id.as_str())
                .is_some_and(|s| !s.is_empty())
            {
                "••••••".to_string()
            } else {
                String::new()
            }
        }
        Some(SettingValue::Text(v)) => option_name(env, spec, v),
        None => String::new(),
    }
}

/// The display name of a choice or language value.
pub fn option_name(env: &Env<'_>, spec: &SettingSpec, value: &str) -> String {
    match spec.kind {
        SettingKind::Language => env
            .i18n
            .languages()
            .into_iter()
            .find(|(tag, _)| *tag == value)
            .map_or(value.to_string(), |(_, name)| name.to_string()),
        // Choices are ids; their translation is their display name.
        SettingKind::Choice(_) => env.i18n().tr(value).to_string(),
        SettingKind::List => {
            let names: Vec<&str> = crate::status::item_ids(value)
                .map(|id| {
                    let title = env
                        .status_items()
                        .title(id)
                        .or_else(|| env.widgets().title(id));
                    env.i18n().tr(title.unwrap_or(id))
                })
                .collect();
            if names.is_empty() {
                env.i18n().tr("core.list.none").to_string()
            } else {
                names.join(", ")
            }
        }
        _ => value.to_string(),
    }
}

/// Whether a choice (or the language) has just two options: choosing it
/// switches to the other at once, as a switch does, instead of opening a
/// list that would only offer that one (fewer presses).
pub fn two_options(env: &Env<'_>, spec: &SettingSpec) -> bool {
    // More ways to choose are listed with the options.
    if !env.menus().items(spec.id.as_str()).is_empty() {
        return false;
    }
    match &spec.kind {
        SettingKind::Choice(options) => options.len() == 2,
        SettingKind::Language => env.i18n().languages().len() == 2,
        _ => false,
    }
}

/// What choosing a setting (confirm or a tap) opens, the same on every
/// screen that lists settings: its editor ([`SettingExtra::editor`](crate::SettingExtra::editor)),
/// the keyboard for text (a secret entered afresh), the values of a
/// choice of more than two on the screen of slot
/// [`CHOOSE_VALUE`](crate::builtin::slots::CHOOSE_VALUE). `None` where
/// the screen changes it itself: a switch, a number, a choice of two
/// (see [`next_value`]), or a choice where no screen fills that slot.
pub fn setting_opens(env: &Env<'_>, spec: &SettingSpec) -> Option<crate::Command> {
    if let Some(editor) = &spec.extra().editor {
        return Some(editor.open(&spec.id));
    }
    match spec.kind {
        SettingKind::Text => {
            let question = crate::ask::text(
                env.i18n().tr(&spec.title),
                &crate::builtin::events::SETTING_TYPED,
            )
            .with(crate::builtin::params::ID, spec.id.as_str().to_string());
            let question = if spec.extra().secret {
                question.password()
            } else {
                question.initial(setting_value(env, spec))
            };
            Some(question.command())
        }
        SettingKind::Choice(_) | SettingKind::Language
            if !two_options(env, spec) && env.has_slot(&crate::builtin::slots::CHOOSE_VALUE) =>
        {
            Some(crate::Editor::Slot(crate::builtin::slots::CHOOSE_VALUE).open(&spec.id))
        }
        _ => None,
    }
}

/// Whether choosing the setting opens another screen (its own editor, or
/// a list to choose from): its row shows a `›`.
pub fn opens_screen(env: &Env<'_>, spec: &SettingSpec) -> bool {
    spec.extra().editor.is_some()
        || (matches!(spec.kind, SettingKind::Choice(_) | SettingKind::Language)
            && !two_options(env, spec))
}

/// The sizes of book text and how the reading font draws them: sizes
/// that look alike are drawn at the same pixel size of a font pack.
struct FontSizes<'e> {
    font: crate::FontHandle<'e>,
    metrics: crate::Metrics,
    min: i64,
    max: i64,
}

impl<'e> FontSizes<'e> {
    /// Those `spec` sets, where it is the size of book text and a reading
    /// font is there.
    fn of(env: &Env<'e>, spec: &SettingSpec) -> Option<Self> {
        let SettingKind::Int { min, max } = spec.kind else {
            return None;
        };
        if spec.id.as_str() != crate::builtin::settings::FONT_SIZE.as_str() {
            return None;
        }
        Some(Self {
            font: env.fonts().role(crate::builtin::fonts::READING.as_str())?,
            metrics: env.metrics(),
            min,
            max: max.max(min),
        })
    }

    /// What `points` looks like: alike for sizes drawn alike.
    fn looks(&self, points: i64) -> (u32, u32) {
        let px = self.metrics.pt(points as f32);
        let height = self.font.line_metrics(px).line_height;
        (
            height.to_bits(),
            self.font.measure(px, "Hamburgefonstiv").to_bits(),
        )
    }

    /// The smallest of the sizes that look as `points` does.
    fn first_alike(&self, points: i64) -> i64 {
        let mut points = points.clamp(self.min, self.max);
        let look = self.looks(points);
        while points > self.min && self.looks(points - 1) == look {
            points -= 1;
        }
        points
    }
}

/// The value number setting `spec` shows for `value`, e.g. where a finger
/// leaves a slider: `value`, but for the size of book text the smallest
/// of the sizes the reading font draws alike (a font pack has a few pixel
/// sizes; on the X4 Pro 9 and 10 points are both 28 pixels), so that no
/// two sizes offered look the same. Values kept stay as they are.
pub fn shown_number(env: &Env<'_>, spec: &SettingSpec, value: i64) -> i64 {
    FontSizes::of(env, spec).map_or(value, |sizes| sizes.first_alike(value))
}

/// The value number setting `spec` steps to from `from` towards `to`
/// (a step of its own, a button's, a key's): `to`, but for the size of
/// book text the next size that looks different from `from`, as
/// [`shown_number`] shows it, and `from` where none that way does.
pub fn shown_step(env: &Env<'_>, spec: &SettingSpec, from: i64, to: i64) -> i64 {
    let Some(sizes) = FontSizes::of(env, spec).filter(|_| to != from) else {
        return to;
    };
    let direction = (to - from).signum();
    let before = sizes.looks(from);
    let mut points = to.clamp(sizes.min, sizes.max);
    while sizes.looks(points) == before {
        let next = (points + direction).clamp(sizes.min, sizes.max);
        if next == points {
            return from;
        }
        points = next;
    }
    sizes.first_alike(points)
}

/// The value a slider of number setting `spec` comes to, moved from
/// `before` (its value when the finger came down, or the key was
/// pressed) to `value` ([`Slider`](crate::ui::slider::Slider) steps and
/// drags): a step away, the next value that looks different
/// ([`shown_step`]); further, the value shown for `value`
/// ([`shown_number`]). Only the size of book text skips values.
pub fn shown_slider_value(env: &Env<'_>, spec: &SettingSpec, before: i64, value: i64) -> i64 {
    if value.abs_diff(before) == spec.extra().step.unsigned_abs().max(1) {
        shown_step(env, spec, before, value)
    } else {
        shown_number(env, spec, value)
    }
}

/// The value after one step of the setting: a switch toggles, a number
/// moves by `step` within its range (the size of book text to the next
/// size that looks different, see [`shown_step`]), a choice or language
/// moves to the next (or previous) option. `None` for text.
pub fn next_value(env: &Env<'_>, spec: &SettingSpec, step: i64) -> Option<SettingValue> {
    let current = env.settings().get(spec.id.as_str())?;
    let cycle = |options: &[&str], value: &str| -> SettingValue {
        let len = options.len().max(1) as i64;
        // From a value not among the options, the first step on is the
        // first option and the first step back the last.
        let index = match options.iter().position(|o| *o == value) {
            Some(index) => index as i64,
            None if step > 0 => -1,
            None => len,
        };
        let next = options
            .get(index.saturating_add(step).rem_euclid(len) as usize)
            .copied()
            .unwrap_or_default();
        SettingValue::Text(Cow::Owned(next.to_string()))
    };
    Some(match (&spec.kind, current) {
        (SettingKind::Bool, SettingValue::Bool(v)) => SettingValue::Bool(!v),
        (SettingKind::Int { min, max }, SettingValue::Int(v)) => {
            let next = v.saturating_add(step.saturating_mul(spec.extra().step));
            SettingValue::Int(shown_step(
                env,
                spec,
                *v,
                next.clamp(*min, (*max).max(*min)),
            ))
        }
        (SettingKind::Choice(options), SettingValue::Text(v)) => {
            let options: Vec<&str> = options.iter().collect();
            cycle(&options, v)
        }
        (SettingKind::Language, SettingValue::Text(v)) => {
            let tags: Vec<&str> = env.i18n().languages().into_iter().map(|(t, _)| t).collect();
            cycle(&tags, v)
        }
        _ => return None,
    })
}
