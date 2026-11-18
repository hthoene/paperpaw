//! The status bar on the dashboard: its plate and items drawn in the strip the
//! page keeps free, a tap on an item that does something, and the bar that
//! shows after a press (the first press of a hidden bar only shows it).
use paperpaw_sdk::native::callbacks::KeyState;
use paperpaw_ui_kit::status_line::{self, Face, Lists, Placed};

use super::*;
use crate::native_bar::{self as bar, Look};
use crate::native_picture::{Visible, ink_over};
use alloc::string::String;

/// How long a bar that shows after a press stays (until the next minute tick
/// after this, `PORTING-7.md`), as the kernel's row of hints does.
pub(super) const BAR_MS: u64 = 4000;

/// The bar as laid out: its plate and the room for its items, what it is on and
/// what it shows, and when it shows.
#[derive(Debug)]
pub(super) struct Plan {
    plate: Rect,
    inner: Rect,
    look: Look,
    items: Lists,
    visible: Visible,
    /// The row of hints shows after a press.
    hints_auto: bool,
}

impl Plan {
    /// The bar of `src` in `strip`; none where the bar is off (its strip is not
    /// kept either).
    ///
    /// # Errors
    /// [`Error::NoMemory`] for the lists of items.
    pub(super) fn new(
        strip: Option<Rect>,
        src: &HostSource,
        (margin, gap): (u16, u16),
    ) -> Result<Option<Self>, Error> {
        let Some(strip) = strip else {
            return Ok(None);
        };
        let plate = bar::plate(strip, (margin, gap), src.bar_placing);
        let items = Lists {
            left: text::copy(&src.bar_items.left)?,
            center: text::copy(&src.bar_items.center)?,
            right: text::copy(&src.bar_items.right)?,
            line: String::new(),
        };
        Ok(Some(Self {
            plate,
            inner: bar::inner(plate, gap),
            look: src.bar_look,
            items,
            visible: src.bar,
            hints_auto: src.hints == Visible::Auto,
        }))
    }

    /// The press that shows the bar is taken: the bar or the hints show after
    /// one.
    pub(super) fn auto(&self) -> bool {
        self.visible == Visible::Auto || self.hints_auto
    }
}

impl Dashboard {
    /// Whether the bar is drawn now.
    pub(super) fn bar_shown(&self, plan: &Plan) -> bool {
        match plan.visible {
            Visible::Off => false,
            Visible::On => true,
            Visible::Auto => self.revealed.is_some(),
        }
    }

    /// Draws the bar into `below` (the plate and the texts) and the items its
    /// owners draw into `placed`, to be composed once the paint is replayed.
    /// A host that does not answer the status calls leaves the line empty.
    pub(super) fn bar_paint<'a>(
        &self,
        face: &Face<'a, '_>,
        ui: &Ui,
        (laid, corner): (&Laid, u32),
        out: (&mut Vec<Paint>, &mut Vec<Placed<'a>>),
        buffers: &'a mut [Vec<u8>; 2],
    ) -> Result<(), Error> {
        let (below, placed) = out;
        let Some(plan) = laid.bar.as_ref().filter(|plan| self.bar_shown(plan)) else {
            return Ok(());
        };
        let ink = match plan.look.plate() {
            Some((paper, ink)) => {
                paint::push(below, Paint::Focus(plan.plate, corner, None, paper))?;
                ink
            }
            // On the picture: the ink that reads on how bright it is behind the
            // plate (black where no picture or no brightness is known).
            None => Color::gray(ink_over(self.backdrop.brightness_behind(plan.plate))),
        };
        match status_line::draw(
            (face, &plan.items),
            ui,
            below,
            placed,
            buffers,
            (plan.inner, ink),
        ) {
            Err(Error::Unsupported) => Ok(()),
            other => other,
        }
    }

