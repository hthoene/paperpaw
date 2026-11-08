//! The native declarations name what the interface-6 registration names: the
//! group and its 28 settings (kinds, defaults, levels, editors, the condition
//! of the bar's alignment), the screens, the `core.home` offer, the icon, the
//! need, the pages as a place of widgets and the bar's place of status items.
//! The screens themselves and the status items' picker are not ported yet.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_plugin_picture_home::PictureHome;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, MenuNeed, Plugin, Registrar};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("picture-home"));
    PictureHome.register(&mut r).unwrap();
    r.into_contributions()
}

fn batches(
    declare: impl FnOnce(&mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>),
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            out.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d);
    }
    out
}

#[test]
fn the_group_and_the_settings_match() {
    let mut groups = Vec::new();
    let mut found = Vec::new();
    for contribution in legacy() {
        match contribution {
            Contribution::SettingGroup(g) => {
                groups.push((g.id.as_str().to_string(), g.title.to_string(), g.order));
            }
            Contribution::FixedSettings(list) => {
                for s in list.iter() {
                    let e = s.extra();
                    found.push((
                        s.id.as_str().to_string(),
                        s.title.to_string(),
                        s.description.as_deref().map(str::to_string),
                        s.group.as_str().to_string(),
                        format!("{:?} {:?} {:?}", s.kind, s.default, s.level),
                        format!("{:?}", e.editor),
                        format!("{:?}", e.shown_if),
                    ));
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        groups,
        [(
            "picture-home.group".to_string(),
            "picture-home.title".to_string(),
            221
        )]
    );
    let ids: Vec<&str> = found.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(
        ids,
        [
            "picture-home.folder",
            "picture-home.picture",
            "picture-home.fit",
            "picture-home.anchor",
            "picture-home.zoom",
            "picture-home.lighten",
            "picture-home.brightness",
            "picture-home.contrast",
            "picture-home.plate",
            "picture-home.render",
            "picture-home.bar",
            "picture-home.bar.at",
            "picture-home.bar.shape",
            "picture-home.bar.align",
            "picture-home.bar.look",
            "picture-home.hints",
            "picture-home.bar.left-items",
            "picture-home.bar.center-items",
            "picture-home.bar.right-items",
            "picture-home.columns",
            "picture-home.rows",
            "picture-home.dots",
            "picture-home.pencil",
            "picture-home.focus-at-once",
            "picture-home.page-1",
            "picture-home.page-2",
            "picture-home.page-3",
            "picture-home.page-4",
        ]
    );
    // Every setting is in the group; its title and help follow its name.
    for f in &found {
        assert_eq!(f.3, "picture-home.group", "{}", f.0);
        let short = f.0.trim_start_matches("picture-home.");
        if short.starts_with("page-") {
            assert_eq!(f.2.as_deref(), Some("picture-home.setting.page.help"));
        } else if short.ends_with("-items") {
            assert_eq!(f.2.as_deref(), Some("picture-home.setting.items.help"));
        } else {
            let key = short.replace('.', "-");
            assert_eq!(
                f.2.as_deref(),
                Some(format!("picture-home.setting.{key}.help").as_str()),
                "{}",
                f.0
            );
        }
    }
    values_match(&found);
    bars_match(&found);
    pages_and_grid_match(&found);
    assert_eq!(
        declarations::DEFAULTS[0],
        "home.clock, home.continue, home.recent"
    );
    native_batch_matches(&found);
}

/// The native batch, decoded, names the same settings in the same order:
/// name, group, title, help, kind, the advanced flag, the number range and
/// the choices.
fn native_batch_matches(found: &[Found]) {
    let batches = batches(|d| declarations::settings(d).unwrap());
    let mut specs = Vec::new();
    for batch in &batches {
        let records = records_decode(batch).unwrap();
        for record in records.iter() {
            if let Record::SettingSpec(spec) = record {
                specs.push(format!(
                    "picture-home.{} {} {} {} kind={} advanced={} {}..{} {:?}",
                    spec.name,
                    spec.group,
                    spec.title,
                    spec.help,
                    spec.kind,
                    spec.flags & (1 << 4) != 0,
                    spec.min,
                    spec.max,
                    spec.choices
                ));
            }
        }
    }
    assert_eq!(specs.len(), found.len());
    for (spec, f) in specs.iter().zip(found) {
        let head = format!("{} {} {} {}", f.0, f.3, f.1, f.2.as_deref().unwrap());
        assert!(spec.starts_with(&head), "{spec} vs {head}");
        assert_eq!(spec.contains("advanced=true"), f.4.ends_with("Advanced"));
        // The range of a number and the number of choices.
        if let Some(range) = f.4.strip_prefix("Int { min: ") {
            let (min, rest) = range.split_once(", max: ").unwrap();
            let max = rest.split_once(' ').unwrap().0;
            assert!(spec.contains(&format!(" {min}..{max} ")), "{spec}");
        }
    }
}

type Found = (
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    String,
);

fn values_match(found: &[Found]) {
    let by = |id: &str| found.iter().find(|f| f.0 == id).unwrap();
    assert_eq!(
        by("picture-home.folder").4,
        "Text Text(\"wallpapers\") Basic"
    );
    assert_eq!(
        by("picture-home.folder").5,
        "Some(Slot(Id(core.choose-folder)))"
    );
    assert_eq!(
        by("picture-home.picture").5,
        "Some(Screen(Id(picture-home.pick)))"
    );
    assert!(
        by("picture-home.fit")
            .4
            .ends_with("Text(\"picture-home.fit.fill\") Basic")
    );
    assert!(
        by("picture-home.anchor")
            .4
            .ends_with("Text(\"picture-home.anchor.center\") Basic")
    );
    assert_eq!(
        by("picture-home.zoom").4,
        "Int { min: 100, max: 300 } Int(100) Basic"
    );
    assert_eq!(
        by("picture-home.lighten").4,
        "Int { min: 0, max: 60 } Int(15) Basic"
    );
    assert!(
        by("picture-home.brightness")
            .4
            .contains("picture-home.brightness.normal\") Basic")
    );
    assert!(
        by("picture-home.contrast")
            .4
            .contains("picture-home.contrast.normal\") Basic")
    );
    assert!(by("picture-home.plate").4.ends_with(" Basic"));
    assert!(
        by("picture-home.render")
            .4
            .contains("picture-home.render.auto\") Basic")
    );
}

fn bars_match(found: &[Found]) {
    let by = |id: &str| found.iter().find(|f| f.0 == id).unwrap();
    assert!(
        by("picture-home.bar")
            .4
            .contains("picture-home.bar.on\") Basic")
    );
    assert!(
        by("picture-home.bar.at")
            .4
            .contains("picture-home.bar.top\") Basic")
    );
    assert!(
        by("picture-home.bar.shape")
            .4
            .contains("picture-home.bar.full\") Basic")
    );
    assert!(
        by("picture-home.bar.align")
            .4
            .contains("picture-home.bar.right\") Basic")
    );
    assert!(
        by("picture-home.bar.look")
            .4
            .contains("picture-home.look.normal\") Basic")
    );
    assert!(
        by("picture-home.hints")
            .4
            .contains("picture-home.bar.on\") Advanced")
    );
    assert_eq!(
        by("picture-home.bar.align").6,
        "Some((Id(picture-home.bar.shape), \"picture-home.bar.compact\"))"
    );
}

fn pages_and_grid_match(found: &[Found]) {
    let by = |id: &str| found.iter().find(|f| f.0 == id).unwrap();
    for n in 1..=4 {
        let page = by(&format!("picture-home.page-{n}"));
        assert!(page.4.ends_with("Advanced"), "{n}");
        assert_eq!(page.5, "Some(Screen(Id(picture-home.dashboard)))");
    }
    assert_eq!(
        by("picture-home.page-1").4,
        "Text Text(\"home.clock, home.continue, home.recent\") Advanced"
    );
    assert_eq!(
        by("picture-home.page-2").4,
        "Text Text(\"home.tools\") Advanced"
    );
    assert_eq!(
        by("picture-home.columns").4,
        "Int { min: 2, max: 6 } Int(3) Basic"
    );
    assert_eq!(
        by("picture-home.rows").4,
        "Int { min: 0, max: 12 } Int(0) Basic"
    );
    assert_eq!(by("picture-home.dots").4, "Bool Bool(true) Basic");
    assert_eq!(by("picture-home.pencil").4, "Bool Bool(true) Basic");
    assert_eq!(by("picture-home.focus-at-once").4, "Bool Bool(false) Basic");
    assert_eq!(
        by("picture-home.bar.left-items").4,
        "List Text(\"core.status.clock\") Basic"
    );
    assert_eq!(
        by("picture-home.bar.center-items").4,
        "List Text(\"\") Basic"
    );
    assert!(
        by("picture-home.bar.right-items")
            .4
            .contains("wifi.status, power.status")
    );
    // The titles are the keys the legacy registration has.
    assert_eq!(by("picture-home.fit").1, "picture-home.setting.fit");
    assert_eq!(
        by("picture-home.bar.left-items").1,
        "picture-home.setting.bar-left"
    );
    assert_eq!(
        by("picture-home.focus-at-once").1,
        "picture-home.setting.focus-at-once"
    );
}

#[test]
fn screens_offer_places_and_need_match() {
    let (mut screens, mut slots, mut icons) = (Vec::new(), Vec::new(), Vec::new());
    let (mut places, mut statuses, mut needs) = (Vec::new(), Vec::new(), Vec::new());
    for contribution in legacy() {
        match contribution {
            Contribution::FixedScreens(list) => {
                for s in list.iter() {
                    screens.push((s.id().as_str().to_string(), s.title().to_string()));
                }
            }
            Contribution::SlotBinding {
                slot,
                screen,
                priority,
                ..
            } => slots.push((slot.to_string(), screen.to_string(), priority.0)),
            Contribution::Icon { item, icon, .. } => {
                icons.push((item.as_str().to_string(), icon.to_string()));
            }
            Contribution::WidgetPlace(p) => places.push((
                p.id.as_str().to_string(),
                p.title.to_string(),
                p.settings
                    .iter()
                    .map(|s| s.as_str().to_string())
                    .collect::<Vec<_>>(),
            )),
            Contribution::StatusPlace(p) => statuses.push((
                p.id.as_str().to_string(),
                p.title.to_string(),
                p.parts
                    .iter()
                    .map(|s| s.as_str().to_string())
                    .collect::<Vec<_>>(),
            )),
            Contribution::MenuNeed { item, need, .. } => {
                needs.push((item.as_str().to_string(), need));
            }
            _ => {}
        }
    }
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    assert_eq!(
        screens,
        [
            pair("picture-home.dashboard", "picture-home.title"),
            pair("picture-home.add", "picture-home.add"),
            pair("picture-home.pick", "picture-home.setting.picture"),
        ]
    );
    assert_eq!(
        slots,
        [(
            "core.home".to_string(),
            "picture-home.dashboard".to_string(),
            declarations::PREFERRED as i16
        )]
    );
    assert!(icons.contains(&pair("picture-home.group", "layout-grid")));
    assert_eq!(places.len(), 1);
    assert_eq!(places[0].0, "picture-home.pages");
    assert_eq!(places[0].1, "picture-home.title");
    assert_eq!(
        places[0].2,
        [
            "picture-home.page-1",
            "picture-home.page-2",
            "picture-home.page-3",
            "picture-home.page-4"
        ]
    );
    assert_eq!(
        statuses,
        [(
            "picture-home.bar".to_string(),
            "picture-home.setting.bar".to_string(),
            vec![
                "picture-home.bar.left-items".to_string(),
                "picture-home.bar.center-items".to_string(),
                "picture-home.bar.right-items".to_string(),
            ]
        )]
    );
    assert_eq!(
        needs,
        [("picture-home.focus-at-once".to_string(), MenuNeed::NoTouch)]
    );
    assert_eq!(batches(|d| declarations::screens(d).unwrap()).len(), 1);
    assert!(!batches(|d| declarations::navigation(d).unwrap()).is_empty());
}
