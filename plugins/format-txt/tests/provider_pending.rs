//! What the provider cannot do until other parts land. Each case names what is missing.

#[test]
#[ignore = "SDK: no format.spec declaration builder, so the provider cannot be declared (Services)"]
fn the_format_is_declared_with_its_extensions() {}

#[test]
#[ignore = "Host #417: the fmt.call adapter is not wired to a loaded plugin; activation is Core #944"]
fn the_host_opens_a_text_book_through_the_provider() {}

#[test]
#[ignore = "Kernel #385: core.format input_rows (fmt.option after fmt.call) is not in the schema"]
fn options_reach_the_provider() {}

#[test]
#[ignore = "LIB/Host: root 3 has no file name; the library names an untitled book by its file"]
fn an_untitled_book_is_named_by_its_file() {}
