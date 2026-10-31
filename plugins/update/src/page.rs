//! The update page: under the header, what there is in large type with a
//! few words below it, and up to two buttons at the bottom (the first one
//! always the way on). While installing, a bar shows how far it is.
//!
//! An update on the card comes first; without one, the page offers to
//! search online, and shows an update the server named (see [`online`]).
//! A file on the card no newer than the version running says so, is
//! installed only as going back (or installing again), and gives way to
//! a newer update the server named. An update that leaves out plugins
//! built in now says which; one not signed by PaperPaw shows its key,
//! which the reader accepts at each install. An update the server named
//! that waits for plugins (see module `waiting`) says which,
//! and offers to keep waiting, to update anyway (the offer then, which
//! says what stops working) or to ask again; the page shown counts as the
//! reader having looked.
//!
//! Another firmware's image on the card (CrossPoint's, to go back) shows
//! where no PaperPaw update is ready: it is not PaperPaw and not signed,
//! which the page says; *Not now* comes first, and installing it is the
//! reader's choice alone.
//!
//! Buttons: up and down (or left and right) move between the buttons,
//! *Select* chooses, *Back* closes (not while installing). Touch: tap.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cmp::Ordering;

use paperpaw_sdk::builtin::{actions, events};
use paperpaw_sdk::firmware::{
    FirmwareCommand, FirmwareState, ForeignImage, InstallFailure, Signer, Update, compare_versions,
    fingerprint,
};
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect,
    RefreshMode, Screen, files,
};

use crate::online::{self, Offer};
use crate::{INSTALLING, found, write_pair};

/// Wi-Fi transfer, offered to put an update on the card.
const TRANSFER: Id = Id::new_static("transfer.open");
/// How long "Restarting" shows before the restart.
const RESTART_MS: u64 = 1500;
/// Lines of the explanation at most.
const HELP_LINES: usize = 8;

/// What the page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum View {
    /// No update file on the card.
    Nothing,
    /// Files, but none for this device.
    Unfit(InstallFailure),
    /// An update to install, the plugin crates it leaves out, and how its
    /// version compares with the one running (older: going back).
    Ready {
        update: Update,
        signer: Signer,
        leaves: Vec<String>,
        age: Ordering,
    },
    /// Another firmware's image on the card, to install in PaperPaw's
    /// place.
    Foreign(ForeignImage),
    /// Asking the update server.
    Checking,
    /// The server knows no newer update.
    UpToDate,
    /// The server could not be asked (no answer, for this reason), or it
    /// answered with this error status.
    Unreachable {
        status: u16,
        reason: String,
    },
    /// An update the server named, the plugin crates it leaves out, and
    /// the names of the code plugins that stop working with it (chosen
    /// anyway while it waits).
    Offer {
        offer: Offer,
        leaves: Vec<String>,
        stopping: String,
        /// Chosen anyway while it waits (and it still does).
        anyway: bool,
    },
    /// An update the server named that waits for plugins: their names,
    /// and those of the crates that cannot come along in any case.
    Waiting {
        offer: Offer,
        names: String,
        unknown: String,
    },
    /// Downloading the update of this version.
    Downloading(String),
    Installing {
        done: u64,
        total: u64,
    },
    Installed,
    Failed(InstallFailure),
}

/// What a button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Install,
    InstallForeign,
    Check,
    Download,
    Transfer,
    Close,
}

pub(crate) struct UpdatePage {
    style: UiStyle,
    header: ui::Header,
    view: View,
    /// The button chosen with the buttons.
    focus: usize,
    /// Whether the focus shows (once a button was used; at once without
    /// touch).
    shown: bool,
    /// An update that waits chosen anyway: offered as one that does not.
    anyway: bool,
    /// The help text scrolled, where it does not fit whole even smaller
    /// (the largest text), and where it is.
    help: Option<(ui::TextView, Rect)>,
}

/// The text for why an update cannot be installed.
fn reason(failure: InstallFailure) -> &'static str {
    match failure {
        InstallFailure::Unreadable => "update.reason.unreadable",
        InstallFailure::NotAnUpdate => "update.reason.not-an-update",
        InstallFailure::NewerFormat => "update.reason.newer-format",
        InstallFailure::OtherDevice => "update.reason.other-device",
        InstallFailure::TooLarge => "update.reason.too-large",
        InstallFailure::Untrusted => "update.reason.untrusted",
        InstallFailure::NotNewer => "update.reason.not-newer",
        InstallFailure::Damaged => "update.reason.damaged",
        InstallFailure::Write => "update.reason.write",
        InstallFailure::LowBattery => "update.reason.low-battery",
        _ => "update.reason.other",
    }
}

