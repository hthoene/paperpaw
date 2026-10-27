use alloc::string::String;

use crate::Id;
use crate::document::Locator;
use crate::event::Event;

crate::event_data! {
    /// Where a book is being read ([`BOOK_OPENED`], [`PAGE_TURNED`],
    /// [`BOOK_CLOSED`], [`BOOK_FINISHED`]).
    pub struct Reading {
        /// The book.
        pub path: String,
        pub title: String,
        /// How far in it, in percent.
        pub progress: f32,
        /// The page's start ([`Reading::at`]).
        pub section: u32,
        pub offset: u32,
        /// The title of the chapter the page is in: empty where the
        /// book has no table of contents or the page comes before
        /// its first entry.
        pub chapter: String,
    }
    optional {
        /// The page and the pages, once the reader counted them.
        pub page: u32,
        pub pages: u32,
        /// The pages after this one in the chapter (0 on its last
        /// page), once the reader counted the pages and where the
        /// chapter is known.
        pub chapter_left: u32,
        /// The page's start as an XPointer (see
        /// [`Document::xpointer`](crate::document::Document::xpointer)),
        /// where the reader found it: with [`BOOK_CLOSED`], for a
        /// sync that sends it on (finding it takes a while on small
        /// devices, so not with every page).
        pub xpointer: String,
    }
}

impl Reading {
    /// The page's start.
    pub fn at(&self) -> Locator {
        Locator::new(self.section, self.offset)
    }
}

crate::event_data! {
    /// What has the focus of the buttons now ([`FOCUS_CHANGED`]), as
    /// the screen describes it (see [`semantics`](crate::semantics)).
    pub struct FocusChanged {
        /// Its role's name ([`semantics::role_name`](crate::semantics::role_name)),
        /// e.g. `row` or `button`.
        pub role: String,
        /// What it says, e.g. a row's title and value.
        pub label: String,
        /// Where it is on the screen.
        pub x: u16,
        pub y: u16,
        pub width: u16,
        pub height: u16,
    }
    optional {
        /// For switches and buttons that are on or off.
        pub on: bool,
    }
}

crate::event_data! {
    /// The screen now on top ([`SCREEN_SHOWN`]).
    pub struct ScreenShown {
        pub screen: Id,
    }
    optional {
        /// The slot it was opened for.
        pub slot: Id,
    }
}

crate::event_data! {
    /// Text typed from outside the screen ([`TYPE_TEXT`]).
    pub struct TypeText {
        /// What was typed, as many characters as there are.
        pub text: String,
    }
    optional {
        /// The text is then confirmed, as with the keyboard's *OK*.
        pub done: bool,
    }
}

crate::event_data! {
    /// Which setting changed ([`SETTING_CHANGED`]).
    pub struct SettingChanged {
        pub id: Id,
    }
    optional {
        /// Its value before, as written in `paperpaw.ini`, where the
        /// kernel changed it and a plugin subscribes to the event or
        /// observes events (it is not made for nobody).
        pub old: String,
    }
}

crate::event_data! {
    /// An action was chosen for a book in the library
    /// ([`BOOK_CHOSEN`], [`menus::BOOK`](super::menus::BOOK)).
    pub struct BookChosen {
        /// The action chosen.
        pub action: Id,
        /// The book's path.
        pub path: String,
    }
}

crate::event_data! {
    /// A book that was deleted ([`BOOK_REMOVED`]).
    pub struct BookRemoved {
        /// Its path, where it was.
        pub path: String,
    }
}

crate::event_data! {
    /// A book that was moved or renamed ([`BOOK_MOVED`]).
    pub struct BookMoved {
        /// Its old path.
        pub from: String,
        /// Its new one.
        pub path: String,
    }
}

crate::event_data! {
    /// A book that came into the library ([`BOOK_ADDED`]).
    pub struct BookAdded {
        pub path: String,
        /// Where it came from: the plugin that brought it (`transfer`,
        /// `calibre`, `opds`), empty where it is not known.
        pub source: String,
    }
}

crate::event_data! {
    /// What changed of a book ([`BOOK_CHANGED`]).
    pub struct BookChanged {
        pub path: String,
        /// What changed: bits of [`library::what`](crate::library::what).
        pub what: u32,
    }
}

crate::event_data! {
    /// The catalog wanted ([`CATALOG_WANTED`]).
    pub struct CatalogWanted {
        /// Made anew in any case, not only where it was not made since
        /// PaperPaw started.
        pub anew: bool,
    }
}

