//! The native declarations name what the interface-6 registration names: the
//! screens, the catalog setting, the actions with their menu places and
//! icons, and the events. Callbacks are a later slice (`PORTING-7.md`).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
mod declarations;
#[path = "../../test-support/native_navigation.rs"]
mod navigation;
use paperpaw_plugin_fonts_browser::FontsBrowser;
use paperpaw_sdk::native::declare::{Declarations, Instances, ScreenDecl};
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar, SettingKind};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("fonts-browser"));
    FontsBrowser.register(&mut r).unwrap();
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
fn screens_match() {
    let mut expected = Vec::new();
    for contribution in legacy() {
        if let Contribution::FixedScreens(list) = contribution {
            for (token, s) in (10..).zip(list.iter()) {
                let instances = if s.how_many() == paperpaw_sdk::Instances::SINGLE {
                    Instances::Single
                } else if s.how_many() == paperpaw_sdk::Instances::MANY {
                    Instances::Many
                } else {
                    assert_eq!(s.how_many(), paperpaw_sdk::Instances::PER_PARAMS);
                    Instances::PerParams
                };
                expected.push((
                    s.id()
                        .as_str()
                        .strip_prefix("fonts-browser.")
                        .unwrap()
                        .to_string(),
                    s.title().to_string(),
                    token,
                    instances,
                ));
            }
        }
    }
    let mut want = Vec::new();
    {
        let mut d = Declarations::new(|b: &[u8]| {
            want.push(b.to_vec());
            Ok(())
        });
        for (name, title, token, instances) in &expected {
            d.screen(ScreenDecl::new(name, title, *token).instances(*instances))
                .unwrap();
        }
        d.submit().unwrap();
    }
    assert_eq!(batches(|d| declarations::screens(d).unwrap()), want);
}

#[test]
fn the_catalog_setting_matches() {
    let mut found = Vec::new();
    for contribution in legacy() {
        if let Contribution::FixedSettings(settings) = contribution {
            for spec in settings.iter() {
                assert!(matches!(spec.kind, SettingKind::Text));
                found.push((
                    spec.id.as_str().to_string(),
                    spec.title.to_string(),
                    spec.description.as_deref().map(str::to_string),
                    spec.group.as_str().to_string(),
                    format!("{:?}", spec.default),
                ));
            }
        }
    }
    assert_eq!(
        found,
        [(
            "fonts-browser.catalog".to_string(),
            "fonts-browser.setting.catalog".to_string(),
            Some("fonts-browser.setting.catalog.help".to_string()),
            "core.group.developer".to_string(),
            "Text(\"https://paperpaw.org/api/v1/fonts.json\")".to_string(),
        )]
    );
    assert_eq!(batches(|d| declarations::settings(d).unwrap()).len(), 1);
}

#[test]
fn actions_menus_and_icons_match() {
    let expected = navigation::encode("fonts-browser", legacy());
    assert_eq!(batches(|d| declarations::navigation(d).unwrap()), expected);
}

#[test]
fn the_events_are_the_download_and_the_start() {
    // Interface 6: `install::DOWNLOADED` and `events::STARTED`.
    assert_eq!(batches(|d| declarations::events(d).unwrap()).len(), 1);
}
