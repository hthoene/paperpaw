# Interface-7 port

Version 1.0.7. Native plugin: `src/native_plugin.rs` (`NativeSettings`: seven screens, questions,
number editing), passwords and keys in `src/native_secrets.rs`, registry reads in `src/native_data.rs`, what the pages are made of without the host
in `src/native_model.rs` (visibility, groups, sections, stepping, units, search words, theme in use).
`NATIVE_ENTRY` (feature `linked`), declarations in `declarations.rs` built into the image by
`build.rs`, metadata in `native/` (regenerate with `python3 tools/abi-metadata.py --schema
abi/abi.def --input native/metadata.json --bytes native/metadata.bin --json
native/metadata.inspected.json`, likewise `metadata.host.*`). Drawing uses `paperpaw-ui-kit`.
The interface-6 `SettingsPlugin` and its suites stay: `paperpaw-app` and other plugins' tests still
link them until the host activates native plugins; then they go together (no shim).

## Ported

- *Events 75 and 76:* the host admits `event.registry_changed` and `event.preview_changed` since #916
  (coalesced hints), so the plugin's four event subscriptions are admitted as a whole and
  `tests/hosted_construction.rs` constructs it in the real registered scope (no longer ignored).
  A registry change (and a plugins change) rebuilds what shows on top and notes the covered pages
  as stale; a preview change redraws. The widget preview above a group's settings stays blocked on
  the registry's `previews` read (SDK accessor) and the host's slot 35 (`settings-group-preview`).

