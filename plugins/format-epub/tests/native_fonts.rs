//! An EPUB with an embedded font, opened through the plugin's real native
//! entry (`NATIVE_ENTRY`: `make`, then a `core.format` stage call per
//! `dispatch_event`) over a fake host table whose storage honours the roots of
//! a format call: root 3 is the book, read only; root 6 is the document cache,
//! writable, where the host reserves `.complete` and `.host/`. The font is
//! copied through a temporary name that is renamed into place, so the plugin's
//! `Storage::rename` must reach the host's `storage_rename` slot (it once
//! answered `Unsupported`, and the font was silently lost).
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]

use std::{alloc::Layout, cell::RefCell, collections::BTreeMap, ffi::c_void, ptr};

use paperpaw_core::native_host::registrations::{Record, StageCall, records_encode};
use paperpaw_native::host_glue::{
    ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, OK, PpBuffer, PpBytes, PpEntry, PpHost,
    PpPlugin, host_table,
};
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_zip::testing::ZipBuilder;

const TTF: &[u8] =
    include_bytes!("../../../crates/paperpaw-epub-family/tests/data/atkinson-subset.ttf");

/// The token the format's declaration names (`declarations.rs`).
const FORMAT_TOKEN: u32 = 1;
const ROOT_BOOK: u32 = 3;
const ROOT_CACHE: u32 = 6;

mod op {
    pub const OPEN: u32 = 2;
    pub const SECTIONS: u32 = 7;
    pub const FONTS: u32 = 16;
}

/// What the fake host holds: the book (root 3) and the document cache (root 6,
/// by path below it), and the renames it was asked for.
struct Context {
    book: Vec<u8>,
    cache: RefCell<BTreeMap<String, Vec<u8>>>,
    renames: RefCell<Vec<(String, String)>>,
    /// Root-6 names the plugin tried to change that the host keeps for itself.
    denied: RefCell<Vec<String>>,
}

/// The names of root 6 the host reserves.
fn reserved(path: &str) -> bool {
    path == ".complete" || path == ".host" || path.starts_with(".host/")
}

/// # Safety
/// `ctx` is the live `Context` of the `Native` that owns this table.
unsafe fn context<'a>(ctx: *mut c_void) -> &'a Context {
    // SAFETY: every hook receives the owner's unchanged pointer.
    unsafe { &*ctx.cast::<Context>() }
}

/// # Safety
/// `bytes` describes readable bytes that live for the call (or is empty).
unsafe fn input<'a>(bytes: PpBytes) -> &'a [u8] {
    if bytes.len == 0 {
        return &[];
    }
    // SAFETY: the plugin lends initialized bytes for this call.
    unsafe { std::slice::from_raw_parts(bytes.ptr, bytes.len) }
}

/// # Safety
/// As [`input`], for a path.
unsafe fn name<'a>(bytes: PpBytes) -> &'a str {
    // SAFETY: as `input`.
    std::str::from_utf8(unsafe { input(bytes) }).unwrap()
}

fn copy_out(bytes: &[u8], at: u64, out: &PpBuffer) -> i32 {
    let tail = bytes
        .get(usize::try_from(at).unwrap()..)
        .unwrap_or_default();
    let count = tail.len().min(out.cap);
    if count > 0 {
        // SAFETY: the plugin lends `out.cap` writable bytes, disjoint from the host's data.
        unsafe { ptr::copy_nonoverlapping(tail.as_ptr(), out.ptr, count) };
    }
    i32::try_from(count).unwrap()
}

unsafe extern "C" fn alloc(
    _: *mut c_void,
    size: usize,
    align: usize,
    out: *mut *mut c_void,
) -> i32 {
    // SAFETY: the entry asks for a valid nonzero layout and a writable output.
    unsafe {
        let block = std::alloc::alloc(Layout::from_size_align(size, align).unwrap());
        if block.is_null() {
            return ERR_NO_MEMORY;
        }
        out.write(block.cast());
    }
    OK
}

unsafe extern "C" fn dealloc(_: *mut c_void, block: *mut c_void, size: usize, align: usize) -> i32 {
    // SAFETY: the entry returns a block of ours with its layout.
    unsafe { std::alloc::dealloc(block.cast(), Layout::from_size_align(size, align).unwrap()) };
    OK
}