crate::event_data! {
    /// The book whose annotations changed ([`ANNOTATIONS_CHANGED`]).
    pub struct AnnotationsChanged {
        pub path: String,
    }
}

crate::event_data! {
    /// How long was read on a day ([`READ_TODAY`]).
    pub struct ReadToday {
        /// The day: days since 1970-01-01, by the local time.
        pub day: i32,
        /// Seconds read that day, as the reading statistics count
        /// them.
        pub seconds: u32,
    }
}

crate::event_data! {
    /// A word looked up in a book ([`WORD_LOOKED_UP`]).
    pub struct WordLookedUp {
        pub word: String,
        /// The book's language, e.g. `de`.
        pub language: String,
        /// The sentence the word is in.
        pub text: String,
        /// The book.
        pub path: String,
        pub title: String,
        /// Where the word is.
        pub section: u32,
        pub offset: u32,
    }
}

crate::event_data! {
    /// A place in a book to show ([`GOTO`]): where it begins, and
    /// where a part to mark ends (e.g. a search hit), if any.
    pub struct Goto {
        /// The book.
        pub path: String,
        pub section: u32,
        pub offset: u32,
    }
    optional {
        /// Where the part to mark ends ([`Goto::end`]).
        pub end_section: u32,
        pub end_offset: u32,
    }
}

impl Goto {
    /// Place `at` in the book at `path`, nothing marked.
    pub fn to(path: impl Into<String>, at: Locator) -> Self {
        Self::new(path.into(), at.section, at.offset)
    }

    /// The same place, with the text up to `end` marked.
    pub fn marking(mut self, end: Locator) -> Self {
        self.end_section = Some(end.section);
        self.end_offset = Some(end.offset);
        self
    }

    /// Where it begins.
    pub fn at(&self) -> Locator {
        Locator::new(self.section, self.offset)
    }

    /// Where the part to mark ends, if one is.
    pub fn end(&self) -> Option<Locator> {
        Some(Locator::new(self.end_section?, self.end_offset?))
    }
}

crate::event_data! {
    /// Text chosen in a book, and the action chosen for it
    /// ([`TEXT_CHOSEN`]).
    pub struct TextChosen {
        /// The action, from [`menus::SELECTION`](super::menus::SELECTION).
        pub action: Id,
        pub text: String,
        /// The book's language, e.g. `de`.
        pub language: String,
        /// The book, and where the text begins and ends.
        pub path: String,
        pub section: u32,
        pub offset: u32,
        pub end_section: u32,
        pub end_offset: u32,
    }
}

impl TextChosen {
    /// Where the text begins.
    pub fn start(&self) -> Locator {
        Locator::new(self.section, self.offset)
    }

    /// Just after its end.
    pub fn end(&self) -> Locator {
        Locator::new(self.end_section, self.end_offset)
    }
}

crate::event_data! {
    /// Where skimming asks the reader to go ([`SKIM`]), in the book
    /// at `path`: `page:+10` (pages on, or back with `-`), `permille:250`
    /// (that far into the book, in thousandths), `chapter:+1` (the
    /// chapter after, or before with `-1`), or `back` (where skimming
    /// began).
    pub struct Skim {
        pub path: String,
        pub to: String,
    }
}

crate::event_data! {
    /// Where the book being skimmed is ([`SKIMMED`], and the parameters
    /// the skimming overlay is opened with): its page as shown
    /// (`12 / 340`, or a percentage), the chapter's title, how far in
    /// it is (in thousandths).
    pub struct Skimmed {
        pub path: String,
        pub page: String,
        pub chapter: String,
        pub permille: u32,
    }
    optional {
        /// Where its chapters begin, in thousandths, comma-separated.
        pub marks: String,
    }
}

crate::event_data! {
    /// A link of a book to follow ([`FOLLOW_LINK`]): its target as the
    /// book has it (`notes.xhtml#n3`, a web address).
    pub struct FollowLink {
        pub path: String,
        pub href: String,
    }
}

crate::event_data! {
    /// The words of the page shown are wanted ([`PAGE_WORDS_WANTED`],
    /// see [`reading::ask_page_words`](crate::reading::ask_page_words)):
    /// the reader answers with the event `reply`.
    pub struct PageWordsWanted {
        pub reply: Id,
    }
}

