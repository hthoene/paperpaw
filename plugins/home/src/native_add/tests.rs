use super::*;
use crate::fail_alloc::sweep;
use crate::native_grid::Sizes;
use crate::native_pages::setting;
use alloc::string::ToString;
use alloc::vec;
use paperpaw_sdk::Rect;
use std::collections::BTreeMap;

struct Fake {
    settings: BTreeMap<String, String>,
}

impl Fake {
    fn new(pages: &[&str]) -> Self {
        let mut settings = BTreeMap::new();
        for (slot, text) in pages.iter().enumerate() {
            settings.insert(setting(slot, false).unwrap(), (*text).to_string());
        }
        Self { settings }
    }
}

impl Sizes for Fake {
    fn widget(&self, geometry: &Geometry, _: &str) -> (u8, u8, u8) {
        (geometry.columns, 1, 1)
    }
}

impl Source for Fake {
    fn text(&self, setting: &str) -> Result<String, Error> {
        copy(self.settings.get(setting).map_or("", String::as_str))
    }
    fn status_bar(&self) -> bool {
        false
    }
    fn columns(&self) -> i64 {
        4
    }
    fn has_widget(&self, _: &str) -> bool {
        true
    }
    fn tools(&self) -> Result<Vec<String>, Error> {
        Ok(Vec::new())
    }
    fn widest_px(&self) -> u32 {
        300
    }
}

fn grid4() -> Geometry {
    Geometry::of(Rect::new(0, 0, 400, 300), 4, 2, 0)
}

fn widget(id: &str, kind: &str) -> WidgetRow {
    WidgetRow {
        id: id.to_string(),
        title: id.to_string(),
        kind: kind.to_string(),
    }
}

fn tool(id: &str) -> (String, String) {
    (id.to_string(), id.to_string())
}

