use super::*;
use crate::frames::path;

fn frame(path: &str, bytes: Option<u32>) -> Frame {
    Frame {
        address: 0,
        bytes,
        at_least: bytes.unwrap_or(0),
        path: path.to_string(),
    }
}

#[test]
fn paths_come_from_legacy_and_v0_names() {
    assert_eq!(
        path("_RNCNvNtCsljJlXh7PErD_15paperpaw_xteink5crash5panics0_0B5_"),
        "paperpaw_xteink::crash::panic"
    );
    assert_eq!(
        path("_RNvCsdtSMxSA38aw_7___rustc17rust_begin_unwind"),
        "__rustc::rust_begin_unwind"
    );
    assert_eq!(
        path("_ZN4core3fmt5write17h0123456789abcdefE"),
        "core::fmt::write"
    );
    // Not a Rust name: as it is.
    assert_eq!(path("ExceptionHandler"), "ExceptionHandler");
}

#[test]
fn a_frame_over_its_budget_fails_and_one_within_does_not() {
    let all = [frame("a::panic", Some(300)), frame("b::cap", Some(200))];
    assert_eq!(
        check(&all, "frame 300 a::panic\nframe 200 b::cap").unwrap(),
        Vec::<String>::new()
    );
    let p = check(&all, "frame 299 a::panic").unwrap();
    assert_eq!(p.len(), 1);
    assert!(p[0].contains("300") && p[0].contains("299"), "{p:?}");
}

#[test]
fn a_name_that_matches_nothing_fails() {
    let all = [frame("a::panic", Some(1))];
    let p = check(&all, "frame 999 a::pannic").unwrap();
    assert!(p[0].contains("no function matches"), "{p:?}");
}

#[test]
fn the_largest_of_several_matches_counts_and_an_unsized_one_fails() {
    let all = [frame("x::f", Some(10)), frame("y::x::f", Some(90))];
    assert!(check(&all, "frame 80 x::f").unwrap()[0].contains("90"));
    let all = [frame("x::f", Some(10)), frame("y::x::f", None)];
    assert!(check(&all, "frame 80 x::f").unwrap()[0].contains("does not size"));
}

#[test]
fn a_chain_adds_up_the_largest_frame_of_each_name() {
    let all = [
        frame("a::panic", Some(1000)),
        frame("b::cap", Some(600)),
        frame("c::fmt", Some(300)),
    ];
    assert!(
        check(&all, "chain 1900 a::panic b::cap c::fmt")
            .unwrap()
            .is_empty()
    );
    let p = check(&all, "chain 1899 a::panic b::cap c::fmt").unwrap();
    assert!(p[0].contains("1900") && p[0].contains("1899"), "{p:?}");
    // A missing name is its own problem, not a sum over what is left.
    let p = check(&all, "chain 5000 a::panic nowhere").unwrap();
    assert_eq!(p.len(), 1);
    assert!(p[0].contains("nowhere"));
}

#[test]
fn the_budget_file_is_read_with_comments_and_refuses_nonsense() {
    let all = [frame("a", Some(5))];
    assert!(
        check(&all, "# note\n\nframe 10 a  # trailing\n")
            .unwrap()
            .is_empty()
    );
    assert!(check(&all, "frame x a").is_err());
    assert!(check(&all, "frame 10").is_err());
    assert!(check(&all, "chain 10").is_err());
    assert!(check(&all, "loop 10 a").is_err());
}

#[test]
fn a_name_is_a_whole_path_or_its_tail_not_a_part_of_a_segment() {
    // A renamed handler does not pass through a helper that holds its name.
    let all = [frame("a::handler_helper", Some(10))];
    let p = check(&all, "frame 100 handler").unwrap();
    assert!(p[0].contains("no function matches"), "{p:?}");
    let all = [frame("a::handler", Some(10)), frame("handler", Some(20))];
    assert_eq!(check(&all, "frame 20 handler").unwrap().len(), 0);
    assert!(check(&all, "frame 19 handler").unwrap()[0].contains("20"));
    // Not a tail of a longer segment, nor a closure inside it.
    assert!(!names("a::my_handler", "handler"));
    assert!(!names("a::handler::{{closure}}", "handler"));
    assert!(names("a::b::handler", "b::handler"));
}

#[test]
fn a_chain_of_frames_that_would_overflow_is_over_its_budget_not_a_panic() {
    let all = [frame("a::f", Some(u32::MAX)), frame("b::g", Some(10))];
    let p = check(&all, "chain 100 a::f b::g").unwrap();
    assert_eq!(p.len(), 1);
    // Not even a budget of u32::MAX lets a sum that wrapped through.
    let p = check(&all, "chain 4294967295 a::f b::g").unwrap();
    assert_eq!(p.len(), 1);
    assert!(p[0].contains("more than"), "{p:?}");
}

