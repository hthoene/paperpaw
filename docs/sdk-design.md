# Designing the plugin SDK

Every plugin builds on the SDK: `crates/paperpaw-sdk` and the crates
whose types cross with it, as `crates/paperpaw-sdk/interface/crates.txt`
lists them. Once a release is out, a change to them can break community
plugins. So the SDK is designed to grow without breaking, and these
rules say how.

Reviews check every rule. Some are also checked by tests in
`tools/checks/tests/sdk.rs`, which run with the workspace's tests. What
such a check found before it existed is listed in `tools/checks/sdk/`.
These lists may only get shorter: a new finding fails, and a fixed one
must leave its list.

## Types

1. **What may grow is `#[non_exhaustive]`, with a constructor.** This
   holds for every public enum and for every struct with a public field.
   A plugin then cannot build the value from its fields or match every
   variant, so a new field or variant breaks nothing. Check:
   `growing_types_are_non_exhaustive`.

   ```rust
   // Good: a field added later breaks no plugin.
   #[non_exhaustive]
   pub struct HttpReply { pub status: u16, pub body: Vec<u8> }
   impl HttpReply {
       pub fn new(status: u16, body: Vec<u8>) -> Self { Self { status, body } }
   }

   // Bad: a plugin writes `HttpReply { status, body }`.
   pub struct HttpReply { pub status: u16, pub body: Vec<u8> }
   ```

   A type stays exhaustive only where a new variant must break plugins,
   as `Handled` does. It is listed with that reason in
   `tools/checks/sdk/exhaustive.txt`.

2. **Results and errors are typed.** An error is an enum that says what
   went wrong and can grow, never a `bool`, `()` or `String`.

   ```rust
   pub fn save(&self) -> Result<(), data::Error> // good
   pub fn save(&self) -> bool                    // bad: what went wrong?
   ```

3. **Values have types.** A place is a `Point` or `Rect`, a day a
   `Date`, a moment a `UnixTime`. Never a tuple, and never a bare `i64`
   whose unit the reader has to guess.

## The API's shape

4. **Each item has one path and its own name.** An item is reachable by
   one path only. Two types called `Look`, or four called `Hint`, are a
   bug.

5. **What only the kernel uses is behind the `kernel` feature.** This
   covers what only the kernel, a shell or an installer calls, such as
   writing the catalog, setting work states or making a `ScreenKey`.
   Such an item is `#[cfg(feature = "kernel")]`, never plain `pub`.
   `#[doc(hidden)]` does not hide it from plugins. Only what an SDK
   macro expands to may stay hidden, listed with that reason. Check:
   `hidden_items_are_behind_the_kernel_feature`.

   ```rust
   #[cfg(feature = "kernel")]
   pub fn set_works(&mut self, works: Vec<(Id, String, WorkState)>) // good

   #[doc(hidden)]
   pub fn set_works(&mut self, works: Vec<(Id, String, WorkState)>) // bad
   ```

6. **No deprecation outlives a release.** What goes is `#[deprecated]`
   while plugins move on, and it is gone before the next release, which
   raises the plugin interface for it
   ([ADR 0026](adr/0026-plugin-interface-version.md)). Check:
   `no_deprecated_item_reaches_a_release`, once the workspace's version
   is a release.

## Arguments and signatures

7. **Text and ids are taken as `impl Into`.** Text is
   `impl Into<Cow<'static, str>>` and an id `impl Into<Id>`. What is
   only read is `AsRef<str>`.

   ```rust
   pub fn title(mut self, title: impl Into<Cow<'static, str>>) -> Self // good
   pub fn title(mut self, title: String) -> Self                        // bad
   ```

8. **No positional `bool` arguments.** A builder method or an enum says
   what is meant.

   ```rust
   ask::confirm(question, &REPLY).only_yes() // good
   ask::confirm(question, &REPLY, true)      // bad: true for what?
   ```

9. **Handlers get a context that can grow.** No signature takes a bare
   `&mut Vec<Command>`. A screen's handler gets `Cx`, and every other
   handler gets `Commands`. Both keep their fields private, so they can
   carry more later, such as which plugin runs, its limits or small
   helpers, through methods. Check: `handlers_take_no_vector_of_commands`.

   ```rust
   fn step(env: &Env<'_>, params: &Params, out: &mut Commands) -> Step     // good
   fn step(env: &Env<'_>, params: &Params, out: &mut Vec<Command>) -> Step // bad
   ```

10. **What runs often does not allocate.** A callback that runs on every
    page or frame returns plain values. A signature is a `u64`, a label
    a `Cow<'static, str>`, never a fresh `String` each time.

## Components and looks

11. **Components own their data.** A list, chips or a header keeps its
    labels and its length. Its `draw` and `hit` do not take them again.

12. **Colours and sizes come from the theme.** A plugin writes
    `style.ink(Ink::NOTE)` and `style.margin()`, never a literal gray or a
    number of pixels.

## Data formats

13. **Lists in data files follow one rule.** Every list is separated the
    same way, and a separator inside a value is escaped the same way
    (`\,`).

## Changing the SDK

14. **Every change passes the interface gate.** Run
    `python3 tools/interface.py`, then
    `cargo test -p paperpaw-sdk --test interface`
    ([ADR 0026](adr/0026-plugin-interface-version.md)). The test says
    whether the change raises `native::INTERFACE`. The same change
    brings `api.txt`, `docs/plugins.md` and the CHANGELOG up to date.
15. **Looks are numbers first.** What a theme can say as numbers (a tone
    curve, a palette) is a setting; code (a painter, a hook) is for what
    numbers cannot say.
16. **A crossing struct has a typed spare, not a promise.** A struct that a
    plugin builds and the kernel (or another plugin) reads has private spare
    fields, zero now, and getters later; kinds are constants of an open type.

15. **A new API is proposed before it is built.** Send a short sketch:
    the types and one call site. Write it out across the plugins only
    after that. An API is easy to add and hard to take back.
