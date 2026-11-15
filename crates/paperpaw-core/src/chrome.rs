//! Bars that stay along an edge of the screen across the screens shown
//! (see [`paperpaw_sdk::chrome`]): the part of the screen they leave to a
//! screen, making and letting go of them, their input.

#[allow(
    unused_imports,
    reason = "a part of the kernel, which sees all of it; not every part uses every name"
)]
use super::*;
use paperpaw_sdk::chrome::{ChromeSpec, Edge};
use paperpaw_sdk::{Gesture, StackEntry};

/// A bar a plugin added, made while it shows.
pub(crate) struct Chrome {
    pub(crate) spec: ChromeSpec,
    pub(crate) screen: Option<Box<dyn Screen>>,
    /// When it asked for [`Screen::on_timer`].
    pub(crate) timer: Option<Instant>,
}

impl fmt::Debug for Chrome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Chrome")
            .field("spec", &self.spec)
            .field("made", &self.screen.is_some())
            .finish()
    }
}

/// `point` of the screen as seen from the corner of `area`.
pub(crate) fn local(point: Point, area: Rect) -> Point {
    Point::new(point.x - i32::from(area.x), point.y - i32::from(area.y))
}

/// `gesture` as seen from the corner of `area`.
pub(crate) fn local_gesture(mut gesture: Gesture, area: Rect) -> Gesture {
    gesture.start = local(gesture.start, area);
    gesture.end = local(gesture.end, area);
    gesture
}

/// `region` of `area`'s coordinates in the screen's.
pub(crate) fn on_screen(region: Rect, area: Rect) -> Rect {
    Rect::new(
        region.x.saturating_add(area.x),
        region.y.saturating_add(area.y),
        region.width,
        region.height,
    )
}

impl Kernel {
    /// Adds a bar a plugin contributed, where the device has what it
    /// needs: along its edge, outside the bars of lower priority.
    pub(crate) fn add_chrome(&mut self, spec: ChromeSpec) {
        if spec.need.is_some_and(|need| !need.met(self.shared.caps)) {
            return;
        }
        let at = self
            .chromes
            .iter()
            .position(|c| c.spec.priority < spec.priority)
            .unwrap_or(self.chromes.len());
        let chrome = Chrome {
            spec,
            screen: None,
            timer: None,
        };
        self.chromes.insert(at, chrome);
    }

    /// The part of the screen a full screen of `entry` gets (above the
    /// row of hints where `hinted`), what the bars shown over it leave
    /// (none without an entry); their strips into `strips`, if given.
    pub(crate) fn layout(
        &self,
        hinted: bool,
        entry: Option<&StackEntry>,
        mut strips: Option<&mut Vec<(usize, Rect)>>,
    ) -> Rect {
        let panel = self.framebuffer.bounds();
        let row = if hinted {
            self.hint.height.min(panel.height / 2)
        } else {
            0
        };
        let mut area = Rect::new(0, 0, panel.width, panel.height - row);
        let Some(entry) = entry.filter(|_| !self.chromes.is_empty()) else {
            return area;
        };
        let env = self.shared.env(self.now);
        for (index, chrome) in self.chromes.iter().enumerate() {
            if !(chrome.spec.shown)(&env, entry) {
                continue;
            }
            let size = (chrome.spec.size)(&env);
            let strip = match chrome.spec.edge {
                Edge::Top => {
                    let size = size.min(area.height / 2);
                    let strip = Rect::new(area.x, area.y, area.width, size);
                    area.y += size;
                    area.height -= size;
                    strip
                }
                Edge::Bottom => {
                    let size = size.min(area.height / 2);
                    area.height -= size;
                    Rect::new(area.x, area.y + area.height, area.width, size)
                }
                Edge::Left => {
                    let size = size.min(area.width / 2);
                    let strip = Rect::new(area.x, area.y, size, area.height);
                    area.x += size;
                    area.width -= size;
                    strip
                }
                _ => {
                    let size = size.min(area.width / 2);
                    area.width -= size;
                    Rect::new(area.x + area.width, area.y, size, area.height)
                }
            };
            if let Some(strips) = strips.as_deref_mut() {
                strips.push((index, strip));
            }
        }
        area
    }

    /// The part of the screen screen `index` has (above the row of hints
    /// where it shows it): what the bars shown over the full screen at or
    /// below it leave, so that an overlay shares its full screen's bars;
    /// no bars for a screen that guards the device and those above it.
    /// One rule for the row, the bars and the screens: the row shows as
    /// the screen on top shows it ([`Kernel::hint_row`]), the bars lie
    /// above it, and the screen on top's part is above both; a full screen
    /// below an overlay keeps its part, the row and bars lying over its
    /// bottom meanwhile.
    ///
    /// A list and the detail beside it ([`Kernel::panes`]) part what the
    /// detail would have alone.
    pub(crate) fn area_of(&self, index: usize) -> Rect {
        if let Some((list, detail)) = self.panes()
            && (index == list || index == detail)
        {
            let (list_part, _, detail_part) = self.pane_parts(self.whole_area(detail));
            return if index == list {
                list_part
            } else {
                detail_part
            };
        }
        self.whole_area(index)
    }

