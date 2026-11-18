# Interface-7 port

Version 1.1.2. Native plugin: `src/native_plugin.rs` (`NativeKeyboard`: the dialog of text
questions on the slot `core.keyboard`, and the screen that chooses the layouts), what typing is
without the host in `src/native_model.rs` (text, shift and its lock, automatic capitals, rows of
keys and where they lie, focus moves, variants), drawing as owned `Paint` operations in
`src/native_paint.rs`, layouts for the native plugin in `layouts::chosen_for`. `NATIVE_ENTRY`
(feature `linked`), declarations in `declarations.rs` built into the image by `build.rs`,
metadata in `native/` (regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def
--input native/metadata.json --bytes native/metadata.bin --json native/metadata.inspected.json`,
likewise `metadata.host.*`; services `core.device`, `core.registry`, `core.ui`). Drawing uses
`paperpaw-ui-kit`. The interface-6 plugin is removed (see below); no shim stays.

## Ported

- The dialog: the host opens the screen for a text question (hand-over `core.request`); it reads
  the question (`ui_question`: title, multiline, secret, mode, live) and the starting text (op 10),
  types on the keys, and answers (op 9 in parts of at most 3,072 bytes, op 11): *OK* or held
  Back = answered, Back = cancelled, live typing as outcome 2 (a refusal of a live update
  stops the live text, the final answer is whole).
- Keys as before: letters, symbols, digits (number mode), shift (twice in a row locks), delete
  (long press clears; Korean jamo are taken back one at a time), space, line break (several
  lines), layout switch, automatic capitals (plain text only), variants on a long press (with
  buttons: long confirm), German ä ö ü ß on their letters, the umlaut keys setting, key shape and
  size, the button focus and its column memory, the text field (end of the text with a cursor, the
  last lines of several lines), the hint of letters held or of how typing ends.
- *Paste* (clipboard, op 19) and *Phone* (a nested text question on `core.remote-keyboard`, op 8,
  with the text so far; never for a password; shown where the device has a network and the slot is
  filled). A password shows dots, makes the instance secret and holds the edges
  (`command.screen_policy`), keeps only Sleep, refresh and night mode, and is never put on the
  clipboard or sent to the phone.
- The layouts chooser (*On (2)* shows the place in switching; *Automatic* stands for itself) over
  the setting `keyboard.layouts`; the five settings, the group, the icon, the first-run step, the
  held-Back action `keyboard.done` bound to `long.back` on `keyboard.screen`, and the need of
  *Key shape* for a touch screen.
- Text, titles and en/de catalogs are unchanged.
- The interface-6 plugin is removed: `Keyboard`, `KeyboardScreen`, `rows_for` (the interface-6 form;
  `try_rows_for` stays), `draw.rs`, `choose.rs`, the interface-6 point reading of added layouts
  (`parse`, `added`) and the suites `typing.rs`, `presses.rs`, `multiline.rs`, `long_text.rs` and
  `size.rs`, with the app's `.plugin(Keyboard)` wiring (the keyboard does not show in the app until
  the host activates native plugins) and the desktop golden `boox-keyboard`. The wifi and store tests
  open a test-only stand-in for the dialog (`plugins/test-support/keyboard_dialog.rs`). Parity tests
  against the interface-6 registration became image-versus-builder and host-admission tests
  (`tests/abi7_declarations.rs`); the reading of added layouts is tested against stated results, not
  against the old `parse`. The test that held the script packs' `keyboard.layouts` lines
  (`tools/plugin-files/keyboards`) to the old reading is ported to `native_layouts`
  (`the_layouts_of_the_script_packs_are_taken_whole`, now with the Korean pack too): the fields
  of the packs' lines are read by `from_fields`; the packs still bring interface-6 lines, and a
  pack that adds layouts through the stage is a later step.

## Tests

`native_model`: layout of the keyboard inside the screen (touch and buttons), shift and its lock,
automatic capitals, cut text, paste filters, layers and layouts, focus moves, variants, German
holds, and allocation-refusal sweeps. The image is what the declaration builders make, the host admits
the screens, linked entry, construction in the kernel's registered scope. `tests/acceptance_pending.rs`: ignored cases that
name what they wait for. No desktop golden: the dialog needs host input (H1/H2/H4).

The interface-6 suites that drove the `Keyboard` screen through a kernel are replaced by Board-level
tests, the same inputs and the same expected values (`src/native_model_tests.rs`: sizes, presses,
multiline and long text; `src/native_model_typing_tests.rs`: typing, variants, focus moves, layout
keys; `src/native_model_layout_tests.rs`: text from a phone, the clipboard and outside, added
layouts, Korean, a huge layout), by `tests/native_translations.rs` (the catalogs' completeness) and
by ignored acceptance cases for what needs the host. The numbers of the screens come from a `Look`
that follows the stock style (`native_model_tests::look`), not from a font, so heights are the
native layout's and the relations the old tests asserted (sizes, finger floor, half the screen,
30/40/50 %) are what is pinned.

### Interface-6 suites replaced

| Old test (suite `tests/…`) | Now |
|---|---|
| size: `larger_sizes_give_higher_keys_and_never_below_a_finger`, `round_keys_take_about_30_40_and_50_percent`, `the_keys_use_nearly_the_whole_width`, `with_buttons_the_size_makes_keys_higher_in_whole_rows`, `the_floor_wins_where_a_size_cannot_be_reached`, `key_widths_on_the_x4` (ignored, measures) | the same names in `native_model_tests` |
| size: `medium_is_the_keyboard_as_before` | `native_model_tests::medium_is_the_keyboard_as_before` (Medium equals the size a stray setting stands for; the interface-6 golden is gone) |
| presses: `texts_take_no_more_presses_than_before` | `native_model_tests` (same name), plus `the_press_counting_moves_as_the_board_does`: the counting model is held to `move_focus` |
| multiline: `lines_come_back_as_typed`, `a_plain_question_gets_one_line`, `more_than_a_line_of_text_may_be_typed`, `the_line_break_key_adds_a_line_break` | the same names in `native_model_tests` (text from the phone is `replace_pasted`) |
| multiline: `the_panel_leaves_the_status_line_free_on_every_screen` | `native_model_tests` (same name; the plain field's panel leaves the status line free too now) |
| multiline: `the_field_shows_the_lines_as_typed`, `a_long_line_wraps_in_the_field`, `the_field_scrolls_to_the_end_of_the_text`, `a_plain_field_stays_one_line` | ignored case `keyboard-field-lines` (drawing needs the fonts) |
| long_text: `a_long_address_comes_back_whole`, `text_past_the_limit_is_cut` | the same names in `native_model_tests` |
| typing: letters by buttons and by touch, shift and its lock, automatic capitals, symbols/digits/space, number mode, delete (empty, whole characters), the longest text, an initial text longer than allowed | `native_model_typing_tests` (same names) |
| typing: `holding_delete_clears_the_text` | `native_model_typing_tests` (`Board::clear`); the gesture is in `keyboard-touch-holds` |
| typing: `variants_by_touch_and_by_buttons` | `variants_by_touch`, `variants_by_buttons`; Back and a hold closing the variants are in `keyboard-finish-cancel` |
| typing: `holding_a_letter_at_the_edge_shows_its_variants`, `a_long_press_on_a_letter_offers_its_capital_first`, `holding_select_works_while_typing_a_password` | same names in `native_model_typing_tests` (the variants and where they lie); the edge strip is in `keyboard-touch-holds` |
| typing: `back_held_finishes_and_keeps_the_text`, `a_long_press_on_confirm_off_a_letter_finishes` (here `a_long_press_off_a_letter_has_no_variants` pins the board's half), `holding_select_without_a_focus_finishes`, `back_cancels_without_an_answer` | ignored case `keyboard-finish-cancel` |
| typing: `the_answer_carries_the_question_context`, `a_live_question_is_answered_on_each_change`, `a_question_not_live_is_answered_once`, the next keyboard keeping the layout | ignored case `keyboard-answers` |
| typing: `with_buttons_alone_the_focus_starts_on_phone`, `right_from_phone_goes_to_the_first_key`, `buttons_move_round_the_rows`, `up_and_down_keep_the_column` | same names in `native_model_typing_tests` |
| typing: `the_layout_key_switches_and_the_next_keyboard_keeps_it`, `the_layout_key_by_buttons`, `a_tap_takes_the_hint_of_held_letters_with_the_focus`, `umlaut_keys_give_german_keys_for_them` | `the_layout_key_switches_by_touch`, `the_layout_key_by_buttons`, same names for the others (the redraw of the field and the kept layout are in `keyboard-field-lines` and `keyboard-answers`) |
| typing: `every_letter_of_every_drawable_layout_types_itself`, `unknown_layouts_fall_back_to_the_language` | same names (with fonts that draw Latin only, and with fonts that draw all) |
| typing: `text_from_a_phone_replaces_the_text_as_one_line`, `…_is_cut_to_the_longest_allowed`, `…_keeps_only_digits_for_numbers`, `text_from_a_phone_is_always_one_short_line` (proptest), `letters_typed_by_touch_come_back` (proptest), `paste_inserts_the_clipboard_as_one_line`, `paste_by_buttons_from_above_the_first_row`, `paste_keeps_only_digits_for_numbers`, `text_typed_from_outside_goes_in_as_typed` | same names in `native_model_layout_tests`; the clipboard, the nested question and event 109 are in `keyboard-paste-phone` and `keyboard-type-text` |
| typing: `a_layout_another_plugin_adds_is_typed_with`, `korean_jamo_join_to_syllables`, `a_layout_far_too_big_stays_on_the_screen` | same names in `native_model_layout_tests` (the stage's wiring is `keyboard-added-layouts`) |
| typing: `layouts_named_as_others_are_left_out` | `native_layouts::built_in_ids_and_repeats_are_not_added_again` |
| typing: `layouts_are_chosen_in_order_and_automatic_stands_alone` | ignored case `keyboard-layout-chooser` |
| typing: `other_actions_pass_before_a_key_has_the_focus` | ignored case `keyboard-other-actions` |
| typing: `the_header_and_the_field_buttons_follow_the_interface_direction` | ignored case `keyboard-rtl` (not mirrored yet, see below) |
| lib: `translations_are_complete` | `tests/native_translations.rs` |
| lib: `several_layouts_get_a_key_to_switch` | `native_model_tests` (same name) |
| layouts: `the_layouts_of_the_script_packs_are_taken_whole` | `native_layouts` (same name) |

**Differences found in the port.** Text of two line-break characters (CR LF) from a phone or the
clipboard is one space (or one line break) in the native `pasted`; the interface-6 keyboard made
two spaces of it in a plain question. The plain field's panel leaves the status line free (the
interface-6 one filled the screen from the top turned on its side). The native paint does not
mirror for right to left yet, which the interface-6 keyboard did (the case `keyboard-rtl`).
The X4 Pro key shares are pinned at 30/36/45 % of the width where interface 6 gave 30/40/50 %:
32-40 px of 800, more than a few pixels; measure with the real fonts at native activation and
adjust. `KeyboardDialog` (app tests of wifi and store) duplicates the `TestKeyboard` stand-in of the
home, menu, picture-home and settings tests; unify them later.

**Activation list.** Script packs' `points/keyboard.layouts.txt` lines are read by nothing until
the stage path (`keyboard.layouts@1`) is wired, yet `docs/data-files.md` still lists them: update
that page at activation. The app test `partial_redraw::keyboard` is ignored until then (todo 380).

- Layouts other plugins add (interface 6 point `keyboard.layouts`, now the stage
  `keyboard.layouts@1`): the schema is `stages/keyboard.layouts@1.def` (input `layouts.ask`: the UI
  language; reply: up to 3 `layouts.layout` records of `id`, `code`, `letters`, `variants`, at most
  3,200 bytes), the codec `src/stages.rs` (generated: `python3 tools/stage-codec.py stages
  src/stages.rs --plugin keyboard`), the declaration `declarations::stages`, and the reading
  `src/native_layouts.rs`. The dialog and the chooser run the stage (`stage.run`, metadata gains it)
  and take the rules of the old point: the id is also the translation key of the name (a layout
  without one shows as its id), at most 4 visible characters of code, 6 rows of 16 letters, 16
  variants a letter; an id that names a built-in layout, *Automatic* or one added before is left
  out. Tests compare the reading with the old `parse` on samples and edge cases, sweep every
  allocation, round-trip the largest answer through the codec, and pin the stage's declaration and
  bounds to the schema. **Differences:** a subscriber answers records, not lines (the variants are
  one field of `e=éèë` pairs, at most 512 bytes; a plugin gives up to 3 layouts a run and 16 are
  taken in all); where the host does not run stages (nothing subscribed, or not wired) there are no
  added layouts and the built-in ones work as before.

## Not ported yet (explicit; each is an ignored acceptance case)

- Text from a hardware keyboard or a remote control (interface 6 `type-text`) is taken as
  `event.text_typed` (109, `Event::TextTyped`): added to the field as if typed on the keys
  (characters that are no text left out, digits only for numbers), `done` confirms. It reaches the
  plugin only while a text question is on top (the host drops it otherwise, and the plugin ignores
  it when no dialog is on top). What needs the host is the dialog and the dispatch (H1/H4): the
  ignored case `keyboard-type-text`.
- Layouts and variants the fonts cannot draw (interface 6 `has_char`) are left out through
  `Font::covers` (`text.covers`, slot 39): built-in and added layouts whose letters (with their
  capitals) the fonts lack are not on the keyboard or in the chooser, and variants of added
  layouts they lack are dropped. Letters are asked in pieces of at most 256 bytes. A host that
  cannot answer (no slot, no font) leaves every layout offered, as before. Tested on what the
  query answers (`layouts.rs` tests); the host's own answer needs a font to ask.
- Right-to-left mirroring (title, hint, buttons, cursor; `ar` is a regression against Beta 3 until it is
  done): the kit's style has a direction now (`Style::rtl`, `flip`, `flip_x`, `text::label_in`); the keyboard
  does not use it yet: the ignored case `keyboard-rtl`.
- Wrapped lines of several lines are broken at spaces and a run of spaces is shown as one.
- Peak memory on the X4 is not measured (device gate open). The keyboard is built into the
  firmware, so there is no `.ppn` size to record.
- Repaint memory: `native_paint::lines` wraps the whole text on every repaint (the wrapped lines
  and the joined candidates copy it; up to about 4 bytes per text byte, so up to about 8 KiB for
  2,000 characters and 24 KiB for 6,000 characters of several lines) and the password dots take 3
  bytes per character. Bounded by `MAX_CHARS` / `MAX_CHARS_MULTILINE`, fallible, freed after each
  repaint; a reused, line-by-line wrap buffer needs a streaming wrap in the kit and is left for
  the X4 memory measurement.
