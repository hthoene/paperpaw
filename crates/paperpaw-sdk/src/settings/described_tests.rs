use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::Cell;
use core::ops::ControlFlow;

use super::*;
use crate::builtin::groups::OTHER;
use crate::collections::VecMap;
use crate::plugin::RegisterError;

/// What a test tells its source after the settings took it.
#[derive(Default)]
struct Tell {
    /// What every `make` answers from now on.
    fail: Cell<Option<DescribeError>>,
    /// The id every `make` gives its spec from now on.
    renamed: Cell<Option<&'static str>>,
    /// Every `make` gives a text setting from now on, whatever it was.
    retyped: Cell<bool>,
    makes: Cell<usize>,
}

/// A plugin's settings kept as specs: each `make` copies one.
struct Source {
    plugin: &'static str,
    specs: Vec<SettingSpec>,
    tell: Rc<Tell>,
}

impl Described for Source {
    fn count(&self) -> usize {
        self.specs.len()
    }

    fn name(&self, at: usize, visit: &mut dyn FnMut(&str)) -> Result<(), DescribeError> {
        let spec = self.specs.get(at).ok_or(DescribeError::Gone)?;
        let name = spec.id.as_str().strip_prefix(self.plugin);
        visit(name.and_then(|n| n.strip_prefix('.')).unwrap_or_default());
        Ok(())
    }

    fn make(&self, at: usize) -> Result<SettingSpec, DescribeError> {
        if let Some(error) = self.tell.fail.get() {
            return Err(error);
        }
        self.tell.makes.set(self.tell.makes.get() + 1);
        let mut spec = self.specs.get(at).ok_or(DescribeError::Gone)?.clone();
        if let Some(id) = self.tell.renamed.get() {
            spec.id = Id::new_static(id);
        }
        if self.tell.retyped.get() {
            let id = spec.id.clone();
            spec = SettingSpec::text("a.text", "a.title", "", OTHER);
            spec.id = id;
        }
        Ok(spec)
    }
}

/// Settings of every kind and with every flag, of plugin `stats`.
fn stats() -> Vec<SettingSpec> {
    alloc::vec![
        SettingSpec::switch("stats.show", "stats.show.title", true, OTHER),
        SettingSpec::number("stats.goal", "stats.goal.title", 5, 600, 30, OTHER)
            .scoped()
            .description("stats.goal.help"),
        SettingSpec::choice(
            "stats.span",
            "stats.span.title",
            &["day", "week", "month"],
            "week",
            OTHER,
        )
        .advanced(),
        SettingSpec::text("stats.name", "stats.name.title", "", OTHER)
            .guarded()
            .kept_on_reset(),
    ]
}

fn switch(id: &'static str) -> SettingSpec {
    SettingSpec::switch(id, "a.title", false, OTHER)
}

fn described(
    plugin: &'static str,
    specs: Vec<SettingSpec>,
) -> (Result<DescribedSettings, DescribeError>, Rc<Tell>) {
    let tell = Rc::new(Tell::default());
    let source = Source {
        plugin,
        specs,
        tell: tell.clone(),
    };
    (
        DescribedSettings::new(Id::new_static(plugin), Box::new(source)),
        tell,
    )
}

/// `before.a`, the stats settings and `later.z`: the stats described, or
/// copied (`None`).
fn settings(lazy: bool) -> (Settings, Rc<Tell>) {
    let mut s = Settings::new();
    s.define(switch("before.a")).unwrap();
    let tell = if lazy {
        let (stats, tell) = described("stats", stats());
        s.define_described(stats.unwrap()).unwrap();
        tell
    } else {
        for spec in stats() {
            s.define(spec).unwrap();
        }
        Rc::new(Tell::default())
    };
    s.define(switch("later.z")).unwrap();
    (s, tell)
}

fn ids(s: &Settings) -> Vec<String> {
    let mut ids = Vec::new();
    s.each(Pick::All, |spec| ids.push(spec.id.to_string()))
        .unwrap();
    ids
}

