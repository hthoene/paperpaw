//! The home widgets. They read what the card holds of the mail checked
//! last (never the network) when they are updated, and only draw as they
//! are drawn:
//!
//! - *Unread mail* (`mail.widget.unread`): how many mails wait, all
//!   accounts or one, the newest one below;
//! - *Latest mail* (`mail.widget.latest`): the latest mails of an
//!   account's folder, the sender, the subject and the time;
//! - *Starred mail* (`mail.widget.flagged`): the mails marked with a star;
//! - *Mailbox* (`mail.widget.folder`): one account's folder, how many are
//!   unread of how many;
//! - *Write a mail* (`mail.widget.write`): a tap opens a new mail.
//!
//! Each has the shared look (frame, spacing, text size, alignment) and
//! settings for what it shows. A tap opens the mail, or the list.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, FixedWidget, Frame, Padding, TextSize, Widget, WidgetTarget,
};
use paperpaw_sdk::{
    Arg, Canvas, Command, Env, Id, Params, Rect, RegisterError, Registrar, SettingSpec,
};

use crate::account::Account;
use crate::browser::param;
use crate::store::{self, Counts};
use crate::{LIST, OPEN, WRITE, texts};

pub const UNREAD: Id = Id::new_static("mail.widget.unread");
pub const LATEST: Id = Id::new_static("mail.widget.latest");
pub const FLAGGED: Id = Id::new_static("mail.widget.flagged");
pub const FOLDER: Id = Id::new_static("mail.widget.folder");
pub const NEW: Id = Id::new_static("mail.widget.write");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Unread,
    Latest,
    Flagged,
    Folder,
    Write,
}

impl Kind {
    fn id(self) -> &'static str {
        match self {
            Kind::Unread => "mail.widget.unread",
            Kind::Latest => "mail.widget.latest",
            Kind::Flagged => "mail.widget.flagged",
            Kind::Folder => "mail.widget.folder",
            Kind::Write => "mail.widget.write",
        }
    }
}

/// The settings every mail widget has, in its group: the look.
macro_rules! looks {
    ($id:literal, $group:expr, $align:expr) => {
        [
            widgets::frame_spec(concat!($id, ".frame"), $group, Frame::None),
            widgets::padding_spec(concat!($id, ".padding"), $group, Padding::Auto),
            widgets::paper_spec(concat!($id, ".paper"), $group),
            widgets::ink_spec(concat!($id, ".ink"), $group),
            widgets::corners_spec(concat!($id, ".corners"), $group),
            widgets::frame_line_spec(concat!($id, ".frame-line"), $group),
            widgets::shadow_spec(concat!($id, ".drop-shadow"), $group),
            widgets::size_spec(concat!($id, ".size"), $group, TextSize::Medium, true),
            widgets::align_spec(concat!($id, ".align"), $group, $align),
            widgets::part_spec(concat!($id, ".caption"), $group, "mail.part.caption", true),
        ]
    };
}

static UNREAD_LOOK: [SettingSpec; 10] = looks!("mail.widget.unread", UNREAD, Align::Left);
static LATEST_LOOK: [SettingSpec; 10] = looks!("mail.widget.latest", LATEST, Align::Left);
static FLAGGED_LOOK: [SettingSpec; 10] = looks!("mail.widget.flagged", FLAGGED, Align::Left);
static FOLDER_LOOK: [SettingSpec; 10] = looks!("mail.widget.folder", FOLDER, Align::Left);
static WRITE_LOOK: [SettingSpec; 10] = looks!("mail.widget.write", NEW, Align::Center);

