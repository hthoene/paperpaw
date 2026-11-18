use super::super::scene::Ui;
use super::*;

const PAPER: [u8; 3] = [232, 228, 220];
const INK: [u8; 3] = [30, 30, 30];

fn ui() -> Ui {
    Ui {
        dpi: 220,
        margin: 26,
        finger: 0,
        line: 2,
        gap: 19,
    }
}

fn d(role: Role, label: &str, area: Rect) -> Described {
    Described::new(role, label.to_string(), area).clip(Rect::new(0, 0, 480, 755))
}

fn fill(image: &mut Image, area: Rect, rgb: [u8; 3]) {
    for y in area.y..area.bottom() as u16 {
        for x in area.x..area.right() as u16 {
            image.set_pixel(u32::from(x), u32::from(y), rgb);
        }
    }
}

/// A word of `letters` letters drawn as bars 3 px wide, 2 apart.
fn word(image: &mut Image, x: u16, y: u16, letters: u16, height: u16) -> Rect {
    for i in 0..letters {
        fill(image, Rect::new(x + i * 5, y, 3, height), INK);
    }
    Rect::new(x, y, letters * 5 - 2, height)
}

fn paper() -> Image {
    Image::filled(480, 800, PAPER)
}

fn scene<'a>(image: &'a Image, described: Vec<Described>) -> Scene<'a> {
    Scene::new(image, described, Rect::new(0, 0, 480, 755), ui())
}

fn checks(issues: &[Issue]) -> Vec<&'static str> {
    issues.iter().map(|i| i.check).collect()
}

#[test]
fn text_cut_at_its_clip_is_found() {
    let mut image = paper();
    word(&mut image, 26, 20, 10, 18);
    word(&mut image, 302, 120, 30, 18);
    let mut cut = d(Role::Text, "A long label", Rect::new(302, 110, 178, 40));
    cut.clip = Rect::new(0, 100, 400, 60);
    let fine = d(Role::Text, "Fine", Rect::new(26, 10, 300, 40));
    let issues = text_cut(&scene(&image, vec![fine, cut]));
    assert_eq!(checks(&issues), [TEXT_CUT]);
    assert!(issues[0].message.contains("right edge"), "{issues:?}");
}

#[test]
fn a_frame_at_the_edge_is_no_cut_text() {
    let mut image = paper();
    // A frame line along the clip's edge, the text well within.
    fill(&mut image, Rect::new(399, 100, 1, 60), INK);
    word(&mut image, 300, 120, 4, 18);
    let mut text = d(Role::Text, "Word", Rect::new(300, 110, 180, 40));
    text.clip = Rect::new(0, 100, 400, 60);
    assert!(text_cut(&scene(&image, vec![text])).is_empty());
}

#[test]
fn shortened_text_is_noted_once() {
    let mut image = paper();
    // Filling its room, then a text ending in "…" of itself.
    word(&mut image, 26, 10, 39, 20);
    word(&mut image, 26, 110, 8, 20);
    let texts = vec![
        d(Role::Text, "Reading stat…", Rect::new(26, 0, 200, 40)),
        d(Role::Text, "Reading stat…", Rect::new(26, 0, 200, 40)),
        d(Role::Text, "Plain", Rect::new(26, 50, 200, 40)),
        d(Role::Text, "Loading…", Rect::new(26, 100, 428, 40)),
    ];
    assert_eq!(checks(&ellipsis(&scene(&image, texts))), [ELLIPSIS]);
}

/// An explanation of several lines cut short is an error, even where its
/// last line, centred, leaves room at its end.
#[test]
fn an_explanation_cut_short_is_an_error() {
    let mut image = paper();
    word(&mut image, 100, 210, 40, 20);
    word(&mut image, 150, 240, 12, 20);
    let texts = vec![d(
        Role::Text,
        "Enter your account, or create one. Only a hash of the pa…",
        Rect::new(0, 200, 480, 70),
    )];
    // A text of the language ending in a "…" of its own after a blank
    // does not pass every "…" as its own.
    let own = [String::from("Loading {host} …")];
    let issues = ellipsis(&scene(&image, texts).ellipses(&own));
    assert_eq!(checks(&issues), [EXPLANATION_CUT]);
}

