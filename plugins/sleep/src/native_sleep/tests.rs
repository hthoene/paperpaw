use super::*;
use crate::fail_alloc::sweep;
use alloc::string::ToString;
use paperpaw_sdk::document::ImageRef;

const NONE: Facts = Facts {
    off: false,
    over_page: false,
    other_screen: false,
    picture: false,
    cover: false,
    widgets_empty: false,
};

fn with(change: impl FnOnce(&mut Facts)) -> Facts {
    let mut facts = NONE;
    change(&mut facts);
    facts
}

#[test]
fn the_cat_shows_where_nothing_else_does() {
    assert_eq!(decide(CAT, with(|f| f.cover = true)), Content::Cat);
    assert_eq!(decide(COVER, NONE), Content::Cat);
    assert_eq!(decide(COVER, with(|f| f.cover = true)), Content::Cover);
    assert_eq!(decide(PICTURE, with(|f| f.cover = true)), Content::Cat);
    assert_eq!(
        decide(PICTURE, with(|f| f.picture = true)),
        Content::Picture
    );
    // A mode no plugin offers any more shows the cover, else the cat.
    assert_eq!(
        decide("gone.mode", with(|f| f.cover = true)),
        Content::Cover
    );
    assert_eq!(decide("gone.mode", NONE), Content::Cat);
    assert_eq!(decide("", NONE), Content::Cat);
}

#[test]
fn another_plugins_screen_takes_the_place_of_a_mode() {
    assert_eq!(
        decide("my.mode", with(|f| f.other_screen = true)),
        Content::Other
    );
    // Not for a mode of the plugin's own.
    assert_eq!(decide(CAT, with(|f| f.other_screen = true)), Content::Cat);
}

/// A stored `sleep.show` that no plugin offers (another plugin's mode after it
/// was removed, a word from a newer version, an empty or a very long text) falls
/// back as interface 6 did: the cover where there is one, else the cat. Never
/// another plugin's screen (there is none), the overlay, a blank page or a
/// panic, whatever the other facts say.
#[test]
fn a_stored_mode_no_plugin_offers_falls_back_to_the_cover_or_the_cat() {
    let long = "x".repeat(300);
    let modes = [
        "reading-goal.sleep",
        "gone.mode",
        "",
        " ",
        "Cat",
        "ünï.cödé",
        long.as_str(),
    ];
    for mode in modes {
        for bits in 0..64u32 {
            let facts = Facts {
                off: bits & 1 != 0,
                over_page: bits & 2 != 0,
                other_screen: false,
                picture: bits & 8 != 0,
                cover: bits & 16 != 0,
                widgets_empty: bits & 32 != 0,
            };
            let want = if facts.cover {
                Content::Cover
            } else {
                Content::Cat
            };
            assert_eq!(decide(mode, facts), want, "{mode:?} {facts:?}");
            // The mode is kept as it is: the stored word is not rewritten.
            assert_eq!(effective(mode, facts), mode);
        }
    }
}

#[test]
fn widgets_message_and_page() {
    assert_eq!(decide(WIDGETS, NONE), Content::Widgets);
    assert_eq!(
        decide(WIDGETS, with(|f| f.widgets_empty = true)),
        Content::Message
    );
    assert_eq!(decide(MESSAGE, NONE), Content::Message);
    assert_eq!(decide(PAGE, NONE), Content::Page);
}

#[test]
fn off_the_page_does_not_stay() {
    let off = with(|f| f.off = true);
    assert_eq!(decide(PAGE, off), Content::Message);
    assert_eq!(effective(PAGE, off), MESSAGE);
    assert_eq!(
        effective(
            OVERLAY,
            with(|f| {
                f.off = true;
                f.over_page = true
            })
        ),
        MESSAGE
    );
    assert_eq!(decide(CAT, off), Content::Cat);
}

#[test]
fn an_overlay_with_nothing_to_show_is_the_cover() {
    let over = with(|f| f.over_page = true);
    assert_eq!(decide(OVERLAY, over), Content::Overlay);
    assert_eq!(effective(OVERLAY, NONE), COVER);
    assert_eq!(decide(OVERLAY, NONE), Content::Cat);
    assert_eq!(decide(OVERLAY, with(|f| f.cover = true)), Content::Cover);
}

