//! The PaperPaw plugin SDK.
//!
//! PaperPaw is a small kernel plus plugins. Every screen, action, setting,
//! key map and translation (including those PaperPaw ships with) is
//! contributed by a plugin through this SDK. Anything a built-in plugin does,
//! a community plugin can do too, and replace it.
//!
//! # Status
//!
//! This is a beta (0.5.0-beta.12). The SDK may still change before 1.0;
//! the CHANGELOG lists every breaking change with what replaces it.
//! A plugin with code is built for a plugin interface
//! ([`native::INTERFACE`]) and rebuilt when a release raises it.
//!
//! # Concepts
//!
//! - **Ids** ([`Id`]): every contribution has a namespaced id such as
//!   `diagnostics.screen`. The namespace is the plugin id; `core` is
//!   reserved for the kernel.
//! - **Plugins** ([`Plugin`]): a [`Manifest`] plus a `register` function that
//!   adds contributions through a [`Registrar`].
//! - **Contributions**: actions ([`ActionSpec`]), screens ([`ScreenSpec`]),
//!   slots ([`SlotSpec`]), settings ([`SettingSpec`]), fonts ([`FontSpec`]),
//!   file formats ([`FormatSpec`]), event subscriptions ([`EventHandler`]),
//!   key maps and translations (data in the PaperPaw config format).
//! - **Slots**: named UI roles such as [`builtin::slots::HOME`]. Any plugin
//!   may bind a screen to any slot; the user's configuration has the final
//!   word.
//! - **Commands** ([`Command`]): plugins never mutate kernel state directly.
//!   They read through [`Env`] (its parts are methods: `env.settings()`,
//!   `env.device()` and so on) and queue changes as commands in
//!   [`Commands`], which the kernel applies after the plugin returns.
//!   Plugins loaded from files are native code too, without a sandbox
//!   (ADR 0024).

#![cfg_attr(docsrs, feature(doc_cfg))]
// Without the `kernel` feature (as plugins build it) what only the
// kernel's methods use is left over; with it, as the kernel and CI build,
// unused code is found as everywhere.
#![cfg_attr(
    not(any(test, feature = "kernel")),
    allow(
        dead_code,
        unused_imports,
        reason = "what only the kernel's methods use is left over without the kernel feature"
    )
)]
#![no_std]

extern crate alloc;

mod action;
pub mod annotations;
pub mod ask;
pub mod bluetooth;
pub mod builtin;
pub mod chrome;
pub mod collections;
mod command;
mod commands;
pub mod contribution;
pub mod data;
pub mod decorations;
pub mod developer;
pub mod device_name;
mod env;
pub mod event;
pub mod files;
pub mod filters;
pub mod firmware;
mod fonts;
mod formats;
mod gesture;
pub mod help;
pub mod host;
#[cfg(test)]
mod test_kit;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use screen::ScreenMaker;
mod hyphenation;
pub mod icons;
mod id;
pub mod install;
pub mod keys;
pub mod layers;
pub mod library;
pub mod logo;
pub mod lookup;
pub mod memory;
mod memory_storage;
mod menus;
pub mod native;
pub mod numbers;
mod params;
pub mod pattern;
mod plugin;
mod plugins;
pub mod positions;
pub mod power;
pub mod presets;
pub mod random;
pub mod readers;
pub mod reading;
pub mod realtime;
pub mod replies;
mod screen;
pub mod secrets;
pub mod semantics;
pub mod served;
pub mod services;
pub mod setting;
mod settings;
pub mod setup;
#[cfg(all(
    any(test, feature = "kernel"),
    not(any(
        all(paperpaw_plugin, target_os = "none", target_arch = "riscv32"),
        paperpaw_table_host
    ))
))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod shared;
pub mod status;
pub mod time;
pub mod ui;
pub mod usage;
mod version;
pub mod voices;
pub mod web;
pub mod widgets;
pub mod wifi;
pub mod words;
pub mod work;