#[test]
fn described_settings_read_as_copied_ones_do() {
    let (lazy, _) = settings(true);
    let (copied, _) = settings(false);
    assert_eq!(lazy.specs().count(), 6);
    assert!(
        lazy.specs()
            .zip(copied.specs())
            .all(|(a, b)| a.same_as(b) && b.same_as(a))
    );
    assert_eq!(ids(&lazy), ids(&copied));
    for spec in copied.specs() {
        let id = spec.id.as_str();
        assert_eq!(lazy.get(id), copied.get(id), "{id}");
        assert_eq!(lazy.global(id), copied.global(id), "{id}");
        assert!(lazy.spec(id).is_some_and(|lazy| lazy.same_as(spec)), "{id}");
    }
    assert_eq!(lazy.get("stats.nothing"), None);
    assert_eq!(lazy.try_spec("stats.nothing"), Ok(None));
    assert_eq!(lazy.get("stats"), None);
}

#[test]
fn values_are_set_and_checked_as_those_of_copied_settings() {
    for lazy in [true, false] {
        let (mut s, _) = settings(lazy);
        s.set("stats.goal", SettingValue::Int(45)).unwrap();
        s.set_from_text("stats.span", "month").unwrap();
        assert_eq!(s.get("stats.goal"), Some(&SettingValue::Int(45)));
        assert_eq!(s.text("stats.span"), "month");
        assert_eq!(
            s.set("stats.goal", SettingValue::Int(601)),
            Err(SettingError::InvalidValue)
        );
        assert_eq!(
            s.set_from_text("stats.span", "year"),
            Err(SettingError::InvalidValue)
        );
        assert_eq!(
            s.set("stats.none", SettingValue::Int(1)),
            Err(SettingError::Unknown)
        );
        s.reset("stats.goal").unwrap();
        assert_eq!(s.get("stats.goal"), Some(&SettingValue::Int(30)));
        // Only the scoped one goes to a scope.
        let mut scope = VecMap::new();
        scope.insert(Id::new_static("stats.goal"), SettingValue::Int(60));
        scope.insert(Id::new_static("stats.show"), SettingValue::Bool(false));
        scope.insert(Id::new_static("stats.gone"), SettingValue::Int(1));
        s.set_scope(Some(scope));
        assert_eq!(s.scope().map(VecMap::len), Some(1), "lazy: {lazy}");
        assert_eq!(
            s.set_in_scope("stats.show", SettingValue::Bool(false)),
            Err(SettingError::InvalidValue)
        );
        s.set_in_scope("stats.goal", SettingValue::Int(90)).unwrap();
        assert_eq!(
            s.scope().and_then(|scope| scope.get("stats.goal")),
            Some(&SettingValue::Int(90))
        );
    }
}

#[test]
fn nothing_is_kept_of_a_spec_read_in_passing() {
    let (mut s, tell) = settings(true);
    let made = tell.makes.get();
    assert_eq!(made, 4, "each read once to keep its default and flags");
    // Reads of values make nothing at all.
    assert_eq!(s.get("stats.goal"), Some(&SettingValue::Int(30)));
    assert!(!s.goes_to_scope("stats.goal"));
    assert_eq!(s.define(switch("stats.show")).map_err(|_| ()), Err(()));
    assert_eq!(tell.makes.get(), made);
    // Writes and walks make a spec and let it go.
    s.set("stats.goal", SettingValue::Int(45)).unwrap();
    assert_eq!(ids(&s).len(), 6);
    assert!(tell.makes.get() > made);
    assert!(!s.any_made());
    // A spec handed out is kept until the kernel rests.
    let made = tell.makes.get();
    assert!(s.spec("stats.goal").is_some());
    assert!(s.spec("stats.goal").is_some());
    assert_eq!(tell.makes.get(), made + 1);
    assert!(s.any_made());
    s.rest();
    assert!(!s.any_made());
    assert!(s.spec("stats.goal").is_some());
    assert_eq!(tell.makes.get(), made + 2);
}

#[test]
fn a_walk_makes_only_what_it_picks() {
    let (s, tell) = settings(true);
    let made = tell.makes.get();
    let mut seen = Vec::new();
    s.each(Pick::Of("later"), |spec| seen.push(spec.id.to_string()))
        .unwrap();
    s.each(Pick::WithExtra, |spec| seen.push(spec.id.to_string()))
        .unwrap();
    assert_eq!(seen, ["later.z"]);
    assert_eq!(tell.makes.get(), made);
    s.each(Pick::Scoped, |spec| seen.push(spec.id.to_string()))
        .unwrap();
    assert_eq!(seen, ["later.z", "stats.goal"]);
    assert_eq!(tell.makes.get(), made + 1);
    let mut first = None;
    s.walk(Pick::OfAny(&["stats", "none"]), &mut |spec| {
        first = Some(spec.id.to_string());
        ControlFlow::Break(())
    })
    .unwrap();
    assert_eq!(first.as_deref(), Some("stats.show"));
    assert_eq!(tell.makes.get(), made + 2);
}

