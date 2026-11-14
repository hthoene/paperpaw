use super::*;
use alloc::string::ToString;
use alloc::vec;

fn group(id: &str, parent: &str, order: i32, title: &str) -> Group {
    Group {
        id: id.to_string(),
        parent: parent.to_string(),
        page: String::new(),
        order,
        title: title.to_string(),
    }
}

fn setting(key: &str, group: &str, kind: Kind, flags: u32) -> Setting {
    Setting {
        key: key.to_string(),
        group: group.to_string(),
        kind,
        flags,
        is_default: true,
        in_scope: false,
        title: key.to_string(),
        value: None,
    }
}

const SHOWN: u32 = flag::SHOWN;

#[test]
fn a_setting_is_shown_in_its_group_or_falls_into_other() {
    let groups = vec![group("a.one", "", 0, "One"), group("a.two", "", 0, "Two")];
    let in_one = setting("t.x", "a.one", Kind::Bool, SHOWN);
    let nowhere = setting("t.y", "b.nowhere", Kind::Bool, SHOWN);
    assert!(group_is(&groups, &in_one, "a.one"));
    assert!(!group_is(&groups, &in_one, "a.two"));
    assert!(!group_is(&groups, &in_one, OTHER_GROUP));
    // A group that is not registered shows nothing of its own.
    assert!(!group_is(&groups, &nowhere, "b.nowhere"));
    assert!(group_is(&groups, &nowhere, OTHER_GROUP));
    // The other group's own settings are shown there.
    let other = setting("t.z", OTHER_GROUP, Kind::Bool, SHOWN);
    assert!(group_is(&groups, &other, OTHER_GROUP));
}

#[test]
fn a_part_is_within_its_parent_at_any_depth() {
    let groups = vec![
        group("a", "", 0, "A"),
        group("a.b", "a", 0, "B"),
        group("a.b.c", "a.b", 0, "C"),
        group("loop.x", "loop.y", 0, "X"),
        group("loop.y", "loop.x", 0, "Y"),
    ];
    assert!(within(&groups, "a.b.c", "a"));
    assert!(within(&groups, "a", "a"));
    assert!(!within(&groups, "a", "a.b"));
    // A damaged cycle ends instead of looping.
    assert!(!within(&groups, "loop.x", "a"));
}

#[test]
fn offered_and_visible_follow_the_levels() {
    let groups = vec![group("g", "", 0, "G"), group(DEVELOPER_GROUP, "", 0, "Dev")];
    let cx = Context::default();
    let basic = setting("g.a", "g", Kind::Bool, SHOWN);
    let advanced = setting("g.b", "g", Kind::Bool, SHOWN | flag::ADVANCED);
    let hidden = setting("g.c", "g", Kind::Bool, 0);
    let dev = setting("g.d", DEVELOPER_GROUP, Kind::Bool, SHOWN | flag::ADVANCED);
    assert!(visible(&groups, cx, &basic, None));
    assert!(!visible(&groups, cx, &advanced, None));
    assert!(offered(&groups, cx, &advanced, None));
    assert!(visible(
        &groups,
        Context {
            advanced: true,
            ..cx
        },
        &advanced,
        None
    ));
    assert!(
        !offered(&groups, cx, &hidden, None),
        "its shown_if does not hold"
    );
    // Developer settings: once asked for, and then without the advanced switch.
    assert!(!visible(&groups, cx, &dev, None));
    assert!(visible(
        &groups,
        Context {
            developer: true,
            ..cx
        },
        &dev,
        None
    ));
    // Only this book means nothing outside a scope.
    let only = setting(SCOPE_ONLY, "g", Kind::Bool, SHOWN);
    assert!(!offered(&groups, cx, &only, None));
    assert!(offered(
        &groups,
        Context { scoped: true, ..cx },
        &only,
        None
    ));
    // Nothing to choose.
    let choice = setting("g.e", "g", Kind::Choice, SHOWN);
    assert!(!offered(&groups, cx, &choice, Some(1)));
    assert!(offered(&groups, cx, &choice, Some(2)));
    assert!(offered(&groups, cx, &choice, None));
    // A secret is kept elsewhere.
    let secret = setting("g.f", "g", Kind::Text, SHOWN | flag::SECRET);
    assert!(!offered(&groups, cx, &secret, None));
}

