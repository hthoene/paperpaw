use alloc::string::ToString;

use super::*;

#[test]
fn code_fetched_for_an_update_has_a_folder_not_loaded() {
    assert_eq!(fetched_folder("stats"), "plugins/.stats.next");
    assert_eq!(fetched_plugin(".stats.next"), Some("stats"));
    for other in [
        "stats",
        ".stats.part",
        ".stats.old",
        "..next",
        ".Stats.next",
    ] {
        assert_eq!(fetched_plugin(other), None, "{other}");
    }
    let storage = crate::MemoryStorage::new();
    assert_eq!(fetched_code(&storage, "stats"), None);
    assert!(remember_fetched(&storage, "stats", &[1; 32]));
    assert_eq!(fetched_code(&storage, "stats"), Some([1; 32]));
    // Apart from the code installed.
    assert_eq!(installed_code(&storage, "stats"), None);
}

#[test]
fn what_cannot_be_read_is_not_written_over() {
    let kept = crate::MemoryStorage::new();
    assert!(remember_author(&kept, "clock", &[1; 32]));
    assert!(remember_code(&kept, "clock", &[2; 32]));
    let authors = crate::read_file(&kept, AUTHORS, 1024).unwrap();
    let codes = crate::read_file(&kept, INSTALLED, 1024).unwrap();
    // The card fails: nothing kept, the other plugins' left as they were.
    let failing = crate::memory_storage::Unreadable(kept);
    assert!(!remember_author(&failing, "weather", &[3; 32]));
    assert!(!remember_code(&failing, "weather", &[4; 32]));
    assert_eq!(
        crate::read_file(&failing.0, AUTHORS, 1024).unwrap(),
        authors
    );
    assert_eq!(
        crate::read_file(&failing.0, INSTALLED, 1024).unwrap(),
        codes
    );
    // A file too large to read is not written over either.
    let large = crate::MemoryStorage::new().with(INSTALLED, "x".repeat(70 * 1024));
    assert!(!remember_code(&large, "clock", &[2; 32]));
    assert_eq!(large.size(INSTALLED), Ok(70 * 1024));
}

/// A plugin file's header of an image of `len` bytes aligned to
/// `align`.
fn header_of(len: u32, align: u32) -> alloc::vec::Vec<u8> {
    let header = Header {
        chip: Chip::Esp32C3,
        fingerprint: 1,
        base: paperpaw_native::LINK_BASE,
        image_len: len,
        code_at: 96,
        entry: 0,
        api: 32,
        relocs: 0,
        align,
        compiler: 1,
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        needs_api: 0,
        families: [0; 8],
        interface: INTERFACE,
        sha256: [0; 32],
    };
    header.to_bytes().to_vec()
}

#[test]
fn the_room_for_code_is_counted_as_the_device_places_it() {
    let room = CodeRoom {
        total: 40_960,
        used: 8192,
        granule: 4096,
        max_plugins: Some(3),
        placed: 1,
        shared: false,
        larger: None,
    };
    // Whole granules, or the image's own alignment where larger.
    assert_eq!(room.needs(1, 16), 4096);
    assert_eq!(room.needs(4096, 16), 4096);
    assert_eq!(room.needs(4097, 16), 8192);
    assert_eq!(room.needs(100, 16_384), 16_384);
    assert_eq!(room.left(8192), 32_768);
    assert!(room.fits(8192, 1, 32_768));
    assert!(!room.fits(8192, 1, 32_769));
    // No more plugins than the device places.
    assert!(!room.fits(0, 3, 4096));
    // Kept by the shell, read by the store; none at all is said too.
    let storage = crate::MemoryStorage::new();
    assert_eq!(code_room(&storage), None);
    assert!(keep_code_room(&storage, Some(&room)));
    assert_eq!(code_room(&storage), Some(room));
    let shared = CodeRoom {
        max_plugins: None,
        shared: true,
        granule: 32,
        ..room
    };
    assert!(keep_code_room(&storage, Some(&shared)));
    assert_eq!(code_room(&storage), Some(shared));
    // The room a full flash would give, kept too; a file of an older
    // shell (six numbers) reads as none known.
    let small = room.in_slot(0x64_0000, 0x7F_0000);
    assert!(keep_code_room(&storage, Some(&small)));
    assert_eq!(code_room(&storage), Some(small));
    storage.write(ROOM, b"1000 0 4096 77 0 0\n").unwrap();
    assert_eq!(code_room(&storage).map(|r| r.larger), Some(None));
    assert!(keep_code_room(&storage, None));
    assert_eq!(code_room(&storage), None);
    assert!(keep_code_room(&storage, None));
}

