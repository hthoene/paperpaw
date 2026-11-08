//! The flat Diagnostics plugin: the test screen and the device check, each an
//! instance owned by this state and keyed by the host's instance number.
use crate::check::Check;
use crate::declarations::{CHECK_TOKEN, EVENTS_TOKEN, OPEN_CHECK_TOKEN, RESET_TOKEN, SCREEN_TOKEN};
use crate::test_screen::TestScreen;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        events::Event,
        registrations,
        reply::Reply,
    },
};
use paperpaw_ui_kit::Screens;

mod image {
    include!(concat!(env!("OUT_DIR"), "/diagnostics_declarations.rs"));
}

const CHECK: &str = "diagnostics.check";
const RESET: &str = "diagnostics.reset";
const OPEN_CHECK: &str = "diagnostics.open-check";

/// A screen's view. The check is held where it is: a box cannot be made
/// with fallible allocation, and the screen list grows fallibly instead.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "a box allocates infallibly; the list of views reserves each slot fallibly"
)]
enum View {
    Test(TestScreen),
    Check(Check),
}

/// Diagnostics' interface-7 entry state.
#[derive(Debug, Default)]
pub struct NativeDiagnostics {
    screens: Screens<View>,
}

impl Plugin for NativeDiagnostics {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS.iter().chain(image::UI).chain(image::EVENTS) {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        event: Event<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        match (token, event) {
            (
                OPEN_CHECK_TOKEN,
                Event::ActionRun {
                    action: OPEN_CHECK, ..
                },
            ) => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open_screen(CHECK)
            }
            (RESET_TOKEN, Event::ActionRun { action: RESET, .. }) => {
                match self.screens.top(SCREEN_TOKEN) {
                    Some(View::Test(screen)) => screen.reset(host).map(|_| ()),
                    _ => Ok(()),
                }
            }
            (SCREEN_TOKEN | CHECK_TOKEN, Event::ScreenOpened { instance, .. }) => {
                // The view is made when the screen first shows: its size is known then.
                let _ = instance;
                Ok(())
            }
            (
                SCREEN_TOKEN | CHECK_TOKEN,
                Event::ScreenShown {
                    width,
                    height,
                    instance: Some(instance),
                    ..
                },
            ) => self.shown(host, token, instance, width, height),
            (SCREEN_TOKEN | CHECK_TOKEN, Event::ScreenClosed { instance, .. }) => {
                if let Some(View::Check(check)) = self.screens.get(instance) {
                    check.closed(host);
                }
                self.screens.close(instance);
                Ok(())
            }
            (EVENTS_TOKEN, event) => {
                for view in self.screens.all() {
                    if let View::Check(check) = view {
                        check.event(host, &event)?;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(
            frame.purpose,
            paperpaw_sdk::native::callbacks::DrawPurpose::Screen
        ) {
            return Ok(());
        }
        match self.screens.top(token).ok_or(Error::State)? {
            View::Test(screen) => screen.draw(host, &mut frame),
            View::Check(check) => check.draw(host, &mut frame),
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        match self.screens.top(token) {
            Some(View::Test(screen)) => match input {
                Input::Action { action, .. } => screen.action(host, action),
                Input::Gesture { position, kind, .. } => screen.gesture(host, position, kind),
                _ => Ok(Handled::No),
            },
            Some(View::Check(check)) => check.input(host, input),
            None => Ok(Handled::No),
        }
    }
}

impl NativeDiagnostics {
    /// A screen shows: its view is made the first time, resized after.
    fn shown(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: paperpaw_sdk::native::ui::Instance,
        width: u32,
        height: u32,
    ) -> Result<(), Error> {
        let (width, height) = (
            u16::try_from(width).map_err(|_| Error::Invalid)?,
            u16::try_from(height).map_err(|_| Error::Invalid)?,
        );
        if self.screens.get(instance).is_none() {
            let view = if token == SCREEN_TOKEN {
                View::Test(TestScreen::new(host, width, height)?)
            } else {
                View::Check(Check::new(host, width, height)?)
            };
            self.screens.open(instance, view)?;
            self.screens.shown(token, instance)?;
            if let Some(View::Check(check)) = self.screens.get(instance) {
                return check.shown(host);
            }
            return Ok(());
        }
        self.screens.shown(token, instance)?;
        match self.screens.get(instance) {
            Some(View::Test(screen)) => screen.resize(width, height),
            Some(View::Check(check)) => {
                check.resize(width, height);
                return check.shown(host);
            }
            None => {}
        }
        Ok(())
    }
}
