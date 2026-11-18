//! The checks on one picture and what its drawing described. Each takes
//! a [`Scene`] and says what it found; none knows about sessions, so they
//! are tested with small hand-made pictures.

use paperpaw_core::sdk::semantics::{Described, Role};
use paperpaw_platform::{Point, Rect};

use super::scene::{INK, Ink, Issue, Panel, Scene, inside, quote, says_something};
use crate::image::Image;

pub const TEXT_CUT: &str = "visual.text-cut";
pub const ELLIPSIS: &str = "visual.ellipsis";
pub const EXPLANATION_CUT: &str = "visual.explanation-cut";
pub const WRAP: &str = "visual.wrap";
pub const OUTSIDE_CELL: &str = "visual.outside-cell";
pub const OFF_SCREEN: &str = "visual.off-screen";
pub const OVERLAP: &str = "visual.overlap";
pub const MARGIN: &str = "visual.margin";
pub const ALIGN: &str = "visual.align";
pub const INK_CENTRE: &str = "visual.ink-centre";
pub const TOUCH_TARGET: &str = "visual.touch-target";
pub const BUTTON_ROOM: &str = "visual.button-room";
pub const FOCUS: &str = "visual.focus";
pub const CONTRAST: &str = "visual.contrast";

/// What a finger or a button acts on.
const CONTROLS: &[Role] = &[Role::Button, Role::Switch, Role::Slider];

/// The checks on the picture alone, for screens and widgets.
pub fn all(scene: &Scene<'_>) -> Vec<Issue> {
    let mut issues = Vec::new();
    issues.extend(text_cut(scene));
    issues.extend(ellipsis(scene));
    issues.extend(wrap(scene));
    issues.extend(off_screen(scene));
    issues.extend(overlap(scene));
    if scene.cell.is_none() {
        issues.extend(margins(scene));
        issues.extend(align(scene));
    }
    issues.extend(ink_centre(scene));
    issues.extend(touch_targets(scene));
    if scene.cell.is_none() {
        issues.extend(button_room(scene));
    }
    issues.extend(contrast(scene));
    issues
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Heading => "heading",
        Role::Row => "row",
        Role::Button => "button",
        Role::Switch => "switch",
        Role::Slider => "slider",
        Role::Text => "text",
        Role::Check => "check box",
        Role::Radio => "option",
        Role::Field => "field",
        Role::Image => "picture",
        Role::Link => "link",
        Role::Dialog => "dialog",
        Role::Progress => "progress bar",
        _ => "part",
    }
}

/// A part's name for messages: its role and label.
fn named(d: &Described) -> String {
    if d.label.trim().is_empty() {
        format!("a {}", role_name(d.role))
    } else {
        format!("the {} {}", role_name(d.role), quote(&d.label))
    }
}

/// The text drawn inside `row`, the first that says something.
fn row_label<'s>(scene: &'s Scene<'_>, row: &Described) -> Option<&'s Described> {
    scene
        .parts(&[Role::Text])
        .filter(|t| says_something(&t.label) && inside_band(t.area, row.area))
        .min_by_key(|t| (t.area.y, t.area.x))
}

/// Whether `inner` starts and lies, top to bottom, within `outer` (text
/// boxes reach to the end of their row, so only that is asked).
fn inside_band(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.x < outer.right() as u16
        && inner.y >= outer.y
        && inner.bottom() <= outer.bottom()
}

/// The ink of the text `d` in `region` (part of its box): without lines
/// across it and without what else is drawn there (switches and buttons;
/// for a text, other texts in its box, e.g. a value or a second line).
fn text_ink(scene: &Scene<'_>, d: &Described, region: Rect, threshold: u8) -> Ink {
    let background = scene.background(region);
    let mut ink = scene.ink_with(region, background, threshold);
    ink.clear_lines();
    for other in &scene.described {
        if std::ptr::eq(other, d) || other.area == d.area || inside(d.area, other.area) {
            continue;
        }
        let control = CONTROLS.contains(&other.role) && other.area.x > d.area.x;
        let text = d.role == Role::Text
            && other.role == Role::Text
            && (other.area.x > d.area.x || other.area.y != d.area.y);
        if control || text {
            ink.clear(other.area);
        }
    }
    ink
}

/// Where the text `d` is drawn: its run of ink from where it starts, up
/// to a gap wider than a word's space; with the height of its tallest
/// line. `None` without ink.
fn text_run(scene: &Scene<'_>, d: &Described, threshold: u8) -> Option<(Rect, u16)> {
    let area = scene.visible(d).and_then(|v| d.area.intersection(&v))?;
    let ink = text_ink(scene, d, area, threshold);
    let lines = ink.lines(2);
    let height = lines.iter().map(|l| l.1).max()?;
    let (first, last) = ink.run_from(0, (height * 2 / 5).max(3))?;
    let top = lines.iter().map(|l| l.0).min().unwrap_or(0);
    let bottom = lines.iter().map(|l| l.0 + l.1).max().unwrap_or(0);
    Some((
        Rect::new(
            area.x + first as u16,
            area.y + top as u16,
            (last - first + 1) as u16,
            (bottom - top) as u16,
        ),
        height as u16,
    ))
}

/// Where the words of text `d` are drawn: all ink in its box from where
/// it starts, up to a gap wider than a word's space, whatever else drew
/// there (unlike [`text_run`], which leaves out what other parts drew, so
/// words drawn over each other would hide each other); `None` without
/// ink.
fn words(scene: &Scene<'_>, d: &Described) -> Option<Rect> {
    let area = scene.visible(d).and_then(|v| d.area.intersection(&v))?;
    let ink = scene.ink(area, scene.background(area));
    let lines = ink.lines(2);
    let height = lines.iter().map(|l| l.1).max()?;
    let (first, last) = ink.run_from(0, (height * 2 / 5).max(3))?;
    let top = lines.iter().map(|l| l.0).min().unwrap_or(0);
    let bottom = lines.iter().map(|l| l.0 + l.1).max().unwrap_or(0);
    Some(Rect::new(
        area.x + first as u16,
        area.y + top as u16,
        (last - first + 1) as u16,
        (bottom - top) as u16,
    ))
}

/// Whether `d` lies in a focused part (drawn on the focus bar).
fn on_focus(scene: &Scene<'_>, d: &Described) -> bool {
    scene
        .described
        .iter()
        .any(|f| f.focused && inside_band(d.area, f.area))
}

