//! Ids of the contributions the kernel itself provides (namespace `core`).
//!
//! Plugins use these to bind screens to the standard slots, to handle the
//! standard navigation actions and to read the global settings.

/// The kernel's own plugin id; it cannot be switched off.
pub const CORE_PLUGIN: crate::Id = crate::Id::new_static("core");

/// Standard actions. Navigation actions have no global handler; screens
/// interpret them. `back`, `home` and `toggle-night-mode` have global
/// handlers, which screens may still override by handling the action.
pub mod actions {
    use crate::Id;

    /// Does nothing: bound to an input, it stops the input from doing
    /// anything there.
    pub const NOTHING: Id = Id::new_static("core.nothing");

    pub const NEXT_PAGE: Id = Id::new_static("core.next-page");
    /// "Forward" in the widest sense: next list entry, increase a value
    /// being edited; screens without such a meaning treat it as the next
    /// page. Lets devices with very few buttons operate everything.
    pub const NEXT: Id = Id::new_static("core.next");
    /// The opposite of [`NEXT`].
    pub const PREVIOUS: Id = Id::new_static("core.previous");
    pub const PREV_PAGE: Id = Id::new_static("core.prev-page");
    pub const UP: Id = Id::new_static("core.up");
    pub const DOWN: Id = Id::new_static("core.down");
    pub const LEFT: Id = Id::new_static("core.left");
    pub const RIGHT: Id = Id::new_static("core.right");
    pub const CONFIRM: Id = Id::new_static("core.confirm");
    /// Select held: what a long press does where the focus is, where a
    /// screen takes it (a widget's own, see
    /// [`WidgetTarget::hold`](crate::widgets::WidgetTarget::hold)); else
    /// the menu, as before.
    pub const LONG_CONFIRM: Id = Id::new_static("core.long-confirm");
    pub const MENU: Id = Id::new_static("core.menu");
    pub const BACK: Id = Id::new_static("core.back");
    pub const HOME: Id = Id::new_static("core.home");
    pub const TOGGLE_NIGHT_MODE: Id = Id::new_static("core.toggle-night-mode");
    /// Opens the library slot.
    pub const LIBRARY: Id = Id::new_static("core.library");
    /// *Open with…* in the book menu ([`menus::BOOK`](super::menus::BOOK)):
    /// asks which reading app opens the book this time
    /// ([`readers::ask_once`](crate::readers::ask_once)).
    pub const OPEN_WITH: Id = Id::new_static("core.open-with");
    /// Opens the settings slot.
    pub const SETTINGS: Id = Id::new_static("core.settings");
    /// Saves what the screen shows as a picture, in `Screenshots` beside
    /// the library ([`Command::Screenshot`](crate::Command::Screenshot)).
    pub const SCREENSHOT: Id = Id::new_static("core.screenshot");
    /// Removes what plugins kept for books no longer on the storage
    /// ([`Registrar::add_book_data`](crate::Registrar::add_book_data)),
    /// e.g. deleted from a computer.
    pub const CLEAN_UP: Id = Id::new_static("core.clean-up");
    /// Opens the tools menu ([`menus::TOOLS`](super::menus::TOOLS)).
    pub const TOOLS: Id = Id::new_static("core.tools");
    /// Puts the device to sleep: shows the sleep screen (slot
    /// [`SLEEP`](super::slots::SLEEP)), then the shell sleeps until it is
    /// woken (see [`Capabilities::sleep`](crate::Capabilities)). Where the system sleeps on
    /// its own (Android), nothing sends it.
    pub const SLEEP: Id = Id::new_static("core.sleep");
    /// Opens the quick panel from the top of the screen: the menu
    /// [`QUICK`](super::menus::QUICK) as a panel of controls (night mode,
    /// light, turning the screen, …). A swipe down from the top edge
    /// opens it.
    pub const QUICK_PANEL: Id = Id::new_static("core.quick-panel");
    /// Refreshes the whole screen with a flash, clearing what e-ink left
    /// of earlier pictures (ghosting).
    pub const REFRESH_SCREEN: Id = Id::new_static("core.refresh-screen");
}

/// Standard slots.
pub mod slots {
    use crate::Id;