#[test]
fn an_ellipsis_of_the_text_itself_is_no_cut_with_labels_whole() {
    let mut image = paper();
    word(&mut image, 26, 10, 39, 20);
    word(&mut image, 26, 110, 8, 20);
    let texts = vec![
        d(Role::Text, "Reading stat…", Rect::new(26, 0, 200, 40)),
        // The last line of "Plugin wird geladen…".
        d(Role::Text, "geladen…", Rect::new(26, 100, 428, 40)),
        // "Loading {host} …" with its host, and a "…" in the middle.
        d(
            Role::Text,
            "Loading example.org …",
            Rect::new(26, 100, 428, 40),
        ),
        d(Role::Text, "[Loading…", Rect::new(26, 100, 428, 40)),
    ];
    let own = [
        String::from("Plugin wird geladen…"),
        String::from("Loading {host} …"),
        String::from("[Loading… ~~]"),
    ];
    let scene = scene(&image, texts).labels_whole(true).ellipses(&own);
    let issues = ellipsis(&scene);
    assert_eq!(checks(&issues), [ELLIPSIS]);
    assert!(issues[0].message.contains("Reading stat"));
}

#[test]
fn a_word_on_two_lines_is_found() {
    let mut image = paper();
    word(&mut image, 30, 10, 4, 16);
    word(&mut image, 30, 30, 3, 16);
    word(&mut image, 30, 100, 7, 16);
    let texts = vec![
        d(Role::Text, "Wrapped", Rect::new(26, 5, 200, 45)),
        d(Role::Text, "Fine", Rect::new(26, 95, 200, 30)),
    ];
    let issues = wrap(&scene(&image, texts));
    assert_eq!(checks(&issues), [WRAP]);
    assert!(issues[0].message.contains("\"Wrapped\""));
}

#[test]
fn parts_off_screen_and_overlapping() {
    let image = paper();
    let parts = vec![
        d(Role::Button, "OK", Rect::new(400, 700, 100, 40)),
        d(Role::Button, "A", Rect::new(10, 10, 60, 40)),
        d(Role::Button, "B", Rect::new(50, 20, 60, 40)),
        d(Role::Row, "", Rect::new(0, 100, 480, 60)),
        d(Role::Row, "", Rect::new(0, 160, 480, 60)),
    ];
    let scene = scene(&image, parts);
    assert_eq!(checks(&off_screen(&scene)), [OFF_SCREEN]);
    assert_eq!(checks(&overlap(&scene)), [OVERLAP]);
}

/// The store's plugin page as it was drawn wrongly (the parts' places as
/// it described them on the X4): the rows left under the header, over
/// the summary; and as it is drawn now, the rows below the facts.
fn store_page(rows_at: u16, install: Role) -> Vec<Described> {
    vec![
        d(
            Role::Text,
            "Reading aids for the words: bold starts of words",
            Rect::new(35, 84, 410, 199),
        ),
        d(
            Role::Text,
            "Data plugin: settings, looks and texts, no code.",
            Rect::new(86, 300, 359, 56),
        ),
        d(
            Role::Text,
            "By Someone, since 2024, 3 plugins",
            Rect::new(86, 363, 359, 28),
        ),
        d(
            Role::Text,
            "Version 1.0.0, 61 B, Licence: MIT",
            Rect::new(86, 397, 359, 28),
        ),
        d(install, "Install, 61 B", Rect::new(0, rows_at, 480, 63)),
        d(
            Role::Row,
            "Source code",
            Rect::new(0, rows_at + 63, 480, 63),
        ),
        // A row's own words lie within it.
        d(
            Role::Text,
            "Source code",
            Rect::new(35, rows_at + 63, 300, 63),
        ),
    ]
}

