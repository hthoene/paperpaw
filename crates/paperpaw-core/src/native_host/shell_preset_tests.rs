//! `command.apply_preset` (1546) through a native callback: checked whole at
//! submit, applied after the callback, with the covered set of each kind.
use super::item_state::run;
use super::*;
use crate::native_host::commands::{
    WireCommand, preset_covers, preset_plan as presets_plan, preset_walk,
};
use alloc::string::String;
use paperpaw_native::host_glue::{ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_sdk::builtin::groups::{LOOK, OTHER};
use paperpaw_sdk::presets::{self, Preset};
use paperpaw_sdk::{Id, SettingSpec};

fn apply(preset: &str) -> WireCommand<'_> {
    WireCommand::ApplyPreset { preset, flags: 0 }
}

fn preset(id: &'static str, kind: &'static str, values: &'static str) -> Preset {
    Preset::new(Id::new_static(id), Id::new_static(kind), "t", "d", values)
}

/// A kernel with `looks` appearance switches `fake.look<n>`, outside ones,
/// a guarded appearance one of another plugin and a grant.
fn kernel(looks: usize) -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let settings = &mut kernel.shared.settings;
    let mut define = |key: String, group| {
        let key: &'static str = std::boxed::Box::leak(key.into_boxed_str());
        settings
            .define(SettingSpec::switch(key, "setting", false, group))
            .unwrap();
    };
    for n in 0..looks {
        define(std::format!("fake.look{n}"), LOOK);
    }
    define("fake.outside".into(), OTHER);
    define("core.grant.fake".into(), OTHER);
    kernel
}

fn guarded_look(kernel: &mut crate::Kernel) {
    kernel
        .shared
        .settings
        .define(SettingSpec::switch("other.locked", "locked", false, LOOK).guarded())
        .unwrap();
}

fn add(kernel: &mut crate::Kernel, preset: Preset) {
    kernel.shared.settings.presets_mut().add(preset);
}

fn on(kernel: &crate::Kernel, key: &str) -> bool {
    kernel.shared.settings.bool(key)
}

#[test]
fn a_theme_covers_the_appearance_but_what_the_reader_needs() {
    let mut kernel = kernel(2);
    add(
        &mut kernel,
        preset(
            "fake.dark",
            presets::THEME,
            "fake.look0 = on\nfake.outside = on\nfake.look9 = on\n",
        ),
    );
    kernel
        .shared
        .settings
        .set("fake.look1", paperpaw_sdk::SettingValue::Bool(true))
        .unwrap();
    let (submitted, result) = run(&mut kernel, &[apply("fake.dark")], OK);
    assert_eq!((submitted, result), (OK, Ok(OK)));
    assert!(on(&kernel, "fake.look0"), "named, covered: set");
    assert!(!on(&kernel, "fake.look1"), "left out, covered: its default");
    assert!(
        !on(&kernel, "fake.outside"),
        "named, outside the set: left out"
    );
}

#[test]
fn the_sets_are_those_the_sdk_defines() {
    let kernel = kernel(3);
    let (settings, widgets) = (&kernel.shared.settings, &kernel.shared.widgets);
    for (kind, sdk) in [
        (presets::THEME, presets::theme_settings(settings)),
        (
            presets::WIDGET_LOOK,
            presets::widget_look_settings(settings, widgets),
        ),
    ] {
        assert!(!sdk.is_empty(), "{kind}");
        let ours: std::vec::Vec<&str> = settings
            .specs()
            .filter(|spec| preset_covers(kind, settings, widgets, spec))
            .map(|spec| spec.id.as_str())
            .collect();
        let sdk: std::vec::Vec<&str> = sdk.iter().map(Id::as_str).collect();
        assert_eq!(ours, sdk, "{kind}");
        // The walk visits them in the same order, a preset naming outside
        // ones (the interface's size, the logo) changes none of those.
        let all = preset(
            "fake.all",
            kind,
            "ui.size = 1\nui.contrast = 1\nfake.outside = on\n",
        );
        let mut seen = std::vec::Vec::new();
        preset_walk(settings, widgets, &all, &mut |key, _, _| {
            seen.push(std::string::String::from(key));
            Ok(())
        })
        .unwrap();
        assert!(
            seen.iter()
                .map(std::string::String::as_str)
                .eq(sdk.iter().copied()),
            "{kind}: {seen:?}"
        );
    }
    // The theme leaves the reader's interface alone.
    let theme = presets::theme_settings(settings);
    for kept in ["ui.size", "ui.contrast"] {
        assert!(!theme.iter().any(|id| id.as_str() == kept), "{kept}");
    }
}