// ---- Text ---------------------------------------------------------------

/// Text whose box reaches past what can be seen of it (its clip or the
/// screen) with ink at that edge: letters cut in half.
pub fn text_cut(scene: &Scene<'_>) -> Vec<Issue> {
    let mut issues = Vec::new();
    for d in scene.parts(&[Role::Text, Role::Heading]) {
        if !says_something(&d.label) {
            continue;
        }
        let Some(visible) = scene.visible(d) else {
            continue;
        };
        let Some(shown) = d.area.intersection(&visible) else {
            continue;
        };
        let background = scene.background(shown);
        let edge_ink = |edge: Rect| {
            let ink = scene.ink(edge, background);
            let span = usize::from(edge.width.max(edge.height));
            let count = ink.count();
            // Letters cross the edge in places; a frame or line along it
            // fills it.
            count >= 2 && count * 10 < span * 7
        };
        let sides = [
            (
                "right",
                d.area.right() > visible.right(),
                Rect::new(visible.right() as u16 - 1, shown.y, 1, shown.height),
            ),
            (
                "left",
                d.area.x < visible.x,
                Rect::new(visible.x, shown.y, 1, shown.height),
            ),
            (
                "bottom",
                d.area.bottom() > visible.bottom(),
                Rect::new(shown.x, visible.bottom() as u16 - 1, shown.width, 1),
            ),
            (
                "top",
                d.area.y < visible.y,
                Rect::new(shown.x, visible.y, shown.width, 1),
            ),
        ];
        if let Some((side, ..)) = sides
            .iter()
            .find(|(_, past, edge)| *past && edge_ink(*edge))
        {
            let place = if inside(visible, scene.screen) && visible != scene.screen {
                "the area it is drawn in"
            } else {
                "the screen"
            };
            issues.push(Issue::error(
                TEXT_CUT,
                shown,
                format!(
                    "{} is cut off at the {side} edge of {place}: shorten it or give it more room",
                    quote(&d.label)
                ),
            ));
        }
    }
    issues
}

/// Text shortened with an ellipsis to fit: a label, a heading, a
/// button's word. A value in a field may be shortened (it is the reader's
/// own, typed); it is left alone.
pub fn ellipsis(scene: &Scene<'_>) -> Vec<Issue> {
    let mut issues: Vec<Issue> = Vec::new();
    let fields: Vec<_> = scene.parts(&[Role::Field]).map(|f| f.area).collect();
    let buttons: Vec<_> = scene.parts(&[Role::Button]).map(|b| b.area).collect();
    for d in scene.parts(&[Role::Text, Role::Heading, Role::Button]) {
        let label = d.label.trim_end();
        if !label.ends_with('…') || !says_something(label) {
            continue;
        }
        if fields.iter().any(|f| f.intersection(&d.area).is_some()) {
            continue;
        }
        // A line that ends at a "…" of the text itself (after what fills
        // its last blank, "Loading {host} …"): it begins as the text does
        // before its first blank, so that not every "…" passes as one.
        let own = |text: &String| {
            text.contains(label)
                || text.rsplit_once('}').is_some_and(|(_, tail)| {
                    let head = text.split('{').next().unwrap_or_default().trim_start();
                    tail.contains('…')
                        && label.ends_with(tail.trim_end())
                        && !head.is_empty()
                        && label.starts_with(head)
                })
        };
        if scene.ellipses.iter().any(own) {
            continue;
        }
        // Shortened to fit, the text fills its room to the end (less a
        // margin); a text that ends with "…" of itself ("Loading…") need
        // not.
        let Some((run, height)) = text_run(scene, d, INK) else {
            continue;
        };
        // A text of several lines cut short: an explanation, which must be
        // read whole, wherever its last line ends. Not a button's label
        // (the ink of a button filled is no lines of text).
        let on_button = d.role == Role::Button
            || buttons
                .iter()
                .any(|b| b.intersection(&d.area) == Some(d.area));
        if run.height > height + height / 2 && !on_button {
            let message = format!(
                "{} is cut short: an explanation shows whole; give it the room there is, or a \
                 way to read all of it (a row that opens it)",
                quote(label)
            );
            if !issues.iter().any(|i| i.message == message) {
                issues.push(Issue::error(EXPLANATION_CUT, run, message));
            }
            continue;
        }
        let room_end = scene
            .visible(d)
            .map_or(d.area.right(), |v| v.right().min(d.area.right()));
        let slack = u32::from(scene.ui.margin + 2 * height);
        if !scene.labels_whole && run.right() + slack < room_end {
            continue;
        }
        let area = run;
        let message = format!(
            "{} is shortened to fit: give it more room or find a shorter text",
            quote(label)
        );
        if !issues.iter().any(|i| i.message == message) {
            issues.push(Issue::warning(ELLIPSIS, area, message));
        }
    }
    issues
}

/// Whether a label is short enough that it should fit on one line.
fn short_label(label: &str) -> bool {
    label.split_whitespace().count() <= 3 && label.chars().count() <= 28
}