    /// The start screen. The default is a dashboard, but any screen can be
    /// bound here: a file browser, a bookshelf, a single "continue" button.
    pub const HOME: Id = Id::new_static("core.home");
    /// Shows a book. Opened with the parameter [`PATH`](super::params::PATH).
    pub const READER: Id = Id::new_static("core.reader");
    /// Browses the library.
    pub const LIBRARY: Id = Id::new_static("core.library");
    /// Shows a menu (an overlay). Opened with the parameter
    /// [`MENU`](super::params::MENU).
    pub const MENU: Id = Id::new_static("core.menu");
    /// Shows and edits settings; with the parameter
    /// [`ID`](super::params::ID), the settings of that group, e.g. a
    /// widget's own (see [`widgets`](crate::widgets)); with
    /// [`FOCUS`](super::params::FOCUS), the page of that setting with
    /// the focus on it.
    pub const SETTINGS: Id = Id::new_static("core.settings");
    /// The first-run setup (see [`setup`](crate::setup)): shown once on
    /// the first start, and again when asked.
    pub const SETUP: Id = Id::new_static("core.setup");
    /// Looks up a word (an overlay): opened with the parameters
    /// [`WORD`](super::params::WORD) and, if known,
    /// [`LANGUAGE`](super::params::LANGUAGE). The standard dictionary
    /// binds here; translators or encyclopedias can too.
    pub const LOOKUP: Id = Id::new_static("core.lookup");
    /// Looks a word up online (an overlay), with the same parameters as
    /// [`LOOKUP`]: an encyclopedia, an online dictionary, a translator.
    /// Dictionaries offer it beside their own entries when a screen is
    /// bound here ([`Env::has_slot`](crate::Env::has_slot)) and there is
    /// a network.
    pub const LOOKUP_ONLINE: Id = Id::new_static("core.lookup-online");
    /// Shows a menu as a panel of controls for quick changes (an overlay):
    /// parameter [`MENU`](super::params::MENU). The reader opens its
    /// font and layout menu this way.
    pub const PANEL: Id = Id::new_static("core.panel");
    /// Asks for text (usually an overlay with an on-screen keyboard).
    /// Parameters: [`TITLE`](super::params::TITLE) (shown above the
    /// field, already translated), [`TEXT`](super::params::TEXT) (the
    /// initial text), [`MODE`](super::params::MODE) and
    /// [`REPLY`](super::params::REPLY). When the text is confirmed the
    /// screen closes and emits the event named by `reply` with all of its
    /// parameters, `text` set to what was entered, so the asking plugin
    /// can pass along anything it needs to know (e.g. which setting). With
    /// [`LIVE`](super::params::LIVE) it emits it on each change too, with
    /// [`TYPING`](super::params::TYPING) set. Bind another screen here for
    /// a different keyboard, handwriting or voice.
    pub const KEYBOARD: Id = Id::new_static("core.keyboard");
    /// Asks for text typed on another device, e.g. a phone that opens an
    /// address shown as a QR code: handy for passwords and keys. Same
    /// parameters as [`KEYBOARD`]; the text comes back as the event named
    /// by [`REPLY`](super::params::REPLY). The keyboard offers it where a
    /// screen is bound here and there is a network.
    pub const REMOTE_KEYBOARD: Id = Id::new_static("core.remote-keyboard");
    /// Opened by PaperPaw when a computer speaks over the cable (USB)
    /// while nothing serves there: the screen bound here serves it on
    /// [`CABLE_PORT`](crate::served::CABLE_PORT) while it is open, and asks
    /// the reader before anything is written (the `transfer` plugin's).
    pub const CABLE: Id = Id::new_static("core.cable");
    /// Shows a web page: parameter [`URL`](super::params::URL), and
    /// [`ID`](super::params::ID) different for every request (opening a
    /// slot with the parameters it is open with only brings it forward).
    /// The reader opens links out of a book this way, if a browser is
    /// bound here ([`Env::has_slot`](crate::Env::has_slot)).
    pub const BROWSER: Id = Id::new_static("core.browser");
    /// The Bluetooth page: the plugin whose screen is bound here is the
    /// Bluetooth manager, which sees every bond and decides when remotes
    /// connect (see [`bluetooth`](crate::bluetooth)).
    pub const BLUETOOTH: Id = Id::new_static("core.bluetooth");
    /// Chooses a folder, browsing the device's folders (with a way to make
    /// a new one). Parameters: [`ID`](super::params::ID), a setting the
    /// folder is written to, or [`REPLY`](super::params::REPLY), an event
    /// emitted with all of the parameters and [`PATH`](super::params::PATH)
    /// set to the folder; [`TEXT`](super::params::TEXT), where to start.
    pub const CHOOSE_FOLDER: Id = Id::new_static("core.choose-folder");
    /// Edits a list of widgets shown above each other (a
    /// [`WidgetPlace`](crate::widgets::WidgetPlace) of kind
    /// [`Stack`](crate::widgets::PlaceKind::Stack), e.g. the sleep
    /// screen's): parameter [`ID`](super::params::ID), the setting that
    /// lists them. The kernel's [`screens::WIDGET_LIST`](super::screens::WIDGET_LIST)
    /// unless a plugin binds another; name it as the setting's editor
    /// ([`SettingExtra::editor_slot`](crate::SettingExtra::editor_slot)).
    pub const EDIT_WIDGETS: Id = Id::new_static("core.edit-widgets");
    /// Picks one of the values of a choice or language setting, on a page
    /// of its own: parameter [`ID`](super::params::ID), the setting. The
    /// settings plugin binds here; menus and the setup open it for a
    /// setting with more values than two.
    pub const CHOOSE_VALUE: Id = Id::new_static("core.choose-value");
    /// Asks to choose one of a few options, e.g. what to do with a file
    /// or whether to delete it (an overlay at the bottom): asked with
    /// [`ask::choice`](crate::ask::choice), answered with a
    /// [`Picked`](crate::ui::choice::Picked). Every plugin's questions
    /// come here, so a screen bound here asks them all another way.
    pub const CHOICE: Id = Id::new_static("core.choice");
    /// Asks yes or no, e.g. whether to delete a book (a box in the middle
    /// of the screen): asked with [`ask::confirm`](crate::ask::confirm),
    /// answered with a [`Confirmed`](crate::replies::Confirmed), nothing
    /// on *Back* or a tap outside. Every plugin's yes or no questions come
    /// here, so a screen bound here asks them all another way (see
    /// [`ui::confirm`](crate::ui::confirm) for its parameters).
    pub const CONFIRM: Id = Id::new_static("core.confirm");
    /// Shows what is known of a book: parameter [`PATH`](super::params::PATH).
    pub const BOOK_INFO: Id = Id::new_static("core.book-info");
    /// The contents of the book being read: parameters
    /// [`PATH`](super::params::PATH) and, where it is read,
    /// [`SECTION`](super::params::SECTION) and
    /// [`OFFSET`](super::params::OFFSET). Choosing an entry closes it and
    /// sends [`events::GOTO`](super::events::GOTO). The reader opens it;
    /// bind another screen here for contents that look different.
    pub const TOC: Id = Id::new_static("core.toc");
    /// Where a text is found in a book: parameters
    /// [`PATH`](super::params::PATH) and [`TEXT`](super::params::TEXT),
    /// what to find. Choosing a place closes it and sends
    /// [`events::GOTO`](super::events::GOTO) with the text found marked.
    pub const SEARCH: Id = Id::new_static("core.search");
    /// A book's bookmarks, highlights and notes: parameter
    /// [`PATH`](super::params::PATH). Choosing one closes it and sends
    /// [`events::GOTO`](super::events::GOTO).
    pub const ANNOTATIONS: Id = Id::new_static("core.annotations");
    /// A note shown over the page (an overlay), from a note mark followed:
    /// parameters [`TEXT`](super::params::TEXT), the note,
    /// [`PATH`](super::params::PATH), the book, and
    /// [`URL`](super::params::URL), the link; going to the note sends
    /// [`events::FOLLOW_LINK`](super::events::FOLLOW_LINK).
    pub const FOOTNOTE: Id = Id::new_static("core.footnote");
    /// Skimming through the book being read (an overlay): opened with the
    /// parameters of [`events::Skimmed`](super::events::Skimmed) (where the
    /// book is, where its chapters begin). It asks the reader to move with
    /// [`events::SKIM`](super::events::SKIM) and hears where the book is
    /// then with [`events::SKIMMED`](super::events::SKIMMED).
    pub const SKIM: Id = Id::new_static("core.skim");
    /// Shows a short message ([`Command::Notify`](crate::Command::Notify))
    /// over whatever is open, for a few seconds: parameter
    /// [`TEXT`](super::params::TEXT). It gets no input and is closed by
    /// the kernel; bind another screen here for messages that look
    /// different. Its [`region`](crate::Screen::region) is where it shows.
    pub const NOTICE: Id = Id::new_static("core.notice");
    /// Translates text (an overlay): parameters
    /// [`TEXT`](super::params::TEXT) and, if known,
    /// [`LANGUAGE`](super::params::LANGUAGE) (the text's). The reader
    /// offers it for selected text when a screen is bound here and there
    /// is a network; the `translate` plugin binds DeepL, other services
    /// can take its place.
    pub const TRANSLATE: Id = Id::new_static("core.translate");
    /// Shown while the device sleeps (see
    /// [`actions::SLEEP`](super::actions::SLEEP)): a cover, a picture, a
    /// message. It closes itself when the device wakes
    /// ([`events::RESUME`](super::events::RESUME)).
    pub const SLEEP: Id = Id::new_static("core.sleep");
}

/// Standard menus. Plugins add entries with
/// [`Registrar::add_menu_entry`](crate::Registrar::add_menu_entry); users can
/// redefine any menu in `[menus]`.
///
/// A menu named like a setting with choices (e.g. `core.reading-font`)
/// holds more ways to choose: where its options are listed, its entries
/// follow them (e.g. *More fonts*, to download one).
pub mod menus {
    use crate::Id;