/// A value of `spec` other than its default, as written, and as a value.
fn other_than_default(
    spec: &SettingSpec,
) -> Option<(std::string::String, paperpaw_sdk::SettingValue)> {
    use paperpaw_sdk::{SettingKind, SettingValue};
    let text = match (&spec.kind, &spec.default) {
        (SettingKind::Bool, SettingValue::Bool(default)) => std::format!("{}", !default),
        (SettingKind::Int { min, max }, SettingValue::Int(default)) => {
            std::format!("{}", if default == min { max } else { min })
        }
        (SettingKind::Choice(choices), SettingValue::Text(default)) => choices
            .iter()
            .find(|choice| *choice != default.as_ref())?
            .into(),
        _ => return None,
    };
    let value = spec
        .kind
        .parse(&text)
        .filter(|value| *value != spec.default)?;
    Some((text, value))
}

#[test]
fn a_widget_look_sets_what_it_names_resets_what_it_leaves_out_and_changes_nothing_else() {
    let mut kernel = kernel(0);
    // Two settings of the look (all widgets') that have another value.
    let settings = &kernel.shared.settings;
    let mut found: std::vec::Vec<(
        std::string::String,
        std::string::String,
        paperpaw_sdk::SettingValue,
    )> = std::vec::Vec::new();
    for spec in settings.specs() {
        if preset_covers(presets::WIDGET_LOOK, settings, &kernel.shared.widgets, spec)
            && let Some((text, value)) = other_than_default(spec)
        {
            found.push((spec.id.as_str().into(), text, value));
        }
    }
    assert!(
        found.len() >= 2,
        "the look has settings to change: {found:?}"
    );
    let (named, named_text, named_value) = found[0].clone();
    let (left_out, _, left_out_value) = found[1].clone();
    let values: &'static str = std::boxed::Box::leak(
        std::format!("{named} = {named_text}\nfake.outside = on\n").into_boxed_str(),
    );
    add(
        &mut kernel,
        preset("fake.look", presets::WIDGET_LOOK, values),
    );
    kernel
        .shared
        .settings
        .set(&left_out, left_out_value.clone())
        .unwrap();
    let default_of = |kernel: &crate::Kernel, key: &str| {
        kernel.shared.settings.spec(key).unwrap().default.clone()
    };
    let get = |kernel: &crate::Kernel, key: &str| {
        kernel
            .shared
            .settings
            .get(key)
            .cloned()
            .unwrap_or_else(|| default_of(kernel, key))
    };
    assert_eq!(get(&kernel, &left_out), left_out_value);
    assert_eq!(get(&kernel, &named), default_of(&kernel, &named));
    let (submitted, result) = run(&mut kernel, &[apply("fake.look")], OK);
    assert_eq!((submitted, result), (OK, Ok(OK)));
    assert_eq!(get(&kernel, &named), named_value, "named: set");
    assert_eq!(
        get(&kernel, &left_out),
        default_of(&kernel, &left_out),
        "left out: its default"
    );
    assert!(!on(&kernel, "fake.outside"), "outside the set: left out");
}

