//! The PaperPaw plugin SDK.
//!
//! PaperPaw is a small kernel plus plugins. Every screen, action, setting,
//! key map and translation (including those PaperPaw ships with) is
//! contributed by a plugin through this SDK. Anything a built-in plugin does,
//! a community plugin can do too, and replace it.
//!
//! # Status
//!
//! This is a beta (0.6.0-beta.3). The SDK may still change before 1.0;
//! the CHANGELOG lists every breaking change with what replaces it.
//! A plugin with code is built for a plugin interface
//! ([`native::INTERFACE`]) and rebuilt when a release raises it.
//!
//! # Concepts
//!
//! - **Ids** ([`Id`]): every contribution has a namespaced id such as
//!   `diagnostics.screen`. The namespace is the plugin id; `core` is
//!   reserved for the kernel.
//! - **Plugins** (`Plugin`): a [`Manifest`] plus a `register` function that
//!   adds contributions through a `Items`.
//! - **Contributions**: actions (`ActionSpec`),
//!   slots ([`SlotSpec`]), settings ([`SettingSpec`]), fonts ([`FontSpec`]),
//!   file formats ([`FormatSpec`]), event subscriptions,
//!   key maps and translations (data in the PaperPaw config format).
//! - **Slots**: named UI roles such as [`builtin::slots::HOME`]. Any plugin
//!   may bind a screen to any slot; the user's configuration has the final
//!   word.
//! - **Commands** ([`Command`]): plugins never mutate kernel state directly.
//!   They read through `Env` (its parts are methods: `env.settings()`,
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
// Without `kernel` the old plugin API is the crate's own (#244, slice D0): most of it has
// no user then.
#![cfg_attr(not(any(test, feature = "kernel")), allow(dead_code, unused_imports))]

extern crate alloc;