/// Headings, button labels and single words drawn on more than one line.
pub fn wrap(scene: &Scene<'_>) -> Vec<Issue> {
    let mut issues = Vec::new();
    let inset = scene.ui.line.max(2) * 2 + 1;
    for d in scene.parts(&[Role::Heading, Role::Button, Role::Text]) {
        let label = d.label.trim();
        if !says_something(label) || !short_label(label) {
            continue;
        }
        if d.role == Role::Text && label.contains(char::is_whitespace) {
            continue;
        }
        // A button's label told as a text of its own where it is drawn
        // apart from the rest of the button (a tool's name, or its short
        // form, below its icon): that text is looked at, not the button
        // with its icon.
        let apart = |b: &Described, t: &Described| {
            let (label, words) = (b.label.trim().to_lowercase(), t.label.trim().to_lowercase());
            t.role == Role::Text
                && says_something(&words)
                && label.contains(&words)
                && inside(t.area, b.area)
                && t.area.height * 3 < b.area.height * 2
        };
        if d.role == Role::Button && scene.parts(&[Role::Text]).any(|t| apart(d, t)) {
            continue;
        }
        // Else a button's label: looked at with its button, inside its
        // frame.
        let start = Point::new(i32::from(d.area.x), i32::from(d.area.y));
        if d.role == Role::Text
            && scene
                .parts(&[Role::Button])
                .any(|b| b.area.contains(start) && !apart(b, d))
        {
            continue;
        }
        let Some(visible) = scene.visible(d) else {
            continue;
        };
        let Some(mut region) = d.area.intersection(&visible) else {
            continue;
        };
        if d.role == Role::Button {
            region = region.inset(inset, inset);
        }
        if region.is_empty() {
            continue;
        }
        // Lines of text lie a leading apart; a thin line drawn through
        // the letters (a flip clock's tiles) is no break. A frame round it
        // (a button's, a day marked in a calendar) is no line of it; the
        // letters of a text boxed tightly are left as they are.
        let mut ink = text_ink(scene, d, region, INK);
        // Words in a box filled round them (a day marked in a calendar):
        // looked at on the box's fill.
        if let Some((boxed, (x, y))) = ink.filled_box() {
            let fill = scene.luma(u32::from(x), u32::from(y));
            ink = scene.ink_with(boxed.inset(1, 1), fill, INK);
        }
        if d.role == Role::Button || ink.framed() {
            ink.clear_edges();
        }
        // What a filled box's round corners leave in the corners.
        ink.clear_corners();
        let tallest = ink.lines(2).iter().map(|l| l.1).max().unwrap_or(0);
        let lines = ink.lines((tallest / 6).max(3));
        if lines.len() >= 2 {
            issues.push(Issue::warning(
                WRAP,
                region,
                format!(
                    "{} is broken onto {} lines: a label this short should fit on one",
                    quote(label),
                    lines.len()
                ),
            ));
        }
    }
    issues
}

// ---- Where parts are ----------------------------------------------------

/// Parts reaching past the screen.
pub fn off_screen(scene: &Scene<'_>) -> Vec<Issue> {
    scene
        .parts(&[
            Role::Row,
            Role::Button,
            Role::Switch,
            Role::Slider,
            Role::Heading,
        ])
        .filter(|d| !d.area.is_empty() && !inside(d.area, scene.screen))
        .map(|d| {
            Issue::error(
                OFF_SCREEN,
                d.area,
                format!(
                    "{} reaches past the edge of the screen: lay it out within the screen's area",
                    named(d)
                ),
            )
        })
        .collect()
}

/// What a widget draws outside its cell: the pixels that differ from the
/// home screen without it (`baseline`, the same place empty), and the
/// parts it describes there.
pub fn outside_cell(scene: &Scene<'_>, baseline: &Image, own: &[Described]) -> Vec<Issue> {
    let Some(cell) = scene.cell else {
        return Vec::new();
    };
    let mut issues = Vec::new();
    let around = cell.grow(1);
    if (baseline.width, baseline.height) == (scene.image.width, scene.image.height) {
        let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
        let mut count = 0;
        for y in 0..scene.image.height {
            for x in 0..scene.image.width {
                if x >= u32::from(around.x)
                    && x < around.right()
                    && y >= u32::from(around.y)
                    && y < around.bottom()
                {
                    continue;
                }
                if scene.luma(x, y).abs_diff(baseline.luma(x, y)) > INK {
                    count += 1;
                    left = left.min(x);
                    right = right.max(x);
                    top = top.min(y);
                    bottom = bottom.max(y);
                }
            }
        }
        if count >= 3 {
            let area = Rect::new(
                left as u16,
                top as u16,
                (right - left + 1) as u16,
                (bottom - top + 1) as u16,
            );
            issues.push(Issue::error(
                OUTSIDE_CELL,
                area,
                "draws outside its cell (the grid does not clip widgets): keep the drawing within the area it is given",
            ));
        }
    }
    for d in own {
        if d.role == Role::Text || d.area.is_empty() || inside(d.area, around) {
            continue;
        }
        issues.push(Issue::error(
            OUTSIDE_CELL,
            d.area,
            format!(
                "{} lies outside its cell: keep its parts within the area it is given",
                named(d)
            ),
        ));
    }
    issues
}

/// Buttons, switches and sliders over each other, rows over each other,
/// text under a control, a row or another text, text running into a
/// switch or button.
pub fn overlap(scene: &Scene<'_>) -> Vec<Issue> {
    let mut issues = Vec::new();
    for kinds in [CONTROLS, &[Role::Row][..]] {
        let parts: Vec<&Described> = scene.parts(kinds).collect();
        for (i, a) in parts.iter().enumerate() {
            for b in &parts[i + 1..] {
                if let Some(common) = partly(a.area, b.area) {
                    issues.push(Issue::error(
                        OVERLAP,
                        common,
                        format!("{} and {} overlap", named(a), named(b)),
                    ));
                }
            }
        }
    }
    // Where each text's words are drawn: its box may reach further, to
    // the end of the room it was given (a label beside a value, a day of
    // a calendar), where nothing of it is.
    let drawn: Vec<(&Described, Rect)> = scene
        .parts(&[Role::Text])
        .filter(|t| says_something(&t.label))
        .map(|t| (t, words(scene, t).unwrap_or(t.area)))
        .collect();
    for (i, &(t, own)) in drawn.iter().enumerate() {
        issues.extend(covered(scene, t, own, &drawn[i + 1..]));
    }
    for t in scene.parts(&[Role::Text]) {
        if !says_something(&t.label) {
            continue;
        }
        let Some(visible) = scene.visible(t) else {
            continue;
        };
        issues.extend(out_of_button(scene, t));
        for c in scene.parts(CONTROLS) {
            if inside(t.area, c.area) || c.area.x <= t.area.x + 2 {
                continue;
            }
            let top = t.area.y.max(c.area.y);
            let bottom = (t.area.bottom().min(c.area.bottom())) as u16;
            if bottom <= top || (bottom - top) * 2 < c.area.height.min(t.area.height) {
                continue;
            }
            if c.area.x >= t.area.right() as u16 {
                continue;
            }
            let band = Rect::new(
                t.area.x,
                top,
                c.area.right() as u16 - t.area.x,
                bottom - top,
            );
            let Some(band) = band.intersection(&visible) else {
                continue;
            };
            let before = Rect::new(band.x, band.y, c.area.x - band.x, band.height);
            let background = scene.background(before);
            let text = scene.ink(before, background);
            let Some(height) = text.lines(2).iter().map(|l| l.1).max() else {
                continue;
            };
            let ink = scene.ink(band, background);
            let Some((first, last)) = ink.run_from(0, (height * 2 / 5).max(3)) else {
                continue;
            };
            if band.x + last as u16 > c.area.x {
                issues.push(Issue::error(
                    OVERLAP,
                    Rect::new(
                        band.x + first as u16,
                        band.y,
                        (last - first + 1) as u16,
                        band.height,
                    ),
                    format!(
                        "{} runs into {}: end it a gap before",
                        quote(&t.label),
                        named(c)
                    ),
                ));
            }
        }
    }
    issues
}