crate::event_data! {
    /// A decorated word that was tapped, or chosen with the buttons
    /// ([`DECORATION_TAPPED`]): the decorator's mark it is in.
    pub struct DecorationTapped {
        /// The decorator, and the role of its mark.
        pub decorator: Id,
        pub role: Id,
        /// The words of the mark, as on the page.
        pub text: String,
        /// The book, and where the mark begins and ends.
        pub path: String,
        pub section: u32,
        pub offset: u32,
        pub end_section: u32,
        pub end_offset: u32,
    }
}

impl DecorationTapped {
    /// Where the mark begins.
    pub fn start(&self) -> Locator {
        Locator::new(self.section, self.offset)
    }

    /// Just after its end.
    pub fn end(&self) -> Locator {
        Locator::new(self.end_section, self.end_offset)
    }
}

crate::event_data! {
    /// The room of a page layer that was tapped, or one of the items it
    /// drew ([`PageLayerSpec::items`](crate::layers::PageLayerSpec::items)),
    /// or chosen with the buttons ([`PAGE_LAYER_TAPPED`]).
    pub struct PageLayerTapped {
        pub layer: Id,
        /// The book.
        pub path: String,
        /// Where: the point tapped, the room's middle when chosen with
        /// the buttons.
        pub x: i32,
        pub y: i32,
    }
    optional {
        /// The start of the line beside it, if any (see
        /// [`PageView::line_at`](crate::layers::PageView::line_at)).
        pub section: u32,
        pub offset: u32,
        /// The item tapped or chosen, by the id the layer gave it
        /// ([`LayerItem::id`](crate::layers::LayerItem)); none for its
        /// room.
        pub item: u32,
    }
}

impl PageLayerTapped {
    /// The start of the line beside it, if any.
    pub fn line(&self) -> Option<Locator> {
        Some(Locator::new(self.section?, self.offset?))
    }
}

crate::event_data! {
    /// A lookup provider's answer that came later
    /// ([`LOOKUP_ANSWERED`], see [`lookup`](crate::lookup)): its
    /// entry for the word, none where `text` is empty.
    pub struct LookupAnswered {
        pub provider: Id,
        pub word: String,
        /// Where it is from, shown above it.
        pub source: String,
        pub text: String,
    }
    optional {
        /// The screen that shows more of it
        /// ([`LookupEntry::more`](crate::lookup::LookupEntry::more)).
        pub more: Id,
    }
}

crate::event_data! {
    /// The reading app of a book or a folder is to be chosen
    /// ([`READING_APP_WANTED`], see
    /// [`readers::ask`](crate::readers::ask)).
    pub struct ReadingAppWanted {
        pub path: String,
        pub folder: bool,
    }
    optional {
        /// Only to open the book this time, not kept
        /// ([`readers::ask_once`](crate::readers::ask_once)).
        pub once: bool,
    }
}

crate::event_data! {
    /// A document that was not ready is now ([`DOCUMENT_READY`]).
    pub struct DocumentReady {
        pub path: String,
    }
}

crate::event_data! {
    /// A book's notes, exported ([`NOTES_EXPORTED`]).
    pub struct NotesExported {
        /// The file written (Markdown).
        pub path: String,
        /// The book's title.
        pub title: String,
    }
}

crate::event_data! {
    /// The menu that was redefined or reset ([`MENU_CHANGED`]).
    pub struct MenuChanged {
        pub id: Id,
    }
}

crate::event_data! {
    /// The widget whose content changed ([`WIDGET_CHANGED`]).
    pub struct WidgetChanged {
        pub id: Id,
    }
}

crate::event_data! {
    /// A network that could not be joined ([`WIFI_FAILED`]).
    pub struct WifiFailed {
        pub ssid: String,
        pub reason: crate::wifi::JoinFailure,
    }
}