    /// A tap at `point` on an item of the bar that does something: runs it.
    /// `false` where the tap is not on such an item.
    pub(super) fn bar_tap(&self, host: &Host<'_>, ui: &Ui, point: Point) -> Result<bool, Error> {
        let Some(plan) = self
            .laid
            .as_ref()
            .and_then(|laid| laid.bar.as_ref())
            .filter(|plan| self.bar_shown(plan) && super::inside(plan.plate, point))
        else {
            return Ok(false);
        };
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let found = Style::read(host, &display, &font).and_then(|style| {
            let face = Face {
                host,
                font: &font,
                small: style.small,
            };
            let area = plan.inner;
            let items = (&face, &plan.items);
            let hit = status_line::targets(items, ui, area)?
                .into_iter()
                .find(|(_, rect)| super::inside(*rect, point));
            match hit {
                Some((target, _)) => status_line::action(items, ui, area, target),
                None => Ok(None),
            }
        });
        let released = font.release();
        let action = found?;
        released?;
        let Some(action) = action else {
            return Ok(false);
        };
        let mut scratch = buffer(SERVICE_BUFFER)?;
        CommandSender::new(host, &mut scratch).run_action(&action)?;
        Ok(true)
    }

    /// A key before the page sees it: the first press while a bar that shows
    /// after a press is hidden only shows it (the power key is the kernel's).
    /// A bar whose time is over goes first.
    pub(super) fn bar_press(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<Option<Handled>, Error> {
        let Input::Key {
            now_ms,
            key,
            state: KeyState::Press,
        } = *input
        else {
            return Ok(None);
        };
        self.expire(now_ms);
        let hidden = !self.editing
            && self.revealed.is_none()
            && self
                .laid
                .as_ref()
                .and_then(|laid| laid.bar.as_ref())
                .is_some_and(Plan::auto);
        if key == "power" || !hidden {
            return Ok(None);
        }
        self.revealed = Some(now_ms.saturating_add(BAR_MS));
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        if self
            .laid
            .as_ref()
            .and_then(|laid| laid.bar.as_ref())
            .is_some_and(|plan| plan.hints_auto)
        {
            // The row of hints is the kernel's: a refusal does not take the press.
            commands.show_hints().ok();
        }
        redraw(host)?;
        Ok(Some(Handled::Yes))
    }

    /// The bar's time is over at `now_ms`: it goes.
    fn expire(&mut self, now_ms: u64) -> bool {
        if self.revealed.is_some_and(|until| now_ms >= until) {
            self.revealed = None;
            return true;
        }
        false
    }

    /// The minute tick: a bar whose time is over goes, a bar that shows is drawn
    /// anew (it shows the time and the rest that changed).
    ///
    /// # Errors
    /// The host's refusal to redraw.
    pub fn tick(&mut self, host: &Host<'_>, now_ms: u64) -> Result<(), Error> {
        let went = self.expire(now_ms);
        let shows = self
            .laid
            .as_ref()
            .and_then(|laid| laid.bar.as_ref())
            .is_some_and(|plan| self.bar_shown(plan));
        if went || shows {
            redraw(host)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(visible: Visible, hints_auto: bool) -> Plan {
        Plan {
            plate: Rect::new(0, 0, 100, 20),
            inner: Rect::new(4, 0, 92, 20),
            look: Look::Normal,
            items: Lists::default(),
            visible,
            hints_auto,
        }
    }

    #[test]
    fn a_bar_shows_always_never_or_after_a_press() {
        let mut dashboard = Dashboard::new();
        assert!(dashboard.bar_shown(&plan(Visible::On, false)));
        assert!(!dashboard.bar_shown(&plan(Visible::Off, false)));
        let auto = plan(Visible::Auto, false);
        assert!(!dashboard.bar_shown(&auto));
        dashboard.revealed = Some(5_000);
        assert!(dashboard.bar_shown(&auto));
        // Off stays off, whatever a press did.
        assert!(!dashboard.bar_shown(&plan(Visible::Off, false)));
    }

    #[test]
    fn the_press_that_shows_a_bar_is_taken_for_a_bar_or_for_the_hints() {
        assert!(plan(Visible::Auto, false).auto());
        assert!(plan(Visible::Off, true).auto());
        assert!(!plan(Visible::On, false).auto());
        assert!(!plan(Visible::Off, false).auto());
    }

    #[test]
    fn a_bar_goes_at_the_first_time_at_or_after_its_deadline() {
        let mut dashboard = Dashboard::new();
        assert!(!dashboard.expire(10_000));
        dashboard.revealed = Some(5_000);
        assert!(!dashboard.expire(4_999));
        assert_eq!(dashboard.revealed, Some(5_000));
        assert!(dashboard.expire(5_000));
        assert_eq!(dashboard.revealed, None);
    }
}
