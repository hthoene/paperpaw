//! Concrete native host table entries shared by all shells.
use super::*;

/// The host table with this shell's entries in it, as a plugin of `S` sees it.
/// The shell's preflight may read it before anything is hosted.
#[must_use]
pub fn host_table<S: Services>() -> PpHost {
    let mut table = host_glue::host_table();
    table.heap_alloc = heap_alloc::<S>;
    table.heap_dealloc = heap_dealloc::<S>;
    table.heap_realloc = heap_realloc::<S>;
    table.clock_micros = clock_micros::<S>;
    table.stage_run = stage_slot::run::<S>;
    table.ui_compose = crate::native_host::ui_compose::compose::<S>;
    table.registrations_submit = registrations_submit::<S>;
    table.commands_submit = commands_submit::<S>;
    table.env_snapshot = env_snapshot::<S>;
    table.settings_read = settings_read::<S>;
    table.canvas_fill_rect = canvas_fill_rect::<S>;
    table.i18n_copy = i18n_copy::<S>;
    table.i18n_format = i18n_format::<S>;
    table.i18n_digits = i18n_digits::<S>;
    table.canvas_draw_line = canvas_draw_line::<S>;
    table.canvas_fill_round = canvas_fill_round::<S>;
    table.canvas_stroke_round = canvas_stroke_round::<S>;
    table.canvas_clip = canvas_clip::<S>;
    table.storage_size = storage_size::<S>;
    table.storage_read_at = storage_read_at::<S>;
    table.storage_write = storage_write::<S>;
    table.storage_append = storage_append::<S>;
    table.storage_rename = storage_rename::<S>;
    table.storage_remove = storage_remove::<S>;
    table.storage_sync = storage_sync::<S>;
    table.service_open = service_open::<S>;
    table.service_call = service_call::<S>;
    table.services_query = services_query::<S>;
    table.blob_read = blob_read::<S>;
    table.blob_release = blob_release::<S>;
    table.font_acquire = font_acquire::<S>;
    table.handle_release = handle_release::<S>;
    table.text_measure = text_measure::<S>;
    table.text_metrics = text_metrics::<S>;
    table.text_draw = text_draw::<S>;
    table.text_hyphenate = text_hyphenate::<S>;
    table.icon_glyph = icon_glyph::<S>;
    table.image_info = image_info::<S>;
    table.canvas_draw_image = canvas_draw_image::<S>;
    // The X4 ink probe is enabled only after device timing qualifies it.
    #[cfg(not(any(target_arch = "riscv32", target_arch = "xtensa")))]
    {
        table.text_ink = text_ink::<S>;
    }
    table
}