#[test]
fn the_main_page_lists_groups_by_order_then_name() {
    let groups = vec![
        group("g.late", "", 0, "Zeta"),
        group("g.first", "", 5, "Beta"),
        group("g.also", "", 0, "Alpha"),
        group("g.part", "g.first", 0, "Part"),
        group("g.empty", "", 1, "Empty"),
        group("w.clock", "", 2, "Clock"),
    ];
    let holding = vec![
        "g.late".to_string(),
        "g.first".to_string(),
        "g.part".to_string(),
        "g.also".to_string(),
        "w.clock".to_string(),
    ];
    let found = main_groups(&groups, &holding, |id| id.starts_with("w.")).unwrap();
    let ids: Vec<&str> = found.iter().map(|i| groups[*i].id.as_str()).collect();
    // Beta (order 5), then those without an order by name; no part, no empty
    // group, no widget.
    assert_eq!(ids, ["g.first", "g.also", "g.late"]);
    // A group holding only through a part is listed.
    let holding = vec!["g.part".to_string()];
    let found = main_groups(&groups, &holding, |_| false).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(groups[found[0]].id, "g.first");
}

#[test]
fn sections_come_standard_first() {
    use paperpaw_sdk::builtin::group_sections as s;
    let order =
        section_order(["z.own", s::PAGE, "", s::FONT, s::PAGE, "a.own"].into_iter()).unwrap();
    assert_eq!(order, ["", s::FONT, s::PAGE, "z.own", "a.own"]);
}

fn int_detail(min: i64, max: i64, step: i64) -> Detail {
    Detail {
        min,
        max,
        step,
        ..Detail::default()
    }
}

#[test]
fn numbers_step_by_their_step_and_stop_at_their_ends() {
    let d = int_detail(1, 10, 2);
    assert_eq!(stepped(Kind::Int, &Val::Int(5), &d, 1), Some(Val::Int(7)));
    assert_eq!(stepped(Kind::Int, &Val::Int(5), &d, -1), Some(Val::Int(3)));
    assert_eq!(stepped(Kind::Int, &Val::Int(9), &d, 1), Some(Val::Int(10)));
    assert_eq!(stepped(Kind::Int, &Val::Int(10), &d, 1), None);
    assert_eq!(stepped(Kind::Int, &Val::Int(1), &d, -1), None);
    // A step of 0 and 1 are one step.
    assert_eq!(
        stepped(Kind::Int, &Val::Int(5), &int_detail(0, 9, 0), 1),
        Some(Val::Int(6))
    );
    // Overflow stops at the end.
    assert_eq!(
        stepped(
            Kind::Int,
            &Val::Int(i64::MAX),
            &int_detail(0, i64::MAX, 5),
            1
        ),
        None
    );
    assert_eq!(
        stepped(Kind::Bool, &Val::Bool(true), &Detail::default(), 1),
        Some(Val::Bool(false))
    );
    assert_eq!(
        stepped(
            Kind::Text,
            &Val::Text("x".to_string()),
            &Detail::default(),
            1
        ),
        None
    );
}

#[test]
fn a_unit_with_a_count_takes_the_number() {
    assert_eq!(with_unit(5, "").unwrap(), "5");
    assert_eq!(with_unit(5, "{count} s").unwrap(), "5 s");
    assert_eq!(with_unit(-12, "{count} mm").unwrap(), "-12 mm");
    assert_eq!(with_unit(30, "min").unwrap(), "30 min");
    assert_eq!(with_unit(i64::MIN, "").unwrap(), "-9223372036854775808");
}

#[test]
fn placements_rename_the_settings_of_their_group() {
    assert_eq!(
        placed("widgets.clock.face", "widgets.clock", "widgets.clock@2")
            .unwrap()
            .unwrap(),
        "widgets.clock@2.face"
    );
    assert_eq!(
        placed("other.face", "widgets.clock", "widgets.clock@2").unwrap(),
        None
    );
    assert_eq!(
        placed("widgets.clockwork", "widgets.clock", "x@1").unwrap(),
        None
    );
}

#[test]
fn a_placement_id_names_its_widget_and_its_settings() {
    assert_eq!(unplaced("widgets.clock@2"), Some("widgets.clock"));
    assert_eq!(unplaced("widgets.clock@12"), Some("widgets.clock"));
    for none in [
        "widgets.clock",
        "widgets.clock@",
        "widgets.clock@x",
        "@2",
        "a@2b",
        "x@2@3",
        "",
    ] {
        assert_eq!(unplaced(none), None, "{none}");
    }
    assert_eq!(
        unplaced_key("widgets.clock@2.face").unwrap().as_deref(),
        Some("widgets.clock.face")
    );
    assert_eq!(
        unplaced_key("widgets.clock@12.frame.width")
            .unwrap()
            .as_deref(),
        Some("widgets.clock.frame.width")
    );
    for none in [
        "widgets.clock.face",
        "widgets.clock@.face",
        "@2.face",
        "x@y",
        "a@2b.x",
        "x@2@3.face",
        "x@2.f@3",
    ] {
        assert_eq!(unplaced_key(none).unwrap(), None, "{none}");
    }
    // The two are inverse: placed -> unplaced gives the key back.
    let placed_key = placed("widgets.clock.face", "widgets.clock", "widgets.clock@2")
        .unwrap()
        .unwrap();
    assert_eq!(
        unplaced_key(&placed_key).unwrap().as_deref(),
        Some("widgets.clock.face")
    );
}