    /// The menu shown when a screen has no menu of its own: short and
    /// always the same (home, library, tools, night mode, settings).
    pub const MAIN: Id = Id::new_static("core.main");
    /// The tools plugins bring (statistics, catalogs, files, transfers,
    /// …): plugins add their entries here rather than to [`MAIN`], so the
    /// main menu stays short however many there are. Also the home
    /// screen's grid of tools.
    pub const TOOLS: Id = Id::new_static("core.tools");
    /// What there is to open besides the tools: a plugin's own app (a
    /// game, a notebook). What a launcher, a drawer or a palette lists:
    /// these and the [`TOOLS`]; the shipped home screen's grid of tools
    /// shows both.
    pub const APPS: Id = Id::new_static("core.apps");
    /// Extra pages on the main settings page, below the setting groups
    /// (e.g. the plugin manager). Entries are actions.
    pub const SETTINGS: Id = Id::new_static("core.settings-pages");
    /// The quick panel ([`actions::QUICK_PANEL`](super::actions::QUICK_PANEL)):
    /// switches and controls for quick changes, from every plugin.
    pub const QUICK: Id = Id::new_static("core.quick");
    /// The reader's menu, while a book is open (whichever plugin is the
    /// reader, [`slots::READER`](super::slots::READER)): plugins add what
    /// belongs to reading.
    pub const READER: Id = Id::new_static("core.reader-menu");
    /// The reader's submenu "More": what is used less often (statistics,
    /// syncing a book, profiles).
    pub const READER_MORE: Id = Id::new_static("core.reader-more");
    /// The reader's panel of font and layout, shown over the page; plugins
    /// add controls such as the light.
    pub const READER_PANEL: Id = Id::new_static("core.reader-panel");
    /// What can be done with text chosen in a book (look it up, share,
    /// translate): plugins add their actions; when one is chosen, the
    /// reader sends [`events::TEXT_CHOSEN`](super::events::TEXT_CHOSEN),
    /// then runs it.
    pub const SELECTION: Id = Id::new_static("core.selection-menu");
    /// What can be done with a book, wherever a library lists it (its
    /// sheet: open, finished, favourite, information, …): plugins add
    /// their actions (send it, sync it now, delete it); when one is
    /// chosen, the library sends [`events::BOOK_CHOSEN`](super::events::BOOK_CHOSEN)
    /// with the book, then runs it.
    pub const BOOK: Id = Id::new_static("core.book-menu");
    /// Other ways to open a book, listed with the reading apps where the
    /// reader chooses one (*Open with…*, see
    /// [`readers::ask_once`](crate::readers::ask_once)): plugins add their
    /// actions (open it in a browser, send it to a computer); when one is
    /// chosen, [`events::BOOK_CHOSEN`](super::events::BOOK_CHOSEN) is sent
    /// with the book, then it runs, as in [`BOOK`].
    pub const OPEN_WITH: Id = Id::new_static("core.open-with-menu");
}

/// The roles of decorations every reader draws (see
/// [`decorations`](crate::decorations)): the kernel adds them, so their
/// looks are settings like any role's.
pub mod roles {
    use crate::Id;

    /// One's own highlights.
    pub const HIGHLIGHT: Id = Id::new_static("core.highlight");
    /// Links in a book.
    pub const LINK: Id = Id::new_static("core.link");
    /// A search hit gone to.
    pub const SEARCH_HIT: Id = Id::new_static("core.search-hit");
    /// The sentence being read aloud.
    pub const ALOUD: Id = Id::new_static("core.aloud");
}

/// Standard parameter names.
pub mod params {
    /// A storage path, e.g. of the book to open.
    pub const PATH: &str = "path";
    /// The menu to show in the menu slot.
    pub const MENU: &str = "menu";
    /// An id, e.g. of the setting in `core.setting-changed`.
    pub const ID: &str = "id";
    /// A title, e.g. of a book or of what a keyboard asks for.
    pub const TITLE: &str = "title";
    /// Text, e.g. entered on the keyboard.
    pub const TEXT: &str = "text";
    /// What kind of text a keyboard asks for: `text` (the default),
    /// `number`, `password` (shown as dots), `url` or `email` (typed as
    /// they are, never with capitals of the keyboard's own).
    pub const MODE: &str = "mode";
    /// The event a screen emits with its result, e.g. the keyboard's text.
    pub const REPLY: &str = "reply";
    /// What a screen shows focused first, e.g. a setting in
    /// [`slots::SETTINGS`](super::slots::SETTINGS).
    pub const FOCUS: &str = "focus";
    /// Set (to `1`) when a question is answered on each change too, e.g.
    /// the keyboard's ([`ask::TextQuestion::live`](crate::ask::TextQuestion::live)).
    pub const LIVE: &str = "live";
    /// Set (to `1`) on an answer given while the reader still types
    /// ([`Typed::typing`](crate::replies::Typed::typing)).
    pub const TYPING: &str = "typing";
    /// Why something failed, e.g. joining a network
    /// ([`events::WIFI_FAILED`](super::events::WIFI_FAILED)).
    pub const REASON: &str = "reason";
    /// A position in a document: section and offset (see
    /// `document::Locator`).
    pub const SECTION: &str = "section";
    pub const OFFSET: &str = "offset";
    /// Reading progress in percent, e.g. `12.5`.
    pub const PROGRESS: &str = "progress";
    /// The page (from 1) and the number of pages, once a reader has
    /// counted them.
    pub const PAGE: &str = "page";
    pub const PAGES: &str = "pages";
    /// A word, e.g. to look up.
    pub const WORD: &str = "word";
    /// A BCP 47 language tag, e.g. of the book a word is from.
    pub const LANGUAGE: &str = "language";
    /// A web address, e.g. for [`slots::BROWSER`](super::slots::BROWSER).
    pub const URL: &str = "url";
    /// An action to run, e.g. when the title of a menu bar is tapped.
    pub const ACTION: &str = "action";
    /// Entries a menu leaves out this time, as ids separated by commas:
    /// what does not apply where it is opened (e.g. searching a comic).
    pub const HIDE: &str = "hide";
    /// Where something was before, e.g. a moved book
    /// ([`events::BOOK_MOVED`](super::events::BOOK_MOVED)).
    pub const FROM: &str = "from";
}

/// Events emitted by the kernel.
pub mod events;

/// The kernel's status lines (see [`status`](crate::status)).
pub mod status_lines {
    use crate::Id;

    /// The items of the place it is drawn in, at its left, centre and
    /// right ([`status::Place`](crate::status::Place)).
    pub const ITEMS: Id = Id::new_static("core.status-line.items");
}

/// Status items of the kernel (see [`status`](crate::status)).
pub mod status_items {
    use crate::Id;

    /// The time, as set in `core.clock-format`.
    pub const CLOCK: Id = Id::new_static("core.status.clock");
    /// The battery's charge.
    pub const BATTERY: Id = Id::new_static("core.status.battery");
    /// The battery's charge, only while it is low
    /// ([`settings::BATTERY_LOW`](super::settings::BATTERY_LOW)): a calm
    /// line while there is enough.
    pub const BATTERY_LOW: Id = Id::new_static("core.status.battery-low");
    /// Today in words ([`time::date_text`](crate::time::date_text)).
    pub const DATE: Id = Id::new_static("core.status.date");
    /// Today in short words, e.g. "Thu 15 Oct"
    /// ([`time::short_date_text`](crate::time::short_date_text)), for
    /// narrow places.
    pub const DATE_SHORT: Id = Id::new_static("core.status.date-short");
}

/// Standard screens.
pub mod screens {
    use crate::Id;

    /// Shown when a slot has no screen bound to it.
    pub const PLACEHOLDER: Id = Id::new_static("core.placeholder");
    /// Chooses the status items a setting lists (see
    /// [`ui::items`](crate::ui::items)): name it as the setting's editor.
    pub const STATUS_ITEMS: Id = Id::new_static("core.status-items");
    /// Chooses the status lines a setting lists.
    pub const STATUS_LINES: Id = Id::new_static("core.status-lines");
    /// Edits a list of widgets a setting holds (see
    /// [`ui::widget_list`](crate::ui::widget_list)), bound to slot
    /// [`EDIT_WIDGETS`](super::slots::EDIT_WIDGETS) unless another screen
    /// is.
    pub const WIDGET_LIST: Id = Id::new_static("core.widget-list");
    /// Adds a widget to a list of widgets a setting holds.
    pub const ADD_WIDGET: Id = Id::new_static("core.add-widget");
    /// The kernel's choice of a few options
    /// ([`ui::choice::Choice`](crate::ui::choice::Choice)), bound to slot
    /// [`CHOICE`](super::slots::CHOICE) unless another screen is.
    pub const CHOICE: Id = Id::new_static("core.choice-dialog");
    /// The kernel's yes or no question
    /// ([`ui::confirm::Confirm`](crate::ui::confirm::Confirm)), bound to
    /// slot [`CONFIRM`](super::slots::CONFIRM) unless another screen is.
    pub const CONFIRM: Id = Id::new_static("core.confirm-dialog");
    /// Chooses a folder for a setting with the folder chooser
    /// ([`slots::CHOOSE_FOLDER`](super::slots::CHOOSE_FOLDER)), whichever
    /// plugin fills it: name it as a text setting's editor.
    pub const FOLDER_EDITOR: Id = Id::new_static("core.folder-editor");
    /// Says that an update is being finished while the firmware just
    /// installed runs on trial, until the device starts again to load its
    /// code plugins; takes no input. The shell opens it with params
    /// `from` and `until`, the trial's start and end in milliseconds.
    pub const FINISHING_UPDATE: Id = Id::new_static("core.finishing-update");
}

