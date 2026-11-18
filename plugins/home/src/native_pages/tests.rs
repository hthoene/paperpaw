use super::*;
use crate::fail_alloc::sweep;
use alloc::string::ToString;
use alloc::vec;
use paperpaw_sdk::Rect;
use std::collections::BTreeMap;

struct Fake {
    settings: BTreeMap<String, String>,
    status_bar: bool,
    missing: Vec<&'static str>,
    tools: Vec<&'static str>,
}

impl Fake {
    fn new(pages: &[&str]) -> Self {
        let mut settings = BTreeMap::new();
        for (slot, text) in pages.iter().enumerate() {
            settings.insert(setting(slot, false).unwrap(), (*text).to_string());
        }
        Self {
            settings,
            status_bar: false,
            missing: Vec::new(),
            tools: vec!["core.library", "core.settings"],
        }
    }

    fn wide(mut self, pages: &[&str]) -> Self {
        for (slot, text) in pages.iter().enumerate() {
            self.settings
                .insert(setting(slot, true).unwrap(), (*text).to_string());
        }
        self
    }
}

impl Sizes for Fake {
    fn widget(&self, geometry: &Geometry, _: &str) -> (u8, u8, u8) {
        (2.min(geometry.columns), 1, 1)
    }
}

impl Source for Fake {
    fn text(&self, setting: &str) -> Result<String, Error> {
        copy(self.settings.get(setting).map_or("", String::as_str))
    }
    fn status_bar(&self) -> bool {
        self.status_bar
    }
    fn columns(&self) -> i64 {
        4
    }
    fn has_widget(&self, id: &str) -> bool {
        !self.missing.contains(&base(id))
    }
    fn tools(&self) -> Result<Vec<String>, Error> {
        let mut found = Vec::new();
        for t in &self.tools {
            push(&mut found, copy(t)?)?;
        }
        Ok(found)
    }
    fn widest_px(&self) -> u32 {
        300
    }
}

fn grid4() -> Geometry {
    Geometry::of(Rect::new(0, 0, 400, 300), 4, 3, 0)
}