unsafe extern "C" fn realloc(
    _: *mut c_void,
    block: *mut c_void,
    old: usize,
    align: usize,
    new: usize,
    out: *mut *mut c_void,
) -> i32 {
    // SAFETY: the entry returns a block of ours with its layout.
    unsafe {
        let moved = std::alloc::realloc(
            block.cast(),
            Layout::from_size_align(old, align).unwrap(),
            new,
        );
        if moved.is_null() {
            return ERR_NO_MEMORY;
        }
        out.write(moved.cast());
    }
    OK
}

unsafe extern "C" fn submit(_: *mut c_void, _: PpBytes) -> i32 {
    OK
}

unsafe extern "C" fn size(ctx: *mut c_void, root: u32, path: PpBytes, out: *mut u64) -> i32 {
    // SAFETY: the owner's context and the plugin's path, live for this call.
    let (context, path) = unsafe { (context(ctx), name(path)) };
    let len = match root {
        ROOT_BOOK if path.is_empty() => context.book.len(),
        ROOT_BOOK => return ERR_NOT_FOUND,
        ROOT_CACHE => match context.cache.borrow().get(path) {
            Some(file) => file.len(),
            None => return ERR_NOT_FOUND,
        },
        _ => return ERR_INVALID,
    };
    // SAFETY: a writable aligned output.
    unsafe { out.write(len as u64) };
    OK
}

unsafe extern "C" fn read_at(
    ctx: *mut c_void,
    root: u32,
    path: PpBytes,
    offset: u64,
    out: PpBuffer,
) -> i32 {
    // SAFETY: the owner's context and the plugin's path, live for this call.
    let (context, path) = unsafe { (context(ctx), name(path)) };
    match root {
        ROOT_BOOK if path.is_empty() => copy_out(&context.book, offset, &out),
        ROOT_BOOK => ERR_NOT_FOUND,
        ROOT_CACHE => match context.cache.borrow().get(path) {
            Some(file) => copy_out(file, offset, &out),
            None => ERR_NOT_FOUND,
        },
        _ => ERR_INVALID,
    }
}

/// The root-6 name `path` for a change, or the status that refuses it.
fn writable<'a>(context: &Context, root: u32, path: &'a str) -> Result<&'a str, i32> {
    if root != ROOT_CACHE {
        // The book is read only; the other roots are not this call's.
        return Err(if root == ROOT_BOOK {
            ERR_DENIED
        } else {
            ERR_INVALID
        });
    }
    if reserved(path) {
        context.denied.borrow_mut().push(path.into());
        return Err(ERR_DENIED);
    }
    Ok(path)
}

unsafe extern "C" fn write(ctx: *mut c_void, root: u32, path: PpBytes, data: PpBytes) -> i32 {
    // SAFETY: the owner's context and the plugin's bytes, live for this call.
    let (context, path, data) = unsafe { (context(ctx), name(path), input(data)) };
    match writable(context, root, path) {
        Ok(path) => {
            context
                .cache
                .borrow_mut()
                .insert(path.into(), data.to_vec());
            OK
        }
        Err(status) => status,
    }
}

unsafe extern "C" fn append(ctx: *mut c_void, root: u32, path: PpBytes, data: PpBytes) -> i32 {
    // SAFETY: the owner's context and the plugin's bytes, live for this call.
    let (context, path, data) = unsafe { (context(ctx), name(path), input(data)) };
    match writable(context, root, path) {
        Ok(path) => {
            context
                .cache
                .borrow_mut()
                .entry(path.into())
                .or_default()
                .extend_from_slice(data);
            OK
        }
        Err(status) => status,
    }
}