/// Global settings.
pub mod settings {
    use crate::setting::{Setting, Text};

    /// UI language tag, e.g. `en` or `de`.
    pub const LANGUAGE: Setting<Text> = Setting::new("core.language");
    /// Developer options: the settings of
    /// [`groups::DEVELOPER`](super::groups::DEVELOPER) show, and plugins may
    /// show what only developers need (e.g. what each plugin took).
    pub const DEVELOPER: Setting<bool> = Setting::new("core.developer");
    /// The library folder, where books are kept (`books`; empty for the
    /// storage's top): the kernel's, so that any library and every plugin
    /// that puts books somewhere reads the same
    /// ([`library::folder`](crate::library::folder)).
    pub const LIBRARY_FOLDER: Setting<Text> = Setting::new("core.library-folder");
    /// Draw everything inverted.
    pub const NIGHT_MODE: Setting<bool> = Setting::new("core.night-mode");
    /// In night mode, pictures inverted too (else as they are, not as
    /// negatives).
    pub const NIGHT_PICTURES: Setting<bool> = Setting::new("core.night-pictures");
    /// How full text is on panels of black and white alone
    /// ([`MenuNeed::BlackAndWhite`](crate::MenuNeed)): from what part
    /// covered a pixel at the edge of a letter is drawn, one of
    /// [`MONO_TEXT_LEVELS`] (half, three eighths, a quarter).
    pub const MONO_TEXT: Setting<Text> = Setting::new("core.mono-text");
    pub const MONO_TEXT_LEVELS: &[&str] = &[
        "core.mono-text.light",
        "core.mono-text.normal",
        "core.mono-text.full",
    ];
    /// How far in from the top and the bottom edge a swipe begins to count
    /// as one in from the edge, in millimetres (0: none).
    pub const EDGE_ENDS: Setting<i64> = Setting::new("core.edge-ends");
    /// The same at the left and right edges.
    pub const EDGE_SIDES: Setting<i64> = Setting::new("core.edge-sides");
    /// Say what the buttons move to (a row, a button), where the device
    /// speaks: for using PaperPaw without seeing the screen.
    pub const SPEAK_FOCUS: Setting<bool> = Setting::new("core.speak-focus");
    /// The reading app books open with where nothing else is chosen
    /// (see [`readers`](crate::readers)): [`READING_APP_DEFAULT`] (the
    /// screen bound to [`slots::READER`](super::slots::READER)) or a
    /// reader's id. Each format has one of its own too
    /// ([`readers::format_setting`](crate::readers::format_setting)).
    pub const READING_APP: Setting<Text> = Setting::new("core.reading-app");
    /// [`READING_APP`] and a format's: as the defaults give.
    pub const READING_APP_DEFAULT: &str = "core.reading-app.default";
    /// The voice that speaks text aloud: [`VOICE_SYSTEM`] or a plugin's
    /// (see [`voices`](crate::voices)).
    pub const VOICE: Setting<Text> = Setting::new("core.voice");
    /// [`VOICE`]: the system's.
    pub const VOICE_SYSTEM: &str = "core.voice.system";
    /// Partial refreshes between two full refreshes; `0` disables full
    /// refreshes for ghosting removal.
    pub const FULL_REFRESH_INTERVAL: Setting<i64> = Setting::new("core.full-refresh-interval");
    /// Whether another screen opening flashes the panel (a full refresh,
    /// no traces of the last one, but slow on some panels); else it counts
    /// as a page turn towards [`FULL_REFRESH_INTERVAL`].
    pub const FLASH_SCREENS: Setting<bool> = Setting::new("core.flash-screens");
    /// Whether a screen with pictures (covers, a sleep screen's picture)
    /// is shown in grays when it is drawn whole, where the panel shows
    /// black and white but more grays with a refresh of its own (the X4):
    /// a little slower to show, the pictures finer. What changes on it
    /// after (the focus moving) is shown in black and white. A screen may
    /// say no ([`Screen::pictures_in_grays`](crate::Screen::pictures_in_grays)).
    pub const PICTURES_IN_GRAYS: Setting<bool> = Setting::new("core.pictures-in-grays");
    /// Minutes without use after which the device goes to sleep by itself
    /// (`0`: never), where PaperPaw puts it to sleep
    /// ([`Capabilities::sleep`](crate::Capabilities)). Turning pages, also by themselves,
    /// counts as use; reading aloud, downloads and servers keep it awake.
    pub const SLEEP_AFTER: Setting<i64> = Setting::new("core.sleep-after");
    /// The charge in percent at and below which the battery counts as low
    /// (the status item [`BATTERY_LOW`](super::status_items::BATTERY_LOW)).
    pub const BATTERY_LOW: Setting<i64> = Setting::new("core.battery-low");
    /// Whether plugins may keep the device from going to sleep by itself
    /// ([`WakeHold`](crate::power::WakeHold)): on by default; off, the
    /// device sleeps after [`SLEEP_AFTER`] whatever plugins ask.
    pub const AWAKE_HOLDS: Setting<bool> = Setting::new("core.awake-holds");
    /// How the screen is turned: one of [`ROTATIONS`], where the shell
    /// can turn it (see [`MenuNeed::Rotation`](crate::MenuNeed)); each
    /// book may have its own ([`SCOPE_ONLY`]).
    pub const ROTATION: Setting<Text> = Setting::new("core.rotation");
    /// Whether a row below the screen shows what the buttons under it do,
    /// where a device has buttons in a row there (see
    /// [`MenuNeed::ButtonRow`](crate::MenuNeed)).
    pub const BUTTON_HINTS: Setting<bool> = Setting::new("core.button-hints");
    /// The page buttons the other way round while reading (on the screen
    /// bound to [`slots::READER`](super::slots::READER)): page forward
    /// goes back and page back forward (the kernel swaps them before key
    /// maps and input filters see them).
    pub const SWAP_PAGE_BUTTONS: Setting<bool> = Setting::new("core.swap-page-buttons");
    /// The values of [`ROTATION`]: upright, on its side (landscape),
    /// upside down, on its other side.
    pub const ROTATIONS: &[&str] = &[
        "core.rotation.upright",
        "core.rotation.landscape",
        "core.rotation.upside-down",
        "core.rotation.landscape-flipped",
    ];
    /// The colour black is shown in: one of [`TEXT_COLORS`], where the
    /// shell tints the screen (see [`MenuNeed::Colors`](crate::MenuNeed)).
    /// Everything is drawn in grays; the shell shows black as this colour,
    /// white as [`PAPER_COLOR`] and the grays between them.
    pub const TEXT_COLOR: Setting<Text> = Setting::new("core.text-color");
    /// The colour white is shown in: one of [`PAPER_COLORS`].
    pub const PAPER_COLOR: Setting<Text> = Setting::new("core.paper-color");
    /// The own text colour, used when [`TEXT_COLOR`] is [`OWN_TEXT`]: a
    /// colour code such as `#5b4636` (see [`parse_rgb`]).
    pub const TEXT_COLOR_OWN: Setting<Text> = Setting::new("core.text-color-own");
    /// The own paper colour, used when [`PAPER_COLOR`] is [`OWN_PAPER`].
    pub const PAPER_COLOR_OWN: Setting<Text> = Setting::new("core.paper-color-own");
    /// The value of [`TEXT_COLOR`] that takes [`TEXT_COLOR_OWN`].
    pub const OWN_TEXT: &str = "core.color.own";
    /// The value of [`PAPER_COLOR`] that takes [`PAPER_COLOR_OWN`].
    pub const OWN_PAPER: &str = "core.paper.own";
    /// The values of [`TEXT_COLOR`]: dark colours, then one's own.
    pub const TEXT_COLORS: &[&str] = &[
        "core.color.black",
        "core.color.dark-gray",
        "core.color.gray",
        "core.color.brown",
        "core.color.dark-blue",
        "core.color.dark-green",
        "core.color.burgundy",
        "core.color.violet",
        OWN_TEXT,
    ];
    /// The values of [`PAPER_COLOR`]: light colours, then one's own.
    pub const PAPER_COLORS: &[&str] = &[
        "core.paper.white",
        "core.paper.cream",
        "core.paper.sepia",
        "core.paper.sand",
        "core.paper.light-gray",
        "core.paper.mint",
        "core.paper.light-blue",
        "core.paper.rose",
        OWN_PAPER,
    ];