#[test]
fn without_room_a_described_setting_is_full_and_nothing_changes() {
    let (mut s, tell) = settings(true);
    s.set("stats.goal", SettingValue::Int(45)).unwrap();
    let mut scope = VecMap::new();
    scope.insert(Id::new_static("stats.goal"), SettingValue::Int(60));
    s.set_scope(Some(scope.clone()));
    tell.fail.set(Some(DescribeError::NoMemory));

    assert_eq!(s.try_spec("stats.goal"), Err(SettingError::Full));
    assert!(s.spec("stats.goal").is_none());
    assert_eq!(
        s.set("stats.goal", SettingValue::Int(50)),
        Err(SettingError::Full)
    );
    assert_eq!(s.set_from_text("stats.goal", "50"), Err(SettingError::Full));
    assert_eq!(
        s.set_in_scope("stats.goal", SettingValue::Int(50)),
        Err(SettingError::Full)
    );
    assert_eq!(
        s.set_default_from_text("stats.goal", "50"),
        Err(SettingError::Full)
    );
    assert_eq!(
        s.set_default_by("stats.goal", "50", Id::new_static("later")),
        Err(SettingError::Full)
    );
    assert!(!s.set_range("stats.goal", 0, 9));
    assert_eq!(s.each(Pick::All, |_| ()), Err(SettingError::Full));
    // The values are all there, and as they were.
    assert_eq!(s.global("stats.goal"), Some(&SettingValue::Int(45)));
    assert_eq!(s.text("stats.span"), "week");
    assert!(s.default_from("stats.goal").is_none());
    // A scope whose values cannot be checked is not taken in part.
    let mut other = VecMap::new();
    other.insert(Id::new_static("stats.goal"), SettingValue::Int(90));
    assert_eq!(s.try_set_scope(Some(other)), Err(SettingError::Full));
    assert_eq!(s.scope(), Some(&scope));
    // Only what could be made is listed; the others are still there.
    assert_eq!(s.specs().count(), 2);

    tell.fail.set(None);
    let (copied, _) = settings(false);
    assert_eq!(ids(&s), ids(&copied));
    assert_eq!(s.try_spec("stats.goal").map(|s| s.is_some()), Ok(true));
    s.set("stats.goal", SettingValue::Int(50)).unwrap();
}

#[test]
fn a_setting_that_cannot_be_read_any_more_is_as_if_not_there() {
    for (gone, renamed) in [(Some(DescribeError::Gone), None), (None, Some("stats.xx"))] {
        let (mut s, tell) = settings(true);
        tell.fail.set(gone);
        tell.renamed.set(renamed);
        assert_eq!(s.try_spec("stats.goal"), Ok(None));
        assert_eq!(
            s.set("stats.goal", SettingValue::Int(50)),
            Err(SettingError::Unknown)
        );
        assert_eq!(s.try_spec("stats.xx"), Ok(None));
        assert_eq!(ids(&s), ["before.a", "later.z"]);
        assert_eq!(s.get("stats.goal"), Some(&SettingValue::Int(30)));
    }
}

#[test]
fn a_changed_described_setting_is_kept_as_it_is_from_then_on() {
    let (mut s, tell) = settings(true);
    let order = ids(&s);
    assert!(s.set_range("stats.goal", 10, 90));
    assert!(s.set_choices("stats.span", alloc::vec!["week".into(), "year".into()]));
    s.rest();
    let made = tell.makes.get();
    assert_eq!(
        s.spec("stats.goal").map(|spec| &spec.kind),
        Some(&SettingKind::Int { min: 10, max: 90 })
    );
    assert!(s.set_from_text("stats.span", "year").is_ok());
    assert_eq!(tell.makes.get(), made, "kept: not made again");
    assert!(!s.any_made());
    assert_eq!(ids(&s), order);
    // Even where the plugin's own data cannot be read any more.
    tell.fail.set(Some(DescribeError::Gone));
    assert!(s.spec("stats.goal").is_some_and(|spec| spec.scoped));
    assert_eq!(ids(&s), ["before.a", "stats.goal", "stats.span", "later.z"]);
    assert!(s.define(switch("stats.goal")).is_err());
    s.remove_plugin("stats");
    assert_eq!(ids(&s), ["before.a", "later.z"]);
    assert_eq!(s.get("stats.goal"), None);
}