pub use action::{
    Action, ActionHandler, ActionHint, ActionOverride, ActionSpec, Actions, FixedAction,
    FixedHandler, Observer, ObserverSpec,
};
pub use command::{
    Broadcast, Command, HttpOptions, HttpRequest, MAX_CLIPBOARD, ServerResponse, SocketRead, Speech,
};
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use command::{HttpParts, ResponseParts};
/// The parameters of replies as they are written, for the kernel (plugins
/// read replies typed: see [`replies`]).
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use command::{http_params, server_params, socket_params, speech_params};
pub use commands::Commands;
pub use contribution::Registrar;
/// What a plugin added, for the kernel to apply: opaque to plugins, which
/// add through [`Registrar`]'s methods.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use contribution::item::Contribution;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use env::EnvParts;
pub use env::{Capabilities, Capability, Env, Metrics};
pub use event::{Event, EventData, EventHandler};
pub use fonts::{FontFace, FontHandle, FontSet, FontSpec, Fonts};
pub use formats::{
    COVER_FIT, FormatOpener, FormatPrepared, FormatSpec, Formats, IMAGE_FIT, OpenOptions, cover_fit,
};
pub use gesture::{Direction, Gesture, GestureKind};
pub use hyphenation::{Hyphenation, HyphenationSpec};
pub use id::{Id, InvalidId};
pub use memory_storage::{MemoryStorage, read_file};
pub use menus::{
    ItemState, ItemStateFn, MenuEntry, MenuItem, MenuNeed, MenuPlace, MenuSection, MenuView, Menus,
};
pub use params::Params;
pub use plugin::{Manifest, Plugin, Priority, RegisterError, SlotSpec, Uses};
pub use plugins::{PluginInfo, PluginKind, PluginProblem, PluginState, Plugins, RequiredFor};
pub use replies::{
    FolderChosen, HttpError, HttpProgress, HttpReply, ServerNotice, SocketNotice, Typed,
};
pub use screen::{
    CloseCause, Cx, FixedFactory, FixedScreen, Handled, Hint, HintVisibility, Instances, Output,
    Outside, Screen, ScreenFactory, ScreenKey, ScreenList, ScreenSpec, ShowsThrough, StackEntry,
};
pub use setting::{Setting, Text};
pub use settings::{
    Choices, Editor, Level, SettingCheck, SettingError, SettingExtra, SettingGroup,
    SettingGroupSpec, SettingKind, SettingSpec, SettingValue, Settings,
};
pub use version::{DATA_FORMATS, SDK_VERSION, Version};

/// Round shapes' geometry: angles as on a clock face, points around a
/// centre (see [`Canvas::fill_ring`], [`Canvas::draw_hand`]).
pub use paperpaw_graphics::round;
pub use paperpaw_graphics::{Bitmap, BitmapError, BitmapFormat, Blit, Canvas, Dither};
pub use paperpaw_i18n::{Arg, Translator};
pub use paperpaw_lang::{self as lang, Lang};
pub use paperpaw_platform::{Battery, SystemStatus, WallClock};
pub use paperpaw_platform::{
    ButtonState, Color, DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass,
    PixelFormat, Point, PointerEvent, PointerPhase, PointerTool, Rect, RefreshMode,
};
pub use paperpaw_platform::{DirEntry, NoStorage, Storage, StorageError};
pub use status::{
    StatusFn, StatusFrame, StatusItemSpec, StatusItems, StatusLineFn, StatusLineSpec,
    StatusSignatureFn, StatusValue,
};

/// The human-editable config format (see `paperpaw-config`), for plugins
/// that store their own data as text files.
pub mod config {
    pub use paperpaw_config::{
        Entry, ErrorKind, ParseError, Parser, Value, parse, quote, remove, set, set_all,
    };
}

/// QR codes, e.g. of an address to open on a phone (see `paperpaw-qr`;
/// [`ui::draw_qr`] draws one).
pub mod qr {
    pub use paperpaw_qr::QrCode;
}

/// The format-independent document model (see `paperpaw-document`).
pub mod document {
    pub use paperpaw_document::{
        Align, Block, BlockKind, Document, DocumentError, EmbeddedFont, ImageRef, Link, LinkKind,
        Locator, MAX_FAMILIES, MAX_INLINE_LANGS, Metadata, OffsetMap, PageProgression, Run,
        TextStyle, TocEntry, is_external,
    };
}

/// Page layout engine (see `paperpaw-layout`), usable by any reader
/// plugin. Unstable until 1.0: it still changes as readers need it, so it
/// comes only with the `unstable` feature.
#[cfg(feature = "unstable")]
pub mod layout {
    pub use paperpaw_layout::{
        BookStyles, Face, Hyphenate, LayoutContext, LineBidi, LineRun, Page, PageImage, PageLine,
        PageNote, Style, Typesetter, VERSION, layout_page, line_start,
    };
    /// Lines broken as the layout breaks them (see `paperpaw-text`): for
    /// laying out text of one's own the way pages are, e.g. a note.
    pub use paperpaw_text::{Line, wrap_lines, wrap_lines_hyphenated, wrap_lines_kerned};
}

/// Streaming XML/XHTML tokenizer (see `paperpaw-xml`) for format plugins.
pub mod xml {
    pub use paperpaw_xml::{Attrs, Token, Tokenizer, decode, escape, local_name};
}

/// Streaming JPEG, PNG and GIF decoding to gray or colour (see
/// `paperpaw-image`).
pub mod image {
    pub use paperpaw_image::{
        AlphaFile, AlphaWriter, ColorFile, ColorWriter, Decoded, Downscaler, Fit, Format, GrayFile,
        GrayWriter, ImageError, ImageFile, ImageSink, PngWriter, Source, decode, dimensions, luma,
        over_white,
    };

    use crate::document::ImageRef;
    use paperpaw_platform::Storage;

