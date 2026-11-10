use super::*;
use crate::fail_alloc::sweep;

fn pairs<'a>(
    list: &'a [(&'a str, &'a str)],
) -> impl Iterator<Item = Result<(&'a str, &'a str), Error>> {
    list.iter().map(|pair| Ok(*pair))
}

#[test]
fn the_page_and_kind_are_read_from_the_handover() {
    let named = [("page", "2"), ("kind", "core.kind.time"), ("other", "x")];
    assert_eq!(
        params(pairs(&named)).unwrap(),
        (2, Some("core.kind.time".into()))
    );
    assert_eq!(params(pairs(&[])).unwrap(), (0, None));
}

#[test]
fn a_page_that_is_no_number_is_the_first_and_an_empty_kind_is_none() {
    let odd = [("page", "x"), ("kind", "")];
    assert_eq!(params(pairs(&odd)).unwrap(), (0, None));
    let negative = [("page", "-1")];
    assert_eq!(params(pairs(&negative)).unwrap(), (0, None));
}

#[test]
fn a_key_given_twice_differently_is_invalid() {
    let two = [("page", "1"), ("page", "2")];
    assert_eq!(params(pairs(&two)), Err(Error::Invalid));
    let same = [("page", "1"), ("page", "1")];
    assert_eq!(params(pairs(&same)).unwrap(), (1, None));
    let kinds = [("kind", "a"), ("kind", "b")];
    assert_eq!(params(pairs(&kinds)), Err(Error::Invalid));
}

#[test]
fn a_host_refusal_while_reading_the_params_is_passed_on() {
    let refused = [Ok(("page", "1")), Err(Error::Busy)];
    assert_eq!(params(refused.into_iter()), Err(Error::Busy));
}

#[test]
fn every_allocation_of_the_params_may_be_refused() {
    sweep(|| {
        let named = [("page", "2"), ("kind", "core.kind.time")];
        params(pairs(&named)).map(drop)
    });
}