#[test]
fn the_room_is_made_as_each_device_places_code() {
    // The X4: a region of 1 MB, its first sector the index, two
    // images up to 40 KB after it.
    let x4 = CodeRoom::in_flash(0x66_0000, 0x76_0000, 0x66_1000 + 40_960, 2);
    assert_eq!((x4.total, x4.used, x4.granule), (1_044_480, 40_960, 4096));
    assert_eq!((x4.placed, x4.max_plugins, x4.shared), (2, Some(77), false));
    // Nothing placed yet: none of it used; a region too small: none.
    assert_eq!(CodeRoom::in_flash(0, 0x10_0000, 0, 0).used, 0);
    assert_eq!(CodeRoom::in_flash(0x1000, 0x1000, 0, 0).total, 0);
    // In a stock slot of 6.25 MiB where PaperPaw's are 7.94 MiB: the
    // room grows by the difference with a full flash; in its own, none.
    let stock = x4.in_slot(0x64_0000, 0x7F_0000);
    assert_eq!(stock.larger, Some(1_044_480 + 0x1B_0000));
    assert_eq!(x4.in_slot(0x7F_0000, 0x7F_0000).larger, None);
    // The X4 Pro: what the images took and what was free beside them.
    let pro = CodeRoom::in_psram(53_000, 6_000_000, 1);
    assert_eq!((pro.total, pro.used, pro.granule), (6_053_000, 53_000, 32));
    assert_eq!((pro.max_plugins, pro.shared), (None, true));
}

#[test]
fn installed_code_is_measured_by_its_headers() {
    let room = CodeRoom {
        total: 1 << 20,
        granule: 4096,
        ..CodeRoom::default()
    };
    let storage = crate::MemoryStorage::new()
        .with("plugins/clock/code.ppn", header_of(5000, 16))
        .with("plugins/clock/plugin.ini", "[plugin]\n")
        .with("plugins/theme/plugin.ini", "[plugin]\n")
        .with("plugins/broken/code.ppn", "PPNP but short")
        // A download being put together: not installed.
        .with("plugins/.clock.part/code.ppn", header_of(5000, 16))
        .with("plugins/weather/code.ppn", header_of(100, 16));
    // The theme has no code, the broken file no header.
    let none = |_: &str| false;
    assert_eq!(
        installed_code_need(&storage, &room, "code.ppn", &none),
        InstalledNeed {
            taken: 8192 + 4096,
            plugins: 2,
            ..InstalledNeed::default()
        }
    );
    // The clock switched off: kept apart.
    let clock_off = |id: &str| id == "clock";
    assert_eq!(
        installed_code_need(&storage, &room, "code.ppn", &clock_off),
        InstalledNeed {
            taken: 4096,
            plugins: 1,
            off: 8192,
            off_plugins: 1,
        }
    );
    assert_eq!(
        read_header(&storage, "plugins/clock/code.ppn").map(|h| h.image_len),
        Some(5000)
    );
    assert!(read_header(&storage, "plugins/broken/code.ppn").is_none());
    assert_eq!(
        installed_code_need(&storage, &room, "code.so", &none),
        InstalledNeed::default()
    );
}

#[test]
fn code_is_named_as_installed() {
    let storage = crate::MemoryStorage::new();
    assert_eq!(installed_code(&storage, "clock"), None);
    assert!(remember_code(&storage, "clock", &[1; 32]));
    assert!(remember_author(&storage, "clock", &[2; 32]));
    assert!(remember_code(&storage, "clock", &[3; 32]));
    assert_eq!(installed_code(&storage, "clock"), Some([3; 32]));
    // Kept apart from the authors.
    assert_eq!(known_author(&storage, "clock"), Some([2; 32]));
    assert_eq!(installed_code(&storage, "clock-face"), None);
}