#[test]
fn another_kind_changes_only_what_its_presets_name() {
    let mut kernel = kernel(1);
    add(
        &mut kernel,
        preset(
            "fake.mode",
            "fake.kind",
            "fake.outside = on\nfake.nothing = on\nfake.look0 = nonsense\ncore.grant.fake = on\n",
        ),
    );
    kernel
        .shared
        .settings
        .set("fake.look0", paperpaw_sdk::SettingValue::Bool(true))
        .unwrap();
    let (submitted, _) = run(&mut kernel, &[apply("fake.mode")], OK);
    assert_eq!(submitted, OK);
    assert!(on(&kernel, "fake.outside"));
    assert!(
        on(&kernel, "fake.look0"),
        "not covered, not named right: as it was"
    );
    assert!(!on(&kernel, "core.grant.fake"), "a grant is never changed");
}

#[test]
fn a_preset_is_checked_whole_and_a_failed_callback_applies_nothing() {
    let mut kernel = kernel(1);
    guarded_look(&mut kernel);
    add(
        &mut kernel,
        preset("fake.ok", "fake.kind", "fake.outside = on\n"),
    );
    add(
        &mut kernel,
        preset(
            "fake.guard",
            "fake.kind",
            "fake.outside = on\nother.locked = on\n",
        ),
    );
    add(
        &mut kernel,
        preset("fake.coverguard", presets::THEME, "fake.look0 = on\n"),
    );
    let flagged = WireCommand::ApplyPreset {
        preset: "fake.ok",
        flags: 1,
    };
    for (commands, status) in [
        (std::vec![apply("fake.none")], ERR_NOT_FOUND),
        (std::vec![apply("")], ERR_NOT_FOUND),
        (std::vec![flagged], ERR_UNSUPPORTED),
        // A foreign guarded setting named, or covered, refuses the batch.
        (
            std::vec![apply("fake.ok"), apply("fake.guard")],
            ERR_INVALID,
        ),
        (
            std::vec![apply("fake.ok"), apply("fake.coverguard")],
            ERR_INVALID,
        ),
    ] {
        let (submitted, _) = run(&mut kernel, &commands, OK);
        assert_eq!(submitted, status);
        assert!(!on(&kernel, "fake.outside"), "nothing staged: {status}");
        assert!(!on(&kernel, "other.locked"));
    }
    // A callback that fails applies nothing.
    let (submitted, result) = run(&mut kernel, &[apply("fake.ok")], ERR_INVALID);
    assert_eq!(submitted, OK);
    assert!(result.is_err());
    assert!(!on(&kernel, "fake.outside"));
    let (submitted, _) = run(&mut kernel, &[apply("fake.ok")], OK);
    assert_eq!(submitted, OK);
    assert!(on(&kernel, "fake.outside"));
}

#[test]
fn a_theme_of_many_settings_applies_all_of_them_a_chunk_at_a_time() {
    let mut kernel = kernel(40);
    let values: std::string::String = (0..40)
        .step_by(2)
        .map(|n| std::format!("fake.look{n} = on\n"))
        .collect();
    let values: &'static str = std::boxed::Box::leak(values.into_boxed_str());
    add(&mut kernel, preset("fake.many", presets::THEME, values));
    for n in (1..40).step_by(2) {
        kernel
            .shared
            .settings
            .set(
                &std::format!("fake.look{n}"),
                paperpaw_sdk::SettingValue::Bool(true),
            )
            .unwrap();
    }
    let (submitted, _) = run(&mut kernel, &[apply("fake.many")], OK);
    assert_eq!(submitted, OK);
    for n in 0..40 {
        assert_eq!(
            on(&kernel, &std::format!("fake.look{n}")),
            n % 2 == 0,
            "{n}"
        );
    }
}

/// The kernel of the memory tests: every kind of value a preset gives.
fn kernel_of_kinds() -> crate::Kernel {
    let mut kernel = kernel(3);
    let settings = &mut kernel.shared.settings;
    settings
        .define(SettingSpec::text("fake.lookt", "text", "", LOOK))
        .unwrap();
    settings
        .define(SettingSpec::choice(
            "fake.lookc",
            "choice",
            &["fake.one", "fake.two", "fake.three"],
            "fake.one",
            LOOK,
        ))
        .unwrap();
    add(
        &mut kernel,
        preset(
            "fake.kinds",
            presets::THEME,
            "fake.look0 = on\nfake.lookt = some text of the preset\nfake.lookc = fake.three\nfake.lookc = fake.two\n",
        ),
    );
    kernel
}