#[test]
fn a_plugin_going_leaves_the_others_where_they_are() {
    let mut s = Settings::new();
    let (a, _) = described("a", alloc::vec![switch("a.one"), switch("a.two")]);
    let (b, _) = described("b", alloc::vec![switch("b.one"), switch("b.two")]);
    let (c, _) = described("c", alloc::vec![switch("c.one")]);
    s.define_described(a.unwrap()).unwrap();
    s.define(switch("mid.m")).unwrap();
    s.define_described(b.unwrap()).unwrap();
    s.define_described(c.unwrap()).unwrap();
    // One of b's is kept as it is: its place is among the made ones.
    assert!(s.set_description("b.two", "b.help"));
    s.set("c.one", SettingValue::Bool(true)).unwrap();
    assert_eq!(
        ids(&s),
        ["a.one", "a.two", "mid.m", "b.one", "b.two", "c.one"]
    );
    s.remove_plugin("a");
    assert_eq!(ids(&s), ["mid.m", "b.one", "b.two", "c.one"]);
    assert_eq!(s.get("a.one"), None);
    assert_eq!(s.get("c.one"), Some(&SettingValue::Bool(true)));
    let help = Some(crate::Str::new_static("b.help"));
    assert!(s.spec("b.two").is_some_and(|spec| spec.description == help));
    s.remove_plugin("mid");
    s.remove_plugin("b");
    assert_eq!(ids(&s), ["c.one"]);
    assert_eq!(s.spec("c.one").map(|spec| spec.id.as_str()), Some("c.one"));
    // And comes again, after the others.
    let (a, _) = described("a", alloc::vec![switch("a.one")]);
    s.define_described(a.unwrap()).unwrap();
    assert_eq!(ids(&s), ["c.one", "a.one"]);
}

#[test]
fn an_id_is_kept_once_however_it_is_defined() {
    let (mut s, _) = settings(true);
    let duplicate = |result: Result<(), RegisterError>, id: &str| match result {
        Err(RegisterError::Duplicate { id: twice }) => assert_eq!(twice.as_str(), id),
        other => panic!("{other:?}"),
    };
    duplicate(s.define(switch("stats.goal")), "stats.goal");
    static FIXED: [SettingSpec; 1] = [SettingSpec::switch("stats.show", "t", false, OTHER)];
    duplicate(s.define_fixed(&FIXED, 0), "stats.show");
    let (again, _) = described(
        "stats",
        alloc::vec![switch("stats.new"), switch("stats.span")],
    );
    duplicate(s.define_described(again.unwrap()), "stats.span");
    let (later, _) = described("later", alloc::vec![switch("later.y"), switch("later.z")]);
    let later = later.unwrap();
    duplicate(s.check_described(&later), "later.z");
    duplicate(s.define_described(later), "later.z");
    assert_eq!(ids(&s).len(), 6, "all of a plugin's or none");
    // A second batch of the same plugin with new names is fine.
    let (more, _) = described("stats", alloc::vec![switch("stats.new")]);
    s.define_described(more.unwrap()).unwrap();
    assert_eq!(s.get("stats.new"), Some(&SettingValue::Bool(false)));
    assert_eq!(ids(&s).last().map(String::as_str), Some("stats.new"));
}

#[test]
fn what_cannot_be_kept_this_way_is_refused_with_its_reason() {
    static EXTRA: SettingExtra = SettingExtra::new().secret();
    let invalid = |specs: Vec<SettingSpec>| match described("stats", specs).0 {
        Err(DescribeError::Invalid(why)) => why,
        other => panic!("{other:?}"),
    };
    let extra = switch("stats.a").with_extra(&EXTRA);
    assert_eq!(invalid(alloc::vec![extra]), "a setting with an extra");
    assert_eq!(
        invalid(alloc::vec![switch("other.a")]),
        "a setting of another plugin"
    );
    assert_eq!(
        invalid(alloc::vec![switch("stats.a"), switch("stats.a")]),
        "a setting declared twice"
    );
    let mut wrong = switch("stats.a");
    wrong.default = SettingValue::Int(1);
    assert_eq!(
        invalid(alloc::vec![wrong]),
        "default value does not match the setting kind"
    );
    let (none, tell) = described("stats", stats());
    drop(none);
    tell.fail.set(Some(DescribeError::NoMemory));
    let source = Source {
        plugin: "stats",
        specs: stats(),
        tell,
    };
    assert_eq!(
        DescribedSettings::new(Id::new_static("stats"), Box::new(source)).map(|_| ()),
        Err(DescribeError::NoMemory)
    );
}