    /// The red, green and blue of a value of [`TEXT_COLOR`] or
    /// [`PAPER_COLOR`] other than one's own.
    pub fn rgb(value: &str) -> Option<[u8; 3]> {
        Some(match value {
            "core.color.black" => [0, 0, 0],
            "core.color.dark-gray" => [51, 51, 51],
            "core.color.gray" => [85, 85, 85],
            "core.color.brown" => [91, 70, 54],
            "core.color.dark-blue" => [28, 42, 68],
            "core.color.dark-green" => [30, 58, 42],
            "core.color.burgundy" => [94, 28, 36],
            "core.color.violet" => [58, 42, 82],
            "core.paper.white" => [255, 255, 255],
            "core.paper.cream" => [250, 246, 236],
            "core.paper.sepia" => [244, 236, 216],
            "core.paper.sand" => [233, 222, 196],
            "core.paper.light-gray" => [232, 232, 232],
            "core.paper.mint" => [226, 240, 230],
            "core.paper.light-blue" => [226, 236, 244],
            "core.paper.rose" => [246, 232, 232],
            _ => return None,
        })
    }

    /// The red, green and blue of a colour code: `#5b4636`, `5b4636` or
    /// the short `#543`; `None` for anything else.
    pub fn parse_rgb(code: &str) -> Option<[u8; 3]> {
        let hex = code.trim().trim_start_matches('#');
        let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
        match hex.len() {
            3 => Some([digit(0)? * 17, digit(1)? * 17, digit(2)? * 17]),
            6 => {
                let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
                Some([byte(0)?, byte(2)?, byte(4)?])
            }
            _ => None,
        }
    }
    /// Margin of menus and lists, in millimetres.
    pub const UI_MARGIN: Setting<i64> = Setting::new("core.ui-margin");
    /// Height of list rows, in percent of a line of text.
    pub const UI_ROW: Setting<i64> = Setting::new("core.ui-row");
    /// Width of lines and borders, in tenths of a millimetre.
    pub const UI_LINES: Setting<i64> = Setting::new("core.ui-lines");
    /// Headers dark ([`HEADER_DARK`]), light ([`HEADER_LIGHT`]) or plain,
    /// without a bar or a line ([`HEADER_PLAIN`]).
    pub const UI_HEADER: Setting<Text> = Setting::new("core.ui-header");
    pub const HEADER_DARK: &str = "core.header.dark";
    pub const HEADER_LIGHT: &str = "core.header.light";
    pub const HEADER_PLAIN: &str = "core.header.plain";
    /// The frame of every widget whose own the reader did not choose: one
    /// of [`widgets::ALL_FRAMES`](crate::widgets::ALL_FRAMES)
    /// (each widget its own at first).
    pub const WIDGET_FRAME: Setting<Text> = Setting::new("core.widgets.frame");
    /// Corners of highlights, buttons and boxes: one of [`CORNERS`].
    pub const UI_CORNERS: Setting<Text> = Setting::new("core.ui-corners");
    /// The values of [`UI_CORNERS`]: square, slightly rounded, round,
    /// soft (rounder still). With round and soft ones, small shapes
    /// (buttons, switches, the focus) are pills.
    pub const CORNERS: &[&str] = &[
        "core.corners.square",
        "core.corners.small",
        "core.corners.round",
        "core.corners.soft",
    ];
    /// How the focus is drawn: a dark bar, a frame, a light bar (a frame
    /// on panels of black and white alone).
    pub const UI_FOCUS: Setting<Text> = Setting::new("core.ui-focus");
    pub const FOCUS_FILL: &str = "core.focus.fill";
    pub const FOCUS_RING: &str = "core.focus.ring";
    pub const FOCUS_SOFT: &str = "core.focus.soft";
    /// The lines between rows: light, faint from the text, none.
    pub const UI_SEPARATORS: Setting<Text> = Setting::new("core.ui-separators");
    pub const SEPARATORS_LINE: &str = "core.separators.line";
    pub const SEPARATORS_INSET: &str = "core.separators.inset";
    pub const SEPARATORS_NONE: &str = "core.separators.none";
    /// The sections of lists as they are or on rounded cards.
    pub const UI_GROUPS: Setting<Text> = Setting::new("core.ui-groups");
    pub const GROUPS_PLAIN: &str = "core.groups.plain";
    pub const GROUPS_CARDS: &str = "core.groups.cards";
    /// Size of headers' titles, in percent of the text's.
    pub const UI_TITLE: Setting<i64> = Setting::new("core.ui-title");
    /// Gray text such as hints and notes: as it is, or black for strong
    /// contrast ([`CONTRAST_STRONG`]), e.g. on screens with few grays.
    pub const UI_CONTRAST: Setting<Text> = Setting::new("core.ui-contrast");
    pub const CONTRAST_NORMAL: &str = "core.contrast.normal";
    pub const CONTRAST_STRONG: &str = "core.contrast.strong";
    /// The grays of the interface by what they are for, e.g. `muted=60
    /// line=170` (see [`ui::Palette`](crate::ui::Palette)): for themes.
    pub const UI_PALETTE: Setting<Text> = Setting::new("core.ui-palette");
    /// How the edges of the interface's letters are inked, as a list of
    /// `[script=]curve` entries (see
    /// [`ToneCurve::parse_for`](crate::text::ToneCurve::parse_for)), e.g.
    /// `smooth` or `sharp:128`; empty is as it is. For themes.
    pub const UI_TEXT_CURVE: Setting<Text> = Setting::new("core.ui-text-curve");
    /// How long short messages show, in seconds.
    pub const NOTICE_SECONDS: Setting<i64> = Setting::new("core.notice-seconds");
    /// Milliseconds a button or finger must be held for a long press.
    pub const LONG_PRESS_MS: Setting<i64> = Setting::new("core.long-press-ms");
    /// Milliseconds within which a second tap or press makes a double tap
    /// or press, where something is bound to one.
    pub const DOUBLE_TAP_MS: Setting<i64> = Setting::new("core.double-tap-ms");
    /// How far, in millimetres, a finger may move and still tap.
    pub const TAP_SLOP: Setting<i64> = Setting::new("core.tap-slop");
    /// How far, in millimetres, a finger must move to swipe.
    pub const SWIPE_DISTANCE: Setting<i64> = Setting::new("core.swipe-distance");
    /// How bright the front light is, in percent, where the device has one
    /// it lets PaperPaw set ([`Capabilities::frontlight`](crate::Capabilities)).
    pub const FRONTLIGHT: Setting<i64> = Setting::new("core.frontlight");
    /// How warm the front light is, in percent (0 cold, 100 warm).
    pub const FRONTLIGHT_WARMTH: Setting<i64> = Setting::new("core.frontlight-warmth");
    /// Base size of user interface text in points; UI metrics scale with it.
    pub const UI_SIZE: Setting<i64> = Setting::new("core.ui-size");
    /// The active key map preset (see [`keymap_presets`](super::keymap_presets)).
    pub const KEYMAP_PRESET: Setting<Text> = Setting::new("core.keymap-preset");
    /// Scoped settings changed now apply to the active scope only, e.g.
    /// the open book ("Only this book"); switching it off returns the
    /// scope to the global values.
    pub const SCOPE_ONLY: Setting<bool> = Setting::new("core.scope-only");
    /// The font family for reading: [`DEFAULT_FONT`] (the plugins'
    /// choice) or one of the registered families.
    pub const READING_FONT: Setting<Text> = Setting::new("core.reading-font");
    /// The size of a book's text, in points; each book may have its own
    /// ([`SCOPE_ONLY`]). Every reader reads it.
    pub const FONT_SIZE: Setting<i64> = Setting::new("core.font-size");
    /// The value of [`READING_FONT`] for the plugins' choice.
    pub const DEFAULT_FONT: &str = "core.font.default";
    /// Darker text: glyphs widened by up to this many steps (0 to 4). The
    /// standard type of book pages, as [`FONT_SIZE`]: the kernel
    /// registers them, each book may have its own ([`SCOPE_ONLY`]), every
    /// reader reads them.
    pub const FONT_WEIGHT: Setting<i64> = Setting::new("core.font-weight");
    /// Book text with smooth letters, their edges in grays; off, each
    /// pixel black or white, from as much coverage as [`MONO_TEXT`] says.
    /// Where the panel shows black and white but grays with a refresh of
    /// its own (the X4), it is off at first: on, every page is shown in
    /// grays, slower and costing more battery; off, only pages with
    /// pictures are, and their letters stay smooth. Each book may have
    /// its own ([`SCOPE_ONLY`]); the interface's text is as it is.
    pub const SMOOTH_TEXT: Setting<bool> = Setting::new("core.smooth-text");
    /// How dark book text is drawn: its letters' edges on a contrast
    /// curve, finer than [`FONT_WEIGHT`] and with it, one of
    /// [`TEXT_CONTRAST_LEVELS`] (lighter, normal, darker, darkest; see
    /// [`ToneCurve::contrast`](crate::text::ToneCurve::contrast), whose steps go
    /// from -1 for the first). Each book may have its own
    /// ([`SCOPE_ONLY`]).
    pub const TEXT_CONTRAST: Setting<Text> = Setting::new("core.text-contrast");
    /// How the edges of book letters are inked, in place of
    /// [`TEXT_CONTRAST`] when it is set: a list of `[script=]curve` entries
    /// (see [`ToneCurve::parse_for`](crate::text::ToneCurve::parse_for)),
    /// e.g. `smooth; arab=contrast:1` (the script is that of the book's
    /// language, see [`Lang::likely_script`](crate::Lang::likely_script)). Empty keeps
    /// [`TEXT_CONTRAST`]. Each
    /// book may have its own ([`SCOPE_ONLY`]).
    pub const TEXT_CURVE: Setting<Text> = Setting::new("core.text-curve");
    pub const TEXT_CONTRAST_LEVELS: &[&str] = &[
        "core.text-contrast.lighter",
        "core.text-contrast.normal",
        "core.text-contrast.darker",
        "core.text-contrast.darkest",
    ];
    /// Space between words in percent of the font's (50 to 200).
    pub const WORD_SPACING: Setting<i64> = Setting::new("core.word-spacing");
    /// Distance between baselines in percent of the font size, at least
    /// the font's own; below 100 that much of the font's own (80 to 250).
    pub const LINE_SPACING: Setting<i64> = Setting::new("core.line-spacing");
    /// Page margins in millimetres, at the sides (and at the top and
    /// bottom while [`MARGIN_ENDS`] follows them).
    pub const MARGIN: Setting<i64> = Setting::new("core.margin");
    /// Margins at the top and bottom in millimetres; as [`MARGIN`] until
    /// set.
    pub const MARGIN_ENDS: Setting<i64> = Setting::new("core.margin-ends");
    /// Lines of paragraphs as wide as the page.
    pub const JUSTIFY: Setting<bool> = Setting::new("core.justify");
    /// Words broken at line ends, in the book's language.
    pub const HYPHENATE: Setting<bool> = Setting::new("core.hyphenate");
    /// The fewest letters left before a hyphen (1 to 5; 0: the language's
    /// own, that of its patterns), see `hyphen::Patterns::with_minimums`.
    pub const HYPHEN_MIN_LEFT: Setting<i64> = Setting::new("core.hyphen-min-left");
    /// The fewest letters carried after a hyphen (as [`HYPHEN_MIN_LEFT`]).
    pub const HYPHEN_MIN_RIGHT: Setting<i64> = Setting::new("core.hyphen-min-right");
    /// The first lines of paragraphs indented.
    pub const INDENT: Setting<bool> = Setting::new("core.indent");
    /// Space between paragraphs.
    pub const PARAGRAPH_SPACING: Setting<bool> = Setting::new("core.paragraph-spacing");
    /// The fewest lines of a paragraph left alone at the top of a page
    /// (1 to 4; 1: as they fall), see
    /// `layout::Style::widows`.
    pub const WIDOWS: Setting<i64> = Setting::new("core.widows");
    /// The same at the bottom of a page.
    pub const ORPHANS: Setting<i64> = Setting::new("core.orphans");
    /// The book's own styles (an EPUB's CSS), see
    /// [`OpenOptions::publisher_styles`](crate::OpenOptions).
    pub const PUBLISHER_STYLES: Setting<bool> = Setting::new("core.publisher-styles");
    /// With [`PUBLISHER_STYLES`] on, which of the book's own styles are
    /// followed, each on its own (see `layout::BookStyles`): its
    /// alignment, indents, text sizes, space between paragraphs, fonts,
    /// margins at the sides and distance between lines.
    pub const BOOK_ALIGN: Setting<bool> = Setting::new("core.book-align");
    pub const BOOK_INDENT: Setting<bool> = Setting::new("core.book-indent");
    pub const BOOK_SIZES: Setting<bool> = Setting::new("core.book-sizes");
    pub const BOOK_SPACING: Setting<bool> = Setting::new("core.book-spacing");
    pub const BOOK_FONTS: Setting<bool> = Setting::new("core.book-fonts");
    pub const BOOK_MARGINS: Setting<bool> = Setting::new("core.book-margins");
    pub const BOOK_LINE_HEIGHT: Setting<bool> = Setting::new("core.book-line-height");
    /// Space between letters, in percent of the text size (-5 to 20).
    pub const LETTER_SPACING: Setting<i64> = Setting::new("core.letter-spacing");
    /// Pairs of letters set as the font kerns them (e.g. `AV` closer).
    pub const KERNING: Setting<bool> = Setting::new("core.kerning");
    /// `fi`, `fl` and the like drawn as ligatures where the font has them
    /// (not with letter spacing).
    pub const LIGATURES: Setting<bool> = Setting::new("core.ligatures");
    /// How far [`INDENT`] indents first lines, in percent of the text size
    /// (50 to 400).
    pub const INDENT_SIZE: Setting<i64> = Setting::new("core.indent-size");
    /// How paragraphs are broken into lines: over all their lines, for
    /// lines that end evenly ([`LINE_BREAKING_BALANCED`]), or line by line
    /// ([`LINE_BREAKING_FAST`]).
    pub const LINE_BREAKING: Setting<Text> = Setting::new("core.line-breaking");
    pub const LINE_BREAKING_BALANCED: &str = "core.line-breaking.balanced";
    pub const LINE_BREAKING_FAST: &str = "core.line-breaking.fast";
    /// How much space [`PARAGRAPH_SPACING`] leaves, in percent of a line
    /// (20 to 150).
    pub const PARAGRAPH_SPACE: Setting<i64> = Setting::new("core.paragraph-space");
    /// Notes shown over the page ([`FOOTNOTES_POPUP`]) or gone to
    /// ([`FOOTNOTES_JUMP`]) when their mark is followed.
    pub const FOOTNOTES: Setting<Text> = Setting::new("core.footnotes");
    pub const FOOTNOTES_POPUP: &str = "core.footnotes.popup";
    pub const FOOTNOTES_JUMP: &str = "core.footnotes.jump";
    /// The frame that sets book pages in columns (see
    /// [`layers`](crate::layers)): [`PAGE_FRAME_SINGLE`] or the id of a
    /// plugin's frame; each book may have its own ([`SCOPE_ONLY`]).
    pub const PAGE_FRAME: Setting<Text> = Setting::new("core.page-frame");
    /// [`PAGE_FRAME`]: the text in one column.
    pub const PAGE_FRAME_SINGLE: &str = "core.page-frame.single";
    /// The font of the status lines while reading
    /// ([`fonts::STATUS`](super::fonts::STATUS)): [`STATUS_FONT_UI`],
    /// [`STATUS_FONT_BOOK`] or a font family's name.
    pub const STATUS_FONT: Setting<Text> = Setting::new("core.status-font");
    /// [`STATUS_FONT`]: as the menus.
    pub const STATUS_FONT_UI: &str = "core.status-font.ui";
    /// [`STATUS_FONT`]: as the book.
    pub const STATUS_FONT_BOOK: &str = "core.status-font.book";
    /// `core.clock.24h` or `core.clock.12h`.
    pub const CLOCK_FORMAT: Setting<Text> = Setting::new("core.clock-format");
    /// The first day of the week, one of
    /// [`time::WEEK_STARTS`](crate::time::WEEK_STARTS); read it with
    /// [`time::week_start`](crate::time::week_start).
    pub const WEEK_START: Setting<Text> = Setting::new("core.week-start");
    /// The time zone: `core.tz.auto` (the system's) or one of
    /// [`time::TIME_ZONES`](crate::time::TIME_ZONES).
    pub const TIME_ZONE: Setting<Text> = Setting::new("core.time-zone");
    /// A POSIX `TZ` rule that overrides [`TIME_ZONE`] when not empty.
    pub const TIME_ZONE_RULE: Setting<Text> = Setting::new("core.time-zone-rule");
}