/// Every allocation of a chunk's planning refused in turn is the memory
/// error and nothing more (never an abort); the first call that is not
/// refused plans what an unrefused one does.
#[test]
fn planning_a_chunk_refuses_every_allocation_and_never_aborts() {
    let kernel = kernel_of_kinds();
    let (settings, widgets) = (&kernel.shared.settings, &kernel.shared.widgets);
    let found = settings.presets().get("fake.kinds").unwrap();
    // The chunks of the whole walk (the theme has several), each then planned
    // with every allocation refused in turn.
    let mut whole = std::vec::Vec::new();
    let mut starts = std::vec::Vec::new();
    for from in (0..).step_by(16) {
        starts.push(from);
        if !presets_plan(settings, widgets, found, from, &mut whole).unwrap() {
            break;
        }
    }
    // Three settings get a new value: look0, the text, the choice.
    assert_eq!(whole.len(), 3, "{whole:?}");
    assert!(starts.len() > 1, "more than one chunk");
    let mut most = 0;
    for from in starts {
        let mut expected = std::vec::Vec::new();
        let more = presets_plan(settings, widgets, found, from, &mut expected).unwrap();
        for nth in 1_u32.. {
            let mut out = std::vec::Vec::new();
            super::arm_failure(nth);
            let planned = presets_plan(settings, widgets, found, from, &mut out);
            super::arm_failure(0);
            match planned {
                Err(status) => {
                    assert_eq!(status, paperpaw_native::host_glue::ERR_NO_MEMORY, "{nth}");
                    assert!(nth < 20, "refused past any plausible last allocation");
                }
                Ok(again) => {
                    assert_eq!(again, more);
                    assert_eq!(std::format!("{out:?}"), std::format!("{expected:?}"));
                    most = most.max(nth);
                    break;
                }
            }
        }
    }
    assert!(most >= 3, "only {most} allocation sites");
}

/// A guarded setting of another plugin that the preset's kind covers by the
/// time it is applied drops the preset whole: submit-time checks do not hold
/// for what an earlier command of the callback changed.
#[test]
fn a_preset_is_checked_again_when_it_is_applied() {
    let mut kernel = kernel(2);
    add(
        &mut kernel,
        preset("fake.dark", presets::THEME, "fake.look0 = on\n"),
    );
    // Passes at submit: nothing guarded is covered yet.
    let owner = Id::parse("fake").unwrap();
    let authority = kernel.command_settings_authority(&owner);
    guarded_look(&mut kernel);
    kernel.apply_native_presets(
        &owner,
        authority,
        std::vec![std::boxed::Box::from("fake.dark")],
    );
    assert!(!on(&kernel, "fake.look0"), "dropped whole, nothing applied");
    // The plugin's own guarded setting, or the editor's authority, may.
    let own = Id::parse("other").unwrap();
    let authority = kernel.command_settings_authority(&own);
    kernel.apply_native_presets(
        &own,
        authority,
        std::vec![std::boxed::Box::from("fake.dark")],
    );
    assert!(on(&kernel, "fake.look0"));
}