#[test]
fn text_under_a_row_is_found() {
    let image = paper();
    assert!(overlap(&scene(&image, store_page(442, Role::Row))).is_empty());
    let issues = overlap(&scene(&image, store_page(84, Role::Row)));
    assert_eq!(checks(&issues), [OVERLAP, OVERLAP], "{issues:?}");
    assert!(issues[0].message.contains("\"Reading aids"), "{issues:?}");
    assert!(issues[0].message.contains("Install"), "{issues:?}");
    assert!(issues[1].message.contains("Source code"), "{issues:?}");
}

#[test]
fn text_under_a_control_starting_before_it_is_found() {
    let image = paper();
    // Install as a button, starting left of the summary.
    assert!(overlap(&scene(&image, store_page(442, Role::Button))).is_empty());
    let issues = overlap(&scene(&image, store_page(84, Role::Button)));
    assert!(
        issues
            .iter()
            .any(|i| i.message.contains("\"Reading aids") && i.message.contains("Install")),
        "{issues:?}"
    );
}

#[test]
fn text_under_another_text_is_found() {
    let image = paper();
    let mut parts = store_page(442, Role::Row);
    // The facts begun before the summary's last line.
    parts[1].area.y = 260;
    let issues = overlap(&scene(&image, parts));
    assert_eq!(checks(&issues), [OVERLAP], "{issues:?}");
    assert!(issues[0].message.contains("\"Data plugin"), "{issues:?}");
    // Lines a few pixels into each other's room pass.
    let mut parts = store_page(442, Role::Row);
    parts[2].area.y = 352;
    assert!(overlap(&scene(&image, parts)).is_empty());
}

/// Texts whose boxes reach to the end of the room they were given (a
/// label beside its value, the days of a calendar) lie over each other
/// where nothing of them is drawn: no overlap. Words drawn over each
/// other are.
#[test]
fn texts_are_compared_where_their_words_are() {
    let mut image = paper();
    word(&mut image, 30, 110, 4, 20);
    word(&mut image, 90, 110, 4, 20);
    // A label wrapped beside its value, its lines' boxes to the edge.
    word(&mut image, 30, 210, 9, 20);
    word(&mut image, 30, 250, 7, 20);
    word(&mut image, 380, 230, 6, 20);
    let parts = vec![
        d(Role::Text, "Mo", Rect::new(30, 100, 420, 40)),
        d(Role::Text, "Tu", Rect::new(90, 100, 360, 40)),
        d(Role::Text, "Paragraph", Rect::new(26, 200, 454, 40)),
        d(Role::Text, "spacing", Rect::new(26, 240, 454, 40)),
        d(Role::Text, "Medium", Rect::new(380, 200, 100, 80)),
    ];
    assert!(overlap(&scene(&image, parts.clone())).is_empty());
    // Drawn half over the first: found.
    word(&mut image, 30, 125, 4, 20);
    let mut over = parts;
    over.push(d(Role::Text, "We", Rect::new(30, 120, 60, 40)));
    let issues = overlap(&scene(&image, over));
    assert!(
        issues
            .iter()
            .any(|i| i.message.contains("under the text \"We\"")),
        "{issues:?}"
    );
}

#[test]
fn text_running_into_a_switch_is_found() {
    let mut image = paper();
    let row = Rect::new(0, 100, 480, 60);
    let switch = Rect::new(374, 115, 80, 30);
    fill(&mut image, switch, INK);
    // Ends a gap before the switch, then a text running into it.
    word(&mut image, 26, 120, 20, 20);
    let fine = vec![
        d(Role::Row, "", row),
        d(Role::Text, "Night mode", Rect::new(26, 100, 330, 60)),
        d(Role::Switch, "", switch),
    ];
    assert!(overlap(&scene(&image, fine)).is_empty());
    word(&mut image, 26, 220, 72, 20);
    let switch = Rect::new(374, 215, 80, 30);
    fill(&mut image, switch, INK);
    let long = vec![
        d(Role::Text, "A very long title", Rect::new(26, 200, 454, 60)),
        d(Role::Switch, "", switch),
    ];
    assert_eq!(checks(&overlap(&scene(&image, long))), [OVERLAP]);
}