/// Key map presets: named sets of bindings users switch between, e.g. for
/// devices with only two buttons. Plugins add more with
/// [`Registrar::add_keymap_preset`](crate::Registrar::add_keymap_preset).
pub mod keymap_presets {
    use crate::Id;

    /// No preset: the plugins' default bindings.
    pub const STANDARD: Id = Id::new_static(STANDARD_NAME);
    /// [`STANDARD`] as a static string, e.g. for setting values.
    pub const STANDARD_NAME: &str = "core.standard";
    /// Everything with "forward" and "back": short press moves, long press
    /// on forward confirms (opens the menu while reading), long press on
    /// back goes back.
    pub const TWO_BUTTONS: Id = Id::new_static("core.two-buttons");
    /// Taps on the left turn forward, on the right back: for the left
    /// hand.
    pub const LEFT_HANDED: Id = Id::new_static("core.left-handed");
}

/// The user's configuration file on the device's storage. Settings changed
/// in the UI are written back to it; comments and order are preserved.
pub const CONFIG_FILE: &str = "paperpaw.ini";

/// The folder with data plugins (one subfolder each) on the storage,
/// unless the shell names others.
pub const PLUGIN_DIR: &str = "plugins";

/// Groups of the generated settings screen; plugins can add their own.
/// A group's page lists its settings, then the actions plugins add to
/// the menu named by the group's id (e.g. *Clean up storage* in
/// [`SYSTEM`](groups::SYSTEM)).
pub mod groups {
    use crate::Id;

