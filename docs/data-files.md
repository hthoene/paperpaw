# Files on the card

What PaperPaw keeps on a device's storage, and how each file stays
readable as PaperPaw changes. PaperPaw keeps its own files in
`.paperpaw`, named by `paperpaw_sdk::data::DIR`. Each plugin keeps its
files in `.paperpaw/plugins/<id>`, named by `Env::plugin_dir`. These are paths in the shell's storage: a shell may
keep the folder elsewhere than the books, e.g. in an app's own storage.
Every file is one of three kinds:

- **Data**: what the reader made or chose (settings, positions, notes,
  statistics). Every later version reads it. Its format has a version
  (`paperpaw_sdk::data::Format`): a text file says it in its first line,
  `# format 2`, and one without that line is version 1. A change that
  older readers would misread raises the version and adds the step from
  the version before; files are upgraded as they are read and written in
  the new format when they are saved next. Adding a key to an ini file
  needs no new version: readers skip keys they do not know. A version of
  PaperPaw older than a file (after going back to an older update) leaves
  the file as it is. Binary data files take a new name for a new version
  instead. Each version of each format has a file in a `testdata` folder
  that the tests read.
- **Cache**: made from the books or the network, made again when missing.
  Each carries a stamp of what it was made from and by which version of
  its maker; with another stamp it is made anew. Never upgraded.
- **Foreign**: formats of other programs (KOReader, Calibre), kept as
  those programs define them.

Reading positions and annotations are **locators** (ADR 0007). A
locator is a section, such as an EPUB's spine item, and an offset into
that section's source data. It does not point into the text a format
plugin made of it. So locators stay valid when a plugin converts books
differently; a plugin must keep that mapping.

## Data