    /// Reads a file in storage as an image source.
    struct FileSource<'a> {
        storage: &'a dyn Storage,
        path: &'a str,
        offset: u64,
    }

    impl Source for FileSource<'_> {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, ImageError> {
            let n = self
                .storage
                .read_at(self.path, self.offset, buf)
                .map_err(|_| ImageError::Io)?;
            self.offset += n as u64;
            Ok(n)
        }
    }

    /// Decodes the JPEG, PNG or GIF file `from` into the file `to`, no
    /// larger than `fit` allows, keeping a PNG's transparency
    /// ([`AlphaFile`]; others are opaque), for
    /// [`ui::draw_image_over`](crate::ui::draw_image_over): a picture over
    /// what is drawn (a page under a sleep picture, a sticker on a
    /// widget). Why not where it is no image PaperPaw reads (another
    /// format, an interlaced PNG, a damaged file).
    pub fn picture_with_alpha(
        storage: &dyn Storage,
        from: &str,
        to: &str,
        fit: Fit,
    ) -> Result<ImageRef, ImageError> {
        let mut head = [0u8; 8];
        let n = storage
            .read_at(from, 0, &mut head)
            .map_err(|_| ImageError::Io)?;
        let format = Format::detect(&head[..n]).ok_or(ImageError::Unsupported("format"))?;
        let mut source = FileSource {
            storage,
            path: from,
            offset: 0,
        };
        let mut writer = AlphaWriter::new(storage, to);
        decode(format, &mut source, fit, &mut writer).and_then(|()| writer.finish())?;
        AlphaFile::open(storage, to)
            .map(|file| ImageRef {
                path: file.path,
                width: file.width,
                height: file.height,
            })
            .ok_or(ImageError::Io)
    }

    /// Decodes the JPEG or PNG file `from` into the gray file `to`, no
    /// larger than `fit` allows, for
    /// [`ui::draw_image`](crate::ui::draw_image); `None` if it is no image
    /// PaperPaw reads.
    pub fn gray_from_file(
        storage: &dyn Storage,
        from: &str,
        to: &str,
        fit: Fit,
    ) -> Option<ImageRef> {
        picture_from_file(storage, from, to, fit, false)
    }

    /// [`gray_from_file`], or in colour (for colour screens) with `color`.
    pub fn picture_from_file(
        storage: &dyn Storage,
        from: &str,
        to: &str,
        fit: Fit,
        color: bool,
    ) -> Option<ImageRef> {
        let mut head = [0u8; 8];
        let n = storage.read_at(from, 0, &mut head).ok()?;
        let format = Format::detect(&head[..n])?;
        let mut source = FileSource {
            storage,
            path: from,
            offset: 0,
        };
        picture_from(storage, format, &mut source, to, fit, color)
    }

    /// A picture a plugin carries in its code (`include_bytes!`), a JPEG,
    /// PNG or GIF, decoded into the file `to` as [`picture_from_file`]
    /// decodes one, a few rows at a time: drawn from there in bands, it
    /// is never whole in memory.
    pub fn picture_from_bytes(
        storage: &dyn Storage,
        bytes: &[u8],
        to: &str,
        fit: Fit,
        color: bool,
    ) -> Option<ImageRef> {
        let format = Format::detect(bytes.get(..8).unwrap_or(bytes))?;
        let mut source = bytes;
        picture_from(storage, format, &mut source, to, fit, color)
    }

    fn picture_from(
        storage: &dyn Storage,
        format: Format,
        source: &mut dyn Source,
        to: &str,
        fit: Fit,
        color: bool,
    ) -> Option<ImageRef> {
        if color {
            let mut writer = ColorWriter::new(storage, to);
            decode(format, source, fit, &mut writer)
                .and_then(|()| writer.finish())
                .ok()?;
        } else {
            let mut writer = GrayWriter::new(storage, to);
            decode(format, source, fit, &mut writer)
                .and_then(|()| writer.finish())
                .ok()?;
        }
        ImageFile::open(storage, to).map(|file| ImageRef {
            path: file.path,
            width: file.width,
            height: file.height,
        })
    }
}

/// Text and font helpers (see `paperpaw-text`), e.g. reading a font
/// file's family, or how glyphs are inked ([`ToneCurve`](text::ToneCurve)).
pub mod text {
    pub use paperpaw_text::{Around, OUTLINES, ToneCurve, bidi, sfnt};
}

/// Hyphenation with compiled patterns (see `paperpaw-hyphen`).
pub mod hyphen {
    pub use paperpaw_hyphen::{BLOCK, InvalidPatterns, Patterns, compile};
}

/// Streaming ZIP reader (see `paperpaw-zip`) for ZIP-based formats.
/// Unstable until 1.0, so it comes only with the `unstable` feature.
#[cfg(feature = "unstable")]
pub mod zip {
    pub use paperpaw_zip::{Entry, EntryReader, Method, ZipArchive, ZipError};
}

/// JSON reading and writing without building a tree (see
/// `paperpaw-json`), e.g. for web services.
pub mod json {
    pub use paperpaw_json::{
        Value, number, object, raw_member, stream, string, string_array, text, unquote,
    };
}

/// Hashes and hex (see `paperpaw-digest`): FNV-1a for names of files kept
/// per book, MD5, SHA-1 and SHA-256 for what services and files ask.
pub mod digest {
    pub use paperpaw_digest::{Fnv1a64, fnv1a64, hex, md5, sha1, sha256};
}
pub use paperpaw_text::{Font, FontError, LineMetrics};