    /// The part of the screen screen `index` has alone, as
    /// [`Kernel::area_of`] tells, no detail beside it.
    pub(crate) fn whole_area(&self, index: usize) -> Rect {
        let base = (0..=index)
            .rev()
            .find(|&i| self.overlay(i).is_none())
            .unwrap_or(0);
        let guarded = self.guard().is_some_and(|guard| index >= guard);
        let entry = self
            .shared
            .stack
            .get(base)
            .filter(|_| !guarded && !self.full_screen(base));
        let hinted = self.stack.get(index).is_some_and(|active| active.hinted);
        self.layout(hinted, entry, None)
    }

    /// The part of the screen `screen`, just made in `area` for `entry`,
    /// gets on top: an overlay its full screen's, a screen that guards the
    /// device (or one above such a screen) no bars; else `area`.
    pub(crate) fn area_on_top(
        &self,
        screen: &dyn Screen,
        hinted: bool,
        entry: &StackEntry,
        area: Rect,
    ) -> Rect {
        let below_guard = self.guard().is_some() && !self.from_guard;
        if self.chromes.is_empty() || below_guard {
            return area;
        }
        let env = self.shared.env(self.now);
        let device = env.device_within(area);
        let overlay = screen.region(&env.within(&device, area)).is_some();
        let below = self
            .stack
            .len()
            .checked_sub(1)
            .map(|top| self.full_base().min(top));
        // `screen` was just made and is not on the stack yet, so no policy was
        // set for it: what it declares is its answer (a screen on the stack is
        // read through `ActiveScreen::guards`, as `area_of` does).
        let entry = if screen.guards() || self.guard().is_some() || screen.mode().is_full_screen() {
            None
        } else if overlay && let Some(base) = below {
            self.shared.stack.get(base)
        } else {
            Some(entry)
        };
        self.layout(hinted, entry, None)
    }

    /// The bars that show now (those shown over the full screen shown;
    /// none while a screen guards the device): made where they come and
    /// shown ([`Screen::on_show`]), closed where they go
    /// ([`Screen::on_close`]), made anew where their size changed; the
    /// screens shown laid out again where what the bars leave changed.
    /// Only strips that are new or moved are drawn again: a bar whose
    /// look follows the screen shown asks for it, hearing
    /// [`events::SCREEN_SHOWN`].
    ///
    /// Bars show over a list and its detail as over the detail alone.
    pub(crate) fn sync_chromes(&mut self) {
        if self.chromes.is_empty() {
            // Only a list and its detail can change their parts.
            return self.lay_out_shown();
        }
        let mut strips: Vec<(usize, Rect)> = Vec::new();
        let base = self.full_base();
        // Above the row of hints as the screen on top shows it, as its
        // part is (see `area_of`).
        let hinted = self.stack.last().is_some_and(|top| top.hinted);
        if self.guard().is_none()
            && !self.full_screen(base)
            && let Some(entry) = self.shared.stack.get(base)
        {
            self.layout(hinted, Some(entry), Some(&mut strips));
        }
        // Gone, or made for another size: closed.
        let mut changed: Vec<Rect> = Vec::new();
        for k in 0..self.chrome_strips.len() {
            let (index, old) = self.chrome_strips[k];
            let now = strips.iter().find(|(i, _)| *i == index).map(|(_, s)| *s);
            if now == Some(old) {
                continue;
            }
            changed.push(old);
            let resized = now.is_none_or(|s| (s.width, s.height) != (old.width, old.height));
            if resized && self.chromes[index].screen.is_some() {
                self.close_chrome(index);
            }
        }
        let old = mem::replace(&mut self.chrome_strips, strips);
        for k in 0..self.chrome_strips.len() {
            let (index, strip) = self.chrome_strips[k];
            if !old.contains(&(index, strip)) {
                changed.push(strip);
            }
            if self.chromes[index].screen.is_some() {
                continue;
            }
            self.make_chrome(index, strip);
            changed.push(strip);
            self.show_chrome(index);
        }
        // Bars not shown are let go.
        for (index, chrome) in self.chromes.iter_mut().enumerate() {
            if chrome.screen.is_some() && !self.chrome_strips.iter().any(|(i, _)| *i == index) {
                chrome.screen = None;
                chrome.timer = None;
            }
        }
        for strip in changed {
            self.invalidate(strip);
        }
        self.lay_out_shown();
    }

    /// Closes bar `index` ([`Screen::on_close`]) and lets it go.
    // Its own frame, as the two below: each has a bar's environment, and
    // `sync_chromes` stays under the firmware's frame limit for host code.
    #[inline(never)]
    fn close_chrome(&mut self, index: usize) {
        self.with_chrome(index, |screen, cx| {
            screen.on_close(cx);
            Handled::Yes
        });
        self.chromes[index].screen = None;
        self.chromes[index].timer = None;
    }

