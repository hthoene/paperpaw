use super::*;
use crate::native_host::HostResources;
use core::pin::pin;
use paperpaw_native::host_glue::{ERR_FULL, ERR_UTF8};
use paperpaw_platform::Instant;
use paperpaw_sdk::builtin::fonts as roles;
use paperpaw_sdk::{Font, FontSpec, Id};

fn bytes(text: &[u8]) -> PpBytes {
    PpBytes {
        ptr: text.as_ptr(),
        len: text.len(),
    }
}

fn add_font(kernel: &mut crate::Kernel, id: &'static str, advance: f32) {
    let mut pack = paperpaw_text::PackBuilder::new();
    pack.add_strike(
        10.0,
        paperpaw_text::LineMetrics::new(8.0, 2.0, 12.0),
        alloc::vec![('a', paperpaw_text::Glyph::empty(advance))],
    );
    kernel
        .shared
        .fonts
        .add(FontSpec::new(
            Id::parse(id).unwrap(),
            Font::from_data(pack.build(), &[]).unwrap(),
        ))
        .unwrap();
}

fn kernel() -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    add_font(&mut kernel, "pack.face", 5.0);
    kernel
}

fn scoped(kernel: &crate::Kernel, call: impl FnOnce(&EnvironmentScope)) {
    let env = kernel.shared.env(Instant(0));
    let frame = crate::abi7::core_frame::CoreFrame {
        callback: None,
        instance: None,
        env: &env,
        meter: &kernel.shared.meter,
        depth: None,
        directory: None,
        stack_generation: kernel.shared.stack_rows.generation(),
        stack_rows: &kernel.shared.stack_rows,
        font_removals: &kernel.shared.font_removals,
        registry_generation: &kernel.shared.registry_generation,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
        log: Default::default(),
        ui: None,
    };
    let scope = pin!(EnvironmentScope::new());
    scope.as_ref().scope_core(&frame, || call(&scope)).unwrap();
}

fn get(scope: &EnvironmentScope, owner: &OwnerResources, key: &str) -> u32 {
    let mut handle = 0;
    // SAFETY: live readable input and exclusive, disjoint scalar output.
    assert_eq!(
        unsafe { acquire(scope, owner, bytes(key.as_bytes()), &mut handle) },
        OK
    );
    assert_ne!(handle, 0);
    handle
}

#[test]
fn named_metrics_measure_release_and_invalid_inputs_preserve_outputs() {
    let kernel = kernel();
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    scoped(&kernel, |scope| {
        let handle = get(scope, &owner, "pack.face");
        let mut width = -91.0;
        let mut values = [-91.0; 3];
        // SAFETY: initialized text and disjoint exclusive scalar outputs.
        unsafe {
            assert_eq!(
                measure(scope, &owner, handle, 10.0, bytes(b"aa"), &mut width),
                OK
            );
            assert_eq!(width, 10.0);
            let out = values.as_mut_ptr();
            assert_eq!(
                metrics(scope, &owner, handle, 10.0, out, out.add(1), out.add(2)),
                OK
            );
            assert_eq!(values, [8.0, 2.0, 12.0]);
            for size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
                width = -91.0;
                assert_eq!(
                    measure(scope, &owner, handle, size, bytes(b"a"), &mut width),
                    ERR_INVALID
                );
                assert_eq!(width, -91.0);
            }
            assert_eq!(
                measure(scope, &owner, handle, 10.0, bytes(&[0xff]), &mut width),
                ERR_UTF8
            );
            assert_eq!(
                metrics(scope, &owner, handle, 10.0, out, out, out.add(2)),
                ERR_INVALID
            );
            assert_eq!(values, [8.0, 2.0, 12.0]);
            assert_eq!(
                metrics(
                    scope,
                    &owner,
                    handle,
                    10.0,
                    core::ptr::null_mut(),
                    out.add(1),
                    out.add(2)
                ),
                ERR_INVALID
            );
            let unaligned = out.cast::<u8>().add(1).cast();
            assert_eq!(
                metrics(
                    scope,
                    &owner,
                    handle,
                    10.0,
                    unaligned,
                    out.add(1),
                    out.add(2)
                ),
                ERR_INVALID
            );
        }
        assert_eq!(release(&owner, handle), OK);
        // SAFETY: valid text and exclusive output, handle now deliberately stale.
        assert_eq!(
            unsafe { measure(scope, &owner, handle, 10.0, bytes(b"a"), &mut width) },
            ERR_STATE
        );
    });
}