impl UpdatePage {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let mut page = Self {
            header: ui::Header::new(env, &style, true),
            style,
            view: View::Nothing,
            focus: 0,
            shown: ui::focus_shown(env),
            anyway: false,
            help: None,
        };
        page.view = page.look_noted(env);
        page
    }

    /// What there is now ([`Self::look`]); an update that waits for
    /// plugins, shown, counts as seen.
    fn look_noted(&self, env: &Env<'_>) -> View {
        let view = self.look(env);
        if matches!(view, View::Waiting { .. }) {
            crate::waiting::seen(env.storage());
        }
        view
    }

    /// The names of the code plugins (ids) `ids`.
    fn code_names<'a>(env: &Env<'_>, ids: impl Iterator<Item = &'a str>) -> Vec<String> {
        ids.map(|id| {
            env.plugins()
                .iter()
                .find(|p| p.manifest.id.as_str() == id)
                .map_or_else(|| String::from(id), |p| String::from(p.name(env.i18n())))
        })
        .collect()
    }

    /// The names of what `holding` holds the update for: its code plugins,
    /// then its crates.
    fn holding_names(env: &Env<'_>, holding: &crate::waiting::Holding) -> String {
        let mut names = Self::code_names(env, holding.plugins.iter().map(|(id, _)| id.as_str()));
        names.extend(
            holding
                .crates
                .iter()
                .map(|name| Self::crate_name(env, name)),
        );
        names.join(", ")
    }

    /// The names of the crates built in now that `offer` lacks and that
    /// are not in the plugin library (they never come).
    fn unknown_names(
        env: &Env<'_>,
        offer: &Offer,
        firmware: &paperpaw_sdk::firmware::Firmware,
    ) -> String {
        firmware
            .plugins
            .missing_from(&offer.plugins)
            .filter(|name| offer.why_missing(name) == Some("unknown"))
            .map(|name| Self::crate_name(env, name))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// A plugin crate's name: first-party crates are named after their
    /// plugin.
    fn crate_name(env: &Env<'_>, name: &str) -> String {
        let key = name
            .strip_prefix("paperpaw-plugin-")
            .map(|id| alloc::format!("{id}.plugin.name"));
        String::from(
            key.as_deref()
                .and_then(|key| env.i18n().get(key))
                .unwrap_or(name),
        )
    }

    /// What there is now: an install going on, or the files on the card.
    fn look(&self, env: &Env<'_>) -> View {
        let Some(firmware) = env.firmware() else {
            return View::Nothing;
        };
        match &firmware.state {
            FirmwareState::Installing { done, total } => {
                return View::Installing {
                    done: *done,
                    total: *total,
                };
            }
            FirmwareState::Installed => return View::Installed,
            FirmwareState::Idle | FirmwareState::Failed(_) => {}
            _ => {}
        }
        // Downloaded, its code plugins still coming.
        if let Some(version) = crate::fetch::fetching(env.storage()) {
            return View::Downloading(version);
        }
        let card = found::find(env.storage(), firmware);
        let offer = online::offer(env.storage(), firmware);
        let age = |update: &Update| compare_versions(&update.info.version, &firmware.version);
        // Another firmware the reader put on the card, where no update of
        // PaperPaw is ready.
        let ready = matches!(&card, Ok(update) if offer.is_none() || age(update).is_gt());
        if !ready && let Some(image) = found::foreign(env.storage(), firmware) {
            return View::Foreign(image);
        }
        match (card, offer) {
            // A newer update online comes before going back or installing
            // the same again.
            (Ok(update), offer) if offer.is_none() || age(&update).is_gt() => {
                let signer = update.signer(firmware);
                let leaves = update
                    .leaves_out(firmware)
                    .into_iter()
                    .filter(|c| !crate::fetch::comes_back(env.storage(), &[], c))
                    .map(String::from)
                    .collect();
                View::Ready {
                    age: age(&update),
                    update,
                    signer,
                    leaves,
                }
            }
            (_, Some(offer)) => {
                let holding = offer.holding(env, firmware);
                if !holding.is_empty() && !self.anyway {
                    return View::Waiting {
                        names: Self::holding_names(env, &holding),
                        unknown: Self::unknown_names(env, &offer, firmware),
                        offer,
                    };
                }
                let wanted = crate::fetch::wanted(env, &offer.builds);
                let leaves = offer
                    .leaves_out(env.storage(), firmware, &wanted)
                    .into_iter()
                    .map(String::from)
                    .collect();
                let stopping =
                    Self::code_names(env, holding.plugins.iter().map(|(id, _)| id.as_str()))
                        .join(", ");
                View::Offer {
                    offer,
                    leaves,
                    stopping,
                    // Waiting no more, it is offered as any update.
                    anyway: self.anyway && !holding.is_empty(),
                }
            }
            (Err(InstallFailure::Unreadable) | Ok(_), None) => View::Nothing,
            (Err(why), None) => View::Unfit(why),
        }
    }

    /// Whether the page waits for an answer or a download (kept when it
    /// shows again).
    fn waiting(&self) -> bool {
        matches!(self.view, View::Checking | View::Downloading(_))
    }

    /// The plugins `crates` leave out, by their names, and why the server
    /// says they are missing.
    fn plugin_names(&self, env: &Env<'_>, crates: &[String]) -> String {
        // Why each is missing; not after choosing anyway, where they are
        // all gone alike (said once for all of them).
        let offer = match &self.view {
            View::Offer {
                offer,
                anyway: false,
                ..
            } => Some(offer),
            _ => None,
        };
        let names: Vec<String> = crates
            .iter()
            .map(|name| {
                let shown = Self::crate_name(env, name);
                match offer.and_then(|o| o.why_missing(name)) {
                    Some(why @ ("unknown" | "not-ready")) => alloc::format!(
                        "{shown} ({})",
                        env.i18n().tr(&alloc::format!("update.missing.{why}"))
                    ),
                    _ => shown,
                }
            })
            .collect();
        names.join(", ")
    }

    /// `help`, followed by what cannot come along if anything.
    fn with_leaves(&self, env: &Env<'_>, help: String, leaves: &[String]) -> String {
        if leaves.is_empty() {
            return help;
        }
        let names = self.plugin_names(env, leaves);
        // Before a download, the choice is to download without them;
        // chosen anyway while it waits, they are gone after it.
        let key = match self.view {
            View::Offer { anyway: true, .. } => "update.leaves.anyway",
            View::Offer { .. } => "update.leaves.download",
            _ => "update.leaves",
        };
        alloc::format!(
            "{help} {}",
            env.i18n().format(key, &[("plugins", Arg::Str(&names))])
        )
    }

    fn buttons(&self, env: &Env<'_>) -> Vec<Button> {
        match &self.view {
            View::Nothing => {
                let mut buttons = Vec::new();
                if env.caps().network {
                    buttons.push(Button::Check);
                }
                if env.actions().get(TRANSFER.as_str()).is_some() {
                    buttons.push(Button::Transfer);
                }
                buttons
            }
            // Before going back, the way to search for a newer one; not
            // beside one's own build, whose key needs the room to be said.
            View::Ready { age, signer, .. }
                if !age.is_gt() && *signer == Signer::PaperPaw && env.caps().network =>
            {
                alloc::vec![Button::Install, Button::Check, Button::Close]
            }
            View::Ready { .. } => alloc::vec![Button::Install, Button::Close],
            // Not installing is the way on; searching online stays.
            View::Foreign(_) => {
                let mut buttons = alloc::vec![Button::Close, Button::InstallForeign];
                if env.caps().network {
                    buttons.push(Button::Check);
                }
                buttons
            }
            // Chosen anyway while it waits: not downloading is the safe
            // way on, which a second press does not pass by.
            View::Offer { anyway: true, .. } => alloc::vec![Button::Close, Button::Download],
            View::Offer { .. } => alloc::vec![Button::Download, Button::Close],
            // Waiting is the way on; updating anyway, or asking again, the
            // others.
            View::Waiting { .. } => alloc::vec![Button::Close, Button::Download, Button::Check],
            View::Unreachable { .. } => alloc::vec![Button::Check, Button::Close],
            View::Unfit(_) | View::Failed(_) | View::UpToDate => alloc::vec![Button::Close],
            View::Installing { .. } | View::Installed | View::Checking | View::Downloading(_) => {
                Vec::new()
            }
        }
    }

    fn label<'a>(&self, env: &'a Env<'_>, button: Button) -> &'a str {
        env.i18n().tr(match (button, &self.view) {
            (
                Button::Install,
                View::Ready {
                    signer: Signer::Unknown,
                    ..
                },
            ) => "update.install-with-key",
            (
                Button::Install,
                View::Ready {
                    age: Ordering::Less,
                    ..
                },
            ) => "update.install-older",
            (
                Button::Install,
                View::Ready {
                    age: Ordering::Equal,
                    ..
                },
            ) => "update.reinstall",
            (Button::Install, View::Ready { leaves, .. }) if !leaves.is_empty() => {
                "update.install-without"
            }
            (Button::Install, _) => "update.install",
            (Button::InstallForeign, _) => "update.install-foreign",
            // Chosen anyway: right whether plugins stay off or are gone.
            (Button::Download, View::Offer { anyway: true, .. }) => "update.download-anyway",
            (Button::Download, View::Offer { leaves, .. }) if !leaves.is_empty() => {
                "update.download-without"
            }
            (Button::Download, View::Waiting { .. }) => "update.anyway",
            (Button::Download, _) => "update.download",
            (Button::Check, View::Unreachable { .. }) => "update.retry",
            (Button::Check, View::Waiting { .. }) => "update.check-again",
            (Button::Check, _) => "update.check",
            (Button::Transfer, _) => "update.transfer",
            (Button::Close, View::Waiting { .. }) => "update.keep-waiting",
            (Button::Close, View::Ready { .. } | View::Offer { .. } | View::Foreign(_)) => {
                "update.later"
            }
            (Button::Close, _) => "update.ok",
        })
    }

    /// The large line and the words below it.
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn texts(&self, env: &Env<'_>) -> (String, String) {
        let current = &crate::shown_version(
            env.i18n(),
            env.firmware().map_or("", |f| f.version.as_str()),
        );
        let tr = |key: &str| env.i18n().tr(key).to_string();
        let with = |key: &str, args: &[(&str, Arg<'_>)]| env.i18n().format(key, args);
        match &self.view {
            View::Nothing => (
                tr("update.nothing.title"),
                with("update.nothing.help", &[("current", Arg::Str(current))]),
            ),
            View::Unfit(why) => (tr("update.unfit.title"), tr(reason(*why))),
            View::Ready {
                update,
                signer,
                leaves,
                age,
            } => {
                let version = Arg::Str(&update.info.version);
                let current = &[("current", Arg::Str(current))];
                let (title, mut help) = if *signer == Signer::Unknown {
                    // Its groups kept together on one line.
                    let key = fingerprint(&update.trailer.key).replace(' ', "\u{a0}");
                    (
                        with("update.unknown.title", &[("version", version)]),
                        with("update.unknown.help", &[("key", Arg::Str(&key))]),
                    )
                } else {
                    let (title, help) = match age {
                        Ordering::Greater => ("update.ready.title", "update.ready.help"),
                        Ordering::Equal => ("update.same.title", "update.same.help"),
                        Ordering::Less => ("update.older.title", "update.older.help"),
                    };
                    (with(title, &[("version", version)]), with(help, current))
                };
                if *signer == Signer::Unknown && !age.is_gt() {
                    help.push(' ');
                    help.push_str(&with("update.older.note", current));
                }
                (title, self.with_leaves(env, help, leaves))
            }
            View::Foreign(image) => {
                let name = alloc::format!("{} {}", image.info.project, image.info.version);
                let name = match name.trim() {
                    "" => image.path.as_str(),
                    name => name,
                };
                (
                    with("update.foreign.title", &[("name", Arg::Str(name))]),
                    with("update.foreign.help", &[("file", Arg::Str(&image.path))]),
                )
            }
            View::Checking => (
                tr("update.checking.title"),
                with(
                    "update.checking.help",
                    &[("server", Arg::Str(&online::server_name(env)))],
                ),
            ),
            View::UpToDate => (
                tr("update.up-to-date.title"),
                with("update.up-to-date.help", &[("current", Arg::Str(current))]),
            ),
            View::Unreachable { status, reason } => {
                let server = online::server_name(env);
                let status = alloc::format!("{status}");
                (
                    tr("update.unreachable.title"),
                    if reason.is_empty() {
                        with(
                            "update.server-error.help",
                            &[("server", Arg::Str(&server)), ("status", Arg::Str(&status))],
                        )
                    } else {
                        with(
                            "update.unreachable.help",
                            &[("server", Arg::Str(&server)), ("reason", Arg::Str(reason))],
                        )
                    },
                )
            }
            View::Offer {
                offer,
                leaves,
                stopping,
                anyway,
            } => {
                let size = files::size(offer.size, env.i18n().tr("update.decimal"));
                let help = with(
                    if offer.size > 0 {
                        "update.offer.help"
                    } else {
                        "update.offer.help-unsized"
                    },
                    &[("current", Arg::Str(current)), ("size", Arg::Str(&size))],
                );
                // Chosen anyway, what it costs is what counts: what stops
                // working first (cut short, the end goes), then what is
                // gone; not the size, which leaves no room for them.
                let mut help = if *anyway { String::new() } else { help };
                if !stopping.is_empty() {
                    help.push(' ');
                    help.push_str(&with("update.stopping", &[("plugins", Arg::Str(stopping))]));
                }
                let help = String::from(self.with_leaves(env, help, leaves).trim_start());
                (
                    with(
                        "update.offer.title",
                        &[(
                            "version",
                            Arg::Str(&crate::shown_version(env.i18n(), &offer.version)),
                        )],
                    ),
                    help,
                )
            }
            View::Waiting {
                offer,
                names,
                unknown,
            } => {
                let mut help = with("update.waiting.help", &[("plugins", Arg::Str(names))]);
                if !unknown.is_empty() {
                    help.push(' ');
                    help.push_str(&with(
                        "update.waiting.unknown",
                        &[("plugins", Arg::Str(unknown))],
                    ));
                }
                (
                    with(
                        "update.waiting.title",
                        &[(
                            "version",
                            Arg::Str(&crate::shown_version(env.i18n(), &offer.version)),
                        )],
                    ),
                    help,
                )
            }
            View::Downloading(version) => (
                with(
                    "update.downloading.title",
                    &[(
                        "version",
                        Arg::Str(&crate::shown_version(env.i18n(), version)),
                    )],
                ),
                tr("update.downloading.help"),
            ),
            View::Installing { .. } => {
                (tr("update.installing.title"), tr("update.installing.help"))
            }
            View::Installed => (tr("update.installed.title"), tr("update.installed.help")),
            View::Failed(why) => (
                tr("update.failed.title"),
                with(
                    "update.failed.help",
                    &[
                        ("reason", Arg::Str(env.i18n().tr(reason(*why)))),
                        ("current", Arg::Str(current)),
                    ],
                ),
            ),
        }
    }

    /// The button areas, the first one above the second, at the bottom:
    /// each in a row as other screens' buttons, within the side margin,
    /// the lowest keeping it from the bottom edge too.
    fn button_rects(&self, env: &Env<'_>, buttons: &[Button]) -> Vec<Rect> {
        let style = &self.style;
        let display = env.device().display;
        let margin = style.margin();
        let width = display.width.saturating_sub(2 * margin);
        let row = style.touch_row(u32::from(style.control_row())) as u16;
        let mut bottom = display.height.saturating_sub(ui::edge_room(style, row));
        // From the bottom up, each in a touch row, higher where its word
        // takes more lines.
        let mut rects: Vec<Rect> = buttons
            .iter()
            .rev()
            .map(|button| {
                let label = self.label(env, *button);
                let slot = ui::button_slot_height(env, style, width, label);
                let top = bottom.saturating_sub(slot);
                bottom = top;
                ui::button_in_slot(style, Rect::new(margin, top, width, slot))
            })
            .collect();
        rects.reverse();
        rects
    }

    fn redraw(&mut self, cx: &mut Cx<'_, '_>, mode: RefreshMode) {
        cx.request_redraw();
        cx.request_refresh(mode);
    }

    fn choose(&mut self, button: Button, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match button {
            Button::Close => cx.command(Command::Back),
            Button::Transfer => cx.command(Command::run(TRANSFER)),
            Button::Check => {
                let mut out = paperpaw_sdk::Commands::new();
                online::check(&env, false, crate::count::Trigger::Manual, &mut out);
                out.into_iter().for_each(|command| cx.command(command));
                self.view = View::Checking;
                self.focus = 0;
                self.redraw(cx, RefreshMode::Partial);
            }
            // Chosen anyway: the offer first, which says what stops working.
            Button::Download if matches!(self.view, View::Waiting { .. }) => {
                self.anyway = true;
                self.view = self.look(&env);
                self.focus = 0;
                self.redraw(cx, RefreshMode::Partial);
            }
            Button::Download => {
                let View::Offer { offer, .. } = &self.view else {
                    return;
                };
                let mut out = paperpaw_sdk::Commands::new();
                online::download(&env, offer, &mut out);
                out.into_iter().for_each(|command| cx.command(command));
                self.view = View::Downloading(offer.version.clone());
                self.focus = 0;
                self.redraw(cx, RefreshMode::Partial);
            }
            Button::Install => {
                let View::Ready {
                    update,
                    signer,
                    age,
                    ..
                } = &self.view
                else {
                    return;
                };
                write_pair(
                    env.storage(),
                    INSTALLING,
                    &update.path,
                    &update.info.version,
                );
                // What the reader saw and chose: the key shown, a version
                // not newer said to be so.
                cx.command(Command::Firmware(FirmwareCommand::install(
                    update.path.clone(),
                    (*signer == Signer::Unknown).then_some(update.trailer.key),
                    !age.is_gt(),
                )));
                self.view = View::Installing {
                    done: 0,
                    total: u64::from(update.trailer.image_len),
                };
                self.focus = 0;
                self.redraw(cx, RefreshMode::Partial);
            }
            // What the reader chose, told it is not PaperPaw. Nothing is
            // kept to say after the restart: another firmware starts.
            Button::InstallForeign => {
                let View::Foreign(image) = &self.view else {
                    return;
                };
                cx.command(Command::Firmware(FirmwareCommand::install_foreign(
                    image.path.clone(),
                )));
                self.view = View::Installing {
                    done: 0,
                    total: image.len,
                };
                self.focus = 0;
                self.redraw(cx, RefreshMode::Partial);
            }
        }
    }

    fn move_focus(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let count = self.buttons(cx.env()).len();
        if count == 0 {
            return;
        }
        let before = self.focus;
        if !self.shown {
            self.shown = true;
        } else {
            self.focus = ui::step_focus(self.focus, delta, count);
        }
        // The buttons the focus left and came to.
        let env = *cx.env();
        let rects = self.button_rects(&env, &self.buttons(&env));
        let reach = |i: usize| rects.get(i).map(|r| ui::button_reach(&self.style, *r));
        match (reach(before), reach(self.focus)) {
            (Some(left), Some(reached)) => ui::ListChange::focus(left, reached).request(cx),
            _ => self.redraw(cx, RefreshMode::Fast),
        }
    }

    fn draw_progress(&self, ui: &mut ui::Ui<'_>, area: Rect, done: u64, total: u64) {
        let style = &self.style;
        let percent = (done * 100).checked_div(total).unwrap_or(0).min(100);
        let permille = (done * 1000).checked_div(total).unwrap_or(0).min(1000) as u16;
        ui.progress(area, permille, Color::BLACK);
        let text = alloc::format!("{percent} %");
        ui.canvas().describe(Role::Text, &text, area);
        if let Some(font) = ui.font() {
            let label = Rect::new(
                area.x,
                area.y + area.height + style.gap(),
                area.width,
                style.row(),
            );
            let tx = f32::from(area.x)
                + (f32::from(area.width) - font.measure(style.text(), &text)) / 2.0;
            ui::Label::new(&text)
                .px(style.text())
                .color(Color::BLACK)
                .draw_from(ui, tx, label);
        }
    }
}

/// Whether [`paragraph`] shows all of `text` in `area` (in `size` or
/// smaller, at most `max` lines).
fn paragraph_fits(
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    size: f32,
    text: &str,
    max: usize,
) -> bool {
    let Some(font) = style.font(env) else {
        return true;
    };
    let size = size * 0.5;
    let height = font.line_metrics(size).line_height as u16;
    let lines = font.wrap(size, text, f32::from(area.width)).len();
    lines <= max.min(usize::from(area.height / height.max(1)))
}

/// Draws `text` left-aligned in `area`, wrapped, at most `max` lines and
/// as many as `area` holds: a little smaller where it needs more, only
/// then cut, the last line ending in "…"; returns the height used.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn paragraph(
    ui: &mut ui::Ui<'_>,
    area: Rect,
    size: f32,
    text: &str,
    max: usize,
    color: Color,
) -> u16 {
    let Some(font) = ui.font() else {
        return 0;
    };
    let width = f32::from(area.width);
    let lines_at = |size: f32| {
        let height = font.line_metrics(size).line_height as u16;
        max.min(usize::from(area.height / height.max(1)))
    };
    // Smaller where it does not fit, down to half (at the largest text,
    // whose fonts come in steps, still as large as text at the default).
    let size = paperpaw_sdk::widgets::largest_size(size, size * 0.5, |size| {
        font.wrap(size, text, width).len() <= lines_at(size)
    })
    .unwrap_or(size * 0.5);
    let (height, max) = (font.line_metrics(size).line_height as u16, lines_at(size));
    let lines = font.wrap(size, text, width);
    let cut = lines.len() > max;
    let used = height * lines.len().min(max) as u16;
    ui.canvas().describe(
        Role::Text,
        text,
        Rect::new(area.x, area.y, area.width, used),
    );
    let mut top = area.y;
    for (n, range) in lines.into_iter().take(max).enumerate() {
        let row = Rect::new(area.x, top, area.width, height);
        let line = if cut && n + 1 == max {
            let rest = text[range.start..].trim_end();
            // The rest is longer than the line: shortened, it ends in "…".
            alloc::borrow::Cow::Owned(ui::fit(&font, size, rest, width))
        } else {
            alloc::borrow::Cow::Borrowed(&text[range])
        };
        ui::Label::new(&line)
            .px(size)
            .color(color)
            .draw_from(ui, f32::from(area.x), row);
        top += height;
    }
    top - area.y
}

impl Screen for UpdatePage {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        canvas.clear(Color::WHITE);
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, env.i18n().tr("update.title"), None, None);
        // Within the margins other pages keep, below the header as their
        // content.
        let margin = style.margin();
        let gap = style.gap();
        let width = env.device().display.width.saturating_sub(2 * margin);
        let (title, help) = self.texts(env);
        let mut y = self.header.area().height + gap;
        let area = Rect::new(
            margin,
            y,
            width,
            env.device().display.height.saturating_sub(y),
        );
        y += paragraph(&mut ui, area, style.title(), &title, 3, Color::BLACK) + gap;
        let buttons = self.buttons(env);
        let rects = self.button_rects(env, &buttons);
        // The words keep clear of the buttons.
        let bottom = rects
            .first()
            .map_or(env.device().display.height, |r| r.y.saturating_sub(gap));
        let area = Rect::new(margin, y, width, bottom.saturating_sub(y));
        if paragraph_fits(env, &style, area, style.small(), &help, HELP_LINES) {
            self.help = None;
            y += paragraph(
                &mut ui,
                area,
                style.small(),
                &help,
                HELP_LINES,
                style.ink(ui::Ink::NOTE),
            ) + gap * 2;
        } else {
            // Too long even smaller: scrolled with the page buttons, a
            // swipe or a tap on it.
            if self.help.as_ref().is_none_or(|(_, at)| *at != area) {
                self.help = Some((ui::TextView::new(env, &style, area, &help), area));
            }
            if let Some((view, _)) = &self.help {
                view.draw(&mut ui, &help);
            }
            y += area.height + gap * 2;
        }
        if let View::Installing { done, total } = self.view {
            let bar = Rect::new(margin, y, width, gap * 3);
            self.draw_progress(&mut ui, bar, done, total);
        }
        for (i, (button, rect)) in buttons.iter().zip(rects).enumerate() {
            let label = self.label(env, *button);
            let focused = self.shown && self.focus == i;
            ui.button(rect, label, i == 0, focused);
        }
    }

    fn hint(&self, _: &Id, _: &Env<'_>) -> paperpaw_sdk::Hint {
        // Nothing can be done while it installs.
        if matches!(self.view, View::Installing { .. } | View::Installed) {
            paperpaw_sdk::Hint::Hidden
        } else {
            paperpaw_sdk::Hint::Default
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        // Plugins for a downloaded update, cut off by a restart: on.
        let mut out = paperpaw_sdk::Commands::new();
        crate::fetch::resume(cx.env(), &mut out);
        out.into_iter().for_each(|command| cx.command(command));
        // Back from Wi-Fi transfer: a file may have come.
        let env = *cx.env();
        if !matches!(self.view, View::Installing { .. } | View::Installed) && !self.waiting() {
            self.view = self.look_noted(&env);
            self.focus = 0;
        }
        self.redraw(cx, RefreshMode::Partial);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if matches!(self.view, View::Installing { .. } | View::Installed) {
            // Held: the install must not be left half way.
            return Handled::Yes;
        }
        if *a == actions::BACK {
            cx.command(Command::Back);
        } else if [actions::PREV_PAGE, actions::NEXT_PAGE].contains(a)
            && let Some((view, _)) = &mut self.help
        {
            if view.on_action(a).changed() {
                self.redraw(cx, RefreshMode::Fast);
            }
        } else if [actions::UP, actions::LEFT, actions::PREVIOUS].contains(a) {
            self.move_focus(-1, cx);
        } else if [actions::DOWN, actions::RIGHT, actions::NEXT].contains(a) {
            self.move_focus(1, cx);
        } else if *a == actions::CONFIRM {
            let buttons = self.buttons(cx.env());
            if let Some(button) = buttons.get(self.focus).copied() {
                if self.shown {
                    self.choose(button, cx);
                } else {
                    self.shown = true;
                    self.redraw(cx, RefreshMode::Fast);
                }
            }
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if matches!(self.view, View::Installing { .. } | View::Installed) {
            return Handled::No;
        }
        // On the help text scrolled: it scrolls.
        if let Some((view, area)) = &mut self.help
            && area.contains(gesture.start)
        {
            if view.on_gesture(&gesture).changed() {
                self.redraw(cx, RefreshMode::Fast);
            }
            return Handled::Yes;
        }
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let env = *cx.env();
        if let Some(action) = self.header.tap(&env, &self.style, gesture.start, None) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        self.shown = false;
        let buttons = self.buttons(&env);
        let rects = self.button_rects(&env, &buttons);
        match buttons
            .iter()
            .zip(rects)
            .position(|(_, rect)| rect.contains(gesture.start))
        {
            Some(i) => {
                self.focus = i;
                self.choose(buttons[i], cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        // The plugin kept the answer or the file before the page hears of
        // it (see `online`).
        if let Some(reply) = online::CHECKED.read(event, params) {
            if self.view == View::Checking {
                self.view = if !(200..300).contains(&reply.status) {
                    View::Unreachable {
                        status: reply.status,
                        reason: if reply.status == 0 {
                            reply.text().into_owned()
                        } else {
                            String::new()
                        },
                    }
                } else {
                    match self.look_noted(&env) {
                        View::Ready { age, .. } if !age.is_gt() => View::UpToDate,
                        View::Nothing | View::Unfit(_) | View::Foreign(_) => View::UpToDate,
                        view => view,
                    }
                };
                self.focus = 0;
                self.redraw(cx, RefreshMode::Partial);
            }
            return;
        }
        if online::DOWNLOADED.read(event, params).is_some()
            || crate::fetch::FETCHED.read(event, params).is_some()
        {
            if matches!(self.view, View::Downloading(_)) {
                self.view = self.look(&env);
                self.focus = 0;
                self.redraw(cx, RefreshMode::Partial);
            }
            return;
        }
        if *event != events::FIRMWARE_CHANGED {
            return;
        }
        let Some(firmware) = env.firmware() else {
            return;
        };
        let (view, mode) = match &firmware.state {
            FirmwareState::Installing { done, total } => {
                // Only the bar and its number change.
                (
                    View::Installing {
                        done: *done,
                        total: *total,
                    },
                    RefreshMode::Fast,
                )
            }
            FirmwareState::Installed => {
                cx.wake_at(env.now().add_millis(RESTART_MS));
                (View::Installed, RefreshMode::Partial)
            }
            FirmwareState::Failed(why) => (View::Failed(*why), RefreshMode::Partial),
            FirmwareState::Idle => return,
            _ => return,
        };
        if view != self.view {
            self.view = view;
            self.focus = 0;
            self.redraw(cx, mode);
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        if self.view == View::Installed {
            cx.command(Command::Firmware(FirmwareCommand::Restart));
        }
    }
}
