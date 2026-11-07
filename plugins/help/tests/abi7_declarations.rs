//! Settings and screen identity parity with the interface-6 registration;
//! callback acceptance is separate.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script also uses the event declarations"
)]
mod declarations;
use paperpaw_sdk::native::declare::{
    Declarations, EditorDecl, ExtraDecl, Instances, Kind, ScreenDecl, SpecDecl,
};
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar, SettingKind, SettingValue};

fn instances(value: paperpaw_sdk::Instances) -> Instances {
    if value == paperpaw_sdk::Instances::SINGLE {
        Instances::Single
    } else if value == paperpaw_sdk::Instances::MANY {
        Instances::Many
    } else {
        assert_eq!(value, paperpaw_sdk::Instances::PER_PARAMS);
        Instances::PerParams
    }
}

fn contributions() -> Vec<Contribution> {
    let mut registrar = Registrar::new(Id::new_static("help"));
    paperpaw_plugin_help::Help.register(&mut registrar).unwrap();
    registrar.into_contributions()
}

#[test]
fn screen_identity_matches_existing_registration() {
    let mut expected = Vec::new();
    {
        let mut d = Declarations::new(|b: &[u8]| {
            expected.push(b.to_vec());
            Ok(())
        });
        let mut token = 10;
        for c in contributions() {
            if let Contribution::FixedScreens(screens) = c {
                for s in screens {
                    let mut decl = ScreenDecl::new(
                        s.id().as_str().strip_prefix("help.").unwrap(),
                        s.title(),
                        token,
                    )
                    .instances(instances(s.how_many()));
                    // The hints show through: the one screen that is an overlay.
                    if s.id().as_str() == "help.overlay" {
                        decl = decl.overlay();
                    }
                    d.screen(decl).unwrap();
                    token += 1;
                }
            }
        }
        d.submit().unwrap();
    }
    let mut actual = Vec::new();
    {
        let mut d = Declarations::new(|b: &[u8]| {
            actual.push(b.to_vec());
            Ok(())
        });
        declarations::screens(&mut d).unwrap();
    }
    assert_eq!(actual, expected);
}

#[test]
fn settings_match_existing_registration() {
    let mut expected = Vec::new();
    {
        let mut d = Declarations::new(|b: &[u8]| {
            expected.push(b.to_vec());
            Ok(())
        });
        let specs: Vec<_> = contributions()
            .into_iter()
            .filter_map(|c| match c {
                Contribution::FixedSettings(list) => Some(list),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(specs.len(), 2);
        for spec in specs {
            let name = spec.id.as_str().strip_prefix("help.").unwrap();
            let kind = match (&spec.kind, &spec.default) {
                (SettingKind::Bool, SettingValue::Bool(on)) => Kind::Bool(*on),
                (SettingKind::Text, SettingValue::Text(text)) => Kind::Text(text),
                other => panic!("unexpected kind {other:?}"),
            };
            let mut decl = SpecDecl::new(name, kind, spec.group.as_str(), &spec.title);
            decl.help = spec.description.as_deref().unwrap_or("");
            d.spec(decl).unwrap();
        }
        for spec in contributions()
            .into_iter()
            .filter_map(|c| match c {
                Contribution::FixedSettings(list) => Some(list),
                _ => None,
            })
            .flatten()
        {
            let extra = spec.extra();
            let mut decl = ExtraDecl::new(spec.id.as_str().strip_prefix("help.").unwrap());
            decl.section = extra.section.as_deref().unwrap_or("");
            if let Some(editor) = &extra.editor {
                let screen = format!("{editor:?}");
                assert!(screen.contains("help.again"), "{screen}");
                decl.editor = Some(EditorDecl::Screen("again"));
            }
            d.extra(decl).unwrap();
        }
        d.submit().unwrap();
    }
    let mut actual = Vec::new();
    {
        let mut d = Declarations::new(|b: &[u8]| {
            actual.push(b.to_vec());
            Ok(())
        });
        declarations::settings(&mut d).unwrap();
    }
    assert_eq!(actual, expected);
}
