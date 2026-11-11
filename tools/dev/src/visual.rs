//! Automatic checks of what is drawn, each with its id:
//!
//! - `visual.text-cut`: text whose letters are cut at the edge of the
//!   area it is drawn in or of the screen.
//! - `visual.ellipsis`: text shortened with "…" to fit its room.
//! - `visual.wrap`: a heading, a button's label or a single word broken
//!   onto two lines.
//! - `visual.outside-cell`: a widget drawing outside its cell (the grid
//!   does not clip widgets).
//! - `visual.off-screen`: rows, buttons, switches reaching past the screen.
//! - `visual.overlap`: buttons or rows over each other, text running into
//!   a switch or button, a label reaching out of its button.
//! - `visual.margin`: labels of rows and headings off the interface's
//!   margin (`UiStyle::margin`, from `core.ui-margin` and the dpi), text
//!   inside the screen's margins, controls at a row's end off the right
//!   margin, screens of a plugin starting their content differently.
//! - `visual.align`: labels of a list's rows not lined up, icons not
//!   centred on one line by their ink.
//! - `visual.ink-centre`: a button's label off its middle by its ink,
//!   across; signs (+, −, arrows) and drawn icons up and down too.
//! - `visual.touch-target`: on touch screens, buttons, rows, switches and
//!   sliders smaller than about 7 mm, the least the SDK gives a finger
//!   (`UiStyle::finger`; Apple asks 44 pt, about 7 mm, Android 48 dp,
//!   about 9 mm).
//! - `visual.focus`: on devices with buttons alone, a press of Right shows
//!   the focus: a part described as focused, drawn otherwise than before.
//! - `visual.contrast`: on black-and-white screens text in a gray that
//!   becomes white, text or controls in dithered gray only; on gray
//!   screens text below 3:1 against its background.
//!
//! Every screen of the plugins looked at is opened on its own over the
//! home screen and every widget is placed alone on the home screen in its
//! sizes; each picture is checked with what its drawing described (see
//! [`checks`]). Edges are allowed a quarter of a millimetre (at least two
//! pixels: letters' side bearings differ by about that); where the code's
//! own positions are compared (labels against the margin), two pixels.
//! Findings come with the picture, saved under `<out>/visual/<variant>/`,
//! the place marked.
//!
//! Which variants: with `--quick` one a device profile (English, light,
//! the default look) and a few sizes of each widget. Otherwise every
//! language and look asked for, as texts' lengths and corners change what
//! fits, but light only where light and dark are both asked: night mode
//! inverts the picture and nothing else, and every check measures ink
//! against its own background. Widgets are looked at in every size in
//! each profile's first variant and in a few sizes in the others.

pub mod checks;
pub mod scene;

use std::collections::BTreeMap;
use std::path::PathBuf;

use paperpaw_core::sdk::builtin::settings as core_settings;
use paperpaw_core::sdk::builtin::slots as core_slots;
use paperpaw_core::sdk::semantics::{Described, Role};
use paperpaw_platform::Rect;

use crate::cases;
use crate::cli::Context;
use crate::image::Image;
use crate::probe;
use crate::report::{Finding, Report};
use crate::session::{Session, Start};
use crate::variant::Variant;
use scene::{Issue, Panel, Scene, Ui};

/// How long notices last in the sessions (see [`start`]).
const NOTICE_SECONDS: (&str, &str) = ("core.notice-seconds", "1");