#[test]
fn pictures_are_taken_one_after_the_other() {
    for (name, dir, yes) in [
        ("a.jpg", false, true),
        ("A.JPEG", false, true),
        ("b.Png", false, true),
        ("c.gif", false, true),
        (".hidden.jpg", false, false),
        ("d.txt", false, false),
        ("e.jpg", true, false),
        ("jpg", false, false),
        ("é.jpg", false, true),
        ("日本語", false, false),
    ] {
        assert_eq!(is_picture(name, dir), yes, "{name}");
    }
    let mut pictures = Pictures::default();
    for name in ["b.png", "a.jpg", "c.txt", "d.gif"] {
        pictures.take(name, false).unwrap();
    }
    pictures.take("sub.jpg", true).unwrap();
    assert_eq!(pictures.next(0), Some(("a.jpg", 1)));
    assert_eq!(pictures.next(1), Some(("b.png", 2)));
    assert_eq!(pictures.next(2), Some(("d.gif", 3)));
    assert_eq!(pictures.next(3), Some(("a.jpg", 4)));
    assert_eq!(pictures.next(usize::MAX), Some(("a.jpg", 0)));
    assert_eq!(Pictures::default().next(5), None);
    assert_eq!(shown_before(b"7\n"), 7);
    assert_eq!(shown_before(b"x"), 0);
    assert_eq!(shown_before(&[0xff, 0xfe]), 0);
    assert_eq!(shown_before(b""), 0);
}

fn screen() -> Screen {
    Screen {
        width: 480,
        height: 800,
        margin: 20,
        status: 30,
    }
}

#[test]
fn a_picture_goes_where_the_position_says() {
    let s = screen();
    let at = |p: &str| placed(s, 100, 100, false, p);
    assert_eq!(at(POSITIONS[0]), (190, 350, 100, 100));
    assert_eq!(at(POSITIONS[1]), (190, 50, 100, 100));
    assert_eq!(at(POSITIONS[2]), (190, 650, 100, 100));
    assert_eq!(at(POSITIONS[3]), (20, 50, 100, 100));
    assert_eq!(at(POSITIONS[4]), (360, 50, 100, 100));
    assert_eq!(at(POSITIONS[5]), (20, 650, 100, 100));
    assert_eq!(at(POSITIONS[6]), (360, 650, 100, 100));
    // An unknown position is the default, below on the right.
    assert_eq!(at("nowhere"), at(POSITIONS[6]));
    // At 100 % the whole screen to the edges; too large is held to the room.
    assert_eq!(placed(s, 480, 800, true, POSITIONS[0]), (0, 0, 480, 800));
    assert_eq!(
        placed(s, 9999, 9999, false, POSITIONS[0]),
        (20, 50, 440, 700)
    );
}

#[test]
fn nothing_overflows_or_panics() {
    for width in [0, 1, 7, 480, u16::MAX] {
        for margin in [0, 20, u16::MAX] {
            let s = Screen {
                width,
                height: width / 2,
                margin,
                status: margin,
            };
            for p in POSITIONS {
                let (x, y, w, h) = placed(s, u16::MAX, u16::MAX, false, p);
                assert!(
                    u32::from(x) + u32::from(w)
                        <= u32::from(width).max(u32::from(margin)) + u32::from(w)
                );
                let _ = (y, h);
                let _ = placed(s, 0, 0, true, p);
            }
            let _ = box_side(s, 100);
            let _ = box_size(s, 100);
        }
    }
    assert_eq!(percent(-5), 10);
    assert_eq!(percent(1000), 100);
    assert_eq!(opacity(0), 255);
    assert_eq!(opacity(100), 0);
    assert_eq!(opacity(50), 127);
    assert_eq!(opacity(-5), 255);
    assert_eq!(opacity(500), 0);
}

#[test]
fn boxes_and_fitting_follow_the_sizes() {
    let s = screen();
    assert_eq!(box_side(s, 50), 240);
    assert_eq!(box_size(s, 50), (240, 400));
    assert_eq!(box_size(s, 100), (480, 800));
    for (iw, ih) in [
        (0, 10),
        (10, 0),
        (1, 1),
        (400, 300),
        (300, 400),
        (u16::MAX, 1),
        (1, u16::MAX),
    ] {
        for (bw, bh) in [(0, 0), (100, 100), (480, 800), (u16::MAX, u16::MAX)] {
            let image = ImageRef {
                path: "x".to_string(),
                width: u32::from(iw),
                height: u32::from(ih),
            };
            assert_eq!(
                fit((iw, ih), bw, bh),
                paperpaw_sdk::ui::fit_image(&image, bw, bh),
                "{iw}x{ih} in {bw}x{bh}"
            );
        }
    }
}

#[test]
fn refused_allocations_end_in_no_memory_and_recover() {
    sweep(|| {
        let mut pictures = Pictures::default();
        for name in ["b.png", "a.jpg", "c.gif", "d.jpeg", "e.png"] {
            pictures.take(name, false)?;
        }
        Ok(pictures.next(2).map(|(n, k)| n.len() + k))
    });
}