/// What each shows, besides the look.
static UNREAD_OWN: [SettingSpec; 2] = [
    SettingSpec::number(
        "mail.widget.unread.account",
        "mail.widget.account",
        0,
        3,
        0,
        UNREAD,
    )
    .description("mail.widget.account.help"),
    widgets::part_spec(
        "mail.widget.unread.newest",
        UNREAD,
        "mail.part.newest",
        true,
    ),
];
static LATEST_OWN: [SettingSpec; 5] = [
    SettingSpec::number(
        "mail.widget.latest.account",
        "mail.widget.account",
        0,
        3,
        0,
        LATEST,
    )
    .description("mail.widget.account.help"),
    SettingSpec::text(
        "mail.widget.latest.folder",
        "mail.widget.folder",
        "INBOX",
        LATEST,
    )
    .description("mail.widget.folder.help"),
    SettingSpec::number(
        "mail.widget.latest.count",
        "mail.widget.count",
        1,
        8,
        3,
        LATEST,
    ),
    widgets::part_spec(
        "mail.widget.latest.subject",
        LATEST,
        "mail.part.subject",
        true,
    ),
    widgets::part_spec("mail.widget.latest.time", LATEST, "mail.part.time", true),
];
static FLAGGED_OWN: [SettingSpec; 3] = [
    SettingSpec::number(
        "mail.widget.flagged.account",
        "mail.widget.account",
        0,
        3,
        0,
        FLAGGED,
    )
    .description("mail.widget.account.help"),
    SettingSpec::number(
        "mail.widget.flagged.count",
        "mail.widget.count",
        1,
        8,
        3,
        FLAGGED,
    ),
    widgets::part_spec(
        "mail.widget.flagged.subject",
        FLAGGED,
        "mail.part.subject",
        true,
    ),
];
static FOLDER_OWN: [SettingSpec; 3] = [
    SettingSpec::number(
        "mail.widget.folder.account",
        "mail.widget.account",
        1,
        3,
        1,
        FOLDER,
    ),
    SettingSpec::text(
        "mail.widget.folder.folder",
        "mail.widget.folder",
        "INBOX",
        FOLDER,
    )
    .description("mail.widget.folder.help"),
    widgets::part_spec("mail.widget.folder.total", FOLDER, "mail.part.total", true),
];
static WRITE_OWN: [SettingSpec; 1] = [SettingSpec::number(
    "mail.widget.write.account",
    "mail.widget.account",
    0,
    3,
    0,
    NEW,
)
.description("mail.widget.write.account.help")];

pub fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    static GROUPS: [paperpaw_sdk::SettingGroupSpec; 5] = [
        paperpaw_sdk::SettingGroupSpec::new(UNREAD, "mail.widget.unread"),
        paperpaw_sdk::SettingGroupSpec::new(LATEST, "mail.widget.latest"),
        paperpaw_sdk::SettingGroupSpec::new(FLAGGED, "mail.widget.flagged"),
        paperpaw_sdk::SettingGroupSpec::new(FOLDER, "mail.widget.folder"),
        paperpaw_sdk::SettingGroupSpec::new(NEW, "mail.widget.write"),
    ];
    for group in &GROUPS {
        r.add_setting_group(group)?;
    }
    for settings in [
        &UNREAD_LOOK[..],
        &UNREAD_OWN,
        &LATEST_LOOK,
        &LATEST_OWN,
        &FLAGGED_LOOK,
        &FLAGGED_OWN,
        &FOLDER_LOOK,
        &FOLDER_OWN,
        &WRITE_LOOK,
        &WRITE_OWN,
    ] {
        r.add_settings(settings)?;
    }
    static WIDGETS: [FixedWidget; 5] = [
        FixedWidget::new(UNREAD, "mail.widget.unread", |_| {
            Box::new(MailWidget::new(Kind::Unread))
        })
        .category(widgets::categories::PERSONAL),
        FixedWidget::new(LATEST, "mail.widget.latest", |_| {
            Box::new(MailWidget::new(Kind::Latest))
        })
        .category(widgets::categories::PERSONAL),
        FixedWidget::new(FLAGGED, "mail.widget.flagged", |_| {
            Box::new(MailWidget::new(Kind::Flagged))
        })
        .category(widgets::categories::PERSONAL),
        FixedWidget::new(FOLDER, "mail.widget.folder", |_| {
            Box::new(MailWidget::new(Kind::Folder))
        })
        .category(widgets::categories::PERSONAL),
        FixedWidget::new(NEW, "mail.widget.write", |_| {
            Box::new(MailWidget::new(Kind::Write))
        })
        .category(widgets::categories::PERSONAL),
    ];
    r.add_widgets(&WIDGETS)
}