#[test]
fn a_default_changed_is_the_one_a_spec_made_later_has() {
    for lazy in [true, false] {
        let (mut s, tell) = settings(lazy);
        s.set_default_from_text("stats.span", "day").unwrap();
        assert_eq!(s.text("stats.span"), "day");
        assert_eq!(
            s.set_default_from_text("stats.span", "year"),
            Err(SettingError::InvalidValue)
        );
        assert!(s.spec("stats.goal").is_some());
        s.set_default_by("stats.goal", "45", Id::new_static("later"))
            .unwrap();
        s.set_default_by("stats.goal", "50", Id::new_static("before"))
            .unwrap();
        let default = |s: &Settings| s.spec("stats.goal").map(|spec| spec.default.clone());
        assert_eq!(default(&s), Some(SettingValue::Int(50)), "lazy: {lazy}");
        assert_eq!(s.get("stats.goal"), Some(&SettingValue::Int(50)));
        assert_eq!(s.default_from("stats.goal").map(Id::as_str), Some("before"));
        // The layers come off as those of a copied setting, without a read.
        tell.fail.set(Some(DescribeError::NoMemory));
        s.remove_plugin("before");
        assert_eq!(s.get("stats.goal"), Some(&SettingValue::Int(45)));
        s.remove_plugin("later");
        assert_eq!(s.get("stats.goal"), Some(&SettingValue::Int(30)));
        tell.fail.set(None);
        assert_eq!(default(&s), Some(SettingValue::Int(30)), "lazy: {lazy}");
        assert_eq!(s.text("stats.span"), "day");
        assert_eq!(s.specs().count(), 4);
    }
}

#[test]
fn a_setting_of_another_kind_than_it_was_described_with_is_as_if_not_there() {
    let (mut s, tell) = settings(true);
    tell.retyped.set(true);
    // The default kept is a number: a text setting is not what it is of.
    assert_eq!(s.try_spec("stats.goal"), Ok(None));
    assert_eq!(
        s.set_from_text("stats.goal", "fifty"),
        Err(SettingError::Unknown)
    );
    assert_eq!(s.get("stats.goal"), Some(&SettingValue::Int(30)));
    // One that was a text setting all along is read as before.
    assert_eq!(
        s.try_spec("stats.name").map(|spec| spec.is_some()),
        Ok(true)
    );
    tell.retyped.set(false);
    assert_eq!(
        s.try_spec("stats.goal").map(|spec| spec.is_some()),
        Ok(true)
    );
}

#[test]
fn what_is_asked_about_a_described_setting_without_its_spec_makes_none() {
    let (s, tell) = settings(true);
    let (copied, _) = settings(false);
    // Each was read once, when the plugin's settings were taken.
    let made = tell.makes.get();
    let asked = [
        "before.a",
        "stats.show",
        "stats.goal",
        "stats.goal@2",
        "stats@2.goal",
        "stats.span",
        "stats.name",
        "stats.none",
        "stats",
        "later.z",
    ];
    for id in asked {
        assert_eq!(s.defines(id), copied.defines(id), "{id}");
        assert_eq!(s.defines_scoped(id), copied.defines_scoped(id), "{id}");
    }
    assert!(s.defines("stats.goal") && !s.defines("stats.goal@2"));
    assert!(s.defines_scoped("stats.goal") && s.defines_scoped("stats.goal@2"));
    assert!(!s.defines_scoped("stats.show"));
    // The specs kept as they are: none of those the plugin describes.
    let kept: Vec<String> = s.kept_specs().map(|spec| spec.id.to_string()).collect();
    assert_eq!(kept, ["before.a", "later.z"]);
    assert_eq!(copied.kept_specs().count(), 6);
    assert_eq!(tell.makes.get(), made);
    assert!(!s.any_made());
}
