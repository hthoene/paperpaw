//! The runtime check behind the `host_slot` source scan, split from
//! `shell_tests.rs` (file length limit). It is a child of `instance_tests` and
//! uses that module's fakes.

use super::*;

/// The runtime check that backs the source scan in `host_slot`: every
/// entry of the host table a plugin can call, called once with empty
/// arguments, announces its host slot (and ends it) before it does or
/// refuses anything. The allocator and the clock are exempt (hot,
/// bounded), and so are the entries this shell leaves to the default stubs
/// of `host_glue` ([`UNWIRED`]), which must answer `ERR_UNSUPPORTED` at once
/// and open no slot: wiring one fails here until it leaves the list. The
/// one list below names every field of the table: a new entry fails the
/// count until added.
/// The entries the core host table leaves to `host_glue`'s stubs: slot 41
/// (`text.draw_inked`, contract only until its host side lands). Named, not found by comparing function
/// pointers: the compiler may give one small function a copy per codegen
/// unit, so the same stub can have two addresses.
const UNWIRED: &[&str] = &["text_draw_inked"];

#[allow(
    clippy::too_many_lines,
    reason = "one flat list: a call per entry of the host table"
)]
#[test]
fn every_called_host_table_entry_but_the_allocator_and_the_clock_announces_its_slot() {
    use paperpaw_native::host_glue::{self, PpBuffer, PpBytes};
    crate::native_host::host_slot::set(slot_begin, slot_end);
    reset(NORMAL);
    let hosted = fake();
    let context = hosted.owner().context();
    let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast::<c_void>();
    let table = &context.table;
    let none = PpBytes {
        ptr: ptr::null(),
        len: 0,
    };
    let nobuf = PpBuffer {
        ptr: ptr::null_mut(),
        cap: 0,
    };
    let (mut word, mut long, mut floats, mut ints) = (0_u32, 0_u64, [0_f32; 3], [0_i32; 4]);
    let (mut wired, mut unwired) = (0_usize, 0_usize);
    let mut silent = Vec::new();
    let mut stale = Vec::new();
    // One entry: its name, whether the shell wired it, the call.
    macro_rules! entry {
        ($name:ident, $call:expr) => {{
            let is_wired = !UNWIRED.contains(&stringify!($name));
            let before = SLOTS.with(Cell::get);
            // SAFETY: a live owner's context; empty or null arguments
            // that every entry must refuse, never read.
            let status = unsafe { $call };
            if is_wired {
                wired += 1;
                if SLOTS.with(Cell::get) != before + 1 {
                    silent.push(stringify!($name));
                }
            } else {
                unwired += 1;
                if status != host_glue::ERR_UNSUPPORTED || SLOTS.with(Cell::get) != before {
                    stale.push(stringify!($name));
                }
            }
            assert_eq!(
                SLOTS_OPEN.with(Cell::get),
                0,
                concat!(stringify!($name), " ended")
            );
        }};
    }
    entry!(env_snapshot, (table.env_snapshot)(ctx, nobuf));
    entry!(settings_read, (table.settings_read)(ctx, none, nobuf));
    entry!(i18n_copy, (table.i18n_copy)(ctx, none, nobuf));
    entry!(font_acquire, (table.font_acquire)(ctx, none, &mut word));
    entry!(handle_release, (table.handle_release)(ctx, 0));
    entry!(
        canvas_fill_rect,
        (table.canvas_fill_rect)(ctx, 0, 0, 0, 0, 0)
    );
    entry!(
        canvas_draw_line,
        (table.canvas_draw_line)(ctx, 0, 0, 0, 0, 0, 0)
    );
    entry!(
        canvas_fill_round,
        (table.canvas_fill_round)(ctx, 0, 0, 0, 0, 0, 0)
    );
    entry!(
        canvas_stroke_round,
        (table.canvas_stroke_round)(ctx, 0, 0, 0, 0, 0, 0, 0)
    );
    entry!(
        text_measure,
        (table.text_measure)(ctx, 0, 0.0, none, &mut floats[0])
    );
    entry!(
        text_draw,
        (table.text_draw)(ctx, 0, 0.0, 0.0, 0, 0, none, &mut floats[0])
    );
    entry!(commands_submit, (table.commands_submit)(ctx, none));
    entry!(
        registrations_submit,
        (table.registrations_submit)(ctx, none)
    );
    entry!(
        services_query,
        (table.services_query)(ctx, none, 0, 0, &mut word)
    );
    entry!(blob_read, (table.blob_read)(ctx, 0, 0, nobuf));
    entry!(blob_release, (table.blob_release)(ctx, 0));
    entry!(storage_size, (table.storage_size)(ctx, 0, none, &mut long));
    entry!(
        storage_read_at,
        (table.storage_read_at)(ctx, 0, none, 0, nobuf)
    );
    entry!(storage_write, (table.storage_write)(ctx, 0, none, none));
    entry!(storage_append, (table.storage_append)(ctx, 0, none, none));
    entry!(storage_rename, (table.storage_rename)(ctx, 0, none, none));
    entry!(storage_remove, (table.storage_remove)(ctx, 0, none));
    entry!(storage_sync, (table.storage_sync)(ctx));
    entry!(i18n_format, (table.i18n_format)(ctx, none, none, nobuf));
    entry!(i18n_digits, (table.i18n_digits)(ctx, none, nobuf));
    {
        let [ascent, descent, line] = &mut floats;
        entry!(
            text_metrics,
            (table.text_metrics)(ctx, 0, 0.0, ascent, descent, line)
        );
    }
    {
        let [top, bottom, left, right] = &mut ints;
        entry!(
            text_ink,
            (table.text_ink)(ctx, 0, 0.0, none, top, bottom, left, right)
        );
    }
    entry!(canvas_clip, (table.canvas_clip)(ctx, 0, 0, 0, 0));
    entry!(stage_run, (table.stage_run)(ctx, none, 0, none, nobuf));
    entry!(
        service_open,
        (table.service_open)(ctx, none, 0, 0, &mut word)
    );
    entry!(service_call, (table.service_call)(ctx, 0, 0, none, nobuf));
    entry!(ui_compose, (table.ui_compose)(ctx, 0, 0, 0, 0, 0, 0));
    entry!(image_info, (table.image_info)(ctx, 0, nobuf));
    entry!(
        canvas_draw_image,
        (table.canvas_draw_image)(ctx, 0, 0, 0, 0, 0, 0)
    );
    entry!(
        text_hyphenate,
        (table.text_hyphenate)(ctx, none, none, 0, 0, nobuf)
    );
    entry!(text_covers, (table.text_covers)(ctx, 0, none));
    entry!(icon_glyph, (table.icon_glyph)(ctx, none, nobuf));
    entry!(
        text_draw_inked,
        (table.text_draw_inked)(ctx, 0, 0.0, 0.0, 0, 0, none, 0, none, 0, &mut floats[0])
    );
    entry!(
        canvas_describe,
        (table.canvas_describe)(ctx, 0, 0, none, 0, 0, 0, 0)
    );
    // Every field of the table is accounted for: the four exempt ones
    // (heap_alloc, heap_dealloc, heap_realloc, clock_micros) plus the
    // list above. A new entry makes the table longer than the list.
    let fields = (size_of::<PpHost>() - 8) / size_of::<usize>();
    assert_eq!(
        wired + unwired + 4,
        fields,
        "the host table has entries this check does not call"
    );
    assert!(wired >= 25, "only {wired} wired entries were called");
    assert!(
        silent.is_empty(),
        "host entries that did not announce one host slot: {silent:?}"
    );
    assert_eq!(
        unwired,
        UNWIRED.len(),
        "every UNWIRED name is a called entry"
    );
    assert!(
        stale.is_empty(),
        "entries in UNWIRED that are wired now (take them off the list): {stale:?}"
    );
    drop(hosted);
}
