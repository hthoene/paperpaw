//! The flat boundary: `#[repr(C)]`, `extern "C"`, plain types only.

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Point, Rect};
use paperpaw_sdk::Command;
use paperpaw_text::{Font, GlyphCache, TextStyle};

use crate::widget::Gfx;

/// What the firmware gives a plugin to draw with. Only appended to; says
/// its size. `ctx` is the host's state, opaque to the plugin.
#[repr(C)]
pub struct FlatTable {
    pub size: u32,
    pub fill_rect: unsafe extern "C" fn(ctx: *mut c_void, x: i32, y: i32, w: u32, h: u32, argb: u32),
    pub draw_line: unsafe extern "C" fn(
        ctx: *mut c_void,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        width: u32,
        argb: u32,
    ),
    pub fill_round:
        unsafe extern "C" fn(ctx: *mut c_void, x: u32, y: u32, w: u32, h: u32, radius: u32, argb: u32),
    pub stroke_round: unsafe extern "C" fn(
        ctx: *mut c_void,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        thickness: u32,
        radius: u32,
        argb: u32,
    ),
    /// Width in 1/256 pixels, or a negative error.
    pub text_measure: unsafe extern "C" fn(
        ctx: *mut c_void,
        font: u32,
        size_q8: u32,
        text: *const u8,
        len: usize,
    ) -> i32,
    /// Advance in 1/256 pixels, or a negative error.
    pub text_draw: unsafe extern "C" fn(
        ctx: *mut c_void,
        font: u32,
        size_q8: u32,
        argb: u32,
        x_q8: i32,
        baseline: i32,
        text: *const u8,
        len: usize,
    ) -> i32,
    /// (appended) Width as a C float into `out`; status, or a negative error.
    pub text_measure_f: unsafe extern "C" fn(
        ctx: *mut c_void,
        font: u32,
        size: f32,
        text: *const u8,
        len: usize,
        out: *mut f32,
    ) -> i32,
    /// (appended) Advance as a C float into `out`; status, or a negative error.
    pub text_draw_f: unsafe extern "C" fn(
        ctx: *mut c_void,
        font: u32,
        size: f32,
        argb: u32,
        x: f32,
        baseline: i32,
        text: *const u8,
        len: usize,
        out: *mut f32,
    ) -> i32,
}

/// What a plugin gives the firmware to deliver events with.
#[repr(C)]
pub struct PluginTable {
    pub size: u32,
    pub state: *mut c_void,
    /// An event of `kind` with `payload`; the plugin writes at most `cap`
    /// bytes of a notice into `out` and returns how many (0: none), or a
    /// negative error.
    pub deliver: unsafe extern "C" fn(
        state: *mut c_void,
        kind: u32,
        payload: *const u8,
        len: usize,
        out: *mut u8,
        cap: usize,
    ) -> i32,
}

/// The host's state behind `ctx`.
pub struct HostCtx<'a, 'c> {
    pub canvas: &'a mut Canvas<'c>,
    pub font: &'a Font,
    pub cache: &'a mut GlyphCache,
}

const ERR_UTF8: i32 = -2;
const ERR_FONT: i32 = -1;

fn color(argb: u32) -> Color {
    Color {
        r: (argb >> 16) as u8,
        g: (argb >> 8) as u8,
        b: argb as u8,
    }
}

fn argb(color: Color) -> u32 {
    0xFF00_0000 | u32::from(color.r) << 16 | u32::from(color.g) << 8 | u32::from(color.b)
}

fn rect(x: u32, y: u32, w: u32, h: u32) -> Rect {
    Rect::new(x as u16, y as u16, w as u16, h as u16)
}

/// # Safety
/// `ctx` is the `HostCtx` the table was made with, not used elsewhere now.
unsafe fn host<'x>(ctx: *mut c_void) -> &'x mut HostCtx<'x, 'x> {
    // SAFETY: the caller's contract.
    unsafe { &mut *ctx.cast::<HostCtx<'x, 'x>>() }
}