#[test]
fn a_chain_family_counts_a_function_and_the_closures_inside_it() {
    // The handler, and closures named in the several ways a build may name
    // them (the handler's own path, `::0`, `::_`).
    let all = [
        frame("x::crash::handle", Some(912)),
        frame("x::crash::handle", Some(272)),
        frame("x::crash::handle::0", Some(272)),
        frame("x::crash::handle::_", Some(200)),
        frame("x::crash::handle_helper", Some(5000)),
        frame("c::fmt", Some(300)),
    ];
    // The two largest of the family: 912 + 272; `handle_helper` is no part of it.
    assert!(
        check(&all, "chain 1484 crash::handle*2 c::fmt")
            .unwrap()
            .is_empty()
    );
    let p = check(&all, "chain 1483 crash::handle*2 c::fmt").unwrap();
    assert!(p[0].contains("1484") && p[0].contains("1483"), "{p:?}");
    // One counts only the handler itself.
    assert!(
        check(&all, "chain 1212 crash::handle*1 c::fmt")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_unsized_or_oversized_closure_fails_its_family() {
    // Code Reviewer's probe on #254: a closure the code does not size, or
    // one far larger than expected, must not pass through a name that misses it.
    let base = [
        frame("x::crash::handle", Some(912)),
        frame("c::fmt", Some(300)),
    ];
    let mut unsized_one = base.to_vec();
    unsized_one.push(frame("x::crash::handle::0", None));
    let p = check(&unsized_one, "chain 5000 crash::handle*2 c::fmt").unwrap();
    assert!(p[0].contains("does not size"), "{p:?}");
    let mut big = base.to_vec();
    big.push(frame("x::crash::handle::_", Some(4000)));
    let p = check(&big, "chain 1700 crash::handle*2 c::fmt").unwrap();
    assert!(p[0].contains("over its 1700"), "{p:?}");
    // A family with no member is a rename, not a pass.
    let p = check(&base, "chain 5000 nothing::here*2").unwrap();
    assert!(p[0].contains("no function matches"), "{p:?}");
}

#[test]
fn a_malformed_family_word_is_refused_when_the_budget_is_read() {
    let all = [frame("a::f", Some(1))];
    assert!(check(&all, "chain 100 a::f*0").is_err());
    assert!(check(&all, "chain 100 a::f*x").is_err());
    assert!(check(&all, "chain 100 a::f*").is_err());
    assert!(check(&all, "chain 100 *2").is_err());
    // A sum that overflows is over any budget, not a panic.
    let all = [frame("a::f", Some(u32::MAX)), frame("a::f::0", Some(10))];
    let p = check(&all, "chain 4294967295 a::f*2").unwrap();
    assert_eq!(p.len(), 1);
}

#[test]
fn each_holds_every_function_of_a_family_to_the_bound() {
    let all = [
        frame("c::native_host::shell::a", Some(2000)),
        frame("c::native_host::shell::b::x::X4Services::m", Some(1712)),
        frame("c::native_host::flat::invoke", Some(80)),
        frame("c::other::big", Some(9000)),
    ];
    // All within 2,047; the function outside the family is not looked at.
    assert!(
        check(&all, "each 2047 native_host::shell")
            .unwrap()
            .is_empty()
    );
    assert!(check(&all, "each 2047 native_host").unwrap().is_empty());
    // One over: named, with its frame.
    let p = check(&all, "each 1999 native_host::shell").unwrap();
    assert_eq!(p.len(), 1);
    assert!(p[0].contains("shell::a") && p[0].contains("2000"), "{p:?}");
    // The bound is inclusive.
    assert!(
        check(&all, "each 2000 native_host::shell")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn each_fails_on_an_unsized_member_and_on_a_family_that_matches_nothing() {
    let all = [
        frame("c::native_host::shell::a", Some(10)),
        frame("c::native_host::shell::b", None),
    ];
    let p = check(&all, "each 2047 native_host::shell").unwrap();
    assert_eq!(p.len(), 1);
    assert!(p[0].contains("does not size"), "{p:?}");
    let p = check(&all, "each 2047 native_host::nothing").unwrap();
    assert!(p[0].contains("no function matches"), "{p:?}");
    // `shell_helper` is no part of the family `shell`.
    let all = [frame("c::native_host::shell_helper", Some(5000))];
    let p = check(&all, "each 2047 native_host::shell").unwrap();
    assert!(p[0].contains("no function matches"), "{p:?}");
    assert!(check(&all, "each x native_host").is_err());
    assert!(check(&all, "each 10").is_err());
}

#[test]
fn each_leaves_out_the_families_after_a_minus() {
    let all = [
        frame("c::native_host::a", Some(100)),
        frame("c::native_host::shell::Error::core::fmt::Debug::fmt", None),
        frame("c::native_host::big", Some(5000)),
    ];
    // The unsized one and the big one both fail the plain rule.
    assert_eq!(check(&all, "each 2047 native_host").unwrap().len(), 2);
    // Left out by family, they pass: only what is named is excluded.
    assert!(
        check(&all, "each 2047 native_host -core::fmt::Debug::fmt -big")
            .unwrap()
            .is_empty()
    );
    let p = check(&all, "each 2047 native_host -core::fmt::Debug::fmt").unwrap();
    assert_eq!(p.len(), 1);
    assert!(p[0].contains("big"), "{p:?}");
    // A bare `-`, or no family, is a mistake in the budget.
    assert!(check(&all, "each 2047 native_host -").is_err());
    assert!(check(&all, "each 2047 -x").is_err());
}
