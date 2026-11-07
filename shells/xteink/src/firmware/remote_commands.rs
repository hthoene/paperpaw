//! The test remote control's commands over USB (`remote` builds): what
//! the main loop does with a line from the computer.

use super::*;

/// What came over USB.
impl Firmware {
    #[inline(never)]
    pub(super) fn remote(&mut self, line: Result<alloc::vec::Vec<u8>, crate::usb_lines::TooLong>) {
        self.settle_frame();
        if let Some(command) = self.remote.command(line)
            && let Some(command) = self.remote.answer(&mut self.kernel, &mut self.usb, command)
        {
            self.remote_command(command);
        }
    }

    /// Lets go of the button pressed over USB, if one is held, and says
    /// so.
    pub(super) fn release_held(&mut self) {
        let Some((name, _)) = self.state.held.take() else {
            return;
        };
        let started = now();
        let at = self.kernel_now();
        let state = ButtonState::Released;
        let effects = self.kernel.handle(at, InputEvent::Button { name, state });
        info!("{name}: {} ms", now().since(started));
        self.apply(effects);
        crate::remote::ok();
    }

    /// Opens a slot (`slot`) or a screen by its id, with parameters, over
    /// USB.
    pub(super) fn open_remote(
        &mut self,
        id: &str,
        params: alloc::vec::Vec<(alloc::string::String, alloc::string::String)>,
        slot: bool,
    ) {
        let Ok(id) = paperpaw_sdk::Id::parse(id) else {
            return crate::remote::error(id);
        };
        let mut with = paperpaw_sdk::Params::new();
        for (key, value) in params {
            with.set(key, value);
        }
        let at = self.kernel_now();
        let effects = if slot {
            self.kernel.open_slot(at, id, with)
        } else {
            self.kernel.open_screen(at, id, with)
        };
        self.apply(effects);
        crate::remote::ok();
    }

    /// Carries out a command over USB that changes the X4's state.
    pub(super) fn remote_command(&mut self, command: paperpaw_x4::remote::Command) {
        use crate::remote;
        use paperpaw_x4::remote::Command;
        match command {
            Command::Network(ask) => self.network.ask_remote(ask),
            Command::IdleSleep(seconds) => {
                // At most ten minutes: the link is gone meanwhile.
                self.state.sleep_on_usb_until = Some(Instant(now().0 + seconds.min(600) * 1000));
                remote::ok();
            }
            Command::Run(action) => {
                let Ok(id) = paperpaw_sdk::Id::parse(&action) else {
                    return remote::error(&action);
                };
                let at = self.kernel_now();
                let started = now();
                let effects = self.kernel.run_action(at, id);
                info!("{action}: {} ms", now().since(started));
                self.apply(effects);
                remote::ok();
            }
            Command::Set { id, value } => {
                let at = self.kernel_now();
                match self.kernel.set_setting(at, &id, &value) {
                    Ok(effects) => {
                        info!("{id} = {value}");
                        self.apply(effects);
                        remote::ok();
                    }
                    Err(why) => remote::error(why),
                }
            }
            Command::Open(path) => {
                // With the reading app chosen for it.
                let at = self.kernel_now();
                let effects = self.kernel.open_book(at, &path);
                self.apply(effects);
                remote::ok();
            }
            Command::Gray(picture) => {
                let shown = crate::gray_test::show(
                    &mut self.panel,
                    self.kernel.storage(),
                    picture.as_deref(),
                );
                if let Err(e) = shown {
                    warn!("display: {e:?}");
                }
                self.state.gray_shown = true;
                remote::ok();
            }
            Command::GrayTone(tone) => {
                self.panel.set_gray_tone(tone);
                remote::ok();
            }
            Command::Refresh { full } => {
                let refresh = if full { Refresh::Full } else { Refresh::Fast };
                remote::refreshed(self.panel.refresh(refresh, false));
                self.state.panel_idle_at = Some(now().0 + self.panel_idle_ms());
            }
            Command::Slot { id, params } => self.open_remote(&id, params, true),
            Command::Show { id, params } => self.open_remote(&id, params, false),
            Command::Emit { event, params } => {
                let Ok(id) = paperpaw_sdk::Id::parse(&event) else {
                    return remote::error(&event);
                };
                let mut with = paperpaw_sdk::Params::new();
                for (key, value) in params {
                    with.set(key, value);
                }
                let at = self.kernel_now();
                let effects = self.kernel.send_event(at, id, with);
                self.apply(effects);
                remote::ok();
            }
            Command::Type { text, done } => {
                let at = self.kernel_now();
                match self.kernel.type_text(at, &text, done) {
                    Some(effects) => {
                        self.apply(effects);
                        remote::ok();
                    }
                    None => remote::error("no text field"),
                }
            }
            Command::Drag {
                from,
                to,
                steps,
                ms,
            } => {
                input::drag(from, to, steps, ms);
                remote::ok();
            }
            Command::Clicks {
                button,
                count,
                every,
                held,
                jitter,
            } => {
                input::clicks(button, count, every, held, jitter);
                remote::ok();
            }
            Command::Touch { phase, x, y } => {
                use paperpaw_x4::remote::Touch;
                let phase = match phase {
                    Touch::Down => PointerPhase::Down,
                    Touch::Move => PointerPhase::Move,
                    Touch::Up => PointerPhase::Up,
                };
                let event = InputEvent::Pointer(PointerEvent::new(
                    0,
                    phase,
                    Point::new(x, y),
                    PointerTool::Finger,
                    1000,
                ));
                let at = self.kernel_now();
                let effects = self.kernel.handle(at, event);
                self.apply(effects);
                remote::ok();
            }
            Command::SetTime(unix_ms) => {
                crate::clock::set(now(), unix_ms);
                remote::ok();
            }
            Command::Restart => {
                // What plugins keep, saved and on the card first.
                self.pause_trial();
                let at = self.kernel_now();
                let _ = self.kernel.stop(at);
                self.network.leave();
                remote::ok();
                esp_hal::system::software_reset();
            }
            Command::Press { button, ms } => {
                if self.state.held.is_some() {
                    return remote::busy();
                }
                let at = self.kernel_now();
                let state = ButtonState::Pressed;
                let started = now();
                let effects = self.kernel.handle(
                    at,
                    InputEvent::Button {
                        name: button,
                        state,
                    },
                );
                info!("{button} pressed: {} ms", now().since(started));
                self.apply(effects);
                self.state.held = Some((button, Instant(now().0.saturating_add(ms))));
            }
            // Only asking: answered by `remote`.
            _ => {}
        }
    }
}
