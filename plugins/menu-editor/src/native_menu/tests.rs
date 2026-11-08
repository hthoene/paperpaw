#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use alloc::vec;
use paperpaw_sdk::Id;
use proptest::prelude::*;

fn graph(edges: u16) -> MenuGraph {
    let ids = ["core.main", "core.settings-pages", "test.sub", "test.cycle"];
    let mut menus = Vec::new();
    for (from, id) in ids.iter().enumerate() {
        let mut entries: Vec<String> = ids
            .iter()
            .enumerate()
            .filter(|(to, _)| edges & (1 << (from * 4 + to)) != 0)
            .map(|(_, id)| String::from(*id))
            .collect();
        if from == 1 {
            entries.extend(PROTECTED.map(String::from));
        }
        menus.push(Menu {
            id: String::from(*id),
            title: String::from(*id),
            declared: entries.clone(),
            user_defined: false,
            current_titles: Vec::new(),
            current: entries,
        });
    }
    MenuGraph {
        menus,
        generation: 7,
    }
}

fn legacy_reaches(graph: &MenuGraph, from: &str, target: &str) -> bool {
    let entries = |id: &str| -> Vec<Id> {
        graph.index(id).map_or_else(Vec::new, |index| {
            graph.menus[index]
                .current
                .iter()
                .map(|id| Id::parse(id).unwrap())
                .collect()
        })
    };
    reaches(&entries, from, &Id::parse(target).unwrap(), &mut Vec::new())
}

/// The reference: whether `from` leads to `target`, directly or through
/// submenus (the walk of the interface-6 plugin, kept as the oracle).
fn reaches(ids: &dyn Fn(&str) -> Vec<Id>, from: &str, target: &Id, seen: &mut Vec<String>) -> bool {
    if seen.iter().any(|s| s == from) {
        return false;
    }
    seen.push(String::from(from));
    ids(from)
        .iter()
        .any(|id| id == target || reaches(ids, id.as_str(), target, seen))
}

proptest! {
    #[test]
    fn menu_order_matches_original_for_arbitrary_cycles(edges: u16) {
        let mut graph = graph(edges);
        for (menu, title) in graph.menus.iter_mut().zip(["Zulu", "alpha", "ALPHA", "Änderungen"]) {
            menu.title = String::from(title);
        }
        let mut roots: Vec<_> = (0..graph.menus.len()).filter(|index| {
            !graph.menus.iter().any(|menu| menu.current.contains(&graph.menus[*index].id))
        }).collect();
        roots.sort_by_key(|index| graph.menus[*index].title.to_lowercase());
        let mut expected = Vec::new();
        for index in roots.into_iter().chain(0..graph.menus.len()) {
            legacy_order(&graph, index, graph.menus[index].title.clone(), &mut expected);
        }
        prop_assert_eq!(graph.display_order().unwrap(), expected);
    }

    #[test]
    fn traversal_matches_original_for_arbitrary_cycles(edges: u16) {
        let graph = graph(edges);
        for root in 0..graph.menus.len() {
            for target in ["core.settings", "menu-editor.open", "test.sub", "test.missing"] {
                prop_assert_eq!(graph.reaches(root, target, false, None).unwrap(),
                    legacy_reaches(&graph, &graph.menus[root].id, target));
            }
        }
    }
}

// Mirrors the original add_with_submenus recursion, restricted to four menus.
fn legacy_order(graph: &MenuGraph, index: usize, name: String, out: &mut Vec<(usize, String)>) {
    if out.iter().any(|row| row.0 == index) {
        return;
    }
    out.push((index, name.clone()));
    for id in &graph.menus[index].current {
        if let Some(child) = graph.index(id) {
            let title = alloc::format!("{name} › {}", graph.menus[child].title);
            legacy_order(graph, child, title, out);
        }
    }
}

#[test]
fn protecting_other_roots_does_not_depend_on_current_menu_visibility() {
    let graph = graph(1 << 1); // main -> settings page
    assert!(
        !graph
            .keeps_access("core.settings-pages", &["menu-editor.open"])
            .unwrap()
    );
    assert!(!graph.keeps_access("core.main", &[]).unwrap());
    assert!(
        graph
            .keeps_access("core.main", &["test.sub", "core.settings-pages"])
            .unwrap()
    );
    assert!(
        graph
            .keeps_access("core.settings-pages", &PROTECTED)
            .unwrap()
    );
}

#[test]
fn cycles_are_refused_without_rejecting_ordinary_actions() {
    let graph = graph((1 << 1) | (1 << 6)); // main -> settings -> sub
    assert!(graph.makes_circle("test.sub", "core.main").unwrap());
    assert!(graph.makes_circle("test.sub", "test.sub").unwrap());
    assert!(!graph.makes_circle("test.sub", "test.action").unwrap());
}

#[test]
fn generation_refusal_keeps_the_snapshot_generation() {
    let mut graph = graph(0);
    assert_eq!(graph.check_generation(8), Err(Error::State));
    assert_eq!(graph.generation(), 7);
    assert_eq!(graph.check_generation(7), Ok(()));
}

#[test]
fn long_chains_use_bounded_traversal_stack() {
    let mut menus = Vec::new();
    for index in 0..1024 {
        let next = if index == 1023 {
            String::from("core.settings")
        } else {
            alloc::format!("test.m{}", index + 1)
        };
        menus.push(Menu {
            id: alloc::format!("test.m{index}"),
            title: String::new(),
            current_titles: Vec::new(),
            current: vec![next],
            declared: Vec::new(),
            user_defined: false,
        });
    }
    let graph = MenuGraph {
        menus,
        generation: 1,
    };
    assert!(graph.reaches(0, "core.settings", false, None).unwrap());
    assert!(!graph.reaches(0, "test.missing", false, None).unwrap());
}

#[test]
fn the_menu_order_survives_every_refused_allocation() {
    let mut graph = graph(0b0000_0000_0110_0100);
    for (menu, title) in graph
        .menus
        .iter_mut()
        .zip(["Zulu", "alpha", "ALPHA", "Änderungen"])
    {
        menu.title = String::from(title);
    }
    crate::fail_alloc::sweep(|| graph.display_order().map(|order| order.len()));
}
