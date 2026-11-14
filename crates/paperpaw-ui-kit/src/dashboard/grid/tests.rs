use super::*;
use crate::fail_alloc::sweep;
use alloc::string::ToString;
use paperpaw_sdk::Rect;

/// Widgets "card" ask for the grid's width and two rows (one at least),
/// "small" for two columns and one row.
struct Fixed;
impl Sizes for Fixed {
    fn widget(&self, geometry: &Geometry, id: &str) -> (u8, u8, u8) {
        match id {
            "card" => (geometry.columns, 2.min(geometry.rows), 1),
            _ => (2.min(geometry.columns), 1, 1),
        }
    }
}

fn grid() -> Geometry {
    Geometry::of(Rect::new(0, 0, 400, 300), 4, 3, 10)
}

fn widget(id: &str) -> What {
    What::Widget(id.to_string())
}

fn tool(id: &str) -> What {
    What::Tool(id.to_string())
}

fn cells(col: u8, row: u8, w: u8, h: u8) -> Cells {
    Cells::new(col, row, w, h)
}

fn whats(items: &[Item]) -> Vec<String> {
    items.iter().map(|i| i.what.text().unwrap()).collect()
}

#[test]
fn layouts_are_read_and_written() {
    let text = "home.clock 0/0 4x1, tool:core.library 1/5 1x1, stats.today";
    let items = parse(text).unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[1].what, tool("core.library"));
    assert_eq!(items[0].at, Some(cells(0, 0, 4, 1)));
    assert_eq!(items[2].at, None);
    assert_eq!(write(&items).unwrap(), text);
    // Written without places: places to come.
    assert!(
        parse("home.status, home.date")
            .unwrap()
            .iter()
            .all(|i| i.at.is_none())
    );
    // What cannot be read is left out.
    assert_eq!(parse("tool:Not An Id, , home.date").unwrap().len(), 1);
    assert_eq!(parse("a 1/1 bad").unwrap()[0].at, None);
    assert_eq!(parse("a 300/1 1x1").unwrap()[0].at, None);
    assert!(parse("").unwrap().is_empty());
}

#[test]
fn what_the_sdk_grid_writes_is_what_this_one_reads() {
    use paperpaw_sdk::widgets::grid as sdk;
    let text = "home.clock 0/0 4x1, tool:core.library 1/5 1x1, stats.today";
    assert_eq!(
        write(&parse(text).unwrap()).unwrap(),
        sdk::write(&sdk::parse(text))
    );
}

#[test]
fn items_with_a_place_keep_it_and_the_others_take_the_first_free_cells() {
    let g = grid();
    let items = [
        Item::new(widget("small")),
        Item::placed(tool("a.b"), cells(0, 0, 1, 1)),
        Item::new(tool("c.d")),
    ];
    let (placed, left) = fit(&Fixed, &g, &items).unwrap();
    assert!(left.is_empty());
    assert_eq!(placed[0].at, Some(cells(1, 0, 2, 1)));
    assert_eq!(placed[1].at, Some(cells(0, 0, 1, 1)));
    assert_eq!(placed[2].at, Some(cells(3, 0, 1, 1)));
}

#[test]
fn a_place_the_grid_no_longer_holds_is_moved_inside_it() {
    let g = grid();
    let items = [Item::placed(widget("small"), cells(5, 5, 2, 1))];
    let (placed, _) = fit(&Fixed, &g, &items).unwrap();
    assert_eq!(placed[0].at, Some(cells(2, 2, 2, 1)));
    // Wider than the grid: narrower.
    let wide = [Item::placed(widget("small"), cells(0, 0, 9, 1))];
    assert_eq!(
        fit(&Fixed, &g, &wide).unwrap().0[0].at,
        Some(cells(0, 0, 4, 1))
    );
}

#[test]
fn an_item_whose_cells_another_took_goes_where_there_is_room() {
    // Both are inside the grid, the second on the first's cells: it is not
    // dropped, it moves to free cells.
    let items = parse("tool:a 0/0 2x2, tool:b 1/1 1x1").unwrap();
    let (shown, left) = fit(&Fixed, &grid(), &items).unwrap();
    assert!(left.is_empty());
    assert_eq!(whats(&shown), ["tool:a", "tool:b"]);
    assert_eq!(shown[0].at, Some(cells(0, 0, 2, 2)));
    let b = shown[1].at.unwrap();
    assert!(!b.overlaps(&shown[0].at.unwrap()), "{b:?}");
}

#[test]
fn what_has_no_room_is_left_as_it_was() {
    let g = Geometry::of(Rect::new(0, 0, 100, 100), 2, 1, 0);
    let items = [
        Item::new(tool("a.b")),
        Item::new(tool("c.d")),
        Item::new(tool("e.f")),
    ];
    let (placed, left) = fit(&Fixed, &g, &items).unwrap();
    assert_eq!(whats(&placed), ["tool:a.b", "tool:c.d"]);
    assert_eq!(left, [Item::new(tool("e.f"))]);
}

#[test]
fn widgets_shrink_to_the_room_left_down_to_their_least() {
    let g = grid();
    let taken = [cells(0, 0, 4, 2)];
    assert_eq!(room_of(&g, &taken, 4, 3, 1), Some(cells(0, 2, 4, 1)));
    assert_eq!(room_of(&g, &taken, 4, 3, 2), None);
    assert_eq!(room_of(&g, &[], 4, 2, 1), Some(cells(0, 0, 4, 2)));
}