    /// Makes bar `index` for its `strip`.
    #[inline(never)]
    fn make_chrome(&mut self, index: usize, strip: Rect) {
        let base = self.shared.env(self.now);
        let device = base.device_within(strip);
        let env = base.within(&device, strip);
        let chrome = &mut self.chromes[index];
        let running = self.shared.meter.enter(chrome.spec.id.namespace());
        chrome.screen = Some((chrome.spec.factory)(&env, &Params::new()));
        drop(running);
    }

    /// Shows bar `index`, just made ([`Screen::on_show`]).
    #[inline(never)]
    fn show_chrome(&mut self, index: usize) {
        self.with_chrome(index, |screen, cx| {
            screen.on_show(cx);
            Handled::Yes
        });
    }

    /// Makes the screens shown anew where the part of the screen they
    /// have changed (a bar came, went or changed its size; a detail
    /// showed beside a list, or went).
    fn lay_out_shown(&mut self) {
        let mut remade = false;
        for index in self.shown_base()..self.stack.len() {
            let area = self.area_of(index);
            if area != self.stack[index].area {
                let old = self.stack[index].area;
                self.remake_screen(index, area);
                self.invalidate(old.union(&area));
                remade = true;
            }
        }
        if remade {
            self.sync_content();
            self.output.request_refresh(RefreshMode::Partial);
        }
    }

    /// Closes every bar shown (before they are made anew for another size
    /// of the screen).
    pub(crate) fn close_chromes(&mut self) {
        for k in 0..self.chrome_strips.len() {
            let index = self.chrome_strips[k].0;
            self.with_chrome(index, |screen, cx| {
                screen.on_close(cx);
                Handled::Yes
            });
            self.chromes[index].screen = None;
            self.chromes[index].timer = None;
        }
        self.chrome_strips.clear();
    }

    /// Bars whose time came ([`Screen::on_timer`]).
    pub(crate) fn tick_chromes(&mut self) {
        for k in 0..self.chrome_strips.len() {
            let index = self.chrome_strips[k].0;
            if self.chromes[index].timer.is_some_and(|at| at <= self.now) {
                self.chromes[index].timer = None;
                self.with_chrome(index, |screen, cx| {
                    screen.on_timer(cx);
                    Handled::Yes
                });
            }
        }
    }

    /// When the next bar wants its timer.
    pub(crate) fn chromes_due(&self) -> Option<Instant> {
        self.chromes.iter().filter_map(|chrome| chrome.timer).min()
    }

    /// The bar whose strip holds `point`, and its strip.
    pub(crate) fn chrome_at(&self, point: Point) -> Option<(usize, Rect)> {
        self.chrome_strips
            .iter()
            .copied()
            .find(|(_, strip)| strip.contains(point))
    }

    /// Runs `f` with bar `index` and a context for it, in its strip's
    /// coordinates.
    pub(crate) fn with_chrome(
        &mut self,
        index: usize,
        f: impl FnOnce(&mut dyn Screen, &mut Cx<'_, '_>) -> Handled,
    ) -> Handled {
        let Some(&(_, strip)) = self.chrome_strips.iter().find(|(i, _)| *i == index) else {
            return Handled::No;
        };
        let running = self
            .shared
            .meter
            .enter(self.chromes[index].spec.id.namespace());
        let Some(screen) = self.chromes[index].screen.as_mut() else {
            return Handled::No;
        };
        let (redraw, area) = (
            mem::take(&mut self.output.redraw),
            self.output.redraw_area.take(),
        );
        let base = self.shared.env(self.now);
        let device = base.device_within(strip);
        let env = base.within(&device, strip);
        let cx_given = self.output.commands.len();
        let mut cx = Cx::new(&env, &mut self.output);
        let given = cx_given;
        let handled = f(screen.as_mut(), &mut cx);
        drop(running);
        let reserved = crate::commands::Reserved::of(&self.shared.settings, &self.shared.registry);
        let owner = self.chromes[index].spec.id.namespace();
        if crate::commands::vet_reserved(&mut self.output.commands, given, owner, &reserved) {
            self.warn_runtime(format!(
                "{owner} asked what only the settings editor may; not done"
            ));
        }
        // What it asks to draw again is in its strip; a time it asks for
        // is its own.
        if let Some(at) = self.output.wake_at.take() {
            let timer = &mut self.chromes[index].timer;
            *timer = Some(timer.map_or(at, |t| t.min(at)));
        }
        if mem::replace(&mut self.output.redraw, redraw) {
            self.invalidate(strip);
        }
        if let Some(asked) = mem::replace(&mut self.output.redraw_area, area) {
            self.invalidate(on_screen(asked, strip));
        }
        handled
    }

    /// Every bar shown hears `event` (see [`Screen::on_event`]).
    pub(crate) fn chromes_hear(&mut self, event: &Id, params: &Params) {
        for k in 0..self.chrome_strips.len() {
            let Some(&(index, _)) = self.chrome_strips.get(k) else {
                break;
            };
            self.with_chrome(index, |screen, cx| {
                screen.on_event(event, params, cx);
                Handled::Yes
            });
        }
    }
}