#[test]
fn tiles_of_a_grid_lie_edge_to_edge() {
    let image = paper();
    // Seven tiles in three columns: the last row has one.
    let tiles = |at: &[(u16, u16)]| {
        at.iter()
            .map(|(c, r)| {
                d(
                    Role::Button,
                    "",
                    Rect::new(30 + c * 140, 100 + r * 140, 140, 140),
                )
            })
            .collect::<Vec<_>>()
    };
    let grid = [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1), (0, 2)];
    assert!(button_room(&scene(&image, tiles(&grid))).is_empty());
    // Buttons of a size in one row, edge to edge, are no grid.
    let issues = button_room(&scene(&image, tiles(&[(0, 0), (1, 0), (2, 0)])));
    assert_eq!(checks(&issues), [BUTTON_ROOM]);
}

#[test]
fn a_label_reaching_out_of_its_button() {
    let mut image = paper();
    let tile = Rect::new(100, 100, 100, 60);
    // The label's descenders below the tile.
    word(&mut image, 120, 140, 8, 26);
    let parts = vec![
        d(Role::Button, "", tile),
        d(Role::Text, "Battery", Rect::new(110, 135, 90, 40)),
    ];
    let issues = overlap(&scene(&image, parts));
    assert_eq!(checks(&issues), [OVERLAP]);
    assert!(issues[0].message.contains("reaches out of"), "{issues:?}");
}

#[test]
fn icons_off_their_column() {
    let mut image = paper();
    let mut parts = Vec::new();
    for (i, x) in [30u16, 30, 30, 22].iter().enumerate() {
        let y = 100 + i as u16 * 60;
        fill(&mut image, Rect::new(*x, y + 20, 20, 20), INK);
        word(&mut image, 77, y + 20, 5, 20);
        parts.push(d(Role::Row, "", Rect::new(0, y, 480, 60)));
        parts.push(d(Role::Text, "Label", Rect::new(77, y, 377, 60)));
    }
    let issues = align(&scene(&image, parts));
    assert_eq!(checks(&issues), [ALIGN]);
    assert!(issues[0].message.contains("8 px left"), "{issues:?}");
}

/// A dotted separator below some rows, running on under their labels,
/// and a label's first letter reaching in: neither moves their icons.
#[test]
fn separators_and_letters_are_no_icons() {
    let mut image = paper();
    let mut parts = Vec::new();
    for i in 0..4u16 {
        let y = 100 + i * 60;
        fill(&mut image, Rect::new(30, y + 20, 20, 20), INK);
        word(&mut image, 77, y + 20, 5, 20);
        if i < 3 {
            // A line of dots from the margin on under the label.
            for x in (26..454).step_by(2) {
                fill(&mut image, Rect::new(x, y + 58, 1, 1), INK);
            }
        }
        // A letter that starts a pixel before its described text.
        fill(&mut image, Rect::new(75, y + 20, 3, 20), INK);
        parts.push(d(Role::Row, "", Rect::new(0, y, 480, 60)));
        parts.push(d(Role::Text, "Label", Rect::new(77, y, 377, 60)));
    }
    assert!(align(&scene(&image, parts)).is_empty());
}