unsafe extern "C" fn host_fill_rect(ctx: *mut c_void, x: i32, y: i32, w: u32, h: u32, c: u32) {
    // SAFETY: the table is only called with its own context.
    let host = unsafe { host(ctx) };
    host.canvas.fill_rect(x, y, w, h, color(c));
}

unsafe extern "C" fn host_draw_line(
    ctx: *mut c_void,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    width: u32,
    c: u32,
) {
    // SAFETY: as above.
    let host = unsafe { host(ctx) };
    host.canvas
        .draw_line(Point::new(x0, y0), Point::new(x1, y1), width, color(c));
}

unsafe extern "C" fn host_fill_round(
    ctx: *mut c_void,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    c: u32,
) {
    // SAFETY: as above.
    let host = unsafe { host(ctx) };
    host.canvas.fill_round(rect(x, y, w, h), radius, color(c));
}

unsafe extern "C" fn host_stroke_round(
    ctx: *mut c_void,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    thickness: u32,
    radius: u32,
    c: u32,
) {
    // SAFETY: as above.
    let host = unsafe { host(ctx) };
    host.canvas
        .stroke_round(rect(x, y, w, h), thickness, radius, color(c));
}

/// The text, if the plugin gave valid UTF-8.
///
/// # Safety
/// `text` points to `len` readable bytes.
unsafe fn text_of<'t>(text: *const u8, len: usize) -> Option<&'t str> {
    // SAFETY: the caller's contract.
    let bytes = unsafe { core::slice::from_raw_parts(text, len) };
    core::str::from_utf8(bytes).ok()
}

unsafe extern "C" fn host_text_measure(
    ctx: *mut c_void,
    font: u32,
    size_q8: u32,
    text: *const u8,
    len: usize,
) -> i32 {
    if font != 0 {
        return ERR_FONT;
    }
    // SAFETY: as above.
    let host = unsafe { host(ctx) };
    // SAFETY: the plugin's pointer and length.
    let Some(text) = (unsafe { text_of(text, len) }) else {
        return ERR_UTF8;
    };
    let width = paperpaw_text::measure(host.font, size_q8 as f32 / 256.0, text);
    (width * 256.0) as i32
}

unsafe extern "C" fn host_text_draw(
    ctx: *mut c_void,
    font: u32,
    size_q8: u32,
    c: u32,
    x_q8: i32,
    baseline: i32,
    text: *const u8,
    len: usize,
) -> i32 {
    if font != 0 {
        return ERR_FONT;
    }
    // SAFETY: as above.
    let host = unsafe { host(ctx) };
    // SAFETY: the plugin's pointer and length.
    let Some(text) = (unsafe { text_of(text, len) }) else {
        return ERR_UTF8;
    };
    let style = TextStyle::new(host.font, 0, size_q8 as f32 / 256.0, color(c));
    let advance = paperpaw_text::draw_text(
        host.canvas,
        host.cache,
        &style,
        x_q8 as f32 / 256.0,
        baseline,
        text,
    );
    (advance * 256.0) as i32
}

const ERR_ARG: i32 = -4;

unsafe extern "C" fn host_text_measure_f(
    ctx: *mut c_void,
    font: u32,
    size: f32,
    text: *const u8,
    len: usize,
    out: *mut f32,
) -> i32 {
    if font != 0 {
        return ERR_FONT;
    }
    if !(size.is_finite() && size > 0.0 && size < 4096.0) {
        return ERR_ARG;
    }
    // SAFETY: as above.
    let host = unsafe { host(ctx) };
    // SAFETY: the plugin's pointer and length.
    let Some(text) = (unsafe { text_of(text, len) }) else {
        return ERR_UTF8;
    };
    let width = paperpaw_text::measure(host.font, size, text);
    // SAFETY: `out` is the plugin's one float.
    unsafe { out.write(width) };
    0
}

