use super::*;
use crate::fail_alloc::sweep;
use alloc::string::ToString;
use alloc::vec;
use paperpaw_sdk::widgets::grid::Geometry;
use std::collections::BTreeMap;

struct Fake {
    settings: BTreeMap<String, String>,
    missing: Vec<&'static str>,
    tools: Vec<&'static str>,
}

impl Fake {
    fn new(pages: &[&str]) -> Self {
        let mut settings = BTreeMap::new();
        for (slot, text) in pages.iter().enumerate() {
            settings.insert(setting(slot).unwrap(), (*text).to_string());
        }
        Self {
            settings,
            missing: Vec::new(),
            tools: vec!["core.library", "core.settings"],
        }
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
}

fn items(text: &str) -> Vec<Item> {
    grid::parse(text).unwrap()
}

fn whats(items: &[Item]) -> Vec<String> {
    items.iter().map(|i| i.what.text().unwrap()).collect()
}

fn text_of(writes: &[Write], slot: usize) -> Option<&str> {
    let key = setting(slot).unwrap();
    writes
        .iter()
        .find(|w| w.setting == key)
        .map(|w| w.text.as_str())
}

#[test]
fn the_pages_are_the_interface_6_pages() {
    assert_eq!(MAX_PAGES, 4);
    assert_eq!(
        DEFAULTS,
        [
            "home.clock, home.continue, home.recent",
            "home.tools",
            "",
            ""
        ]
    );
    for slot in 0..MAX_PAGES {
        assert_eq!(
            setting(slot).unwrap(),
            alloc::format!("picture-home.page-{}", slot + 1)
        );
        assert_eq!(slot_of(&setting(slot).unwrap()), Some(slot));
    }
    for id in ["picture-home.page-5", "picture-home.page-0", "home.page-1"] {
        assert_eq!(slot_of(id), None);
    }
    assert_eq!(slot_of("picture-home.page-1.wide"), None);
    assert_eq!(TOOLS, "home.tools");
    assert_eq!(SPACE, paperpaw_sdk::widgets::SPACE);
    assert_eq!(FILL, paperpaw_sdk::widgets::FILL);
}

#[test]
fn the_first_page_always_shows_the_others_where_they_hold_something() {
    let src = Fake::new(&["", "", "core.space", ""]);
    let found = pages(&src).unwrap();
    let slots: Vec<usize> = found.iter().map(|p| p.slot).collect();
    assert_eq!(slots, [0, 2]);
    // A page with a space shows (as on interface 6).
    assert_eq!(whats(&found[1].items), ["core.space"]);
}

#[test]
fn expanding_puts_the_tools_in_and_leaves_out_what_the_grid_cannot_use() {
    let mut src = Fake::new(&[]);
    src.missing.push("gone.widget");
    let page =
        items("home.tools, core.space, core.fill, gone.widget@2, tool:core.library, own.card");
    let shown = expand(&src, &page).unwrap();
    // The tools not on the page otherwise, then what stays.
    assert_eq!(
        whats(&shown),
        ["tool:core.settings", "tool:core.library", "own.card"]
    );
}

#[test]
fn on_its_side_the_items_are_placed_anew_in_order_upright_as_they_are() {
    let page = items("a.b 2/1 2x1, tool:core.library 0/0 1x1, c.d");
    let upright = placed_anew(&page, false).unwrap();
    assert_eq!(upright, page);
    let side = placed_anew(&page, true).unwrap();
    assert_eq!(whats(&side), whats(&page));
    assert!(side.iter().all(|i| i.at.is_none()));
}

#[test]
fn storing_keeps_what_the_page_hides_and_a_space_for_an_emptied_page() {
    let mut src = Fake::new(&["a.b 0/0 2x1, gone.widget, core.fill, home.tools", ""]);
    src.missing.push("gone.widget");
    let shown = items("a.b 1/0 2x1");
    let writes = store(&src, 0, &shown).unwrap();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].setting, "picture-home.page-1");
    // What it hides is kept after what it shows; the tools and spaces are not
    // hidden, so a layout that dropped them drops them.
    assert_eq!(writes[0].text, "a.b 1/0 2x1, gone.widget, core.fill");
    // An emptied first page is empty, any other keeps a space.
    assert_eq!(
        store(&src, 0, &[]).unwrap()[0].text,
        "gone.widget, core.fill"
    );
    let bare = Fake::new(&["a.b", "c.d"]);
    assert_eq!(store(&bare, 0, &[]).unwrap()[0].text, "");
    assert_eq!(store(&bare, 1, &[]).unwrap()[0].text, "core.space");
}

