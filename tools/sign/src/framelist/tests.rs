use super::*;

fn frame(path: &str, bytes: Option<u32>) -> Frame {
    Frame {
        address: 0,
        bytes,
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