#[test]
fn authors_are_known_by_plugin() {
    let storage = crate::MemoryStorage::new();
    assert_eq!(known_author(&storage, "clock"), None);
    assert_eq!(author(&storage, "clock", &[1; 32]), Author::New);
    assert!(remember_author(&storage, "clock", &[1; 32]));
    assert!(remember_author(&storage, "clock-face", &[2; 32]));
    assert_eq!(author(&storage, "clock", &[1; 32]), Author::Known);
    assert_eq!(author(&storage, "clock", &[2; 32]), Author::Other);
    // The call itself replaces a known key (the callers refuse `Other`
    // first): the new one from now on.
    assert!(remember_author(&storage, "clock", &[3; 32]));
    assert_eq!(known_author(&storage, "clock"), Some([3; 32]));
    assert_eq!(known_author(&storage, "clock-face"), Some([2; 32]));
}

#[test]
fn fingerprints_as_text() {
    let fingerprint = Fingerprint(0x0085_073a_6132_3717);
    assert_eq!(fingerprint.to_string(), "0085073a61323717");
    assert_eq!(Fingerprint::parse("0085073a61323717"), Some(fingerprint));
    assert_eq!(Fingerprint::parse(" 0085073A61323717\n"), Some(fingerprint));
    assert_eq!(Fingerprint::parse("85073a61323717"), None);
    assert_eq!(Fingerprint::parse("0085073a6132371g"), None);
    assert_eq!(Fingerprint::parse("+085073a61323717"), None);
}

/// A plugin of another interface, or needing more than this
/// PaperPaw has, is said to be older or to need a newer PaperPaw, not
/// "another build".
#[test]
fn older_and_newer_plugins_are_said_as_such() {
    let native = Native::new("code.ppn");
    let header = Header::read(&file()).unwrap();
    let fits = Header {
        fingerprint: FINGERPRINT.0,
        ..header
    };
    assert_eq!(native.problem(&fits), None);
    assert_eq!(native.problem(&header), Some(PluginProblem::OtherBuild));
    let older = Header {
        interface: INTERFACE - 1,
        ..fits
    };
    assert_eq!(
        native.problem(&older),
        Some(PluginProblem::OlderInterface {
            built_for: INTERFACE - 1
        })
    );
    for newer in [
        Header {
            interface: INTERFACE + 1,
            ..fits
        },
        Header {
            needs_api: native.api_size + 4,
            ..fits
        },
        Header {
            families: [native.family_revs[0] + 1, 0, 0, 0, 0, 0, 0, 0],
            ..fits
        },
    ] {
        assert_eq!(
            native.problem(&newer),
            Some(PluginProblem::NeedsNewerPaperPaw)
        );
    }
    // A file of an older format: its fingerprint names its interface.
    let mut old = file();
    old[4] = 3;
    old[8..16].copy_from_slice(&Fingerprint::of(4, 7).0.to_le_bytes());
    let problem = paperpaw_native::Problem::OlderFormat;
    assert_eq!(
        Native::problem_of_file(&old, problem),
        PluginProblem::OlderInterface { built_for: 4 }
    );
    assert_eq!(
        Native::problem_of_file(b"PP", problem),
        PluginProblem::Damaged
    );
    assert_eq!(
        Native::problem_of_file(&[], paperpaw_native::Problem::NewerFormat),
        PluginProblem::NeedsNewerPaperPaw
    );
}

/// A small plugin file of `Header`'s own tests: no code to run, only
/// to check.
pub(crate) fn file() -> Vec<u8> {
    let mut header = Header {
        chip: Chip::Esp32C3,
        fingerprint: 1,
        base: paperpaw_native::LINK_BASE,
        image_len: 128,
        code_at: 96,
        entry: 0,
        api: 32,
        relocs: 0,
        align: 16,
        compiler: 1,
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        needs_api: 0,
        families: [0; 8],
        interface: INTERFACE,
        sha256: [0; 32],
    };
    let image = [9u8; 128];
    let mut sha = header.digest();
    sha.update(&image);
    header.sha256 = sha.finish();
    let mut file = header.to_bytes().to_vec();
    file.extend_from_slice(&image);
    file
}

