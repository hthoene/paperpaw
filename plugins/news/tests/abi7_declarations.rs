//! The native declarations name what the interface-6 registration named (kept
//! as data, `interface_6`, since the plugin is gone): the
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
use paperpaw_sdk::native::declare::Declarations;

/// What the interface-6 registration of the plugin declared (it was decoded
/// with `declared::legacy` before the plugin was removed), kept as data.
mod interface_6 {
    pub const LIBRARY_SPECS: &[&str] = &[
        "articles|1|16|core.group.library|news.setting.articles|news.setting.articles.help|i:30|5|200|\"\"",
    ];
    pub const LIBRARY_EXTRAS: &[&str] = &["articles||news.plugin.name||=|0:|5|0"];
    pub const WIDGET_SPECS: &[&str] = &[
        "widget.headlines.frame|2|0|news.widget.headlines|core.setting.frame|core.setting.frame.help|t:core.frame.none|0|0|\"core.frame.none\\ncore.frame.line\\ncore.frame.card\\ncore.frame.dark\"",
        "widget.headlines.padding|2|0|news.widget.headlines|core.setting.padding|core.setting.padding.help|t:core.padding.auto|0|0|\"core.padding.auto\\ncore.padding.none\\ncore.padding.small\\ncore.padding.normal\\ncore.padding.large\"",
        "widget.headlines.paper|2|0|news.widget.headlines|core.setting.paper|core.setting.paper.help|t:core.plate.normal|0|0|\"core.plate.normal\\ncore.plate.inverted\\ncore.plate.light-gray\\ncore.plate.dark-gray\\ncore.plate.none\"",
        "widget.headlines.ink|2|0|news.widget.headlines|core.setting.ink|core.setting.ink.help|t:core.ink.auto|0|0|\"core.ink.auto\\ncore.tone.black\\ncore.tone.white\"",
        "widget.headlines.corners|2|0|news.widget.headlines|core.setting.corners|core.setting.corners.help|t:core.corners.auto|0|0|\"core.corners.auto\\ncore.corners.square\\ncore.corners.small\\ncore.corners.round\\ncore.corners.soft\"",
        "widget.headlines.frame-line|2|0|news.widget.headlines|core.setting.frame-line|core.setting.frame-line.help|t:core.lines.auto|0|0|\"core.lines.auto\\ncore.lines.thin\\ncore.lines.medium\\ncore.lines.thick\"",
        "widget.headlines.drop-shadow|2|0|news.widget.headlines|core.setting.shadow|core.setting.shadow.help|t:core.shadow.none|0|0|\"core.shadow.none\\ncore.shadow.hard\"",
        "widget.headlines.size|2|0|news.widget.headlines|core.setting.size|core.setting.size.help|t:core.size.small|0|0|\"core.size.small\\ncore.size.medium\\ncore.size.large\"",
        "widget.headlines.align|2|0|news.widget.headlines|core.setting.align|core.setting.align.help|t:core.align.left|0|0|\"core.align.left\\ncore.align.center\\ncore.align.right\"",
        "widget.headlines.count|2|0|news.widget.headlines|news.setting.count|news.setting.count.help|t:news.count.4|0|0|\"news.count.2\\nnews.count.3\\nnews.count.4\\nnews.count.6\\nnews.count.8\"",
        "widget.headlines.lines|2|0|news.widget.headlines|news.setting.lines|news.setting.lines.help|t:news.lines.1|0|0|\"news.lines.1\\nnews.lines.2\"",
        "widget.headlines.caption|0|0|news.widget.headlines|news.part.caption|core.setting.part.help|b:1|0|0|\"\"",
        "widget.headlines.source|0|0|news.widget.headlines|news.part.source|core.setting.part.help|b:0|0|0|\"\"",
        "widget.headlines.rules|0|0|news.widget.headlines|news.part.rules|core.setting.part.help|b:1|0|0|\"\"",
    ];
    pub const WIDGET_EXTRAS: &[&str] = &[
        "widget.headlines.paper||||=|0:|0|4",
        "widget.headlines.ink||||=|0:|0|4",
    ];
    pub const WIDGET_GROUPS: &[&str] = &["widget.headlines|news.widget.headlines|0||"];
    pub const SCREENS: &[&str] = &["list|news.title"];
    pub const ACTIONS: &[&str] = &["fetch|news.fetch", "open|news.title"];
    pub const MENU_ENTRIES: &[&str] = &["core.tools|news.open|44"];
    pub const ICONS: &[&str] = &["news.fetch|refresh", "news.open|rss"];
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
/// interface-6 registration said.
#[test]
fn the_decoded_batches_say_what_the_registration_said() {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::settings(d).unwrap()));
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    all.extend(batches(|d| declarations::widgets(d).unwrap()));
    let native = declared::native("news", &all);
    // The widget's group and its fourteen settings, after the one in the library's.
    assert_eq!(native.groups, interface_6::WIDGET_GROUPS);
    assert_eq!(
        native.groups,
        ["widget.headlines|news.widget.headlines|0||"]
    );
    assert_eq!(interface_6::LIBRARY_SPECS.len(), 1);
    assert_eq!(interface_6::WIDGET_SPECS.len(), 14);
    assert_eq!(
        native.specs,
        [interface_6::LIBRARY_SPECS, interface_6::WIDGET_SPECS].concat()
    );
    // Paper and ink keep colours their choices lack (the widget's extras).
    assert_eq!(
        native.extras,
        [interface_6::LIBRARY_EXTRAS, interface_6::WIDGET_EXTRAS].concat()
    );
    assert_eq!(native.extras.len(), 3);
    assert!(native.extras[0].contains("news.plugin.name"));
    assert_eq!(native.screens, interface_6::SCREENS);
    assert_eq!(native.actions, interface_6::ACTIONS);
    assert_eq!(native.menu_entries, interface_6::MENU_ENTRIES);
    assert_eq!(native.menu_entries, ["core.tools|news.open|44"]);
    assert_eq!(native.icons, interface_6::ICONS);
    assert_eq!(native.icons.len(), 2);
    // The widget record is the one the comparison does not read.
    assert_eq!(native.others, ["WidgetSpec"]);
}

/// The list opens one at a time, as the interface-6 screen did (single
/// instance): the decoded `screen.spec` flags (bits 6 and 7: 0 per parameters,
/// 1 single, 2 many).
#[test]
fn the_list_opens_one_at_a_time() {
    let mut new = Vec::new();
    for batch in batches(|d| declarations::screens(d).unwrap()) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::ScreenSpec(s) = record {
                new.push((format!("news.{}", s.name), (s.flags >> 6) & 3));
            }
        }
    }
    assert_eq!(new, [("news.list".to_string(), 1)]);
}