#[test]
fn widgets_drawing_outside_their_cell() {
    let baseline = paper();
    let mut image = paper();
    let cell = Rect::new(20, 100, 100, 100);
    fill(&mut image, Rect::new(30, 110, 100, 20), INK);
    let own = vec![d(Role::Button, "Go", Rect::new(30, 150, 120, 30))];
    let scene = scene(&image, Vec::new()).cell(Some(cell));
    let issues = outside_cell(&scene, &baseline, &own);
    assert_eq!(checks(&issues), [OUTSIDE_CELL, OUTSIDE_CELL]);
    assert_eq!(issues[0].area, Rect::new(121, 110, 9, 20));
    let inside = paper();
    let scene =
        super::Scene::new(&inside, Vec::new(), Rect::new(0, 0, 480, 755), ui()).cell(Some(cell));
    assert!(outside_cell(&scene, &baseline, &[]).is_empty());
}

#[test]
fn rows_off_the_margin() {
    let mut image = paper();
    let mut parts = Vec::new();
    for (i, x) in [26u16, 26, 33, 26].iter().enumerate() {
        let y = 100 + i as u16 * 60;
        word(&mut image, *x, y + 20, 5, 20);
        parts.push(d(Role::Row, "", Rect::new(0, y, 480, 60)));
        parts.push(d(Role::Text, "Label", Rect::new(*x, y, 454 - x, 60)));
    }
    let scene = scene(&image, parts);
    let issues = margins(&scene);
    assert_eq!(checks(&issues), [MARGIN]);
    assert!(issues[0].message.contains("7 px right"), "{issues:?}");
    let issues = align(&scene);
    assert_eq!(checks(&issues), [ALIGN]);
}

#[test]
fn a_fields_outline_is_not_an_icon_in_the_margin() {
    // A search field: its outline at the focus, half the margin in, the
    // icon and the words from the margin on. A second row's icon
    // reaches into the margin.
    let mut image = paper();
    let field = Rect::new(13, 102, 454, 56);
    fill(&mut image, Rect::new(field.x, field.y, field.width, 2), INK);
    fill(&mut image, Rect::new(field.x, 156, field.width, 2), INK);
    fill(
        &mut image,
        Rect::new(field.x, field.y, 2, field.height),
        INK,
    );
    fill(&mut image, Rect::new(465, field.y, 2, field.height), INK);
    fill(&mut image, Rect::new(26, 120, 20, 20), INK);
    word(&mut image, 60, 120, 5, 20);
    fill(&mut image, Rect::new(16, 180, 20, 20), INK);
    word(&mut image, 60, 180, 5, 20);
    let parts = vec![
        d(Role::Row, "", Rect::new(0, 100, 480, 60)),
        d(Role::Text, "Search", Rect::new(60, 100, 394, 60)),
        d(Role::Row, "", Rect::new(0, 160, 480, 60)),
        d(Role::Text, "Label", Rect::new(60, 160, 394, 60)),
    ];
    let scene = scene(&image, parts);
    let issues = margins(&scene);
    assert_eq!(checks(&issues), [MARGIN], "{issues:?}");
    assert!(issues[0].message.starts_with("\"Label\""), "{issues:?}");
}

#[test]
fn text_in_the_margin() {
    let mut image = paper();
    word(&mut image, 6, 20, 5, 20);
    word(&mut image, 26, 80, 5, 20);
    let parts = vec![
        d(Role::Text, "Edge", Rect::new(6, 10, 200, 40)),
        d(Role::Text, "Fine", Rect::new(26, 70, 200, 40)),
    ];
    let issues = margins(&scene(&image, parts));
    assert_eq!(checks(&issues), [MARGIN]);
    assert!(
        issues[0].message.contains("20 px inside the left"),
        "{issues:?}"
    );
}