/// Two areas that lie partly over each other (by 2 px at least), neither
/// within the other: where they do.
fn partly(a: Rect, b: Rect) -> Option<Rect> {
    a.intersection(&b)
        .filter(|i| i.width >= 2 && i.height >= 2)
        .filter(|_| !inside(a, b) && !inside(b, a))
}

/// Whether a text is a part's own (a row's or a button's words): at
/// least half of it lies within the part.
fn belongs(text: Rect, part: Rect) -> bool {
    let size = |r: Rect| u32::from(r.width) * u32::from(r.height);
    text.intersection(&part)
        .is_some_and(|common| size(common) * 2 >= size(text))
}

/// A text with words, drawn at `own`, that lies partly under a part it is
/// not of: a row, a control starting where the text starts or before it
/// (one starting within the text is looked at by its ink, in `overlap`),
/// or one of the texts `later` (each drawn where it says). Drawn where the
/// rows are, or the rows placed where it is.
fn covered(
    scene: &Scene<'_>,
    t: &Described,
    own: Rect,
    later: &[(&Described, Rect)],
) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut under = |part: &Described, common: Rect| {
        issues.push(Issue::error(
            OVERLAP,
            common,
            format!(
                "{} lies under {}: place each where the other is not",
                quote(&t.label),
                named(part)
            ),
        ));
    };
    for r in scene.parts(&[Role::Row]) {
        if let Some(common) = partly(own, r.area).filter(|_| !belongs(own, r.area)) {
            under(r, common);
        }
    }
    for c in scene.parts(CONTROLS) {
        if c.area.x > own.x + 2 {
            continue;
        }
        if let Some(common) = partly(own, c.area).filter(|_| !belongs(own, c.area)) {
            under(c, common);
        }
    }
    for &(other, theirs) in later {
        // Lines a few pixels into each other's room are no overlap.
        let lower = own.height.min(theirs.height);
        if let Some(common) = partly(own, theirs).filter(|c| c.height * 3 >= lower) {
            under(other, common);
        }
    }
    issues
}

/// A text that starts in a button, is the button's own but whose ink
/// reaches out of it: the button's frame, or its focus, cuts through the
/// text.
fn out_of_button(scene: &Scene<'_>, t: &Described) -> Option<Issue> {
    let start = Point::new(i32::from(t.area.x), i32::from(t.area.y));
    let b = scene
        .parts(&[Role::Button])
        .filter(|b| b.area.contains(start) && !inside(t.area, b.area))
        .filter(|b| belongs(t.area, b.area))
        .last()?;
    let (run, _) = text_run(scene, t, INK)?;
    let out = run.right() > b.area.right() + 1 || run.bottom() > b.area.bottom() + 1;
    out.then(|| {
        Issue::warning(
            OVERLAP,
            run,
            format!(
                "{} reaches out of {}: its frame or focus cuts through the text; keep the text within the button",
                quote(&t.label),
                named(b)
            ),
        )
    })
}

// ---- Margins and alignment ----------------------------------------------

/// The rows across most of the screen (lists, not grids), unfocused.
fn list_rows<'s>(scene: &'s Scene<'_>) -> impl Iterator<Item = &'s Described> + 's {
    let half = scene.screen.width / 2;
    scene
        .parts(&[Role::Row])
        .filter(move |r| !r.focused && r.area.width >= half && r.area.height >= 8)
}

/// Where the content of the screen's list rows and headings starts,
/// from their left edge: the least inset, an icon's or a label's. `None`
/// without such rows.
pub fn content_inset(scene: &Scene<'_>) -> Option<u16> {
    let half = scene.screen.width / 2;
    list_rows(scene)
        .chain(
            scene
                .parts(&[Role::Heading])
                .filter(move |h| h.area.width >= half),
        )
        .filter_map(|c| {
            let label = if c.role == Role::Heading {
                heading_text(scene, c)
            } else {
                row_label(scene, c)
            }?;
            // An icon is drawn from the margin, its ink a little in.
            let margin = c.area.x + scene.ui.margin;
            let start = drawn_before(scene, c.area, label).map_or(label.area.x, |icon| {
                if icon.x + scene.ui.tolerance() >= margin {
                    margin
                } else {
                    icon.x
                }
            });
            Some(start.saturating_sub(c.area.x))
        })
        .min()
}

/// Groups `(label, area, offset)` by offset into issues: one a distinct
/// offset, naming up to three.
fn grouped(
    check: &'static str,
    found: Vec<(String, Rect, i32)>,
    say: impl Fn(i32, &str) -> String,
) -> Vec<Issue> {
    let mut offsets: Vec<i32> = found.iter().map(|f| f.2).collect();
    offsets.sort_unstable();
    offsets.dedup();
    offsets
        .into_iter()
        .map(|offset| {
            let of: Vec<&(String, Rect, i32)> = found.iter().filter(|f| f.2 == offset).collect();
            let mut names: Vec<String> = of.iter().take(3).map(|f| f.0.clone()).collect();
            if of.len() > 3 {
                names.push(format!("{} more", of.len() - 3));
            }
            let area = of.iter().fold(Rect::default(), |a, f| a.union(&f.1));
            Issue::warning(check, area, say(offset, &names.join(", ")))
        })
        .collect()
}

fn side(offset: i32, left: &'static str, right: &'static str) -> &'static str {
    if offset < 0 { left } else { right }
}

/// The text a heading shows (its own drawn text), the first that says
/// something.
fn heading_text<'s>(scene: &'s Scene<'_>, h: &Described) -> Option<&'s Described> {
    scene
        .parts(&[Role::Text])
        .filter(|t| says_something(&t.label) && inside_band(t.area, h.area))
        .min_by_key(|t| t.area.x)
}

