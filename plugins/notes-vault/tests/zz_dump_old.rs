#![allow(clippy::unwrap_used, dead_code)]
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_plugin_notes_vault::NotesVault;
use paperpaw_sdk::native::declare::{Declarations, ExtraDecl, GroupDecl, Kind, SpecDecl, WidgetDecl};
use paperpaw_sdk::{Choices, Contribution, Id, Plugin, Registrar, SettingKind, SettingValue};

#[path = "../declarations.rs"]
mod declarations;

fn records(f: impl FnOnce(&mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>)) -> Vec<String> {
    let mut batches: Vec<Vec<u8>> = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            batches.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        f(&mut d);
    }
    let mut out = Vec::new();
    for b in batches {
        for r in records_decode(&b).unwrap().iter() {
            match r {
                Record::WidgetSpec(w) => {
                    let mut w = w;
                    w.previews = "";
                    out.push(format!("{w:?}"));
                }
                other => out.push(format!("{other:?}")),
            }
        }
    }
    out.sort();
    out
}

#[test]
fn widgets_are_as_interface_6_registered_them() {
    let mut r = Registrar::new(Id::new_static("notes-vault"));
    NotesVault.register(&mut r).unwrap();
    let mut groups = Vec::new();
    let mut specs = Vec::new();
    let mut widgets = Vec::new();
    for c in r.into_contributions() {
        match c {
            Contribution::SettingGroup(g) if g.id.as_str() != "notes-vault.group" => {
                groups.push((g.id.as_str().to_string(), g.title.to_string()));
            }
            Contribution::FixedSettings(list) => {
                for s in list.iter().filter(|s| s.id.as_str() != "notes-vault.folder") {
                    specs.push((
                        s.id.as_str().to_string(),
                        s.title.to_string(),
                        s.description.as_ref().map(ToString::to_string).unwrap_or_default(),
                        s.group.as_str().to_string(),
                        s.kind.clone(),
                        s.default.clone(),
                        s.extra().keeps_unknown,
                    ));
                }
            }
            Contribution::Widget(w) => widgets.push(format!("{w:?}")),
            _ => {}
        }
    }
    let old = records(|d| {
        for (id, title) in &groups {
            d.group(GroupDecl::new(id.strip_prefix("notes-vault.").unwrap(), title)).unwrap();
        }
        for (id, title, help, group, kind, default, keeps) in &specs {
            let name = id.strip_prefix("notes-vault.").unwrap();
            let kind = match (kind, default) {
                (SettingKind::Bool, SettingValue::Bool(v)) => Kind::Bool(*v),
                (SettingKind::Int { min, max }, SettingValue::Int(v)) => Kind::Int { default: *v, min: *min as i32, max: *max as i32 },
                (SettingKind::Text, SettingValue::Text(t)) => Kind::Text(Box::leak(t.to_string().into_boxed_str())),
                (SettingKind::Choice(Choices::Fixed(list)), SettingValue::Text(t)) => Kind::Choice { default: Box::leak(t.to_string().into_boxed_str()), choices: list },
                other => panic!("{other:?}"),
            };
            let mut spec = SpecDecl::new(name, kind, group, title);
            spec.help = help;
            d.spec(spec).unwrap();
            if *keeps {
                let mut e = ExtraDecl::new(name);
                e.keeps_unknown = true;
                d.extra(e).unwrap();
            }
        }
        for (name, token, cells) in [("recent", 40, (2, 2)), ("pinned", 41, (2, 1)), ("day", 42, (2, 1)), ("random", 43, (2, 1)), ("tags", 44, (2, 1)), ("search", 45, (2, 1))] {
            let title = format!("notes-vault.{name}");
            d.widget(WidgetDecl::new(name, &title, "core.widgets.personal", token).unwrap().cells(cells.0, cells.1)).unwrap();
        }
        d.submit().unwrap();
    });
    let new = records(|d| declarations::widgets(d).unwrap());
    assert_eq!(old.len(), new.len());
    for (a, b) in old.iter().zip(&new) {
        assert_eq!(a, b);
    }
    println!("{} records equal", old.len());
}