/// The buttons pressed to move the focus, the first the device has.
const NAVIGATION: &[&str] = &["right", "down", "page-forward"];

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    for (variant, all_sizes) in plan(context) {
        let mut session = start(context, &variant)?;
        let mut looker = Looker::new(&mut session);
        looker.labels_whole = context.labels_whole;
        let mut found = Found {
            context,
            report,
            variant: &variant,
        };

        let screens = cases::screens(&session, &context.target);
        // Where each plugin's screens start their content.
        let mut insets: BTreeMap<String, Vec<(String, u16, bool)>> = BTreeMap::new();
        for id in &screens {
            let Some(looked) = looker.screen(&mut session, id)? else {
                continue;
            };
            let flagged = looked.issues.iter().any(|i| i.check == checks::MARGIN);
            if let Some(inset) = looked.inset {
                insets
                    .entry(owner(context, id))
                    .or_default()
                    .push((id.clone(), inset, flagged));
            }
            found.add(id, &looked.image, looked.issues)?;
        }
        // Screens of a plugin whose content starts elsewhere than on its
        // other screens (icons' ink starts a little in: some slack).
        for screens in insets.values() {
            for (id, inset, common) in uneven(screens, looker.ui.tolerance() + 2) {
                cases::home(&mut session);
                let shown = cases::show_screen(&mut session, &id, &[])?;
                let issue = Issue::warning(
                    checks::MARGIN,
                    looker.screen,
                    format!(
                        "content here starts {inset} px from the edge of its rows, on the plugin's other screens {common} px: use the same margin on every screen"
                    ),
                );
                found.add(&id, &shown.image, vec![issue])?;
            }
        }

        let grid = cases::grid(&session);
        let sizes = if all_sizes {
            cases::sizes(grid)
        } else {
            cases::some_sizes(grid)
        };
        let widgets = cases::widgets(&session, &context.target);
        for (id, _) in &widgets {
            // What each size shows, found once for all sizes it shows in.
            let mut seen: Vec<SizeIssue> = Vec::new();
            for &size in &sizes {
                let Some(looked) = looker.widget(&mut session, id, size)? else {
                    continue;
                };
                let size = format!("{}x{}", size.0, size.1);
                for issue in looked.issues {
                    match seen
                        .iter_mut()
                        .find(|s| s.issue.check == issue.check && s.issue.message == issue.message)
                    {
                        Some(s) => s.sizes.push(size.clone()),
                        None => {
                            let picture = found.picture(&format!("{id} {size}"), &looked.image)?;
                            seen.push(SizeIssue {
                                issue,
                                sizes: vec![size.clone()],
                                picture,
                            });
                        }
                    }
                }
            }
            for s in seen {
                let place = format!("{id} {}", s.sizes[0]);
                let mut issue = s.issue;
                if s.sizes.len() > 1 {
                    issue.message = format!(
                        "{} (in {} sizes: {})",
                        issue.message,
                        s.sizes.len(),
                        s.sizes.join(", ")
                    );
                }
                found.finding(&place, s.picture, issue);
            }
        }
        eprintln!(
            "  visual: {} ({} screens, {} widgets in {} sizes)",
            variant.name(),
            screens.len(),
            widgets.len(),
            sizes.len()
        );
    }
    Ok(())
}

/// A session for the visual checks in `variant`: notices (a toast over
/// the screen) last a second, so that they are gone before a screen is
/// looked at.
pub fn start(context: &Context, variant: &Variant) -> Result<Session, String> {
    let start = Start {
        settings: vec![(NOTICE_SECONDS.0.into(), NOTICE_SECONDS.1.into())],
        ..Start::default()
    };
    Session::start(context, variant, &start)
}

/// What the checks found on a screen or a widget.
#[derive(Debug)]
pub struct Looked {
    pub image: Image,
    pub issues: Vec<Issue>,
    /// Where the content of a screen's rows starts (see
    /// [`checks::content_inset`]).
    pub inset: Option<u16>,
}

/// Looks at the screens and widgets of a session.
#[derive(Debug)]
pub struct Looker {
    pub ui: Ui,
    /// The screens' area as the kernel gives it (without the row of
    /// hints under it).
    pub screen: Rect,
    pub panel: Panel,
    /// What the home screen describes, below overlays.
    home: Vec<Described>,
    /// Whether focus is checked: devices with buttons alone.
    buttons: bool,
    /// Every shortened label reported ([`Scene::labels_whole`]).
    pub labels_whole: bool,
    /// The texts of the language looked at that hold a "…" of their own
    /// ("Loading…"): not shortened there ([`Scene::ellipses`]).
    ellipses: Vec<String>,
    /// The screen the keyboard slot opens: its keys may be as narrow as
    /// the screen lets a row of them be ([`Scene::keyboard`]).
    keyboard: Option<String>,
}

impl Looker {
    pub fn new(session: &mut Session) -> Self {
        let variant = session.variant.clone();
        let display = session.device.kernel.device().display;
        // The pseudo-language is English made long: its texts so made.
        let ellipses: Vec<String> = if variant.language == crate::lint::PSEUDO {
            crate::lint::pseudo::english(session)
                .into_values()
                .filter(|text| text.contains('…'))
                .map(|text| crate::lint::pseudo::pseudo(&text))
                .collect()
        } else {
            crate::lint::catalogs::languages(session.device.kernel.translator())
                .into_iter()
                .filter(|l| l.code == variant.language)
                .flat_map(|l| l.messages.into_values())
                .filter(|text| text.contains('…'))
                .collect()
        };
        // The keyboard, as the slot opens it.
        session.device.call(|kernel, now| {
            // A native keyboard opens for a question (as `ui.ask` makes).
            kernel
                .stage_text_question(now, "", false)
                .unwrap_or_else(|_| {
                    kernel.open_slot(now, core_slots::KEYBOARD, paperpaw_core::sdk::Params::new())
                })
        });
        session.settle();
        let keyboard = session
            .device
            .kernel
            .active_screen()
            .map(ToString::to_string);
        cases::home(session);
        Looker {
            ui: ui(session, &variant),
            screen: Rect::new(0, 0, display.width, display.height),
            panel: if display.format.gray_levels() == Some(2) {
                Panel::Mono
            } else {
                Panel::Gray
            },
            home: session.described(),
            buttons: !variant.has_touch(),
            labels_whole: false,
            ellipses,
            keyboard,
        }
    }