#[test]
fn same_id_replacement_stales_named_handles_but_roles_follow_the_new_font() {
    let mut kernel = kernel();
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    let mut named = 0;
    let mut role = 0;
    scoped(&kernel, |scope| {
        named = get(scope, &owner, "pack.face");
        role = get(scope, &owner, "role:reading-bold-italic");
    });
    assert_eq!(kernel.remove_plugin_fonts(&Id::new_static("pack")), 1);
    add_font(&mut kernel, "pack.face", 7.0);
    scoped(&kernel, |scope| {
        let mut width = -91.0;
        // SAFETY: live text and exclusive scalar output.
        unsafe {
            assert_eq!(
                measure(scope, &owner, named, 10.0, bytes(b"a"), &mut width),
                ERR_STATE
            );
            assert_eq!(width, -91.0);
            assert_eq!(
                measure(scope, &owner, role, 10.0, bytes(b"a"), &mut width),
                OK
            );
            assert_eq!(width, 7.0);
            let fresh = get(scope, &owner, "pack.face");
            assert_eq!(
                measure(scope, &owner, fresh, 10.0, bytes(b"a"), &mut width),
                OK
            );
        }
    });
}

#[test]
fn icons_acquire_unbound_and_all_font_keys_share_the_eight_font_cap() {
    let kernel = kernel();
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    scoped(&kernel, |scope| {
        let icons = get(scope, &owner, "role:icons");
        let mut width = -91.0;
        // SAFETY: live text, exclusive scalar output, unbound role is intentional.
        unsafe {
            assert_eq!(
                measure(scope, &owner, icons, 10.0, bytes(b"a"), &mut width),
                ERR_NOT_FOUND
            );
            assert_eq!(width, -91.0);
            let mut out = 91;
            for key in ["role:mono", "role:ui-bold", "role:unknown"] {
                assert_eq!(
                    acquire(scope, &owner, bytes(key.as_bytes()), &mut out),
                    ERR_NOT_FOUND
                );
                assert_eq!(out, 91);
            }
            for _ in 1..8 {
                get(scope, &owner, "pack.face");
            }
            assert_eq!(
                acquire(scope, &owner, bytes(b"role:ui"), &mut out),
                ERR_FULL
            );
            assert_eq!(out, 91);
        }
    });
}

#[test]
fn env_only_named_access_fails_closed_while_roles_work_and_release_needs_no_frame() {
    let kernel = kernel();
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    let env = kernel.shared.env(Instant(0));
    let scope = pin!(EnvironmentScope::new());
    let mut role_handle = 0;
    scope
        .as_ref()
        .scope(&env, || {
            role_handle = get(&scope, &owner, "role:ui");
            let mut out = 91;
            // SAFETY: valid key and exclusive output, no trusted removal tracker.
            assert_eq!(
                unsafe { acquire(&scope, &owner, bytes(b"pack.face"), &mut out) },
                ERR_STATE
            );
            assert_eq!(out, 91);
        })
        .unwrap();
    assert_eq!(release(&owner, role_handle), OK);
    assert_eq!(release(&owner, role_handle), ERR_STATE);
}