#[test]
fn plugin_files_on_the_storage_are_checked_whole() {
    let storage = crate::MemoryStorage::new();
    let unsigned = file();
    let mut signed = unsigned.clone();
    let author = signature::sign(&unsigned, &[2; 32]);
    signed.extend_from_slice(&author.to_bytes());
    storage.write("a.ppn", &unsigned).unwrap();
    storage.write("b.ppn", &signed).unwrap();
    assert_eq!(check_file(&storage, "a.ppn").map(|(_, s)| s), Ok(None));
    assert_eq!(
        check_file(&storage, "b.ppn").map(|(_, s)| s),
        Ok(Some(author))
    );
    let mut changed = signed.clone();
    changed[100] ^= 1;
    storage.write("c.ppn", &changed).unwrap();
    assert_eq!(check_file(&storage, "c.ppn"), Err(Problem::Damaged));
    let mut resigned = signed.clone();
    resigned[signed.len() - 30] ^= 1;
    storage.write("d.ppn", &resigned).unwrap();
    assert_eq!(check_file(&storage, "d.ppn"), Err(Problem::Signature));
    storage.write("e.ppn", &signed[..signed.len() - 1]).unwrap();
    assert_eq!(check_file(&storage, "e.ppn"), Err(Problem::Length));
    assert_eq!(check_file(&storage, "none.ppn"), Err(Problem::Length));
}

/// The web flasher's bundle installs only what `install` would: the
/// same rules, kept in two places as the format crate is below the SDK.
#[test]
fn bundles_take_the_names_install_takes() {
    use paperpaw_native::bundle::{is_plugin_id, is_safe_path};
    let ids = [
        "a",
        "reading-nook",
        "a1",
        "",
        "-a",
        "a-",
        "A",
        "a.b",
        "a_b",
        "\u{e9}",
    ];
    let long_id = "a".repeat(41);
    for id in ids.iter().copied().chain([long_id.as_str(), &long_id[1..]]) {
        assert_eq!(
            is_plugin_id(id),
            crate::install::is_plugin_name(id),
            "{id:?}"
        );
    }
    let paths = [
        "plugin.ini",
        "i18n/de.ini",
        "fonts/A_b-c.ttf",
        "",
        "/a",
        "a/",
        "a//b",
        "../a",
        ".a",
        "a/.b",
        "a b",
        "a\\b",
        "a:b",
    ];
    let long_path = "a".repeat(101);
    for path in paths
        .iter()
        .copied()
        .chain([long_path.as_str(), &long_path[1..]])
    {
        assert_eq!(
            is_safe_path(path),
            crate::install::is_safe_path(path),
            "{path:?}"
        );
    }
}

#[test]
fn this_build_names_itself() {
    let native = Native::new("code.ppn");
    assert_eq!(native.fingerprint, FINGERPRINT);
    assert_eq!(native.toolchain, TOOLCHAIN);
    assert_eq!(native.path("plugins/hello/"), "plugins/hello/code.ppn");
}

/// A file of this interface fits, of another it says which; the
/// interface is in the fingerprint's top bits.
#[test]
fn a_file_of_another_interface_says_so() {
    assert_eq!(FINGERPRINT.interface(), INTERFACE);
    assert_eq!(FINGERPRINT.misfit(), None);
    let build = FINGERPRINT.0 & 0x0000_ffff_ffff_ffff;
    let next = Fingerprint::of(INTERFACE + 1, build);
    let message = next.misfit().unwrap();
    assert!(
        message.contains(&alloc::format!("plugin interface {}", INTERFACE + 1)),
        "{message}"
    );
    assert!(
        message.contains(&alloc::format!("has {INTERFACE}")),
        "{message}"
    );
    let other = Fingerprint::of(INTERFACE, build ^ 1);
    assert!(other.misfit().unwrap().contains("same plugin interface"));
}