/// One line of a widget.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Item {
    pub first: String,
    pub second: String,
    pub time: String,
    /// Where a tap on it goes.
    pub open: Option<(u8, String, u32)>,
}

/// What a widget shows, read from the card.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shown {
    /// A line of large text (a count, "Write a mail").
    pub headline: String,
    pub items: Vec<Item>,
    /// The account and folder a tap opens, where it is one.
    pub folder: Option<(u8, String)>,
}

struct MailWidget {
    kind: Kind,
    shown: Shown,
    /// The newest mails it shows, kept while the card's files are as they
    /// were: every minute only the times are made anew.
    cache: Cache,
}

impl MailWidget {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            shown: Shown::default(),
            cache: Cache::default(),
        }
    }
}

/// The files the mails shown were read from, as they were (path, which write
/// of the index it was, size), and how many were kept.
type Stamp = (Vec<(String, u64, u64)>, usize);

/// The newest mails of a widget, and what they were read from.
#[derive(Default)]
struct Cache {
    stamp: Option<Stamp>,
    rows: Vec<(u8, store::Row)>,
}

/// The accounts a widget's `account` setting means: all, or the one.
fn selected(env: &Env<'_>, widget: &str) -> Vec<u8> {
    let wanted = env.settings().int(&format!("{widget}.account"));
    Account::numbers(env)
        .into_iter()
        .filter(|n| wanted == 0 || i64::from(*n) == wanted)
        .collect()
}

fn folder_of(env: &Env<'_>, widget: &str) -> String {
    let raw = env
        .settings()
        .text(&format!("{widget}.folder"))
        .trim()
        .to_string();
    if raw.is_empty() { "INBOX".into() } else { raw }
}

/// The newest `keep` mails of the selected accounts' folder that `wanted`
/// takes, newest first. Only while the index files changed the card is read
/// again, and no more than `keep` rows are held.
fn newest(
    storage: &dyn paperpaw_sdk::Storage,
    accounts: &[u8],
    folder: &str,
    keep: usize,
    wanted: fn(&store::Row) -> bool,
    cache: &mut Cache,
) -> Vec<(u8, store::Row)> {
    let files: Vec<(String, u64, u64)> = accounts
        .iter()
        .map(|&a| {
            let file = store::index_file(a, folder);
            let (serial, size) = (
                store::index_serial(storage, &file),
                storage.size(&file).unwrap_or(0),
            );
            (file, serial, size)
        })
        .collect();
    let stamp = (files, keep);
    if cache.stamp.as_ref() != Some(&stamp) {
        let mut all: Vec<(u8, store::Row)> = Vec::new();
        for &a in accounts {
            let rows = store::read_index(storage, &store::index_file(a, folder));
            all.extend(rows.into_iter().filter(&wanted).map(|r| (a, r)));
            all.sort_by(|a, b| b.1.date.cmp(&a.1.date).then(b.1.uid.cmp(&a.1.uid)));
            all.truncate(keep);
        }
        cache.rows = all;
        cache.stamp = Some(stamp);
    }
    cache.rows.clone()
}

fn item(env: &Env<'_>, account: u8, folder: &str, row: &store::Row) -> Item {
    Item {
        first: row.from.clone(),
        second: if row.subject.is_empty() {
            env.i18n().tr("mail.no-subject").into()
        } else {
            row.subject.clone()
        },
        time: texts::when(env, row.date),
        open: Some((account, folder.to_string(), row.uid)),
    }
}