#[test]
fn a_widget_look_covers_the_looks_of_the_widgets_there_are() {
    let widgets = [
        String::from("widgets.clock"),
        String::from("widgets.weather"),
    ];
    for key in [
        "widgets.clock.frame",
        "widgets.weather.drop-shadow",
        "widgets.clock.frame-line",
        "core.widgets.gap",
        "core.widgets.frame",
    ] {
        assert!(widget_look_covers(key, &widgets), "{key}");
    }
    for key in [
        // Not a widget there is, not a part a look covers, or a longer name.
        "widgets.news.frame",
        "widgets.clock.face",
        "widgets.clock.frames",
        "status.line.size",
        "core.widgets.other",
        "",
    ] {
        assert!(!widget_look_covers(key, &widgets), "{key}");
    }
    assert!(!widget_look_covers("widgets.clock.frame", &[]));
}

#[test]
fn every_word_counts() {
    let find = |text: &str, query: &str| matches(text, &words(query).unwrap()).unwrap();
    assert!(find("Font size of the book's text", "SIZE font"));
    assert!(!find("Font size", "font weight"));
    assert!(find("Schriftgröße", "größe"));
    assert!(find("Ränder", "rand"));
    assert!(find("Schriftgrösse", "grosse"));
    assert!(find("Café", "cafe"));
    assert!(words("   ").unwrap().is_empty());
}

#[test]
fn a_theme_is_in_use_when_its_values_are_set_and_the_rest_is_default() {
    let mut a = setting("look.a", "g", Kind::Choice, SHOWN);
    a.value = Some(Val::Text("x".to_string()));
    a.is_default = false;
    let b = setting("look.b", "g", Kind::Bool, SHOWN);
    let outside = setting("other", "g", Kind::Bool, SHOWN);
    let all = vec![a, b, outside];
    let covered = vec!["look.a".to_string(), "look.b".to_string()];
    let theme = vec![("look.a".to_string(), Some(Val::Text("x".to_string())))];
    assert!(applied(&all, &covered, &theme));
    let other = vec![("look.a".to_string(), Some(Val::Text("y".to_string())))];
    assert!(!applied(&all, &covered, &other));
    // A value a theme resets must be the default.
    let resets = vec![("look.a".to_string(), None)];
    assert!(!applied(&all, &covered, &resets));
    // A covered setting a theme leaves out must be the default.
    assert!(!applied(&all, &covered, &[]));
}

/// Every allocation of the model's helpers may be refused: they end in
/// `NoMemory` (or succeed), never abort; a copy is whole or an error.
#[test]
fn allocation_refusals_end_in_no_memory() {
    use crate::fail_alloc::sweep;
    let mut a = setting("look.a", "g", Kind::Choice, SHOWN);
    a.value = Some(Val::Text("x".repeat(600)));
    sweep(|| a.try_clone().map(|c| c.value == a.value));
    sweep(|| placed("widgets.clock.face", "widgets.clock", "widgets.clock@2").map(|p| p.is_some()));
    sweep(|| unplaced_key("widgets.clock@2.face").map(|k| k.is_some()));
    sweep(|| section_order(["b", "a", "b", ""].into_iter()).map(|s| s.len()));
    sweep(|| words("Brightness  sleep").map(|w| w.len()));
    sweep(|| {
        let w = words("bright")?;
        matches("Brightness of the light", &w)
    });
    sweep(|| with_unit(30, "{count} min").map(|t| t.len()));
}

/// A copy keeps a text value longer than the 512 bytes a short buffer holds.
#[test]
fn a_copy_of_a_long_text_value_is_whole() {
    let mut a = setting("t.long", "g", Kind::Text, SHOWN);
    let long = "é".repeat(2000);
    a.value = Some(Val::Text(long.clone()));
    assert_eq!(a.try_clone().unwrap().value, Some(Val::Text(long)));
}

