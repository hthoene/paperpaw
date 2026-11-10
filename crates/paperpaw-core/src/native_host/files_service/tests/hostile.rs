//! Hostile roots, features and paths on every op: the order of the checks
//! (framing, root, path rules, root-1 exclusions, then the op's rules), the
//! caps (512 bytes, 255 a segment), owner isolation and a refusal that
//! changes nothing.
use super::*;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_UTF8};

/// Every op with the path in the place a hostile one goes.
fn all_ops(host: &Rc<TestHost>, setup: Setup, root: u32, p: &str) -> [Result<Vec<u8>, i32>; 6] {
    [
        call_with(host, setup, 1, list_record(root, p, 0)),
        call_with(host, setup, 2, path(root, p)),
        call_with(host, setup, 3, path(root, p)),
        call_with(host, setup, 4, path(root, p)),
        call_with(host, setup, 5, list_record(root, p, 0)),
        call_with(host, setup, 6, move_record(root, p, root, "ok", 0)),
    ]
}

fn long(n: usize) -> std::string::String {
    "a".repeat(n)
}

#[test]
fn hostile_paths_are_refused_by_every_op_and_change_nothing() {
    let (card, host) = fixture();
    card.file("Keep/k", 1);
    card.file(&std::format!("{DATA}/own"), 1);
    let before = card.paths();
    let hostile: &[(&str, i32)] = &[
        ("..", ERR_INVALID),
        ("../x", ERR_INVALID),
        ("a/../x", ERR_INVALID),
        ("a/..", ERR_INVALID),
        (".", ERR_INVALID),
        ("a/./b", ERR_INVALID),
        ("/abs", ERR_INVALID),
        ("/", ERR_INVALID),
        ("a//b", ERR_INVALID),
        ("a/", ERR_INVALID),
        ("a\\b", ERR_INVALID),
        ("c:x", ERR_INVALID),
        ("a\0b", ERR_INVALID),
        ("a\nb", ERR_INVALID),
        ("a\tb", ERR_INVALID),
        ("a\u{7f}b", ERR_INVALID),
        ("trailing.", ERR_INVALID),
        ("trailing /x", ERR_INVALID),
        ("CON", ERR_INVALID),
        ("dir/nul.txt", ERR_INVALID),
        ("COM1", ERR_INVALID),
    ];
    for root in [0, 1] {
        for (bad, status) in hostile {
            // `space` takes only `""`: its own rule is the same refusal.
            for (n, result) in all_ops(&host, NORMAL, root, bad).into_iter().enumerate() {
                assert_eq!(result, Err(*status), "root {root} op {} {bad:?}", n + 1);
            }
        }
    }
    // Root 2 reads refuse the same; its changes are denied first.
    for (bad, status) in hostile {
        assert_eq!(
            call(&host, 1, list_record(2, bad, 0)),
            Err(*status),
            "{bad:?}"
        );
        assert_eq!(call(&host, 2, path(2, bad)), Err(*status), "{bad:?}");
        assert_eq!(call(&host, 4, path(2, bad)), Err(ERR_DENIED), "{bad:?}");
    }
    assert_eq!(card.paths(), before, "refused calls changed nothing");
}