| File | Owner | Format | Tested |
|------|-------|--------|--------|
| `paperpaw.ini` | kernel | ini: settings by plugin, `[plugins]` switched on or off, `[menus]`, `[keymap.*]`, `[fonts]`, `[slots]` | version 1 |
| `.paperpaw/power.ini` | kernel | ini: the power mode worn (`mode`), whether *Automatic* chose it, the settings the reader took back from it (`released`) | |
| `.paperpaw/cleaned-up.txt` | kernel | when the storage was last cleaned up by itself, in seconds since 1970 | |
| `.paperpaw/secrets.txt` | kernel, SDK `secrets` | a line per password or key: id, label, flags, value veiled (tab separated) | version 1 |
| `.paperpaw/library/books/<hash>.ini` | SDK `library` | ini: a book's details, cover, progress, fields of plugins (e.g. `end-of-book.rating`, "1" to "5"; the details read again when their `version` is older) | version 1 |
| `.paperpaw/library/opened.txt`, `favourites.txt` | SDK `library` | a path per line | version 1 |
| `.paperpaw/readers/<hash>.txt`, `.paperpaw/readers-folders.txt` | SDK `readers` | the reading app chosen for a book; for folders a line each: reading app, folder (tab separated) | |
| `.paperpaw/annotations/<hash>.txt` | SDK `annotations` | a line per bookmark or highlight (tab separated; 8 fields before times were kept, 10 since). Version 2 (`# format 2`, written once any annotation says where it was made) has 13: after the times, the conversion its offsets are in (16 hex digits; `?` for one kept from before conversions, never moved) and its start and end as XPointers (empty where there are none), so it is found again in another conversion; one that cannot be found stays as it is. A line `# book <size> <md5>` after the version's says which book they are of by its content (its size and KOReader's partial MD5, the reader writes it when it opens the book); PaperPaw of before keeps it as a line it does not read, so the version stays. Before all of them are removed because KOReader's file has none any more (the reader agreed, asked in the book), the file is copied to `<hash>.txt.removed` (or `<hash>.txt (2).removed` and on) | versions 1 and 2 |
| `.paperpaw/plugins/reader/state.ini` | reader | ini: the book read last | version 1 |
| `.paperpaw/positions/<hash>.txt` | SDK `positions` | ini: where a book was left (`[position]` section, offset, percent, `conversion`, `xpointer` when it was closed, `updated`, `book`: which book it is of by its content, `<size> <md5>`, KOReader's partial MD5); one the reader cannot read is kept as `<hash>.txt.bad` (or `.bad2` and on) before a new one is written | |
| `.paperpaw/plugins/reader/books/<hash>/progress.ini` | reader | ini: the position before positions were shared; read while a book has none | version 1 |
| `.paperpaw/plugins/reader/status-lines.txt` | reader | the status line chosen last at the top, then at the bottom, a line each | version 1 |
| `.paperpaw/plugins/reader/books/<hash>/settings.ini` | kernel (a book's own settings) | ini, as `paperpaw.ini` | version 1 |
| `.paperpaw/plugins/stats/books.ini` | stats | ini: a section per book (title, authors, md5, pages, language, series, `last-open`, path; `finished`, when it was last read to the end, added later) | version 1 |
| `.paperpaw/plugins/stats/pages.bin` | stats | 16 bytes per stay on a page | version 1 |
| `.paperpaw/plugins/stats/webdav.ini` | stats | ini of earlier versions: its password moves to the passwords and keys; the file goes at a later start, once the password is kept there | version 1 |
| `.paperpaw/plugins/profiles/profiles.ini` | profiles | ini: a section per profile, its name and values | versions 1 and 2 |
| `.paperpaw/plugins/profiles/last.txt` | profiles | the profile applied last | version 1 |
| `.paperpaw/plugins/rules/rules.txt` | rules | a rule per line: trigger, effect, target, on, condition (tab separated) | version 1 |
| `.paperpaw/plugins/vocabulary/words.txt` | vocabulary | a word per line: word, language, place, book, title, sentence (tab separated) | version 1 |
| `.paperpaw/plugins/vocabulary/cards/decks.txt`, `deck-<id>.txt` | vocabulary | `paperpaw-decks 1` / `paperpaw-cards 1`, then a deck per line (id, name) or a card per line (id, due, last, stability, difficulty, reps, lapses, front, back, example; tab separated, texts escaped); and a line `next`, tab, the id the next deck or card gets (after the version line in `decks.txt`, after the cards in a deck), so no id is given twice. A file without it (from before) goes on after the highest id; the higher of the two counts. Older versions skip the line (a deck keeps it as it is), so the version stays 1 | |
| `.paperpaw/plugins/vocabulary/cards/answers.txt`, `due.txt`, `state.txt` | vocabulary | answers not yet written into their decks; when each deck's cards are due, by quarter hour; the time last known and the new cards shown that day | |
| `.paperpaw/plugins/vocabulary/cards/sources.txt` | vocabulary | `paperpaw-sources 1`, then a file of the folder `Flashcards` (settings can name another) a line, merged into a deck: file name, deck id, size, modification time, hash of the content (tab separated, the hash in hex). A sixth field, `1`, says the deck's switch *Remove missing cards* is on (off where the field is not there), and a seventh, the number of lines the last update left out (unreadable, or without room in the deck; 0 where it is not there): a save is refused while it is more than 0, and no card is removed after such an update. A hash of 0 marks a file that was tried and is no deck as it is (no cards, too big, UTF-16, too many equal fronts): it is told, not tried again until it changes. Lines a reader does not understand are kept as they are, and a ledger it cannot read (another version, over 128 KiB) is never written over. It lets the reader read only files that are new or changed, and find one renamed | version 1 |
| `Flashcards/<deck>.tsv` (the folder is a setting), `_<deck>.bak.tsv` | vocabulary | an exchange file *Save to card* writes and an update reads: optional byte order mark (`.csv` only), the line `#paperpaw:deck <id>`, the header `id`, `front`, `back`, `example` (separated by a tab, or the separator of the `.csv` it replaces), then a card on each line: its number, the texts, in quotes where they hold the separator, a quote or a line break. The `id` column is the cards' numbers only in a file whose marker names the deck it is read into; elsewhere it is an ordinary column. Written to `<file>.part` and put in place whole, after the file there was copied to `_<name>.bak.<extension>` the first time (a later save does not replace that copy unless the file in place was not written by a save of this deck); a save is refused while the last update left lines out; a `.csv` gets a byte order mark only if it had one or is new. What was learnt is not in it | |
| `.paperpaw/plugins/vocabulary/cards/looked-up.txt`, `adding.txt`, `asleep.txt` | vocabulary | words looked up waiting to become cards (word, sentence); the deck cards are being added to; the card the sleep screen showed (deck, id) | |
| `.paperpaw/plugins/lock/tries.txt` | lock | the wrong tries in a row, a number | version 1 |
| `.paperpaw/plugins/plugin-manager/screens.txt` | plugin-manager | the screens offered for a slot that were already asked about, `slot screen` a line | |
| `.paperpaw/plugins/store/queue.txt`, `queue-quiet.txt` | store | the plugins waiting to be installed, a line each, the one whose turn it is first (marked `> `, those of the setup `~`); what became of the setup's, `+<id>` or `-<id>` a line | |
| `.paperpaw/plugins/store/author.txt` | store | the key the plugin library names for the author of the code plugin being downloaded, until it is installed | |
| `.paperpaw/plugins/<id>/install.txt` | SDK `install` | a download of a plugin not yet complete, in the folder of the plugin that installs (the store, the fonts browser) | |
| `.paperpaw/plugins/calibre/host.txt` | calibre | the address of the computer the reader chose to trust | version 1 |
| `.paperpaw/plugins/opds/catalogs.txt` | opds | a catalog per line: name, address, user, (password, empty), folder, `sync`, `webdav` | version 1 |
| `.paperpaw/plugins/opds/synced/<md5 of the address>.txt` | opds | the books a catalog's sync fetched, by their ids in the catalog, so that one deleted is not fetched again | |
| `.paperpaw/plugins/news/feeds.txt` | news | a feed per line: address, name | version 1 |
| `.paperpaw/plugins/browser/bookmarks.txt`, `history.txt` | browser | a page per line: title, address | version 1 |
| `.paperpaw/plugins/kosync/device.ini`, `account.ini` | kosync | ini: the device's id; the key of earlier versions (moved to the passwords and keys) | version 1 |
| `.paperpaw/plugins/wallabag/articles.txt`, `finished.txt` | wallabag | an article per line: id and path; ids read to the end | version 1 |
| `.paperpaw/plugins/wallabag/account.txt` | wallabag | the server and the user name the lists belong to, one a line: another account starts them anew | |
| `.paperpaw/plugins/joplin/sent.txt`, `sending.txt`, `waiting.txt`, `notebook.txt` | joplin | a book per line: notes file, note id, MD5 of the note's text (tab separated); the same for notes sent whose answer did not come; a notes file per line; the notebook's name and id | version 1 |
| `.paperpaw/plugins/readwise/waiting.txt` | readwise | the notes files whose highlights wait to be sent, a path per line | |
| `.paperpaw/plugins/battery/log.csv` | battery | CSV with a header | version 1 |
| `.paperpaw/slept` | X4 firmware (`paperpaw_x4::slept`) | an empty file: written as the device goes to sleep, removed by the next start, which then sends `core.resume` after `core.started` (the X4s' sleep ends in a start) | |
| `.paperpaw/crashes.txt` | X4 firmware | a line per panic: the version and why, written for bug reports, never read back | |
| `.paperpaw/plugins/battery/plugins.csv` | battery | CSV with a header, written for reading on a computer, never read back | |
| `.paperpaw/plugins/battery/log.old.csv`, `plugins.old.csv` | battery | the logs before, kept once a log grew large | |
| `.paperpaw/plugins/sleep/next.txt` | sleep | the next picture, counted | version 1 |
| `.paperpaw/plugins/reading-goal/today.txt` | reading-goal | the day (days since 1970, local time) and the seconds read on it, as the statistics told them | |
| `.paperpaw/plugins/setup/done` | setup | its presence | version 1 (the app's screen tests) |
| `.paperpaw/plugins/help/seen.txt` | help | a screen per line whose hints were shown | |
| `.paperpaw/plugins/update/installing.txt`, `offered.txt` | update | a path and a version, a line each (`offered.txt`: `online` and the version for an update the server named) | version 1 |
| `.paperpaw/plugins/update/waiting.txt` | update | an update that waits for plugins: its version, `since <unix seconds or ->`, `checked <unix seconds or ->`, `asked`, `seen` and `told` lines where set, then what it waits for, `plugin <id> <version>` and `crate <name>` lines | version 1 |
| `.paperpaw/plugin-authors.txt`, `plugin-code.txt` | SDK `native` | a code plugin per line: its id, a tab, its author's key remembered at the first install; its id, a tab, the SHA-256 of its file's header as installed (only such a file is loaded) | |
| `.paperpaw/plugin-origin.txt` | SDK `install` | the versions a store installed, a line each: a plugin's id, a tab, its version (the last four); a plugin counts as the store's, and gets its updates, only while its version is one of them | |
| `plugins/<id>/.install-marks` | SDK `install` | what an installer keeps for a plugin once it is in place (its code named as installed, its author, where it came from), handed over in the folder and kept by the kernel right after the install, or at the next start where that was cut short | |
| `plugins/.<id>.part` | SDK `install` | a plugin's files while they are downloaded or sent, until the kernel puts the folder in its place | |
| `.paperpaw/plugins/<installer>/install.txt`, `install.done`, `install.count` | SDK `install` | an installer's job, kept so that it goes on with no screen open; the job last done; how many jobs there were, so that none has a number twice | |
| `.paperpaw/plugins/store/code-told` | store | that the reader was told once what plugins with code are | |
| `.paperpaw/plugin-code-fetched.txt` | SDK `native` | a code plugin per line: its id, a tab, the SHA-256 of its file's header fetched for a firmware update (into `plugins/.<id>.next`), which the updated firmware installs at its start | |
| `.paperpaw/plugins/weather/weather.txt` | weather | the last forecast (place, coordinates, when, now and the days), first line `weather 1` | version 1 |
| `.paperpaw/plugins/widgets/todo.txt` | widgets | an item per line, done ones as `x <date> <text>` (as todo.txt has them); an open item that would read as done (beginning with `x `) kept with the date `1970-01-01` in front, where todo.txt has the day an item was made | version 1 |
| `.paperpaw/plugins/widgets/quotes.txt` | widgets | the reader's own quotes, a line each: `text \| author` | version 1 |
| `.paperpaw/plugins/widgets/quote.txt`, `picture.txt` | widgets | the day and the turns of the quote and the picture shown | version 1 |
| `.paperpaw/plugins/widgets/seen.bits`, `newest.txt` | widgets | the books *Next up* saw, by the hashes of their paths, as a set of bits; the books that came since, a few | |
| `.paperpaw/plugins/widgets/reading.ini` | widgets | ini: the book read last, its page and pages, the seconds read and the pages turned for the speed; `chapter` and `chapter-left` (the pages left in it) added later | version 1 |
| `.paperpaw/plugins/koreader/<hash>.txt` | koreader | what both sides had at the last exchange of annotations, a line each: `b` or `h`, start and end as section and offset, and a hash of the note, separated by spaces | |
| `.paperpaw/device-check.txt` | diagnostics | the device check's report in English, a line per step (`<step>: <outcome>`); also where an unfinished check goes on from | |
| `.paperpaw/device-check-light.txt` | diagnostics | the reader's own front light while the check tries other levels; set again from there, then removed | |

## Caches

| Files | Owner | Stamp |
|-------|-------|-------|
| `.paperpaw/plugins/format-epub/<hash>/` | format-epub | `CACHE_VERSION` and the book's size and start; HTML, Markdown, FB2 and CBZ in folders of their own, `html-<hash>/`, `md-<hash>/`, `fb2-<hash>/` and `cbz-<hash>/` |
| `.paperpaw/plugins/format-xtc/<hash>/` | format-xtc | the book's size and start |
| `.paperpaw/plugins/format-txt/<hash>.txt` | format-txt | a UTF-16 book as UTF-8; `<hash>.source`: the book's size and a hash of its first and last 4 KiB |
| `.paperpaw/plugins/widgets/picture.pixels` | widgets | `picture.stamp`: the picture's path and size, the screen's size and colour |
| `.paperpaw/logo.pixels` | SDK `logo` | `logo.stamp`: the picture's path and size, the size drawn, colour or not |
| `.paperpaw/library/covers/<hash>.thumb`, `<hash>.<variant>.thumb` | SDK `library` | covers made small for views of many books, made again when missing |
| `.paperpaw/plugins/sleep/cat-<side>.gray`, `picture.gray` | sleep | the sleeping cat decoded for a screen, its side in the name; the picture of the pictures folder shown last |
| `.paperpaw/plugins/sleep/overlay.gray`, `overlay-failed.txt` | sleep | `overlay.key`: the picture over the page and its size; why it did not show at the last sleep, written only when that changed |
| `.paperpaw/plugins/reading-goal/cat-<mood>-<bytes>.gray`, `own-<mood>-<bytes>.gray` | reading-goal | the mood's picture decoded (the shipped one, or the reader's own), the size of its file in the name |
| `.paperpaw/plugins/reader/books/<hash>/pages-<hash>.bin` | reader | the layout's settings, `paperpaw_layout::VERSION`, the book's size and the plugins' stylesheets, in the file's name; a new one removes the others once four are kept |
| `.paperpaw/plugins/reader/books/<hash>/headings.txt` | reader | the book's size on its first line: the headings found in a book without contents, found again when the size changes |
| `.paperpaw/library/catalog.txt`, `status.log`, `catalog.tmp`, `walk.list` | SDK `library` | the folders' contents; `status.log` holds how far books are read, favourites and books removed since, folded into the catalog once it is long; the other two only while a new catalog is made |
| `.paperpaw/plugins/stats/days.bin` | stats | the bytes of `pages.bin` it covers |
| `.paperpaw/plugins/dictionary/<hash>/` | dictionary | the dictionary's `.ifo` |
| `.paperpaw/plugins/news/headlines.txt`, `wallabag/entries.json`, `wallabag/entry.json`, `fonts-browser/catalog.json` | their plugins | fetched again |
| `.paperpaw/plugins/opds/tmp/<n>.<ext>` | opds | an answer to a request each, removed once it is handled; what is left goes at the next start, with the files of earlier versions |
| `.paperpaw/plugins/opds/covers/<md5 of the address>.gray` | opds | a catalog's cover decoded, by its address |
| `.paperpaw/plugins/store/index.txt`, `index.head`, `index.bin`, `search.bin`, `plugin-<id>.part`, `shot.gray` | store | the plugin library's list for this device, fetched again; a plugin's page and a screenshot as they are shown |
| `.paperpaw/plugins/store/list.part`, `sort.bin`, `shot-<n>.part` | store | the list and a screenshot while they are downloaded, the list's sort while it is sorted |
| `.paperpaw/plugins/joplin/info.json`, `locks.txt`, `found.md`, `note.md` | joplin | fetched or written again with each sending, removed after it |
| `.paperpaw/plugins/readwise/body.json`, `answer.json` | readwise | a request's body and Readwise's answer, written again with each request |
| `.paperpaw/plugins/update/offer.txt`; `updates/.download` and `updates/<firmware>-<version>.bin` at the top of the card | update | the update server's last answer, a download until it is whole, and the update downloaded; fetched again, and let go once the version is no longer newer |
| `.paperpaw/plugins/update/fetch.txt`, `fetch-current.txt`, `fetch-failed.txt`, `fetch-native.txt`, `plugins/.<id>.next/` | update | the code plugins left to fetch for a downloaded update, the one being fetched, those that could not be, the fingerprint and compiler the update's need; the folders fetched, installed by the updated firmware |
| `.paperpaw/plugins/about/licences.md`, `about/plugins.md`, `about/logo.png`, `plugin-manager/how.txt`, `store/how.txt` | about, plugin-manager, store | written when they are opened to read |
| `.paperpaw/tmp/request-<n>` | kernel | the body of a request to a page a plugin serves, until the plugin has it; those left from before go with the next request |
| `.paperpaw/code-room.txt` | SDK `native` | the room code plugins have, as the shell found it at this start |
| `.paperpaw/device-check.bin` | diagnostics | 128 KiB the check writes and reads back, removed after |
| `.paperpaw/plugins/browser/pages/<hash>.html` | browser | a page shown, by the hash of its address, its address as `<base href>` on its first line |
| `.paperpaw/plugins/fonts-browser/samples/` | fonts-browser | samples of the fonts offered; those no longer offered are removed |
| `.paperpaw/plugins/files/index.bin` | files | the library's index while a book covers its view, read back when the view shows again |
| `.paperpaw/plugins/android/<hash>/` | Android shell | a PDF's pages drawn; `source`: the file's size, its pages and the size drawn for |
| `.paperpaw/plugins/browser/download.tmp`, `calibre/dropped.part`, `news/page.tmp`, `news/feed-<hash>.tmp`, `wallabag/article.tmp`, `vocabulary/words.tmp`, `stats/incoming.sqlite3` | their plugins | a download or a file being written, until it is moved where it belongs or removed |
| `.paperpaw/plugins/stats/pages.new`, `books.new` | stats | `pages.bin` and `books.ini` while they are written anew: put in place at the start where the file itself is missing, else removed |
| `~<hash>.TMP`, `~<hash>.NEW` beside a file (the X4's card; seven letters from the file's name) | `paperpaw-fat` | a file while it is written: `.NEW` is whole and takes the file's place when its folder is first looked at after a start, unless the file was changed after `.NEW` was made (its time, size and first cluster then are kept in `.NEW`'s creation time); neither listed nor taken by name. `~PPMOVE.BIN` in the root (hidden, kept, not listed): the move being made, cleared after it, settled at the next start where it is not; where it cannot be, its two paths wait for the next start, all else goes on. Left by earlier versions, `~pp<n>.tmp`: removed at a write in their folder where empty or the same as a file beside them, else kept |

## A data plugin's files

A plugin without code is a folder, `plugins/<id>/`, with a `plugin.ini`
and the files below. The kernel reads them at the start
(`crates/paperpaw-core/src/data_plugin.rs`). Together they may take at
most 96 KiB, and the translations another 512 KiB. Each file may take
at most 64 KiB, and `plugin.ini` at most 4 KiB. The files of formats 3
may take at most 24 KiB of the 96 KiB, and `points/` holds at most 16
files. `paperpaw-dev check` says where a folder holds a file PaperPaw
does not read. Some files came with a later version of the data
formats: they are read only where `plugin.ini` says that version or a
later one (`formats = 3`), else left out, and PaperPaw says so.

### Which file for what

| To change | File | For example |
|-----------|------|-------------|
| any setting's default (sizes, fonts, the look, what a widget shows) | `settings.ini` | `[core]` `font-size = 14` |
| several settings chosen together (a power mode, a theme, a look of widgets) | `presets.ini` | `[night]` `kind = core.power-mode` |
| what buttons, taps and swipes do | `keymap.ini` | `[keymap.global]` `button.menu = core.refresh-screen` |
| an action of one's own, made of steps | `actions.ini` | `[night]` `run = core.night-mode=on` |
| what a menu holds, a menu of one's own | `menus.ini` | `[menu core.reader-menu]` `add = my.night after core.toc` |
| the icon of an action or a setting | `icons.ini` | `my.night = moon` |
| the words, or a whole new language | `i18n/<lang>.ini` | `[messages]` `my.night = Night reading` |
| fonts to read with, letters other fonts lack | `fonts/` (and `[extends]`, `[family]` in `plugin.ini`) | `fonts/Fredoka.ttf` |
| how books look | `styles.css` | `p { line-height: 1.6 }` |
| words marked on book pages | `marks.ini` | `[names]` `pattern = ...` |
| the home screen | `settings.ini` (`home.*`, see below) | `[home]` `page-1 = home.clock 0/0 4x1` |
| the logo (sleep screen, About, the logo widget) | a picture, named in `settings.ini` | `[core]` `logo = plugins/<id>/logo.png` |
| the sleep screen's pictures | a folder of pictures, named in `settings.ini` | `[sleep]` `show = sleep.show.picture`, `folder = plugins/<id>/sleep` |
| the pictures of the picture widget | a folder of pictures, named in `settings.ini` | `[widgets]` `picture.folder = plugins/<id>/pictures` |
| steps of the first-run setup | `setup.ini` | `[size]` `setting = core.font-size` |
| keyboard layouts, lines on *About* | `points/<point>.txt` | `points/keyboard.layouts.txt` |

There is no background picture for screens yet: no setting takes one,
so a plugin without code cannot set it.

| Path | Format | Read by, for | Since formats |
|------|--------|--------------|---------------|
| `plugin.ini` | ini: `[plugin]` with `id`, `name`, `description`, `version`, `formats`; `[extends]`, `[family]` and `[fonts]` (formats 3) where it brings fonts | the kernel: the plugin, its fonts, the fonts bound to roles | 1 |
| `i18n/<lang>.ini` | a translation catalog: `[meta]` (`language`, `name`, `plural`) and `[messages]` | the kernel: texts of any plugin, also a whole new language | 1 |
| `settings.ini` | ini: a section per plugin, a key per setting, as `paperpaw.ini` writes them | the kernel: new defaults for any plugin's settings (a theme, a home screen, a logo) | 1 |
| `keymap.ini` | ini: `[keymap.<layer>]` sections, `button.<name> = <action>` | the kernel: what buttons, taps and swipes do | 1 |
| `styles.css` | CSS | the reader: a stylesheet for books, after a book's own | 1 |
| `presets.ini` | ini: a section per preset, `kind`, `name`, `description`, then setting ids and values | the plugin that offers presets of that kind (power modes, themes, looks of widgets) | 1 |
| `fonts/*.ttf`, `.otf`, `.ttc` | font files, in `fonts/` or one folder below it | the kernel: fonts to read with, where scalable fonts are drawn | 1 |
| `fonts/*.ppfp` | font packs, named in `plugin.ini` (`[extends]`, `[family]`) | the kernel: letters other fonts lack, a family's faces, icons | 1 |
| pictures (`.png`, `.jpg`) | a file, or a folder of them, named in `settings.ini` (`logo = plugins/<id>/logo.png`) | the setting that names it: the logo, the sleep screen's pictures | 1 |
| `marks.ini` | ini: a section per role, `title`, one or more `pattern`, the look (`background`, `line`) | the reader: words marked on book pages | 2 |
| `menus.ini` | ini: another plugin's menu as `[menu <id>]` with `add = <entry> after <entry>`, `hide = <entry>`; a menu of one's own as `[<name>]` with `title` and `entries`; an entry a line | the kernel: entries of any menu | 3 |
| `actions.ini` | ini: a section per action, `title`, `icon`, `run` (a step a line, at most 16) | the kernel: actions made of steps | 3 |
| `icons.ini` | ini without sections: `<action or setting> = <icon name>` | the kernel: icons of actions and settings | 3 |
| `setup.ini` | ini: a section per step, `setting` or `screen`, `order`, `title`, `help` | the first-run setup: steps of its own | 3 |
| `points/<point>.txt` | text: a line each, `#` lines left out, at most 64 lines in all | the plugin that reads the point (`keyboard.layouts`, `about.lines`) | 3 |
| `how/<lang>.txt`, `*.md`, `*.txt` | text | people, and the plugin library (How it works); the kernel does not read them | |

A short example of each that a plugin of a few lines might ship:

```ini
# plugin.ini
[plugin]
id = night-reader
name = Night reader
description = Reading in the dark, one tap away.
version = 1.0.0
# The version of the data formats the files are written in: 1 for
# settings, presets, key maps, translations, fonts, styles and pictures;
# 2 where there is a marks.ini; 3 where there is a menus.ini,
# actions.ini, icons.ini, setup.ini, points/ or [fonts] here.
formats = 3

# settings.ini
[core]
font-size = 14

# keymap.ini
[keymap.global]
button.menu = night-reader.night

# presets.ini
[night]
kind = core.power-mode
name = Night
power.level = offline

# marks.ini
[names]
title = Names
pattern = \b(?:Mr|Mrs|Miss)\. [A-Z]\w+
background = yellow

# actions.ini
[night]
title = Night reading
icon = moon
run = core.night-mode=on
run = core.font-size=14
run = notify:night-reader.good-night

# i18n/en.ini
[meta]
language = en
name = English
plural = one-other

[messages]
night-reader.good-night = Good night, sleep well

# menus.ini
[menu core.reader-menu]
add = night-reader.night after core.toc

# icons.ini
night-reader.night = moon

# setup.ini
[size]
setting = core.font-size
order = 35
```

```text
# points/about.lines.txt
Night reader is made by Ada.
```

### settings.ini

A setting's id is its plugin, a dot and the rest: in `settings.ini` the
plugin is the section and the rest is the key, so `core.font-size` is
`font-size` in `[core]`, and `widgets.picture.folder` is
`picture.folder` in `[widgets]`. The values are written as
`paperpaw.ini` writes them:

- a switch: `true` or `false` (also `on`, `off`, `yes`, `no`, `1`, `0`);
- a number: a whole number within the setting's range. The settings
  list shows the range, and `paperpaw-emu --registry registry.json`
  writes it as `min` and `max`;
- a choice: the whole id of the option, as `files.view.covers`, not
  `covers`;
- a list: ids separated by commas;
- a text, a folder or a file: as it is, a path from the top of the card
  (`plugins/<id>/...`).

A value PaperPaw does not take (a number out of its range, a choice
that is no option, a setting no plugin has) is not taken, and PaperPaw
says why; it is not made to fit. A default that keeps the device safe
(the lock's) is taken only from its own plugin.

These are defaults, nothing more: what the person chose themselves (in
the settings, kept in `paperpaw.ini`) stays, whichever plugin brings
another default, and switching the plugin off in the plugin manager
brings the defaults back as they were without it, after the restart.

### actions.ini

An action runs at most 16 steps in order, a step a `run` line. A line
is one step as it is, commas and all, so a value with a comma needs no
escape. A step is one of these: a setting and a value, as
`core.night-mode=on`; `open:<slot>`; `notify:<key>`, which shows the
text of that key in the plugin's translations, in the reader's
language (a key the translations lack is shown as it is);
`emit:<event>`, with an event of the plugin's own named
`<plugin>.<name>`; or the id of another action that is not in the
same file. A step setting a value of the wrong kind is passed over, and one
setting what keeps the device safe is never done. An action of more
than 16 steps, or one that runs an action of its own file, is no
mistake PaperPaw passes over: the file is refused, and the plugin with
it, which the plugin manager then shows as *Error*. `paperpaw-dev check`
finds this before, as `package.actions`, and also steps written on one
line separated by commas (one step to PaperPaw) and a `notify:` key
the plugin's translations lack. It also finds a default in
`settings.ini` that PaperPaw would not take, as `package.settings`.

```ini
[night]
title = Night reading
run = core.night-mode=on
run = my.greeting=Hello, reader
run = notify:my.good-night
```

### menus.ini

A section's name says whose menu it is. `[menu <id>]` is another
plugin's menu, as `[menu core.reader-menu]`: `add = <entry>` puts an
entry into it, `add = <entry> after <entry>` beside another, and
`hide = <entry>` takes one out. Any other section is
a menu of the plugin's own, named like an action (`[tools]` in the
plugin `my` is `my.tools`): `title` names it, `entries` lists what it
holds, and other menus list it by its id.

```ini
[menu core.reader-menu]
add = my.night after core.toc
add = my.tools

[menu core.reader-more]
hide = stats.open

[tools]
title = My tools
entries = my.night
entries = core.library
```

`title` or `entries` in a `[menu <id>]` section is refused, with the
file and the plugin, as a step too many is in `actions.ini`. `add` or
`hide` in a section without `menu` in front is read as before, as
another plugin's menu; `paperpaw-dev check` says to write it as
`[menu <id>]` (`package.menus`).

### Lists

A list in `menus.ini` and `actions.ini` is a key a line: `run`,
`entries`, `add` and `hide` once for each item, in order. `entries`
and `hide` also read ids separated by commas on one line, as they
were first written. A setting's value in `settings.ini` and
`presets.ini` is another thing: it is written as `paperpaw.ini` keeps
it, a list there being ids separated by commas.

### A home screen without code

The home screen keeps its pages in settings (`plugins/home/src/pages.rs`).
So a `settings.ini` can bring a home screen of its own as new defaults:
the pages `home.page-1` to `home.page-8` and the grid they are laid on.
The first page always shows; the others show where they hold
something. Each page is a list
of items, separated by commas: a widget's id, or `tool:` and an action's
id, then its column and row and its size in cells (`<column>/<row>
<width>x<height>`, counted from 0). An item without a place goes into
the first free cells, as wide as the grid.

```ini
# settings.ini
[home]
columns = 4
rows = 0
status-bar = true
page-1 = home.clock 0/0 4x1, home.continue 0/1 4x2, tool:core.library 0/3 2x1, tool:store.open 2/3 2x1
page-2 = home.tools
footer.dots = true
footer.pencil = true
```

| Setting | Value |
|---------|-------|
| `home.page-1` to `home.page-8` | a page's items, as above |
| `home.columns` | columns of the grid, 2 to 6 |
| `home.rows` | rows, at most 12; 0 makes cells about as high as wide |
| `home.status-bar` | whether the status line shows above the grid |
| `home.footer.dots`, `home.footer.pencil` | the page dots and the pencil that edits the page, below it |
| `home.footer.items` | status items of any plugin below the page, their ids separated by commas |

The widgets, their ids and the actions a tool can run are those that
`paperpaw-emu --registry registry.json` lists under `widgets` and
`actions`. The home screen's own widgets are `home.clock`, `home.date`,
`home.status`, `home.continue`, `home.recent` and `home.tools`.
`home.tools` shows every tool not placed otherwise, a cell each. Defaults change only what the reader has not
chosen: a reader who arranged the home screen by hand keeps that.

A widget's own settings take defaults the same way, by the setting's
id: `home.clock.date` (whether the clock shows the date) is `clock.date`
in `[home]`, `widgets.picture.folder` is `picture.folder` in
`[widgets]`. Such a default holds for every place of the widget that
has no value of its own. A widget placed twice has its second place
named with `@2`, as `widgets.clock@2` on the page. That place has
settings of its own, such as `widgets.clock@2.face`, and those take no
default from a plugin.

## Foreign

| Files | Owner | Defined by |
|-------|-------|------------|
| `.paperpaw/plugins/stats/statistics.sqlite3` and the files of a sync | stats | KOReader's statistics |
| `<book>.sdr/metadata.<ext>.lua` | koreader | KOReader's book settings |
| `<book>.sdr/<book file>.annotations.lua` | koreader | KOReader, which takes the annotations over and removes the file |
| `.paperpaw/plugins/wifi/picked.txt` | wifi | the network picked last on the Wi-Fi page, its name in one line: joined first when in reach (setting *Join first*). |
| `wifi.ini` (at the top of the card) | wifi | networks to join, `ssid = ...` and `password = ...` lines. The firmware writes them there at a start from what the web flasher gave, or the reader writes them by hand. At a start they are kept with the passwords and keys, and the file is removed. |
| `.paperpaw/plugins/calibre/books/*.json`, `device.json` | calibre | Calibre's device driver |