#[test]
fn a_folder_of_thousands_keeps_the_first_by_name() {
    use alloc::format;
    // Listed in an order that is not the name's: the same first pictures.
    let total = MAX_PICTURES + 300;
    let mut pictures = Pictures::default();
    for i in 0..total {
        let n = (i * 7919) % total;
        pictures.take(&format!("p{n:05}.jpg"), false).unwrap();
    }
    assert_eq!(pictures.names.len(), MAX_PICTURES);
    assert_eq!(pictures.next(0), Some(("p00000.jpg", 1)));
    assert_eq!(
        pictures.next(MAX_PICTURES - 1),
        Some((
            format!("p{:05}.jpg", MAX_PICTURES - 1).as_str(),
            MAX_PICTURES
        ))
    );
    // Then round again: only the kept ones are shown.
    assert_eq!(
        pictures.next(MAX_PICTURES),
        Some(("p00000.jpg", MAX_PICTURES + 1))
    );
    // The names' bytes are bounded as well, and a name that cannot fit
    // is left out.
    let mut pictures = Pictures::default();
    let long = |c: char| format!("{}.jpg", c.to_string().repeat(250));
    for c in ('a'..='z').chain('A'..='Z') {
        pictures.take(&long(c), false).unwrap();
    }
    assert!(pictures.bytes <= MAX_NAME_BYTES);
    assert!(pictures.names.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(pictures.bytes, pictures.names.iter().map(String::len).sum());
    pictures.take(&"z".repeat(MAX_NAME_BYTES), false).unwrap();
    let kept = pictures.names.len();
    pictures
        .take(&format!("{}.png", "x".repeat(MAX_NAME_BYTES)), false)
        .unwrap();
    assert_eq!(pictures.names.len(), kept);
}

const GOAL: &str = "reading-goal.sleep";

#[test]
fn the_slot_is_bound_to_a_foreign_mode_that_is_a_candidate() {
    let offered = [OWN_SCREEN, GOAL, "other.sleep"];
    assert_eq!(binding(GOAL, &offered), Some(GOAL));
    assert_eq!(binding("other.sleep", &offered), Some("other.sleep"));
    // Also from owned strings, and with the candidate alone.
    let owned = [GOAL.to_string()];
    assert_eq!(binding(GOAL, &owned), Some(GOAL));
}

#[test]
fn the_slot_returns_to_the_default_for_the_plugins_own_modes_and_an_empty_value() {
    let offered = [OWN_SCREEN, GOAL];
    for mode in BUILT_IN {
        assert_eq!(binding(mode, &offered), None, "{mode}");
        // Even if a plugin offered a screen of that very name.
        assert_eq!(binding(mode, &[mode]), None, "{mode}");
    }
    assert_eq!(binding("", &offered), None);
    assert_eq!(binding("", &[""]), None);
}

#[test]
fn a_stale_foreign_value_binds_nothing() {
    // The plugin that offered the mode is gone, or never was.
    assert_eq!(binding(GOAL, &[OWN_SCREEN]), None);
    assert_eq!(binding(GOAL, &["other.sleep"]), None);
    assert_eq!(binding(GOAL, &[] as &[&str]), None);
    // A prefix or a longer word of a candidate is not the candidate.
    assert_eq!(binding("reading-goal", &[GOAL]), None);
    assert_eq!(binding("reading-goal.sleep2", &[GOAL]), None);
    assert_eq!(binding("Reading-Goal.Sleep", &[GOAL]), None);
}

#[test]
fn the_plugins_own_screen_among_the_candidates_is_never_a_foreign_mode() {
    let offered = [OWN_SCREEN, GOAL];
    // Its id as the stored value is the default, not a pinned binding.
    assert_eq!(binding(OWN_SCREEN, &offered), None);
    assert_eq!(binding(GOAL, &offered), Some(GOAL));
    assert_eq!(binding(GOAL, &[OWN_SCREEN]), None);
}

#[test]
fn odd_and_long_values_bind_nothing_and_do_not_panic() {
    let long = "x".repeat(4096);
    let offered = [OWN_SCREEN, GOAL];
    for value in [
        " ",
        " reading-goal.sleep",
        "reading-goal.sleep ",
        "reading-goal.sleep\n",
        "\0",
        "é.sleep",
        "😀",
        long.as_str(),
    ] {
        assert_eq!(binding(value, &offered), None, "{value:?}");
    }
    // A long candidate matches itself only; the caller's refusal of a screen
    // id too long for the command is a refusal like any other.
    let list = [long.clone()];
    assert_eq!(binding(&long, &list), Some(long.as_str()));
    assert_eq!(binding(&long[1..], &list), None);
    assert!(is_foreign(&long));
    assert!(!is_foreign(""));
    assert!(!is_foreign(CAT));
}

/// The slot as the host keeps it: what `plan` says is applied to it.
fn apply(slot: &mut Option<String>, plan: Bind<'_>) {
    match plan {
        Bind::Screen(screen) => *slot = Some(screen.to_string()),
        Bind::Default => *slot = None,
        _ => {}
    }
}

const BUILT: [&str; 8] = [CAT, COVER, PICTURE, OVERLAY, WIDGETS, MESSAGE, PAGE, ""];

/// A reader bound `core.sleep` by hand in the plugin manager, to a screen that
/// is not one of `sleep.show`'s foreign choices: changing `sleep.show` to an
/// own mode leaves that binding alone. (A start asks nothing at all: the plugin
/// does not subscribe to it, so a binding survives a restart.)
#[test]
fn a_binding_the_reader_made_survives() {
    let by_hand = Some("other.tool-screen".to_string());
    for value in BUILT {
        let mut slot = by_hand.clone();
        apply(
            &mut slot,
            plan(value, &[] as &[&str], by_hand.as_deref(), &[GOAL]),
        );
        assert_eq!(slot, by_hand, "{value:?}");
    }
    // Nothing known of the slot: nothing to unbind.
    assert_eq!(plan(CAT, &[] as &[&str], None, &[GOAL]), Bind::Leave);
    // Stale values and the plugin's own screen's id leave the slot alone.
    for value in ["gone.mode", OWN_SCREEN] {
        assert_eq!(
            plan(value, &["x"], Some(GOAL), &[GOAL]),
            Bind::Leave,
            "{value}"
        );
    }
}

/// Changing to an own mode puts the slot back to its default only where it is
/// bound to a sleep mode (one of `sleep.show`'s foreign choices); an offered
/// foreign mode binds.
#[test]
fn a_change_unbinds_only_a_sleep_mode_and_binds_an_offered_one() {
    for value in BUILT {
        let mut slot = Some(GOAL.to_string());
        let decided = plan(value, &[GOAL], Some(GOAL), &[GOAL]);
        apply(&mut slot, decided);
        assert_eq!(slot, None, "{value:?}");
    }
    // The plugin's own screen's id is no sleep mode to unbind.
    assert_eq!(
        plan(CAT, &[] as &[&str], Some(OWN_SCREEN), &[OWN_SCREEN]),
        Bind::Leave
    );
    let mut slot = None;
    apply(&mut slot, plan(GOAL, &[GOAL], None, &[GOAL]));
    assert_eq!(slot.as_deref(), Some(GOAL));
}

/// A stored value of Beta 3 (`reading-goal.sleep`) binds the slot at the first
/// start and when the plugins change, where the reader has not bound it.
#[test]
fn a_beta_3_stored_value_binds_at_first_start() {
    assert_eq!(plan_stored(GOAL, &[GOAL], false), Bind::Screen(GOAL));
    // Its plugin is gone, or it is not offered for the slot: nothing to bind.
    assert_eq!(plan_stored(GOAL, &["other.sleep"], false), Bind::Leave);
    assert_eq!(plan_stored(GOAL, &[] as &[&str], false), Bind::Leave);
    assert_eq!(plan_stored(OWN_SCREEN, &[OWN_SCREEN], false), Bind::Leave);
    // Never an unbinding, whatever is stored.
    for value in BUILT {
        assert_eq!(plan_stored(value, &[GOAL], false), Bind::Leave, "{value:?}");
    }
}

/// The reader's binding (bit 0) survives the start, event 70 (the same call)
/// and a reset (the empty key runs the change's reconcile: a binding that is
/// not a sleep mode stays, whatever `sleep.show` holds).
#[test]
fn a_binding_of_a_non_sleep_screen_survives_start_event_70_and_a_reset() {
    for value in [GOAL, CAT, "gone.mode", ""] {
        // Start and event 70: the reader's binding is never touched.
        assert_eq!(plan_stored(value, &[GOAL], true), Bind::Leave, "{value:?}");
        // A reset: the slot is bound to a screen no choice of `sleep.show`
        // names, so a foreign value binds only if offered, and nothing unbinds.
        let after = plan(value, &[] as &[&str], Some("other.tool-screen"), &[GOAL]);
        assert_eq!(after, Bind::Leave, "{value:?}");
    }
}

/// What version 1 wrote stays readable (see `docs/data-files.md`).
#[test]
fn the_next_picture_of_version_1_is_read() {
    assert_eq!(
        shown_before(include_bytes!("../../testdata/next.txt.v1")),
        7
    );
}
