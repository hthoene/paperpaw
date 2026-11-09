# Format plugins (EPUB, HTML, Markdown, FB2, CBZ) on interface 7: reading on a full heap

Status: Opening and reading Markdown, HTML, EPUB, FictionBook and comic books answers a refused
allocation, it never aborts. The refusal sweeps (`tests/allocation.rs`) cover all five (an EPUB of
two chapters, opened and read whole), and a book that converts a chapter again and finds no room part
of the way reads on as a clean run does. A
picture that finds no room to be decoded shows its alternative text for now, but its chapter is
**not marked complete** in the cache (`<n>.end` is not written), so the next opening converts it
again; one that cannot be decoded for other reasons is cached as its alternative text, as before.
Any other refused allocation fails the chapter with `OutOfMemory`. Until Kernel's decoder tells
"too big for the heap" from a refused allocation, a picture too big for the heap costs a conversion
on each opening.

- Every growth on the path of opening and reading a book (strings, lists, formatted text, the cache
  record reader) goes through `src/fallible.rs` or `try_reserve` itself. A refusal is
  `DocumentError::OutOfMemory`, which a provider maps to the format refusal. Successful parses
  give the same output as before.
- `tests/allocation.rs` refuses the n-th allocation for every n over a representative Markdown
  and HTML document, and checks: the answer is `OutOfMemory` or the same document, never a panic,
  and a retry on the same storage reads as a clean run does (nothing half-made stays).
  `list_infallible_sites` (ignored, developer aid) lists allocations that do not go through
  `try_*`.
- **Documented residual:** the one `Box<dyn Document>` per opened document, made by the format
  factory in `src/lib.rs` (`Box::new(d) as Box<dyn Document>`). The host's factory type is
  `Result<Box<dyn Document>, DocumentError>`, the SDK has no safe fallible box and a plugin crate
  holds no `unsafe`. A sweep skips that site.
- **Documented residual (SDK):** `paperpaw_sdk::ImageFile::open` / `Decoded::open` (crate
  `paperpaw-image`) copy a path with no way to refuse; the picture lookups of this crate call them.
  The sweep leaves allocations made inside `paperpaw_image` be (`in_sdk_image` in
  `tests/allocation.rs`). Follow-up: a fallible `ImageFile::try_open` (Kernel's API).
- Cache line reads (`cached::lines`) and picture lookups answer `OutOfMemory` where the caller can
  say so (conversion, block reads); lookups with no way to say it (labels, links, anchors, the
  Document trait's `Option` returns) find nothing for that call and are not cached.
