//! What the provider cannot do until other parts land. Each case names what is missing.

#[test]
#[ignore = "Host #417: the fmt.call adapter is not wired to a loaded plugin; activation is Core #944"]
fn the_host_opens_a_book_through_the_provider() {}

#[test]
#[ignore = "LIB/Host: root 3 has no file name; the library names an untitled book by its file"]
fn an_untitled_book_is_named_by_its_file() {}