    /// Opens screen `id` over the home screen and checks it; `None` if it
    /// does not open by itself (it needs parameters).
    pub fn screen(&self, session: &mut Session, id: &str) -> Result<Option<Looked>, String> {
        cases::home(session);
        let shown = cases::show_screen(session, id, cases::params(session, id))?;
        if !shown.opened() {
            return Ok(None);
        }
        let own = own(&shown.described, &self.home, self.screen);
        let scene = Scene::new(&shown.image, own, self.screen, self.ui)
            .panel(self.panel)
            .labels_whole(self.labels_whole)
            .ellipses(&self.ellipses)
            .keyboard(self.keyboard.as_deref() == Some(id));
        let mut issues = checks::all(&scene);
        if self.buttons {
            issues.extend(focus(session, id, &scene, &self.home));
        }
        issues.extend(self.hints(&shown.image, &shown.described));
        let inset = checks::content_inset(&scene);
        Ok(Some(Looked {
            image: shown.image,
            issues,
            inset,
        }))
    }

    /// The row of hints below the screen (what is described below its
    /// area): its words are never shortened either.
    fn hints(&self, image: &Image, described: &[Described]) -> Vec<Issue> {
        let row: Vec<Described> = described
            .iter()
            .filter(|d| u32::from(d.clip.y) >= self.screen.bottom())
            .cloned()
            .collect();
        if row.is_empty() {
            return Vec::new();
        }
        let whole = Rect::new(0, 0, image.width as u16, image.height as u16);
        let scene = Scene::new(image, row, whole, self.ui)
            .panel(self.panel)
            .labels_whole(self.labels_whole)
            .ellipses(&self.ellipses);
        checks::ellipsis(&scene)
    }

    /// Places widget `id` alone on the home screen in `size` and checks
    /// it; `None` if the grid does not place it.
    pub fn widget(
        &self,
        session: &mut Session,
        id: &str,
        size: (u8, u8),
    ) -> Result<Option<Looked>, String> {
        let shown = cases::show_widget(session, id, size)?;
        let Some(cell) = shown.cell else {
            return Ok(None);
        };
        // The same place with the probe, which draws nothing: what the
        // home screen shows without the widget.
        let place = format!("{} 0/0 {}x{}", probe::WIDGET.as_str(), size.0, size.1);
        session.set(cases::PAGE, &place)?;
        session.device.draw_again();
        let baseline = session.image();
        let without = session.described();
        let own = widget_own(&shown.described, &without, self.screen);
        let scene = Scene::new(&shown.image, own.clone(), self.screen, self.ui)
            .panel(self.panel)
            .cell(Some(cell))
            .labels_whole(self.labels_whole)
            .ellipses(&self.ellipses);
        let mut issues = checks::all(&scene);
        issues.extend(checks::outside_cell(&scene, &baseline, &own));
        Ok(Some(Looked {
            image: shown.image,
            issues,
            inset: None,
        }))
    }
}

/// The variants looked at, each with whether widgets are looked at in
/// every size (see the module's documentation).
fn plan(context: &Context) -> Vec<(Variant, bool)> {
    let variants = context.variants();
    let kept: Vec<Variant> = if context.quick {
        variants
    } else {
        variants
            .iter()
            .filter(|v| {
                !v.night
                    || !variants.iter().any(|l| {
                        !l.night
                            && l.profile.device.id == v.profile.device.id
                            && l.language == v.language
                            && l.theme.name == v.theme.name
                    })
            })
            .cloned()
            .collect()
    };
    let mut seen: Vec<&str> = Vec::new();
    kept.into_iter()
        .map(|v| {
            let first = !seen.contains(&v.profile.device.id);
            if first {
                seen.push(v.profile.device.id);
            }
            (v, first && !context.quick)
        })
        .collect()
}

