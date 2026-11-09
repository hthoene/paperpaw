//! What the provider cannot do until other parts land. Each case names what is missing.

#[test]
#[ignore = "SDK: no list of root 3 (the book's folder) in native storage, so the pages after the opened picture are not found (Services)"]
fn the_pages_after_the_opened_picture_are_listed_in_the_host() {}

#[test]
#[ignore = "Host #417: the fmt.call adapter is not wired to a loaded plugin; activation is Core #944"]
fn the_host_opens_a_picture_through_the_provider() {}