#[test]
fn a_widget_keeps_its_share_of_the_width_in_a_wider_grid() {
    // A portrait grid of 4 columns to a landscape one of 6.
    let g = Geometry::of(Rect::new(0, 0, 600, 300), 6, 3, 0);
    let items = [
        Item::placed(widget("card"), cells(0, 0, 4, 1)),
        Item::placed(widget("small"), cells(0, 1, 2, 1)),
        Item::placed(tool("a.b"), cells(2, 1, 1, 1)),
        Item::new(widget("card")),
    ];
    let (placed, left) = refit(&Fixed, &g, 4, 5, &items).unwrap();
    assert!(left.is_empty());
    let at: Vec<Cells> = placed.iter().filter_map(|i| i.at).collect();
    assert_eq!(at[0], cells(0, 0, 5, 1));
    assert_eq!(at[1], cells(0, 1, 3, 1));
    // The first free cell, row by row.
    assert_eq!(at[2], cells(5, 0, 1, 1));
    // The card asks for 6 by 2, at most 5 wide, and finds room for 3 by 2.
    assert_eq!(at[3], cells(3, 1, 3, 2));
}

#[test]
fn adding_puts_the_item_in_the_first_free_cells() {
    let g = grid();
    let items = [Item::placed(widget("card"), cells(0, 0, 4, 2))];
    let added = add(&Fixed, &g, &items, tool("a.b")).unwrap().unwrap();
    assert_eq!(added[1].at, Some(cells(0, 2, 1, 1)));
    // A card needs two rows, of which one is enough: it gets the last row.
    let full = add(&Fixed, &g, &items, widget("card")).unwrap().unwrap();
    assert_eq!(full[1].at, Some(cells(0, 2, 4, 1)));
    // No room at all.
    let packed = [Item::placed(widget("card"), cells(0, 0, 4, 3))];
    assert!(add(&Fixed, &g, &packed, tool("a.b")).unwrap().is_none());
}

#[test]
fn refused_allocations_end_in_no_memory_and_recover() {
    let g = grid();
    let items = [
        Item::placed(widget("card"), cells(0, 0, 4, 2)),
        Item::new(widget("small")),
        Item::new(tool("a.b")),
        Item::placed(tool("c.d"), cells(9, 9, 1, 1)),
    ];
    let text = "home.clock 0/0 4x1, tool:core.library 1/5 1x1, stats.today";
    sweep(|| parse(text).map(|v| v.len()));
    sweep(|| write(&items).map(|t| t.len()));
    sweep(|| copy_items(&items).map(|v| v.len()));
    sweep(|| fit(&Fixed, &g, &items).map(|(a, b)| a.len() + b.len()));
    sweep(|| refit(&Fixed, &g, 4, 3, &items).map(|(a, b)| a.len() + b.len()));
    let extra = tool("e.f");
    sweep(|| add(&Fixed, &g, &items, extra.try_clone()?).map(|v| v.map(|v| v.len())));
}

/// A layout setting is user data: whatever it holds, reading and placing it
/// ends in items (some unplaced) or an error, never a panic, and the result
/// is written and read again unchanged.
#[test]
fn corrupt_layouts_are_read_and_placed_without_panics() {
    let g = grid();
    let commas = ",".repeat(4096);
    let spaced = ", ".repeat(2048);
    let long_id = "x".repeat(4096);
    let cases = [
        "",
        "   ",
        &commas,
        &spaced,
        "home.clock 99999999999999999999/0 4x1",
        "home.clock 0/99999999999999999999 4x1",
        "home.clock 0/0 99999999999999999999x1",
        "home.clock 0/0 4x99999999999999999999",
        "home.clock 256/256 255x255",
        "home.clock -1/-1 -1x-1",
        "home.clock 0/0 0x0",
        "home.clock 0/0 0x0, home.clock 0/0 0x0",
        "tool:, tool: 0/0 1x1",
        "tool:not an id 0/0 1x1, tool:..., tool:A B",
        "tool:core.library 0/0 1x1, tool:core.library 0/0 1x1",
        "home.clock 0/0, home.clock 0/0 4, home.clock 0/0 4x, home.clock /0 4x1",
        "home.clock 0/0 4x1 extra words, ,, ,",
        "\u{0}\u{ffff}, \u{1f980}/\u{1f980} \u{1f980}x\u{1f980}",
        &long_id,
    ];
    for text in cases {
        let items = parse(text).unwrap();
        let written = write(&items).unwrap();
        let again = parse(&written).unwrap();
        assert_eq!(write(&again).unwrap(), written, "{text:.40}");
        let (shown, left) = fit(&Fixed, &g, &items).unwrap();
        assert_eq!(shown.len() + left.len(), items.len(), "{text:.40}");
        assert!(shown.iter().all(|i| i.at.is_some()));
        let (shown, left) = refit(&Fixed, &g, 4, 3, &items).unwrap();
        assert_eq!(shown.len() + left.len(), items.len(), "{text:.40}");
        let (shown, left) = refit(&Fixed, &g, 0, 0, &items).unwrap();
        assert_eq!(shown.len() + left.len(), items.len(), "{text:.40}");
    }
    // What is no id or no number is left out, or left unplaced.
    assert!(parse(&commas).unwrap().is_empty());
    assert!(parse("tool:, tool:A B").unwrap().is_empty());
    assert_eq!(
        parse("home.clock 99999999999999999999/0 4x1").unwrap()[0].at,
        None
    );
    assert_eq!(parse("home.clock 0/0 -1x1").unwrap()[0].at, None);
}
