//! The decisions against the interface-6 functions they port (called with the
//! same inputs), the cases of the old tests, and refusals of allocation.
use super::*;
use crate::fail_alloc::sweep;
use alloc::format;
use alloc::string::ToString;
use core::fmt::Write;
use paperpaw_sdk::{MemoryStorage, Storage};

const FITS: [Fitting; 5] = [
    Fitting::Fill,
    Fitting::Whole,
    Fitting::Stretch,
    Fitting::Original,
    Fitting::Tile,
];

fn recipe(fitting: Fitting, zoom: u32, across: u8, down: u8, size: (u16, u16)) -> Recipe {
    Recipe {
        path: "pictures/a.jpg".to_string(),
        bytes: 1234,
        modified: -5,
        fitting,
        across,
        down,
        zoom,
        lighten: 20,
        brightness: -1,
        contrast: 1,
        dotted: true,
        grays: false,
        width: size.0,
        height: size.1,
    }
}

#[test]
fn choices_fall_back_when_they_are_gone() {
    assert_eq!(Fitting::of("gone"), Fitting::Fill);
    assert_eq!(Render::of("gone"), Render::Automatic);
    assert_eq!(Visible::of("gone"), Visible::On);
    assert_eq!(Look::of("gone"), Look::Normal);
}

#[test]
fn the_recipe_is_what_the_settings_ask() {
    let asked = Asked {
        path: "pictures/a.jpg",
        bytes: 9,
        modified: 7,
        fit: settings::FITS[4],
        anchor: settings::ANCHORS[2],
        zoom: 999,
        lighten: -4,
        brightness: settings::BRIGHTNESSES[0],
        contrast: settings::CONTRASTS[2],
        render: settings::RENDERS[2],
    };
    let r = Recipe::new(&asked, 480, 800).unwrap().unwrap();
    assert_eq!(r.path, "pictures/a.jpg");
    assert_eq!((r.bytes, r.modified), (9, 7));
    assert_eq!(r.fitting, Fitting::Tile);
    // The top right.
    assert_eq!((r.across, r.down), (2, 0));
    assert_eq!((r.zoom, r.lighten), (300, 0));
    assert_eq!((r.brightness, r.contrast), (-2, 1));
    assert!(r.dotted);
    assert_eq!((r.width, r.height), (480, 800));
    // No picture chosen; unknown choices fall to the middle and normal.
    assert_eq!(
        Recipe::new(&Asked { path: "", ..asked }, 480, 800).unwrap(),
        None
    );
    let odd = Asked {
        anchor: "",
        brightness: "",
        contrast: "",
        render: "",
        zoom: 50,
        lighten: 90,
        ..asked
    };
    let r = Recipe::new(&odd, 480, 800).unwrap().unwrap();
    assert_eq!((r.across, r.down), (1, 1));
    assert_eq!(
        (r.brightness, r.contrast, r.zoom, r.lighten),
        (0, 0, 100, 60)
    );
    assert!(!r.dotted);
}

#[test]
fn the_note_does_not_grow_with_the_path() {
    let mut r = recipe(Fitting::Fill, 100, 0, 0, (480, 800));
    let before = r.key().unwrap();
    r.path = "x".repeat(300);
    assert_eq!(r.key().unwrap().len(), before.len());
}

#[test]
fn the_old_cases_of_the_plan_hold() {
    let screen = (480, 800);
    // Fill covers the screen and cuts the sides of a wide picture.
    let p = plan(&recipe(Fitting::Fill, 100, 1, 1, screen), (4000, 3000));
    assert_eq!((p.height, p.top), (800, 0));
    assert!(p.width > 480 && p.left < 0);
    // Whole leaves white above and below.
    let p = plan(&recipe(Fitting::Whole, 100, 1, 1, screen), (4000, 3000));
    assert_eq!(p.width, 480);
    assert!(p.height < 800 && p.top > 0);
    // Stretch is the screen, whatever the zoom.
    let p = plan(&recipe(Fitting::Stretch, 300, 1, 1, screen), (4000, 3000));
    assert_eq!((p.width, p.height, p.left, p.top), (480, 800, 0, 0));
    // A tile is at most half the screen.
    let p = plan(&recipe(Fitting::Tile, 100, 0, 0, screen), (4000, 3000));
    assert!(p.width <= 240 && p.height <= 400);
    // The size is kept in bounds.
    let p = plan(&recipe(Fitting::Original, 300, 1, 1, screen), (9999, 9999));
    assert_eq!((p.width, p.height), (LARGEST, LARGEST));
}