unsafe extern "C" fn rename(ctx: *mut c_void, root: u32, from: PpBytes, to: PpBytes) -> i32 {
    // SAFETY: the owner's context and the plugin's paths, live for this call.
    let (context, from, to) = unsafe { (context(ctx), name(from), name(to)) };
    let (from, to) = match (writable(context, root, from), writable(context, root, to)) {
        (Ok(from), Ok(to)) => (from, to),
        (Err(status), _) | (_, Err(status)) => return status,
    };
    context.renames.borrow_mut().push((from.into(), to.into()));
    let mut cache = context.cache.borrow_mut();
    // Replacing the destination, as the host's rename does.
    let Some(file) = cache.remove(from) else {
        return ERR_NOT_FOUND;
    };
    cache.insert(to.into(), file);
    OK
}

unsafe extern "C" fn remove(ctx: *mut c_void, root: u32, path: PpBytes) -> i32 {
    // SAFETY: the owner's context and the plugin's path, live for this call.
    let (context, path) = unsafe { (context(ctx), name(path)) };
    match writable(context, root, path) {
        Ok(path) => {
            // Removing an absent file succeeds.
            context.cache.borrow_mut().remove(path);
            OK
        }
        Err(status) => status,
    }
}

unsafe extern "C" fn sync(_: *mut c_void) -> i32 {
    OK
}

fn table() -> PpHost {
    let mut table = host_table();
    table.heap_alloc = alloc;
    table.heap_dealloc = dealloc;
    table.heap_realloc = realloc;
    table.registrations_submit = submit;
    table.storage_size = size;
    table.storage_read_at = read_at;
    table.storage_write = write;
    table.storage_append = append;
    table.storage_rename = rename;
    table.storage_remove = remove;
    table.storage_sync = sync;
    table
}

/// The plugin made through its entry, over the fake host. Dropping it destroys
/// the instance.
struct Native {
    context: Box<Context>,
    _table: Box<PpHost>,
    plugin: *const PpPlugin,
}

impl Native {
    fn new(book: Vec<u8>) -> Self {
        let entry: &'static PpEntry = paperpaw_plugin_format_epub::NATIVE_ENTRY.entry();
        let context = Box::new(Context {
            book,
            cache: RefCell::new(BTreeMap::new()),
            renames: RefCell::new(Vec::new()),
            denied: RefCell::new(Vec::new()),
        });
        let table = Box::new(table());
        let mut plugin = ptr::null();
        // SAFETY: both boxes keep their addresses until `drop` destroys the
        // instance; the output is a local pointer and `make` runs once.
        let status = unsafe {
            entry.make.unwrap()(
                &raw const *table,
                ptr::from_ref(&*context).cast_mut().cast(),
                &mut plugin,
            )
        };
        assert_eq!(status, OK);
        assert!(!plugin.is_null());
        Self {
            context,
            _table: table,
            plugin,
        }
    }

    /// One `core.format` stage call with `fmt.call` payload `payload`: the
    /// plugin's reply rows, encoded.
    fn call(&mut self, payload: &[u8]) -> Vec<u8> {
        let mut event = vec![0u8; 4096];
        let n = records_encode(
            &[Record::StageCall(StageCall {
                stage: "core.format",
                schema: 1,
                payload,
            })],
            &mut event,
        )
        .unwrap();
        let mut out = vec![0u8; 3840];
        // SAFETY: this owner exclusively admits the live instance; the input
        // and the output are disjoint buffers lent for this call only.
        let status = unsafe {
            let plugin = &*self.plugin;
            plugin.dispatch_event.unwrap()(
                plugin.state,
                FORMAT_TOKEN,
                PpBytes {
                    ptr: event.as_ptr(),
                    len: n,
                },
                PpBuffer {
                    ptr: out.as_mut_ptr(),
                    cap: out.len(),
                },
            )
        };
        let len = usize::try_from(status).unwrap_or_else(|_| panic!("status {status}"));
        out.truncate(len);
        out
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        // SAFETY: no call is running; the table and context are still alive and
        // the instance is destroyed once.
        unsafe {
            let plugin = &*self.plugin;
            plugin.drop.unwrap()(plugin.state);
        }
    }
}