#[test]
fn button_labels_off_centre() {
    let mut image = paper();
    let centred = Rect::new(100, 100, 100, 40);
    let off = Rect::new(100, 200, 100, 40);
    for b in [centred, off] {
        fill(&mut image, b, INK);
        fill(&mut image, b.inset(2, 2), PAPER);
    }
    fill(&mut image, Rect::new(140, 110, 20, 20), INK);
    fill(&mut image, Rect::new(143, 210, 20, 20), INK);
    let parts = vec![d(Role::Button, "+", centred), d(Role::Button, "+", off)];
    let issues = ink_centre(&scene(&image, parts));
    assert_eq!(checks(&issues), [INK_CENTRE]);
    assert!(issues[0].message.contains("3 px right of"), "{issues:?}");
}

#[test]
fn small_touch_targets() {
    let image = paper();
    let parts = vec![
        d(Role::Button, "x", Rect::new(10, 10, 30, 30)),
        d(Role::Button, "OK", Rect::new(10, 100, 120, 70)),
        d(Role::Row, "", Rect::new(0, 200, 480, 70)),
        d(Role::Switch, "", Rect::new(380, 220, 60, 30)),
    ];
    let mut scene = scene(&image, parts);
    assert!(touch_targets(&scene).is_empty());
    scene.ui.finger = 61;
    let issues = touch_targets(&scene);
    assert_eq!(checks(&issues), [TOUCH_TARGET]);
    assert!(issues[0].message.contains("3.5 mm"), "{issues:?}");
}

/// Narrow keys pass in a row of many on the keyboard's screen only.
#[test]
fn narrow_keys_pass_on_the_keyboard_alone() {
    let image = paper();
    let parts: Vec<_> = (0..10u16)
        .map(|i| d(Role::Button, "q", Rect::new(26 + i * 43, 400, 43, 70)))
        .collect();
    let mut scene = scene(&image, parts);
    scene.ui.finger = 61;
    assert_eq!(checks(&touch_targets(&scene)), [TOUCH_TARGET]);
    let scene = scene.keyboard(true);
    assert!(touch_targets(&scene).is_empty());
}

/// A keyboard's keys nearly round, 4 mm and a little more each way (35 px
/// here), a thin seam apart: neither too small nor too close there; as
/// buttons elsewhere, both.
#[test]
fn low_keys_a_seam_apart_pass_on_the_keyboard_alone() {
    let image = paper();
    let mut parts: Vec<_> = (0..10u16)
        .map(|i| d(Role::Button, "q", Rect::new(28 + i * 42, 400, 38, 41)))
        .collect();
    // The row below with fewer, wider keys (the space bar's).
    parts.extend([
        d(Role::Button, "123", Rect::new(28, 445, 80, 41)),
        d(Role::Button, "Space", Rect::new(112, 445, 248, 41)),
        d(Role::Button, "OK", Rect::new(364, 445, 80, 41)),
    ]);
    let mut scene = scene(&image, parts);
    scene.ui.finger = 61;
    assert_eq!(checks(&touch_targets(&scene)), [TOUCH_TARGET]);
    assert_eq!(checks(&button_room(&scene)), [BUTTON_ROOM]);
    let scene = scene.keyboard(true);
    assert!(touch_targets(&scene).is_empty());
    assert!(button_room(&scene).is_empty());
}

/// A keyboard's keys keep a quarter of the margin from the screen's sides:
/// closer is found, as for other buttons.
#[test]
fn keyboard_keys_keep_a_thin_margin_from_the_sides() {
    let image = paper();
    let row = |x0: u16| -> Vec<_> {
        (0..10u16)
            .map(|i| d(Role::Button, "q", Rect::new(x0 + i * 46, 400, 42, 45)))
            .chain([
                d(Role::Button, "123", Rect::new(x0, 449, 80, 45)),
                d(Role::Button, "Space", Rect::new(x0 + 84, 449, 248, 45)),
            ])
            .collect()
    };
    let near = scene(&image, row(10)).keyboard(true);
    assert!(button_room(&near).is_empty(), "{:?}", button_room(&near));
    let touching = scene(&image, row(0)).keyboard(true);
    assert_eq!(checks(&button_room(&touching)), [BUTTON_ROOM]);
    // Not a keyboard: the margin of the screen's sides.
    let plain = scene(&image, row(10));
    assert_eq!(checks(&button_room(&plain)), [BUTTON_ROOM]);
}