/// Whether something is drawn before `text` in its row or heading (an
/// icon, a back arrow): ink between the container's edge and the text,
/// level with its line (an icon a little higher and lower).
fn drawn_before(scene: &Scene<'_>, container: Rect, text: &Described) -> Option<Rect> {
    let (line, _) = text_run(scene, text, INK)?;
    let reach = line.height / 4;
    let before = Rect::new(
        container.x,
        line.y.saturating_sub(reach),
        text.area.x.saturating_sub(container.x + 1),
        line.height + 2 * reach,
    )
    .intersection(&scene.screen)?;
    let mut ink = scene.ink_in(before);
    ink.clear_lines();
    // A field's outline (the settings' search) passes by at the focus,
    // inside the margin: its side is not drawn before the words.
    ink.clear_sides();
    ink.bounds()
}

/// Content against the interface's margin: the labels of list rows and
/// headings that start off it (where the code put them, so the side
/// bearings of letters do not count), icons reaching into it, text
/// inside the screen's margins, controls at a row's end off the right
/// margin.
pub fn margins(scene: &Scene<'_>) -> Vec<Issue> {
    let ui = scene.ui;
    let margin = i32::from(ui.margin);
    let tolerance = i32::from(ui.tolerance());
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    let half = scene.screen.width / 2;
    let containers = list_rows(scene).chain(
        scene
            .parts(&[Role::Heading])
            .filter(move |h| h.area.width >= half),
    );
    for c in containers {
        let label = if c.role == Role::Heading {
            heading_text(scene, c)
        } else {
            row_label(scene, c)
        };
        let Some(label) = label else {
            continue;
        };
        let offset = i32::from(label.area.x) - i32::from(c.area.x) - margin;
        match drawn_before(scene, c.area, label) {
            // An icon or arrow first: it, not the label, starts at the
            // margin; only one reaching into the margin is wrong.
            Some(icon) => {
                let into = i32::from(icon.x) - i32::from(c.area.x) - margin;
                if into < -tolerance && i32::from(icon.x) > i32::from(c.area.x) {
                    starts.push((quote(&label.label), icon, into));
                }
            }
            // Off by a little is a slip; much more is an indent meant.
            None if offset.abs() >= 2 && offset.abs() < margin => {
                starts.push((quote(&label.label), label.area, offset));
            }
            None => {}
        }
        if c.role != Role::Row {
            continue;
        }
        // Switches and buttons at the row's end end at the margin.
        for k in scene.parts(CONTROLS) {
            if !inside(k.area, c.area)
                || k.area.right() + 3 * u32::from(ui.margin) <= c.area.right()
            {
                continue;
            }
            let offset = c.area.right() as i32 - margin - k.area.right() as i32;
            if offset.abs() >= 2 && offset.abs() <= margin {
                ends.push((named(k), k.area, offset));
            }
        }
    }
    let mut issues = grouped(MARGIN, starts, |offset, names| {
        format!(
            "{names} start {} px {} of the margin ({} px from the edge): start content at the margin",
            offset.abs(),
            side(offset, "left", "right"),
            ui.margin
        )
    });
    issues.extend(grouped(MARGIN, ends, |offset, names| {
        format!(
            "{names} end {} px {} of the right margin: end controls at a row's end at the margin",
            offset.abs(),
            side(offset, "right", "left"),
        )
    }));
    issues.extend(text_in_margins(scene));
    issues
}

/// Text inside the margins at the screen's sides.
fn text_in_margins(scene: &Scene<'_>) -> Vec<Issue> {
    let ui = scene.ui;
    let tolerance = ui.tolerance();
    let left_line = scene.screen.x + ui.margin;
    let right_line = scene.screen.right() as u16 - ui.margin;
    let mut found = Vec::new();
    for t in scene.parts(&[Role::Text]) {
        if !says_something(&t.label) {
            continue;
        }
        if on_focus(scene, t) {
            continue;
        }
        let Some((run, _)) = text_run(scene, t, INK) else {
            continue;
        };
        let (left, right) = (run.x, run.right() as u16);
        let area = run;
        let into_left = t.area.x < left_line && left + tolerance < left_line;
        let into_right = right > right_line + tolerance && left > scene.screen.width / 2;
        if into_left || into_right {
            found.push((
                quote(&t.label),
                area,
                if into_left {
                    i32::from(left_line - left)
                } else {
                    -i32::from(right - right_line)
                },
            ));
        }
    }
    found
        .into_iter()
        .map(|(name, area, by)| {
            Issue::warning(
                MARGIN,
                area,
                format!(
                    "{name} lies {} px inside the {} margin of the screen ({} px): keep text within the margins",
                    by.abs(),
                    side(by, "right", "left"),
                    ui.margin
                ),
            )
        })
        .collect()
}

/// The labels of a list's rows start at the same place: where several
/// rows agree, the others should too.
pub fn align(scene: &Scene<'_>) -> Vec<Issue> {
    let rows: Vec<&Described> = list_rows(scene).collect();
    let mut found = Vec::new();
    // Rows of one list: the same left edge and width.
    let mut lists: Vec<(u16, u16)> = rows.iter().map(|r| (r.area.x, r.area.width)).collect();
    lists.sort_unstable();
    lists.dedup();
    for (x, width) in lists {
        let starts: Vec<(&Described, u16)> = rows
            .iter()
            .filter(|r| (r.area.x, r.area.width) == (x, width))
            .filter_map(|r| row_label(scene, r).map(|t| (*r, t.area.x)))
            .collect();
        if starts.len() < 3 {
            continue;
        }
        let mut counts: Vec<(u16, usize)> = Vec::new();
        for (_, s) in &starts {
            match counts.iter_mut().find(|c| c.0 == *s) {
                Some(c) => c.1 += 1,
                None => counts.push((*s, 1)),
            }
        }
        let Some(&(common, n)) = counts.iter().max_by_key(|c| (c.1, std::cmp::Reverse(c.0))) else {
            continue;
        };
        if n * 2 <= starts.len() {
            continue;
        }
        for (row, s) in starts {
            let offset = i32::from(s) - i32::from(common);
            // Off by a little is a slip; much more is an indent meant.
            if offset.abs() >= 2 && offset.abs() < i32::from(scene.ui.margin) {
                let name = row_label(scene, row).map_or_else(|| named(row), |t| quote(&t.label));
                found.push((name, row.area, offset));
            }
        }
    }
    let mut issues = grouped(ALIGN, found, |offset, names| {
        format!(
            "the labels of {names} start {} px {} of the other rows' labels: line them up",
            offset.abs(),
            side(offset, "left", "right")
        )
    });
    issues.extend(icon_column(scene, &rows));
    issues
}

