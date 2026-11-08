# Format plugins (EPUB, HTML, Markdown, FB2, CBZ) on interface 7: reading on a full heap

Status: the shared reading code behind the format plugins answers a refused allocation, it never
aborts.

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