/// The interface's measures in a session, as `UiStyle::new` computes
/// them from the device and the settings.
fn ui(session: &Session, variant: &Variant) -> Ui {
    let settings = session.device.kernel.settings();
    let dpi = variant.profile.device.display.dpi;
    let mut ui = Ui {
        dpi,
        margin: 0,
        finger: 0,
        line: 1,
        gap: 0,
    };
    let margin = settings.int(core_settings::UI_MARGIN.as_str()).clamp(0, 15) as u32;
    ui.margin = if margin == 0 {
        0
    } else {
        ui.tenth_mm(margin * 10)
    };
    ui.line = ui.tenth_mm(settings.int(core_settings::UI_LINES.as_str()).clamp(1, 10) as u32);
    ui.gap = ui.tenth_mm(20);
    if variant.has_touch() {
        ui.finger = ui.tenth_mm(70);
    }
    ui
}

/// Whether two described parts are the same one drawn again.
fn same(a: &Described, b: &Described) -> bool {
    a.role == b.role && a.area == b.area
}

/// What a screen itself described: without the screen below it (an
/// overlay is drawn over the home screen, whose parts come first) and the
/// row of hints under the screen.
fn own(described: &[Described], home: &[Described], screen: Rect) -> Vec<Described> {
    let below = described
        .iter()
        .zip(home)
        .take_while(|(a, b)| same(a, b))
        .count();
    // A full screen shares nothing with the home screen but by chance.
    let below = if below * 2 >= home.len() { below } else { 0 };
    described[below..]
        .iter()
        .filter(|d| u32::from(d.clip.y) < screen.bottom())
        .cloned()
        .collect()
}

/// What a widget described: what the home screen with it described less
/// what it described without it.
fn widget_own(with: &[Described], without: &[Described], screen: Rect) -> Vec<Described> {
    let mut rest: Vec<&Described> = without.iter().collect();
    let mut own = Vec::new();
    for d in with {
        if let Some(i) = rest.iter().position(|w| same(w, d) && w.label == d.label) {
            rest.remove(i);
        } else if u32::from(d.clip.y) < screen.bottom() {
            own.push(d.clone());
        }
    }
    own
}

/// After a press of a navigation button, the focus shows (buttons-only
/// devices): a part described as focused, drawn otherwise than before.
fn focus(session: &mut Session, id: &str, scene: &Scene<'_>, home: &[Described]) -> Vec<Issue> {
    let targets = scene
        .parts(&[Role::Row, Role::Button, Role::Switch, Role::Slider])
        .count();
    // With one target there is nowhere to move the focus to.
    if targets < 2 {
        return Vec::new();
    }
    let Some(button) = NAVIGATION
        .iter()
        .find(|b| session.variant.profile.buttons.contains(b))
    else {
        return Vec::new();
    };
    let before = scene.described.iter().find(|d| d.focused).map(|d| d.area);
    if session.press(button).is_err() || session.screen().as_deref() != Some(id) {
        return Vec::new();
    }
    let image = session.image();
    let described = session.described();
    let after = own(&described, home, scene.screen);
    let focused = after.iter().rev().find(|d| d.focused);
    let changed = image.differing(scene.image, 60) > 0;
    let name = button.replace('-', " ");
    match focused {
        // Nothing to move to: the focus stays where it was.
        Some(f) if Some(f.area) == before => Vec::new(),
        Some(f) if !checks::focus_shows(scene.image, &image, f.area) => vec![Issue::warning(
            checks::FOCUS,
            f.area,
            format!(
                "the focus moved to {} with the {name} button but does not show there: draw the focused part inverted or framed",
                if f.label.is_empty() {
                    "a part".to_string()
                } else {
                    scene::quote(&f.label)
                }
            ),
        )],
        Some(_) => Vec::new(),
        None if changed => vec![Issue::warning(
            checks::FOCUS,
            scene.screen,
            format!(
                "after the {name} button nothing is described as focused: describe the focused part with `focused` (Canvas::describe), for screen readers and checks"
            ),
        )],
        None => vec![Issue::warning(
            checks::FOCUS,
            scene.screen,
            format!(
                "the {name} button shows no focus here: with buttons alone nobody can tell what Confirm acts on"
            ),
        )],
    }
}

/// The plugin an id belongs to: one looked at, else its namespace.
fn owner(context: &Context, id: &str) -> String {
    context
        .target
        .owner(id)
        .map(str::to_string)
        .unwrap_or_else(|| id.split(['.', ' ']).next().unwrap_or(id).to_string())
}