#[test]
fn ink_without_drawn_pixels_and_refusals_preserve_all_outputs() {
    let kernel = kernel();
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    scoped(&kernel, |scope| {
        let handle = get(scope, &owner, "pack.face");
        let mut values = [91; 4];
        let out = values.as_mut_ptr();
        // SAFETY: live text, disjoint writable scalar outputs.
        unsafe {
            assert_eq!(
                ink(
                    scope,
                    &owner,
                    handle,
                    10.0,
                    bytes(b"a"),
                    out,
                    out.add(1),
                    out.add(2),
                    out.add(3)
                ),
                ERR_NOT_FOUND
            );
            assert_eq!(values, [91; 4]);
            assert_eq!(
                ink(
                    scope,
                    &owner,
                    handle,
                    10.0,
                    bytes(b"a"),
                    out,
                    out.add(1),
                    core::ptr::null_mut(),
                    out.add(3)
                ),
                ERR_INVALID
            );
            assert_eq!(values, [91; 4]);
            release(&owner, handle);
            assert_eq!(
                ink(
                    scope,
                    &owner,
                    handle,
                    10.0,
                    bytes(b"a"),
                    out,
                    out.add(1),
                    out.add(2),
                    out.add(3)
                ),
                ERR_STATE
            );
            assert_eq!(values, [91; 4]);
        }
    });
}

fn add_icon_font(kernel: &mut crate::Kernel, chars: &[char]) {
    let mut pack = paperpaw_text::PackBuilder::new();
    pack.add_strike(
        10.0,
        paperpaw_text::LineMetrics::new(8.0, 2.0, 12.0),
        chars
            .iter()
            .map(|c| (*c, paperpaw_text::Glyph::empty(5.0)))
            .collect(),
    );
    kernel
        .shared
        .fonts
        .add(FontSpec::new(
            Id::parse("pack.icons").unwrap(),
            Font::from_data(pack.build(), &[]).unwrap(),
        ))
        .unwrap();
    kernel
        .shared
        .fonts
        .bind_user(roles::ICONS, Id::parse("pack.icons").unwrap());
}

fn glyph(scope: &EnvironmentScope, name: &[u8], cap: usize) -> (i32, [u8; 8]) {
    let mut out = [0xaa_u8; 8];
    let len = unsafe {
        icon_glyph(
            scope,
            bytes(name),
            PpBuffer {
                ptr: out.as_mut_ptr(),
                cap,
            },
        )
    };
    (len, out)
}

#[test]
fn icon_glyph_writes_the_utf8_of_a_named_icon_of_the_bound_font() {
    let star = paperpaw_sdk::icons::char("star").unwrap();
    let mut with = kernel();
    add_icon_font(&mut with, &[star]);
    scoped(&with, |scope| {
        let (len, out) = glyph(scope, b"star", 8);
        let mut want = [0_u8; 4];
        let want = star.encode_utf8(&mut want).as_bytes();
        assert_eq!(usize::try_from(len).unwrap(), want.len());
        assert_eq!(&out[..want.len()], want);
        assert_eq!(out[want.len()], 0xaa, "only the character is written");
        // A smaller `out` is ERR_NO_SPACE and nothing is written.
        assert_eq!(glyph(scope, b"star", 1), (ERR_NO_SPACE, [0xaa; 8]));
        assert_eq!(glyph(scope, b"star", 0).0, ERR_NO_SPACE);
        // A name the host lacks, or one the font has no drawing for.
        assert_eq!(glyph(scope, b"no-such-icon", 8).0, ERR_NOT_FOUND);
        assert_eq!(glyph(scope, b"zoom-in", 8).0, ERR_NOT_FOUND);
        // Not `[a-z0-9-]` once, 1 to 64 bytes: invalid.
        for bad in [&b""[..], b"Star", b"a_b", b"a b", b"a/b", &[b'a'; 65][..]] {
            assert_eq!(glyph(scope, bad, 8).0, ERR_INVALID, "{bad:?}");
        }
        assert_eq!(glyph(scope, &[0xff], 8).0, ERR_UTF8);
    });
    // No icon font bound: not found, the plugin shows text.
    let without = kernel();
    scoped(&without, |scope| {
        assert_eq!(glyph(scope, b"star", 8).0, ERR_NOT_FOUND);
    });
}