/// A plugin with an account: a server typed in (kept on a reset), a password
/// (a secret) and a number (reset); the library folder is kept on reset too.
fn account_world() -> (Vec<Group>, Vec<Setting>) {
    let groups = vec![
        group("acct.page", "", 0, "Account"),
        group("core.g", "", 0, "Core"),
    ];
    let mut server = setting(
        "acct.server",
        "acct.page",
        Kind::Text,
        SHOWN | flag::KEPT_ON_RESET,
    );
    let mut password = setting(
        "acct.password",
        "acct.page",
        Kind::Text,
        SHOWN | flag::SECRET,
    );
    let mut count = setting("acct.count", "acct.page", Kind::Int, SHOWN);
    let mut folder = setting(
        "core.library-folder",
        "core.g",
        Kind::Text,
        SHOWN | flag::KEPT_ON_RESET,
    );
    let mut language = setting(LANGUAGE, "core.g", Kind::Language, SHOWN);
    let mut size = setting("core.font-size", "core.g", Kind::Int, SHOWN);
    for s in [
        &mut server,
        &mut password,
        &mut count,
        &mut folder,
        &mut language,
        &mut size,
    ] {
        s.is_default = false;
    }
    (
        groups,
        vec![server, password, count, folder, language, size],
    )
}

fn plan(groups: &[Group], all: &[Setting], of: &str) -> Vec<String> {
    reset_plan(groups, Context::default(), all, of).unwrap()
}

#[test]
fn a_page_reset_keeps_the_account_and_its_password() {
    let (groups, all) = account_world();
    // Only what the page holds and is not kept: the number. "Only this book"
    // goes last, always.
    assert_eq!(plan(&groups, &all, "acct.page"), ["acct.count", SCOPE_ONLY]);
    // Only "reset all" keeps the language; a page that holds it resets it.
    assert_eq!(
        plan(&groups, &all, "core.g"),
        [LANGUAGE, "core.font-size", SCOPE_ONLY]
    );
}

#[test]
fn everything_reset_keeps_accounts_folders_passwords_and_the_language() {
    let (groups, all) = account_world();
    assert_eq!(
        plan(&groups, &all, ""),
        ["acct.count", "core.font-size", SCOPE_ONLY]
    );
}

#[test]
fn reset_all_does_not_reload_unchanged_defaults() {
    let (groups, mut all) = account_world();
    // Back at its default: nothing to put back, no event, no read of the card.
    all[2].is_default = true;
    assert_eq!(plan(&groups, &all, ""), ["core.font-size", SCOPE_ONLY]);
    // But a value kept for the book alone is cleared though it equals the default.
    all[2].in_scope = true;
    assert_eq!(
        plan(&groups, &all, ""),
        ["acct.count", "core.font-size", SCOPE_ONLY]
    );
    // Nothing set at all: only the book scope's own switch goes.
    for s in &mut all {
        s.is_default = true;
        s.in_scope = false;
    }
    assert_eq!(plan(&groups, &all, ""), [SCOPE_ONLY]);
}

#[test]
fn resetting_a_books_values_clears_the_scope_last_so_none_fall_to_global_values() {
    let (groups, mut all) = account_world();
    // The book's switch is itself set, and listed among the settings.
    let mut scope = setting(SCOPE_ONLY, "core.g", Kind::Bool, SHOWN);
    scope.is_default = false;
    scope.in_scope = true;
    all.insert(0, scope);
    all[3].in_scope = true;
    for of in ["", "acct.page", "core.g"] {
        let keys = plan(&groups, &all, of);
        // Last, and once: clearing it earlier would send the later resets to
        // the global settings instead of the book's values.
        assert_eq!(keys.last().map(String::as_str), Some(SCOPE_ONLY), "{of:?}");
        assert_eq!(
            keys.iter().filter(|k| *k == SCOPE_ONLY).count(),
            1,
            "{of:?}"
        );
    }
}

#[test]
fn a_page_reset_leaves_what_the_page_does_not_show() {
    let (groups, mut all) = account_world();
    // An advanced setting of the page that is not shown stays; shown, goes.
    all[2].flags |= flag::ADVANCED;
    assert_eq!(plan(&groups, &all, "acct.page"), [SCOPE_ONLY]);
    let shown = Context {
        advanced: true,
        ..Context::default()
    };
    assert_eq!(
        reset_plan(&groups, shown, &all, "acct.page").unwrap(),
        ["acct.count", SCOPE_ONLY]
    );
    // A setting not offered at all (the developer's) is left on a page reset
    // and goes with everything.
    let mut dev = setting("dev.x", DEVELOPER_GROUP, Kind::Bool, SHOWN);
    dev.is_default = false;
    all.push(dev);
    let groups: Vec<Group> = groups
        .into_iter()
        .chain([group(DEVELOPER_GROUP, "", 0, "Developer")])
        .collect();
    assert_eq!(plan(&groups, &all, DEVELOPER_GROUP), [SCOPE_ONLY]);
    assert!(plan(&groups, &all, "").contains(&"dev.x".to_string()));
}

