//! The `frames-in` family match and the `allow frame` exceptions (todo 637:
//! Senior #2899 M1, M2, R1).

use super::tests::{RETW_N, entry, f, run, sym};

/// The symbol of a trait-impl method (`<type as trait>::method`): its path is
/// the method's name, the impl's type is only in the symbol.
fn method(ty: &str, tr: &str, name: &str, hash: u32) -> String {
    let segment = format!("_$LT${ty}$u20$as$u20${tr}$GT$");
    format!(
        "_ZN{}{segment}{}{name}17h{hash:016x}E",
        segment.len(),
        name.len()
    )
}

fn root_and(extra: Vec<(String, Vec<u8>)>) -> Vec<(String, Vec<u8>)> {
    let mut funcs = vec![("root".to_string(), f(&[&entry(32), &RETW_N]))];
    funcs.extend(extra);
    funcs
}

fn scan(funcs: &[(String, Vec<u8>)], extra: &str) -> Vec<String> {
    let borrowed: Vec<(&str, Vec<u8>)> =
        funcs.iter().map(|(n, c)| (n.as_str(), c.clone())).collect();
    run(&borrowed, &["root"], extra).problems
}

#[test]
fn a_module_family_holds_the_trait_impl_methods_of_its_types() {
    let m = method(
        "paperpaw_core..native_host..shell..Thing",
        "paperpaw_core..Tr",
        "write",
        1,
    );
    let funcs = root_and(vec![(m, f(&[&entry(2048), &RETW_N]))]);
    let held = scan(&funcs, "frames-in paperpaw_core::native_host the host\n");
    assert_eq!(held.len(), 1, "{held:?}");
    assert!(held[0].contains("takes a frame of"), "{held:?}");
    // Another module's type is not in the family (and the family matches something).
    let other = method(
        "paperpaw_core..other..Thing",
        "paperpaw_core..Tr",
        "write",
        2,
    );
    let mut both = funcs.clone();
    both.push((other, f(&[&entry(2048), &RETW_N])));
    assert_eq!(
        scan(&both, "frames-in paperpaw_core::native_host x\n").len(),
        1
    );
}

#[test]
fn functions_with_no_crate_are_held_with_every_frames_in_line() {
    // An impl on `&T` and one on `[T]`: no crate names them.
    let a = method("$RF$T", "core..fmt..Debug", "fmt", 3);
    let b = method("$u5b$T$u5d$", "core..fmt..Debug", "fmt", 4);
    let funcs = root_and(vec![
        (a, f(&[&entry(2048), &RETW_N])),
        (b, f(&[&entry(64), &RETW_N])),
        (sym("paperpaw_x", "run", 5), f(&[&entry(64), &RETW_N])),
    ]);
    assert!(scan(&funcs, "").is_empty());
    let held = scan(&funcs, "frames-in paperpaw_x the crate\n");
    assert_eq!(held.len(), 1, "{held:?}");
}

#[test]
fn an_impl_on_a_reference_belongs_to_the_type_behind_it() {
    let m = method(
        "$RF$paperpaw_plugin_one..Thing",
        "core..fmt..Debug",
        "fmt",
        6,
    );
    let funcs = root_and(vec![(m, f(&[&entry(2048), &RETW_N]))]);
    let held = scan(
        &funcs,
        "frames-in paperpaw_core::native_host x\nframes-in paperpaw_plugin_* y\n",
    );
    assert!(held.iter().any(|p| p.contains("takes a frame")), "{held:?}");
}

#[test]
fn an_allow_frame_line_names_a_known_frame_and_goes_stale_when_it_is_cut() {
    let big = sym("paperpaw_x", "big", 7);
    let funcs = root_and(vec![(big, f(&[&entry(2048), &RETW_N]))]);
    let family = "frames-in paperpaw_x the crate\n";
    assert_eq!(scan(&funcs, family).len(), 1);
    let ok = scan(
        &funcs,
        &format!("{family}allow frame paperpaw_x::big 2064 todo 1\n"),
    );
    assert!(ok.is_empty(), "{ok:?}");
    // It grew past what the line allows.
    let grown = scan(
        &funcs,
        &format!("{family}allow frame paperpaw_x::big 2048 todo 1\n"),
    );
    assert_eq!(grown.len(), 1, "{grown:?}");
    // Cut under the limit: the line is stale. Named nowhere: also a problem.
    let cut = root_and(vec![(
        sym("paperpaw_x", "big", 7),
        f(&[&entry(64), &RETW_N]),
    )]);
    let stale = scan(
        &cut,
        &format!("{family}allow frame paperpaw_x::big 2064 todo 1\n"),
    );
    assert!(stale[0].contains("remove its `allow frame`"), "{stale:?}");
    let none = scan(
        &cut,
        &format!("{family}allow frame paperpaw_x::gone 2064 todo 1\n"),
    );
    assert!(none[0].contains("matches no function"), "{none:?}");
}

#[test]
fn an_entry_in_the_middle_of_a_function_is_literal_data_not_a_frame() {
    // A C function with a literal pool: the bytes of an `entry a1, 4096` between its
    // instructions (the pool's word) are not its frame, which is the first `entry`.
    let funcs = root_and(vec![(
        sym("paperpaw_x", "pool", 8),
        f(&[&entry(32), &entry(4080), &RETW_N]),
    )]);
    let held = scan(&funcs, "frames-in paperpaw_x the crate\n");
    assert!(held.is_empty(), "{held:?}");
}

#[test]
fn a_function_with_no_entry_at_its_start_is_reported_unless_named() {
    // Assembly such as the RTOS's task switch has no `entry`: its frame is not read, which must
    // not pass as "no frame" (Senior #2900 R1).
    let funcs = root_and(vec![(sym("paperpaw_x", "asm", 9), f(&[&RETW_N]))]);
    let family = "frames-in paperpaw_x the crate\n";
    let held = scan(&funcs, family);
    assert_eq!(held.len(), 1, "{held:?}");
    assert!(held[0].contains("no `entry`"), "{held:?}");
    let named = scan(
        &funcs,
        &format!("{family}allow frame paperpaw_x::asm 0 the RTOS switch\n"),
    );
    assert!(named.is_empty(), "{named:?}");
    let gone = scan(
        &funcs,
        &format!("{family}allow frame paperpaw_x::gone 0 x\n"),
    );
    assert!(
        gone.iter().any(|p| p.contains("matches no function")),
        "{gone:?}"
    );
}

#[test]
fn a_fn_nested_in_a_trait_impl_method_is_in_the_crate_of_the_impl() {
    // `<paperpaw_x::Thing as core::fmt::Debug>::fmt::helper`: the path begins with the method.
    let segment = "_$LT$paperpaw_x..Thing$u20$as$u20$core..fmt..Debug$GT$";
    let symbol = format!("_ZN{}{segment}3fmt6helper17h{:016x}E", segment.len(), 10);
    let funcs = root_and(vec![(symbol, f(&[&entry(2048), &RETW_N]))]);
    let held = scan(&funcs, "frames-in paperpaw_x the crate\n");
    assert!(
        held.len() == 1 && held[0].contains("takes a frame"),
        "{held:?}"
    );
}
