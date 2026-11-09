//! A book downloaded from a catalog: in the best format PaperPaw reads,
//! into the catalog's folder in the library, with what the catalog
//! says of it; then read at once, or later. It comes into a file of its
//! own and goes into the library only once it is whole, so a download
//! stopped (*Back*) or failed leaves nothing there.

use super::*;

pub(super) const DOWNLOADED: Event<HttpReply> = Event::new("opds.downloaded");

/// How far a download came, while it comes.
pub(super) const PROGRESS: Event<HttpProgress> = Event::new("opds.download-progress");

/// A book being downloaded: its title, and how much came of how much
/// (where the server said).
pub(super) struct Download {
    title: String,
    received: u64,
    total: Option<u64>,
    /// When the line was last drawn, and the share it showed.
    shown: Option<(paperpaw_sdk::Instant, Option<u16>)>,
}

/// The line is drawn again at most once it came this many thousandths
/// further, or this long after: each refresh holds the device (on the X4
/// Pro some 0.6 s), and the download waits meanwhile.
const SHOWN_STEP: u16 = 100;
const SHOWN_MS: u64 = 5_000;

impl Download {
    /// How much came, in thousandths, where the size is known.
    pub(super) fn permille(&self) -> Option<u16> {
        HttpProgress::new(self.received, self.total, Params::new()).permille()
    }
}

/// Parameter: the file name a download gets.
pub(super) const NAME: &str = "name";

/// Parameter: the authors of a download, as the catalog names them.
pub(super) const AUTHORS: &str = "authors";

impl BrowseScreen {
    /// Downloads entry `i`'s book, or opens the feed it leads to.
    pub(super) fn open(&mut self, i: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(entry) = self.feed.entries.get(i) else {
            return;
        };
        let readable = |ext: &str| env.formats().supports(&format!("book.{ext}"));
        if let Some((book, ext)) = entry.readable_book(readable) {
            if !env.caps().network {
                let message = env.i18n().tr("opds.no-network").to_string();
                return self.say(message, cx);
            }
            let name = book_name(entry, &book.url, ext);
            let params = Params::new()
                .with(URL, book.url.clone())
                .with(NAME, name.clone())
                .with(params::TITLE, entry.title.clone())
                .with(AUTHORS, entry.authors.clone());
            cx.command(Command::http(
                request(
                    &paperpaw_sdk::web::user_agent_of(cx.env()),
                    &self.catalog,
                    &book.url,
                    self.temp.next("tmp"),
                    &DOWNLOADED,
                    params,
                )
                .progress(&PROGRESS),
            ));
            let message = env
                .i18n()
                .format("opds.downloading", &[("title", Arg::Str(&entry.title))]);
            self.downloading = Some(Download {
                shown: None,
                title: entry.title.clone(),
                received: 0,
                total: None,
            });
            return self.say(message, cx);
        }
        if let Some(url) = entry.feed.clone() {
            return self.load(&url, false, cx);
        }
        let message = env.i18n().tr("opds.no-format").to_string();
        self.say(message, cx);
    }

    /// How far the download came: its share (or its bytes, where its
    /// size is not known) in the line below, redrawn there alone.
    pub(super) fn progressed(&mut self, progress: &HttpProgress, cx: &mut Cx<'_, '_>) {
        let Some(download) = self.downloading.as_mut() else {
            return;
        };
        download.received = progress.received;
        download.total = progress.total;
        let env = *cx.env();
        let share = download.permille();
        let due = download.shown.is_none_or(|(at, was)| {
            env.now().since(at) >= SHOWN_MS
                || matches!((share, was), (Some(now), Some(was)) if now >= was.saturating_add(SHOWN_STEP))
        });
        if !due {
            return;
        }
        download.shown = Some((env.now(), share));
        let title = Arg::Str(&download.title);
        let message = match download.permille() {
            Some(permille) => env.i18n().format(
                "opds.downloading-share",
                &[
                    ("title", title),
                    ("percent", Arg::Int(i64::from(permille / 10))),
                ],
            ),
            None => {
                let size = files::shown_size(&env, download.received);
                env.i18n().format(
                    "opds.downloading-size",
                    &[("title", title), ("size", Arg::Str(&size))],
                )
            }
        };
        self.message = Some(message);
        // A line that changes: refreshed fast, no flash of the whole
        // panel's partial (1.7 s on the X4 Pro).
        cx.request_redraw_area(self.status);
        cx.set_refresh(RefreshMode::Fast);
    }

    /// Stops the download going on: its answer is not waited for, and
    /// what came of it goes with its file.
    pub(super) fn stop_download(&mut self, cx: &mut Cx<'_, '_>) {
        self.downloading = None;
        cx.command(Command::CancelHttp(DOWNLOADED.id().clone()));
        let message = cx.env().i18n().tr("opds.download-stopped").to_string();
        self.say(message, cx);
    }

    pub(super) fn downloaded(&mut self, reply: &HttpReply, file: &str, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        // Stopped: said so already.
        if reply.error == Some(paperpaw_sdk::HttpError::Cancelled) {
            return;
        }
        self.downloading = None;
        let url = reply.context.get(URL).unwrap_or_default().to_string();
        if !self.answered(reply, &url, cx) {
            return;
        }
        // The catalog's folder, for order on the device.
        let dir = self.catalog.sync_folder(&env);
        let name = reply.context.get(NAME).unwrap_or("download").to_string();
        if !crate::temp::may_be_book(env.storage(), file, &name) {
            let message = env.i18n().tr("opds.not-a-book").to_string();
            return self.say(message, cx);
        }
        let _ = env.storage().create_dir(&dir);
        let Ok(target) = files::put_in_free_place(env.storage(), file, &dir, &name) else {
            let message = env.i18n().tr("opds.cannot-save").to_string();
            return self.say(message, cx);
        };
        cx.command(paperpaw_sdk::library::added(&target, "opds"));
        // What the catalog says of it, for books that say nothing.
        let first = |authors: &str| authors.split(" & ").next().map(String::from);
        paperpaw_sdk::library::provide(
            env.storage(),
            &target,
            paperpaw_sdk::library::Provided::default()
                .title(reply.context.get(params::TITLE).map(String::from))
                .author(reply.context.get(AUTHORS).and_then(first)),
        );
        self.message = None;
        let text = env.i18n().format(
            "opds.saved",
            &[
                ("name", Arg::Str(files::name(&target))),
                ("folder", Arg::Str(&dir)),
            ],
        );
        cx.command(
            paperpaw_sdk::ask::choice(&text, &CHOSEN)
                .about(&target)
                .options(OPTION_LABELS, &["read", "ok"])
                .command(),
        );
        self.redraw(cx);
    }
}