/// The icons before rows' labels centred on one line, by their ink.
fn icon_column(scene: &Scene<'_>, rows: &[&Described]) -> Vec<Issue> {
    let tolerance = i32::from(scene.ui.tolerance());
    let mut centres: Vec<(&Described, String, Rect)> = Vec::new();
    for row in rows {
        let Some(label) = row_label(scene, row) else {
            continue;
        };
        let Some(band) = scene
            .visible(label)
            .and_then(|v| label.area.intersection(&v))
        else {
            continue;
        };
        let before = Rect::new(
            row.area.x,
            band.y,
            label.area.x.saturating_sub(row.area.x),
            band.height,
        );
        if before.width < 4 {
            continue;
        }
        let mut ink = scene.ink_in(before);
        ink.clear_lines();
        ink.clear_right();
        ink.clear_dotted();
        // Ink from the line's top to its bottom is something taller than
        // an icon cut by the line: a cover or a picture, not an icon.
        if let Some(icon) = ink.bounds()
            && icon.height < before.height
        {
            centres.push((row, quote(&label.label), icon));
        }
    }
    if centres.len() < 3 {
        return Vec::new();
    }
    // Twice the centre, to stay in whole pixels.
    let middle = |r: Rect| i32::from(r.x) * 2 + i32::from(r.width);
    let mut all: Vec<i32> = centres.iter().map(|c| middle(c.2)).collect();
    all.sort_unstable();
    let common = all[all.len() / 2];
    let found: Vec<(String, Rect, i32)> = centres
        .into_iter()
        .filter_map(|(_, name, icon)| {
            let offset = (middle(icon) - common) / 2;
            (offset.abs() > tolerance).then_some((name, icon, offset))
        })
        .collect();
    grouped(ALIGN, found, |offset, names| {
        format!(
            "the icons of {names} sit {} px {} of the other rows' icons: centre icons on one line",
            offset.abs(),
            side(offset, "left", "right")
        )
    })
}

// ---- Centred ink ----------------------------------------------------------

/// How far `ink` is off the middle of `outer` along one axis, if it looks
/// meant to be centred (the smaller gap at least half the larger):
/// positive towards the end.
fn off_centre(outer: (u32, u32), ink: (u32, u32)) -> Option<i32> {
    let before = i64::from(ink.0) - i64::from(outer.0);
    let after = i64::from(outer.1) - i64::from(ink.1);
    if before < 0 || after < 0 {
        return None;
    }
    let (small, large) = (before.min(after), before.max(after));
    if small * 2 < large {
        return None;
    }
    Some(((before - after) / 2) as i32)
}

/// Labels and signs of buttons centred by their ink: across, and for a
/// single character or a drawing, up and down too.
pub fn ink_centre(scene: &Scene<'_>) -> Vec<Issue> {
    let mut issues = Vec::new();
    let tolerance = i32::from(scene.ui.tolerance());
    let inset = scene.ui.line.max(2) + 2;
    for b in scene.parts(&[Role::Button]) {
        let Some(visible) = scene.visible(b) else {
            continue;
        };
        if !inside(b.area, visible) || b.area.width < 12 || b.area.height < 12 {
            continue;
        }
        let texts: Vec<&Described> = scene
            .parts(&[Role::Text])
            .filter(|t| {
                let start = Point::new(i32::from(t.area.x), i32::from(t.area.y));
                b.area.contains(start) && says_something(&t.label)
            })
            .collect();
        // Only a label or a drawing of its own: cards with several texts
        // are laid out otherwise; a label reaching out below the button
        // is found by `overlap`.
        if texts.len() > 1 || texts.iter().any(|t| t.area.bottom() > b.area.bottom()) {
            continue;
        }
        // Up and down by ink only for drawings and signs (+, −, arrows):
        // letters sit on their line, with descenders below it.
        let sign = |label: &str| {
            let mut chars = label.trim().chars();
            matches!((chars.next(), chars.next()), (Some(c), None)
                if !c.is_alphanumeric() && !".,;:'\"!?_`".contains(c))
        };
        // Inside its frame, round corners and all.
        let across = b.area.inset(inset, inset);
        let background = scene.background(across);
        let mut inner = scene.ink(across, background);
        inner.clear_edges();
        let Some(ink) = inner.bounds() else {
            continue;
        };
        // A drawing of its own (a word for its name, no text drawn): an
        // icon, but not a flat mark such as a space bar's.
        let drawing = texts.is_empty()
            && b.label.trim().chars().count() > 1
            && ink.width <= ink.height * 3
            && ink.height <= ink.width * 3;
        let single = drawing || sign(&b.label);
        let mut off = Vec::new();
        if let Some(dx) = off_centre(
            (u32::from(b.area.x), b.area.right()),
            (u32::from(ink.x), ink.right()),
        ) && dx.abs() > tolerance
        {
            off.push(format!(
                "{} px {}",
                dx.abs(),
                side(dx, "left of", "right of")
            ));
        }
        if single {
            let middle = Rect::new(
                b.area.x + b.area.width / 5,
                b.area.y + inset,
                b.area.width * 3 / 5,
                b.area.height.saturating_sub(2 * inset),
            );
            let mut inner = scene.ink(middle, background);
            inner.clear_edges();
            if let Some(ink) = inner.bounds()
                && let Some(dy) = off_centre(
                    (u32::from(b.area.y), b.area.bottom()),
                    (u32::from(ink.y), ink.bottom()),
                )
                && dy.abs() > tolerance
            {
                off.push(format!("{} px {}", dy.abs(), side(dy, "above", "below")));
            }
        }
        if !off.is_empty() {
            issues.push(Issue::warning(
                INK_CENTRE,
                b.area,
                format!(
                    "the ink of {} is {} the middle: centre it by its ink",
                    named(b),
                    off.join(" and ")
                ),
            ));
        }
    }
    issues
}

// ---- Touch --------------------------------------------------------------