fn set_of<'a>(writes: &'a [Write], key: &str) -> &'a str {
    writes
        .iter()
        .find_map(|w| match w {
            Write::Set { setting, text } if setting == key => Some(text.as_str()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn the_grids_own_widgets_and_the_tools_on_the_page_are_not_offered() {
    let widgets = [
        widget("home.clock", "core.kind.time"),
        widget("core.space", "core.kind.other"),
        widget("core.fill", "core.kind.other"),
        widget("home.tools", "core.kind.other"),
        widget("home.status", "core.kind.other"),
    ];
    let tools = [tool("core.library"), tool("core.stats")];
    let on_page = grid::parse("tool:core.library 0/0 1x1").unwrap();
    let with_bar = entries(&widgets, &tools, true, &on_page).unwrap();
    let names: Vec<String> = with_bar.iter().map(|e| e.what.text().unwrap()).collect();
    assert_eq!(names, ["home.clock", "tool:core.stats"]);
    // Without the bar the status line is a widget like the others.
    let without = entries(&widgets, &tools, false, &on_page).unwrap();
    assert_eq!(without.len(), 3);
    assert!(without.iter().any(|e| e.what.text().unwrap() == "home.status"));
}

#[test]
fn kinds_keep_the_order_found_with_the_tools_last() {
    let widgets = [
        widget("a.one", "k.time"),
        widget("a.two", "k.books"),
        widget("a.three", "k.time"),
    ];
    let tools = [tool("core.library")];
    let found = entries(&widgets, &tools, false, &[]).unwrap();
    let kinds = kinds(&found).unwrap();
    assert_eq!(
        kinds,
        [
            ("k.time".to_string(), 2),
            ("k.books".to_string(), 1),
            (TOOLS_KIND.to_string(), 1)
        ]
    );
    // Tools found before a widget are still last.
    let first_tools = entries(&[], &tools, false, &[]).unwrap();
    let mixed: Vec<Entry> = first_tools.into_iter().chain(found).collect();
    assert_eq!(kinds_of(&mixed).last().unwrap().0, TOOLS_KIND);
}

fn kinds_of(entries: &[Entry]) -> Vec<(String, usize)> {
    kinds(entries).unwrap()
}

#[test]
fn the_first_step_lists_kinds_and_the_page_actions_by_what_is_possible() {
    let found = entries(&[widget("a.one", "k.time")], &[], false, &[]).unwrap();
    let rows = rows(None, &found, 1, false).unwrap();
    assert_eq!(rows, [Row::Kind("k.time".to_string(), 1), Row::AddPage]);
    let many = super::rows(None, &found, 3, true).unwrap();
    assert_eq!(
        many,
        [
            Row::Kind("k.time".to_string(), 1),
            Row::AddPage,
            Row::RemovePage,
            Row::ResetWide
        ]
    );
    // The most pages: no more to add.
    let full = super::rows(None, &found, MAX_PAGES, false).unwrap();
    assert!(!full.contains(&Row::AddPage));
    assert!(full.contains(&Row::RemovePage));
}

#[test]
fn the_second_step_lists_the_things_of_the_kind() {
    let widgets = [widget("a.one", "k.time"), widget("a.two", "k.books")];
    let found = entries(&widgets, &[tool("core.library")], false, &[]).unwrap();
    assert_eq!(rows(Some("k.books"), &found, 1, false).unwrap(), [Row::Add(1)]);
    assert_eq!(
        rows(Some(TOOLS_KIND), &found, 1, false).unwrap(),
        [Row::Add(2)]
    );
    assert!(rows(Some("nothing"), &found, 1, false).unwrap().is_empty());
}

fn layout<'a>(grid: &'a Geometry, pages: &'a [Page]) -> Layout<'a> {
    Layout {
        wide: false,
        grid,
        wide_grid: grid,
        pages,
    }
}

#[test]
fn an_added_tool_goes_into_the_first_free_cell_and_is_kept() {
    let fake = Fake::new(&["a.b 0/0 4x1", ""]);
    let g = grid4();
    let pages = crate::native_pages::pages(&fake).unwrap();
    let plan = add_item(&fake, &layout(&g, &pages), 0, What::Tool("core.library".to_string()))
        .unwrap();
    let Plan::Writes(writes) = plan else {
        panic!("not written")
    };
    assert_eq!(
        set_of(&writes, "home.page-1"),
        "a.b 0/0 4x1, tool:core.library 0/1 1x1"
    );
}

#[test]
fn a_full_page_has_no_room() {
    let fake = Fake::new(&["a.b 0/0 4x2"]);
    let g = grid4();
    let pages = crate::native_pages::pages(&fake).unwrap();
    let plan = add_item(&fake, &layout(&g, &pages), 0, What::Tool("core.library".to_string()))
        .unwrap();
    assert_eq!(plan, Plan::NoRoom);
    // No such page: nothing.
    let plan = add_item(&fake, &layout(&g, &pages), 5, What::Tool("core.library".to_string()))
        .unwrap();
    assert_eq!(plan, Plan::Nothing);
}

#[test]
fn a_page_is_added_after_the_others_as_a_space_and_the_last_is_never_removed() {
    let fake = Fake::new(&["a.b"]);
    let g = grid4();
    let pages = crate::native_pages::pages(&fake).unwrap();
    let Plan::Writes(writes) = add_page(&fake, &layout(&g, &pages)).unwrap() else {
        panic!("not written")
    };
    assert_eq!(set_of(&writes, "home.page-1"), "a.b");
    assert_eq!(set_of(&writes, "home.page-2"), "core.space");
    // Only one page: it stays.
    assert_eq!(
        remove_page(&fake, &layout(&g, &pages), 0).unwrap(),
        Plan::Nothing
    );
}

#[test]
fn a_removed_page_empties_the_last_setting_and_shifts_the_rest_up() {
    let fake = Fake::new(&["a.b", "c.d", "e.f"]);
    let g = grid4();
    let pages = crate::native_pages::pages(&fake).unwrap();
    let Plan::Writes(writes) = remove_page(&fake, &layout(&g, &pages), 1).unwrap() else {
        panic!("not written")
    };
    assert_eq!(set_of(&writes, "home.page-1"), "a.b");
    assert_eq!(set_of(&writes, "home.page-2"), "e.f");
    assert_eq!(set_of(&writes, "home.page-3"), "");
    assert_eq!(
        remove_page(&fake, &layout(&g, &pages), 9).unwrap(),
        Plan::Nothing
    );
}

#[test]
fn the_layout_for_a_screen_on_its_side_is_emptied_by_reset() {
    let Plan::Writes(writes) = reset_wide().unwrap() else {
        panic!("not written")
    };
    assert_eq!(writes.len(), MAX_PAGES);
    assert_eq!(set_of(&writes, "home.page-1.wide"), "");
}

#[test]
fn a_placement_is_the_first_number_not_held_and_without_values() {
    let fake = Fake::new(&["a.b@1, a.b@2", "a.b@3"]);
    let mut asked = Vec::new();
    let found = placement(&fake, "a.b", |id| {
        asked.push(id.to_string());
        Ok(id == "a.b@4")
    })
    .unwrap();
    assert_eq!(found, "a.b@5");
    // Held ones are not asked about.
    assert_eq!(asked, ["a.b@4", "a.b@5"]);
    // A placed id gives another placement of the same widget.
    assert_eq!(placement(&Fake::new(&[]), "a.b@7", |_| Ok(false)).unwrap(), "a.b@1");
    // A host refusal is passed on.
    assert_eq!(
        placement(&Fake::new(&[]), "a.b", |_| Err(Error::Busy)),
        Err(Error::Busy)
    );
}

#[test]
fn a_placement_falls_back_to_the_id_when_all_are_taken() {
    let fake = Fake::new(&[]);
    assert_eq!(placement(&fake, "a.b", |_| Ok(true)).unwrap(), "a.b");
}

#[test]
fn every_allocation_may_be_refused() {
    let widgets = vec![widget("a.one", "k.time"), widget("a.two", "k.books")];
    let tools = vec![tool("core.library")];
    let fake = Fake::new(&["a.b 0/0 4x1", "c.d"]);
    let g = grid4();
    let pages = crate::native_pages::pages(&fake).unwrap();
    let library = What::Tool("core.library".to_string());
    sweep(|| {
        let found = entries(&widgets, &tools, false, &[])?;
        rows(None, &found, 2, true)?;
        rows(Some("k.time"), &found, 2, true)?;
        let l = layout(&g, &pages);
        add_item(&fake, &l, 0, library.try_clone()?)?;
        add_page(&fake, &l)?;
        remove_page(&fake, &l, 0)?;
        reset_wide()?;
        placement(&fake, "a.one", |_| Ok(false))?;
        Ok(())
    });
}