#[test]
fn covers_answers_the_length_of_the_drawn_start_and_refuses_what_is_no_text() {
    let kernel = kernel();
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    scoped(&kernel, |scope| {
        let handle = get(scope, &owner, "pack.face");
        let ask = |text: &[u8]| {
            // SAFETY: live readable text for the call.
            unsafe { covers(scope, &owner, handle, bytes(text)) }
        };
        // The font draws only `a`: the length is where the first other
        // character starts, in bytes, and case is not folded.
        assert_eq!(ask(b""), 0);
        assert_eq!(ask(b"a"), 1);
        assert_eq!(ask(b"aab"), 2);
        assert_eq!(ask(b"b"), 0);
        assert_eq!(ask(b"aA"), 1);
        assert_eq!(ask("aaéa".as_bytes()), 2);
        assert_eq!(ask("éa".as_bytes()), 0);
        // The bound: 1,024 bytes are read, one more is invalid; no text is
        // ERR_UTF8 (also a cut character).
        let all = [b'a'; 1024];
        assert_eq!(ask(&all), 1024);
        assert_eq!(ask(&[b'a'; 1025]), ERR_INVALID);
        assert_eq!(ask(&[0xff]), ERR_UTF8);
        assert_eq!(ask(&"é".as_bytes()[..1]), ERR_UTF8);
        // Null with a length is no pair.
        // SAFETY: the pair is refused before any read.
        assert_eq!(
            unsafe {
                covers(
                    scope,
                    &owner,
                    handle,
                    PpBytes {
                        ptr: core::ptr::null(),
                        len: 3,
                    },
                )
            },
            ERR_INVALID
        );
        // An icon role with no icon font bound draws nothing.
        let icons = get(scope, &owner, "role:icons");
        // SAFETY: live readable text for the call.
        assert_eq!(unsafe { covers(scope, &owner, icons, bytes(b"a")) }, 0);
        // A stale or never given handle is ERR_STATE (the bad text still wins).
        assert_eq!(release(&owner, handle), OK);
        assert_eq!(ask(b"a"), ERR_STATE);
        assert_eq!(ask(&[0xff]), ERR_UTF8);
    });
}

#[test]
fn covers_outside_an_environment_callback_fails_closed() {
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    let scope = EnvironmentScope::new();
    // SAFETY: live readable text for the call.
    assert_eq!(unsafe { covers(&scope, &owner, 1, bytes(b"a")) }, ERR_STATE);
}