#[test]
fn the_caps_are_512_bytes_and_255_a_segment() {
    let (card, host) = fixture();
    card.dir("Books");
    // 255 bytes in a segment is a name; 256 is over the cap.
    let ok = long(255);
    let over = long(256);
    assert_eq!(call(&host, 2, path(1, &ok)), Err(ERR_NOT_FOUND));
    assert_eq!(call(&host, 2, path(1, &over)), Err(ERR_NO_SPACE));
    assert_eq!(call(&host, 4, path(1, &over)), Err(ERR_NO_SPACE));
    // 512 bytes of path (two segments of 255 and a slash) is the limit.
    let two = std::format!("{}/{}", long(255), long(255));
    assert_eq!(two.len(), 511);
    assert_eq!(call(&host, 2, path(1, &two)), Err(ERR_NOT_FOUND));
    let max = std::format!("{}/{}b", long(255), long(255));
    assert_eq!(max.len(), 512);
    assert_eq!(call(&host, 2, path(1, &max)), Err(ERR_NO_SPACE));
    let max = std::format!("{}/{}", long(255), long(256));
    assert_eq!(call(&host, 2, path(1, &max)), Err(ERR_NO_SPACE));
    // The data folder's own prefix does not cut the plugin's 512 bytes.
    let deep = std::format!("{}/{}/{}", long(170), long(170), long(170));
    assert_eq!(deep.len(), 512);
    assert_eq!(call(&host, 2, path(0, &deep)), Err(ERR_NOT_FOUND));
    assert_eq!(
        call(&host, 4, path(0, &deep)),
        Ok(Vec::new()),
        "the longest path of root 0 can be made"
    );
    assert!(call(&host, 2, path(0, &deep)).is_ok());
    // One more byte cannot even be framed: the record caps a path at 512.
    let one_more = std::format!("{deep}x");
    let mut buffer = std::vec![0; 4096];
    assert!(wire::records_encode_counted(&[path(0, &one_more)], 1, 1024, &mut buffer).is_err());
    let tail = std::format!("{}/{}", long(255), long(256));
    assert_eq!(
        call(&host, 6, move_record(1, &tail, 1, "x", 0)),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(
        call(&host, 6, move_record(1, "x", 1, &over, 0)),
        Err(ERR_NO_SPACE)
    );
}

#[test]
fn a_path_that_is_not_utf8_is_refused() {
    let (_, host) = fixture();
    for id in [1u32, 2, 3, 4] {
        let op = schema_op(id);
        let record = if id == 1 {
            list_record(1, "abcd", 0)
        } else {
            path(1, "abcd")
        };
        let mut bytes = input(id, record);
        let at = bytes.windows(4).position(|w| w == b"abcd").unwrap();
        bytes[at + 1] = 0xff;
        let mut out = std::vec![0; op.max_reply];
        let result = with(&host, NORMAL, |frame| frame.run(op, &bytes, &mut out));
        assert_eq!(result, Err(ERR_UTF8), "op {id}");
    }
}

#[test]
fn roots_three_and_above_are_unsupported_and_come_before_path_rules() {
    let (_, host) = fixture();
    for root in [3, 4, 255, u32::MAX] {
        for (n, result) in all_ops(&host, NORMAL, root, "../hostile")
            .into_iter()
            .enumerate()
        {
            assert_eq!(result, Err(ERR_UNSUPPORTED), "root {root} op {}", n + 1);
        }
    }
}

#[test]
fn root_one_needs_the_declared_feature() {
    let (card, host) = fixture();
    card.file("Books/a", 1);
    // Not asked for: denied, before the path rules and on every op.
    let denied = Setup {
        granted: false,
        ..NORMAL
    };
    for (n, result) in all_ops(&host, denied, 1, "../x").into_iter().enumerate() {
        assert_eq!(result, Err(ERR_DENIED), "op {}", n + 1);
    }
    assert_eq!(call_with(&host, denied, 3, path(1, "")), Err(ERR_DENIED));
    // Roots 0 and 2 need nothing.
    assert!(call_with(&host, denied, 3, path(0, "")).is_ok());
    assert!(call_with(&host, denied, 2, path(0, "")).is_ok());
    assert!(call_with(&host, denied, 4, path(0, "cache")).is_ok());
    // Moves: either side.
    assert_eq!(
        call_with(&host, denied, 6, move_record(0, "x", 1, "y", 0)),
        Err(ERR_DENIED)
    );
    assert_eq!(
        call_with(&host, denied, 6, move_record(1, "x", 0, "y", 0)),
        Err(ERR_DENIED)
    );
    assert!(card.has("Books/a"));
}

#[test]
fn a_host_without_the_feature_offers_no_root_one() {
    let (_, host) = fixture();
    // The host's catalog has no `uses.storage`: unsupported, not denied.
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let hosts = crate::native_host::host_services::HostServices::new();
    hosts.wire_files(host.clone()).unwrap();
    let frame = TestFrame {
        env: &env,
        card: Some(&*host.card),
        plugin: PLUGIN,
        hosts,
        granted: true,
    };
    let caller = crate::native_host::host_services::Caller {
        owner: OWNER,
        plugin: PLUGIN,
        features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
        hosts: &frame.hosts,
    };
    let op = schema_op(3);
    let bytes = input(3, path(1, ""));
    let mut out = std::vec![0; op.max_reply];
    assert_eq!(
        backend(&frame, &caller, op, &bytes, &mut out),
        Err(ERR_UNSUPPORTED)
    );
    let bytes = input(3, path(0, ""));
    assert!(backend(&frame, &caller, op, &bytes, &mut out).is_ok());
}

#[test]
fn a_plugin_reaches_only_its_own_folders() {
    let (card, host) = fixture();
    card.file(".paperpaw/plugins/battery/data.txt", 1);
    card.file(".paperpaw/plugins/battery-plus/data.txt", 1);
    card.file("plugins/battery/x", 1);
    card.file("plugins/battery-plus/x", 1);
    let battery = Setup {
        plugin: "battery",
        ..NORMAL
    };
    // Root 0 is `battery`'s folder, root 2 its install folder.
    let own = call_with(&host, battery, 1, list_record(0, "", 0)).unwrap();
    assert_eq!(
        rows(&own)
            .0
            .iter()
            .map(|r| r.0.as_str())
            .collect::<Vec<_>>(),
        ["data.txt"]
    );
    // Removing its data folder leaves `battery-plus`.
    let progress = call_with(&host, battery, 5, list_record(0, "", 0)).unwrap();
    assert!(
        matches!(wire::record_decode_one(&progress).unwrap(), Record::ServiceFsProgress(p) if p.flags == 1)
    );
    assert!(!card.has(".paperpaw/plugins/battery/data.txt"));
    assert!(card.has(".paperpaw/plugins/battery-plus/data.txt"));
    assert!(card.has("plugins/battery/x") && card.has("plugins/battery-plus/x"));
    // The install folder is not changed.
    assert_eq!(
        call_with(&host, battery, 5, list_record(2, "", 0)),
        Err(ERR_DENIED)
    );
    // No way across by moves.
    assert_eq!(
        call_with(
            &host,
            battery,
            6,
            move_record(0, "../battery-plus/data.txt", 0, "x", 0)
        ),
        Err(ERR_INVALID)
    );
    assert_eq!(
        call_with(&host, battery, 6, move_record(2, "x", 0, "y", 0)),
        Err(ERR_DENIED)
    );
    // An id that cannot be a segment is refused, never joined.
    for id in ["", ".", "..", "a/b", "a\\b"] {
        let odd = Setup {
            plugin: id,
            ..NORMAL
        };
        assert_eq!(
            call_with(&host, odd, 1, list_record(0, "", 0)),
            Err(ERR_INVALID),
            "{id:?}"
        );
        assert_eq!(
            call_with(&host, odd, 5, list_record(0, "", 0)),
            Err(ERR_INVALID),
            "{id:?}"
        );
    }
    assert!(card.has(".paperpaw/plugins/battery-plus/data.txt"));
}

#[test]
fn root_one_never_reaches_the_install_or_kernel_folders() {
    let (card, host) = fixture();
    card.file("plugins/files/lib.bin", 1);
    card.file(".paperpaw/settings.ini", 1);
    card.file(&std::format!("{DATA}/own"), 1);
    host.aliases.borrow_mut().push((1, "PAPERP~1"));
    for first in [
        "plugins",
        "Plugins",
        "PLUGINS",
        ".paperpaw",
        ".PAPERPAW",
        "PAPERP~1",
    ] {
        for tail in ["", "/files", "/files/lib.bin", "/new"] {
            let p = std::format!("{first}{tail}");
            for (n, result) in all_ops(&host, NORMAL, 1, &p).into_iter().enumerate() {
                assert_eq!(result, Err(ERR_INVALID), "op {} {p}", n + 1);
            }
            let moved = call(&host, 6, move_record(1, "Books/x", 1, &p, 0));
            assert_eq!(moved, Err(ERR_INVALID), "{p}");
        }
    }
    // Spelled like them below another folder, they are ordinary names.
    assert_eq!(call(&host, 4, path(1, "Books/plugins")), Ok(Vec::new()));
    assert!(card.has("plugins/files/lib.bin") && card.has(".paperpaw/settings.ini"));
}

#[test]
fn root_one_never_reaches_paperpaw_ini_by_any_op_spelling_or_end_of_a_move() {
    // The kernel's settings file: a plugin that could write it could turn off
    // the user's second firmware (`dual-boot = off`). It is its own 8.3 name,
    // so case and the driver's folding are all the spellings there are.
    let (card, host) = fixture();
    card.file("paperpaw.ini", 7);
    card.file("Books/a.txt", 1);
    card.file(&std::format!("{DATA}/own"), 1);
    let before = card.paths();
    for name in [
        "paperpaw.ini",
        "PAPERPAW.INI",
        "Paperpaw.Ini",
        "paperpaw.INI",
        // The driver folds `ı` to `I`: this is the same file to it.
        "paperpaw.ını",
        "PAPERPAW.INı",
    ] {
        // list, stat, space, make_dir, remove_tree (delete) and move away.
        for (n, result) in all_ops(&host, NORMAL, 1, name).into_iter().enumerate() {
            assert_eq!(result, Err(ERR_INVALID), "op {} {name}", n + 1);
        }
        // Create under it, and write-ish ops below it.
        for tail in ["/x", "/x/y"] {
            let p = std::format!("{name}{tail}");
            for (n, result) in all_ops(&host, NORMAL, 1, &p).into_iter().enumerate() {
                assert_eq!(result, Err(ERR_INVALID), "op {} {p}", n + 1);
            }
        }
        // A move onto it (rename onto, replace or not) and one away from it.
        for flags in [0, 1] {
            assert_eq!(
                call(&host, 6, move_record(1, "Books/a.txt", 1, name, flags)),
                Err(ERR_INVALID),
                "to {name}"
            );
            assert_eq!(
                call(&host, 6, move_record(1, name, 1, "Books/b.txt", flags)),
                Err(ERR_INVALID),
                "from {name}"
            );
            // Between the roots: from the plugin's data, and into it.
            assert_eq!(
                call(&host, 6, move_record(0, "own", 1, name, flags)),
                Err(ERR_INVALID),
                "data to {name}"
            );
            assert_eq!(
                call(&host, 6, move_record(1, name, 0, "own", flags)),
                Err(ERR_INVALID),
                "{name} to data"
            );
        }
    }
    // Nothing changed, and the host's own read of it is whole.
    assert_eq!(card.paths(), before);
    assert!(card.has("paperpaw.ini") && card.has("Books/a.txt"));
    // Below another folder it is an ordinary name; so are its neighbours.
    for other in [
        "Books/paperpaw.ini",
        "paperpaw.ini2",
        "paperpaw.inx",
        "xpaperpaw.ini",
    ] {
        assert_eq!(call(&host, 4, path(1, other)), Ok(Vec::new()), "{other}");
    }
}

#[test]
fn a_wrong_or_missing_record_is_invalid() {
    let (_, host) = fixture();
    let op = schema_op(2);
    let mut out = std::vec![0; op.max_reply];
    // An empty batch, a truncated one and garbage.
    for bytes in [&[][..], &[0u8][..], &[0xff; 9][..]] {
        let result = with(&host, NORMAL, |frame| frame.run(op, bytes, &mut out));
        assert!(result.is_err(), "{bytes:?}");
    }
    // Every op given the record of another.
    assert_eq!(call(&host, 4, list_record(1, "x", 0)), Err(ERR_INVALID));
    assert_eq!(call(&host, 5, path(1, "x")), Err(ERR_INVALID));
    assert_eq!(call(&host, 6, path(1, "x")), Err(ERR_INVALID));
    assert_eq!(call(&host, 1, path(1, "x")), Err(ERR_INVALID));
    assert_eq!(
        call(&host, 3, move_record(1, "a", 1, "b", 0)),
        Err(ERR_INVALID)
    );
}