    /// Reading books: a page of its parts below.
    pub const READING: Id = Id::new_static("core.group.reading");
    /// Parts of [`READING`], each a page of its own: the text (font, size,
    /// weight, spacing, colours), the layout of pages (margins, line
    /// spacing, justification and line breaking, lines kept together),
    /// hyphenation, which of a book's own styles are followed, how text
    /// and pictures are shown (smooth, contrast, night mode, refresh),
    /// reading aloud, aids to reading, and what happens while reading
    /// (books opened again, footnotes, the reading app); with
    /// [`STATUS_LINES`] and [`MARKS`].
    pub const READING_TEXT: Id = Id::new_static("core.group.reading-text");
    pub const READING_LAYOUT: Id = Id::new_static("core.group.reading-layout");
    pub const READING_HYPHENATION: Id = Id::new_static("core.group.reading-hyphenation");
    pub const READING_BOOK_STYLES: Id = Id::new_static("core.group.reading-book-styles");
    pub const READING_DISPLAY: Id = Id::new_static("core.group.reading-display");
    pub const READING_ALOUD: Id = Id::new_static("core.group.reading-aloud");
    pub const READING_AIDS: Id = Id::new_static("core.group.reading-aids");
    pub const READING_BEHAVIOUR: Id = Id::new_static("core.group.reading-behaviour");
    /// Screen: front light, rotation.
    pub const DISPLAY: Id = Id::new_static("core.group.display");
    /// Buttons and gestures.
    pub const CONTROLS: Id = Id::new_static("core.group.controls");
    /// Books and folders.
    pub const LIBRARY: Id = Id::new_static("core.group.library");
    /// Language and everything about the device.
    pub const SYSTEM: Id = Id::new_static("core.group.system");
    /// Date and time: the clock's format, the first day of the week, the
    /// time zone.
    pub const TIME: Id = Id::new_static("core.group.time");
    /// Plugins: the store, each plugin's network.
    pub const PLUGINS: Id = Id::new_static("core.group.plugins");
    /// Words: dictionaries, translation, vocabulary.
    pub const WORDS: Id = Id::new_static("core.group.words");
    /// How menus and lists look: text size, margins, rows, lines, headers.
    /// Themes (data plugins) set its defaults.
    pub const LOOK: Id = Id::new_static("core.group.look");
    /// Marks in the text: the looks of the roles of decorations (see
    /// [`decorations`](crate::decorations)), a section per role; a part
    /// of [`READING`].
    pub const MARKS: Id = Id::new_static("core.group.marks");
    /// The status lines above and below a book's text (what they show,
    /// their size); the reader puts its settings for them here. A part of
    /// [`READING`].
    pub const STATUS_LINES: Id = Id::new_static("core.group.status");
    /// Settings whose plugin names no registered group.
    pub const OTHER: Id = Id::new_static("core.group.other");
    /// For developers: measuring, trying, finding faults. Its settings show
    /// only with [`settings::DEVELOPER`](super::settings::DEVELOPER) on.
    pub const DEVELOPER: Id = Id::new_static("core.group.developer");
}