mod action;
// facade: annotations (#244, D0)
#[cfg(any(test, feature = "kernel"))]
pub mod annotations;
#[cfg(not(any(test, feature = "kernel")))]
#[path = "annotations/mod.rs"]
mod annotations_old;
/// What interface-7 code uses of the module; the rest of it is the kernel's (feature
/// `kernel`).
#[cfg(not(any(test, feature = "kernel")))]
pub mod annotations {
    pub use crate::annotations_old::Kind;
    pub(crate) use crate::annotations_old::*;
}
// end of facade: annotations
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod bluetooth;
#[cfg(not(any(test, feature = "kernel")))]
mod bluetooth;
// facade: builtin (#244, D0)
#[cfg(any(test, feature = "kernel"))]
pub mod builtin;
#[cfg(not(any(test, feature = "kernel")))]
#[path = "builtin/mod.rs"]
mod builtin_old;
/// What interface-7 code uses of the module; the rest of it is the kernel's (feature
/// `kernel`).
#[cfg(not(any(test, feature = "kernel")))]
pub mod builtin {
    pub(crate) use crate::builtin_old::*;
    pub use crate::builtin_old::{
        CONFIG_FILE, group_sections, groups, menus, params, settings, slots,
    };
}
// end of facade: builtin
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod collections;
#[cfg(not(any(test, feature = "kernel")))]
mod collections;
mod command;
mod commands;
#[cfg(test)]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod contribution;
#[cfg(not(test))]
mod contribution;
pub mod data;
// facade: decorations (#244, D0)
#[cfg(test)]
pub mod decorations;
#[cfg(not(test))]
#[path = "decorations.rs"]
mod decorations_old;
/// What interface-7 code uses of the module; the rest of it is the kernel's (feature
/// `kernel`).
#[cfg(not(test))]
pub mod decorations {
    pub(crate) use crate::decorations_old::*;
    pub use crate::decorations_old::{AS_IS, Decoration, LineKind, NONE, THICKNESS, Tone};
}
// end of facade: decorations
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod developer;
#[cfg(not(any(test, feature = "kernel")))]
mod developer;
pub mod device_name;
mod env;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod event;
#[cfg(not(any(test, feature = "kernel")))]
mod event;
pub mod files;
// facade: filters (#244, D0)
#[cfg(any(test, feature = "kernel"))]
pub mod filters;
#[cfg(not(any(test, feature = "kernel")))]
#[path = "filters.rs"]
mod filters_old;
/// What interface-7 code uses of the module; the rest of it is the kernel's (feature
/// `kernel`).
#[cfg(not(any(test, feature = "kernel")))]
pub mod filters {
    pub use crate::filters_old::Verdict;
    pub(crate) use crate::filters_old::*;
}
// end of facade: filters
pub mod firmware;
mod float_text;
mod fonts;
mod formats;
mod gesture;
// facade: help (#244, D0)
#[cfg(test)]
pub mod help;
#[cfg(not(test))]
#[path = "help.rs"]
mod help_old;
/// What interface-7 code uses of the module; the rest of it is the kernel's (feature
/// `kernel`).
#[cfg(not(test))]
pub mod help {}
// end of facade: help
mod hyphenation;
pub mod icons;
mod id;
// facade: install (#244, D0)
#[cfg(test)]
pub mod install;
#[cfg(not(test))]
#[path = "install/mod.rs"]
mod install_old;
/// What interface-7 code uses of the module; the rest of it is the kernel's (feature
/// `kernel`).
#[cfg(not(test))]
pub mod install {
    /// The kernel's and the store's: whether a data plugin fits a kernel.
    #[cfg(feature = "kernel")]
    pub use crate::install_old::fits_sdk;
    pub use crate::install_old::{
        Digest, MAX_FILES, Package, PackageFile, is_plugin_name, is_safe_path,
    };
}
// end of facade: install
pub mod keys;
// facade: library (#244, D0)
#[cfg(test)]
pub mod library;
#[cfg(not(test))]
#[path = "library/mod.rs"]
mod library_old;
/// What interface-7 code uses of the module; the rest of it is the kernel's (feature
/// `kernel`).
#[cfg(not(test))]
pub mod library {
    /// The kernel's: the event that says a book changed.
    #[cfg(feature = "kernel")]
    pub use crate::library_old::changed_book;
    pub(crate) use crate::library_old::*;
    pub use crate::library_old::{Fingerprint, kept_path};
}
// end of facade: library
// facade: logo (#244, D0)
#[cfg(test)]
pub mod logo;
#[cfg(not(test))]
#[path = "logo.rs"]
mod logo_old;
/// What interface-7 code uses of the module; the rest of it is the kernel's (feature
/// `kernel`).
#[cfg(not(test))]
pub mod logo {
    pub(crate) use crate::logo_old::*;
}
// end of facade: logo
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
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod power;
#[cfg(not(any(test, feature = "kernel")))]
mod power;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod presets;
#[cfg(not(any(test, feature = "kernel")))]
mod presets;
pub mod random;
// facade: readers (#244, D0)
#[cfg(test)]
pub mod readers;
#[cfg(not(test))]
#[path = "readers.rs"]
mod readers_old;
/// What interface-7 code uses of the module; the rest of it is the kernel's (feature
/// `kernel`).
#[cfg(not(test))]
pub mod readers {}
// end of facade: readers
// facade: reading (#244, D0)
#[cfg(any(test, feature = "kernel"))]
pub mod reading;
#[cfg(not(any(test, feature = "kernel")))]
#[path = "reading/mod.rs"]
mod reading_old;
/// What interface-7 code uses of the module; the rest of it is the kernel's (feature
/// `kernel`).
#[cfg(not(any(test, feature = "kernel")))]
pub mod reading {
    pub(crate) use crate::reading_old::*;
    pub use crate::reading_old::{PageIndex, WordBox};
}
// end of facade: reading
pub mod realtime;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod replies;
#[cfg(not(any(test, feature = "kernel")))]
mod replies;
mod screen;
pub mod secrets;
pub mod semantics;
pub mod served;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod services;
#[cfg(not(any(test, feature = "kernel")))]
mod services;
pub mod setting;
mod titled;
/// One shared sorter, stable or in place without memory: use it instead of
/// `slice::sort_by`, `slice::sort_unstable` and their kin, which compile a
/// copy per element type (see `paperpaw_sort`).
pub use paperpaw_sort as sort;
mod settings;
#[cfg(test)]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod setup;
#[cfg(not(test))]
mod setup;
mod shared_str;
mod sockets;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod sound;
#[cfg(not(any(test, feature = "kernel")))]
mod sound;
// facade: status (#244, D0)
#[cfg(test)]
pub mod status;
#[cfg(not(test))]
#[path = "status.rs"]
mod status_old;
/// What interface-7 code uses of the module; the rest of it is the kernel's (feature
/// `kernel`).
#[cfg(not(test))]
pub mod status {
    pub use crate::status_old::item_ids;
}
// end of facade: status
// facade: time (#244, D0)
#[cfg(test)]
pub mod time;
#[cfg(not(test))]
#[path = "time.rs"]
mod time_old;
/// What interface-7 code uses of the module; the rest of it is the kernel's (feature
/// `kernel`).
#[cfg(not(test))]
pub mod time {