/// A keyboard's key: on the keyboard's screen, a button as high as the
/// keys of a row of eight or more (its own, or one like it, e.g. the row
/// with the space bar below the letters), as wide as the screen lets
/// that many be and at least 4 mm each way, which a finger hits there
/// (as the keyboard keeps them). Keys lie a thin seam apart.
fn keyboard_key(scene: &Scene<'_>, d: &Described) -> bool {
    if !scene.keyboard || d.role != Role::Button {
        return false;
    }
    let tolerance = scene.ui.tolerance();
    let least = (4.0 * f32::from(scene.ui.dpi) / 25.4) as u16;
    let row = |y: u16| {
        scene
            .parts(&[Role::Button])
            .filter(|b| b.area.y == y && b.area.height == d.area.height)
            .count() as u16
    };
    // The most keys of its height in a row.
    let peers = scene
        .parts(&[Role::Button])
        .filter(|b| b.area.height == d.area.height)
        .map(|b| row(b.area.y))
        .max()
        .unwrap_or(0);
    peers >= 8
        && d.area.height + tolerance >= least
        && d.area.width + tolerance >= least.min(scene.screen.width / peers / 2)
}

/// On touch screens, what a finger presses at least about 7 mm.
pub fn touch_targets(scene: &Scene<'_>) -> Vec<Issue> {
    let finger = scene.ui.finger;
    if finger == 0 {
        return Vec::new();
    }
    let least = finger.saturating_sub(scene.ui.tolerance());
    let rows: Vec<Rect> = scene.parts(&[Role::Row]).map(|r| r.area).collect();
    let mut small: Vec<(&Described, u16)> = Vec::new();
    for d in scene.parts(&[Role::Button, Role::Row, Role::Switch, Role::Slider]) {
        // A switch in a row: the row is what is tapped.
        if d.role == Role::Switch && rows.iter().any(|r| inside(d.area, *r)) {
            continue;
        }
        let size = if d.role == Role::Row {
            d.area.height
        } else {
            d.area.width.min(d.area.height)
        };
        // A keyboard's key: smaller, as many as a row holds.
        if size < least && !keyboard_key(scene, d) {
            small.push((d, size));
        }
    }
    if small.is_empty() {
        return Vec::new();
    }
    // One finding a screen, naming a few: a keyboard has many keys.
    let mut names: Vec<String> = small
        .iter()
        .take(4)
        .map(|(d, size)| {
            let across = if d.area.height == *size {
                "high"
            } else {
                "wide"
            };
            let name = if d.label.trim().is_empty() {
                role_name(d.role).to_string()
            } else {
                quote(&d.label)
            };
            format!("{name} ({:.1} mm {across})", scene.ui.mm(*size))
        })
        .collect();
    if small.len() > 4 {
        names.push(format!("{} more", small.len() - 4));
    }
    let area = small
        .iter()
        .fold(Rect::default(), |a, (d, _)| a.union(&d.area));
    vec![Issue::warning(
        TOUCH_TARGET,
        area,
        format!(
            "too small for a finger, which needs about 7 mm (UiStyle::finger): {}",
            names.join(", ")
        ),
    )]
}

// ---- Room around buttons ------------------------------------------------

/// Buttons keep their room as they grow: the margin of the screen's
/// sides from every edge of the screen (a bar of buttons along its top
/// or bottom keeps it too, `ui::edge_bar_height`), and half a gap (the
/// room a button has in its row) from the edges of the row they are in
/// and from each other. A button grown into the room around it (a bar
/// that did not grow with it, `UiStyle::touch_row`) is found. A keyboard's
/// keys keep a quarter of the margin (at least 4 px) from the edges
/// instead: they take the width (the keyboard's `side_margin`).
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
pub fn button_room(scene: &Scene<'_>) -> Vec<Issue> {
    let ui = scene.ui;
    let tolerance = i32::from(ui.tolerance());
    let half = i32::from(ui.gap / 2);
    let margin = i32::from(ui.margin);
    let screen = scene.screen;
    let buttons: Vec<&Described> = scene
        .parts(&[Role::Button])
        .filter(|b| b.area.width > 0 && b.area.height > 0)
        .collect();
    let rows: Vec<Rect> = scene.parts(&[Role::Row]).map(|r| r.area).collect();
    // A header's note or arrows follow the header's own layout.
    let headers: Vec<Rect> = scene.parts(&[Role::Heading]).map(|h| h.area).collect();
    // A button across the whole width (a filter row over a list) is a
    // row: its words keep the margin, the row reaches the edges.
    let buttons: Vec<&Described> = buttons
        .into_iter()
        .filter(|b| !headers.iter().any(|h| inside(b.area, *h)))
        .filter(|b| (b.area.x, b.area.width) != (scene.screen.x, scene.screen.width))
        .collect();
    let mut found: Vec<(String, Rect, String)> = Vec::new();
    let (left, right) = (i32::from(screen.x), screen.right() as i32);
    let (top, bottom) = (i32::from(screen.y), screen.bottom() as i32);
    for b in &buttons {
        let a = b.area;
        let (x0, x1) = (i32::from(a.x), a.right() as i32);
        let (y0, y1) = (i32::from(a.y), a.bottom() as i32);
        let margin = if keyboard_key(scene, b) {
            (margin / 4).max(4)
        } else {
            margin
        };
        let side = (x0 - left).min(right - x1);
        if side < margin - tolerance {
            found.push((
                named(b),
                a,
                format!("{side} px from the side (margin {margin} px)"),
            ));
        }
        let edge = (y0 - top).min(bottom - y1);
        if edge < margin - tolerance {
            found.push((
                named(b),
                a,
                format!("{edge} px from the screen's top or bottom (margin {margin} px)"),
            ));
        }
        // Its row: room above and below it.
        if let Some(row) = rows.iter().find(|r| inside(a, **r) && **r != a) {
            let room = (y0 - i32::from(row.y)).min(row.bottom() as i32 - y1);
            if room < half - tolerance {
                found.push((named(b), a, format!("{room} px from its row's edge")));
            }
        }
    }
    // Tiles of a grid, each cell the target, lie edge to edge: a button
    // is one where buttons of its size lie on its grid's lines (its x
    // and y a whole number of cells from theirs), in two columns and two
    // rows or more. A last row of one tile is the grid's too.
    let tile = |d: &Described| {
        let (w, h) = (i32::from(d.area.width), i32::from(d.area.height));
        let mut columns: Vec<u16> = Vec::new();
        let mut lines: Vec<u16> = Vec::new();
        for o in &buttons {
            let (dx, dy) = (
                i32::from(o.area.x) - i32::from(d.area.x),
                i32::from(o.area.y) - i32::from(d.area.y),
            );
            if o.area.width == d.area.width
                && o.area.height == d.area.height
                && dx % w == 0
                && dy % h == 0
            {
                columns.push(o.area.x);
                lines.push(o.area.y);
            }
        }
        for v in [&mut columns, &mut lines] {
            v.sort_unstable();
            v.dedup();
        }
        columns.len() >= 2 && lines.len() >= 2
    };
    // Neighbours side by side, and above each other.
    for (i, a) in buttons.iter().enumerate() {
        for b in &buttons[i + 1..] {
            let (p, q) = (a.area, b.area);
            let across = i32::from(p.y) < q.bottom() as i32 && i32::from(q.y) < p.bottom() as i32;
            let down = i32::from(p.x) < q.right() as i32 && i32::from(q.x) < p.right() as i32;
            let gap = if across {
                (i32::from(q.x) - p.right() as i32).max(i32::from(p.x) - q.right() as i32)
            } else if down {
                (i32::from(q.y) - p.bottom() as i32).max(i32::from(p.y) - q.bottom() as i32)
            } else {
                continue;
            };
            // Tiles of a grid lie edge to edge, a keyboard's keys a seam
            // apart; overlapping ones are the overlap check's.
            let tiles = gap == 0 && tile(a) && tile(b);
            let keys = gap > 0 && keyboard_key(scene, a) && keyboard_key(scene, b);
            if !tiles && !keys && (0..half - tolerance).contains(&gap) {
                found.push((
                    format!("{} and {}", named(a), named(b)),
                    p.union(&q),
                    format!("{gap} px apart"),
                ));
            }
        }
    }
    if found.is_empty() {
        return Vec::new();
    }
    let area = found
        .iter()
        .fold(Rect::default(), |s, (_, a, _)| s.union(a));
    let mut names: Vec<String> = found
        .iter()
        .take(4)
        .map(|(name, _, what)| format!("{name} {what}"))
        .collect();
    if found.len() > 4 {
        names.push(format!("{} more", found.len() - 4));
    }
    vec![Issue::warning(
        BUTTON_ROOM,
        area,
        format!(
            "buttons without their room (the margin from every edge of the screen, half a gap, {half} px, in rows and between them; rows and bars grow with their buttons, UiStyle::touch_row, ui::edge_bar_height): {}",
            names.join(", ")
        ),
    )]
}