unsafe extern "C" fn host_text_draw_f(
    ctx: *mut c_void,
    font: u32,
    size: f32,
    c: u32,
    x: f32,
    baseline: i32,
    text: *const u8,
    len: usize,
    out: *mut f32,
) -> i32 {
    if font != 0 {
        return ERR_FONT;
    }
    if !(size.is_finite() && size > 0.0 && size < 4096.0 && x.is_finite()) {
        return ERR_ARG;
    }
    // SAFETY: as above.
    let host = unsafe { host(ctx) };
    // SAFETY: the plugin's pointer and length.
    let Some(text) = (unsafe { text_of(text, len) }) else {
        return ERR_UTF8;
    };
    let style = TextStyle::new(host.font, 0, size, color(c));
    let advance = paperpaw_text::draw_text(host.canvas, host.cache, &style, x, baseline, text);
    // SAFETY: `out` is the plugin's one float.
    unsafe { out.write(advance) };
    0
}

/// The firmware's table.
pub static FLAT: FlatTable = FlatTable {
    size: core::mem::size_of::<FlatTable>() as u32,
    fill_rect: host_fill_rect,
    draw_line: host_draw_line,
    fill_round: host_fill_round,
    stroke_round: host_stroke_round,
    text_measure: host_text_measure,
    text_draw: host_text_draw,
    text_measure_f: host_text_measure_f,
    text_draw_f: host_text_draw_f,
};

/// What a ported plugin draws with: the table and the host's context.
pub struct FlatGfx<'a> {
    pub table: &'a FlatTable,
    pub ctx: *mut c_void,
}

impl Gfx for FlatGfx<'_> {
    fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, c: Color) {
        // SAFETY: the table and its context belong together.
        unsafe { (self.table.fill_rect)(self.ctx, x, y, w, h, argb(c)) };
    }

    fn draw_line(&mut self, from: Point, to: Point, width: u32, c: Color) {
        // SAFETY: as above.
        unsafe {
            (self.table.draw_line)(self.ctx, from.x, from.y, to.x, to.y, width, argb(c));
        }
    }

    fn fill_round(&mut self, area: Rect, radius: u32, c: Color) {
        // SAFETY: as above.
        unsafe {
            (self.table.fill_round)(
                self.ctx,
                u32::from(area.x),
                u32::from(area.y),
                u32::from(area.width),
                u32::from(area.height),
                radius,
                argb(c),
            );
        }
    }

    fn stroke_round(&mut self, area: Rect, thickness: u32, radius: u32, c: Color) {
        // SAFETY: as above.
        unsafe {
            (self.table.stroke_round)(
                self.ctx,
                u32::from(area.x),
                u32::from(area.y),
                u32::from(area.width),
                u32::from(area.height),
                thickness,
                radius,
                argb(c),
            );
        }
    }

    fn measure(&mut self, size: f32, text: &str) -> f32 {
        // SAFETY: as above; the text is valid for its length.
        let q8 = unsafe {
            (self.table.text_measure)(self.ctx, 0, (size * 256.0) as u32, text.as_ptr(), text.len())
        };
        q8 as f32 / 256.0
    }

    fn text(&mut self, size: f32, x: f32, baseline: i32, text: &str, c: Color) -> f32 {
        // SAFETY: as above.
        let q8 = unsafe {
            (self.table.text_draw)(
                self.ctx,
                0,
                (size * 256.0) as u32,
                argb(c),
                (x * 256.0) as i32,
                baseline,
                text.as_ptr(),
                text.len(),
            )
        };
        q8 as f32 / 256.0
    }
}

/// The prototype's plugin as a flat plugin: its state is a counter.
pub struct FlatPlugin {
    opened: u32,
}

impl FlatPlugin {
    pub const fn new() -> Self {
        Self { opened: 0 }
    }

    /// Its table; `self` must stay where it is while the table is used.
    pub fn table(&mut self) -> PluginTable {
        PluginTable {
            size: core::mem::size_of::<PluginTable>() as u32,
            state: (self as *mut FlatPlugin).cast(),
            deliver: plugin_deliver,
        }
    }
}

impl Default for FlatPlugin {
    fn default() -> Self {
        Self::new()
    }
}

const PREFIX: &[u8] = b"Opened: ";