#[test]
fn reset_refusals_end_in_no_memory() {
    use crate::fail_alloc::sweep;
    let (groups, all) = account_world();
    sweep(|| reset_plan(&groups, Context::default(), &all, "").map(|k| k.len()));
    sweep(|| reset_plan(&groups, Context::default(), &all, "acct.page").map(|k| k.len()));
}

/// Words made up at random from the letters of a search (the old property
/// test's alphabet), by a small generator without a dependency.
fn made_up_queries(count: usize) -> Vec<String> {
    const ALPHABET: &[char] = &[
        'a', 'b', 'c', 'z', 'A', 'B', 'Z', 'ä', 'ö', 'ü', 'Ä', 'Ö', 'Ü', 'é', 'ß', ' ', ' ', '0',
        '9', '.', ',', '%', '-',
    ];
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    (0..count)
        .map(|_| {
            let length = (next() % 25) as usize;
            (0..length)
                .map(|_| ALPHABET[(next() % ALPHABET.len() as u64) as usize])
                .collect()
        })
        .collect()
}

#[test]
fn any_words_search_without_a_panic_and_case_does_not_matter() {
    let titles = [
        "Font size",
        "Schriftgröße",
        "Ränder",
        "Café Straße",
        "Brightness of the light",
        "100 % - ok, 9.5",
        "",
    ];
    for query in made_up_queries(2000) {
        let lower = words(&query.to_lowercase()).unwrap();
        let upper = words(&query.to_uppercase()).unwrap();
        // The same words, whatever the case.
        assert_eq!(lower, upper, "{query:?}");
        assert_eq!(lower.is_empty(), query.trim().is_empty(), "{query:?}");
        for title in titles {
            let a = matches(title, &lower).unwrap();
            let b = matches(title, &upper).unwrap();
            assert_eq!(a, b, "{query:?} in {title:?}");
            // And the title found by its own upper or lower case.
            assert!(
                matches(
                    &title.to_uppercase(),
                    &words(&title.to_lowercase()).unwrap()
                )
                .unwrap()
            );
        }
    }
}

#[test]
fn a_blank_search_finds_nothing_to_open() {
    // Typed blank, the search does not open: no words.
    for blank in ["", " ", "   ", "\t \n"] {
        assert!(words(blank).unwrap().is_empty());
    }
    // Words a setting's title and description hold, in either language,
    // with or without accents.
    let find = |text: &str, query: &str| matches(text, &words(query).unwrap()).unwrap();
    assert!(find("Font size the size of the font", "  SIZE   font "));
    assert!(find("Schriftgröße", "schriftgröße"));
    assert!(find("Schriftgröße", "SCHRIFTGROSSE"));
    assert!(!find("Font size", "zzqx flibbertigibbet"));
}

#[test]
fn a_choice_with_nothing_to_choose_is_not_offered() {
    let groups = vec![group("g", "", 0, "G")];
    let choice = setting("g.frame", "g", Kind::Choice, SHOWN);
    let cx = Context::default();
    // The page frame offers one column alone until a plugin adds a frame.
    assert!(!offered(&groups, cx, &choice, Some(1)));
    assert!(!offered(&groups, cx, &choice, Some(0)));
    // A choice of two is (the clock's 24 or 12 hours).
    assert!(offered(&groups, cx, &choice, Some(2)));
    assert!(offered(&groups, cx, &choice, None));
}

#[test]
fn advanced_settings_are_found_only_once_shown() {
    let groups = vec![
        group("g", "", 0, "G"),
        group(DEVELOPER_GROUP, "", 0, "Developer"),
    ];
    let advanced = setting("g.address", "g", Kind::Text, SHOWN | flag::ADVANCED);
    let basic = Context::default();
    let shown = Context {
        advanced: true,
        ..basic
    };
    assert!(!visible(&groups, basic, &advanced, None));
    assert!(visible(&groups, shown, &advanced, None));
    // The developer's settings show with the developer switch alone.
    let dev = setting("g.dev", DEVELOPER_GROUP, Kind::Bool, SHOWN | flag::ADVANCED);
    let dev_on = Context {
        developer: true,
        ..basic
    };
    assert!(!visible(&groups, basic, &dev, None));
    assert!(visible(&groups, dev_on, &dev, None));
}