// ---- Focus --------------------------------------------------------------

/// Whether moving the focus changed the picture where the focus is now:
/// the focused part looks otherwise than before.
pub fn focus_shows(before: &Image, after: &Image, focused: Rect) -> bool {
    let area = focused.grow(4);
    let a = before.crop(area);
    let b = after.crop(area);
    let changed = a.differing(&b, 60);
    let size = (a.width * a.height) as usize;
    changed >= 12.max(size / 50)
}

// ---- Contrast -----------------------------------------------------------

/// Relative luminance of a gray, as WCAG computes it.
fn luminance(luma: u8) -> f32 {
    let c = f32::from(luma) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The contrast ratio of two grays, 1 to 21.
pub fn contrast_ratio(a: u8, b: u8) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The least contrast of text against its background on gray screens:
/// the 3:1 that WCAG asks of large text, as e-ink panels show less
/// contrast than their numbers.
pub const LEAST_CONTRAST: f32 = 3.0;

/// What counts as ink when contrast is measured: faint gray too.
const FAINT: u8 = 16;

/// Text and controls that black-and-white screens do not show well (gray
/// text rounded away to white, only dithered gray), and text of too
/// little contrast on gray screens.
pub fn contrast(scene: &Scene<'_>) -> Vec<Issue> {
    let mut issues = Vec::new();
    for t in scene.parts(&[Role::Text]) {
        if !says_something(&t.label) {
            continue;
        }
        let Some(area) = scene.visible(t).and_then(|v| t.area.intersection(&v)) else {
            continue;
        };
        let background = scene.background(area);
        match scene.panel {
            Panel::Mono => {
                let mut ink = text_ink(scene, t, area, INK);
                // Within the text's lines: rules and dotted separators
                // drawn through its box are not the text.
                let lines = ink.lines(2);
                if !lines.is_empty() {
                    ink.keep_rows(&lines);
                }
                let count = ink.count();
                if count == 0 {
                    issues.push(Issue::warning(
                        CONTRAST,
                        area,
                        format!(
                            "{} does not show on black-and-white screens: its gray is lighter than half and becomes white; draw it darker",
                            quote(&t.label)
                        ),
                    ));
                } else if count >= 20 && ink.isolated() * 2 > count {
                    issues.push(Issue::warning(
                        CONTRAST,
                        area,
                        format!(
                            "{} shows as a dot pattern on black-and-white screens (a dithered gray): draw text black",
                            quote(&t.label)
                        ),
                    ));
                }
            }
            Panel::Gray => {
                // Faint text too: the run of the text is what is measured.
                if on_focus(scene, t) {
                    continue;
                }
                let Some((run, _)) = text_run(scene, t, FAINT) else {
                    continue;
                };
                // The text's own colour: its darkest (or, light on dark,
                // lightest) pixels, past the antialiased edges.
                let mut extreme = background;
                for y in u32::from(run.y)..run.bottom() {
                    for x in u32::from(run.x)..run.right() {
                        let l = scene.luma(x, y);
                        if l.abs_diff(background) > extreme.abs_diff(background) {
                            extreme = l;
                        }
                    }
                }
                let ratio = contrast_ratio(extreme, background);
                if ratio < LEAST_CONTRAST {
                    issues.push(Issue::warning(
                        CONTRAST,
                        run,
                        format!(
                            "{} has a contrast of {ratio:.1}:1 against its background, less than {LEAST_CONTRAST}:1: draw it darker",
                            quote(&t.label)
                        ),
                    ));
                }
            }
        }
    }
    if scene.panel == Panel::Mono {
        for d in scene.parts(CONTROLS) {
            let Some(area) = scene.visible(d).and_then(|v| d.area.intersection(&v)) else {
                continue;
            };
            let ink = scene.ink_in(area);
            if ink.count() >= 20 && !ink.has_solid() {
                issues.push(Issue::warning(
                    CONTRAST,
                    area,
                    format!(
                        "{} is drawn only in dithered gray, a faint dot pattern on black-and-white screens: give it solid black lines",
                        named(d)
                    ),
                ));
            }
        }
    }
    issues
}

#[cfg(test)]
mod tests;