/// What the walk tells of a value is what `SettingKind::parse` accepts.
#[test]
fn a_value_fits_a_kind_as_the_sdk_parses_it() {
    let kernel = kernel_of_kinds();
    let settings = &kernel.shared.settings;
    for spec in settings
        .specs()
        .filter(|s| !s.id.as_str().starts_with("core.grant.") && !s.extra().secret)
    {
        for text in [
            "", "on", "off", "1", "-5", "99999", "x", "en", "en-US", "-", "fake.two", "a b",
        ] {
            let mut fits = None;
            let line: &'static str = std::boxed::Box::leak(
                std::format!("{} = {text}\n", spec.id.as_str()).into_boxed_str(),
            );
            let one = preset("fake.probe", "fake.kind", line);
            preset_walk(
                settings,
                &kernel.shared.widgets,
                &one,
                &mut |_, _, given| {
                    fits = Some(given.is_some());
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(
                fits.unwrap_or(false),
                spec.kind.parse(text.trim()).is_some(),
                "{}: {text:?}",
                spec.id.as_str()
            );
        }
    }
}

/// The commands `plan` makes, chunk after chunk.
fn planned(kernel: &crate::Kernel, preset: &Preset) -> std::vec::Vec<paperpaw_sdk::Command> {
    let (settings, widgets) = (&kernel.shared.settings, &kernel.shared.widgets);
    let mut all = std::vec::Vec::new();
    for from in (0..).step_by(16) {
        if !presets_plan(settings, widgets, preset, from, &mut all).unwrap() {
            break;
        }
    }
    all
}

/// The commands the SDK makes of the same preset (`Preset::changes` with the
/// covered set of its kind, then `presets::commands`): what interface 6's
/// plugins sent, which `plan` must stay equal to.
fn sdk_commands(kernel: &crate::Kernel, preset: &Preset) -> std::vec::Vec<paperpaw_sdk::Command> {
    let (settings, widgets) = (&kernel.shared.settings, &kernel.shared.widgets);
    let changes = match preset.kind.as_str() {
        presets::THEME => presets::theme_changes(settings, preset),
        presets::WIDGET_LOOK => presets::widget_look_changes(settings, widgets, preset),
        _ => preset.changes(settings, &[]),
    };
    presets::commands(settings, &changes)
}

#[test]
fn the_commands_planned_are_those_of_the_sdk() {
    use paperpaw_sdk::SettingValue;
    let mut kernel = kernel_of_kinds();
    // A theme: one reset, one set, one that holds already, a value that
    // does not fit, one outside the set.
    add(
        &mut kernel,
        preset(
            "fake.mixed",
            presets::THEME,
            "fake.look0 = on\nfake.lookt = held\nfake.lookc = nonsense\nfake.outside = on\nui.size = 3\n",
        ),
    );
    let settings = &mut kernel.shared.settings;
    settings
        .set("fake.look1", SettingValue::Bool(true))
        .unwrap();
    settings
        .set("fake.lookt", SettingValue::Text("held".into()))
        .unwrap();
    settings
        .set("fake.lookc", SettingValue::Text("fake.two".into()))
        .unwrap();
    // The widget look: a named setting and one left out, as the apply test.
    let looks: std::vec::Vec<(std::string::String, std::string::String, SettingValue)> = {
        let (settings, widgets) = (&kernel.shared.settings, &kernel.shared.widgets);
        settings
            .specs()
            .filter(|spec| preset_covers(presets::WIDGET_LOOK, settings, widgets, spec))
            .filter_map(|spec| {
                other_than_default(spec).map(|(text, value)| (spec.id.as_str().into(), text, value))
            })
            .collect()
    };
    assert!(looks.len() >= 2);
    let values: &'static str =
        std::boxed::Box::leak(std::format!("{} = {}\n", looks[0].0, looks[0].1).into_boxed_str());
    add(
        &mut kernel,
        preset("fake.shape", presets::WIDGET_LOOK, values),
    );
    kernel
        .shared
        .settings
        .set(&looks[1].0, looks[1].2.clone())
        .unwrap();
    // Another kind: named values, one held, one that does not fit.
    add(
        &mut kernel,
        preset(
            "fake.named",
            "fake.kind",
            "fake.outside = on\nfake.look2 = nonsense\nfake.lookt = held\nfake.nothing = 1\n",
        ),
    );
    for id in ["fake.mixed", "fake.shape", "fake.named", "fake.kinds"] {
        let found = kernel.shared.settings.presets().get(id).unwrap();
        let (ours, sdk) = (planned(&kernel, found), sdk_commands(&kernel, found));
        assert!(!sdk.is_empty(), "{id}: there is something to change");
        assert_eq!(std::format!("{ours:?}"), std::format!("{sdk:?}"), "{id}");
    }
}
