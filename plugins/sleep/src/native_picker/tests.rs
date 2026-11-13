use super::*;
use alloc::string::ToString;

fn preset(id: &str) -> Source {
    Source::Preset(id.to_string())
}

fn widget(id: &str) -> Source {
    Source::Widget(id.to_string())
}

#[test]
fn the_folder_is_chosen_where_no_file_and_no_widget_is_named() {
    assert!(marked(&Source::Folder, "", "", false));
    assert!(!marked(&Source::Folder, "", "plugins/owls/owl.png", false));
    assert!(!marked(&Source::Folder, "home.date", "", false));
}

#[test]
fn a_widget_chosen_is_the_only_row_marked() {
    assert!(marked(&widget("home.date"), "home.date", "", false));
    assert!(!marked(&widget("home.recent"), "home.date", "", false));
    // A preset whose values the settings happen to have is not chosen over it.
    assert!(!marked(&preset("owls.big"), "home.date", "", true));
    assert!(!marked(&Source::Folder, "home.date", "", false));
}

#[test]
fn a_preset_is_chosen_where_the_settings_have_its_values() {
    assert!(marked(
        &preset("owls.big"),
        "",
        "plugins/owls/owl.png",
        true
    ));
    assert!(!marked(
        &preset("owls.big"),
        "",
        "plugins/owls/owl.png",
        false
    ));
    // No widget is chosen while no widget is named.
    assert!(!marked(&widget("home.date"), "", "", false));
}

#[test]
fn a_value_is_had_when_it_is_the_same_or_is_left_out() {
    use SettingValue::{Bool, Int, Text};
    assert!(same(Some(Int(35)), Int(35)));
    assert!(!same(Some(Int(35)), Int(40)));
    assert!(same(Some(Text("a.png")), Text("a.png")));
    assert!(!same(Some(Text("a.png")), Text("")));
    assert!(!same(Some(Bool(true)), Int(1)));
    // A reset or a value too long to read says nothing against it.
    assert!(same(None, Text("anything")));
    assert!(same(None, SettingValue::Absent));
}