/// A JPEG's head: an EXIF block of `meta` bytes (with the orientation where
/// `orientation` is not 0), then a frame of 4000 by 3000.
fn head(meta: usize, orientation: u16, little: bool) -> Vec<u8> {
    let mut exif = Vec::new();
    exif.extend(b"Exif\0\0");
    exif.extend(if little { *b"II" } else { *b"MM" });
    let word = |v: u16| {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let long = |v: u32| {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    exif.extend(word(42));
    exif.extend(long(8));
    exif.extend(word(if orientation == 0 { 1 } else { 2 }));
    exif.extend(word(0x010f));
    exif.extend(word(2));
    exif.extend(long(1));
    exif.extend([0u8; 4]);
    if orientation != 0 {
        exif.extend(word(0x0112));
        exif.extend(word(3));
        exif.extend(long(1));
        exif.extend(word(orientation));
        exif.extend([0u8; 2]);
    }
    exif.resize(exif.len().max(meta), 0);
    let mut bytes = alloc::vec![0xff, 0xd8, 0xff, 0xe1];
    bytes.extend((exif.len() as u16 + 2).to_be_bytes());
    bytes.extend(exif);
    bytes.extend([0xff, 0xc0, 0, 17, 8, 0x0b, 0xb8, 0x0f, 0xa0, 3]);
    bytes.extend([0u8; 9]);
    bytes
}

impl ReadAt for &[u8] {
    fn read_at(&self, at: u64, out: &mut [u8]) -> Option<usize> {
        let from = usize::try_from(at).ok()?.min(self.len());
        let n = out.len().min(self.len() - from);
        out[..n].copy_from_slice(&self[from..from + n]);
        Some(n)
    }
}

#[test]
fn a_jpegs_size_and_turn_are_read() {
    let big = head(40_000, 6, false);
    assert_eq!(jpeg_size(&big.as_slice()), Some((4000, 3000)));
    assert_eq!(jpeg_orientation(&big.as_slice()), 6);
    assert_eq!(jpeg_orientation(&head(0, 9, true).as_slice()), 1);
    assert_eq!(jpeg_orientation(&b"junk".as_slice()), 1);
}

#[test]
fn the_folder_offers_its_pictures_by_name() {
    for (name, is_dir, offered) in [
        ("a.jpg", false, true),
        ("A.JPEG", false, true),
        ("b.PnG", false, true),
        ("c.gif", false, true),
        ("d.bmp", false, false),
        (".hidden.jpg", false, false),
        ("folder.jpg", true, false),
        ("jpg", false, false),
        ("", false, false),
    ] {
        assert_eq!(is_picture(name, is_dir), offered, "{name}");
    }
    let entries = [
        ("b.jpg", false),
        ("A.png", false),
        ("sub", true),
        ("notes.txt", false),
        ("a.gif", false),
        ("Ä.jpg", false),
    ];
    let found = pictures(entries.iter().copied()).unwrap();
    assert_eq!(found, ["a.gif", "A.png", "b.jpg", "Ä.jpg"]);
    // A long folder keeps the alphabetically first 300, whatever order the
    // card lists them in.
    let names: Vec<String> = (0..400).map(|n| format!("{:03}.jpg", 399 - n)).collect();
    let found = pictures(names.iter().map(|n| (n.as_str(), false))).unwrap();
    assert_eq!(found.len(), MOST);
    assert_eq!(found[0], "000.jpg");
    assert_eq!(found[MOST - 1], "299.jpg");
}

#[test]
fn the_bar_sits_where_the_settings_say() {
    let placing = Placing::of(
        settings::BAR_ATS[1],
        settings::BAR_SHAPES[1],
        settings::BAR_ALIGNS[0],
    );
    assert_eq!(
        placing,
        Placing {
            bottom: true,
            compact: true,
            align: 0
        }
    );
    // Unknown choices: top, full, right.
    assert_eq!(
        Placing::of("", "", ""),
        Placing {
            bottom: false,
            compact: false,
            align: 2
        }
    );
    let strip = Area {
        x: 0,
        y: 760,
        width: 480,
        height: 40,
    };
    let full = Placing {
        bottom: true,
        compact: false,
        align: 1,
    };
    let a = plate_area(full, strip, 10);
    assert_eq!((a.x, a.y, a.width, a.height), (10, 760, 460, 40));
    // A pill is 55 percent of it, at the left, the middle or the right.
    let pill = |align| {
        plate_area(
            Placing {
                compact: true,
                align,
                ..full
            },
            strip,
            10,
        )
    };
    assert_eq!((pill(0).x, pill(0).width), (10, 253));
    assert_eq!(pill(1).x, 10 + (460 - 253) / 2);
    assert_eq!(pill(2).x, 10 + 460 - 253);
    // A margin wider than the strip leaves nothing, and does not wrap.
    assert_eq!(plate_area(full, strip, 400).width, 0);
    let wide = Area {
        width: 3000,
        ..strip
    };
    assert_eq!(
        plate_area(
            Placing {
                compact: true,
                align: 2,
                ..full
            },
            wide,
            0
        )
        .width,
        1650
    );
}

#[test]
fn the_ink_reads_on_the_plate_and_on_the_picture() {
    assert_eq!(Look::Normal.plate(), Some((255, 0)));
    assert_eq!(Look::Inverted.plate(), Some((0, 255)));
    assert_eq!(Look::Light.plate(), Some((165, 0)));
    assert_eq!(Look::Dark.plate(), Some((85, 255)));
    assert_eq!(Look::Clear.plate(), None);
    assert_eq!(ink_over(Some(10)), 255);
    assert_eq!(ink_over(Some(200)), 0);
    assert_eq!(ink_over(None), 0);
}

#[test]
fn refused_allocations_are_errors_and_change_nothing() {
    let asked = Asked {
        path: "pictures/a.jpg",
        bytes: 1,
        modified: 2,
        fit: settings::FITS[1],
        anchor: settings::ANCHORS[0],
        zoom: 100,
        lighten: 0,
        brightness: "",
        contrast: "",
        render: "",
    };
    let entries = [("a.jpg", false), ("B.png", false), ("c.txt", false)];
    sweep(|| {
        let r = Recipe::new(&asked, 480, 800)?.ok_or(Error::NotFound)?;
        let key = r.key()?;
        assert!(!key.is_empty());
        let found = pictures(entries.iter().copied())?;
        assert_eq!(found.len(), 2);
        Ok(())
    });
}

#[test]
fn a_strip_that_does_not_fit_in_sixteen_bits_does_not_wrap() {
    let near = Area {
        x: 65_000,
        y: 0,
        width: 1_000,
        height: 40,
    };
    let compact = Placing {
        bottom: false,
        compact: true,
        align: 2,
    };
    for align in 0..=2 {
        let a = plate_area(Placing { align, ..compact }, near, 10);
        // Kept to what 16 bits hold, never wrapped to a small number.
        assert!(a.x >= 65_000, "{a:?}");
        assert_eq!(a.width, 539);
    }
    let full = plate_area(
        Placing {
            compact: false,
            ..compact
        },
        near,
        10,
    );
    assert_eq!((full.x, full.width), (65_010, 980));
    let past = Area { x: 65_535, ..near };
    assert_eq!(plate_area(compact, past, 10).x, u16::MAX);
    assert_eq!(
        plate_area(
            Placing {
                compact: false,
                ..compact
            },
            past,
            10
        )
        .x,
        u16::MAX
    );
    // The widest strip, the widest margin.
    let widest = Area {
        x: u16::MAX,
        y: u16::MAX,
        width: u16::MAX,
        height: u16::MAX,
    };
    let a = plate_area(compact, widest, u16::MAX);
    assert_eq!((a.x, a.width), (u16::MAX, 0));
}

fn card(names: &[&str]) -> MemoryStorage {
    names.iter().fold(MemoryStorage::new(), |card, name| {
        card.with(&format!("pictures/{name}"), &[1u8][..])
    })
}

fn listed(card: &MemoryStorage) -> Vec<String> {
    let mut entries = Vec::new();
    // A folder that is not there lists nothing, as on interface 6.
    let _ = card.list("pictures", &mut |entry| {
        entries.push((entry.name.to_string(), entry.is_dir));
    });
    pictures(entries.iter().map(|(name, dir)| (name.as_str(), *dir))).unwrap()
}

#[test]
fn a_folder_of_more_than_the_most_keeps_the_first_the_card_lists() {
    let names: Vec<String> = (0..400).map(|n| format!("p{n:03}.jpg")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    assert_eq!(listed(&card(&names)).len(), MOST);
}

#[test]
fn names_are_kept_within_their_budget() {
    // 300 names of 100 bytes: 24 KiB hold 245 of them, then the list ends.
    let names: Vec<String> = (0..300)
        .map(|n| format!("{n:03}{}.jpg", "x".repeat(93)))
        .collect();
    let found = pictures(names.iter().map(|n| (n.as_str(), false))).unwrap();
    assert_eq!(names[0].len(), 100);
    assert_eq!(found.len(), NAMES / 100);
    assert!(found.iter().map(String::len).sum::<usize>() <= NAMES);
    assert_eq!(found[0], names[0]);
    // Short names are not touched by it.
    let short: Vec<String> = (0..MOST).map(|n| format!("{n:03}.jpg")).collect();
    assert_eq!(
        pictures(short.iter().map(|n| (n.as_str(), false)))
            .unwrap()
            .len(),
        MOST
    );
}

#[test]
fn what_is_kept_does_not_depend_on_the_order_of_the_card() {
    // Names of every length, so that the budget cuts as well as the count.
    let names: Vec<String> = (0..700)
        .map(|n| {
            let pad = (n * 37) % 180;
            format!("{:03}{}.JPG", (n * 7919) % 700, "x".repeat(pad))
        })
        .collect();
    let list =
        |order: &[usize]| pictures(order.iter().map(|n| (names[*n].as_str(), false))).unwrap();
    let forward: Vec<usize> = (0..names.len()).collect();
    let backward: Vec<usize> = forward.iter().rev().copied().collect();
    let shuffled: Vec<usize> = (0..names.len()).map(|n| (n * 311) % names.len()).collect();
    let a = list(&forward);
    assert_eq!(a, list(&backward));
    assert_eq!(a, list(&shuffled));
    // It is the prefix of all the names, sorted, that fits.
    let mut all: Vec<&String> = names.iter().collect();
    all.sort_by(|x, y| x.to_lowercase().cmp(&y.to_lowercase()).then(x.cmp(y)));
    let mut bytes = 0;
    let mut expected = Vec::new();
    for name in all {
        bytes += name.len();
        if expected.len() == MOST || bytes > NAMES {
            break;
        }
        expected.push(name.clone());
    }
    assert_eq!(a, expected);
    assert!(a.len() < MOST, "the budget cut it: {}", a.len());
}

#[test]
fn a_folder_read_page_by_page_keeps_what_one_listing_keeps() {
    let names: Vec<String> = (0..700)
        .map(|n| alloc::format!("p{:03}.{}", (n * 7919) % 700, ["jpg", "PNG", "txt"][n % 3]))
        .collect();
    let all = pictures(names.iter().map(|name| (name.as_str(), false))).unwrap();
    for size in [1, 5, 32, 699] {
        let mut kept = Pictures::default();
        for page in names.chunks(size) {
            for name in page {
                kept.add(name, false).unwrap();
            }
        }
        assert_eq!(kept.finish(), all, "pages of {size}");
    }
    let mut kept = Pictures::default();
    kept.add("folder.jpg", true).unwrap();
    kept.add(".hidden.jpg", false).unwrap();
    kept.add("a.jpg", false).unwrap();
    kept.add("a.jpg", false).unwrap();
    assert_eq!(kept.finish(), ["a.jpg"]);
}

#[test]
fn refused_allocations_while_adding_page_by_page() {
    let names = ["b.jpg", "a.png", "c.gif", "a.png", "d.txt"];
    sweep(|| {
        let mut kept = Pictures::default();
        for name in names {
            kept.add(name, false)?;
        }
        assert_eq!(kept.finish(), ["a.png", "b.jpg", "c.gif"]);
        Ok(())
    });
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
    })
}

/// A long run of lines, kept as its count and a hash.
fn sum(out: &mut String, name: &str, body: &str) {
    writeln!(out, "{name} {} {:016x}", body.lines().count(), fnv(body)).unwrap();
}

/// What the native code makes of the inputs of `pinned.txt`, in its lines.
fn decided() -> String {
    let mut out = String::new();
    choices_and_notes(&mut out);
    plans(&mut out);
    crops(&mut out);
    tones(&mut out);
    jpegs(&mut out);
    folders_listed(&mut out);
    out
}

fn choices_and_notes(out: &mut String) {
    for value in settings::FITS.iter().chain(&["", "gone"]) {
        writeln!(out, "fit {value} {:?}", Fitting::of(value)).unwrap();
    }
    for value in settings::RENDERS.iter().chain(&["", "gone"]) {
        writeln!(out, "render {value} {:?}", Render::of(value)).unwrap();
    }
    for value in settings::BARS.iter().chain(&["", "gone"]) {
        writeln!(out, "bar {value} {:?}", Visible::of(value)).unwrap();
    }
    for value in settings::BAR_LOOKS.iter().chain(&["", "gone"]) {
        writeln!(out, "look {value} {:?}", Look::of(value)).unwrap();
    }
    for fitting in FITS {
        let r = recipe(fitting, 150, 2, 1, (480, 800));
        writeln!(out, "{fitting:?} {}", r.key().unwrap()).unwrap();
    }
}

fn plans(out: &mut String) {
    let mut b = String::new();
    let sizes = [(480, 800), (800, 480), (1404, 1872), (3, 3), (1, 1)];
    let sources = [
        (4000, 3000),
        (3000, 4000),
        (1, 1),
        (0, 0),
        (480, 800),
        (9999, 5),
    ];
    for fitting in FITS {
        for size in sizes {
            for source in sources {
                for zoom in [100, 133, 300] {
                    for (across, down) in [(0, 0), (1, 1), (2, 2), (0, 2)] {
                        let p = plan(&recipe(fitting, zoom, across, down, size), source);
                        writeln!(
                            b,
                            "{fitting:?} {size:?} {source:?} {zoom} {across} {down}: {} {} {} {}",
                            p.width, p.height, p.left, p.top
                        )
                        .unwrap();
                    }
                }
            }
        }
    }
    sum(out, "plan", &b);
}

fn crops(out: &mut String) {
    let mut b = String::new();
    for fitting in FITS {
        for orientation in 1..=8u16 {
            for source in [(4000, 3000), (3000, 4000), (10, 10), (1, 1)] {
                let r = recipe(fitting, 160, 2, 0, (480, 800));
                let p = plan(&r, source);
                let turn = Turn::of(orientation);
                let c = visible(&r, &p, source, turn);
                writeln!(
                    b,
                    "{fitting:?} {orientation} {source:?}: {} {} {} | {} {} {} {} 0",
                    turn.transpose, turn.flip_x, turn.flip_y, c.x, c.y, c.width, c.height
                )
                .unwrap();
                writeln!(b, "size {:?}", turn.size(source)).unwrap();
            }
        }
    }
    sum(out, "crop", &b);
}

fn tones(out: &mut String) {
    let mut b = String::new();
    for brightness in -2..=2 {
        for contrast in -1..=1 {
            let curve = tone(brightness, contrast);
            assert!(curve.windows(2).all(|w| w[0] <= w[1]));
            writeln!(b, "tone {brightness} {contrast} {curve:?}").unwrap();
        }
    }
    let normal = tone(0, 0);
    assert!((0..=255u8).all(|g| normal[usize::from(g)] == g));
    for value in 0..=255u8 {
        for (x, y) in [(0, 0), (1, 2), (7, 5)] {
            writeln!(b, "dot {value} {x} {y} {:?}", dotted(value, x, y)).unwrap();
        }
        for percent in [0, 1, 20, 60, 100, 500] {
            writeln!(b, "lighten {value} {percent} {:?}", lighten(value, percent)).unwrap();
        }
    }
    sum(out, "tone", &b);
}

fn jpegs(out: &mut String) {
    let mut files: Vec<Vec<u8>> = Vec::new();
    for little in [true, false] {
        for meta in [0, 10, 40_000] {
            for orientation in 0..=9 {
                files.push(head(meta, orientation, little));
            }
        }
    }
    files.push(b"not a picture".to_vec());
    files.push(alloc::vec![0xff, 0xd8, 0xff]);
    let mut short = head(0, 6, true);
    short.truncate(20);
    files.push(short);
    files.push(Vec::new());
    for bytes in &files {
        let file = bytes.as_slice();
        writeln!(out, "{:?} {}", jpeg_size(&file), jpeg_orientation(&file)).unwrap();
    }
}

fn folders_listed(out: &mut String) {
    let folders = [
        card(&[
            "b.jpg",
            "A.png",
            "notes.txt",
            "a.gif",
            "Ä.jpg",
            "ä.JPEG",
            "Zebra.PNG",
            ".hidden.jpg",
            "z.bmp",
            "ß.png",
            "Éa.jpg",
            "éa.jpg",
        ]),
        card(&[]),
        card(&["only.txt"]),
        card(
            &(0..400)
                .map(|n| format!("p{n:03}.jpg"))
                .collect::<Vec<_>>()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        ),
        card(&["ΑΣ.jpg", "ΑΣΑ.jpg", "ΑΑ.jpg"]),
    ];
    let mut b = String::new();
    for card in &folders {
        writeln!(b, "{:?}", listed(card)).unwrap();
    }
    sum(out, "folders", &b);
}

#[test]
fn the_values_are_those_interface_6_made() {
    let want = include_str!("pinned.txt");
    let got = decided();
    for (n, (w, g)) in want.lines().zip(got.lines()).enumerate() {
        assert_eq!(g, w, "line {}", n + 1);
    }
    assert_eq!(got.lines().count(), want.lines().count());
}