/// A word split onto two lines in a box drawn tight round it is still
/// found; a button's own label is looked at even where it holds a text
/// of something else.
#[test]
fn tight_texts_and_buttons_with_other_words_are_looked_at() {
    let mut image = paper();
    word(&mut image, 30, 10, 4, 16);
    word(&mut image, 30, 30, 3, 16);
    let tight = vec![d(Role::Text, "Wrapped", Rect::new(30, 10, 70, 36))];
    assert_eq!(checks(&wrap(&scene(&image, tight))), [WRAP]);
    let mut image = paper();
    word(&mut image, 40, 110, 4, 16);
    word(&mut image, 40, 130, 3, 16);
    let parts = vec![
        d(Role::Button, "Wrapped", Rect::new(26, 100, 120, 120)),
        d(Role::Text, "Note", Rect::new(40, 190, 40, 20)),
    ];
    assert_eq!(checks(&wrap(&scene(&image, parts))), [WRAP]);
}

/// Words on a box filled round them, its corners round (a day marked in
/// a calendar): one line, not three.
#[test]
fn words_on_a_filled_box_with_round_corners_are_one_line() {
    let mut image = paper();
    let cell = Rect::new(100, 100, 45, 45);
    fill(&mut image, cell, INK);
    // The paper left in its round corners.
    for (x, y) in [(100, 100), (143, 100), (100, 143), (143, 143)] {
        fill(&mut image, Rect::new(x, y, 2, 2), PAPER);
    }
    for i in 0..2u16 {
        fill(&mut image, Rect::new(112 + i * 12, 112, 6, 20), PAPER);
    }
    let parts = vec![d(Role::Text, "14", cell)];
    assert!(wrap(&scene(&image, parts)).is_empty());
}

#[test]
fn focus_must_change_the_picture() {
    let before = paper();
    let mut after = paper();
    let row = Rect::new(0, 100, 480, 60);
    assert!(!focus_shows(&before, &after, row));
    fill(&mut after, row.inset(13, 2), INK);
    assert!(focus_shows(&before, &after, row));
}

#[test]
fn contrast_on_black_and_white_and_gray() {
    let mut image = paper();
    word(&mut image, 26, 20, 5, 20);
    // A dithered button: every other pixel.
    let button = Rect::new(100, 100, 40, 40);
    for y in button.y..button.bottom() as u16 {
        for x in button.x..button.right() as u16 {
            if (x + y) % 2 == 0 {
                image.set_pixel(u32::from(x), u32::from(y), INK);
            }
        }
    }
    let parts = vec![
        d(Role::Text, "Shown", Rect::new(26, 10, 200, 40)),
        d(Role::Text, "Gone", Rect::new(26, 60, 200, 40)),
        d(Role::Button, "", button),
    ];
    let mono = scene(&image, parts.clone()).panel(Panel::Mono);
    assert_eq!(checks(&contrast(&mono)), [CONTRAST, CONTRAST]);

    let mut gray = paper();
    word(&mut gray, 26, 20, 5, 20);
    for i in 0..5 {
        fill(&mut gray, Rect::new(26 + i * 5, 70, 3, 20), [170, 170, 170]);
    }
    let parts = vec![
        d(Role::Text, "Dark", Rect::new(26, 10, 200, 40)),
        d(Role::Text, "Light", Rect::new(26, 60, 200, 40)),
    ];
    let issues = contrast(&scene(&gray, parts));
    assert_eq!(checks(&issues), [CONTRAST]);
    assert!(issues[0].message.contains("\"Light\""), "{issues:?}");
    assert!(contrast_ratio(0, 255) > 20.0);
}
