//! What the provider cannot do until other parts land. Each case names what is missing.

#[test]
#[ignore = "Host #417: the fmt.call adapter is not wired to a loaded plugin; activation is Core #944"]
fn the_host_opens_a_book_through_the_provider() {}

#[test]
#[ignore = "Host: op 3 prepare is called again and again from the host's main loop with a 50 ms budget; a record's time on the X4 is not measured"]
fn a_text_record_is_converted_within_the_preparation_budget() {}

#[test]
#[ignore = "Host/LIB: root 3 has no file name; the library names an untitled book by its file (interface 6 used the file name)"]
fn an_untitled_book_is_named_by_its_file() {}

#[test]
#[ignore = "Kernel (draft 29 doc_state): a status carries no message; interface 6 said why (DRM, Huffman, KF8) in a sentence the reader showed"]
fn a_refused_book_says_why() {}

#[test]
#[ignore = "Host: two documents of one conversion open at once would share a cache that one of them is still making; the host's pinning rule must keep them apart (document.md 2.1)"]
fn two_documents_of_one_book_do_not_share_a_cache_being_made() {}

#[test]
#[ignore = "SDK: GrayWriter::start / ColorWriter allocate infallibly (Kernel: write the header from a stack array), so decoding a picture is not in the refusal sweep"]
fn decoding_a_picture_survives_refused_allocations() {}