/// *Mailbox*: one account's folder, how many are unread of how many.
fn mailbox(env: &Env<'_>, id: &str, accounts: &[u8]) -> Shown {
    let i18n = env.i18n();
    let folder = folder_of(env, id);
    let Some(account) = accounts.first() else {
        return Shown {
            headline: i18n.tr("mail.widget.set-up").into(),
            ..Shown::default()
        };
    };
    let counts = Counts::read(env.storage(), &store::status_file(*account, &folder));
    let headline = counts.map_or(String::new(), |c| {
        let n = i64::from(c.unseen);
        i18n.plural("mail.widget.unread.count", n, &[("count", Arg::Int(n))])
    });
    let second = counts
        .filter(|_| widgets::shows(env, id, "total"))
        .map_or(String::new(), |c| {
            i18n.plural(
                "mail.widget.total",
                i64::from(c.messages),
                &[("count", Arg::Int(i64::from(c.messages)))],
            )
        });
    let items = if second.is_empty() {
        Vec::new()
    } else {
        alloc::vec![Item {
            first: second,
            ..Item::default()
        }]
    };
    Shown {
        headline,
        items,
        folder: Some((*account, folder)),
    }
}

fn read(kind: Kind, env: &Env<'_>, cache: &mut Cache) -> Shown {
    let id = kind.id();
    let i18n = env.i18n();
    let accounts = selected(env, id);
    if matches!(kind, Kind::Write) {
        return Shown {
            headline: i18n.tr("mail.widget.write.text").into(),
            ..Shown::default()
        };
    }
    if Account::numbers(env).is_empty() {
        return Shown {
            headline: i18n.tr("mail.widget.set-up").into(),
            ..Shown::default()
        };
    }
    let count = env.settings().int(&format!("{id}.count")).clamp(1, 8) as usize;
    match kind {
        Kind::Unread => {
            let unseen: i64 = accounts
                .iter()
                .filter_map(|&a| Counts::read(env.storage(), &store::status_file(a, "INBOX")))
                .map(|c| i64::from(c.unseen))
                .sum();
            let mut items = Vec::new();
            if widgets::shows(env, id, "newest")
                && let Some((n, row)) =
                    newest(env.storage(), &accounts, "INBOX", 1, |r| !r.seen(), cache)
                        .into_iter()
                        .next()
            {
                items.push(item(env, n, "INBOX", &row));
            }
            Shown {
                headline: i18n.plural(
                    "mail.widget.unread.count",
                    unseen,
                    &[("count", Arg::Int(unseen))],
                ),
                items,
                folder: accounts
                    .first()
                    .map(|a| (*a, "INBOX".into()))
                    .filter(|_| accounts.len() == 1),
            }
        }
        Kind::Latest => {
            let folder = folder_of(env, id);
            let items = newest(env.storage(), &accounts, &folder, count, |_| true, cache)
                .iter()
                .map(|(n, r)| item(env, *n, &folder, r))
                .collect();
            Shown {
                items,
                folder: accounts
                    .first()
                    .map(|a| (*a, folder))
                    .filter(|_| accounts.len() == 1),
                ..Shown::default()
            }
        }
        Kind::Flagged => {
            let items = newest(
                env.storage(),
                &accounts,
                "INBOX",
                count,
                store::Row::flagged,
                cache,
            )
            .iter()
            .map(|(n, r)| item(env, *n, "INBOX", r))
            .collect();
            Shown {
                items,
                ..Shown::default()
            }
        }
        Kind::Folder => mailbox(env, id, &accounts),
        Kind::Write => Shown::default(),
    }
}

impl MailWidget {
    fn lines_per_item(&self, env: &Env<'_>) -> u16 {
        let id = self.kind.id();
        let subject =
            matches!(self.kind, Kind::Latest | Kind::Flagged) && widgets::shows(env, id, "subject");
        1 + u16::from(subject)
    }