    pub use crate::time_old::{
        CLOCK_12H, LocalTime, TIME_ZONES, TimeZone, UnixTime, civil_from_days, day_of_year,
        days_from_civil, days_in_month, days_in_year, iso_week, weekday, weekday_of,
    };
}
// end of facade: time
// facade: ui (#244, D0)
#[cfg(test)]
pub mod ui;
#[cfg(not(test))]
#[path = "ui/mod.rs"]
mod ui_old;
/// What interface-7 code uses of the module; the rest of it is the kernel's (feature
/// `kernel`).
#[cfg(not(test))]
pub mod ui {
    /// The kernel's: the class of the display that events carry.
    #[cfg(feature = "kernel")]
    pub use crate::ui_old::ScreenClass;
    pub(crate) use crate::ui_old::*;
    pub use crate::ui_old::{Ink, step_focus};
    /// What interface-7 code uses of it.
    pub mod reorder {
        pub use crate::ui_old::reorder::moved;
    }
}
// end of facade: ui
pub mod usage;
mod version;
pub mod web;
// facade: widgets (#244, D0)
#[cfg(test)]
pub mod widgets;
#[cfg(not(test))]
#[path = "widgets/mod.rs"]
mod widgets_old;
/// What interface-7 code uses of the module; the rest of it is the kernel's (feature
/// `kernel`).
#[cfg(not(test))]
pub mod widgets {

    pub use crate::widgets_old::{
        CellSpan, ColourSpec, PAPER_LIGHT_GRAY, PAPER_NONE, PAPER_NORMAL, categories, colour_spec,
    };
    /// What interface-7 code uses of it.
    pub mod grid {

        pub use crate::widgets_old::grid::{
            Cells, Geometry, Item, MAX_COLUMNS, MAX_ROWS, What, columns_for, dragged, free_place,
            parse, write,
        };
    }
}
// end of facade: widgets
pub mod wifi;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod words;
#[cfg(not(any(test, feature = "kernel")))]
mod words;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub mod work;
#[cfg(not(any(test, feature = "kernel")))]
mod work;

#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use command::{
    Broadcast, Command, HttpOptions, HttpRequest, MAX_CLIPBOARD, ServerResponse, SocketRead, Speech,
};
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) use command::{
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
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use commands::Commands;
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) use commands::Commands;
/// What a plugin added, for the kernel to apply: opaque to plugins, which
/// add through `Items`'s methods.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use env::{Capabilities, Capability, Metrics};
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) use env::{Capabilities, Capability, Metrics};
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use event::{Event, EventData};
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) use event::{Event, EventData};
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use fonts::{BuiltInFont, BuiltInFonts};
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) use fonts::{FontFace, FontHandle, FontSet, FontSpec, Fonts};
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use fonts::{FontFace, FontHandle, FontSet, FontSpec, Fonts, RoleMemo};
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use fonts::{OpenError as FontOpenError, open_file as open_font_file};
pub use formats::{COVER_FIT, IMAGE_FIT, OpenOptions, cover_fit};
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use formats::{FormatOpener, FormatPrepared, FormatSpec, Formats};
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) use formats::{FormatOpener, FormatPrepared, FormatSpec, Formats};
pub use gesture::{Direction, Gesture, GestureKind};
#[cfg(feature = "kernel")]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use hyphenation::{CardTable, plugin_wins};
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use hyphenation::{Hyphenation, HyphenationSpec, Hyphenator};
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) use hyphenation::{Hyphenation, HyphenationSpec, Hyphenator};
/// A text kept in the program or owned, in two words, for the kernel's tables.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use id::Text as ShortText;
pub use id::{Id, IdError, InvalidId};
pub use memory_storage::{MemoryStorage, read_file};
pub use menus::MenuNeed;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use params::Params;
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) use params::Params;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use plugin::{Manifest, Priority, RegisterError, SlotSpec, Uses};
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) use plugin::{Manifest, Priority, RegisterError, SlotSpec, Uses};
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use plugins::requires;
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) use plugins::requires;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use plugins::{
    MAX_REQUESTED_PLUGINS, ManifestRef, PluginInfo, PluginKind, PluginProblem, PluginState,
    Plugins, RequiredFor,
};
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) use plugins::{
    MAX_REQUESTED_PLUGINS, ManifestRef, PluginInfo, PluginKind, PluginProblem, PluginState,
    Plugins, RequiredFor,
};
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use replies::{
    FolderChosen, HttpError, HttpProgress, HttpReply, ServerNotice, SocketNotice, Typed,
};
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) use replies::{
    FolderChosen, HttpError, HttpProgress, HttpReply, ServerNotice, SocketNotice, Typed,
};
pub use screen::Handled;
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use screen::{
    CloseCause, Hint, HintVisibility, Instances, Output, Outside, Pane, SCREEN_REVISION, ScreenKey,
    ShowsThrough, StackEntry,
};
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) use screen::{
    CloseCause, Hint, HintVisibility, Instances, Output, Outside, Pane, SCREEN_REVISION, ScreenKey,
    ShowsThrough, StackEntry,
};
pub use setting::{Setting, Text};
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use settings::Settings;
#[cfg(not(any(test, feature = "kernel")))]
pub(crate) use settings::Settings;
pub use settings::{
    Choices, Editor, Level, SettingCheck, SettingError, SettingExtra, SettingGroup,
    SettingGroupSpec, SettingKind, SettingSpec, SettingValue,
};
/// Settings a plugin loaded from the card describes on demand, for the
/// kernel: of each only its default and flags are kept.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub use settings::{DescribeError, Described, DescribedSettings, Pick, SettingMemo};
pub use shared_str::Str;
pub use sockets::{
    CertificateHash, Distrust, KeyHash, MAX_PINS, MAX_TLS_IDLE_MS, MAX_TLS_TIMEOUT_MS, SocketError,
    Tls, Trust,
};
pub use titled::{TitleEntry, Titled};
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