/// Sections of the settings' main page, with the setting groups and the
/// pages in [`menus::SETTINGS`] placed by their order: reading 1 to 49,
/// the device 51 to 199, customising PaperPaw from 201. A group whose
/// order is 0, the default, names none: it is listed at the end of the
/// section for customising, by name. A group shows the icon its plugin
/// gives it (`Registrar::add_icon` with its id), else a plugin's.
pub mod settings_page {
    use crate::Id;

    /// Text, status bars, library, statistics, synchronisation.
    pub const READING: Id = Id::new_static("core.settings-section.reading");
    /// Display, controls, language and the system.
    pub const DEVICE: Id = Id::new_static("core.settings-section.device");
    /// Menus, plugins, advanced settings.
    pub const CUSTOMISE: Id = Id::new_static("core.settings-section.customise");
}

/// Parts of setting groups shared by plugins, in the order shown: a
/// setting names one with
/// `SettingSpec::section_key`. Other parts
/// (any translation key) follow these.
pub mod group_sections {
    /// The reading font, its size and weight.
    pub const FONT: &str = "core.setting-section.font";
    /// How a page is set: spacing, margins, justification, hyphenation.
    pub const PAGE: &str = "core.setting-section.page";
    /// What happens when reading: books opened again, bookmarks (now
    /// mostly [`groups::READING_BEHAVIOUR`](super::groups::READING_BEHAVIOUR)).
    pub const BEHAVIOUR: &str = "core.setting-section.behaviour";
    /// Reading aloud: its speed, what is marked, turning the pages (now
    /// mostly [`groups::READING_ALOUD`](super::groups::READING_ALOUD)).
    pub const ALOUD: &str = "core.setting-section.aloud";

    /// The standard parts in their order.
    pub const ORDER: &[&str] = &[FONT, PAGE, BEHAVIOUR, ALOUD];
}

/// Sections of the main menu ([`menus::MAIN`]): plugins put their
/// entries in one by their order (reading 1 to 29, tools 31 to 79,
/// system 81 to 99).
pub mod menu_sections {
    use crate::Id;

    /// Home, library and the like.
    pub const READING: Id = Id::new_static("core.section.reading");
    /// What plugins add: files, statistics, transfers, catalogs.
    pub const TOOLS: Id = Id::new_static("core.section.tools");
    /// Settings and the device.
    pub const SYSTEM: Id = Id::new_static("core.section.system");
}

/// Font roles.
pub mod fonts {
    use crate::Id;

    /// Menus, labels and other user interface text.
    pub const UI: Id = Id::new_static("core.font-ui");
    /// Book text.
    pub const READING: Id = Id::new_static("core.font-reading");
    /// Bold book text; readers fall back to [`READING`] without it.
    pub const READING_BOLD: Id = Id::new_static("core.font-reading-bold");
    /// Italic book text; readers fall back to [`READING`] without it.
    pub const READING_ITALIC: Id = Id::new_static("core.font-reading-italic");
    /// Bold italic book text; readers fall back to [`READING_BOLD`].
    pub const READING_BOLD_ITALIC: Id = Id::new_static("core.font-reading-bold-italic");
    /// Icons (see [`icons`](crate::icons)); without a font here, menus
    /// show text only.
    pub const ICONS: Id = Id::new_static("core.font-icons");
    /// The status lines while reading: as the menus ([`UI`]), as the book
    /// ([`READING`]) or a family of their own, as
    /// [`settings::STATUS_FONT`](super::settings::STATUS_FONT) says.
    /// Readers draw their status lines in it.
    pub const STATUS: Id = Id::new_static("core.font-status");
}

/// The name of the key map layer that applies everywhere.
pub const GLOBAL_KEYMAP_LAYER: &str = "global";

#[cfg(test)]
mod tests {
    use super::settings::*;
    use crate::document::Locator;
    use crate::event::EventData;

    #[test]
    fn standard_events_carry_what_they_say() {
        use super::events::{Goto, TextChosen};
        let goto = Goto::to("books/a.epub", Locator::new(3, 120));
        assert_eq!(goto.end(), None);
        let back = Goto::from_params(&goto.to_params()).unwrap();
        assert_eq!((back.at(), back.end()), (Locator::new(3, 120), None));
        let marked = goto.marking(Locator::new(3, 140));
        let back = Goto::from_params(&marked.to_params()).unwrap();
        assert_eq!(back.end(), Some(Locator::new(3, 140)));
        let chosen = TextChosen {
            action: crate::Id::new_static("share.send"),
            text: "Alice".into(),
            language: "en".into(),
            path: "books/a.epub".into(),
            section: 1,
            offset: 10,
            end_section: 1,
            end_offset: 15,
        };
        let back = TextChosen::from_params(&chosen.to_params()).unwrap();
        assert_eq!(back, chosen);
        assert_eq!(
            (back.start(), back.end()),
            (Locator::new(1, 10), Locator::new(1, 15))
        );
    }

    #[test]
    fn skimming_says_where_the_book_is() {
        use super::events::{SKIMMED, Skimmed};
        let skimmed = Skimmed::new(
            "books/a.epub".into(),
            "12 / 340".into(),
            "Chapter 2".into(),
            35,
        )
        .marks(alloc::string::String::from("0,120,480"));
        let crate::Command::Emit(event, params) = SKIMMED.emit(skimmed.clone()) else {
            panic!("an event");
        };
        assert_eq!(SKIMMED.read(&event, &params), Some(skimmed));
    }

    #[test]
    fn reading_carries_the_chapter() {
        use super::events::Reading;
        let reading = Reading {
            path: "books/a.epub".into(),
            title: "Alice".into(),
            progress: 12.5,
            section: 3,
            offset: 120,
            page: Some(40),
            pages: Some(300),
            chapter: "Down the Rabbit-Hole".into(),
            chapter_left: Some(7),
            xpointer: Some("/body/DocFragment[4]/body/p[2]".into()),
        };
        let params = reading.to_params();
        assert_eq!(params.get("chapter_left"), Some("7"));
        assert_eq!(
            params.get("xpointer"),
            Some("/body/DocFragment[4]/body/p[2]")
        );
        assert_eq!(Reading::from_params(&params).unwrap(), reading);
        assert_eq!(reading.at(), Locator::new(3, 120));
        // A book without a table of contents, its pages not counted.
        let unknown = Reading {
            page: None,
            pages: None,
            chapter: "".into(),
            chapter_left: None,
            ..reading
        };
        let params = unknown.to_params();
        assert_eq!(params.get("chapter_left"), None, "absent, not empty");
        assert_eq!(Reading::from_params(&params).unwrap(), unknown);
    }

    #[test]
    fn every_colour_but_ones_own_is_known() {
        for value in TEXT_COLORS.iter().chain(PAPER_COLORS) {
            let own = *value == OWN_TEXT || *value == OWN_PAPER;
            assert_eq!(rgb(value).is_none(), own, "{value}");
        }
    }

    #[test]
    fn colour_codes() {
        assert_eq!(parse_rgb("#5b4636"), Some([0x5b, 0x46, 0x36]));
        assert_eq!(parse_rgb(" F4ECD8 "), Some([0xf4, 0xec, 0xd8]));
        assert_eq!(parse_rgb("#543"), Some([0x55, 0x44, 0x33]));
        for wrong in ["", "#", "#12345", "#ggg", "blue", "#1234567"] {
            assert_eq!(parse_rgb(wrong), None, "{wrong}");
        }
    }
}