    fn rows_wanted(&self, env: &Env<'_>) -> u16 {
        let style = UiStyle::new(env);
        let id = self.kind.id();
        let text = widgets::line_height(env, widgets::size(env, id).text(&style));
        let small = widgets::line_height(env, style.small());
        let headline = if self.shown.headline.is_empty() {
            0
        } else {
            text
        };
        let items = self.shown.items.len() as u16;
        let per = text
            + if self.lines_per_item(env) > 1 || matches!(self.kind, Kind::Unread) {
                small
            } else {
                0
            };
        headline + items * per
    }
}

impl Widget for MailWidget {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let shown = read(self.kind, env, &mut self.cache);
        let changed = shown != self.shown;
        self.shown = shown;
        changed
    }

    fn live(&self) -> bool {
        // The time of a mail ("14:05") moves with the clock.
        matches!(self.kind, Kind::Latest | Kind::Flagged | Kind::Unread)
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let id = self.kind.id();
        let caption = if widgets::shows(env, id, "caption") && self.kind != Kind::Write {
            widgets::caption_height(env)
        } else {
            0
        };
        let empty = if self.shown.headline.is_empty() && self.shown.items.is_empty() {
            widgets::line_height(env, UiStyle::new(env).text())
        } else {
            0
        };
        widgets::frame_height(env, id) + caption + self.rows_wanted(env).max(empty)
    }

    fn min_height(&self, env: &Env<'_>, _: u16) -> u16 {
        let id = self.kind.id();
        let caption = if widgets::shows(env, id, "caption") && self.kind != Kind::Write {
            widgets::caption_height(env)
        } else {
            0
        };
        widgets::frame_height(env, id)
            + caption
            + widgets::line_height(env, UiStyle::new(env).text())
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let id = self.kind.id();
        let style = UiStyle::new(env);
        let look = widgets::draw_frame(canvas, env, id, area);
        let mut inner = look.area;
        if widgets::shows(env, id, "caption") && self.kind != Kind::Write {
            let caption = env.i18n().tr(id);
            inner = widgets::draw_caption(canvas, env, id, &look, caption);
        }
        let Some(font) = style.font(env) else {
            return;
        };
        let size = widgets::size(env, id).text(&style);
        let (text_h, small_h) = (
            widgets::line_height(env, size),
            widgets::line_height(env, style.small()),
        );
        let width = f32::from(inner.width);
        let mut ui = ui::Ui::new(canvas, env, &style);
        let mut y = inner.y;
        let bottom = i32::from(inner.y) + i32::from(inner.height);
        let put = |ui: &mut ui::Ui<'_>, text: &str, px: f32, h: u16, y: &mut u16| {
            if i32::from(*y) + i32::from(h) > bottom {
                return false;
            }
            let text = ui::fit(&font, px, text, width);
            let x = widgets::align(env, id).x(inner, font.measure(px, &text));
            ui::Label::new(&text).px(px).color(look.ink).draw_from(
                ui,
                x,
                Rect::new(inner.x, *y, inner.width, h),
            );
            *y += h;
            true
        };
        if self.shown.headline.is_empty() && self.shown.items.is_empty() {
            put(
                &mut ui,
                env.i18n().tr("mail.widget.nothing"),
                style.text(),
                widgets::line_height(env, style.text()),
                &mut y,
            );
            return;
        }
        if !self.shown.headline.is_empty() {
            put(&mut ui, &self.shown.headline, size, text_h, &mut y);
        }
        let with_second = self.lines_per_item(env) > 1 || matches!(self.kind, Kind::Unread);
        let with_time = widgets::shows(env, id, "time");
        for item in &self.shown.items {
            let first = if with_time && !item.time.is_empty() {
                format!("{}  {}", item.first, item.time)
            } else {
                item.first.clone()
            };
            let (first_px, first_h) = if self.shown.headline.is_empty() {
                (size, text_h)
            } else {
                (style.small(), small_h)
            };
            if !put(&mut ui, &first, first_px, first_h, &mut y) {
                break;
            }
            if with_second
                && !item.second.is_empty()
                && !put(&mut ui, &item.second, style.small(), small_h, &mut y)
            {
                break;
            }
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let command = match (self.kind, &self.shown.folder) {
            (Kind::Write, _) => Command::run(WRITE),
            (_, Some((account, folder))) => Command::OpenScreen(
                LIST,
                Params::new()
                    .with(param::ACCOUNT, format!("{account}"))
                    .with(param::FOLDER, folder.clone()),
            ),
            _ => Command::run(OPEN),
        };
        alloc::vec![WidgetTarget::new(area, command)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_named_after_their_widget() {
        let all: [(&[SettingSpec], &str); 10] = [
            (&UNREAD_LOOK, "mail.widget.unread"),
            (&UNREAD_OWN, "mail.widget.unread"),
            (&LATEST_LOOK, "mail.widget.latest"),
            (&LATEST_OWN, "mail.widget.latest"),
            (&FLAGGED_LOOK, "mail.widget.flagged"),
            (&FLAGGED_OWN, "mail.widget.flagged"),
            (&FOLDER_LOOK, "mail.widget.folder"),
            (&FOLDER_OWN, "mail.widget.folder"),
            (&WRITE_LOOK, "mail.widget.write"),
            (&WRITE_OWN, "mail.widget.write"),
        ];
        for (settings, widget) in all {
            for spec in settings {
                assert!(
                    spec.id.as_str().starts_with(&format!("{widget}.")),
                    "{}",
                    spec.id
                );
                assert_eq!(spec.group.as_str(), widget);
            }
        }
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use paperpaw_sdk::{MemoryStorage, Storage};

    fn row(uid: u32, date: i64, flags: &str) -> store::Row {
        store::Row {
            uid,
            flags: flags.into(),
            date,
            size: 1,
            from: "Ada".into(),
            from_address: "ada@example.org".into(),
            subject: format!("Subject {uid}"),
            message_id: String::new(),
        }
    }

    #[test]
    fn the_newest_few_are_kept_and_the_card_is_read_again_only_when_it_changed() {
        let storage = MemoryStorage::new();
        let many: Vec<store::Row> = (1..=50).map(|n| row(n, i64::from(n), "")).collect();
        store::write_index(&storage, &store::index_file(1, "INBOX"), &many).unwrap();
        let mut cache = Cache::default();
        let shown = newest(&storage, &[1], "INBOX", 3, |_| true, &mut cache);
        assert_eq!(
            shown.iter().map(|(_, r)| r.uid).collect::<Vec<_>>(),
            [50, 49, 48]
        );
        // Held: the three, not the fifty.
        assert_eq!(cache.rows.len(), 3);
        // Unchanged files: what was read is what is shown.
        let again = newest(&storage, &[1], "INBOX", 3, |_| true, &mut cache);
        assert_eq!(again, shown);
        // A changed index (another size): read anew.
        let fewer: Vec<store::Row> = (1..=5).map(|n| row(n, i64::from(n), "F")).collect();
        store::write_index(&storage, &store::index_file(1, "INBOX"), &fewer).unwrap();
        let flagged = newest(&storage, &[1], "INBOX", 3, store::Row::flagged, &mut cache);
        assert_eq!(
            flagged.iter().map(|(_, r)| r.uid).collect::<Vec<_>>(),
            [5, 4, 3]
        );
        // Written again at the same size (and time, as without a clock):
        // the card is read again all the same.
        let other: Vec<store::Row> = (1..=5).map(|n| row(n + 10, i64::from(n), "F")).collect();
        store::write_index(&storage, &store::index_file(1, "INBOX"), &other).unwrap();
        let again = newest(&storage, &[1], "INBOX", 3, store::Row::flagged, &mut cache);
        assert_eq!(
            again.iter().map(|(_, r)| r.uid).collect::<Vec<_>>(),
            [15, 14, 13]
        );
        let _ = storage.remove(&store::index_file(1, "INBOX"));
    }
}