mod data_fonts {
    use super::*;
    use alloc::{rc::Rc, vec::Vec};
    use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID, ERR_UNSUPPORTED};
    use paperpaw_platform::Storage;
    use paperpaw_sdk::MemoryStorage;

    const FOLDER: &str = ".paperpaw/plugins/fonts-browser/";

    fn pack(advance: f32) -> Vec<u8> {
        let mut pack = paperpaw_text::PackBuilder::new();
        pack.add_strike(
            10.0,
            paperpaw_text::LineMetrics::new(8.0, 2.0, 12.0),
            alloc::vec![('a', paperpaw_text::Glyph::empty(advance))],
        );
        pack.build()
    }

    fn pack_with_two_glyphs() -> Vec<u8> {
        let mut pack = paperpaw_text::PackBuilder::new();
        pack.add_strike(
            10.0,
            paperpaw_text::LineMetrics::new(8.0, 2.0, 12.0),
            alloc::vec![
                ('a', paperpaw_text::Glyph::empty(6.0)),
                ('b', paperpaw_text::Glyph::empty(7.0)),
            ],
        );
        pack.build()
    }

    /// A card with a font pack, a file that is no font, and another plugin's
    /// pack; hosts wired with it; the owner is the plugin `fonts-browser`.
    fn world() -> (HostResources, OwnerResources, Rc<dyn Storage>) {
        let card: Rc<dyn Storage> = Rc::new(
            MemoryStorage::new()
                .with(&alloc::format!("{FOLDER}fonts/a.ppfp"), pack(6.0))
                .with(
                    &alloc::format!("{FOLDER}fonts/bad.ppfp"),
                    alloc::vec![7_u8; 64],
                )
                .with(
                    &alloc::format!("{FOLDER}fonts/bad.ttf"),
                    alloc::vec![7_u8; 64],
                )
                .with(".paperpaw/plugins/other/fonts/b.ppfp", pack(9.0))
                .with("fonts/top.ppfp", pack(9.0)),
        );
        let resources = HostResources::try_new().unwrap();
        resources.hosts().wire_font_card(card.clone()).unwrap();
        let mut owner = resources.try_owner().unwrap();
        owner.bind_caller("fonts-browser").unwrap();
        (resources, owner, card)
    }

    fn data(scope: &EnvironmentScope, owner: &OwnerResources, path: &str) -> Result<u32, i32> {
        let key = alloc::format!("data:{path}");
        let mut out = 91;
        // SAFETY: live readable key and exclusive scalar output.
        let status = unsafe { acquire(scope, owner, bytes(key.as_bytes()), &mut out) };
        if status == OK {
            Ok(out)
        } else {
            assert_eq!(out, 91, "a refusal leaves the output");
            Err(status)
        }
    }

    #[test]
    fn a_pack_of_the_data_folder_is_acquired_measured_and_released() {
        let kernel = kernel();
        let (_resources, owner, _card) = world();
        scoped(&kernel, |scope| {
            let handle = data(scope, &owner, "fonts/a.ppfp").unwrap();
            let mut width = 0.0;
            // SAFETY: live text, exclusive scalar output.
            assert_eq!(
                unsafe { measure(scope, &owner, handle, 10.0, bytes(b"a"), &mut width) },
                OK
            );
            assert!((width - 6.0).abs() < 0.01, "{width}");
            assert_eq!(release(&owner, handle), OK);
            assert_eq!(release(&owner, handle), ERR_STATE);
        });
    }

    #[test]
    fn paths_outside_the_data_folder_are_refused_before_the_card_is_asked() {
        let kernel = kernel();
        let (_resources, owner, _card) = world();
        scoped(&kernel, |scope| {
            for path in [
                "",
                "../other/fonts/b.ppfp",
                "fonts/../../other/fonts/b.ppfp",
                "/fonts/a.ppfp",
                "fonts//a.ppfp",
                "C:/fonts/a.ppfp",
                "fonts\\a.ppfp",
                "fonts/./a.ppfp",
            ] {
                assert_eq!(data(scope, &owner, path), Err(ERR_INVALID), "{path:?}");
            }
            // A key over 128 bytes is refused as every key is.
            let long = "a/".repeat(60);
            assert_eq!(data(scope, &owner, &long), Err(ERR_INVALID));
            let longest = alloc::format!("{}.ppfp", "a".repeat(118));
            assert_eq!(data(scope, &owner, &longest), Err(ERR_NOT_FOUND));
            // The card's top and another plugin's folder are not the folder.
            assert_eq!(data(scope, &owner, "fonts/top.ppfp"), Err(ERR_NOT_FOUND));
            assert_eq!(
                data(scope, &owner, "other/fonts/b.ppfp"),
                Err(ERR_NOT_FOUND)
            );
        });
    }

    #[test]
    fn a_file_changed_or_removed_after_the_acquire_makes_the_handle_stale() {
        let kernel = kernel();
        let (_resources, owner, card) = world();
        let path = alloc::format!("{FOLDER}fonts/a.ppfp");
        scoped(&kernel, |scope| {
            let handle = data(scope, &owner, "fonts/a.ppfp").unwrap();
            let mut width = 0.0;
            let mut measure_now = || {
                // SAFETY: live text, exclusive scalar output.
                unsafe { measure(scope, &owner, handle, 10.0, bytes(b"a"), &mut width) }
            };
            assert_eq!(measure_now(), OK);
            // Changed (another size): stale, also for a repeated call.
            card.write(&path, &pack_with_two_glyphs()).unwrap();
            assert_eq!(measure_now(), ERR_STATE);
            assert_eq!(measure_now(), ERR_STATE);
            // Acquired again: the new file.
            let again = data(scope, &owner, "fonts/a.ppfp").unwrap();
            assert_ne!(again, handle);
            // Removed: stale; a new acquire is not found.
            card.remove(&path).unwrap();
            let mut at = 0.0;
            // SAFETY: live text, exclusive scalar output.
            assert_eq!(
                unsafe { measure(scope, &owner, again, 10.0, bytes(b"a"), &mut at) },
                ERR_STATE
            );
            assert_eq!(data(scope, &owner, "fonts/a.ppfp"), Err(ERR_NOT_FOUND));
            // Released handles of either kind are released as usual.
            assert_eq!(release(&owner, handle), OK);
            assert_eq!(release(&owner, again), OK);
        });
    }

    #[test]
    fn the_glyph_cache_of_a_data_font_is_small_on_the_x4_and_bigger_elsewhere() {
        use crate::native_host::fonts_data::{CACHE_LARGE, CACHE_SMALL, cache_for};
        use paperpaw_sdk::PackLimits;
        assert_eq!((CACHE_SMALL, CACHE_LARGE), (4 * 1024, 16 * 1024));
        assert_eq!(cache_for(PackLimits::SMALL), CACHE_SMALL);
        assert_eq!(cache_for(PackLimits::LARGE), CACHE_LARGE);
    }

    #[test]
    fn a_missing_file_is_not_found_and_a_bad_one_invalid() {
        let kernel = kernel();
        let (_resources, owner, _card) = world();
        scoped(&kernel, |scope| {
            assert_eq!(data(scope, &owner, "fonts/none.ppfp"), Err(ERR_NOT_FOUND));
            assert_eq!(data(scope, &owner, "fonts/none.ttf"), Err(ERR_NOT_FOUND));
            assert_eq!(data(scope, &owner, "fonts/bad.ppfp"), Err(ERR_INVALID));
            assert_eq!(data(scope, &owner, "fonts/bad.ttf"), Err(ERR_INVALID));
            // Nothing was kept for the refusals: all eight handles are free.
            for _ in 0..8 {
                data(scope, &owner, "fonts/a.ppfp").unwrap();
            }
        });
    }

    #[test]
    fn data_fonts_share_the_eight_font_cap() {
        let kernel = kernel();
        let (_resources, owner, _card) = world();
        scoped(&kernel, |scope| {
            let mut handles = Vec::new();
            for _ in 0..7 {
                handles.push(data(scope, &owner, "fonts/a.ppfp").unwrap());
            }
            // The eighth is a role: roles and data fonts count together.
            get(scope, &owner, "role:ui");
            assert_eq!(data(scope, &owner, "fonts/a.ppfp"), Err(ERR_FULL));
            assert_eq!(release(&owner, handles.pop().unwrap()), OK);
            data(scope, &owner, "fonts/a.ppfp").unwrap();
        });
    }

    #[test]
    fn a_host_without_the_card_or_a_caller_refuses() {
        let kernel = kernel();
        let resources = HostResources::try_new().unwrap();
        let mut owner = resources.try_owner().unwrap();
        scoped(&kernel, |scope| {
            // No plugin bound to the owner.
            assert_eq!(data(scope, &owner, "fonts/a.ppfp"), Err(ERR_DENIED));
        });
        owner.bind_caller("fonts-browser").unwrap();
        scoped(&kernel, |scope| {
            // No card wired.
            assert_eq!(data(scope, &owner, "fonts/a.ppfp"), Err(ERR_UNSUPPORTED));
        });
    }

    #[test]
    fn another_plugins_handle_cannot_reach_the_first_ones_file() {
        let kernel = kernel();
        let (resources, _first, _card) = world();
        let mut other = resources.try_owner().unwrap();
        other.bind_caller("other").unwrap();
        scoped(&kernel, |scope| {
            assert_eq!(data(scope, &other, "fonts/a.ppfp"), Err(ERR_NOT_FOUND));
            data(scope, &other, "fonts/b.ppfp").unwrap();
        });
    }
}