fn set_of(writes: &[Write], slot: usize, wide: bool) -> &str {
    let key = setting(slot, wide).unwrap();
    writes
        .iter()
        .find_map(|w| match w {
            Write::Set { setting, text } if *setting == key => Some(text.as_str()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn settings_are_named_by_page_and_layout() {
    assert_eq!(setting(0, false).unwrap(), "home.page-1");
    assert_eq!(setting(7, true).unwrap(), "home.page-8.wide");
    assert_eq!(slot_of("home.page-3"), Some(2));
    assert_eq!(slot_of("home.page-3.wide"), Some(2));
    assert_eq!(slot_of("home.page-9"), None);
    assert_eq!(slot_of("home.page-0"), None);
    assert_eq!(slot_of("home.columns"), None);
    assert_eq!(base("widgets.clock@2"), "widgets.clock");
    assert_eq!(base("widgets.clock"), "widgets.clock");
}

#[test]
fn the_first_page_always_shows_the_others_where_they_hold_something() {
    let fake = Fake::new(&["a.b", "", "c.d", "", "", "", "", ""]);
    let pages = pages(&fake).unwrap();
    assert_eq!(pages.iter().map(|p| p.slot).collect::<Vec<_>>(), [0, 2]);
    let empty = Fake::new(&[]);
    assert_eq!(super::pages(&empty).unwrap().len(), 1);
    assert!(!wide_kept(&fake).unwrap());
    assert!(wide_kept(&fake.wide(&["", "x.y"])).unwrap());
}

#[test]
fn expanding_puts_the_tools_in_and_leaves_out_what_the_grid_cannot_use() {
    let mut fake = Fake::new(&[]);
    fake.status_bar = true;
    fake.missing = vec!["gone.widget"];
    let items = grid::parse(
        "home.status, home.tools, core.space, core.fill, gone.widget, a.b, tool:core.library 0/0 1x1",
    )
    .unwrap();
    let out = expand(&fake, &items).unwrap();
    let names: Vec<String> = out.iter().map(|i| i.what.text().unwrap()).collect();
    // The library is shown already; the status line is above the grid.
    assert_eq!(names, ["tool:core.settings", "a.b", "tool:core.library"]);
}

#[test]
fn storing_upright_keeps_what_the_page_hides_and_a_space_for_an_emptied_page() {
    let mut fake = Fake::new(&["a.b 0/0 2x1, gone.widget, home.status", "c.d"]);
    fake.missing = vec!["gone.widget"];
    fake.status_bar = true;
    let items = grid::parse("a.b 1/1 2x1").unwrap();
    let writes = store(&fake, 0, &items, None).unwrap();
    assert_eq!(writes.len(), 1);
    assert_eq!(
        set_of(&writes, 0, false),
        "a.b 1/1 2x1, gone.widget, home.status"
    );
    // An emptied page other than the first keeps a space.
    let writes = store(&fake, 1, &[], None).unwrap();
    assert_eq!(set_of(&writes, 1, false), "core.space");
    // The first page is left empty.
    let writes = store(&Fake::new(&[]), 0, &[], None).unwrap();
    assert_eq!(set_of(&writes, 0, false), "");
}

#[test]
fn storing_all_pages_writes_the_changed_settings_and_empties_the_rest() {
    // The third setting holds something: emptied. The others are as they are.
    let fake = Fake::new(&["", "", "old.page"]);
    let pages = vec![
        Page {
            slot: 0,
            items: grid::parse("a.b 0/0 2x1").unwrap(),
        },
        Page {
            slot: 1,
            items: Vec::new(),
        },
    ];
    let writes = store_all(&fake, &pages, false).unwrap();
    assert_eq!(writes.len(), 3);
    assert_eq!(set_of(&writes, 0, false), "a.b 0/0 2x1");
    assert_eq!(set_of(&writes, 1, false), "core.space");
    assert_eq!(set_of(&writes, 2, false), "");
    // Written as they are kept: nothing is written.
    let kept = Fake::new(&["a.b 0/0 2x1", "core.space"]);
    assert!(store_all(&kept, &pages, false).unwrap().is_empty());
}

#[test]
fn a_screen_on_its_side_derives_its_pages_from_the_upright_ones() {
    let fake = Fake::new(&["a.b 0/0 4x1, c.d 0/1 2x1", "e.f 0/0 2x1"]);
    let g = Geometry::of(Rect::new(0, 0, 600, 300), 6, 3, 0);
    let pages = pages_wide(&fake, &g).unwrap();
    // Each widget keeps its share of the width (4 of 4 columns is 6 of 6; 2 of 4
    // is 3 of 6), at most 3 columns here: 300 px of cells 100 px wide.
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].items.len(), 3);
    let widths: Vec<u8> = pages[0]
        .items
        .iter()
        .filter_map(|i| i.at.map(|c| c.w))
        .collect();
    assert_eq!(widths, [3, 3, 3]);
}

#[test]
fn the_first_change_on_its_side_keeps_the_whole_derived_layout() {
    let fake = Fake::new(&["a.b 0/0 4x1, c.d 0/1 2x1", "e.f 0/0 2x1"]);
    let g = Geometry::of(Rect::new(0, 0, 600, 300), 6, 3, 0);
    let items = grid::parse("a.b 0/0 3x1, c.d 3/0 2x1, e.f 0/1 2x1").unwrap();
    let writes = store(&fake, 0, &items, Some(&g)).unwrap();
    // The other pages of the layout hold nothing: only the first is written.
    assert_eq!(writes.len(), 1);
    assert_eq!(
        set_of(&writes, 0, true),
        "a.b 0/0 3x1, c.d 3/0 2x1, e.f 0/1 2x1"
    );
    // Too many pages to keep: a message, nothing written.
    let many: Vec<Page> = (0..=MAX_PAGES)
        .map(|slot| Page {
            slot,
            items: Vec::new(),
        })
        .collect();
    assert_eq!(
        store_all(&fake, &many, true).unwrap(),
        [Write::Notify(WIDE_FULL)]
    );
}

#[test]
fn what_the_upright_pages_hide_is_kept_on_the_last_wide_page() {
    let mut fake = Fake::new(&["a.b 0/0 2x1, gone.widget"]);
    fake.missing = vec!["gone.widget"];
    let pages = vec![Page {
        slot: 0,
        items: grid::parse("a.b 0/0 2x1").unwrap(),
    }];
    let writes = store_all(&fake, &pages, true).unwrap();
    assert_eq!(set_of(&writes, 0, true), "a.b 0/0 2x1, gone.widget");
}

#[test]
fn adding_to_the_other_layout_goes_to_the_same_page_or_the_last() {
    let fake = Fake::new(&["a.b 0/0 2x1", "c.d 0/0 2x1"]).wide(&["x.y 0/0 2x1"]);
    let g = Geometry::of(Rect::new(0, 0, 600, 300), 6, 3, 0);
    // Shown upright: it goes to the layout on its side, page 2 → its last page.
    let writes = add_to_other(&fake, false, 1, What::Widget("n.w".to_string()), &g).unwrap();
    assert_eq!(set_of(&writes, 0, true), "x.y 0/0 2x1, n.w");
    // Already there: nothing.
    let none = add_to_other(&fake, false, 0, What::Widget("x.y".to_string()), &g).unwrap();
    assert!(none.is_empty());
    // On its side with a derived layout elsewhere: nothing to add to.
    let derived = Fake::new(&["a.b"]);
    assert!(
        add_to_other(&derived, false, 0, What::Widget("n.w".to_string()), &g)
            .unwrap()
            .is_empty()
    );
    // Shown on its side: the upright page.
    let up = add_to_other(&fake, true, 0, What::Widget("n.w".to_string()), &g).unwrap();
    assert_eq!(set_of(&up, 0, false), "a.b 0/0 2x1, n.w");
}

#[test]
fn a_page_holds_a_widget_in_either_layout() {
    let fake = Fake::new(&["a.b"]).wide(&["x.y"]);
    assert!(holds(&fake, "a.b").unwrap());
    assert!(holds(&fake, "x.y").unwrap());
    assert!(!holds(&fake, "n.w").unwrap());
    assert_eq!(reset_wide(&fake).unwrap().len(), 1);
    assert!(reset_wide(&Fake::new(&["a.b"])).unwrap().is_empty());
}

#[test]
fn a_changed_grid_moves_what_has_no_room_to_the_next_page() {
    // A grid of 4 columns by 1 row cannot hold three widgets of two columns.
    let fake = Fake::new(&["a.b 0/0 2x1, c.d 2/0 2x1, e.f 0/1 2x1, home.tools"]);
    let g = Geometry::of(Rect::new(0, 0, 400, 100), 4, 1, 0);
    let pages = reflow(&fake, &g, false).unwrap().unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(
        pages[0]
            .items
            .iter()
            .map(|i| i.what.text().unwrap())
            .collect::<Vec<_>>(),
        ["a.b", "c.d", "home.tools"]
    );
    assert_eq!(pages[1].items.len(), 1);
    // Nothing moved on a grid that fits: nothing.
    let roomy = Geometry::of(Rect::new(0, 0, 400, 300), 4, 3, 0);
    let fine = Fake::new(&["a.b 0/0 2x1, c.d 2/0 2x1"]);
    assert!(reflow(&fine, &roomy, false).unwrap().is_none());
    // Not for a screen on its side that has no layout of its own.
    assert!(reflow(&fake, &g, true).unwrap().is_none());
    let _ = grid4();
}

#[test]
fn a_changed_grid_writes_each_layout_fitted_to_its_own_grid() {
    let fake = Fake::new(&["a.b 0/0 2x1, c.d 2/0 2x1, e.f 0/1 2x1, home.tools"]);
    let tight = Geometry::of(Rect::new(0, 0, 400, 100), 4, 1, 0);
    let roomy = Geometry::of(Rect::new(0, 0, 400, 300), 4, 3, 0);
    // The upright pages overflow and are written (two pages); the layout on
    // its side is derived, so it has nothing to write.
    let writes = refit(&fake, &tight, &roomy).unwrap();
    assert_eq!(writes.len(), 2, "{writes:?}");
    assert!(writes.iter().all(|w| matches!(w, Write::Set { .. })));
    // Nothing moves on a grid that holds them: nothing is written.
    assert!(refit(&fake, &roomy, &roomy).unwrap().is_empty());
}

#[test]
fn refused_allocations_end_in_no_memory_and_recover() {
    let fake = Fake::new(&["a.b 0/0 4x1, c.d 0/1 2x1, home.tools", "e.f 0/0 2x1"]);
    let g = Geometry::of(Rect::new(0, 0, 600, 300), 6, 3, 0);
    let small = Geometry::of(Rect::new(0, 0, 400, 100), 4, 1, 0);
    let items = grid::parse("a.b 0/0 3x1, c.d 3/0 2x1").unwrap();
    sweep(|| pages(&fake).map(|p| p.len()));
    sweep(|| pages_wide(&fake, &g).map(|p| p.len()));
    sweep(|| store(&fake, 0, &items, None).map(|w| w.len()));
    sweep(|| store(&fake, 0, &items, Some(&g)).map(|w| w.len()));
    sweep(|| reflow(&fake, &small, false).map(|p| p.map(|p| p.len())));
    sweep(|| holds(&fake, "x.y"));
    sweep(|| refit(&fake, &small, &g).map(|w| w.len()));
    sweep(|| reset_wide(&fake).map(|w| w.len()));
    let what = What::Widget("n.w".to_string());
    sweep(|| add_to_other(&fake, true, 0, what.try_clone()?, &g).map(|w| w.len()));
}
