//! The native declarations name what the interface-6 registration names: the
//! setting (kind, range, default, flags, title, help, the part and step it is
//! edited in), the screen and its instance policy, the actions, the place in
//! the tools and the icons; and the host's rules accept or refuse them as they
//! should. The headlines widget, its group and its fourteen settings are
//! compared the same way (order and keys of Beta 3).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_plugin_news::News;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("news"));
    News.register(&mut r).unwrap();
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

/// What the batches say, decoded and checked by the host's rules, is what the
/// interface-6 registration says.
#[test]
fn the_decoded_batches_say_what_the_registration_says() {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::settings(d).unwrap()));
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    all.extend(batches(|d| declarations::widgets(d).unwrap()));
    let native = declared::native("news", &all);
    let old = declared::legacy("news", "core.group.library", legacy());
    // The widget's group and its fourteen settings, after the one in the library's.
    let widget = declared::legacy("news", "news.widget.headlines", legacy());
    assert_eq!(native.groups, widget.groups);
    assert_eq!(
        native.groups,
        ["widget.headlines|news.widget.headlines|0||"]
    );
    assert_eq!(old.specs.len(), 1);
    assert_eq!(widget.specs.len(), 14);
    assert_eq!(native.specs, [old.specs.clone(), widget.specs].concat());
    // Paper and ink keep colours their choices lack (the widget's extras).
    assert_eq!(native.extras, [old.extras.clone(), widget.extras].concat());
    assert_eq!(native.extras.len(), 3);
    assert!(native.extras[0].contains("news.plugin.name"));
    assert_eq!(native.screens, old.screens);
    assert_eq!(native.actions, old.actions);
    assert_eq!(native.menu_entries, old.menu_entries);
    assert_eq!(native.menu_entries, ["core.tools|news.open|44"]);
    assert_eq!(native.icons, old.icons);
    assert_eq!(native.icons.len(), 2);
    // The widget record is the one the comparison does not read.
    assert_eq!(native.others, ["WidgetSpec"]);
}

/// The list opens one at a time: the decoded `screen.spec` flags (bits 6 and
/// 7: 0 per parameters, 1 single, 2 many) say what the registration says.
#[test]
fn the_list_opens_one_at_a_time() {
    let mut old = Vec::new();
    for contribution in legacy() {
        if let Contribution::FixedScreens(list) = contribution {
            for s in list.iter() {
                let policy = match s.how_many() {
                    i if i == paperpaw_sdk::Instances::PER_PARAMS => 0,
                    i if i == paperpaw_sdk::Instances::SINGLE => 1,
                    i if i == paperpaw_sdk::Instances::MANY => 2,
                    other => panic!("{other:?}"),
                };
                old.push((s.id().as_str().to_string(), policy));
            }
        }
    }
    let mut new = Vec::new();
    for batch in batches(|d| declarations::screens(d).unwrap()) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::ScreenSpec(s) = record {
                new.push((format!("news.{}", s.name), (s.flags >> 6) & 3));
            }
        }
    }
    assert_eq!(new, old);
    assert_eq!(new, [("news.list".to_string(), 1)]);
}