/// After the home screen has been opened at start-up. Plugins use it to
/// e.g. reopen the last book.
pub const STARTED: Event<()> = Event::new("core.started");
/// After a setting changed: which ([`SettingChanged`]).
pub const SETTING_CHANGED: Event<SettingChanged> = Event::new("core.setting-changed");
/// Text typed for a setting, asked with its parameter
/// [`ID`](super::params::ID) (as [`ui::setting_opens`](crate::ui::setting_opens)
/// asks): the kernel sets the setting to it, trimmed.
pub const SETTING_TYPED: Event<crate::replies::Typed> = Event::new("core.setting-typed");
/// Text typed from outside the screen, by keys of a keyboard or a
/// remote control for tests ([`TypeText`]): the screen bound to
/// [`slots::KEYBOARD`](super::slots::KEYBOARD) adds it to its field as if
/// typed on its keys (characters that are no text left out) and, with
/// `done`, confirms it. Sent only while that screen is shown.
pub const TYPE_TEXT: Event<TypeText> = Event::new("core.type-text");
/// The focus of the buttons moved to another part of the screen
/// ([`FocusChanged`]): for screen readers of one's own. Worked out
/// only while a plugin listens or the focus is spoken.
pub const FOCUS_CHANGED: Event<FocusChanged> = Event::new("core.focus-changed");
/// A book was opened for reading ([`Reading`]). Every reader plugin
/// emits it, so home screens, statistics or sync plugins work with any
/// reader. Not for documents in hidden folders
/// ([`files::is_hidden`](crate::files::is_hidden)), e.g. web pages:
/// they are no books.
pub const BOOK_OPENED: Event<Reading> = Event::new("core.book-opened");
/// The reader moved to another page ([`Reading`]).
pub const PAGE_TURNED: Event<Reading> = Event::new("core.page-turned");
/// The book read was closed (the reader left, or another book
/// opened): where it was left ([`Reading`]).
pub const BOOK_CLOSED: Event<Reading> = Event::new("core.book-closed");
/// The last page of a book was reached, once each time it is opened
/// ([`Reading`]).
pub const BOOK_FINISHED: Event<Reading> = Event::new("core.book-finished");
/// A book file was moved or renamed ([`BookMoved`]). Plugins that keep
/// data by a book's path move it along (progress, notes, statistics).
pub const BOOK_MOVED: Event<BookMoved> = Event::new("core.book-moved");
/// A book file was deleted ([`BookRemoved`]): what plugins keep for it
/// goes ([`Registrar::add_book_data`](crate::Registrar::add_book_data)
/// folders by the kernel; other data by the plugin). Whoever deletes a
/// book emits it.
pub const BOOK_REMOVED: Event<BookRemoved> = Event::new("core.book-removed");
/// Books came into the library or left it (received, downloaded,
/// copied, deleted): views of the library read it again. Plugins that
/// change the library emit it ([`library::changed`](crate::library::changed));
/// a move is [`BOOK_MOVED`].
pub const LIBRARY_CHANGED: Event<()> = Event::new("core.library-changed");
/// A book came into the library ([`BookAdded`]): received, downloaded,
/// copied by a plugin. Whoever brings a book emits it
/// ([`library::added`](crate::library::added)); views may show it at
/// once, the catalog is made anew.
pub const BOOK_ADDED: Event<BookAdded> = Event::new("core.book-added");
/// Something of a book changed ([`BookChanged`]): its details, how far
/// it is read, its cover, whether it is a favourite, a plugin's field.
/// Sent by the kernel for the reading events and by plugins that change
/// a book ([`library::changed_book`](crate::library::changed_book));
/// views show the book anew.
pub const BOOK_CHANGED: Event<BookChanged> = Event::new("core.book-changed");
/// An action of the book menu ([`menus::BOOK`](super::menus::BOOK)) was
/// chosen for a book ([`BookChosen`]), sent just before it runs: the
/// plugin whose action it is does it for that book.
pub const BOOK_CHOSEN: Event<BookChosen> = Event::new("core.book-chosen");
/// The catalog of the library was made anew
/// ([`library::catalog`](crate::library::catalog)): views of it read it
/// again.
pub const CATALOG_CHANGED: Event<()> = Event::new("core.catalog-changed");
/// The catalog is wanted ([`CatalogWanted`],
/// [`library::request_catalog`](crate::library::request_catalog)):
/// the kernel walks the library in the background where needed.
pub const CATALOG_WANTED: Event<CatalogWanted> = Event::new("core.catalog-wanted");
/// The values shown without being kept changed, or ended (see
/// [`Command::PreviewSettings`](crate::Command::PreviewSettings)).
pub const PREVIEW_CHANGED: Event<()> = Event::new("core.preview-changed");
/// The bookmarks or highlights of a book changed
/// ([`annotations`](crate::annotations), [`AnnotationsChanged`]).
/// Readers show them anew, syncs pass them on.
pub const ANNOTATIONS_CHANGED: Event<AnnotationsChanged> = Event::new("core.annotations-changed");
/// Asks the reader to show a place in the book it has open ([`Goto`]);
/// any reader takes it. Sent by the table of contents, bookmarks,
/// search, syncs of the reading position.
pub const GOTO: Event<Goto> = Event::new("core.goto");
/// A book's notes were exported as a file ([`NotesExported`]), for
/// plugins that take them further (a notes app, a sync). Readers send
/// it; its form is Markdown.
pub const NOTES_EXPORTED: Event<NotesExported> = Event::new("core.notes-exported");
/// An action was chosen for text chosen in a book ([`TextChosen`]),
/// just before it runs: the plugin whose action it is reacts (e.g. a
/// "Share" action needs no more than a menu entry in
/// [`menus::SELECTION`](super::menus::SELECTION) and this event).
pub const TEXT_CHOSEN: Event<TextChosen> = Event::new("core.text-chosen");
/// A word a decorator marked was tapped, or chosen with the buttons
/// ([`DecorationTapped`]); sent for decorators that take taps (see
/// [`decorations`](crate::decorations)), whose plugin answers.
pub const DECORATION_TAPPED: Event<DecorationTapped> = Event::new("core.decoration-tapped");
/// The room of a page layer was tapped, or an item it drew, or either
/// was chosen with the buttons ([`PageLayerTapped`]); sent for layers
/// that take taps or have items (see [`layers`](crate::layers)), whose
/// plugin answers.
pub const PAGE_LAYER_TAPPED: Event<PageLayerTapped> = Event::new("core.page-layer-tapped");
/// A lookup provider that answered later ([`LookupAnswered`], see
/// [`lookup`](crate::lookup)) has its entry, for a query that names
/// no reply of its own (`LookupQuery::reply`): the screen that asked
/// shows it.
pub const LOOKUP_ANSWERED: Event<LookupAnswered> = Event::new("core.lookup-answered");
/// A document that a format could not open yet
/// ([`DocumentError::NotReady`](crate::document::DocumentError), e.g.
/// still coming in) is there now ([`DocumentReady`]): its format's
/// plugin says so, readers waiting for it open it.
pub const DOCUMENT_READY: Event<DocumentReady> = Event::new("core.document-ready");
/// The reading app of a book or a folder is to be chosen
/// ([`ReadingAppWanted`]): the kernel lists the readers that fit.
pub const READING_APP_WANTED: Event<ReadingAppWanted> = Event::new("core.reading-app-wanted");
/// Skimming asks the reader to go somewhere in the book ([`Skim`]).
pub const SKIM: Event<Skim> = Event::new("core.skim");
/// Where the book being skimmed is now ([`Skimmed`]): the reader tells
/// the skimming overlay after each move.
pub const SKIMMED: Event<Skimmed> = Event::new("core.skimmed");
/// A link of the open book is to be followed ([`FollowLink`]), as a
/// tap on it would: the reader goes there (or opens a web address in
/// the browser). A footnote shown over the page sends it to go to the
/// note.
pub const FOLLOW_LINK: Event<FollowLink> = Event::new("core.follow-link");
/// The words of the page shown are wanted ([`PageWordsWanted`]):
/// a reader with a book open answers with
/// [`reading::PageWords`](crate::reading::PageWords).
pub const PAGE_WORDS_WANTED: Event<PageWordsWanted> = Event::new("core.page-words-wanted");
/// A word of a book was looked up ([`WordLookedUp`]).
pub const WORD_LOOKED_UP: Event<WordLookedUp> = Event::new("core.word-looked-up");
/// How long was read today ([`ReadToday`]), as the plugin that keeps
/// the reading statistics counts it (`stats`): sent as it records
/// reading, as it starts and as the device learns the date, when the
/// number changed. Plugins that show it (a reading goal) keep the
/// last one: on a day after its `day`, nothing was read yet.
pub const READ_TODAY: Event<ReadToday> = Event::new("core.read-today");
/// What a widget shows changed by other means than the events screens
/// already follow, e.g. a tap on it turned to another quote or ticked
/// an item off ([`WidgetChanged`]): screens showing it read it anew
/// ([`Widget::update`](crate::widgets::Widget::update)). See
/// [`widgets::changed`](crate::widgets::changed).
pub const WIDGET_CHANGED: Event<WidgetChanged> = Event::new("core.widget-changed");
/// A data plugin was loaded or removed while running
/// ([`Command::LoadPlugin`](crate::Command::LoadPlugin),
/// [`Command::RemovePlugin`](crate::Command::RemovePlugin)): lists of
/// plugins, fonts and presets read them anew.
pub const PLUGINS_CHANGED: Event<()> = Event::new("core.plugins-changed");
/// Another screen is shown on top ([`ScreenShown`]): one opened, or
/// the one below a screen closed. The whole stack is in
/// [`Env::stack`](crate::Env::stack). For plugins that subscribe:
/// screens learn that they show from [`Screen::on_show`](crate::Screen::on_show).
pub const SCREEN_SHOWN: Event<ScreenShown> = Event::new("core.screen-shown");
/// A menu was redefined or reset ([`MenuChanged`]).
pub const MENU_CHANGED: Event<MenuChanged> = Event::new("core.menu-changed");
/// The device goes to sleep or the app to the background: a moment to
/// save or send what is pending.
pub const SUSPEND: Event<()> = Event::new("core.suspend");
/// Back from [`SUSPEND`]. Where a sleep restarts the device (the X4s cut
/// their power to sleep), it follows [`STARTED`] at the start that ends
/// the sleep, so a plugin that does something on waking needs nothing
/// more; a power-up with no sleep before it sends [`STARTED`] alone.
pub const RESUME: Event<()> = Event::new("core.resume");
/// PaperPaw stops (the device restarts, or an app closes for good):
/// save what is pending, as at [`SUSPEND`]; the device may lose its
/// power right after. No [`RESUME`] follows, and it is no sleep.
pub const STOPPING: Event<()> = Event::new("core.stopping");
/// The battery or the time zone changed; status lines redraw.
pub const STATUS_CHANGED: Event<()> = Event::new("core.status-changed");
/// The battery was measured again ([`Env::status`](crate::Env::status)),
/// about once a minute where the device measures it: for plugins that
/// record its use. Nothing need be drawn anew.
pub const BATTERY_MEASURED: Event<()> = Event::new("core.battery-measured");
/// The Wi-Fi state changed ([`Env::wifi`](crate::Env), see
/// [`wifi`](crate::wifi)).
pub const WIFI_CHANGED: Event<()> = Event::new("core.wifi-changed");
/// Bluetooth changed: the radio, a bonded device, a link
/// ([`Env::bluetooth`](crate::Env::bluetooth)).
pub const BLUETOOTH_CHANGED: Event<()> = Event::new("core.bluetooth-changed");
/// Networks were found ([`Wifi::networks`](crate::wifi::Wifi)).
pub const WIFI_SCANNED: Event<()> = Event::new("core.wifi-scanned");
/// Joining a network failed ([`WifiFailed`]).
pub const WIFI_FAILED: Event<WifiFailed> = Event::new("core.wifi-failed");
/// A plugin's request, server or connection waits for the network,
/// where PaperPaw manages Wi-Fi and is in none: e.g. to join one.
pub const NETWORK_WANTED: Event<()> = Event::new("core.network-wanted");
/// Nothing uses the network any more (no request, server or
/// connection), where PaperPaw manages Wi-Fi: e.g. to switch the radio
/// off a little later.
pub const NETWORK_IDLE: Event<()> = Event::new("core.network-idle");
/// An install of a firmware update went on, ended or failed
/// ([`Firmware::state`](crate::firmware::Firmware), see
/// [`firmware`](crate::firmware)).
pub const FIRMWARE_CHANGED: Event<()> = Event::new("core.firmware-changed");
/// The power level or mode changed ([`power`](crate::power)), e.g. by
/// the battery in Automatic.
pub const POWER_CHANGED: Event<()> = Event::new("core.power-changed");
/// A plugin's declared work was taken out because it can never run
/// (its network switched off, not declared in its manifest), sent to
/// that plugin; work only held back stays queued (see
/// [`work`](crate::work)).
pub const WORK_REFUSED: Event<crate::work::WorkRefused> = Event::new("core.work-refused");
/// What held work or requests back no longer does (the mode changed,
/// the reader switched a plugin's network on, went online): a plugin
/// told [`Retry::WhenAllowed`](crate::work::Retry::WhenAllowed) may
/// try again.
pub const WORK_ALLOWED: Event<()> = Event::new("core.work-allowed");
/// A plugin's declared work changed its state or its progress (at most
/// once a second), sent to that plugin, e.g. for its page; the work's
/// id comes as [`params::ID`](super::params::ID).
pub const WORK_CHANGED: Event<()> = Event::new("core.work-changed");
