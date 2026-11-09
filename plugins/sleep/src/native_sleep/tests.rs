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