fn fmt_call(op: u32, f: impl FnOnce(&mut stage::FmtCall<'static>)) -> Vec<u8> {
    let mut c = stage::FmtCall::new(op, 1, 0, 0, 0, 0, 0, 1, 0, 0, 7, "", &[]);
    f(&mut c);
    let mut buf = vec![0u8; 4096];
    let n = stage::input_encode(&[stage::Record::FmtCall(c)], &mut buf).unwrap();
    buf.truncate(n);
    buf
}

fn rows(reply: &[u8]) -> Vec<stage::Record<'_>> {
    stage::reply_decode(reply)
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// An EPUB 3 with one chapter whose stylesheet embeds `font` (`fonts/a.ttf`).
fn book_with_font(font: &[u8]) -> Vec<u8> {
    let container = "<?xml version=\"1.0\"?><container version=\"1.0\" \
        xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles>\
        <rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/>\
        </rootfiles></container>";
    let package = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
        <package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"uid\">\
        <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
        <dc:identifier id=\"uid\">urn:test</dc:identifier><dc:language>de</dc:language></metadata>\
        <manifest><item id=\"c\" href=\"c.xhtml\" media-type=\"application/xhtml+xml\"/>\
        <item id=\"css\" href=\"style.css\" media-type=\"text/css\"/></manifest>\
        <spine><itemref idref=\"c\"/></spine></package>";
    let chapter = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
        <html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>t</title></head>\
        <body><p class=\"f0\">Font text</p></body></html>";
    let css = "@font-face { font-family: F0; src: url(fonts/a.ttf) } .f0 { font-family: F0 }\n";
    ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated("META-INF/container.xml", container)
        .deflated("OEBPS/content.opf", package)
        .deflated("OEBPS/c.xhtml", chapter)
        .deflated("OEBPS/style.css", css)
        .deflated("OEBPS/fonts/a.ttf", font.to_vec())
        .build()
}

#[test]
fn an_embedded_font_is_copied_into_the_document_cache() {
    let mut native = Native::new(book_with_font(TTF));
    // The book is the file itself: root 3, the empty path.
    let opened = native.call(&fmt_call(op::OPEN, |c| c.text = ""));
    let stage::Record::FmtState(state) = rows(&opened)[0] else {
        panic!("open answers a state row");
    };
    assert_eq!((state.state, state.status), (0, 0), "the book opens");

    let reply = native.call(&fmt_call(op::FONTS, |_| {}));
    let faces: Vec<_> = rows(&reply)
        .into_iter()
        .filter_map(|row| match row {
            stage::Record::FmtFont(f) => Some((f.index, f.bytes)),
            _ => None,
        })
        .collect();
    assert_eq!(faces, [(0, u32::try_from(TTF.len()).unwrap())]);

    let context = &native.context;
    let cache = context.cache.borrow();
    assert_eq!(
        cache.get("fonts/0").map(Vec::as_slice),
        Some(TTF),
        "the font file is in the cache with the font's bytes: {:?}",
        cache.keys().collect::<Vec<_>>()
    );
    // Only the font is below `fonts/`: the temporary name is gone.
    let below: Vec<_> = cache.keys().filter(|k| k.starts_with("fonts/")).collect();
    assert_eq!(below, ["fonts/0"]);
    // It got there by the rename of the temporary file, within root 6.
    assert_eq!(
        *context.renames.borrow(),
        [("fonts/0.tmp".to_string(), "fonts/0".to_string())]
    );
    // The plugin kept off what the host reserves, and the book was not touched.
    assert!(context.denied.borrow().is_empty());
}

/// The same open, read for its chapters: the first open of a book natively
/// does not leave the document "uncleared" (the cache folder is the host's
/// and starts empty), so the sections are listed too.
#[test]
fn the_sections_of_a_book_are_listed_after_its_first_native_open() {
    let mut native = Native::new(book_with_font(TTF));
    let opened = native.call(&fmt_call(op::OPEN, |c| c.text = ""));
    let stage::Record::FmtState(state) = rows(&opened)[0] else {
        panic!("open answers a state row");
    };
    assert_eq!((state.state, state.status), (0, 0), "the book opens");
    let reply = native.call(&fmt_call(op::SECTIONS, |_| {}));
    let sections = rows(&reply)
        .into_iter()
        .filter(|row| matches!(row, stage::Record::FmtSection(_)))
        .count();
    assert!(sections >= 1, "the book's chapter is listed");
}