unsafe extern "C" fn plugin_deliver(
    state: *mut c_void,
    kind: u32,
    payload: *const u8,
    len: usize,
    out: *mut u8,
    cap: usize,
) -> i32 {
    // SAFETY: the table's own state.
    let this = unsafe { &mut *state.cast::<FlatPlugin>() };
    if kind != 1 {
        return 0;
    }
    this.opened += 1;
    let total = PREFIX.len() + len;
    if total > cap {
        return -3;
    }
    // SAFETY: `out` has `cap` bytes and `payload` has `len`; `total <= cap`.
    unsafe {
        core::ptr::copy_nonoverlapping(PREFIX.as_ptr(), out, PREFIX.len());
        core::ptr::copy_nonoverlapping(payload, out.add(PREFIX.len()), len);
    }
    total as i32
}

/// The host delivers an event to a flat plugin and turns what comes back
/// into a `Command`.
pub fn deliver(plugin: &PluginTable, kind: u32, payload: &str, out: &mut Vec<Command>) {
    let mut buffer = [0u8; 96];
    // SAFETY: the table's state and the buffers are valid for the call.
    let written = unsafe {
        (plugin.deliver)(
            plugin.state,
            kind,
            payload.as_ptr(),
            payload.len(),
            buffer.as_mut_ptr(),
            buffer.len(),
        )
    };
    if written > 0 {
        let bytes = &buffer[..written as usize];
        if let Ok(text) = core::str::from_utf8(bytes) {
            out.push(Command::Notify(String::from(text)));
        }
    }
}

/// The plugin call alone into `out` (`cap` bytes, not zeroed by the caller).
pub fn deliver_raw(plugin: &PluginTable, kind: u32, payload: &str, out: *mut u8, cap: usize) -> i32 {
    // SAFETY: the table's state; `out` has `cap` bytes.
    unsafe { (plugin.deliver)(plugin.state, kind, payload.as_ptr(), payload.len(), out, cap) }
}

/// Like [`deliver`], but the 96 byte buffer is not zeroed first.
pub fn deliver_uninit(plugin: &PluginTable, kind: u32, payload: &str, out: &mut Vec<Command>) {
    let mut buffer = core::mem::MaybeUninit::<[u8; 96]>::uninit();
    let written = deliver_raw(plugin, kind, payload, buffer.as_mut_ptr().cast(), 96);
    if written > 0 {
        // SAFETY: the plugin wrote `written` bytes, at most 96.
        let bytes = unsafe { core::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), written as usize) };
        if let Ok(text) = core::str::from_utf8(bytes) {
            out.push(Command::Notify(String::from(text)));
        }
    }
}

/// The same plugin-side wrapper with text through the appended C-float entries.
pub struct FlatGfxF<'a>(pub FlatGfx<'a>);

impl Gfx for FlatGfxF<'_> {
    fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, c: Color) {
        self.0.fill_rect(x, y, w, h, c);
    }
    fn draw_line(&mut self, from: Point, to: Point, width: u32, c: Color) {
        self.0.draw_line(from, to, width, c);
    }
    fn fill_round(&mut self, area: Rect, radius: u32, c: Color) {
        self.0.fill_round(area, radius, c);
    }
    fn stroke_round(&mut self, area: Rect, thickness: u32, radius: u32, c: Color) {
        self.0.stroke_round(area, thickness, radius, c);
    }
    fn measure(&mut self, size: f32, text: &str) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: as above; `out` lives for the call.
        let status = unsafe {
            (self.0.table.text_measure_f)(self.0.ctx, 0, size, text.as_ptr(), text.len(), &raw mut out)
        };
        if status < 0 { 0.0 } else { out }
    }
    fn text(&mut self, size: f32, x: f32, baseline: i32, text: &str, c: Color) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: as above.
        let status = unsafe {
            (self.0.table.text_draw_f)(
                self.0.ctx,
                0,
                size,
                argb(c),
                x,
                baseline,
                text.as_ptr(),
                text.len(),
                &raw mut out,
            )
        };
        if status < 0 { 0.0 } else { out }
    }
}