/// The human-editable config format (see `paperpaw-config`), for plugins
/// that store their own data as text files.
pub mod config {
    pub use paperpaw_config::{
        Entry, ErrorKind, ParseError, Parser, Value, parse, quote, remove, set, set_all,
    };
}

/// QR codes, e.g. of an address to open on a phone (see `paperpaw-qr`;
/// `ui::draw_qr` draws one).
pub mod qr {
    pub use paperpaw_qr::QrCode;
}

/// The format-independent document model (see `paperpaw-document`).
pub mod document {
    pub use paperpaw_document::{
        Align, Annotation, AnnotationKind, Block, BlockKind, Document, DocumentError, EditError,
        EmbeddedFont, ImageRef, LETTER_SPACING_STEPS, LineStyle, Link, LinkKind, Locator,
        MAX_COLUMNS, MAX_FAMILIES, MAX_INLINE_LANGS, MAX_LETTER_SPACING, MIN_LETTER_SPACING,
        Metadata, OffsetMap, PageProgression, Run, TableCell, TableRow, TextEdit, TextStyle,
        TocEntry, VerticalAlign, is_external,
    };
}

/// Page layout engine (see `paperpaw-layout`), usable by any reader
/// plugin. Unstable until 1.0: it still changes as readers need it, so it
/// comes only with the `unstable` feature.
#[cfg(feature = "unstable")]
pub mod layout {
    pub use paperpaw_layout::{
        BookStyles, Face, Hyphenate, LayoutContext, LayoutOptions, LineBidi, LineRun, Page,
        PageImage, PageLine, PageNote, PageRule, PieceStarts, ScriptOptions, Style, Typesetter,
        VERSION, layout_page, line_start,
    };
    /// Lines broken as the layout breaks them (see `paperpaw-text`): for
    /// laying out text of one's own the way pages are, e.g. a note.
    pub use paperpaw_text::{Line, wrap_lines, wrap_lines_hyphenated, wrap_lines_kerned};
}

/// Streaming XML/XHTML tokenizer (see `paperpaw-xml`) for format plugins.
pub mod xml {
    pub use paperpaw_xml::{Attrs, Token, Tokenizer, decode, escape, local_name, try_decode};
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
    /// `ui::draw_image_over`: a picture over
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
    /// `ui::draw_image`; `None` if it is no image
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
    pub use paperpaw_text::{Around, OUTLINES, TextInk, ToneCurve, bidi, is_invisible, sfnt};
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
pub use paperpaw_text::{Font, FontError, LineMetrics, PackLimits};