#[test]
fn what_is_written_is_what_the_sdk_grid_writes() {
    let page = items("a.b 1/0 2x1, tool:core.library 0/1 1x1, c.d");
    let src = Fake::new(&[]);
    let writes = store(&src, 0, &page).unwrap();
    use paperpaw_sdk::widgets::grid as sdk;
    assert_eq!(writes[0].text, sdk::write(&sdk::parse(&writes[0].text)));
}

#[test]
fn storing_all_pages_writes_the_changed_settings_and_empties_the_rest() {
    let src = Fake::new(&["a.b", "c.d", "e.f", "g.h"]);
    // The second page was removed: the pages move up.
    let kept = vec![
        Page {
            slot: 0,
            items: items("a.b"),
        },
        Page {
            slot: 1,
            items: items("e.f 0/0 2x1"),
        },
        Page {
            slot: 2,
            items: items("g.h"),
        },
    ];
    let writes = store_all(&src, &kept).unwrap();
    // The first is the same text as it was: not written.
    assert_eq!(text_of(&writes, 0), None);
    assert_eq!(text_of(&writes, 1), Some("e.f 0/0 2x1"));
    assert_eq!(text_of(&writes, 2), Some("g.h"));
    assert_eq!(text_of(&writes, 3), Some(""));
    // An emptied page after the first keeps a space.
    let kept = vec![
        Page {
            slot: 0,
            items: items("a.b"),
        },
        Page {
            slot: 1,
            items: Vec::new(),
        },
    ];
    let writes = store_all(&src, &kept).unwrap();
    assert_eq!(text_of(&writes, 1), Some("core.space"));
}

#[test]
fn a_page_holds_a_widget_by_its_placement() {
    let src = Fake::new(&["a.b@2, c.d", "e.f"]);
    assert!(holds(&src, "a.b@2").unwrap());
    assert!(!holds(&src, "a.b").unwrap());
    assert!(holds(&src, "e.f").unwrap());
}

#[test]
fn a_placement_is_its_widget() {
    assert_eq!(base("home.clock@2"), "home.clock");
    assert_eq!(base("home.clock"), "home.clock");
}

#[test]
fn refused_allocations_end_in_no_memory_and_recover() {
    let mut src = Fake::new(&[
        "a.b 0/0 2x1, gone.widget, home.tools",
        "c.d",
        "",
        "core.space",
    ]);
    src.missing.push("gone.widget");
    let page = items("a.b 0/0 2x1, home.tools, tool:core.library");
    sweep(|| pages(&src).map(|p| p.len()));
    sweep(|| expand(&src, &page).map(|p| p.len()));
    sweep(|| placed_anew(&page, true).map(|p| p.len()));
    sweep(|| placed_anew(&page, false).map(|p| p.len()));
    sweep(|| store(&src, 0, &page).map(|w| w.len()));
    sweep(|| holds(&src, "a.b").map(drop));
    sweep(|| setting(2).map(drop));
    let all = pages(&src).unwrap();
    sweep(|| store_all(&src, &all).map(|w| w.len()));
}

#[test]
fn a_change_within_the_stage_budget_fits_and_one_over_it_does_not() {
    let small = [Write {
        setting: "picture-home.page-1".into(),
        text: "a.b".into(),
    }];
    assert!(fits(&small, &[]));
    let big = [Write {
        setting: "picture-home.page-1".into(),
        text: "x".repeat(STAGE_BUDGET),
    }];
    assert!(!fits(&big, &[]));
    // The reset placements count too.
    let resets = ["y".repeat(STAGE_BUDGET + 1)];
    assert!(!fits(&[], &resets));
    assert!(fits(&[], &[]));
}