/// Screens whose inset differs by more than `tolerance` from the one most
/// screens share, unless already found off the margin: with that inset.
fn uneven(screens: &[(String, u16, bool)], tolerance: u16) -> Vec<(String, u16, u16)> {
    if screens.len() < 2 {
        return Vec::new();
    }
    let mut counts: Vec<(u16, usize)> = Vec::new();
    for (_, inset, _) in screens {
        match counts
            .iter_mut()
            .find(|c| c.0.abs_diff(*inset) <= tolerance)
        {
            Some(c) => c.1 += 1,
            None => counts.push((*inset, 1)),
        }
    }
    let Some(&(majority, n)) = counts.iter().max_by_key(|c| c.1) else {
        return Vec::new();
    };
    if n < 2 || counts.iter().filter(|c| c.1 == n).count() > 1 {
        return Vec::new();
    }
    screens
        .iter()
        .filter(|(_, inset, flagged)| !flagged && inset.abs_diff(majority) > tolerance)
        .map(|(screen, inset, _)| (screen.clone(), *inset, majority))
        .collect()
}

/// An issue a widget shows in sizes, the picture of the first.
struct SizeIssue {
    issue: Issue,
    sizes: Vec<String>,
    picture: Option<PathBuf>,
}

/// Turns issues into findings, with the picture saved once a place.
struct Found<'a> {
    context: &'a Context,
    report: &'a mut Report,
    variant: &'a Variant,
}

impl Found<'_> {
    fn add(&mut self, place: &str, image: &Image, issues: Vec<Issue>) -> Result<(), String> {
        if issues.is_empty() {
            return Ok(());
        }
        let picture = self.picture(place, image)?;
        for mut issue in issues {
            if self.context.labels_whole && issue.check == checks::ELLIPSIS {
                issue.level = crate::report::Level::Error;
            }
            self.finding(place, picture.clone(), issue);
        }
        Ok(())
    }

    /// Saves the picture of `place`; its path in the output folder.
    /// None where nothing is written ([`Context::in_memory`]).
    fn picture(&self, place: &str, image: &Image) -> Result<Option<PathBuf>, String> {
        if self.context.in_memory {
            return Ok(None);
        }
        let file: String = place
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '.' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let picture = PathBuf::from("visual")
            .join(self.variant.slug())
            .join(format!("{file}.png"));
        image.save_png(&self.context.out.join(&picture))?;
        Ok(Some(picture))
    }

    fn finding(&mut self, place: &str, picture: Option<PathBuf>, issue: Issue) {
        let mut finding = Finding::new(issue.check, issue.level, place, issue.message)
            .plugin(owner(self.context, place))
            .variant(self.variant.name())
            .area(issue.area);
        if let Some(picture) = picture {
            finding = finding.picture(picture);
        }
        self.report.add(finding);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(role: Role, area: Rect, clip: Rect) -> Described {
        Described::new(role, String::new(), area).clip(clip)
    }

    #[test]
    fn overlays_and_hints_are_left_out() {
        let screen = Rect::new(0, 0, 480, 755);
        let hints = Rect::new(0, 755, 480, 45);
        let home = vec![
            d(Role::Heading, Rect::new(0, 0, 480, 60), screen),
            d(Role::Row, Rect::new(0, 60, 480, 60), screen),
            d(Role::Text, Rect::new(10, 760, 50, 30), hints),
        ];
        let mut overlay = home[..2].to_vec();
        overlay.push(d(Role::Button, Rect::new(100, 300, 200, 60), screen));
        overlay.push(home[2].clone());
        let own = own(&overlay, &home, screen);
        assert_eq!(own.len(), 1);
        assert_eq!(own[0].role, Role::Button);
        let full = vec![d(Role::Heading, Rect::new(0, 0, 480, 70), screen)];
        assert_eq!(super::own(&full, &home, screen).len(), 1);
    }

    #[test]
    fn a_widget_is_what_differs() {
        let screen = Rect::new(0, 0, 480, 755);
        let without = vec![d(Role::Heading, Rect::new(0, 0, 480, 60), screen)];
        let mut with = without.clone();
        with.insert(0, d(Role::Button, Rect::new(10, 100, 60, 60), screen));
        assert_eq!(widget_own(&with, &without, screen).len(), 1);
    }

    #[test]
    fn the_odd_screen_out() {
        let screens = vec![
            ("a".to_string(), 26, false),
            ("b".to_string(), 27, false),
            ("c".to_string(), 40, false),
            ("d".to_string(), 60, true),
        ];
        assert_eq!(uneven(&screens, 2), [("c".to_string(), 40, 26)]);
    }
}