- *Passwords and keys* (`src/native_secrets.rs`, screens `settings.secrets` and `settings.secret`, the
  action `settings.secrets.open`, its entry in `core.settings-pages` at order 95 and the icon `key`,
  as in interface 6; parity in `tests/abi7_declarations.rs`). On `core.secrets` with the feature
  `uses.secrets` (not required: the device may not let the settings list; the list is then empty and
  the main page's row stays hidden): the list of every plugin's secrets by label (the owner's text for
  a label that is a catalogue key, else the label as it is, else the name), the value never shown, in
  the host's order (at most 128); choosing one offers *Type anew* (only for a changeable one; a
  masked question, the answer trimmed, an empty one ignored, over 1,024 bytes told with
  `settings.secret.too-long`) and *Remove* (asked first, *Remove*/*Keep*), after which the screen
  goes back and the pages that show secrets are made anew. A secret that is not kept (any more)
  offers only *Remove*, which is nothing, and goes back. The main page's row shows only while a secret
  is kept (the service is asked when the page is built). **Differences from interface 6:** a refused
  save (card away, secret gone, not changeable) is told with `settings.secret.failed`, it was silent;
  an owner of a secret is named by its plugin id and the secret's screen is opened with the hand-over
  `id` = `plugin:name` (a name may hold `:`; an owner's id never does); the host, not the plugin,
  checks that a replaced secret exists and is changeable. What is typed is read through buffers that
  are wiped, and the text copy is wiped after the host has it. The calls that use the service
  (`keep`, `remove`) are run over the scripted host (`tests/scripted_secrets.rs`): the question for
  a removal and its yes/no, a value typed anew (kept byte for byte as typed, white space trimmed; nothing typed leaves the secret), and
  a secret no longer kept removed without asking. The list runs page after page and every refusal of
  the host is swept; the rows' allocations are swept. That the typed and read buffers are wiped is not asserted by a test: they are freed straight after (`wipe_typed`, `mem::wipe`), so no memory can be read back without undefined behaviour; the wiping calls are on every path (`read_typed`, `change`, `wipe_typed`) and the kit's own `mem::wipe` is unit-tested. Not run: drawing, moving the focus and going
  back, which are screen commands the scripted host refuses (BLOCKED H1).
  Secret *text settings* stay out of the pages as in interface 6 (they are secrets, listed here).

- Main page: groups with visible settings (by order, then name; widgets' own groups left out), the
  pages other plugins add to `core.settings-pages` under their section headings, *Search*, the
  advanced switch, *Reset all*, *Restart* (asks first; `CommandSender::restart`, so the
  metadata declares `uses.power` as not required: a shell without it refuses the command, and a notice says so). Group page: parts, actions plugins add to the group, the theme row in
  the appearance group, settings under their sections with value, unit and description, *More*
  (advanced settings one level down), *Reset to defaults* (asks first, leaves language, kept-on-reset
  settings and secrets, "only this book" last). Rows follow the registry's `shown`, `advanced`,
  `guarded` and `secret` flags and the developer switch.
- Numbers: confirm starts changing, left/right (or up/down, page buttons) preview the value
  (`preview_setting`), confirm keeps it, back drops it; touch screens get − and + on the row.
  Switches and choices of two change at once; longer choices open `core.choose-value`
  (`settings.choose`); text asks with the host dialog; a setting's own editor opens as declared.
- Themes: *None* (every appearance setting but size, contrast and logo back to default) and the
  `core.theme` presets through `apply_preset`; the one in use is marked.
- Search over names and descriptions (accents folded) of the listed settings and the pages.
- Slots `core.settings` and `core.choose-value`; parameters (hand-over) `id`, `more`, `focus`,
  `text`, and `scope` (a root-1 book path: the screen sets the book scope while it is open).

- *Placement pages:* the screen `settings.group` with `id` = `widgets.clock@2` shows the settings of
  its widget's group as that placement has them: values read with the placement's key (slot 5
  falls back to the widget's value, `registry.md` 10.1), a value the placement does not set itself
  counts as default (op 18 with the placement), changes are set under the placement's key, and
  *Reset to defaults* resets that placement only (`reset_setting` bit 3). No parts, actions, theme or
  preview on such a page. The chooser for a placement's choice reads the options of the setting it
  stands for. The id rules (`unplaced`, `unplaced_key`) are unit-tested and swept; the page itself
  needs the host (ignored case `settings-placement-page`).

- *Widget look:* on the appearance page a second row, *Widget look*, opens the chooser of the
  presets of kind `core.widget-look` (`settings.themes` with `mode` = `widgets`), as the themes
  row opens the themes': *None* first (every covered setting back to its default), the look in
  use marked, choosing one applies it (`apply_preset`). What a look covers is each widget's
  frame, padding, size, alignment, paper, ink, corners, frame line and shadow, and the settings of
  all widgets and the gap (`native_model::widget_look_covers`, from the registry's widgets;
  tested). A theme no longer resets the frame of all widgets (it never did in interface 6). The
  row shows only where a look exists. The chooser's drawing needs the host (`settings-widget-look`).
  **Differences and host duties:** (1) `apply_preset` takes the covered set per kind from the host
  (`registry.md` 4.3): the host's theme set must leave out `core.widgets.frame` and its look set must
  equal `widget_look_covers`, else the mark here and the result of choosing differ. (2) Beta 3
  counted a widget's `.frame` as in use only where it was written down, even at its default; here
  "in use" compares with `is_default`, so a look that names a frame at its default may be marked as
  *None*. Not asserted by a test (the host decides what `is_default` means for a named default).

## Tests

`native_model`: group membership, offered/visible levels, main-page order, sections, stepping and
ends, units, placements, search folding, theme in use. Declaration parity with the interface-6
registration, linked entry, construction in the kernel's registered scope (refused as `Unsupported`
until the host admits every declaration family, #944). No desktop golden: pages need host input
(H1/H4). `tests/acceptance_pending.rs`: refusals that never pass (pages, number preview, reset,
search, secrets, restart).

## Not ported yet (explicit)

- The preview of a group's settings (a widget composed above them, interface 6
  `env.widgets().preview`): the facade (`ui.compose`, `widget_make`) exists, but the registry does
  not say which widget previews a group (`widget.spec.previews` is not in `reg_widget`; asked of
  Kernel). Ignored case `settings-group-preview`, blocked by `reg_widget.previews` and the host's
  slot 35 (H3).
- No icons in rows, switches are shown as *On*/*Off* text, no sliders (numbers use − and +); the kit
  draws none yet. Reading-font sizes that look alike are not merged when stepping.
- A unit is the host's translated text: `{count}` is replaced, else the number comes first. Whether
  the registry's `unit` is the plural template or a finished word is for Kernel to say.
- Per-setting `core.registry` op 2 for page rows (range, unit, section, description): a page of 40
  settings makes 40 calls when it is built, search up to one per listed setting. Peak memory on the
  X4 is not measured: device gate open.
- Settings is built into the firmware (no `.ppn`), so there is no store-file size to record; the
  native packing waits for the shared 6→7 linker migration.

## Right-to-left (`ar`: a regression against Beta 3 until this is done)

Beta 3 mirrored the interface where its language is written right to left (Arabic). The UI kit now
does so for what it draws (`Style::rtl`, read from `core.language`: header, list rows, items, dialog,
chips, the sheet's pager; `Style::flip`, `flip_x` and `text::label_in` for the rest). What this plugin
draws itself is not mirrored yet, so its screens are left to right in those parts for `ar` until it
takes the kit's helpers (and, where it keeps its own keys, swaps left and right as `Chips::step` does).

## Interface-6 suites replaced (#228)

The interface-6 suites in `tests/` (`pages`, `groups`, `search`, `secrets`, `covered`, `resize`,
`frames`, `font_slider`, `common/`; 42 tests) drove the removed screens through a kernel test
harness. Each is replaced by a native test or an ignored acceptance case that names what it waits
for. Native tests: `src/native_model/tests.rs` (model), `tests/scripted_pages.rs` and
`tests/scripted_secrets.rs` (the plugin over the scripted host, with the registry and `core.ui`
scripted in `tests/scripted_world/`), `src/native_secrets.rs` (unit). Ignored cases are in
`tests/acceptance_pending.rs`. The scripted host has no screen stack: `open`, `run_action`, `redraw`,
`close` and `reset_setting` are refused (BLOCKED H1), so only the first such command a callback
submits is seen (read from its batches), and the focus starts on the first row or where a hand-over
puts it (`focus`); moving it is a redraw.

| Interface-6 test | Native test / ignored case |
|---|---|
| pages: `search_finds_settings_by_words_in_english` | `scripted_pages::search_finds_settings_by_words_in_english`; model `every_word_counts` |
| pages: `search_finds_settings_by_words_in_german_with_or_without_accents` | `scripted_pages::search_finds_settings_by_words_in_german_with_or_without_accents` |
| pages: `a_hit_opens_its_page_with_the_setting_focused` | `scripted_pages::a_hit_opens_its_page_with_the_setting_focused` (back to results: H1, `settings-search`) |
| pages: `search_finds_the_pages_of_other_plugins_and_opens_them` | `scripted_pages::search_finds_the_pages_of_other_plugins_and_opens_them` |
| pages: `a_search_without_hits_says_so_and_an_empty_one_opens_nothing` | `scripted_pages::a_search_without_hits_opens_nothing`, `the_search_asks_for_words_and_a_blank_answer_opens_nothing`; the words said on screen: `settings-search` |
| pages: `advanced_settings_are_found_only_once_shown` | `scripted_pages::advanced_settings_are_found_only_once_shown`; model `advanced_settings_are_found_only_once_shown` |
| pages: `a_choice_with_nothing_to_choose_is_not_listed` | `scripted_pages::a_choice_with_nothing_to_choose_is_not_listed`; model `a_choice_with_nothing_to_choose_is_not_offered` |
| pages: `a_choice_of_more_than_two_opens_its_list_and_the_value_is_kept` | `scripted_pages::a_choice_of_more_than_two_opens_its_list`, `a_value_chosen_in_the_list_is_kept` (closes the list; the value set after it: `settings-choose-value`) |
| pages: `the_language_chosen_shows_at_once_and_after_a_restart` | `settings-choose-value` (host) |
| pages: `a_choice_opened_for_no_setting_shows_nothing_to_choose` | `scripted_pages::a_choice_opened_for_no_setting_shows_nothing_to_choose` |
| pages: `text_typed_for_a_setting_is_kept_trimmed` | `scripted_pages::text_typed_for_a_setting_is_kept_trimmed` (starts from the value it had) |
| pages: `a_page_reset_asks_first_and_sets_its_settings_back` | the question and the answer: `settings-reset` (H1/H2/H4); the settings it resets: model `a_page_reset_keeps_the_account_and_its_password`, `a_page_reset_leaves_what_the_page_does_not_show` |
| pages: `everything_reset_keeps_the_language` | model `everything_reset_keeps_accounts_folders_passwords_and_the_language` |
| pages: `a_page_reset_keeps_the_account_and_its_password` | model `a_page_reset_keeps_the_account_and_its_password` (a password is a secret, never listed) |
| pages: `everything_reset_keeps_accounts_folders_and_passwords` | model `everything_reset_keeps_accounts_folders_passwords_and_the_language` |
| pages: `resetting_a_books_values_does_not_fall_through_to_global_values` | model `resetting_a_books_values_clears_the_scope_last_so_none_fall_to_global_values` (the keys and the order); the host honouring it: `settings-reset-scope` |
| pages: `reset_all_does_not_reload_unchanged_defaults_or_reread_the_card_for_them` | model `reset_all_does_not_reload_unchanged_defaults`; the card reads: `settings-reset-scope` |
| pages: `restart_now_asks_then_restarts` | `settings-restart` (H2/H4); the focus that wraps to the last row: `settings-resize` (moving it is a redraw) |
| pages: `the_settings_open_on_a_setting_focused` | `scripted_pages::the_settings_open_on_a_setting_focused` |
| pages: `plugin_management_and_plugin_settings_have_distinct_names_and_destinations` (ignored there) | `settings-plugin-rows` (ignored, #944) |
| pages: `a_theme_is_chosen_at_the_top_of_the_appearance_settings` | `scripted_pages::a_theme_is_chosen_at_the_top_of_the_appearance_settings`, `none_gives_the_appearance_back_its_defaults_but_keeps_the_text_size`, `a_theme_in_use_has_the_focus_and_choosing_one_applies_its_preset`; model `a_theme_is_in_use_when_its_values_are_set_and_the_rest_is_default` |
| pages: `the_rows_of_a_choice_stand_in_the_readable_column_on_a_wide_screen` | `settings-wide-column` (ignored, drawing) |
| groups: `groups_without_an_order_come_last_among_those_for_customising` | model `the_main_page_lists_groups_by_order_then_name` |
| groups: `an_action_of_a_group_is_its_first_row_with_its_state` | `scripted_pages::a_group_action_is_its_first_row_and_the_settings_follow`; the state at the right: `settings-group-action-state` (**not shown yet**) |
| search: `any_words_search_without_a_panic_and_case_does_not_matter` | model `any_words_search_without_a_panic_and_case_does_not_matter` (2,000 made-up queries), `a_blank_search_finds_nothing_to_open` |
| secrets: `the_entry_shows_only_once_there_are_secrets` | `scripted_pages::the_secrets_are_asked_of_the_service_only_for_the_page_that_lists_them`; the row on screen: `settings-secrets` |
| secrets: `a_secret_is_listed_by_its_label_never_its_value` | `native_secrets::tests::rows_name_the_secret_and_hide_the_value` |
| secrets: `typed_anew_as_a_password_without_the_old_value` | `scripted_secrets::typing_a_secret_anew_keeps_what_was_typed_and_nothing_typed_leaves_it` (masked, nothing written, never in a command) |
| secrets: `nothing_typed_keeps_the_secret_as_it_was` | the same test, second case |
| secrets: `removing_a_secret_asks_first` | `scripted_secrets::removing_a_secret_asks_first_and_removes_only_on_yes` |
| secrets: `a_secret_removed_is_gone_with_its_file_and_entry` | the same test (the removal is the service's; its file and entry are the host's) |
| secrets: `a_secret_that_cannot_be_typed_anew_offers_only_removing` | the same test (first row is Remove); `native_secrets::tests::what_can_be_done_follows_who_may_change_it` |
| secrets: `a_secret_page_for_a_secret_not_kept_only_goes_back` | `scripted_secrets::a_secret_that_is_not_kept_any_more_is_removed_without_asking` |
| secrets: `many_secrets_are_all_reached_and_none_shown` | `scripted_secrets::the_list_asks_for_every_plugins_secrets_a_page_at_a_time`; `native_secrets::tests::rows_name_the_secret_and_hide_the_value` |
| covered: `a_covered_group_page_is_made_anew_once_it_shows` | `scripted_pages::a_covered_group_page_is_made_anew_once_it_shows`, `a_covered_page_is_made_anew_for_a_registry_change_once_it_shows` (a page below another of the same screen); covered by another plugin's screen: `settings-covered-by-foreign-screen` |
| covered: `a_covered_group_list_does_nothing_for_a_change` | `settings-covered-by-foreign-screen` |
| resize: `the_main_page_keeps_its_focus_when_turned` | `settings-resize` (laying out inside the new size and the moved focus) |
| resize: `a_settings_page_keeps_its_focus_when_turned` | `scripted_pages::a_page_keeps_its_focus_when_shown_again_at_another_size` |
| resize: `the_chooser_and_the_search_keep_their_focus_when_turned` | the chooser: the same test; the search's moved focus: `settings-resize` |
| frames: `opening_a_group_does_not_draw_it_aside_for_pictures` | `settings-frames` (ignored: the host's frame) |
| font_slider: `the_sizes_are_spread_evenly_along_the_track` | `settings-font-slider` (ignored: the native plugin has no slider) |
| font_slider: `the_slider_runs_from_the_right_in_a_right_to_left_language` | `settings-font-slider` |

**Differences found.** (1) A group action's row does not show the state of its item (a badge such as
*New*) at the right: `Row::Action` takes none from the registry's menu row
(`settings-group-action-state`). (2) The host sends no "covered" event, so the page on top of its
token (the main page below a group's page included) is made anew for every setting changed, where
interface 6 made a covered page anew once when it showed again; only a page below another of the
same screen waits (`settings-covered-by-foreign-screen`).

## Held removal: to settle at activation (review remarks on the removal PR)

- `crates/paperpaw-app/tests/raw_ids.rs` checks only the registry's titles, notes and choice options
  for every language, not rendered rows: add the rendered native settings screens (every page and
  chooser, in every language) to the activation list, with the tests set aside by the removal.
- The catalogue keys `settings.theme.own` and `settings.theme.none.help`, and the theme descriptions the
  interface-6 chooser showed, are not used by the native plugin (the native chooser shows the preset
  titles only). **Difference from interface 6:** those texts are not shown; restore them in the
  native chooser or drop the keys at activation.
- A reset of all (`reset_plan`) submits one command per key in a single callback; the host caps the
  commands of a callback at 8 (open ruling #422), so the caller must split the plan then.
- While the removal is held, a build without `native-linked` has no Settings page: merge right after
  native activation.

## Heap of the open pages (#248, X4)

- The registry is read **a page at a time** (`Reader::each_setting`): a page of 250 settings is never held
  whole. The main page, the group page, the chooser, the themes, search and the reset plan each copy only
  what they need: the heading groups need no titles or values (`Fields::Light`); a group's page copies only
  its own group's settings; the chooser one; the themes the covered ones; search copies a setting and drops
  it unless it matches. Behaviour is as before (search finds the same settings).
- `ui_kit::mem::push` grows a full vector by its length up to 16 elements, not by doubling, so a list of a
  few hundred rows has at most 15 spare slots instead of up to half of it.
- Measured on the native-default branch (`fw/native-default-228`, emulator `--heap-report`, X4 profile): the
  peak of opening Settings from a reading screen fell from 248.5 to 154.2 KiB (live heap 136.5 KiB at the
  end); the TXT reading flow's peak (`memory.rs`) from 234.1 to 168.0 KiB, which the start and the book now
  set (168.0 KiB at the first page, 136.2 KiB live), not Settings. The gate of #248 (108.5 KiB) is not met
  by the Settings change alone: what native plugins keep resident at start is Core's.
